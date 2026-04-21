# Tasks: Agentic CLI

**Feature**: agentic-cli - Standalone CLI binary using core-agentic  
**Status**: Done  
**Created**: 2026-04-20  
**Depends on**: `tasks-core-agentic.md`

---

## Relevant Files

- `agentic-cli/Cargo.toml` - Binary manifest
- `agentic-cli/src/main.rs` - Entry point
- `agentic-cli/src/cli.rs` - CLI argument parsing
- `agentic-cli/src/commands.rs` - Command handlers
- `agentic-cli/src/interactive.rs` - Interactive mode
- `agentic-cli/src/output.rs` - Streaming output
- `agentic-cli/src/config.rs` - Config loading
- `agentic-cli/config/default.json` - Default config

---

## Instructions for Completing Tasks

**IMPORTANT:** As you complete each task, check it off by changing `- [ ]` to `- [x]`. Update after completing each sub-task.

## Tasks

- [x] 0.0 Create feature branch
  - [x] 0.1 Create and checkout new branch (`git checkout -b feature/agentic-cli`)
- [x] 1.0 Set up project structure
  - [x] 1.1 Create agentic-cli directory
  - [x] 1.2 Initialize Cargo.toml with dependencies (core-agentic, clap, tokio)
  - [x] 1.3 Create main.rs with basic structure
- [x] 2.0 Implement CLI argument parsing
  - [x] 2.1 Create CLI struct with clap derive
  - [x] 2.2 Add subcommands (run, interactive, config, version)
  - [x] 2.3 Add options (provider, model, config, verbose, no-stream)
- [x] 3.0 Build configuration system
  - [x] 3.1 Create config file structure
  - [x] 3.2 Implement config loading from JSON
  - [x] 3.3 Implement environment variable substitution
  - [x] 3.4 Create default config
- [x] 4.0 Implement command handlers
  - [x] 4.1 Implement single task run
  - [x] 4.2 Integrate with core-agentic Orchestrator
  - [x] 4.3 Handle streaming output
- [x] 5.0 Build interactive mode
  - [x] 5.1 Create interactive loop
  - [x] 5.2 Add input reading (readline-style)
  - [x] 5.3 Add exit command handling
- [x] 6.0 Implement streaming output
  - [x] 6.1 Create output formatter
  - [x] 6.2 Add color support (ansi colors)
  - [x] 6.3 Handle output categories (thought, tool, tool_output, system, error)
- [ ] 7.0 Implement confirmation UI
  - [ ] 7.1 Add confirmation prompt
  - [ ] 7.2 Handle user input (y/n/s/a/q)
  - [ ] 7.3 Integrate with core-agentic safety
- [ ] 8.0 Add error handling
  - [ ] 8.1 Handle provider errors
  - [ ] 8.2 Handle tool errors
  - [ ] 8.3 Add recovery logic
- [x] 9.0 Testing
  - [x] 9.1 Test CLI argument parsing
  - [x] 9.2 Test config loading
  - [ ] 9.3 Test command execution