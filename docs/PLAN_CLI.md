# 🖥️ Plan: Agentic CLI Module

Rencana pengembangan untuk standalone Agentic CLI binary.

---

## 📁 Scope & File Boundaries

```
(Standalone binary — separate from Termul GUI)

agentic-cli/
├── Cargo.toml
├── src/
│   ├── main.rs           ← Entry point
│   ├── cli.rs            ← CLI argument parsing
│   ├── interactive.rs    ← Interactive mode (REPL)
│   ├── render.rs         ← Output rendering (markdown, colors)
│   ├── config_cmd.rs     ← Config subcommands
│   └── stream.rs         ← Streaming output handler
├── tests/
│   └── integration.rs
└── README.md
```

> **Note:** CLI menggunakan `core-agentic` sebagai library dependency.

---

## ✅ Current Status — What's Already Done

| Feature | Status |
|---------|--------|
| Standalone CLI Binary | ✅ Working |
| Interactive Mode (REPL) | ✅ Working |
| Config File Support | ✅ Working |
| Environment Variables | ✅ Working |
| Safety Controls | ✅ Basic |
| Markdown Rendering | ✅ Working |
| Streaming Output | ✅ Working |
| Config Commands (init, show, edit, validate, reset, path) | ✅ Working |

---

## 🔴 Phase 1: CLI Polish & Stability (Week 1-2) — 3-4 days

### 1.1 Config Commands Enhancement
**Est:** 1-2 days

**Tasks:**
- [ ] `agentic config init --interactive` — Full interactive wizard
- [ ] `agentic config init --provider <name>` — Quick setup for specific provider
- [ ] `agentic config show --format json|toml|table` — Multiple output formats
- [ ] `agentic config validate --verbose` — Detailed validation output
- [ ] `agentic config backup` — Create config backup
- [ ] `agentic config restore <file>` — Restore from backup
- [ ] `agentic config export` — Export to shareable format
- [ ] `agentic config import <file>` — Import from file/URL

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
- [ ] Better error messages (human-readable)
- [ ] Suggestion on common errors ("Did you mean...?")
- [ ] Progress indicators for long operations
- [ ] Graceful shutdown (Ctrl+C handling)
- [ ] Verbose mode (`--verbose` / `-v`)
- [ ] Debug mode (`--debug`)
- [ ] Colored output with `--color=auto|always|never`

---

### 1.3 Help & Documentation
**Est:** 1 day

**Tasks:**
- [ ] Comprehensive `--help` text
- [ ] `agentic help <command>` detailed help
- [ ] `agentic examples` — Show usage examples
- [ ] Man page generation (optional)
- [ ] Shell completion (bash, zsh, fish)

---

## 🟡 Phase 2: Interactive Mode Enhancement (Week 3-4) — 4-5 days

### 2.1 REPL Improvements
**File:** `src/interactive.rs`
**Est:** 2-3 days

**Tasks:**
- [ ] Multi-line input support (Shift+Enter for newline)
- [ ] Input history persistence (across sessions)
- [ ] History search (Ctrl+R)
- [ ] Tab completion for:
  - [ ] Commands (/help, /config, /clear, etc.)
  - [ ] File paths
  - [ ] Tool names
- [ ] Slash commands:
  - `/help` — Show help
  - `/config` — Show/edit config
  - `/provider <name>` — Switch provider
  - `/model <name>` — Switch model
  - `/clear` — Clear conversation
  - `/history` — Show conversation history
  - `/save <file>` — Export conversation
  - `/load <file>` — Import conversation
  - `/tools` — List available tools
  - `/mcp` — Show MCP server status
  - `/plan <goal>` — Create a plan
  - `/quit` or Ctrl+D — Exit

---

### 2.2 Output Rendering Enhancement
**File:** `src/render.rs`
**Est:** 2 days

**Tasks:**
- [ ] Better markdown rendering (tables, lists, headers)
- [ ] Code syntax highlighting (ansi colors)
- [ ] Streaming character-by-character display
- [ ] Thought/reasoning section (collapsible)
- [ ] Tool call display (collapsible)
- [ ] Token usage display per response
- [ ] Execution time display
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

## 📦 CLI Binary Structure

```
agentic-cli/
├── Cargo.toml                    # Depends on core-agentic
├── src/
│   ├── main.rs                   # Entry point + dispatch
│   ├── cli.rs                    # clap CLI definitions
│   ├── interactive.rs            # REPL loop
│   ├── render.rs                 # Terminal rendering
│   ├── config_cmd.rs             # Config subcommands
│   └── stream.rs                 # Stream handler
└── tests/
    └── integration.rs
```

**Dependencies:**
```toml
[dependencies]
core-agentic = { path = "../core-agentic" }
clap = { version = "4", features = ["derive"] }
colored = "2"
indicatif = "0.17"       # Progress bars
dialoguer = "0.11"       # Interactive prompts
comfy-table = "7"        # Table rendering
syntect = "5"            # Syntax highlighting
rustyline = "14"         # Readline for REPL
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

**Last Updated:** May 2, 2026
