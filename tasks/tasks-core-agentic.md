# Tasks: Core Agentic Library

**Feature**: core-agentic — Rust library for AI agent orchestration
**Status**: Foundation complete; ~95% architecture coverage (post-M3 + alignment refactor)
**Created**: 2026-04-20
**Updated**: 2026-05-31

---

## Relevant Files

- `core-agentic/Cargo.toml` — Library manifest
- `core-agentic/src/lib.rs` — Public re-exports
- `core-agentic/src/orchestrator/` — Agent loop (mod, messages, tool_exec, compaction, run)
- `core-agentic/src/agent.rs` — Higher-level Agent wrapper
- `core-agentic/src/tool.rs` — Tool trait
- `core-agentic/src/tool_registry.rs` — Tool registration
- `core-agentic/src/memory/` — Context/memory (mod, types, store, compaction, persist)
- `core-agentic/src/safety/` — Risk + permissions (mod, risk, config, audit, engine)
- `core-agentic/src/events.rs` — Event emitter + types
- `core-agentic/src/providers/` — `openai`, `anthropic`, `failover`
- `core-agentic/src/tools/` — Built-in tools (read, write, edit, list, search, glob, grep, run_command, run_script, fetch, web_search, update_memory, spawn_subagent)
- `core-agentic/src/prompts.rs` — Default system prompt + AGENT.md loading
- `core-agentic/src/file_tracker.rs` — Read/edit staleness detection
- `core-agentic/src/memory_file.rs` — Persistent user/project memory.md
- `core-agentic/src/diff_util.rs` — Unified-diff producer (consumed by widgets::diff)
- `core-agentic/src/mcp/` — MCP client (transport, types, client)

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

### Phase 1 — Foundation (✅ done)

- [x] 0.0 Project setup
  - [x] 0.1 Create core-agentic crate
  - [x] 0.2 Cargo manifest + workspace integration
  - [x] 0.3 Public exports in `lib.rs`
- [x] 1.0 Core types
  - [x] 1.1 `Message`, `MessageRole`
  - [x] 1.2 `ToolCall`, `ToolResult`, `ToolSchema`
  - [x] 1.3 `Config` (multi-provider, MCP, safety, output, agent loop)
- [x] 2.0 Tool system
  - [x] 2.1 `Tool` trait + `ToolRegistry`
  - [x] 2.2 Built-ins: read, write, edit, list, search, glob, grep
  - [x] 2.3 Built-ins: run_command, run_script
  - [x] 2.4 Built-ins: fetch, web_search (URL-aware tools)
  - [x] 2.5 Built-in: apply_patch (atomic unified-diff applier)
  - [x] 2.4 Edit-tool string replacement with uniqueness + quote normalization
  - [x] 2.5 `FileTracker` — staleness detection between read/edit
- [x] 3.0 LLM providers
  - [x] 3.1 OpenAI-compatible
  - [x] 3.2 Anthropic
  - [x] 3.3 Streaming (text deltas + buffered tool calls)
  - [x] 3.4 Failover wrapper

### Phase 2 — Orchestrator (✅ done)

- [x] 4.0 Agent loop
  - [x] 4.1 Synchronous `run()` and async `run_stream()`
  - [x] 4.2 State machine
  - [x] 4.3 Max-iterations cap
  - [x] 4.4 Cooperative cancel (`Arc<AtomicBool>`)
- [x] 5.0 System prompt
  - [x] 5.1 Default prompt with the three rules
  - [x] 5.2 `AGENT.md` walk-up auto-load
  - [x] 5.3 Persistent memory section in prompt

### Phase 3 — Memory (✅ done)

- [x] 6.0 Memory management
  - [x] 6.1 `Memory` struct + message tracking
  - [x] 6.2 Token-budget context window (replaces message-count slicing)
  - [x] 6.3 Pinned messages
  - [x] 6.4 Session isolation
  - [x] 6.5 Disk persistence (`persist`, `load`, `list_sessions`, `delete_session`)
  - [x] 6.6 Keyword search (`Memory::search`)
  - [x] 6.7 Optional tiktoken backend behind a feature flag
  - [x] 6.8 Configurable `context_budget_ratio`

### Phase 4 — Safety (✅ done)

- [x] 7.0 Safety system
  - [x] 7.1 Risk scoring (0.0–1.0)
  - [x] 7.2 Configurable confirmation thresholds
  - [x] 7.3 Hard blocklist (`rm -rf /`, `mkfs`, `dd if=`, fork bombs, …)
  - [x] 7.4 Pattern-based risk detection (25+ regex patterns)
  - [x] 7.5 Path sandboxing
  - [x] 7.6 Per-tool rate limiting
  - [x] 7.7 Audit log (ring buffer)
  - [x] 7.8 Permission modes: `default` / `plan` / `yolo`
  - [x] 7.9 URL allowlist for `fetch` / `web_search` (`safety.allowed_domains`, `safety.block_ip_urls`)
  - [x] 7.10 Prompt-injection scanner for content from `fetch` / `web_search` (annotates results, does not block)

### Phase 5 — Compression (✅ done)

- [x] 8.0 Three-layer compression pipeline
  - [x] 8.1 Layer 1 — UTF-8 safe truncate of large tool results
  - [x] 8.2 Layer 2 — replace older tool results with `[Cleared]` placeholder
  - [x] 8.3 Layer 3 — heuristic autocompact at ~85% budget
  - [x] 8.4 Layer 3 — LLM-based summarization (opt-in, falls back on error)
  - [x] 8.5 Wired through Config (`agent.auto_compact_with_llm`, `agent.summarizer_model`)

### Phase 6 — Advanced (✅ done)

- [x] 9.0 Subagents
  - [x] 9.1 `SpawnSubagentTool` with fresh context
  - [x] 9.2 Shared `ToolRegistry` + `Provider` via `Arc`
  - [x] 9.3 Linked cancel flag with parent
- [x] 10.0 Web access
  - [x] 10.1 `fetch` tool — HTML→text + per-session cache
  - [x] 10.2 `web_search` tool — Tavily / Brave / DuckDuckGo backends
- [x] 11.0 Persistent memory
  - [x] 11.1 User-global `~/.config/agentic/memory.md`
  - [x] 11.2 Project-local `<cwd>/memory.md` walk-up
  - [x] 11.3 `update_memory` tool

### Phase 7 — Events / Concurrency / MCP (✅ done)

- [x] 12.0 Events system
  - [x] 12.1 Event types (`ToolCall`, `ToolOutput`, …)
  - [x] 12.2 `EventEmitter` (Mutex<Vec<…>>) for `&self.on()`
  - [x] 12.3 Public `on_event` / `clear_event_handlers`
- [x] 13.0 Concurrency
  - [x] 13.1 Concurrent read-only tool batches in `run_stream`
- [x] 14.0 MCP integration
  - [x] 14.1 stdio transport
  - [x] 14.2 HTTP transport
  - [x] 14.3 SSE transport
  - [x] 14.4 Tool schema conversion

### Phase 8 — Testing & docs

- [x] 15.0 Unit tests (290+ across the crate; see milestone docs)
- [x] 15.1 Integration tests for the orchestrator agent loop
      (`tests/orchestrator_loop.rs`: 8 end-to-end tests covering happy
      path, max-iterations, plan-mode denial, cancel, budget cap, event
      emission, memory recording, restart workflow)
- [x] 16.0 Architecture docs
  - [x] 16.1 `AGENT_ARCHITECTURE.md` reference
  - [x] 16.2 Three milestone docs (foundational / quality-of-life / additions)
  - [x] 16.3 Architecture alignment overview

### Phase 9 — Cost & budget (✅ done)

- [x] 17.0 Per-token pricing module (`pricing.rs`)
  - [x] 17.1 Built-in rates for OpenAI / Anthropic / DeepSeek / GLM
  - [x] 17.2 Longest-`contains` fallback for vendor-suffixed model names
  - [x] 17.3 `Config.agent.pricing` per-model overrides
- [x] 18.0 Cost tracking on `Orchestrator`
  - [x] 18.1 `record_usage()` updates cumulative cost on every chat call
  - [x] 18.2 `Event::Usage { model, input_tokens, output_tokens, cost_usd, cumulative_cost_usd }`
  - [x] 18.3 Soft USD budget cap (`Config.agent.budget_usd`)
  - [x] 18.4 Loop bails with `"Budget exceeded"` at next iteration when over cap
  - [x] 18.5 Streaming providers attach `usage` to the final chunk

### Open / future work

- [ ] Concurrent tools in sync `run()` — intentional gap; concurrent path is `run_stream` only
- [ ] LSP integration, prompt caching, file watching — out of scope per architecture doc

> **Architecture reference:** [AGENT_ARCHITECTURE.md](../AGENT_ARCHITECTURE.md)
> **Coverage detail:** [docs/architecture-alignment-overview-25052026.md](../docs/architecture-alignment-overview-25052026.md)
> **Roadmap:** [docs/ROADMAP.md](../docs/ROADMAP.md)
