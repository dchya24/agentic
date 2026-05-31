# Tasks: Agentic CLI

**Feature**: agentic-cli — Standalone CLI binary using core-agentic
**Status**: Foundation done; shared widgets stack landed; alignment refactor merged
**Created**: 2026-04-20
**Updated**: 2026-05-31
**Depends on**: `tasks-core-agentic.md`

---

## Relevant Files

- `agentic-cli/Cargo.toml` — Binary manifest
- `agentic-cli/src/main.rs` — Entry point + Ctrl+C handler + `--color` flag
- `agentic-cli/src/cli.rs` — clap argument tree
- `agentic-cli/src/commands.rs` — Command handlers (run, interactive, config, models, mcp, tools, …)
- `agentic-cli/src/interactive.rs` — Reedline REPL + slash-command parser
- `agentic-cli/src/confirmation.rs` — Risk-coloured confirmation panel
- `agentic-cli/src/file_ref.rs` — `@`-completion for file references
- `agentic-cli/src/widgets/` — Shared ratatui widgets (markdown, spinner, progress, components, tool_call, diff, inline, capabilities)
- `agentic-cli/src/tui/` — Full-screen TUI mode (app, ui, dropdown, input)

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

### Phase 1 — Foundation (✅ done)

- [x] 0.0 Project setup
  - [x] 0.1 Crate skeleton with `core-agentic`, `clap`, `tokio`
  - [x] 0.2 Cargo manifest + workspace integration
- [x] 1.0 CLI argument parsing
  - [x] 1.1 Subcommands (`run`, `interactive`, `tui`, `config`, `models`, …)
  - [x] 1.2 Options (`--provider`, `--model`, `--config`, `--mode`, `--color`, …)
- [x] 2.0 Configuration
  - [x] 2.1 Reads `core_agentic::Config` (multi-provider). The legacy single-provider shape was deleted.
  - [x] 2.2 Env-var substitution
  - [x] 2.3 Config wizard / templates / setup commands

### Phase 2 — Run loop & output (✅ done)

- [x] 3.0 Single-task run
  - [x] 3.1 `agentic run <task>` integrates with `Orchestrator`
  - [x] 3.2 Streaming output through `widgets::inline`
  - [x] 3.3 Confirmation handler wired into `Safety`
- [x] 4.0 Interactive REPL
  - [x] 4.1 Reedline-based input
  - [x] 4.2 Slash commands: `/help`, `/clear`, `/config`, `/history`, `/tools`, `/stats`, `/mcp`, `/save`, `/load`, `/provider`, `/models`, `/plan`, `/search`, `/quit`
  - [x] 4.3 `@`-completion with recursive `.gitignore`-aware listing
  - [x] 4.4 `/`-completion popup with command descriptions
  - [x] 4.5 Tab navigation, inline hints, Ctrl+R history search
  - [x] 4.6 Status bar (model, provider, tokens, elapsed)
  - [x] 4.7 Resize-safe full-width prompt
- [x] 5.0 Full-screen TUI mode
  - [x] 5.1 `agentic tui` launches alternate-screen ratatui app
  - [x] 5.2 Subscribes to orchestrator events; renders tool calls + diffs through shared widgets
  - [x] 5.3 Dropdown widget for `@` and `/`
  - [x] 5.4 Live transient spinner shared with inline mode

### Phase 3 — Shared widgets architecture (✅ done)

- [x] 6.0 Widget extraction
  - [x] 6.1 Single ratatui-based stack used by inline + TUI
  - [x] 6.2 Capability detection (`NO_COLOR`, `TERM=dumb`, isatty, `--color` override)
  - [x] 6.3 Markdown renderer
  - [x] 6.4 Tool-call panel + tool-result notification
  - [x] 6.5 Unified-diff renderer + summary line
  - [x] 6.6 Spinner + progress widgets
  - [x] 6.7 Panels, badges, headers, gradients, tables, sparklines
  - [x] 6.8 Zero raw `\x1b[` escapes remain in `agentic-cli/src/`

### Phase 4 — Safety / permissions (✅ done)

- [x] 7.0 `--mode default|plan|yolo` flag wired through to `Safety`
- [x] 7.1 Two-stage Ctrl+C → cooperative cancel (process-global `Arc<AtomicBool>`)
- [x] 7.2 Risk-coloured confirmation panel (border colour drives by risk level)

### Phase 5 — Memory & search (✅ done)

- [x] 8.0 `/search <query>` slash command surfaces `Memory::search`
- [x] 8.1 Snippet renderer with role badges + UTF-8-safe windowing

### Phase 6 — Testing & docs

- [x] 9.0 Unit tests for argument parsing + widget helpers
- [x] 9.1 README in `agentic-cli/`
- [ ] 9.2 End-to-end smoke test for `agentic run` against a mock provider

### Open / future work

- [ ] Markdown rendering polish for streaming responses (richer code-fence handling, copy hints)
- [ ] Restart-agentic / kill-current-turn slash command
- [ ] Status line: surface persistent-memory + AGENT.md detection state

> **Architecture reference:** [docs/shared-widgets-architecture-26052026.md](../docs/shared-widgets-architecture-26052026.md)
