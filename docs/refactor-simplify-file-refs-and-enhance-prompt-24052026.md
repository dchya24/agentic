# Simplify File Refs & Enhance Prompt UI

**Date:** 2026-05-24

## Summary

Refactored file reference expansion to emit lightweight XML tags instead of full file contents, enhanced the interactive prompt with contextual info, improved OpenAI stream error handling, and fixed tool role message serialization in the orchestrator.

## Changes

### `agentic-cli/src/file_ref.rs` — Simplified file reference expansion
- Replaced verbose `<file>` and `<directory>` tags (with full content/listing) with minimal empty-element tags:
  - `<file path="..." />` instead of embedding entire file contents
  - `<directory path="..." />` instead of recursive listing
  - `<path="..." /> [Not found]` for missing files
- Removed `read_single_file()` and `read_directory()` functions (dropped `ignore` crate dependency for directory walking)
- Tests updated to match new minimal output format

### `agentic-cli/src/interactive.rs` — Enhanced prompt UI
- Added git branch detection and display in prompt indicator (green text)
- Added model name (yellow) and provider (gray) display in prompt
- Changed prompt layout to multi-line box format:
  ```
  ╭── dirname main -- model (provider)
  ╰─ >
  ```
  with continuation lines using `│   `
- Fixed file path suggestions: `value` now includes `@` prefix (`format!("@{}", display)`)
- Removed left prompt text (previously showed `agentic> `)
- `AgenticPrompt` now takes `ModelInfo` to display active model/provider

### `agentic-cli/src/commands.rs` — Removed verbose logging
- Removed "Expanded file references in prompt" log line
- Removed "Running task: ..." log line (task output now only shown via spinner)

### `core-agentic/src/providers/openai.rs` — Improved stream error handling
- Changed from `error_for_status()` (which drops body) to manual status check
- Now reads response body text and includes it in error messages
- Logs the full HTTP status and body at error level

### `core-agentic/src/orchestrator.rs` — Fixed tool role message handling
- `MessageRole::Tool { tool_call_id }` now properly serializes `tool_call_id` into `ChatMessageRequest`
- Previously all roles mapped to `tool_call_id: None`; now Tool messages carry their call ID
