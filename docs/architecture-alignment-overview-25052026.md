# Architecture Alignment — Overview

**Period:** 2026-05-25
**Branch:** `dev`
**Reference:** [`AGENT_ARCHITECTURE.md`](../AGENT_ARCHITECTURE.md)

This document is the high-level entry point for the three-milestone effort
that brought `agentic-cli` and `core-agentic` into alignment with the
conceptual architecture described in `AGENT_ARCHITECTURE.md`. For
implementation detail per milestone, see:

- [Milestone 1 — Foundational Alignment](milestone-1-foundational-alignment-25052026.md)
- [Milestone 2 — Quality of Life](milestone-2-quality-of-life-25052026.md)
- [Milestone 3 — Architectural Additions](milestone-3-architectural-additions-25052026.md)

## TL;DR

Coverage of the architecture doc moved from **~63% → ~95%** across three
commits on `dev`:

```
cd7467b  feat(core): subagents, fetch, persistent memory, llm summarizer (milestone 3)
c8b0d86  feat(core,cli): permission modes, parallel tools, layer-2
         compression, cooperative cancel (milestone 2)
c8435d6  feat(core): align agent loop with architecture doc (milestone 1)
57ff143  docs: add agent architecture reference
```

64 new unit tests, all passing. 270 core-agentic tests pass total. The
two pre-existing `providers::failover::tests` failures (Tokio runtime
context required) are unrelated and were verified to predate this work.

## Why this work

The architecture doc lays out a layered design for an AI coding agent:
loop, tools, system prompt, context compression, permissions, subagents,
streaming, concurrency, persistence. The codebase had strong fundamentals
(orchestrator loop, safety scoring, MCP, multi-provider) but several
gaps where components existed without being wired in (e.g. memory
compaction was implemented but never invoked from the loop) or were
absent entirely (subagents, web access, persistent memory, cooperative
cancel).

The work was deliberately split into three milestones from
**stabilization** → **quality-of-life** → **architectural additions**,
so each commit lands a coherent slice and earlier work compounds.

## What changed, by architecture layer

### Layer 1 — Foundation

| Item | Status | Where |
|------|--------|-------|
| Max-iterations exit | ✅ M1 | `Orchestrator::set_max_iterations`, polled at top of loop |
| Cooperative cancel | ✅ M2 | `Arc<AtomicBool>` flag, two-stage Ctrl+C handler |
| Tool: read/write/edit/list/search/grep/glob/run | ✅ pre-existing | `tools/` |
| Edit tool: string replacement with uniqueness | ✅ pre-existing | `tools/edit_file.rs` |
| Edit tool: staleness detection | ✅ M1 | `FileTracker` shared between read_file and edit_file |
| Edit tool: quote normalization | ✅ M1 | curly→straight fallback when exact match fails |

### Layer 2 — Intelligence

| Item | Status | Where |
|------|--------|-------|
| Default system prompt with the three rules | ✅ M1 | `prompts::DEFAULT_SYSTEM_PROMPT` |
| Search funnel pattern in prompt | ✅ M1 | encoded in default prompt |
| Project instructions auto-load (`AGENT.md`) | ✅ M1 | walk-up from cwd |
| Persistent memory layered into system prompt | ✅ M3 | `memory_file::assemble_memory_section` |
| Full conversation history sent every turn | ✅ pre-existing | `Memory::get_context` |

### Layer 3 — Robustness

| Item | Status | Where |
|------|--------|-------|
| Compression Layer 1 — truncate large tool results | ✅ M1 | `truncate_tool_result()` UTF-8 safe |
| Compression Layer 2 — clear old tool results | ✅ M2 | `build_request_messages()` placeholder substitution |
| Compression Layer 3 — autocompact (heuristic) | ✅ M1 | `Memory::compact()` wired into loop |
| Compression Layer 3 — autocompact (LLM-based) | ✅ M3 | `set_auto_compact_with_llm`, falls back on error |
| Permission modes: `default` / `plan` / `yolo` | ✅ M2 | `PermissionMode` enum + `--mode` CLI flag |
| Pattern-based blocklist | ✅ pre-existing | `safety::default_risk_patterns()` |
| Path sandboxing | ✅ pre-existing | `Safety::is_path_allowed` |
| Per-tool rate limiting | ✅ pre-existing | `Safety::check_rate_limit` |
| Audit log (ring buffer) | ✅ pre-existing | `Safety::audit_log` |

### Layer 4 — Advanced

| Item | Status | Where |
|------|--------|-------|
| Subagents (fresh context, shared tools, linked cancel) | ✅ M3 | `SpawnSubagentTool` |
| Streaming (text deltas + buffered tool calls) | ✅ pre-existing | `Orchestrator::run_stream` |
| Concurrent tool execution (read-only batches) | ✅ M2 | `handle_tool_calls_parallel` |
| Web access — `fetch` (HTML→text + cache) | ✅ M3 | `tools::fetch` |
| Web access — `web_search` | ✅ follow-up | `tools::web_search` (Tavily / Brave / DuckDuckGo) |
| Persistence — conversation history | ✅ pre-existing | `Memory::persist`/`load`/`list_sessions` |
| Persistence — project instructions | ✅ M1 | AGENT.md walk-up |
| Persistence — memory across sessions | ✅ M3 | user + project memory.md, `update_memory` tool |
| MCP integration | ✅ pre-existing | `mcp/` |

## Tests added per milestone

| Module | M1 | M2 | M3 | Total |
|--------|----|----|----|-------|
| `orchestrator::orchestrator_unit_tests` | 4 | +6 | — | 10 |
| `prompts::tests` | 7 | — | — | 7 |
| `file_tracker::tests` | 5 | — | — | 5 |
| `tools::edit_file::edit_file_tests` | 5 | — | — | 5 |
| `safety::tests` | — | +10 | — | +10 |
| `tool_registry::tests` | — | +4 | — | +4 |
| `memory::tests` | — | — | +6 | +6 |
| `memory_file::tests` | — | — | +6 | +6 |
| `tools::update_memory::tests` | — | — | +3 | +3 |
| `tools::spawn_subagent::tests` | — | — | +3 | +3 |
| `tools::fetch::tests` | — | — | +5 | +5 |
| **Total per milestone** | **21** | **20** | **23** | **64** |

## New public API surface

### Modules

```rust
core_agentic::prompts          // DEFAULT_SYSTEM_PROMPT, assemble_system_prompt, find_project_instructions
core_agentic::file_tracker     // FileTracker, Freshness
core_agentic::memory_file      // user/project memory.md helpers, assemble_memory_section
```

### Types

```rust
core_agentic::PermissionMode   // Default | Plan | Yolo
core_agentic::AgenticError::Cancelled
```

### Tools

```rust
core_agentic::FetchTool
core_agentic::UpdateMemoryTool
core_agentic::SpawnSubagentTool
core_agentic::WebSearchTool
```

### Orchestrator setters

```rust
orch.set_max_iterations(u32);
orch.set_tool_result_max_chars(usize);
orch.set_auto_compact(bool);
orch.set_keep_recent_tool_results(usize);
orch.set_auto_compact_with_llm(bool);
orch.set_summarizer_model(impl Into<String>);
orch.set_permission_mode(PermissionMode);
orch.set_cancel_handle(Arc<AtomicBool>);
orch.cancel_handle() -> Arc<AtomicBool>;
orch.reset_cancel();
```

### CLI flag

```
--mode <default|plan|yolo>
```

## Integration points in `agentic-cli`

`Commands::ensure_orchestrator()` is the single place where per-session
configuration is composed:

```
1. Load Config (provider + model).
2. Build ToolRegistry with builtin_tools_with_tracker(Arc::new(FileTracker::new())).
3. Register SpawnSubagentTool with provider + tools + cancel + parent mode.
4. Orchestrator::new(provider, tools).
5. Wire process-global cancel flag (Ctrl+C handler shares it).
6. Discover AGENT.md walking up from cwd.
7. Discover persistent memory (user + project) via memory_file.
8. assemble_system_prompt(default, project_instructions, config_override)
9. Append memory section under "# Persistent Memory".
10. orchestrator.set_system_prompt(final_prompt).
11. orchestrator.set_permission_mode(self.permission_mode).
12. orchestrator.set_confirmation_handler(...).
```

`Commands::run` and `Commands::run_with_callback` call
`orchestrator.reset_cancel()` at the start so previous cancels don't
poison fresh REPL turns.

## Compression in practice

The three compression layers from the architecture doc now run together
on every turn:

```
            ┌── Memory ──┐
            │  messages  │
            │  summary   │
            └──── │ ─────┘
                  ▼
   build_messages() ──── Layer 2 (cheap)
   replace older tool results with [Cleared]
                  │
                  ▼
   execute_tool() ───── Layer 1 (cheap)
   truncate any single tool result over 25k chars
                  │
                  ▼
   maybe_autocompact() ── Layer 3 (expensive)
   when needs_compaction() at ~85% budget:
     - if LLM mode: ask provider to summarize, fall back to
       heuristic on error
     - else: heuristic string-truncation summary
```

## Permission modes in practice

```
default mode:  ask for medium+ risk; allow reads.
plan mode:     deny write_file/edit_file/delete_file/run_command/run_script.
               reads still allowed.
yolo mode:     auto-approve everything except the hard blocklist
               (rm -rf /, mkfs, dd if=, fork bombs, …).
```

`Plan` is enforced by `Safety::evaluate()` at the top of
`handle_tool_calls`, so plan-mode denials show up as
`Blocked: Blocked by plan mode: <tool>` in the conversation history,
and the model gets a chance to recover (e.g. switch to `read_file`).

## Subagent topology

```
   Main Orchestrator
   ├─ Memory (history A)
   ├─ ToolRegistry (shared via Arc)
   ├─ Provider (shared via Arc)
   └─ Cancel flag (shared via Arc)

   On spawn_subagent:
       Subagent Orchestrator
       ├─ Memory (history B, FRESH)
       ├─ ToolRegistry (same Arc clone)
       ├─ Provider (same Arc clone)
       └─ Cancel flag (same Arc clone — linked abort)

   Parent only sees: subagent's final text answer.
```

## Deferred work

| # | Item | Reason |
|---|------|--------|
| — | Domain allowlist for `fetch` / `web_search` | Existing safety pipeline already gates URLs; allowlist is an enhancement |
| — | Concurrent tools in sync `run()` | Intentional — no Tokio context. Concurrent path is in `run_stream()` only |
| — | LSP integration, prompt caching, file watching | Out of scope per architecture doc ("Production Extensions Beyond This Tutorial") |

## Coverage summary

| Layer | Pre-M1 | After M1 | After M2 | After M3 |
|-------|:------:|:--------:|:--------:|:--------:|
| 1 — Foundation | 80% | 95% | 95% | 98% |
| 2 — Intelligence | 70% | 95% | 95% | 100% |
| 3 — Robustness | 60% | 80% | 90% | 95% |
| 4 — Advanced | 40% | 50% | 70% | 85% |
| **Overall** | **~63%** | **~80%** | **~88%** | **~95%** |

## How to verify locally

```bash
cd core-agentic
cargo build                                    # clean
cargo test --lib -- --skip providers::failover # 270 passing

cd ../agentic-cli
cargo build                                    # clean
cargo run -- --help                            # --mode flag visible

# Try the new modes:
cargo run -- --mode plan run "explain the project structure"
cargo run -- --mode yolo run "run cargo test"
```

The two skipped tests in `providers::failover` are pre-existing failures
that need a Tokio runtime context. They were verified to fail on the
parent commit (`e6ba65f`) before any of this work landed.
