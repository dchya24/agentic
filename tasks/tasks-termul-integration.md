# Tasks: Termul Integration

**Feature**: termul-integration — embed core-agentic into the Termul terminal manager
**Status**: Backend + frontend foundation done; advanced UI features pending
**Created**: 2026-04-20
**Updated**: 2026-05-31
**Depends on**: `tasks-core-agentic.md`

---

## Relevant Files

- `src-tauri/Cargo.toml` — Tauri manifest (depends on `core-agentic`)
- `src-tauri/src/agentic/mod.rs` — Module exports
- `src-tauri/src/agentic/commands.rs` — Tauri command handlers
- `src-tauri/src/agentic/state.rs` — `AppState` for orchestrator + config
- `src-tauri/src/agentic/tools/pty_tool.rs` — PTY-backed `run_command`
- `src/renderer/components/agentic/` — React components (panel, sidebar, output, input, banner)
- `src/renderer/stores/agentic-store.ts` — Zustand state
- `src/renderer/lib/agentic-config-helper.ts` — Config check + wizard launch
- `src/renderer/pages/AppPreferences.tsx` — Agentic settings section

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

### Phase 1 — Backend integration (✅ done)

- [x] 0.0 Workspace + dependency wiring
- [x] 1.0 Tauri command surface
  - [x] 1.1 `agentic_load_config` / `agentic_save_config`
  - [x] 1.2 `agentic_chat` / `agentic_chat_stream`
  - [x] 1.3 `agentic_get_status`
  - [x] 1.4 `agentic_read_file_config`

### Phase 2 — Termul-specific tools (✅ done)

- [x] 2.0 PTY-based `run_command`
- [x] 2.1 Stream tool output to the agentic panel
- [x] 2.2 Reuse existing terminal session for the PTY

### Phase 3 — Frontend (✅ done)

- [x] 3.0 React components
  - [x] 3.1 `AgenticPanel`
  - [x] 3.2 `AgenticOutput`
  - [x] 3.3 `AgenticInput`
  - [x] 3.4 `AgenticSidebar`
  - [x] 3.5 `FirstRunBanner` mounted in `App.tsx`
- [x] 3.1 Tab integration
  - [x] 3.1.1 Agentic tab in `PaneContent` via workspace store
  - [x] 3.1.2 Close button + lifecycle
- [x] 3.2 Sidebar status (provider, model, token usage)

### Phase 4 — Config persistence (shared with agentic-cli) (✅ done)

- [x] 4.0 Load `~/.config/agentic/config.json` on startup
- [x] 4.1 Parse both native (multi-provider) and legacy CLI shapes
- [x] 4.2 Save config to disk on frontend updates
- [x] 4.3 Initialize config from file when sidebar mounts

### Phase 5 — Preferences & command palette (✅ done)

- [x] 5.0 Agentic section in `AppPreferences`
  - [x] 5.0.1 Status, wizard, validate, manual edit
  - [ ] 5.0.2 Reset config button
  - [ ] 5.0.3 Mask + show API-key state
- [x] 5.1 Command palette actions
  - [x] 5.1.1 Open Agentic Chat
  - [x] 5.1.2 Open Agentic Settings
  - [x] 5.1.3 Clear Agentic History
  - [ ] 5.1.4 Restart Agentic

### Phase 6 — UI polish (open)

- [ ] 6.0 Markdown rendering for AI responses (headers, lists, code blocks)
- [ ] 6.1 Syntax highlighting + copy button for code blocks
- [ ] 6.2 Message timestamps + dividers
- [ ] 6.3 Loading + error states
- [ ] 6.4 Empty state copy
- [ ] 6.5 Collapsible message-history sidebar

### Phase 7 — Advanced surfaces (open)

- [ ] 7.0 Planner agent panel
- [ ] 7.1 Multi-provider management UI
- [ ] 7.2 MCP server management UI (templates, status, enable/disable)
- [ ] 7.3 Memory search + visualization UI
- [ ] 7.4 Safety panel — risk levels, command preview, undo

### Phase 8 — Testing

- [ ] 8.0 E2E for first-run setup flow
- [ ] 8.1 E2E for chat + streaming
- [ ] 8.2 E2E for file/terminal operations triggered by agent

### Open project-wide TODOs

- [ ] Pass actual system env from backend for variable expansion (WorkspaceLayout, use-terminal-restore, use-snapshots, env-parser)
- [ ] Route to active terminal pane via context (WorkspaceLayout)
- [ ] Batch delete with multi-select (FileExplorer)
- [ ] Store secret values in OS keyring (use-projects-persistence)

> **Roadmap source:** [docs/IMPLEMENTATION_ROADMAP.md](../docs/IMPLEMENTATION_ROADMAP.md)
