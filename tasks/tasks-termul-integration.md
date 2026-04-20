# Tasks: Termul Integration

**Feature**: termul-integration - Integrate core-agentic into Termul terminal manager  
**Status**: Planning  
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

- [ ] 0.0 Create feature branch
  - [ ] 0.1 Create and checkout new branch (`git checkout -b feature/termul-agentic`)
- [ ] 1.0 Set up backend integration
  - [ ] 1.1 Add core-agentic to src-tauri/Cargo.toml
  - [ ] 1.2 Create agentic module directory
  - [ ] 1.3 Create mod.rs with module exports
- [ ] 2.0 Build Tauri commands
  - [ ] 2.1 Implement agentic_load_config command
  - [ ] 2.2 Implement agentic_save_config command
  - [ ] 2.3 Implement agentic_chat command
  - [ ] 2.4 Implement agentic_chat_stream command
  - [ ] 2.5 Implement agentic_get_status command
- [ ] 3.0 Implement Termul-specific tools
  - [ ] 3.1 Create PTY-based run_command tool
  - [ ] 3.2 Integrate with existing terminal system
  - [ ] 3.3 Add tool output streaming
- [ ] 4.0 Build React frontend
  - [ ] 4.1 Create AgenticPanel component
  - [ ] 4.2 Create AgenticOutput component
  - [ ] 4.3 Create AgenticInput component
  - [ ] 4.4 Create AgenticSidebar component
- [ ] 5.0 Integrate with tab system
  - [ ] 5.1 Add agentic tab to tab bar
  - [ ] 5.2 Implement tab creation logic
  - [ ] 5.3 Add close button handling
- [ ] 6.0 Add sidebar status
  - [ ] 6.1 Implement status display
  - [ ] 6.2 Show provider info
  - [ ] 6.3 Show token usage
- [ ] 7.0 Testing
  - [ ] 7.1 Test backend commands
  - [ ] 7.2 Test frontend components
  - [ ] 7.3 Test end-to-end flow