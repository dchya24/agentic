# Milestone 4 — Interactive Tools + Skill System Foundation

**Date:** 2026-06-02
**Branch:** `dev`
**Scope:** `core-agentic`, `agentic-cli`

## Goal

Add two interactive tools (`question`, `todowrite`) that close the gap with
the opencode tool reference, and lay the groundwork for a future skill system.

The `lsp` and `codesearch` tools from the reference are **intentionally
excluded** — out of scope per the architecture spec. The agent uses
`read_file` + `grep` + `glob` for code navigation and `web_search` for
external documentation.

---

## Items shipped

| # | Item | Files |
|---|------|-------|
| 4.1 | `question` tool — ask user questions mid-execution | `core-agentic/src/tools/question.rs` |
| 4.2 | `todowrite` tool — session-scoped task list | `core-agentic/src/tools/todowrite.rs` |
| 4.3 | Updated TOOL_REFERENCE.md | `core-agentic/docs/TOOL_REFERENCE.md` |

---

## Architectural notes

### 4.1 `question` tool

The agent uses this to gather preferences, clarify ambiguous instructions,
or present implementation choices to the user.

**Mechanism — callback pattern:**

`Tool::execute` is synchronous and must return a `ToolResult<Value>`. User
interaction (stdin reads, TUI dialogs) cannot happen inside `execute`
directly. Instead, the tool uses a global handler slot:

```rust
pub(crate) static QUESTION_HANDLER: LazyLock<Mutex<Option<Box<dyn QuestionHandler>>>> =
    LazyLock::new(|| Mutex::new(None));
```

The CLI registers a handler at startup:

```rust
set_question_handler(Box::new(MyCliHandler));
```

The handler trait:

```rust
pub trait QuestionHandler: Send + Sync {
    fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer>;
}
```

**Graceful degradation:** If no handler is registered (e.g. `agentic run`
in non-interactive mode), the tool returns `skipped: true` for every
question. The agent receives the skip markers and proceeds without
blocking. This means the tool is always safe to call — it never deadlocks.

**Question schema:**

```jsonc
{
  "question": "Which framework?",
  "header": "Framework",          // optional — short label for TUI
  "options": ["react", "vue"],     // optional — pre-defined choices
  "custom": true,                  // optional — allow free-text
  "multiple": false                // optional — allow multi-select
}
```

**Response schema:**

```jsonc
{
  "answers": [
    { "question": "Which framework?", "answer": ["react"], "skipped": false }
  ],
  "total": 1,
  "skipped": 0,
  "answered": 1
}
```

**Why global state?** The `Tool` trait has no access to the orchestrator
or CLI context — `execute(&self, args: Value)` is the only method. The
same pattern is used by `SpawnSubagentTool` (which receives provider +
tools at construction time). The global handler avoids threading callback
references through the entire tool registration chain.

### 4.2 `todowrite` tool

The agent uses this to plan complex tasks, track progress, and organize
multi-step work within a session.

**Design decisions:**

1. **Full-replace semantics.** Every call replaces the entire list. This
   matches the opencode reference and keeps the model's mental model
   simple — it always sends the complete current state.

2. **Session-scoped.** The list lives in a `LazyLock<Mutex<Vec<TodoItem>>>`
   and is lost on process exit. For cross-session persistence, the agent
   should use `update_memory`.

3. **UI notification.** A `TodoChangeHandler` callback is fired after
   every successful write. The CLI/TUI can use this to render a progress
   panel.

4. **Flexible parsing.** Custom `Deserialize` impl for `TodoItem` accepts
   aliases:
   - Status: `in_progress` / `in-progress` / `active`, `done` / `finished`,
     `canceled` / `skipped`
   - Priority: `normal` → Medium, `important` / `critical` → High

**Why `is_read_only: false`?** The tool doesn't touch the filesystem, but
it modifies session state and its ordering matters. Returning `false`
prevents the orchestrator from batching it with other tools.

**Todo item schema:**

```jsonc
{
  "content": "Implement auth module",   // required
  "status": "in_progress",             // optional — default: "pending"
  "priority": "high"                    // optional — default: "medium"
}
```

**Response schema:**

```jsonc
{
  "total": 3,
  "completed": 1,
  "in_progress": 1,
  "pending": 1,
  "progress_pct": 33
}
```

---

## CLI integration (pending)

Both tools are registered in `builtin_tools()` and will appear in the
tool definitions sent to the LLM. However, **the CLI does not yet register
handlers** for either tool. The integration points are:

### `question` — CLI handler

In `agentic-cli/src/commands.rs` or `agentic-cli/src/interactive.rs`:

```rust
struct CliQuestionHandler;

impl QuestionHandler for CliQuestionHandler {
    fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer> {
        // For each question:
        //   1. Print the question (with header if present)
        //   2. If options exist, show numbered list
        //   3. Read user input from stdin
        //   4. Return parsed answer
    }
}

// During startup:
set_question_handler(Box::new(CliQuestionHandler));
```

**TUI integration:** The TUI could render questions as modal dialogs
or inline panels, using the same `QuestionHandler` trait.

### `todowrite` — CLI handler

In `agentic-cli/src/commands.rs`:

```rust
struct CliTodoRenderer;

impl TodoChangeHandler for CliTodoRenderer {
    fn on_change(&self, todos: &[TodoItem]) {
        // Render the todo list in the TUI status bar or sidebar
        // e.g. show "Tasks: 3/5 (60%)" or a full panel
    }
}

// During startup:
set_todo_change_handler(Box::new(CliTodoRenderer));
```

### When to register / skip

| Mode | `question` handler | `todowrite` handler |
|------|:-:|:-:|
| `agentic run "..."` | ❌ skip (non-interactive) | ✅ render in output |
| `agentic interactive` | ✅ stdin-based prompts | ✅ render in output |
| `agentic tui` | ✅ TUI dialog | ✅ TUI sidebar/panel |

---

## Skill system (future work)

A `skill` tool (load domain-specific instructions into the conversation)
requires infrastructure that doesn't exist yet:

1. **Skill format.** Define a `SKILL.md` schema that describes the skill's
   purpose, trigger conditions, instructions, and resource files.

2. **Skill discovery.** Walk a well-known directory
   (`~/.config/agentic/skills/`, `.agentic/skills/`) and build an
   index of available skills.

3. **Skill injection.** When the agent calls `skill(name)`, the tool:
   - Reads the skill's `SKILL.md` and any referenced files.
   - Returns the content as tool output (the model absorbs it into its
     working context).
   - Optionally appends to the system prompt for the rest of the session.

4. **Skill authoring UX.** A `/skills` REPL command to list available
   skills. Maybe a `agentic skill create <name>` wizard.

This is deferred to a future milestone. The `question` tool's callback
pattern (`set_question_handler`) is a blueprint for how CLI ↔ tool
integration will work for skills too (a `SkillResolver` trait that the
CLI implements).

---

## Test coverage added

| Module | Tests |
|--------|-------|
| `tools::question::tests` | +8 (reject empty array, reject empty text, skip-all fallback, handler invocation, field parsing, defaults) |
| `tools::todowrite::tests` | +11 (reject empty content, reject too many, store+summary, replace list, clear list, change handler, status parse, priority parse, defaults, progress pct) |

**Total: 19 new tests.** Cumulative across all milestones: **+83 tests**.

## Verification

```
core-agentic:  cargo build          # clean
core-agentic:  cargo test --lib     # 438 passing
agentic-cli:   cargo build          # clean
```

## Mapping back to TOOL_REFERENCE.md (opencode)

| Tool | Before M4 | After M4 |
|------|:-:|:-:|
| `bash` / `run_command` | ✅ | ✅ |
| `read` / `read_file` | ✅ | ✅ |
| `edit` / `edit_file` | ✅ | ✅ |
| `write` / `write_file` | ✅ | ✅ |
| `glob` | ✅ | ✅ |
| `grep` | ✅ | ✅ |
| `task` / `spawn_subagent` | ✅ | ✅ |
| `question` | ❌ | ✅ (handler wiring pending) |
| `webfetch` / `fetch` | ✅ | ✅ |
| `websearch` / `web_search` | ✅ | ✅ |
| `todowrite` | ❌ | ✅ (handler wiring pending) |
| `skill` | ❌ | ❌ (skill system not yet built) |
| `apply_patch` | ✅ | ✅ |
| `codesearch` | — | Intentionally excluded |
| `lsp` | — | Intentionally excluded |
| `plan_exit` | — | Covered by `--mode plan` |

**Coverage: 14/16 reference tools implemented** (2 excluded, 1 skill deferred).

## Remaining work

| Item | Scope | Effort |
|------|-------|--------|
| Wire `QuestionHandler` in CLI interactive mode | `agentic-cli` | 1–2 days |
| Wire `TodoChangeHandler` in CLI | `agentic-cli` | 1 day |
| Skill system design + `skill` tool | `core-agentic` + `agentic-cli` | 2–3 weeks |
