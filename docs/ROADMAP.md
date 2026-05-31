# Roadmap

> Consolidated current-state + next-steps for the agentic CLI + core library.
> Last updated: 2026-05-31.

This replaces the older `PLAN.md` / `IMPLEMENTATION_ROADMAP.md` /
`QUICK_START_IMPLEMENTATION.md` triad, which mixed Tauri/Termul integration
work with the CLI/core scope. The Tauri integration is no longer tracked in
this repo.

---

## What this project is

A two-crate workspace:

- **`core-agentic`** — Rust library: agent loop, tools, memory, safety, MCP
  client, multi-provider LLM support, streaming.
- **`agentic-cli`** — standalone binary: shared widgets stack (`ratatui`),
  inline + full-screen TUI modes, REPL with slash commands, config wizard.

Reference docs:
- Product framing — [AGENTIC_PRD.md](AGENTIC_PRD.md)
- Architecture overview — [architecture-alignment-overview-25052026.md](architecture-alignment-overview-25052026.md)
- Config schema — [CONFIGURATION.md](CONFIGURATION.md)
- TUI/inline widget contract — [shared-widgets-architecture-26052026.md](shared-widgets-architecture-26052026.md)
- Architecture spec the codebase is aligned with —
  [`../AGENT_ARCHITECTURE.md`](../AGENT_ARCHITECTURE.md)

---

## Current state

Foundation is complete; coverage of the architecture spec is at ~95%.

### Library (`core-agentic`)

| Layer | Surface |
|-------|---------|
| Loop | `Orchestrator::run` (sync) and `run_stream` (async, with concurrent read-only batches). Max-iterations cap. Cooperative cancel via shared `Arc<AtomicBool>`. Soft USD budget cap. |
| Tools | read / write / edit / list / glob / grep / search / run_command / run_script / fetch / web_search / update_memory / spawn_subagent / apply_patch (14 builtins). |
| Memory | Token-budget context window, message pinning, session isolation, disk persistence, keyword search, optional tiktoken backend. |
| Safety | Risk scoring (0.0–1.0), pattern-based detection, hard blocklist, path sandboxing, URL allowlist (+ optional IP-literal block), per-tool rate limiting, audit log, permission modes (`default` / `plan` / `yolo`). Prompt-injection scanner for content brought in by `fetch` / `web_search`. |
| Compression | Three-layer pipeline: tool-result truncation → older tool-result clearing → autocompact (heuristic or LLM-driven) at ~85% of token budget. |
| Providers | OpenAI (+ compatible), Anthropic, ZAI, failover wrapper. Streaming usage on the final chunk. |
| MCP | stdio + HTTP + SSE transports. |
| Cost | Built-in pricing for OpenAI / Anthropic / DeepSeek / GLM. Per-model overrides via config. `Event::Usage` per chat call. |

### CLI (`agentic-cli`)

| Layer | Surface |
|-------|---------|
| Modes | `agentic run`, `agentic interactive`, `agentic tui`, `agentic config …`. `--mode default|plan|yolo`. |
| REPL | Reedline + slash commands (`/help`, `/clear`, `/config`, `/history`, `/tools`, `/stats`, `/mcp`, `/save`, `/load`, `/provider`, `/models`, `/plan`, `/search`, `/quit`). `@` file completion, `/` command completion. Resize-safe full-width prompt. Status bar surfaces model / provider / tokens / cost / elapsed. |
| Widgets | One ratatui-based stack used by inline + TUI: markdown, spinner, progress, panels, badges, headers, tool-call panel, unified-diff renderer. Capability detection (`NO_COLOR`, `TERM=dumb`, isatty, `--color`). Zero raw `\x1b[` escapes in the source. |
| Safety UX | Risk-coloured confirmation panel. Diff preview in the confirmation prompt for `write_file` / `edit_file` / `apply_patch`. |
| Cancel | Two-stage Ctrl+C → cooperative cancel (process-global `Arc<AtomicBool>`). |

---

## Open work

Sized by effort. Each item has its own home in
[`../tasks/tasks-core-agentic.md`](../tasks/tasks-core-agentic.md) or
[`../tasks/tasks-agentic-cli.md`](../tasks/tasks-agentic-cli.md).

### Quick wins (under a day each)

- [x] **`/restart` REPL command.** Mid-session reset of memory + cancel
      flag + cumulative cost, without quitting the process.
      *(landed; alias `/reset`)*
- [x] **Status-line indicators for `AGENT.md` + persistent memory.**
      Banner + status bar surface `📄 AGENT.md` / `🧠 memory.md`
      chips when those sources are folded into the system prompt.

### Medium (2–4 days)

- [ ] **Integration tests for the orchestrator agent loop.** Mock provider,
      scripted tool-call sequences, assert memory + safety + cancel +
      budget behaviour. The only unchecked item in `tasks-core-agentic`
      Phase 8.
- [ ] **Streaming markdown polish.** Today the streaming path renders code
      fences as plain text; the parser exists in `widgets::markdown` but
      isn't applied to delta chunks.
- [ ] **End-to-end smoke test** for `agentic run` against a mock provider,
      so refactors flag regressions before they hit a release.

### Larger (1–2 weeks)

- [ ] **Planner agent.** Largest unblocked feature. Subagent infra
      (`SpawnSubagentTool`) is already in place to build on. Decompose
      goal → plan → step execution with replanning on failure. The
      existing `--mode plan` (which just denies state-changing tools)
      stays orthogonal to this.

### Out of scope (intentional)

- Concurrent tools in sync `run()` — concurrent path is `run_stream` only.
- LSP integration, prompt caching, file watching — out of scope per the
  architecture spec ("Production Extensions Beyond This Tutorial").

---

## How to choose what's next

Three reasonable paths from here:

1. **Solidify before building.** Integration tests first, then `/restart`
   + status-line indicators. Lowest risk, makes the next big feature
   safer to land.
2. **Polish daily-use ergonomics.** Streaming markdown polish + the two
   quick wins. Highest visible-per-day return.
3. **Strategic feature.** Planner agent. Biggest single bet; depends on
   the existing subagent + cancel + memory machinery being stable.

The repo currently leans toward path 1 because the recent refactors
(memory/orchestrator/safety module split, cost tracking, URL allowlist,
prompt-injection scanner) added a lot of new surface area that benefits
from end-to-end tests.

---

## Recent history

The 13 commits that landed this session, in order:

```
(in progress)  feat(cli): /restart slash command + AGENT.md/memory.md indicators
722b32c  docs: drop Tauri/Termul-flavored plans, consolidate into ROADMAP
3b1f740  feat(safety+cli): interactive diff preview in confirmation prompt
72a7edf  feat(safety): prompt-injection detector for fetch / web_search
3685965  feat(tools): add apply_patch tool for unified-diff application
bcb0ced  feat(orchestrator): token cost tracking + soft USD budget cap
7ebce34  feat(safety): URL allowlist for fetch / web_search
d91d94a  docs(tasks): refresh task files to reflect post-M3 state
18b6e94  feat(repl): /search slash command for conversation memory
4888cca  feat(tools): add web_search with Tavily/Brave/DuckDuckGo backends
e8d5dbb  feat(config): wire summarizer_model + auto_compact_with_llm from Config
dd7ceed  refactor(cli): drop dead config/markdown modules and trim unused helpers
ff9aef1  refactor(core): split memory/orchestrator/safety into submodules
```

Earlier milestone work is documented in
[`milestone-1-foundational-alignment-25052026.md`](milestone-1-foundational-alignment-25052026.md),
[`milestone-2-quality-of-life-25052026.md`](milestone-2-quality-of-life-25052026.md),
[`milestone-3-architectural-additions-25052026.md`](milestone-3-architectural-additions-25052026.md).
