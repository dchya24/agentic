# Tasks: Agentic CLI

**Feature**: agentic-cli - Standalone CLI binary using core-agentic  
**Status**: Planning  
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

- [ ] 0.0 Create feature branch
  - [ ] 0.1 Create and checkout new branch (`git checkout -b feature/agentic-cli`)
- [ ] 1.0 Set up project structure
  - [ ] 1.1 Create agentic-cli directory
  - [ ] 1.2 Initialize Cargo.toml with dependencies (core-agentic, clap, tokio)
  - [ ] 1.3 Create main.rs with basic structure
- [ ] 2.0 Implement CLI argument parsing
  - [ ] 2.1 Create CLI struct with clap derive
  - [ ] 2.2 Add subcommands (run, interactive, config, version)
  - [ ] 2.3 Add options (provider, model, config, verbose, no-stream)
- [ ] 3.0 Build configuration system
  - [ ] 3.1 Create config file structure
  - [ ] 3.2 Implement config loading from JSON
  - [ ] 3.3 Implement environment variable substitution
  - [ ] 3.4 Create default config
- [ ] 4.0 Implement command handlers
  - [ ] 4.1 Implement single task run
  - [ ] 4.2 Integrate with core-agentic Orchestrator
  - [ ] 4.3 Handle streaming output
- [ ] 5.0 Build interactive mode
  - [ ] 5.1 Create interactive loop
  - [ ] 5.2 Add input reading (readline-style)
  - [ ] 5.3 Add exit command handling
- [ ] 6.0 Implement streaming output
  - [ ] 6.1 Create output formatter
  - [ ] 6.2 Add color support (ansi colors)
  - [ ] 6.3 Handle output categories (thought, tool, tool_output, system, error)
- [ ] 7.0 Implement confirmation UI
  - [ ] 7.1 Add confirmation prompt
  - [ ] 7.2 Handle user input (y/n/s/a/q)
  - [ ] 7.3 Integrate with core-agentic safety
- [ ] 8.0 Add error handling
  - [ ] 8.1 Handle provider errors
  - [ ] 8.2 Handle tool errors
  - [ ] 8.3 Add recovery logic
- [ ] 9.0 Testing
  - [ ] 9.1 Test CLI argument parsing
  - [ ] 9.2 Test config loading
  - [ ] 9.3 Test command execution