# Milestone 1 — Foundational Alignment with AGENT_ARCHITECTURE.md

**Date:** 2026-05-25
**Branch:** `dev`
**Scope:** `core-agentic`, `agentic-cli`

## Goal

Close the highest-impact gaps between `AGENT_ARCHITECTURE.md` and the current
implementation. These items either prevent runtime failures (runaway loops,
context overflow) or activate components that already exist in the codebase
but were never wired into the agent loop.

## Items shipped

| # | Item | Files |
|---|------|-------|
| 1.1 | Max iterations enforcement in agent loop | `core-agentic/src/orchestrator.rs` |
| 1.2 | Auto-compact integration in agent loop | `core-agentic/src/orchestrator.rs` |
| 1.3 | Tool result truncation (Compression Layer 1) | `core-agentic/src/orchestrator.rs` |
| 1.4 | Project instructions auto-load (`AGENT.md`) | `core-agentic/src/prompts.rs`, `agentic-cli/src/commands.rs` |
| 1.5 | Staleness detection for `edit_file` | `core-agentic/src/file_tracker.rs`, `core-agentic/src/tools/{read_file,edit_file,mod}.rs` |
| 1.6 | Quote normalization in `edit_file` | `core-agentic/src/tools/edit_file.rs` |
| 2.3 | Search-funnel hints in default system prompt | `core-agentic/src/prompts.rs` |

## Architectural notes

### Agent loop bounds (1.1 + 1.2 + 1.3)

The orchestrator now exposes three knobs that map directly to the
"Robustness" layer of the architecture doc:

```rust
orchestrator.set_max_iterations(30);          // 1.1 — runaway protection
orchestrator.set_auto_compact(true);          // 1.2 — Compression Layer 3
orchestrator.set_tool_result_max_chars(25_000); // 1.3 — Compression Layer 1
```

Inside both `run()` and `run_stream()`:

```
loop {
    iteration += 1;
    if iteration > max_iterations { return Err(...) }
    maybe_autocompact();              // calls Memory::compact() if needed
    // ... build messages, call provider, handle tool calls ...
}
```

`execute_tool()` wraps every tool result through `truncate_tool_result()`,
which:
- passes through unchanged when under the cap,
- slices on a UTF-8 char boundary,
- appends a `[truncated: N chars omitted of M total]` marker so the model
  knows it didn't see the full output.

### Layered system prompt (1.4 + 2.3)

The new `core_agentic::prompts` module exposes:

```rust
DEFAULT_SYSTEM_PROMPT          // the three rules + search funnel
PROJECT_INSTRUCTION_FILES      // [AGENT.md, AGENTS.md, .agentic/AGENT.md, ...]
find_project_instructions(cwd) // walk-up search
load_project_instructions(cwd) // returns (path, content)
assemble_system_prompt(base, project, user_override) -> String
```

`agentic-cli`'s `Commands::ensure_orchestrator()` calls these on startup:

```
DEFAULT_SYSTEM_PROMPT
+ AGENT.md (auto-discovered, walk-up from cwd)
+ config.system_prompt (user override, optional)
```

The result is set via `Orchestrator::set_system_prompt()`, sent to the model
on every turn.

### File staleness (1.5)

A new `FileTracker` (thread-safe `HashMap<PathBuf, SystemTime>`) is shared
between `read_file` and `edit_file` via `Arc`:

```rust
let tracker = Arc::new(FileTracker::new());
let tools = builtin_tools_with_tracker(tracker);  // both tools share it
```

- `read_file` calls `tracker.mark_read(path)` after a successful read.
- `edit_file` calls `tracker.check(path)` before applying any edit:
  - `Fresh` → proceed
  - `NeverRead` → proceed (the model may legitimately edit a file it
    just wrote, or has strong context about)
  - `Stale { last_read, current }` → reject with `"Stale read: ... was
    modified after the agent last read it. Re-read the file before editing."`
- After a successful edit, `tracker.mark_written(path)` refreshes the
  recorded mtime so subsequent edits aren't falsely flagged.

mtime comparison uses a 1 ms tolerance to handle filesystems that round
timestamps.

The default `builtin_tools()` keeps backward compatibility: it constructs
its own tracker per call, so consumers that don't care still get the
benefit within a single tool batch.

### Quote normalization (1.6)

`edit_file` first attempts an exact match. If zero matches, it retries with
both the file content and the search/replacement strings normalized
(curly `"` `'` → straight `"` `'`). On a normalized match it writes the
**normalized** content back, so the file ends up consistent.

The result JSON exposes `quotes_normalized: bool` so callers can see when
the fallback path was used.

## Test coverage added

| Module | Tests |
|--------|-------|
| `orchestrator::orchestrator_unit_tests` | 4 tests on `truncate_tool_result` (passthrough, truncate, disabled, UTF-8 boundary) |
| `prompts::tests` | 7 tests on file discovery + prompt assembly |
| `file_tracker::tests` | 5 tests on Fresh/NeverRead/Stale transitions |
| `tools::edit_file::edit_file_tests` | 5 tests: quote normalization match, staleness rejection, mark_written round-trip |

Total: **21 new tests**, all passing.

## Verification

```
core-agentic:  cargo build         # clean
core-agentic:  cargo test --lib    # 227 passing (2 pre-existing failover
                                   #              tests that need a Tokio
                                   #              runtime — unrelated)
agentic-cli:   cargo build         # clean
```

The two failing tests (`providers::failover::tests::test_failover_*`) were
verified to be pre-existing on `dev` before this work, via `git stash`.

## Mapping back to AGENT_ARCHITECTURE.md

| Doc section | Doc requirement | Status |
|-------------|-----------------|--------|
| Layer 1 — Agentic Loop | "Max turns reached" exit condition | ✅ now enforced |
| Layer 1 — Edit Tool | Staleness detection | ✅ implemented |
| Layer 1 — Edit Tool | Quote normalization | ✅ implemented |
| Layer 2 — System Prompt | Three rules (read/search/understand) | ✅ in default prompt |
| Layer 2 — System Prompt | Search funnel pattern | ✅ in default prompt |
| Layer 3 — Compression Layer 1 | Truncate large tool results | ✅ implemented |
| Layer 3 — Compression Layer 3 | Autocompact at threshold | ✅ wired into loop (heuristic; LLM-based summarization deferred to M3) |
| Layer 4 — Persistence | Project instructions auto-load | ✅ AGENT.md walk-up |

## Out of scope (deferred)

- **2.1** Concurrent tool execution
- **2.2** Permission modes enum (`Default` / `Plan` / `Yolo`)
- **2.4** Compression Layer 2 (`[Cleared]` for old tool results)
- **2.5** Cooperative cancellation (replace `process::exit(130)` with `CancellationToken`)
- **3.x** Subagents, web tools, cross-session memory, LLM-based summarizer

These will land in Milestone 2 and 3.
