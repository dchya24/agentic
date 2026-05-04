# 🧪 Plan: Testing Strategy

Rencana testing komprehensif untuk seluruh project Termul — unit, integration, dan E2E.

---

## 📁 Scope & Test Boundaries

```
core-agentic/
├── src/
│   ├── safety.rs          ← unit tests inline
│   ├── memory.rs          ← unit tests inline
│   ├── planner.rs         ← unit tests inline
│   ├── tool_registry.rs   ← unit tests inline
│   └── providers/
│       ├── openai.rs      ← unit tests inline (mock)
│       └── anthropic.rs   ← unit tests inline (mock)
└── tests/
    ├── agent_loop.rs      ← integration: full agent loop with mock LLM
    ├── planner_flow.rs    ← integration: plan → execute → replan
    ├── memory_persist.rs  ← integration: session save/restore
    └── mcp_client.rs      ← integration: MCP server lifecycle

agentic-cli/
└── tests/
    ├── cli_parsing.rs     ← integration: argument parsing
    ├── config_lifecycle.rs← integration: init → edit → validate → reset
    ├── interactive.rs     ← integration: REPL session (mock LLM)
    └── pipe_mode.rs       ← integration: stdin pipe & batch

src-tauri/  (Tauri backend)
└── tests/
    ├── agentic_commands.rs← integration: Tauri command handlers
    ├── config_persist.rs  ← integration: file config read/write
    └── pty_tool.rs        ← integration: PTY-based tool execution

src/renderer/  (React frontend)
├── components/agentic/
│   ├── __tests__/
│   │   ├── AgenticPanel.test.tsx
│   │   ├── AgenticOutput.test.tsx
│   │   ├── AgenticInput.test.tsx
│   │   ├── FirstRunBanner.test.tsx
│   │   └── ProviderManager.test.tsx   (Phase 3)
│   └── ...
├── stores/
│   └── __tests__/
│       └── agentic-store.test.ts
└── e2e/
    ├── first-run.spec.ts
    ├── agentic-chat.spec.ts
    ├── config-setup.spec.ts
    └── provider-mgmt.spec.ts          (Phase 3)
```

---

## 🏗️ Testing Architecture

```
┌─────────────────────────────────────────────────────┐
│                    E2E Tests                         │
│   (Playwright / Tauri WebDriver)                     │
│   Full user flows across UI ↔ Backend ↔ Core        │
├─────────────────────────────────────────────────────┤
│              Integration Tests                        │
│   Core:   agent loop, planner flow, MCP lifecycle    │
│   CLI:    config lifecycle, REPL session, pipe mode   │
│   Tauri:  command handlers, config persistence        │
├─────────────────────────────────────────────────────┤
│                Unit Tests                             │
│   Core:   safety scoring, memory, tools, providers   │
│   CLI:    arg parsing, rendering, input handling      │
│   UI:     component rendering, store actions          │
└─────────────────────────────────────────────────────┘
```

---

## 🔴 Phase 1: Foundation (Week 1-2) — 3-4 days

> **Goal:** Testing infrastructure + critical path unit tests

### 1.1 Test Infrastructure Setup
**Est:** 1 day

**Tasks:**
- [ ] Configure Vitest for React components (unit + component tests)
- [ ] Setup Testing Library + user-event for UI tests
- [ ] Add Rust test utilities / fixtures in `core-agentic/tests/common/`
- [ ] Create mock LLM provider for core tests
- [ ] Setup Playwright (or Tauri WebDriver) for E2E
- [ ] Add test scripts to `package.json` and `Makefile`

**Test Scripts:**
```json
{
  "test": "vitest run",
  "test:watch": "vitest",
  "test:coverage": "vitest run --coverage",
  "test:e2e": "playwright test",
  "test:core": "cd core-agentic && cargo test",
  "test:cli": "cd agentic-cli && cargo test",
  "test:all": "npm run test:core && npm run test:cli && npm run test && npm run test:e2e"
}
```

---

### 1.2 Core Unit Tests
**Est:** 1-2 days

#### Safety System (`core-agentic/src/safety.rs`)
- [ ] Risk scoring accuracy: low commands → `RiskLevel::Low`
- [ ] Risk scoring accuracy: medium commands → `RiskLevel::Medium`
- [ ] Risk scoring accuracy: high commands → `RiskLevel::High`
- [ ] Edge cases: empty string, very long commands, unicode
- [ ] Blocklist matching (exact + pattern)
- [ ] Configurable thresholds
- [ ] Rate limiting logic

#### Memory System (`core-agentic/src/memory.rs`)
- [ ] Add/remove messages
- [ ] Sliding window compaction
- [ ] Context budget tracking
- [ ] Message pinning
- [ ] Session isolation
- [ ] Disk persistence save/load

#### Tool System (`core-agentic/src/tool_registry.rs` + `tools/`)
- [ ] Tool registration & lookup
- [ ] Tool execution with valid args
- [ ] Tool execution with invalid args → error
- [ ] Tool timeout handling
- [ ] Output size limit enforcement
- [ ] Permission level checks

#### Providers (`core-agentic/src/providers/`)
- [ ] OpenAI: request formatting
- [ ] OpenAI: response parsing
- [ ] OpenAI: streaming chunk parsing
- [ ] OpenAI: error handling (rate limit, auth, network)
- [ ] Anthropic: request formatting
- [ ] Anthropic: response parsing
- [ ] Anthropic: streaming chunk parsing
- [ ] Anthropic: error handling
- [ ] Provider health check

---

### 1.3 Frontend Unit Tests
**Est:** 1 day

#### AgenticPanel
- [ ] Renders correctly with default props
- [ ] Shows input area
- [ ] Shows empty state when no messages

#### AgenticOutput
- [ ] Renders text messages
- [ ] Renders code blocks with copy button
- [ ] Renders error messages
- [ ] Shows loading indicator

#### AgenticInput
- [ ] Sends message on Enter
- [ ] Does not send empty message
- [ ] Clears input after send
- [ ] Handles multi-line input

#### FirstRunBanner
- [ ] Shows when no config exists
- [ ] Hides when config is valid
- [ ] Quick Setup button triggers action
- [ ] Setup Wizard button triggers action

#### Agentic Store (`agentic-store.test.ts`)
- [ ] Initial state
- [ ] `sendMessage` action
- [ ] `addMessage` action
- [ ] `clearMessages` action
- [ ] `setConfig` action
- [ ] `setStreaming` state toggle

---

## 🟡 Phase 2: Integration Tests (Week 3-4) — 4-5 days

> **Goal:** Cross-module flows work correctly

### 2.1 Core Integration Tests
**Est:** 2 days

#### Agent Loop (`core-agentic/tests/agent_loop.rs`)
- [ ] Full loop: user message → LLM response → done
- [ ] Full loop with tool call: message → LLM → tool → LLM → done
- [ ] Multi-turn conversation
- [ ] Safety block: dangerous command → blocked event
- [ ] Safety confirmation: medium risk → confirmation event
- [ ] Memory compaction triggers when context too long
- [ ] Streaming: chunks emitted in order
- [ ] Error recovery: LLM error → retry or graceful failure

#### Planner Flow (`core-agentic/tests/planner_flow.rs`)
- [ ] Create plan from goal description
- [ ] Execute plan step by step
- [ ] Replan on step failure
- [ ] Plan approval flow (emit → approve → continue)
- [ ] Plan with dependencies (step B waits for step A)

#### MCP Client (`core-agentic/tests/mcp_client.rs`)
- [ ] Connect to MCP server (stdio)
- [ ] List tools from server
- [ ] Execute MCP tool via adapter
- [ ] Handle server disconnect
- [ ] Multiple MCP servers simultaneously

---

### 2.2 CLI Integration Tests
**Est:** 1 day

#### Config Lifecycle (`agentic-cli/tests/config_lifecycle.rs`)
- [ ] `init` creates valid config file
- [ ] `init --interactive` creates with user input
- [ ] `show` displays current config
- [ ] `edit` opens editor
- [ ] `validate` reports valid config
- [ ] `validate` reports invalid config with details
- [ ] `reset` restores defaults
- [ ] `backup` creates backup file
- [ ] `restore` restores from backup

#### Interactive Session (`agentic-cli/tests/interactive.rs`)
- [ ] Start REPL → send message → receive response (mock LLM)
- [ ] Slash commands: `/help`, `/clear`, `/config`, `/quit`
- [ ] History navigation (↑/↓)
- [ ] Graceful Ctrl+C / Ctrl+D handling

#### Pipe Mode (`agentic-cli/tests/pipe_mode.rs`)
- [ ] Pipe stdin: `echo "hello" | agentic "translate"`
- [ ] JSON output mode: `agentic "list" --format json`
- [ ] Quiet mode: only final output

---

### 2.3 Tauri Backend Integration Tests
**Est:** 1-2 days

#### Agentic Commands (`src-tauri/tests/agentic_commands.rs`)
- [ ] `agentic_load_config` loads valid config
- [ ] `agentic_load_config` handles invalid config
- [ ] `agentic_save_config` persists to file
- [ ] `agentic_chat_stream` emits events in order
- [ ] `agentic_get_status` returns correct state
- [ ] `agentic_confirm_action` approves/rejects

#### Config Persistence (`src-tauri/tests/config_persist.rs`)
- [ ] Load from `~/.config/agentic/config.json`
- [ ] Parse both flat (native) and nested (CLI) formats
- [ ] Save creates file if not exists
- [ ] Save preserves non-agentic fields
- [ ] Corrupted file → graceful error

---

## 🟢 Phase 3: E2E Tests (Month 2) — 3-5 days

> **Goal:** Full user journey tests via browser automation

### 3.1 Setup E2E Framework
**Est:** 1 day

**Tasks:**
- [ ] Install & configure Playwright (or Tauri WebDriver)
- [ ] Create test helpers (login, navigate, wait for response)
- [ ] Mock Tauri IPC for isolated frontend E2E
- [ ] Setup CI job for E2E tests

---

### 3.2 User Flow Tests
**Est:** 2-3 days

#### First-Run Wizard Flow (`e2e/first-run.spec.ts`)
- [ ] Launch app with no config → banner appears
- [ ] Click "Quick Setup" → default config created → banner disappears
- [ ] Click "Setup Wizard" → wizard opens → complete all steps → config saved
- [ ] Relaunch app → banner does not appear

#### Agentic Chat Flow (`e2e/agentic-chat.spec.ts`)
- [ ] Open agentic panel
- [ ] Type message → send → streaming response appears
- [ ] Code block renders with syntax highlighting
- [ ] Copy button on code block works
- [ ] Tool call section shows (collapsible)
- [ ] Token usage updates after response
- [ ] Error message displays on failure
- [ ] Clear chat history works

#### Config Setup Flow (`e2e/config-setup.spec.ts`)
- [ ] Open Settings → Agentic AI section visible
- [ ] Config status shows (valid/invalid)
- [ ] Click "Configure" → wizard opens
- [ ] Click "Edit Config" → editor opens
- [ ] Click "Validate" → validation result shows
- [ ] Click "Reset" → confirmation → defaults restored

#### Provider Management Flow (`e2e/provider-mgmt.spec.ts`)
- [ ] Open Provider Manager
- [ ] Add new provider → fill details → save → appears in list
- [ ] Edit provider → change model → save → updated
- [ ] Test provider → success indicator
- [ ] Delete provider → confirmation → removed
- [ ] Switch default provider → persists

---

## 🔵 Phase 4: Advanced Testing (Month 3+) — 3-5 days

> **Goal:** Performance, security, chaos testing

### 4.1 Performance Tests
**Est:** 1-2 days

- [ ] Startup time benchmark (core + UI)
- [ ] Large file operation performance
- [ ] Terminal rendering performance (xterm.js)
- [ ] Memory usage under long conversations (1000+ messages)
- [ ] Streaming throughput benchmark
- [ ] Bundle size tracking over time

### 4.2 Security Audit Tests
**Est:** 1-2 days

- [ ] API key handling (never logged, never in plaintext storage)
- [ ] File access permission tests (sandbox boundaries)
- [ ] Command injection prevention
- [ ] MCP communication security
- [ ] Rate limiting effectiveness
- [ ] Config file permission checks

### 4.3 Chaos / Resilience Tests
**Est:** 1 day

- [ ] LLM provider timeout → graceful degradation
- [ ] Network disconnect during streaming → recovery
- [ ] MCP server crash → auto-reconnect or graceful disable
- [ ] Corrupted config file → fallback to defaults
- [ ] Disk full during memory persistence → error handling
- [ ] Very large tool output → truncation works

---

## 📊 Coverage Targets

| Module | Unit | Integration | E2E | Target Coverage |
|--------|------|-------------|-----|-----------------|
| **core-agentic** | ✅ Phase 1 | ✅ Phase 2 | — | ≥ 80% |
| **agentic-cli** | ✅ Phase 1 | ✅ Phase 2 | — | ≥ 75% |
| **Tauri backend** | — | ✅ Phase 2 | — | ≥ 70% |
| **React frontend** | ✅ Phase 1 | — | ✅ Phase 3 | ≥ 70% |
| **E2E flows** | — | — | ✅ Phase 3 | Key flows covered |

---

## 📦 Testing Dependencies

### Rust (core-agentic / agentic-cli)
| Crate | Purpose | Phase |
|-------|---------|-------|
| `tokio` (test) | Async test runtime | ✅ Existing |
| `mockall` | Mock LLM provider | Phase 1 |
| `tempfile` | Temp dirs for config tests | Phase 1 |
| `assert_cmd` | CLI process assertions | Phase 1 |
| `predicates` | CLI output assertions | Phase 1 |

### Frontend (React)
| Package | Purpose | Phase |
|---------|---------|-------|
| `vitest` | Test runner | ✅ Existing |
| `@testing-library/react` | Component testing | Phase 1 |
| `@testing-library/user-event` | User interaction simulation | Phase 1 |
| `msw` | Mock Service Worker (API mocking) | Phase 1 |
| `playwright` | E2E browser automation | Phase 3 |

---

## 🔗 Relationship with Feature Plans

Testing dibangun **paralel** dengan feature development, bukan setelahnya.

| Feature Plan | Testing Phase | Keterkaitan |
|-------------|---------------|-------------|
| PLAN_CORE Phase 1 (Safety, Memory) | Testing Phase 1.2 (Core Unit) | Unit tests untuk setiap fitur baru |
| PLAN_UI Phase 1 (Integration) | Testing Phase 1.3 (Frontend Unit) | Komponen test saat integrasi |
| PLAN_CLI Phase 1 (Polish) | Testing Phase 2.2 (CLI Integration) | Config lifecycle tests |
| PLAN_CORE Phase 2 (Planner, Providers) | Testing Phase 2.1 (Core Integration) | Planner flow, provider mock |
| PLAN_UI Phase 3 (Management UIs) | Testing Phase 3.2 (E2E Flows) | Full user journey |
| PLAN_CORE Phase 3 (Multi-Agent, Vector) | Testing Phase 4 (Advanced) | Performance + resilience |

**Rule of thumb:** Setiap feature PR harus menyertakan minimal unit tests.

---

## 🚦 Priority Legend

| Priority | Timeline |
|----------|----------|
| 🔴 Critical | Week 1-2 (bersama feature integration) |
| 🟡 High | Week 3-4 (bersama feature polish) |
| 🟢 Medium | Month 2 (bersama advanced features) |
| 🔵 Low | Month 3+ |

---

**Last Updated:** May 4, 2026
