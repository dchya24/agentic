//! Event types for agentic runtime

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "thought")]
    Thought { content: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        tool_name: String,
        arguments: serde_json::Value,
    },

    /// Emitted right before a tool actually executes (after safety passes).
    #[serde(rename = "tool_start")]
    #[serde(rename_all = "camelCase")]
    ToolStart {
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },

    /// Live output chunk from a streaming tool (run_command/run_script).
    #[serde(rename = "tool_delta")]
    #[serde(rename_all = "camelCase")]
    ToolDelta {
        tool_call_id: String,
        tool_name: String,
        delta: String,
    },

    #[serde(rename = "tool_output")]
    #[serde(rename_all = "camelCase")]
    ToolOutput {
        tool_name: String,
        output: serde_json::Value,
        error: Option<String>,
        tool_call_id: String,
        duration_ms: u64,
        success: bool,
        truncated: bool,
    },

    #[serde(rename = "confirmation_request")]
    ConfirmationRequest {
        action: String,
        description: String,
        risk_level: String,
    },

    #[serde(rename = "error")]
    Error { message: String },

    #[serde(rename = "completed")]
    Completed { result: String },

    #[serde(rename = "system")]
    System { message: String },

    /// Reports progress of a planner-agent plan execution.
    #[serde(rename = "plan_progress")]
    PlanProgress {
        plan_id: String,
        plan_goal: String,
        step_id: String,
        step_description: String,
        step_status: String,
        steps_total: usize,
        steps_completed: usize,
        steps_failed: usize,
        steps_pending: usize,
    },

    /// Emitted when the planner revises a plan after a step failure.
    #[serde(rename = "plan_replanned")]
    PlanReplanned {
        plan_id: String,
        plan_goal: String,
        reason: String,
        steps_carried_over: usize,
        steps_total: usize,
    },

    // ------------------------------------------------------------------
    // Session lifecycle (P0-3). UI-agnostic signals for frontends that
    // observe a run rather than drive it.
    // ------------------------------------------------------------------
    /// Emitted once at the start of every `run` / `run_stream`.
    #[serde(rename = "session_started")]
    SessionStarted,

    /// Emitted before each provider request.
    #[serde(rename = "model_request")]
    #[serde(rename_all = "camelCase")]
    ModelRequest { model: String, message_count: usize },

    /// Live text chunk from a streaming provider response.
    #[serde(rename = "model_chunk")]
    #[serde(rename_all = "camelCase")]
    ModelChunk { delta: String },

    /// Emitted when a skill becomes active for the session.
    #[serde(rename = "skill_activated")]
    #[serde(rename_all = "camelCase")]
    SkillActivated { name: String },

    /// Emitted when the planner produces a plan.
    #[serde(rename = "plan_created")]
    #[serde(rename_all = "camelCase")]
    PlanCreated { steps_total: usize },

    /// Emitted when a plan step begins executing.
    #[serde(rename = "step_started")]
    #[serde(rename_all = "camelCase")]
    StepStarted {
        index: usize,
        total: usize,
        description: String,
    },

    /// Emitted when a plan step finishes.
    #[serde(rename = "step_completed")]
    #[serde(rename_all = "camelCase")]
    StepCompleted {
        index: usize,
        total: usize,
        status: String,
    },

    /// Emitted when auto-compaction kicks in.
    #[serde(rename = "compaction_started")]
    CompactionStarted,

    /// Emitted right before the run blocks on a user confirmation.
    #[serde(rename = "waiting_for_user")]
    WaitingForUser,

    /// Emitted once when a run ends successfully. Carries the final
    /// assistant text so event-stream-only frontends (headless runtime,
    /// kanban) never need to reassemble it from `model_chunk` deltas.
    #[serde(rename = "session_completed")]
    #[serde(rename_all = "camelCase")]
    SessionCompleted { result: String },

    /// Emitted whenever the orchestrator's coarse lifecycle state
    /// changes (`idle` → `planning` → `completed`). The payload is the
    /// snake_case state name; P1-2 formalizes the machine behind it.
    #[serde(rename = "state_changed")]
    StateChanged { state: String },

    /// Emitted when a run ends in an error.
    #[serde(rename = "session_failed")]
    #[serde(rename_all = "camelCase")]
    SessionFailed { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Thought,
    ToolCall,
    ToolStart,
    ToolDelta,
    ToolOutput,
    ConfirmationRequest,
    Error,
    Completed,
    System,
    PlanProgress,
    PlanReplanned,
    SessionStarted,
    ModelRequest,
    ModelChunk,
    SkillActivated,
    PlanCreated,
    StepStarted,
    StepCompleted,
    CompactionStarted,
    WaitingForUser,
    SessionCompleted,
    StateChanged,
    SessionFailed,
}

impl Event {
    pub fn event_type(&self) -> EventType {
        match self {
            Event::Thought { .. } => EventType::Thought,
            Event::ToolCall { .. } => EventType::ToolCall,
            Event::ToolStart { .. } => EventType::ToolStart,
            Event::ToolDelta { .. } => EventType::ToolDelta,
            Event::ToolOutput { .. } => EventType::ToolOutput,
            Event::ConfirmationRequest { .. } => EventType::ConfirmationRequest,
            Event::Error { .. } => EventType::Error,
            Event::Completed { .. } => EventType::Completed,
            Event::System { .. } => EventType::System,
            Event::PlanProgress { .. } => EventType::PlanProgress,
            Event::PlanReplanned { .. } => EventType::PlanReplanned,
            Event::SessionStarted => EventType::SessionStarted,
            Event::ModelRequest { .. } => EventType::ModelRequest,
            Event::ModelChunk { .. } => EventType::ModelChunk,
            Event::SkillActivated { .. } => EventType::SkillActivated,
            Event::PlanCreated { .. } => EventType::PlanCreated,
            Event::StepStarted { .. } => EventType::StepStarted,
            Event::StepCompleted { .. } => EventType::StepCompleted,
            Event::CompactionStarted => EventType::CompactionStarted,
            Event::WaitingForUser => EventType::WaitingForUser,
            Event::SessionCompleted { .. } => EventType::SessionCompleted,
            Event::StateChanged { .. } => EventType::StateChanged,
            Event::SessionFailed { .. } => EventType::SessionFailed,
        }
    }
}

/// Event handler: a boxed closure invoked for every emitted `Event`.
type EventHandler = Box<dyn Fn(Event) + Send + Sync>;

pub struct EventEmitter {
    handlers: Mutex<Vec<EventHandler>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            handlers: Mutex::new(Vec::new()),
        }
    }

    /// Register a handler. Multiple handlers may be registered; they are
    /// called in registration order.
    ///
    /// Takes `&self` because handlers are stored behind a `Mutex`, so the
    /// emitter can sit behind a shared reference (e.g. inside an `Arc` or
    /// as a non-mut field of a longer-lived struct).
    pub fn on<F>(&self, handler: F)
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        self.handlers.lock().unwrap().push(Box::new(handler));
    }

    pub fn emit(&self, event: Event) {
        // Clone the event for each handler so we don't move it twice.
        let handlers = self.handlers.lock().unwrap();
        for handler in handlers.iter() {
            handler(event.clone());
        }
    }

    /// Drop all registered handlers. Useful between runs in long-lived
    /// orchestrators that get re-subscribed each invocation.
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.handlers.lock().unwrap().clear();
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_lifecycle_variants_map_to_type() {
        let start = Event::ToolStart {
            tool_call_id: "c1".into(),
            tool_name: "run_command".into(),
            arguments: serde_json::json!({ "command": "echo hi" }),
        };
        assert_eq!(start.event_type(), EventType::ToolStart);

        let delta = Event::ToolDelta {
            tool_call_id: "c1".into(),
            tool_name: "run_command".into(),
            delta: "hi\n".to_string(),
        };
        assert_eq!(delta.event_type(), EventType::ToolDelta);

        let out = Event::ToolOutput {
            tool_name: "run_command".into(),
            output: serde_json::json!({ "stdout": "hi" }),
            error: None,
            tool_call_id: "c1".into(),
            duration_ms: 42,
            success: true,
            truncated: false,
        };
        assert_eq!(out.event_type(), EventType::ToolOutput);
    }

    #[test]
    fn tool_delta_serializes_with_type_tag() {
        let delta = Event::ToolDelta {
            tool_call_id: "c1".into(),
            tool_name: "run_command".into(),
            delta: "hi\n".to_string(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["type"], "tool_delta");
        assert_eq!(json["toolName"], "run_command");
    }

    #[test]
    fn session_lifecycle_variants_serialize() {
        // Every P0-3 variant must round-trip with its wire tag so
        // non-Rust frontends (TUI/kanban) can consume the stream.
        let cases: Vec<(Event, &str)> = vec![
            (Event::SessionStarted, "session_started"),
            (
                Event::ModelRequest {
                    model: "gpt-4o".into(),
                    message_count: 3,
                },
                "model_request",
            ),
            (Event::ModelChunk { delta: "hi".into() }, "model_chunk"),
            (
                Event::SkillActivated {
                    name: "postgres".into(),
                },
                "skill_activated",
            ),
            (Event::PlanCreated { steps_total: 4 }, "plan_created"),
            (
                Event::StepStarted {
                    index: 1,
                    total: 4,
                    description: "write tests".into(),
                },
                "step_started",
            ),
            (
                Event::StepCompleted {
                    index: 1,
                    total: 4,
                    status: "completed".into(),
                },
                "step_completed",
            ),
            (Event::CompactionStarted, "compaction_started"),
            (Event::WaitingForUser, "waiting_for_user"),
            (
                Event::SessionCompleted {
                    result: "done".into(),
                },
                "session_completed",
            ),
            (
                Event::StateChanged {
                    state: "planning".into(),
                },
                "state_changed",
            ),
            (
                Event::SessionFailed {
                    message: "provider down".into(),
                },
                "session_failed",
            ),
        ];
        for (event, tag) in cases {
            let json = serde_json::to_value(&event).unwrap();
            assert_eq!(json["type"], tag, "wrong tag for {event:?}");
            let parsed: Event = serde_json::from_value(json).unwrap();
            assert_eq!(parsed.event_type(), event.event_type());
        }
    }

    #[test]
    fn model_request_uses_camel_case_fields() {
        let event = Event::ModelRequest {
            model: "claude-3".into(),
            message_count: 7,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["model"], "claude-3");
        assert_eq!(json["messageCount"], 7);
    }
}
