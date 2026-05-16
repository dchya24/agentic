# 🖥️ Plan: Agentic CLI Module

Rencana pengembangan untuk standalone Agentic CLI binary.

---

## 📁 Scope & File Boundaries

```
(Standalone binary — separate from Termul GUI)

agentic-cli/
├── Cargo.toml
├── src/
│   ├── main.rs           ← Entry point + command dispatch
│   ├── cli.rs            ← clap CLI argument parsing (v4)
│   ├── commands.rs       ← Command handlers (run, config, status, examples)
│   ├── config.rs         ← Local Config struct (legacy, used in tests)
│   ├── interactive.rs    ← Interactive mode (REPL) — minimal
│   ├── markdown.rs       ← Output rendering (markdown → colored terminal)
│   ├── confirmation.rs   ← Risk-level confirmation prompts
│   ├── error.rs          ← CommandError enum + suggestion system
│   ├── output.rs         ← Categorized output helpers (Thought/Tool/System/Error)
│   └── tests.rs          ← Unit tests (11 tests)
│   └── tui/
│       ├── mod.rs               # TUI module exports
│       ├── app.rs               # TUI app state + event loop + @ file dropdown + / command dropdown
│       ├── dropdown.rs          # Dropdown: recursive .gitignore-aware file listing (ignore crate)
│       ├── input.rs             # Input rendering with cursor + @ highlighting
│       ├── ui.rs                # Full UI layout: header, messages, progress, input, dropdown overlay
│       ├── markdown_widget.rs   # Markdown → ratatui styled lines
│       └── progress.rs          # Spinner + progress bar state
└── README.md
```

**Note:** The first `└── README.md` is in the root file listing.
> **Actual files** differ slightly from original plan — see `📦 CLI Binary Structure` below.

---

## ✅ Current Status — What's Already Done

| Feature | Status |
|---------|--------|
| Standalone CLI Binary | ✅ Working |
| Interactive Mode (REPL) | ✅ Working (reedline: history, Ctrl+R, `@` file completion, `/` command completion, 12+ slash commands) |
| Config File Support | ✅ Working |
| Environment Variables | ✅ Working |
| Safety Controls | ✅ Basic |
| Markdown Rendering | ✅ Working (headings, code blocks, lists, tables, links, blockquotes) |
| Streaming Output | ✅ Working |
| Config Commands (init, show, edit, validate, reset, path) | ✅ Working |
| Config Commands (backup, restore, export, import) | ✅ Working |
| Status Command | ✅ Working |
| Examples Command | ✅ Working |
| Error Types with Suggestions | ✅ Working |
| Retryable Error Detection | ✅ Working |
| Colored Output (termcolor) | ✅ Working |
| Confirmation Prompts (risk-level based) | ✅ Working |
| CLI Flags (verbose, debug, color) | ✅ Defined & wired up |
| Progress Indicator (spinner) | ✅ Working (indicatif) |
| Ctrl+C Graceful Shutdown | ✅ Working (tokio signal) |
| Lazy Orchestrator Init | ✅ Working (avoids panic on non-run commands) |
| Execution Time Display | ✅ Working |
| Conversation Save/Load | ✅ Working (JSON) |
| Session History | ✅ Working (timestamped) |
| `@` File Completion (recursive, .gitignore-aware) | ✅ Working (ignore crate) |
| TUI Mode (ratatui) | ✅ Working |
| Unit Tests | ✅ 31 tests passing |

---

## 🔴 Phase 1: CLI Polish & Stability (Week 1-2) — 3-4 days

### 1.1 Config Commands Enhancement
**Est:** 1-2 days

**Tasks:**
- [x] `agentic config init --interactive` — Full interactive wizard (dialoguer: Select, Input, Confirm)
- [x] `agentic config init --provider <name>` — Quick setup (openai, anthropic, zai, custom)
- [x] `agentic config show --format json|toml|table` — Multiple output formats (comfy-table for table)
- [x] `agentic config validate --verbose` — Detailed validation output
- [x] `agentic config backup` — Create config backup (timestamped, stored in `~/.config/agentic/backups/`)
- [x] `agentic config restore <file>` — Restore from backup (auto-backups current config)
- [x] `agentic config export` — Export to shareable format (API keys masked)
- [x] `agentic config import <file>` — Import from file (auto-backups current config)

**Command Design:**
```bash
# Quick start
agentic init                          # Interactive wizard
agentic init --provider openai        # Quick OpenAI setup
agentic init --provider anthropic     # Quick Anthropic setup

# Config management
agentic config show                   # Show current config
agentic config edit                   # Open in $EDITOR
agentic config validate               # Validate config
agentic config backup                 # Backup current config
agentic config restore backup.json    # Restore from backup

# Status
agentic status                        # Show provider/model/status
```

---

### 1.2 Error Handling & UX
**Est:** 1 day

**Tasks:**
- [x] Better error messages (human-readable) — `CommandError` enum with context
- [x] Suggestion on common errors ("Did you mean...?") — `suggest_command()` function
- [x] Progress indicators for long operations — indicatif spinner
- [x] Graceful shutdown (Ctrl+C handling) — tokio::signal::ctrl_c
- [x] Verbose mode (`--verbose` / `-v`) — wired to tracing log levels (warn→info→debug→trace)
- [x] Debug mode (`--debug`) — enables debug tracing with targets
- [x] Colored output with `--color=auto|always|never` — respected by all print helpers

---

### 1.3 Help & Documentation
**Est:** 1 day

**Tasks:**
- [x] Comprehensive `--help` text — all commands have `long_about`
- [ ] `agentic help <command>` detailed help
- [x] `agentic examples` — Show usage examples
- [ ] Man page generation (optional)
- [ ] Shell completion (bash, zsh, fish)

---

## 🟡 Phase 2: Interactive Mode Enhancement (Week 3-4) — 4-5 days

### 2.1 REPL Improvements
**File:** `src/interactive.rs` (536 lines)
**Est:** 2-3 days

**Tasks:**
- [ ] Multi-line input support (Shift+Enter for newline)
- [x] Input history persistence (across sessions) — `reedline` with file-backed history
- [x] History search (Ctrl+R) — `reedline` built-in
- [x] Tab completion for:
  - [x] Commands (/help, /config, /clear, etc.) — slash command completer
  - [x] File paths — recursive `.gitignore`-aware completion via `ignore` crate
  - [ ] Tool names
- [x] Upgrade to `reedline` for proper readline support
- [x] Basic REPL loop (stdin, help/clear/exit) — `interactive.rs`
- [x] Slash commands:
  - [x] `/help` — Show help (detailed, with tips)
  - [x] `/config` — Show current config (inline)
  - `/provider <name>` — Switch provider (placeholder, needs core support)
  - `/model <name>` — Switch model (placeholder, needs core support)
  - [x] `/clear` — Clear screen + conversation reset
  - [x] `/history` — Show conversation history with timestamps
  - [x] `/save <file>` — Export conversation to JSON
  - [x] `/load <file>` — Import conversation from JSON
  - [x] `/tools` — List available tools with params
  - [x] `/mcp` — Show MCP server status
  - [x] `/plan <goal>` — Create a plan
  - [x] `/quit` or Ctrl+D — Exit with session stats

---

### 2.2 Output Rendering Enhancement
**File:** `src/markdown.rs` + `src/commands.rs`
**Est:** 2 days

**Tasks:**
- [x] Better markdown rendering (tables, lists, headers) — `pulldown-cmark` with termcolor
- [ ] Code syntax highlighting (ansi colors) — needs `syntect` or `bat`
- [x] Streaming character-by-character display — via `print_chunk()`
- [ ] Thought/reasoning section (collapsible)
- [ ] Tool call display (collapsible)
- [ ] Token usage display per response
- [x] Execution time display — `print_response_stats()` in commands.rs
- [ ] Plan step progress visualization

**Design:**
```
┌─ 💬 Assistant ──────────────────────────────────┐
│                                                  │
│  Here's the plan to refactor the auth module:    │
│                                                  │
│  1. Read current auth files                      │
│  2. Analyze structure                            │
│  3. Design new architecture                      │
│  4. Implement changes                            │
│                                                  │
│  ┌─ 🔧 Tool: read_file ──────────────────────┐  │
│  │  Reading: src/auth/mod.rs                   │  │
│  │  Status: ✅ Done (234 lines)                │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  📊 Tokens: 1,234 in / 567 out | ⏱ 3.2s        │
└──────────────────────────────────────────────────┘
```

---

## 🟢 Phase 3: Advanced CLI Features (Month 2) — 7-10 days

### 3.1 Non-Interactive Mode (Piping)
**Est:** 2-3 days

**Tasks:**
- [ ] Pipe input: `cat file.rs | agentic "explain this code"`
- [ ] File argument: `agentic "fix bugs" --file src/main.rs`
- [ ] Batch mode: `agentic --batch tasks.jsonl`
- [ ] JSON output mode: `agentic "list files" --format json`
- [ ] Quiet mode: `agentic "do task" --quiet` (only output result)

**Design:**
```bash
# Piping
echo "Hello World" | agentic "translate to French"

# File analysis
agentic "find bugs" --file src/auth.rs

# Batch processing
agentic --batch tasks.jsonl --output results.json

# CI/CD integration
agentic "review PR changes" --format json --quiet
```

---

### 3.2 Session Management
**Est:** 2 days

**Tasks:**
- [ ] Save session: `agentic session save <name>`
- [ ] Load session: `agentic session load <name>`
- [ ] List sessions: `agentic session list`
- [ ] Resume last session: `agentic session resume`
- [ ] Session branching (fork conversation)

---

### 3.3 MCP Server Management
**Est:** 2-3 days

**Tasks:**
- [ ] `agentic mcp list` — List configured servers
- [ ] `agentic mcp add <name>` — Add MCP server interactively
- [ ] `agentic mcp remove <name>` — Remove server
- [ ] `agentic mcp test <name>` — Test connection
- [ ] `agentic mcp tools <name>` — List available tools
- [ ] `agentic mcp enable/disable <name>` — Toggle server
- [ ] MCP server templates (built-in)

---

### 3.4 Provider Management
**Est:** 2 days

**Tasks:**
- [ ] `agentic provider list` — List configured providers
- [ ] `agentic provider add` — Add provider interactively
- [ ] `agentic provider remove <name>` — Remove provider
- [ ] `agentic provider test <name>` — Test API key
- [ ] `agentic provider models <name>` — List available models
- [ ] `agentic provider default <name>` — Set default

---

## 🔵 Phase 4: Enterprise CLI (Month 3+) — 5-7 days

### 4.1 Remote Execution
**Est:** 2-3 days

- [ ] SSH remote execution
- [ ] Docker container execution
- [ ] WSL execution (from Windows)

### 4.2 Scripting API
**Est:** 2-3 days

- [ ] Lua/WASM scripting for custom tools
- [ ] Hook system (pre/post execution)
- [ ] Custom command registration

### 4.3 Plugin System
**Est:** 2-3 days

- [ ] Plugin discovery
- [ ] Plugin installation
- [ ] Plugin sandboxing
- [ ] Plugin API

---

## 🧪 Testing

Testing untuk CLI module diatur di **[PLAN_TESTING.md](./PLAN_TESTING.md)**:
- Phase 2.2: Integration tests (config lifecycle, REPL session, pipe mode)
- CLI quality: shell completion, man page, exit codes

---

## 📦 CLI Binary Structure (Actual)

```
agentic-cli/
├── Cargo.toml                    # Depends on core-agentic
├── src/
│   ├── main.rs                   # Entry point + command dispatch (157 lines)
│   ├── cli.rs                    # clap CLI definitions (211 lines)
│   ├── commands.rs               # Command handlers: run, config, status, examples (1268 lines)
│   ├── config.rs                 # Local Config struct (legacy, used in tests) (138 lines)
│   ├── interactive.rs            # REPL with reedline: history, completion, @ file popup, slash commands (~1100 lines)
│   ├── markdown.rs               # Markdown → colored terminal rendering (216 lines)
│   ├── confirmation.rs           # Risk-level confirmation prompts (62 lines)
│   ├── error.rs                  # CommandError enum + suggestion system (105 lines)
│   ├── output.rs                 # Categorized output helpers (81 lines)
│   ├── tests.rs                  # Unit tests (105 lines)
│   └── tui/
│       ├── mod.rs               # TUI module exports
│       ├── app.rs               # TUI app state + event loop + @ dropdown
│       ├── dropdown.rs          # Dropdown: recursive .gitignore-aware file listing
│       ├── input.rs             # Input rendering with cursor + @ highlighting
│       ├── ui.rs                # Full UI layout: header, messages, progress, input
│       ├── markdown_widget.rs   # Markdown → ratatui styled lines
│       └── progress.rs          # Spinner + progress bar state
└── README.md
```

**Actual Dependencies (Cargo.toml):**
```toml
[dependencies]
core-agentic = { path = "../core-agentic" }
clap = { version = "4.5", features = ["derive", "string"] }
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
pulldown-cmark = "0.10"          # Markdown parsing
termcolor = "1.4"                # Colored output
indicatif = "0.17"               # Progress bars
dialoguer = { version = "0.11", features = ["fuzzy-select"] }
comfy-table = "7"                # Table rendering
chrono = "0.4"                   # Timestamps for backups
console = "0.15"                 # Terminal utilities
dirs = "5.0"                     # Home directory
nu-ansi-term = "0.50"           # ANSI colors for reedline
reedline = "0.47"               # Readline library (replaces rustyline)
uuid = { version = "1", features = ["v4"] }
ratatui = "0.29"                # TUI framework
crossterm = "0.28"               # Terminal backend for ratatui
tui-textarea = "0.7"             # Multi-line textarea widget
syntect = "5.2"                  # Syntax highlighting
ignore = "0.4"                   # .gitignore-aware file walking (ripgrep ecosystem)
```

---

## 🔗 Relationship with Other Modules

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  Termul UI  │────▶│  Tauri APIs  │────▶│ core-agentic│
│  (React)    │     │  (Rust)      │     │  (Rust lib) │
└─────────────┘     └──────────────┘     └──────┬──────┘
                                                 │
                                                 │ shared library
                                                 │
                    ┌──────────────┐     ┌──────┴──────┐
                    │  Agentic CLI │────▶│ core-agentic│
                    │  (Binary)    │     │  (Rust lib) │
                    └──────────────┘     └─────────────┘
```

- **Core Agentic** = shared Rust library (business logic)
- **Termul UI** = uses core via Tauri commands
- **Agentic CLI** = uses core directly as Rust dependency

---

**Last Updated:** May 15, 2026 (Phase 2 complete + @ file completion rewritten)
