# Agentic CLI

AI agent orchestration command-line interface using [core-agentic](https://github.com/nutec/termul/tree/main/core-agentic).

## Installation

```bash
cargo install --path agentic-cli
```

Or build manually:

```bash
cd agentic-cli
cargo build --release
# Binary at target/release/agentic.exe
```

## Quick Start

```bash
# Set API key
export OPENAI_API_KEY="sk-your-key"

# Run a task
agentic run "list files in current directory"

# Interactive REPL with @ file completion
agentic interactive

# TUI mode
agentic tui
```

## Usage

### Commands

| Command | Description |
|---------|------------|
| `agentic run <task>` | Run a single task |
| `agentic interactive` | Start interactive REPL mode |
| `agentic tui` | Start full TUI mode |
| `agentic config show` | Display current configuration |
| `agentic config init --interactive` | Guided config wizard |
| `agentic version` | Show version |

### Options

| Option | Description |
|--------|------------|
| `-c, --config <PATH>` | Custom config file |
| `-v, --verbose <LEVEL>` | Verbose output (error/warn/info/debug/trace) |
| `--debug` | Enable debug tracing |
| `--color <auto|always|never>` | Color output control |

## Configuration

### Environment Variables

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
```

### Config File

Location: `~/.config/agentic/config.json` or custom via `--config`

```json
{
  "provider": {
    "type": "openai-compatible",
    "base_url": "$OPENAI_BASE_URL",
    "api_key": "$OPENAI_API_KEY"
  },
  "model": {
    "id": "gpt-4o",
    "temperature": 0.7,
    "max_tokens": 4096
  },
  "safety": {
    "auto_approve_low_risk": true,
    "blocked_commands": ["rm -rf /", "mkfs", "dd if="]
  },
  "output": {
    "color": true,
    "stream": true,
    "show_thoughts": true,
    "show_tool_calls": true
  }
}
```

### Environment Variable Substitution

Use `$VAR` in config to reference environment variables:

```json
{
  "provider": {
    "api_key": "$OPENAI_API_KEY"
  }
}
```

## Interactive Mode

The REPL uses [reedline](https://github.com/nushell/reedline) with rich features:

- **`@` file completion** — type `@` to see all project files recursively
  - Respects `.gitignore` (node_modules, target, .git, etc. excluded automatically)
  - Type `@src/` to browse files under `src/`
  - Type `@chat` to search files matching "chat"
- **`/` command completion** — type `/` to see all slash commands with descriptions
- **Inline hints** — fish-style autocomplete suggestions
- **History** — file-backed history with Ctrl+R search
- **Syntax highlighting** — `/` commands in yellow, `@` file refs in blue

```
> agentic interactive
╔══════════════════════════════════════════════════╗
║            🤖 Agentic Interactive Mode           ║
╠══════════════════════════════════════════════════╣
║  📂 /home/user/project                          ║
║  ⚡ Provider: openai / Model: gpt-4o             ║
╠══════════════════════════════════════════════════╣
║  /help    Show commands                          ║
║  /quit    Exit (Ctrl+D)                          ║
╚══════════════════════════════════════════════════╝

project agentic> @src/components/  ← popup with files
project agentic> /help              ← popup with commands
project agentic> list all rust files
project agentic> exit
```

## Available Tools

When running tasks, the agent has access to:

- **run_command** - Execute shell commands
- **read_file** - Read file contents
- **write_file** - Write content to files
- **list_files** - List directory contents

## Examples

```bash
# Simple task
agentic run "hello world"

# File operations
agentic run "create a hello.txt file with 'hello world'"

# List files
agentic run "show me all files in src directory"

# Interactive
agentic interactive

# Debug mode
agentic run "list files" -v trace
```

## Building

Requirements:
- Rust 1.70+
- Cargo

```bash
# Development
cargo build

# Release (optimized)
cargo build --release

# Run tests
cargo test
```

## License

MIT