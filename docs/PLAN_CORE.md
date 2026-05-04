# 🧠 Plan: Agentic Core Module

Rencana pengembangan untuk core engine agentic AI (Rust library).

---

## 📁 Scope & File Boundaries

```
core-agentic/src/
├── lib.rs               ← Public API
├── agent.rs             ← Agent loop
├── orchestrator.rs      ← Orchestration engine
├── config.rs            ← Configuration
├── events.rs            ← Event system
├── safety.rs            ← Safety system
├── memory.rs            ← Memory management
├── tool.rs              ← Tool trait
├── tool_registry.rs     ← Tool registry
├── providers/
│   ├── mod.rs           ← Provider trait
│   ├── openai.rs        ← OpenAI provider
│   └── anthropic.rs     ← Anthropic provider
├── mcp/
│   ├── mod.rs
│   ├── client.rs        ← MCP client
│   ├── tool_adapter.rs  ← MCP→Tool adapter
│   ├── transport.rs     ← stdio + HTTP
│   └── types.rs         ← MCP types
├── tools/
│   ├── mod.rs
│   ├── run_command.rs
│   ├── read_file.rs
│   ├── write_file.rs
│   ├── edit_file.rs
│   ├── list_files.rs
│   ├── glob.rs
│   └── grep.rs
└── planner.rs           (NEW) ← Planner agent
```

---

## ✅ Current Status — What's Already Done

| Feature | File | Status |
|---------|------|--------|
| Agent Loop | `agent.rs` | ✅ Working |
| Orchestrator | `orchestrator.rs` | ✅ Working |
| Config System | `config.rs` | ✅ Working |
| Event System | `events.rs` | ✅ Working |
| Safety System | `safety.rs` | ✅ Enhanced (risk scoring, patterns, sandbox, rate limit, audit) |
| Memory System | `memory.rs` | ✅ Enhanced (sliding window, pinning, sessions, persistence) |
| Tool System | `tool.rs`, `tool_registry.rs` | ✅ Working |
| OpenAI Provider | `providers/openai.rs` | ✅ Working |
| Anthropic Provider | `providers/anthropic.rs` | ✅ Working |
| MCP Client | `mcp/` | ✅ Working |
| Planner Agent | `planner.rs` | ✅ Working |
| 7 Builtin Tools | `tools/` | ✅ Working |

---

## 🔴 Phase 1: Core Stability (Week 1-2) — 3-5 days

### 1.1 Safety System Enhancement
**File:** `core-agentic/src/safety.rs`
**Est:** 2-3 days
**Status:** ✅ **Completed** (May 4, 2026)

**Tasks:**
- [x] Command risk scoring algorithm (0.0 - 1.0)
- [x] Risk categories: low (auto-approve), medium (confirm), high (block)
- [x] Configurable risk thresholds
- [x] Pattern-based risk detection (25+ regex patterns)
- [x] Command blocklist (configurable)
- [x] Path restriction (sandbox boundaries)
- [x] Rate limiting per tool
- [x] Audit logging (ring buffer)
- [x] Full backward compatibility with existing code

**Risk Scoring Design:**
```rust
pub enum RiskLevel {
    Low,      // 0.0 - 0.3 → auto-approve
    Medium,   // 0.3 - 0.7 → requires confirmation
    High,     // 0.7 - 1.0 → blocked
}

pub fn score_command(cmd: &str) -> RiskLevel {
    // Pattern matching:
    // "ls", "cat", "head" → Low
    // "rm", "mv", "git reset" → Medium
    // "rm -rf /", "mkfs", "dd" → High
}
```

---

### 1.2 Memory System Enhancement
**File:** `core-agentic/src/memory.rs`
**Est:** 2 days
**Status:** ✅ **Completed** (May 4, 2026)

**Tasks:**
- [x] Sliding window context management (token-based)
- [x] Message summarization (compact old messages)
- [x] Context budget tracking (token counting)
- [x] Important message pinning
- [x] Session-based memory isolation
- [x] Memory persistence to disk
- [x] Message search (keyword + role)
- [x] Message metadata (model, duration, tokens)
- [x] Auto-persist option
- [x] Full backward compatibility with existing code

**Design:**
```rust
pub struct MemoryManager {
    max_context_tokens: usize,
    messages: Vec<Message>,
    summary: Option<String>,
    pinned_ids: HashSet<String>,
}

impl MemoryManager {
    pub fn add_message(&mut self, msg: Message) -> ContextWindow;
    pub fn compact(&mut self) -> SummarizedContext;
    pub fn get_context(&self) -> Vec<Message>;
    pub fn search(&self, query: &str) -> Vec<&Message>;
}
```

---

### 1.3 MCP Client
**File:** `core-agentic/src/mcp/`
**Est:** 3-5 days
**Status:** ✅ **Completed** (May 4, 2026)

**Tasks:**
- [x] MCP protocol types (JSON-RPC 2.0)
- [x] Transport trait + StdioTransport (child process)
- [x] Transport trait + HttpTransport (reqwest blocking)
- [x] MCP client (connect, initialize, discover tools, call tools)
- [x] Tool adapter (wraps remote MCP tools as local `Tool` trait)
- [x] Config integration (`mcp_servers` field in `Config`)
- [x] ToolRegistry helper (`register_mcp_server()`)
- [x] Unit tests (types, serialization, config roundtrip)

**Remaining improvements (deferred to Phase 2):**
- [ ] Async transport (replace `reqwest::blocking` with tokio-based)
- [ ] SSE streaming transport support
- [ ] Auto-reconnection on server disconnect
- [ ] Integration test against real MCP server

---

## 🟡 Phase 2: Advanced Features (Week 3-6) — 10-15 days

### 2.1 Planner Agent
**File:** `core-agentic/src/planner.rs`
**Est:** 5-7 days
**Status:** ✅ **Completed** (May 4, 2026)

**Tasks:**
- [x] Design planner architecture
- [x] Task decomposition algorithm (LLM-based + manual)
- [x] Step planning (ordered + dependencies)
- [x] Plan execution tracking
- [x] Re-planning on failure (configurable max attempts)
- [x] Plan approval flow (event + callback)
- [x] Plan step status management
- [ ] Integration with orchestrator (deferred)

**Architecture:**
```rust
pub struct Plan {
    id: String,
    goal: String,
    steps: Vec<Step>,
    status: PlanStatus,
}

pub struct Step {
    id: String,
    description: String,
    tool: Option<String>,
    args: Option<serde_json::Value>,
    status: StepStatus,
    result: Option<String>,
    depends_on: Vec<String>,
}

pub enum PlanStatus {
    Draft,
    PendingApproval,
    Executing,
    Completed,
    Failed,
}

pub struct PlannerAgent {
    llm: Box<dyn LlmProvider>,
    max_steps: usize,
}

impl PlannerAgent {
    pub async fn create_plan(&self, goal: &str) -> Result<Plan>;
    pub async fn execute_plan(&self, plan: &mut Plan) -> Result<PlanResult>;
    pub async fn replan(&self, plan: &Plan, failed_step: &Step) -> Result<Plan>;
}
```

**Event Extensions:**
```rust
// events.rs additions
pub enum AgenticEvent {
    // ... existing events
    PlanCreated(Plan),
    PlanStepStarted(Step),
    PlanStepCompleted(Step),
    PlanStepFailed(Step),
    PlanApprovalRequired(Plan),
    PlanReplanned(Plan),
}
```

---

### 2.2 MCP Async & Streaming
**Files:** `core-agentic/src/mcp/transport.rs`, `core-agentic/src/mcp/client.rs`
**Est:** 2-3 days

**Tasks:**
- [ ] Async transport trait (`tokio` + `reqwest` async)
- [ ] SSE streaming transport for HTTP MCP servers
- [ ] Auto-reconnection on server disconnect
- [ ] Connection health check
- [ ] Integration test against real MCP server (e.g. `@modelcontextprotocol/server-filesystem`)

---

### 2.3 Provider Enhancements
**Files:** `core-agentic/src/providers/`
**Est:** 3-4 days

**Tasks:**
- [ ] Provider trait enhancement (health check, model list)
- [ ] Z.ai provider implementation
- [ ] OpenAI-compatible generic provider
- [ ] Provider failover support
- [ ] Model capability detection
- [ ] Token counting per provider
- [ ] Streaming improvements

**New Provider Trait:**
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<LlmResponse>;
    async fn chat_stream(&self, messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<ChatStream>;
    async fn health_check(&self) -> Result<bool>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;
    fn name(&self) -> &str;
    fn count_tokens(&self, text: &str) -> usize;
}
```

---

### 2.4 Tool Enhancements
**Files:** `core-agentic/src/tools/`
**Est:** 2-3 days

**Tasks:**
- [ ] Tool execution timeout
- [ ] Tool output size limits
- [ ] Tool permission levels
- [ ] Tool result caching (read operations)
- [ ] New tool: `search_files` (full-text search)
- [ ] New tool: `run_script` (multi-line script execution)
- [ ] Tool composition (chain tools)

---

## 🟢 Phase 3: Enterprise Features (Month 2-3) — 15-20 days

### 3.1 Multi-Agent System
**File:** `core-agentic/src/multi_agent/` (NEW)
**Est:** 7-10 days

**Architecture:**
```rust
pub struct AgentOrchestrator {
    planner: PlannerAgent,
    executor: ExecutorAgent,
    reviewer: ReviewerAgent,
    communication: AgentBus,
}

pub struct AgentBus {
    // Inter-agent message passing
}

pub trait SpecializedAgent {
    async fn execute(&self, task: Task) -> Result<TaskResult>;
    fn capabilities(&self) -> Vec<Capability>;
}
```

**Agents:**
- **PlannerAgent** — Decomposes tasks, creates plans
- **ExecutorAgent** — Executes coding tasks, runs tools
- **ReviewerAgent** — Reviews code, suggests improvements

---

### 3.2 Vector DB Memory
**File:** `core-agentic/src/vector_memory/` (NEW)
**Est:** 5-7 days

**Tasks:**
- [ ] Embedding generation (via LLM provider)
- [ ] Vector storage backend (SQLite-vss atau ChromaDB)
- [ ] Semantic search
- [ ] RAG pipeline
- [ ] Memory indexing on new messages
- [ ] Context retrieval optimization

---

### 3.3 Advanced Planner
**File:** `core-agentic/src/planner.rs` (enhance)
**Est:** 3-4 days

**Tasks:**
- [ ] Parallel step execution
- [ ] Step dependency graph
- [ ] Conditional branches
- [ ] Loop/iteration support
- [ ] Plan templates
- [ ] Plan history & versioning

---

## 🧪 Testing

Testing untuk core module diatur di **[PLAN_TESTING.md](./PLAN_TESTING.md)**:
- Phase 1.2: Unit tests (safety scoring, memory, tools, providers)
- Phase 2.1: Integration tests (agent loop, planner flow, MCP)
- Phase 4: Performance & resilience tests

**Rule:** Setiap fitur baru harus menyertakan unit test.

---

## 📊 Dependencies (Rust)

| Crate | Purpose | Phase |
|-------|---------|-------|
| `serde` / `serde_json` | Serialization | ✅ Existing |
| `tokio` | Async runtime | ✅ Existing |
| `reqwest` | HTTP client | ✅ Existing |
| `regex` | Pattern matching (safety) | Phase 1 |
| `uuid` | IDs for plans/steps | Phase 2 |
| `chrono` | Timestamps | Phase 2 |
| `sqlite` atau `rusqlite` | Vector storage | Phase 3 |

---

## 🔗 API Contract (Tauri Commands)

Core module mengekspos API melalui `src-tauri/src/agentic/commands.rs`:

```rust
// Existing commands
agentic_chat_stream(message, cwd)
agentic_load_config(config)
agentic_read_file_config()
agentic_get_status()
agentic_confirm_action(confirmed)
agentic_get_pending_confirmation()

// New commands needed (Phase 2+)
agentic_create_plan(goal)          → Plan
agentic_approve_plan(plan_id)      → ()
agentic_reject_plan(plan_id)       → ()
agentic_execute_plan(plan_id)      → PlanResult
agentic_list_providers()           → Vec<ProviderInfo>
agentic_test_provider(config)      → bool
agentic_list_mcp_servers()         → Vec<McpServerInfo>
agentic_test_mcp_server(config)    → bool
```

---

**Last Updated:** May 4, 2026 — Phase 1 + §2.1 Planner Agent complete. Next: §2.2 MCP Async, §2.3 Providers, §2.4 Tools.
