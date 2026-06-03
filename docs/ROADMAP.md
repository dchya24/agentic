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
| Loop | `Orchestrator::run` (sync) and `run_stream` (async, with concurrent read-only batches). Max-iterations cap. Cooperative cancel via shared `Arc<AtomicBool>`. |
| Tools | read / write / edit / list / glob / grep / search / run_command / run_script / fetch / web_search / update_memory / spawn_subagent / apply_patch / question / todowrite (16 builtins). |
| Memory | Token-budget context window, message pinning, session isolation, disk persistence, keyword search, optional tiktoken backend. |
| Safety | Risk scoring (0.0–1.0), pattern-based detection, hard blocklist, path sandboxing, URL allowlist (+ optional IP-literal block), per-tool rate limiting, audit log, permission modes (`default` / `plan` / `yolo`). Prompt-injection scanner for content brought in by `fetch` / `web_search`. |
| Compression | Three-layer pipeline: tool-result truncation → older tool-result clearing → autocompact (heuristic or LLM-driven) at ~85% of token budget. |
| Providers | OpenAI-compatible, Anthropic-compatible. |
| Vision | Image attachments (PNG/JPEG/GIF/WebP) flow through `core_agentic::Attachment`; OpenAI-compatible providers serialize them as multimodal content. Per-model `ModelCapabilities` gate the request before dispatch. |
| MCP | stdio + HTTP + SSE transports. |

### CLI (`agentic-cli`)

| Layer | Surface |
|-------|---------|
| Modes | `agentic run`, `agentic interactive`, `agentic tui`, `agentic config …`. `--mode default|plan|yolo`. |
| REPL | Reedline + slash commands (`/help`, `/new`, `/config`, `/history`, `/tools`, `/stats`, `/mcp`, `/sessions`, `/models`, `/plan`, `/search`, `/image`, `/provider`, `/quit`). `@` file completion (auto-detects images for vision channel), `/` command completion, `/models ` model name completion. Sessions auto-saved to `~/.config/agentic/sessions/`. Inline fuzzy-select model picker (dialoguer). Resize-safe full-width prompt. Status bar surfaces model / provider / tokens / vision indicator / elapsed. |
| Widgets | One ratatui-based stack used by inline + TUI: markdown, spinner, progress, panels, badges, headers, tool-call panel, unified-diff renderer. Capability detection (`NO_COLOR`, `TERM=dumb`, isatty, `--color`). Zero raw `\x1b[` escapes in the source. |
| Safety UX | Risk-coloured confirmation panel. Diff preview in the confirmation prompt for `write_file` / `edit_file` / `apply_patch`. |
| Cancel | Two-stage Ctrl+C → cooperative cancel (process-global `Arc<AtomicBool>`). |

---

## Phase 11 — Prompt Caching (landed)

Prompt caching for Anthropic providers: `cache_control` breakpoints injected
into the system prompt (SystemOnly) and conversation prefix (Prefix). Cache
metrics (`cache_read_input_tokens`, `cache_creation_input_tokens`) flow through
`ChatUsage`. CLI observability shows cache hit ratio in the status bar, stats
panel, and goodbye summary. Session persistence includes cache token counters.

**Deferred (v2):**
- ~~Wire usage events from the orchestrator to `SessionStats`~~ — caching works, just no visibility UI.
- ~~Expose cache settings in `agentic config` wizard~~ — configurable via manual config.toml edit.

## Open work

Sized by effort. Each item has its own home in
[`../tasks/tasks-core-agentic.md`](../tasks/tasks-core-agentic.md) or
[`../tasks/tasks-agentic-cli.md`](../tasks/tasks-agentic-cli.md).

### Quick wins (under a day each)

- [x] **`/new` REPL command.** Mid-session reset: saves current session,
      creates fresh one, clears conversation + memory + cost.
      Aliases `/n`, `n`. Replaces `/clear` and `/restart`.
- [x] **Status-line indicators for `AGENT.md` + persistent memory.**
      Banner + status bar surface `📄 AGENT.md` / `🧠 memory.md`
      chips when those sources are folded into the system prompt.
- [x] **Session management.** Auto-save conversations to
      `~/.config/agentic/sessions/` as JSON. `/sessions` to list,
      `/sessions <id>` to resume. No more `history.txt`.
- [x] **`question` and `todowrite` tools.** Interactive question tool
      + session-scoped task list. Documented in
      [milestone-4-interactive-tools-02062026.md](milestone-4-interactive-tools-02062026.md).
      CLI handler wiring pending.

### Medium (2–4 days)

- [x] **Integration tests for the orchestrator agent loop.** 7 tests in
      `core-agentic/tests/orchestrator_loop.rs` covering happy-path tool
      execution, max-iterations cap, plan-mode denial, cooperative
      cancel, event emission, memory recording, and the
      `/restart` workflow.
- [x] **Streaming markdown polish.** `MarkdownContent::parse_partial`
      auto-closes unclosed code fences for in-flight delta streams.
- [x] **End-to-end smoke test** for `agentic run` against a mock provider,
       so refactors flag regressions before they hit a release.

### Medium (2–4 days)

- [x] **Wire `question` + `todowrite` CLI handlers.** Register
       `QuestionHandler` in interactive/TUI modes, `TodoChangeHandler`
       for progress rendering. See milestone-4 doc.
- [x] **End-to-end smoke test** for `agentic run` against a mock provider,
       so refactors flag regressions before they hit a release.

### Larger (1–2 weeks)

- [x] **Planner agent.** Subagent infra (`SpawnSubagentTool`) in place.
      Goal → plan → step execution with replanning on failure, approval
      flow, dependency ordering, Subagent delegation, `PlanProgress` +
      `PlanReplanned` events. 8 E2E integration tests in
      `tests/planner_loop.rs`. `--mode plan` and `--plan` flag route
      through the planner agent. Implemented in a parallel branch and
      merged alongside milestone-3/4 work.
- [x] **Skill system.** Skill format (`SKILL.md`), discovery
      (5 locations: global `~/.agents/skills/`, `~/.config/agentic/skills/`,
      project `.agents/skills/`, `.agentic/skills/` walk-up, + compat dirs),
      `skill` tool to load domain-specific instructions, and
      `skill create` / `skill list` / `skill info` CLI commands.
      Implemented in Milestone 5.

### Out of scope (intentional)

- LSP integration, codesearch, prompt caching, file watching — out of scope
  per the architecture spec ("Production Extensions Beyond This Tutorial").

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

Key changes that landed recently:

```
(latest)     feat: Phase 11 — Prompt caching (core + CLI observability)
(latest)     feat(cli): session system — auto-save, /new, /sessions, /sessions <id>
(latest)     feat(tools): add question + todowrite tools (M4)
(latest)     docs: update TOOL_REFERENCE.md for 16-tool set
(latest)     refactor(cli): replace ratatui model_picker with dialoguer inline fuzzy-select
(latest)     feat(cli): /models auto-complete via reedline completer
(latest)     feat(cli): /restart slash command + AGENT.md/memory.md indicators
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
