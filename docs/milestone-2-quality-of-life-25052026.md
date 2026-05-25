# Milestone 2 — Quality of Life

**Date:** 2026-05-25
**Branch:** `dev`
**Scope:** `core-agentic`, `agentic-cli`

## Goal

Build on the foundational alignment from Milestone 1 with the four
"behavioral" features from the architecture doc:

- 2.1 Concurrent tool execution
- 2.2 Permission modes (`Default` / `Plan` / `Yolo`)
- 2.4 Compression Layer 2 (clear old tool results)
- 2.5 Cooperative cancel

These are the items a user can immediately feel: speed, control, safety.

## Items shipped

| # | Item | Files |
|---|------|-------|
| 2.1 | Concurrent read-only tool execution | `core-agentic/src/{tool,tool_registry,orchestrator}.rs`, `core-agentic/src/tools/{read_file,list_files,glob,grep,search_files}.rs` |
| 2.2 | Permission modes enum + CLI flag | `core-agentic/src/safety.rs`, `agentic-cli/src/{cli,main,commands}.rs` |
| 2.4 | Compression Layer 2 (`[Cleared]` for old tool results) | `core-agentic/src/orchestrator.rs` |
| 2.5 | Cooperative cancel via shared `Arc<AtomicBool>` | `core-agentic/src/{lib,orchestrator}.rs`, `agentic-cli/src/{main,commands}.rs` |

## Architectural notes

### 2.1 Concurrent tool execution

The architecture doc says read-only tools should run in parallel and
state-changing tools should run alone in order. Implementation:

1. Added `Tool::is_read_only() -> bool` (default `false`) to the trait.
2. Marked the five known-safe tools as read-only:
   `read_file`, `list_files`, `glob`, `grep`, `search_files`.
3. `ToolRegistry` switched `Mutex` → `RwLock`. Reads (the hot path during
   parallel execution) no longer serialize on the registry lock.
4. New async path `Orchestrator::handle_tool_calls_parallel` used by
   `run_stream`:
   - Pre-pass evaluates safety + confirmation **sequentially** in original
     order. Gating semantics are unchanged by parallelism.
   - Resolved slots are then walked: contiguous read-only `Pending` slots
     form one batch executed via `tokio::task::spawn_blocking`; mutating
     slots are batches of one.
   - Results are collected position-aligned and pushed to memory in the
     **original order** so the model sees a coherent transcript.
   - On `JoinError` (panic), a synthetic `Tool error: task panicked` is
     recorded so the loop can keep going.

The synchronous `run()` path keeps the original serial implementation —
no Tokio runtime context is required.

### 2.2 Permission modes

Maps the doc's three modes to a single enum and a stateful field on
`Safety`:

```rust
pub enum PermissionMode { Default, Plan, Yolo }
```

- `Default` — current behavior. Auto-approve low risk; ask for medium+.
- `Plan` — read-only. State-changing tools (`write_file`, `edit_file`,
  `delete_file`, `run_command`, `run_script`) are denied outright with
  `"Blocked by plan mode"`. Reads pass through.
- `Yolo` — auto-approve everything except the hard blocklist (the
  critical safety net stays active even here).

`PermissionMode::parse()` accepts a few synonyms (`readonly`, `dry-run`,
`trust`, `auto`, etc.) so the CLI flag is forgiving.

CLI: new global `--mode <MODE>` flag wired through `main.rs` →
`Commands::with_permission_mode()` → `Orchestrator::set_permission_mode()`.
Visible via `agentic --mode plan run "..."` etc.

The handler in `Orchestrator::handle_tool_calls(_parallel)` now consults
`Safety::evaluate()` (mode-aware) instead of just `needs_confirmation`,
so `Plan` mode actually blocks rather than asking for confirmation
the user can't satisfy.

### 2.4 Compression Layer 2

Implemented as a transparent rewrite at the request boundary, not a
mutation of memory. `build_request_messages` walks newest-first to find
the indices of the `keep_recent_tool_results` most-recent tool messages
(default 6). Older tool messages have their content replaced with:

```
[Cleared: older tool result removed to save context. Re-run the tool if
 you need this output.]
```

User and assistant messages always pass through verbatim. Tool messages
within the keep window also pass through verbatim. Setting
`keep_recent_tool_results = 0` disables the substitution entirely.

This is cheap (string replacement, no LLM call) and matches the doc's
description of Layer 2.

### 2.5 Cooperative cancel

Old behavior: Ctrl+C → `process::exit(130)`. The agent could be killed
mid-tool, mid-write, or mid-stream with no chance to clean up.

New behavior:
- `Orchestrator` gains an `Arc<AtomicBool>` cancel flag.
  `cancel_handle()` clones it; `set_cancel_handle()` injects an external
  one; `reset_cancel()` clears it between runs.
- The `run()` and `run_stream()` loops check the flag at every iteration
  boundary and return `AgenticError::Cancelled` if set.
- New error variant `AgenticError::Cancelled`.
- `agentic-cli/main.rs` owns a process-global `CANCEL_FLAG`
  (`OnceLock<Arc<AtomicBool>>`). The Ctrl+C signal handler:
  - First Ctrl+C: flips the flag, prints
    `"Cancel requested (press Ctrl+C again to force-exit)"`.
  - Second Ctrl+C: `process::exit(130)` (we may be stuck inside a tool
    that doesn't observe the flag).
- `Commands::ensure_orchestrator()` calls
  `orchestrator.set_cancel_handle(crate::cancel_flag())` so the same
  atomic is observed by both the signal handler and the loop.
- `Commands::run()` and `run_with_callback()` call `reset_cancel()` at
  the start of each invocation so a previous cancel doesn't poison a
  fresh REPL turn.

Cancel granularity is between turns and between tool batches — not
mid-LLM-stream and not mid-tool-call. That matches the architecture doc
("model decides when it's done; loop has exit conditions for cancel").
For finer granularity the LLM provider streams would need their own
cancel-aware reqwest abort, which is deferred.

## Test coverage added

| Module | Tests |
|--------|-------|
| `orchestrator::orchestrator_unit_tests` | +6 (clears older tool results, keeps when under limit, keep=0 disables, non-tool unaffected, cancel default, cancel flag propagation) |
| `safety::tests` | +10 (all permission mode behaviors, parser, getters/setters, mode-aware needs_confirmation) |
| `tool_registry::tests` | +4 (read_only flag propagation, unknown tool default, builtin tools coverage, concurrent read-only no-deadlock stress test) |

Total: **20 new tests**. Combined milestone 1+2: **+41 tests**.

## Verification

```
core-agentic:  cargo build         # clean
core-agentic:  cargo test --lib    # 247 passing
                                   # (2 pre-existing failover tests still
                                   #  failing — unrelated, need Tokio rt)
agentic-cli:   cargo build         # clean
agentic-cli:   --help              # --mode flag visible with descriptions
```

## Mapping back to AGENT_ARCHITECTURE.md

| Doc section | Doc requirement | Status before M2 | Status after M2 |
|-------------|-----------------|------------------|-----------------|
| Layer 3 — Compression Layer 2 | Replace old tool results with `[Cleared]` | ❌ | ✅ |
| Layer 3 — Permissions | `default` / `plan` / `yolo` modes | ⚠️ partial (toggles) | ✅ explicit enum + CLI |
| Layer 4 — Concurrent Tool Execution | Read-only tools in parallel; state-changing alone; results in original order | ❌ | ✅ (in `run_stream`) |
| Layer 1 — Agentic Loop exit | "User cancelled" exit condition | ⚠️ hard exit | ✅ cooperative + escalation |

## Out of scope (deferred to M3)

- **3.1** Subagents — fresh-context recursive orchestrator
- **3.2** Web access (`fetch`, `web_search`)
- **3.3** Cross-session memory (`~/.config/agentic/memory.md` + `update_memory` tool)
- **3.4** LLM-based summarization for `Memory::compact()`

The synchronous `run()` path also still uses serial tool execution. This
is intentional (it doesn't run inside a Tokio context). If `run()` is
ever migrated behind an async runtime we can collapse the two code paths.
