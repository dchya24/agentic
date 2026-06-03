# Planner Agent — Completion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the 6 remaining items for the Planner Agent feature: `plan` tool, Config wiring, subagent delegation, integration tests, event-to-CLI renderer, and TUI panel.

**Architecture:** The `core-agentic/src/planner.rs` already has a full `PlannerAgent` implementation with plan creation, step execution, approval flow, and replanning (45 tests passing). CLI integration exists via `/plan` REPL command. The missing pieces are: (1) an agent-callable `plan` tool, (2) config wiring, (3) subagent delegation for steps, (4) E2E integration tests, (5) proper event wiring to CLI widgets, (6) a TUI panel for plan display.

**Tech Stack:** Rust, `core-agentic` library, `agentic-cli` binary (ratatui TUI, reedline REPL)

---

## Task 1: Add `PlanProgress` Event Type

**Files:**
- Modify: `core-agentic/src/events.rs:9-50`
- Test: `core-agentic/src/events.rs` (existing tests section)

**Step 1: Add `PlanProgress` variant to `Event` enum**

Add after the `System` variant in `core-agentic/src/events.rs`:

```rust
#[serde(rename = "plan_progress")]
PlanProgress {
    plan_id: String,
    plan_goal: String,
    step_id: String,
    step_description: String,
    step_status: String,  // "pending" | "in_progress" | "completed" | "failed" | "skipped"
    steps_total: usize,
    steps_completed: usize,
    steps_failed: usize,
    steps_pending: usize,
},
```

**Step 2: Add `PlanProgress` variant to `EventType` enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    // ... existing variants
    PlanProgress,
}
```

Add match arm in `Event::event_type()`:
```rust
Event::PlanProgress { .. } => EventType::PlanProgress,
```

**Step 3: Run tests**

```bash
cd core-agentic && cargo test --lib events
```
Expected: PASS

**Step 4: Commit**

```bash
git add core-agentic/src/events.rs
git commit -m "feat(planner): add PlanProgress event type"
```

---

## Task 2: Wire Planner Config into Core Config

**Files:**
- Modify: `core-agentic/src/config.rs:5-40`
- Modify: `core-agentic/src/planner.rs` (PlannerAgent constructor)
- Test: `core-agentic/src/config.rs` (existing tests)

**Step 1: Add `planner` field to `AgentLoopConfig`**

In `core-agentic/src/config.rs`, add to `AgentLoopConfig`:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentLoopConfig {
    #[serde(default)]
    pub auto_compact_with_llm: bool,
    #[serde(default)]
    pub summarizer_model: Option<String>,
    /// Planner agent configuration.
    #[serde(default)]
    pub planner: PlannerLoopConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerLoopConfig {
    /// Maximum steps per plan.
    #[serde(default = "default_planner_max_steps")]
    pub max_steps: usize,
    /// Maximum re-plan attempts on failure.
    #[serde(default = "default_planner_max_replan")]
    pub max_replan_attempts: usize,
    /// Whether plans require user approval before execution.
    #[serde(default = "default_true")]
    pub require_approval: bool,
    /// Model override for planning LLM calls (cheaper/faster model).
    #[serde(default)]
    pub model: Option<String>,
    /// Provider name override for planning LLM calls.
    #[serde(default)]
    pub provider: Option<String>,
}

fn default_planner_max_steps() -> usize { 20 }
fn default_planner_max_replan() -> usize { 3 }
```

Also add `impl Default for PlannerLoopConfig`:
```rust
impl Default for PlannerLoopConfig {
    fn default() -> Self {
        Self {
            max_steps: 20,
            max_replan_attempts: 3,
            require_approval: true,
            model: None,
            provider: None,
        }
    }
}
```

**Step 2: Update `PlannerAgent::new()` to accept `PlannerConfig` from Config**

In `core-agentic/src/planner.rs`, add a constructor that takes `PlannerLoopConfig`:

```rust
impl PlannerAgent {
    pub fn new(provider: Arc<dyn LLMProvider>) -> Self {
        // existing
    }

    pub fn from_config(
        provider: Arc<dyn LLMProvider>,
        config: &crate::config::PlannerLoopConfig,
    ) -> Self {
        let planner_config = PlannerConfig {
            max_steps: config.max_steps,
            require_approval: config.require_approval,
            max_replan_attempts: config.max_replan_attempts,
        };
        Self {
            provider,
            config: planner_config,
            events: EventEmitter::new(),
            approval_callback: std::sync::Mutex::new(None),
        }
    }
}
```

**Step 3: Update CLI `plan_inline()` to use config**

In `agentic-cli/src/commands.rs`, change `PlannerAgent::new(provider)` to:
```rust
let planner = core_agentic::PlannerAgent::from_config(provider, &self.config.agent.planner);
```

**Step 4: Add tests**

Add test in `config.rs`:
```rust
#[test]
fn planner_config_round_trip() {
    let json = r#"{
        "providers": [...],
        "agent": {
            "planner": {
                "max_steps": 10,
                "max_replan_attempts": 5,
                "require_approval": false,
                "model": "gpt-4o-mini"
            }
        }
    }"#;
    let cfg: Config = serde_json::from_str(json).expect("parse");
    assert_eq!(cfg.agent.planner.max_steps, 10);
    assert_eq!(cfg.agent.planner.max_replan_attempts, 5);
    assert!(!cfg.agent.planner.require_approval);
    assert_eq!(cfg.agent.planner.model.as_deref(), Some("gpt-4o-mini"));
}
```

**Step 5: Run tests**

```bash
cd core-agentic && cargo test --lib config
```
Expected: PASS

**Step 6: Commit**

```bash
git add core-agentic/src/config.rs core-agentic/src/planner.rs agentic-cli/src/commands.rs
git commit -m "feat(planner): wire planner config into Config struct"
```

---

## Task 3: Subagent Delegation for Steps

**Files:**
- Modify: `core-agentic/src/planner.rs` (execute_plan method)
- Test: `core-agentic/src/planner.rs` (existing tests)

**Step 1: Add subagent support to step execution**

In `core-agentic/src/planner.rs`, modify the step execution block inside `execute_plan()`:

When a step has `tool: "spawn_subagent"`, instead of calling `tools.execute_by_name("spawn_subagent", ...)`, create a `SpawnSubagentTool` and execute it directly with the step description as the task. This allows the planner to delegate complex steps to subagents.

Add a helper method:
```rust
fn execute_step_with_subagent(&self, step: &Step) -> (StepStatus, Option<String>) {
    let subagent = crate::tools::SpawnSubagentTool::new(
        self.provider.clone(),
        // tools passed via method param or stored as field
        // For now, we need to pass ToolRegistry
    );
    // execute subagent
}
```

Actually, since `ToolRegistry` isn't stored on `PlannerAgent`, we need to pass it differently. The cleanest approach:

Modify the step execution loop in `execute_plan()`:
- If `step.tool == Some("spawn_subagent")`, use `step.description` as the subagent task
- Otherwise, use `tools.execute_by_name()` as before

```rust
// Inside execute_plan, replace the tool execution block:
let result = if let Some(tool_name) = &step_tool {
    if tool_name == "spawn_subagent" {
        // Delegate to subagent
        let subagent_tool = crate::tools::SpawnSubagentTool::new(
            self.provider.clone(),
            tools.clone(), // ToolRegistry is Clone
            "planner-subagent".to_string(), // model
        );
        let args = step_args.clone().unwrap_or(serde_json::json!({
            "task": step_desc
        }));
        match subagent_tool.execute(args) {
            Ok(output) => (StepStatus::Completed, Some(serde_json::to_string(&output).unwrap())),
            Err(e) => (StepStatus::Failed, Some(format!("Subagent error: {}", e))),
        }
    } else {
        let args = step_args.clone().unwrap_or(serde_json::json!({}));
        match tools.execute_by_name(tool_name, &args) {
            Ok(output) => (StepStatus::Completed, Some(serde_json::to_string(&output).unwrap_or_default())),
            Err(e) => (StepStatus::Failed, Some(format!("Tool error: {}", e))),
        }
    }
} else {
    (StepStatus::Completed, None)
};
```

**Step 2: Add test for subagent delegation**

Add test in `planner.rs`:
```rust
#[test]
fn test_execute_plan_with_subagent_step() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        text_response("Subagent result: done"),
    ]));
    let planner = PlannerAgent::new(provider).with_config(PlannerConfig {
        require_approval: false,
        max_replan_attempts: 0,
        ..Default::default()
    });
    let tools = ToolRegistry::new();

    // Step with spawn_subagent tool
    let plan = Plan::new("Delegate task")
        .with_steps(vec![
            Step::new("Explore API")
                .with_tool("spawn_subagent", serde_json::json!({"task": "Read the API files"})),
        ]);
    let mut plan = plan;
    plan.status = PlanStatus::Draft;

    let result = planner.execute_plan(&mut plan, &tools).unwrap();
    assert_eq!(result.status, PlanStatus::Completed);
    assert_eq!(result.steps_completed, 1);
}
```

**Step 3: Run tests**

```bash
cd core-agentic && cargo test --lib planner
```
Expected: PASS (46+ tests)

**Step 4: Commit**

```bash
git add core-agentic/src/planner.rs
git commit -m "feat(planner): subagent delegation for plan steps"
```

---

## Task 4: Emit `PlanProgress` Events from Planner

**Files:**
- Modify: `core-agentic/src/planner.rs` (PlannerAgent methods)
- Test: `core-agentic/src/planner.rs` (existing tests)

**Step 1: Emit PlanProgress in execute_plan()**

In `core-agentic/src/planner.rs`, inside `execute_plan()`:

After marking a step as `InProgress`, emit:
```rust
self.events.emit(crate::events::Event::PlanProgress {
    plan_id: plan.id.clone(),
    plan_goal: plan.goal.clone(),
    step_id: step_id.clone(),
    step_description: step_desc.clone(),
    step_status: "in_progress".to_string(),
    steps_total: plan.steps.len(),
    steps_completed: completed_count,
    steps_failed: failed_count,
    steps_pending: pending_count,
});
```

After marking a step as `Completed` or `Failed`, emit again with updated count.

Create a helper `emit_plan_progress()` to reduce duplication.

**Step 2: Add test**

```rust
#[test]
fn test_plan_progress_events_emitted() {
    // Create a planner, execute a plan with 2 steps
    // Verify that PlanProgress events are emitted with correct counts
}
```

**Step 3: Run tests**

```bash
cd core-agentic && cargo test --lib planner
```
Expected: PASS

**Step 4: Commit**

```bash
git add core-agentic/src/planner.rs
git commit -m "feat(planner): emit PlanProgress events during step execution"
```

---

## Task 5: Integration Tests for Planner

**Files:**
- Create: `core-agentic/tests/planner_loop.rs`
- Modify: `core-agentic/Cargo.toml` (add dev-dependencies if needed)

**Step 1: Create integration test file**

`core-agentic/tests/planner_loop.rs`:

```rust
//! Integration tests for the Planner Agent.
//!
//! These tests use ScriptedProvider similar to orchestrator_loop.rs
//! to test planner workflows end-to-end.

// ... test structure following orchestrator_loop.rs pattern

#[test]
fn test_planner_create_and_execute_plan() {
    // Script provider responses:
    // 1. Plan creation → returns JSON steps
    // 2. Tool execution → returns success
    // Expected: plan completes successfully
}

#[test]
fn test_planner_replan_on_failure() {
    // Script provider responses:
    // 1. Plan creation → returns JSON steps
    // 2. Tool execution → returns error
    // 3. Replan → returns revised steps
    // 4. Tool execution → returns success
    // Expected: plan completes after replan
}

#[test]
fn test_planner_cancelled_on_approval_reject() {
    // Set approval callback to reject
    // Expected: plan cancelled, no steps executed
}

#[test]
fn test_planner_max_steps_exceeded() {
    // Plan creation returns > max_steps steps
    // Expected: error returned
}

#[test]
fn test_planner_subagent_step() {
    // Step with spawn_subagent tool
    // Expected: subagent executed, step completed
}

#[test]
fn test_planner_dependency_order() {
    // Plan with depends_on
    // Expected: steps execute in correct order
}
```

**Step 2: Run tests**

```bash
cd core-agentic && cargo test --test planner_loop
```
Expected: PASS

**Step 3: Commit**

```bash
git add core-agentic/tests/planner_loop.rs
git commit -m "test(planner): add integration tests for planner workflows"
```

---

## Task 6: Wire Planner Events to CLI Renderer

**Files:**
- Modify: `agentic-cli/src/commands.rs` (plan_inline method + event handler)
- Modify: `agentic-cli/src/widgets/` (new plan progress widget or extend existing)

**Step 1: Subscribe to PlannerAgent events in plan_inline()**

In `agentic-cli/src/commands.rs`, before calling `planner.execute_plan()`, register an event handler:

```rust
use core_agentic::events::Event as PlannerEvent;

// Create a shared progress state
let progress = Arc::new(Mutex::new(PlanProgressState::default()));

planner.on({
    let progress = progress.clone();
    move |event: PlannerEvent| {
        if let PlannerEvent::PlanProgress {
            plan_goal,
            step_description,
            step_status,
            steps_total,
            steps_completed,
            steps_failed,
            steps_pending,
            ..
        } = event
        {
            // Update shared state
            let mut state = progress.lock().unwrap();
            state.goal = plan_goal;
            state.current_step = step_description;
            state.status = step_status;
            state.total = steps_total;
            state.completed = steps_completed;
            state.failed = steps_failed;
            state.pending = steps_pending;

            // Render live progress bar
            render_plan_progress(&state);
        }
    }
});
```

**Step 2: Create PlanProgressState and render helper**

```rust
struct PlanProgressState {
    goal: String,
    current_step: String,
    status: String,
    total: usize,
    completed: usize,
    failed: usize,
    pending: usize,
}
```

And a render function that outputs a progress bar using the existing widgets:

```rust
fn render_plan_progress(state: &PlanProgressState) {
    // Use components::progress_bar or similar
    // Show: [████████░░] 3/5 steps completed
    // Show: current step description
}
```

**Step 3: Run tests**

```bash
cd agentic-cli && cargo test
```
Expected: PASS

**Step 4: Commit**

```bash
git add agentic-cli/src/commands.rs
git commit -m "feat(cli): wire planner PlanProgress events to inline renderer"
```

---

## Task 7: TUI Panel for Plan Display

**Files:**
- Modify: `agentic-cli/src/tui/app.rs` (App struct, handle AppMessage)
- Modify: `agentic-cli/src/tui/ui.rs` (render plan panel)
- Create: `agentic-cli/src/tui/plan_panel.rs` (plan panel widget)

**Step 1: Add PlanProgress variant to AppMessage**

In `agentic-cli/src/tui/app.rs`, add to `AppMessage`:
```rust
pub enum AppMessage {
    // ... existing variants
    PlanProgress {
        goal: String,
        current_step: String,
        status: String,
        total: usize,
        completed: usize,
        failed: usize,
        pending: usize,
    },
}
```

**Step 2: Add plan state to App**

```rust
pub struct App {
    // ... existing fields
    pub plan_state: Option<PlanUiState>,
}

pub struct PlanUiState {
    pub goal: String,
    pub current_step: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending: usize,
    pub is_active: bool,
}
```

**Step 3: Create plan panel widget**

`agentic-cli/src/tui/plan_panel.rs`:
```rust
//! TUI plan panel widget.
//!
//! Renders a dedicated panel for plan display with:
//! - Goal header
//! - Progress bar (completed/total/failed)
//! - Current step description
//! - Step list with status indicators

use ratatui::{prelude::*, widgets::*};

pub fn render_plan_panel(area: Rect, buf: &mut Buffer, state: &super::app::PlanUiState) {
    // Render goal section
    // Render progress bar
    // Render current step
}
```

**Step 4: Wire into ui.rs render loop**

In `agentic-cli/src/tui/ui.rs`, add plan panel rendering when `plan_state.is_active`:

```rust
pub fn draw(app: &mut App, frame: &mut Frame) {
    // ... existing layout
    
    if let Some(ref plan) = app.plan_state {
        if plan.is_active {
            let plan_area = /* allocate area */;
            plan_panel::render_plan_panel(plan_area, frame.buffer_mut(), plan);
        }
    }
}
```

**Step 5: Handle PlanProgress messages in event loop**

Add handler for `AppMessage::PlanProgress` in the TUI event loop:

```rust
AppMessage::PlanProgress { goal, current_step, status, total, completed, failed, pending } => {
    app.plan_state = Some(PlanUiState {
        goal,
        current_step,
        total,
        completed,
        failed,
        pending,
        is_active: true,
    });
}
```

**Step 6: Run tests**

```bash
cd agentic-cli && cargo test
cd agentic-cli && cargo build
```
Expected: Both PASS

**Step 7: Commit**

```bash
git add agentic-cli/src/tui/
git commit -m "feat(tui): add plan panel for displaying plan progress"
```

---

## Summary

| Task | Area | Effort |
|------|------|--------|
| 1. `PlanProgress` event type | events.rs | ~10 min |
| 2. Config wiring | config.rs, planner.rs, commands.rs | ~20 min |
| 3. Subagent delegation | planner.rs | ~30 min |
| 4. Emit PlanProgress events | planner.rs | ~20 min |
| 5. Integration tests | tests/planner_loop.rs | ~30 min |
| 6. CLI event renderer | commands.rs | ~20 min |
| 7. TUI panel | tui/ | ~45 min |
| **Total** | | **~3 hours** |
