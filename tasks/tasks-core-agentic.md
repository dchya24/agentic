# Tasks: Core Agentic Library

**Feature**: core-agentic - Rust library for AI agent orchestration  
**Status**: In Progress (Safety + Memory enhanced, Phase 1 core done)  
**Created**: 2026-04-20
**Updated**: 2026-05-04

---

## Relevant Files

- `core-agentic/Cargo.toml` - Library manifest
- `core-agentic/src/lib.rs` - Main exports
- `core-agentic/src/orchestrator.rs` - Agent loop implementation
- `core-agentic/src/agent.rs` - Agent struct
- `core-agentic/src/tool.rs` - Tool trait
- `core-agentic/src/tool_registry.rs` - Tool registration
- `core-agentic/src/memory.rs` - Context/memory management
- `core-agentic/src/safety.rs` - Safety checks
- `core-agentic/src/events.rs` - Event types
- `core-agentic/src/providers/mod.rs` - Provider trait
- `core-agentic/src/providers/openai.rs` - OpenAI provider
- `core-agentic/src/tools/mod.rs` - Built-in tools
- `core-agentic/src/tools/run_command.rs` - Command execution tool
- `core-agentic/src/tools/read_file.rs` - File read tool
- `core-agentic/src/tools/write_file.rs` - File write tool
- `core-agentic/src/tools/list_files.rs` - Directory listing tool

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

- [ ] 0.0 Create feature branch
  - [ ] 0.1 Create and checkout new branch (`git checkout -b feature/core-agentic`)
- [ ] 1.0 Set up project structure
  - [ ] 1.1 Create core-agentic directory
  - [ ] 1.2 Initialize Cargo.toml with dependencies
  - [ ] 1.3 Create lib.rs with module exports
- [ ] 2.0 Define core types
  - [ ] 2.1 Implement AgentConfig struct
  - [ ] 2.2 Implement Message and MessageRole
  - [ ] 2.3 Implement ToolCall and ToolResult
  - [ ] 2.4 Implement ToolSchema
- [ ] 3.0 Build Tool system
  - [ ] 3.1 Define Tool trait
  - [ ] 3.2 Implement ToolRegistry
  - [ ] 3.3 Create built-in tools (run_command, read_file, write_file, list_files)
- [ ] 4.0 Implement LLM Provider
  - [ ] 4.1 Define Provider trait
  - [ ] 4.2 Implement OpenAI-compatible provider
  - [ ] 4.3 Add streaming support
- [ ] 5.0 Build Orchestrator
  - [ ] 5.1 Implement agent loop logic
  - [ ] 5.2 Implement state machine (IDLE → PLANNING → EXECUTING → etc.)
  - [ ] 5.3 Add tool execution flow
- [x] 6.0 Add Memory management
  - [x] 6.1 Implement Memory struct
  - [x] 6.2 Add message tracking
  - [x] 6.3 Add context window/summarization
  - [x] 6.4 Add message pinning
  - [x] 6.5 Add session isolation
  - [x] 6.6 Add disk persistence
  - [x] 6.7 Add message search
- [x] 7.0 Implement Safety system
  - [x] 7.1 Add risk detection (numeric scoring 0.0-1.0)
  - [x] 7.2 Implement confirmation system (configurable thresholds)
  - [x] 7.3 Add blocked commands list (configurable)
  - [x] 7.4 Add pattern-based risk detection (25+ regex patterns)
  - [x] 7.5 Add path sandboxing
  - [x] 7.6 Add rate limiting per tool
  - [x] 7.7 Add audit logging (ring buffer)
- [ ] 8.0 Add Events system
  - [ ] 8.1 Define event types
  - [ ] 8.2 Implement event emission
- [x] 9.0 Testing
  - [x] 9.1 Write unit tests for core types
  - [x] 9.2 Write unit tests for tools
  - [x] 9.3 Write unit tests for safety (48 tests)
  - [x] 9.4 Write unit tests for memory (48 tests)
  - [ ] 9.5 Write integration tests for orchestrator

> **Full testing plan:** [docs/PLAN_TESTING.md](../../docs/PLAN_TESTING.md)
