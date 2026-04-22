# Tasks: Termul Integration

**Feature**: termul-integration - Integrate core-agentic into Termul terminal manager  
**Status**: In Progress (Phase 7: Config file persistence complete)  
**Created**: 2026-04-20  
**Depends on**: `tasks-core-agentic.md`

---

## Relevant Files

- `src-tauri/Cargo.toml` - Tauri manifest (add core-agentic dependency)
- `src-tauri/src/agentic/mod.rs` - Module exports
- `src-tauri/src/agentic/commands.rs` - Tauri commands
- `src-tauri/src/agentic/state.rs` - AppState management
- `src-tauri/src/agentic/tools/mod.rs` - Termul-specific tools
- `src-tauri/src/agentic/tools/pty_tool.rs` - PTY-based run_command tool
- `src/renderer/components/agentic/` - React components
- `src/renderer/stores/agentic-store.ts` - Zustand state

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

- [x] 0.0 Create feature branch
  - [x] 0.1 Create and checkout new branch (`git checkout -b feature/termul-agentic`)
- [x] 1.0 Set up backend integration
  - [x] 1.1 Add core-agentic to src-tauri/Cargo.toml
  - [x] 1.2 Create agentic module directory
  - [x] 1.3 Create mod.rs with module exports
- [x] 2.0 Build Tauri commands
  - [x] 2.1 Implement agentic_load_config command
  - [x] 2.2 Implement agentic_save_config command
  - [x] 2.3 Implement agentic_chat command
  - [x] 2.4 Implement agentic_chat_stream command
  - [x] 2.5 Implement agentic_get_status command
- [x] 3.0 Implement Termul-specific tools
  - [x] 3.1 Create PTY-based run_command tool
  - [x] 3.2 Integrate with existing terminal system
  - [x] 3.3 Add tool output streaming
- [x] 4.0 Build React frontend
  - [x] 4.1 Create AgenticPanel component
  - [x] 4.2 Create AgenticOutput component
  - [x] 4.3 Create AgenticInput component
  - [x] 4.4 Create AgenticSidebar component
- [x] 5.0 Integrate with tab system
  - [x] 5.1 Add agentic tab to tab bar
  - [x] 5.2 Implement tab creation logic
  - [x] 5.3 Add close button handling
- [x] 6.0 Add sidebar status
  - [x] 6.1 Implement status display
  - [x] 6.2 Show provider info
  - [x] 6.3 Show token usage
- [x] 7.0 Config file persistence (shared with agentic-cli)
  - [x] 7.1 Load config from ~/.config/agentic/config.json on startup
  - [x] 7.2 Parse both native (flat) and CLI (nested) config formats
  - [x] 7.3 Save config to file when load_config is called from frontend
  - [x] 7.4 Add agentic_read_file_config Tauri command for frontend
  - [x] 7.5 Init config from file on AgenticSidebar mount
- [ ] 8.0 Testing
  - [ ] 8.1 Test backend commands
  - [ ] 8.2 Test frontend components
  - [ ] 8.3 Test end-to-end flow