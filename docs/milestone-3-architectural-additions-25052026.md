# Milestone 3 — Architectural Additions

**Date:** 2026-05-25
**Branch:** `dev`
**Scope:** `core-agentic`, `agentic-cli`

## Goal

Close the remaining gaps with `AGENT_ARCHITECTURE.md` Layer 4 by adding
the four "production extension" features:

- 3.1 Subagents
- 3.2 Web access (`fetch`)
- 3.3 Cross-session memory file
- 3.4 LLM-based summarization

## Items shipped

| # | Item | Files |
|---|------|-------|
| 3.1 | `spawn_subagent` tool | `core-agentic/src/tools/spawn_subagent.rs` |
| 3.2 | `fetch` tool with HTML→text + cache | `core-agentic/src/tools/fetch.rs` |
| 3.3 | Persistent memory + `update_memory` tool | `core-agentic/src/{memory_file,tools/update_memory}.rs` |
| 3.4 | LLM-based memory summarization | `core-agentic/src/{memory,orchestrator}.rs` |

## Architectural notes

### 3.4 LLM-based summarization

The orchestrator's autocompact path used to truncate strings as a stand-in
for summarization. Now `Memory` exposes two new APIs:

```rust
mem.build_summarization_prompt() -> Option<String>
mem.compact_with_summary(&str)  -> SummarizedContext
```

`build_summarization_prompt` walks the messages slated for compaction
(everything older than `keep_recent`, excluding pinned), and returns a
single prompt that asks the LLM to preserve user intent, decisions, files
touched, and open questions. The pure-string design keeps `memory.rs`
free of any provider/runtime dependency.

The orchestrator wires this in via two new setters:

```rust
orch.set_auto_compact_with_llm(true);
orch.set_summarizer_model("gpt-4o-mini");  // optional override
```

When enabled, `maybe_autocompact` builds the prompt, calls the provider
(synchronously), and feeds the response into `compact_with_summary`. On
provider error it falls back to the heuristic `compact()` so the loop
never blocks on summarization.

The summarizer call is intentionally synchronous and `&self` — it runs
between turns, not inside a hot streaming path.

### 3.3 Cross-session memory file

Two layers, mirroring the architecture doc's "memory across sessions":

- **User-global**: `~/.config/agentic/memory.md`
  (overridable via `$AGENTIC_MEMORY_PATH`, respects `$XDG_CONFIG_HOME`)
- **Project-local**: `.agentic/memory.md` walking up from cwd

`memory_file::assemble_memory_section(&cwd) -> Option<String>` returns the
labeled concatenation. `Commands::ensure_orchestrator()` appends it to
the system prompt under a `# Persistent Memory` heading after the AGENT.md
project instructions.

The agent writes to memory via the new `update_memory` tool:

```jsonc
{
  "name": "update_memory",
  "arguments": {
    "content": "User prefers async/await over .then()",
    "scope": "user"   // or "project"
  }
}
```

Each entry is appended with a UTC-timestamped `## YYYY-MM-DD HH:MM UTC`
header so accumulated notes stay readable. The tool is marked
`is_read_only = false` so it isn't scheduled in a parallel batch with
reads.

### 3.1 Subagents

`SpawnSubagentTool` is the first tool that needs structured access to
the parent's runtime. It carries:

```rust
provider: Arc<dyn LLMProvider>,
tools: ToolRegistry,             // shared, by clone of the Arc inside
model: String,
mode: PermissionMode,            // mirror of parent's mode
parent_cancel: Option<Arc<AtomicBool>>,
max_iterations: u32,             // default 12
```

Inside `execute()`:

```
let mut sub = Orchestrator::new(self.provider.clone(), self.tools.clone());
sub.set_model(...);
sub.set_max_iterations(max_iter);
sub.set_system_prompt(SUBAGENT_SYSTEM_PROMPT);
sub.set_permission_mode(self.mode);
sub.set_cancel_handle(parent_cancel);   // linked abort
let answer = sub.run(task)?;
return Ok(answer);
```

This matches the architecture doc's table:

| Component | Shared | Isolated |
|-----------|:------:|:--------:|
| Messages  |        | ✅ (fresh `Memory`) |
| Tools     | ✅     |          |
| Abort     | ✅ (linked cancel) |          |
| Permission rules | ✅ (parent mode copied) |          |

Subagents use the **synchronous** `run()` because `Tool::execute` is sync
— no Tokio context required, and the orchestrator may have invoked us
from `spawn_blocking`.

The `SUBAGENT_SYSTEM_PROMPT` reminds the model that:
1. It has fresh context, no parent memory.
2. Its final text is what the parent sees.
3. It should not ask clarifying questions.
4. It should stop as soon as it has an answer.

CLI integration: `Commands::ensure_orchestrator()` registers a
`SpawnSubagentTool` on the same `ToolRegistry` after the built-ins, so the
parent agent can call it. The subagent inherits the same registry, which
means it can also call `spawn_subagent` (nested subagents) — in practice
the model rarely does so unprompted, but if it becomes a problem the
parent can register a stripped registry for subagents only.

### 3.2 Web access (`fetch`)

A lightweight `fetch` tool that:

1. Validates the URL is http/https (no `file://`, no `gopher://`, etc).
2. Uses `reqwest::blocking` (already a dependency) so it fits the sync
   `Tool::execute` signature.
3. Sets a 20s timeout and a `User-Agent: agentic-cli/<version>` header.
4. Detects HTML responses by content-type or doctype heuristic.
5. For HTML: drops `<script>`, `<style>`, `<noscript>` blocks (with
   contents); converts block-level closers + `<br>` to newlines; strips
   remaining tags; decodes the most common entities; collapses
   whitespace. The `regex` crate doesn't support backreferences, so each
   block tag has its own pattern.
6. Caches results in a process-global `Mutex<HashMap<String, String>>`
   keyed by URL — same URL fetched twice in one session is a cache hit.
7. Returns up to `max_chars` (default 25_000) of cleaned text with a
   `[truncated: ...]` marker if the full content was longer.

The output JSON:
```jsonc
{
  "url": "...",
  "content": "...",
  "total_chars": 42_000,
  "truncated": true,
  "cached": false
}
```

The tool is marked `is_read_only = true` so it can run in parallel with
other reads in the orchestrator's concurrent path.

A `web_search` tool was deliberately deferred — it requires an external
API key (Brave / Serper / SerpAPI / DuckDuckGo HTML scrape). Adding it
later is a 1-file change with the same shape.

## Test coverage added

| Module | Tests |
|--------|-------|
| `memory::tests` | +6 (build_prompt: nothing-to-summarize / old-only / pinned-skipped; compact_with_summary: keeps recent / no-op below threshold / accumulates with prior summary) |
| `memory_file::tests` | +6 (env override, append+header, walk-up discovery, directory creation, empty assemble = None, both-present assembly) |
| `tools::update_memory::tests` | +3 (rejects empty / unknown scope / writes user scope) |
| `tools::spawn_subagent::tests` | +3 (rejects empty task / returns answer / respects max_iterations) — uses a `ScriptedProvider` fake |
| `tools::fetch::tests` | +5 (strip tags+scripts, paragraph breaks, reject non-http, reject empty, content-type detection) |

Total: **23 new tests**. Cumulative across all milestones: **+64 tests**.

## Verification

```
core-agentic:  cargo build         # clean
core-agentic:  cargo test --lib    # 270 passing
                                   # (2 pre-existing failover tests still
                                   #  failing — unrelated, need Tokio rt)
agentic-cli:   cargo build         # clean
```

## Mapping back to AGENT_ARCHITECTURE.md

| Doc section | Doc requirement | Before M3 | After M3 |
|-------------|-----------------|-----------|----------|
| Layer 3 — Compression Layer 3 | LLM-based autocompact | ⚠️ heuristic only | ✅ opt-in LLM with heuristic fallback |
| Layer 4 — Subagents | Fresh context, shared tools, linked cancel | ❌ | ✅ |
| Layer 4 — Web Access | `fetch` tool with HTML→text + cache | ❌ | ✅ |
| Layer 4 — Persistence — Memory across sessions | Auto-loaded notes file + agent-writable | ❌ | ✅ user + project scopes |

## Out of scope

- **`web_search`**: needs an external API; trivial to add when picking a
  provider (DuckDuckGo HTML, Brave, SerpAPI, etc).
- **Multi-model strategy** for the summarizer: the `summarizer_model`
  setter exists, but the CLI doesn't yet expose a config field for it.
  Wiring it through `Config::ModelConfig` is a follow-up.
- **Domain-based permissions for `fetch`**: the architecture doc mentions
  pre-approved domains auto-allowing while unknown domains ask. The
  current implementation runs the call through the existing safety
  pipeline (so blocklist/yolo/plan all work) but doesn't add a
  domain allowlist. Follow-up if needed.

## Final alignment with AGENT_ARCHITECTURE.md

After M1 + M2 + M3, the project's coverage of the architecture doc:

| Layer | Coverage |
|-------|----------|
| Layer 1 — Foundation | ~98% (sync `run()` still has serial tools, intentional) |
| Layer 2 — Intelligence | ~100% |
| Layer 3 — Robustness | ~95% (cleared old results + heuristic + LLM compaction; permission modes; sandbox; rate-limit; audit) |
| Layer 4 — Advanced | ~85% (subagents, web fetch, persistence, streaming, concurrency, MCP all in; only `web_search` deferred) |

**Overall: ~88% → ~95%** of the architecture doc is now implemented.
