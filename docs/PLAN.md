# 📋 Development Plan

Rencana pengembangan Termul Manager dan Agentic AI.

---

## 🎯 Overview

**Current Version:** 0.3.1
**Last Update:** May 2, 2026
**Status:** Config automation complete, integration pending

---

## ✅ Completed Features

### Core Terminal Management
- ✅ Project-Based Workspaces
- ✅ Tabbed Interface (Windows Terminal-style)
- ✅ Multiple Shell Support (PowerShell, CMD, Git Bash, WSL)
- ✅ Session Persistence
- ✅ Workspace Snapshots
- ✅ Git Integration
- ✅ Command History
- ✅ Keyboard Shortcuts
- ✅ Cross-Platform Support

### Advanced UI
- ✅ File Explorer with Tree View
- ✅ Code Editor (Markdown + Code)
- ✅ Table of Contents (auto-generated)
- ✅ Context Bar (command palette)
- ✅ Auto-Updates
- ✅ Color Themes
- ✅ Resizable Panels

### AI Agent System
- ✅ LLM Providers (OpenAI, Anthropic)
- ✅ Tool System (7 builtin tools)
- ✅ MCP Client (stdio + HTTP)
- ✅ Agent Orchestration
- ✅ Safety System
- ✅ Memory Management
- ✅ Streaming Output

### Agentic CLI
- ✅ Standalone CLI Binary
- ✅ Interactive Mode
- ✅ Config File Support
- ✅ Environment Variables
- ✅ Safety Controls
- ✅ Markdown Rendering
- ✅ Streaming Output

### Config Automation (Just Completed)
- ✅ First-Run Setup Wizard (UI)
- ✅ First-Run Banner
- ✅ CLI Config Commands (init, show, edit, validate, reset, path)
- ✅ Core Library Helper Functions
- ✅ Tauri Backend Commands
- ✅ Complete Documentation

---

## 🔴 IMMEDIATE NEXT STEPS (This Week)

### 1. Integrate FirstRunBanner (1-2 days)

**Status:** Component created, NOT integrated

**Task:** Add FirstRunBanner to App.tsx

```typescript
// src/renderer/App.tsx
import { FirstRunBanner } from './components/agentic'

return (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>
      <AppEffects />
      <FirstRunBanner /> {/* ADD THIS */}
      <Toaster />
      <Sonner />
      <RouterProvider router={router} />
    </TooltipProvider>
  </QueryClientProvider>
)
```

**Acceptance:**
- Banner shows on first launch with no config
- Quick Setup creates default config
- Setup Wizard opens and completes
- Banner disappears after wizard completes

---

### 2. Integrate AgenticPanel (1-2 days)

**Status:** Component created, NOT in layout

**Task:** Add AgenticPanel to WorkspaceLayout

```typescript
// src/renderer/layouts/WorkspaceLayout.tsx
import { AgenticPanel } from '../components/agentic'

<WorkspaceContent>
  <AgenticPanel /> {/* ADD THIS */}
</WorkspaceContent>
```

**Acceptance:**
- Panel visible in workspace
- Message sending works
- Streaming responses display
- Token usage updates

---

### 3. Add Agentic Settings UI (1 day)

**Status:** No UI for agentic config

**Task:** Add "Agentic AI" section to AppPreferences

```typescript
// src/renderer/pages/AppPreferences.tsx
const AgenticSettings = () => {
  return (
    <Section title="Agentic AI">
      <ConfigStatus />
      <Button onClick={openConfigWizard}>Configure</Button>
      <Button onClick={openConfigEditor}>Edit Config</Button>
    </Section>
  )
}
```

**Acceptance:**
- Can access agentic settings from Preferences
- Can view current config status
- Can open config wizard
- Can manually edit config

---

## 🟡 HIGH PRIORITY (Next 2-3 Weeks)

### 4. Polish Agentic UI (2-3 days)

**Tasks:**
- [ ] Markdown rendering for AI responses
- [ ] Syntax highlighting for code blocks
- [ ] Copy buttons for code blocks
- [ ] Message timestamps
- [ ] Loading indicators
- [ ] Error display
- [ ] Empty states
- [ ] Message input validation

---

### 5. Add Command Palette Actions (1 day)

**Tasks:**
- [ ] "Open Agentic Chat" action
- [ ] "Open Agentic Settings" action
- [ ] "Clear Chat History" action
- [ ] Register in command palette
- [ ] Add keyboard shortcuts

---

### 6. Resolve TODO Items (2-3 days)

**Current TODOs:**
- [ ] Pass actual system env from backend for variable expansion
- [ ] Implement batch delete in FileExplorer
- [ ] Store secret values in secure OS storage (keyring)

---

## 🟢 MEDIUM PRIORITY (1-2 Months)

### 7. Planner Agent (5-7 days)

**Description:** Agent that breaks down complex tasks into steps

**Features:**
- Task decomposition
- Step planning algorithm
- Plan visualization UI
- Plan approval before execution
- Replanning on failure

**Example Flow:**
```
User: "Refactor authentication module"

Planner:
  1. Read auth module files
  2. Analyze current structure
  3. Design new structure
  4. Create refactoring plan
  5. Execute step by step
  6. Run tests after each step
  7. Roll back if tests fail
```

---

### 8. Safety & Sandbox Improvements (3-4 days)

**Status:** ✅ **Core engine completed** (May 4, 2026). Risk scoring, patterns, sandbox, rate limiting, audit logging implemented. UI still pending.

**Description:** Enhanced safety system with risk scoring

**Features:**
- ✅ Command risk scoring (low/medium/high/critical)
- ✅ Dynamic approval based on risk
- ✅ Command blocklist
- ✅ Pattern-based detection (25+ regex patterns)
- ✅ Path sandboxing
- ✅ Rate limiting per tool
- ✅ Audit logging
- [ ] Command preview before execution (UI)
- [ ] Undo capability for file operations
- [ ] Command history review UI
- [ ] Safety settings UI

**Risk Scoring:**
```rust
"ls -la"           → risk: 0.1 (auto-approve)
"npm install"       → risk: 0.3 (auto-approve)
"rm file.txt"       → risk: 0.6 (confirm)
"git reset --hard"  → risk: 0.7 (confirm)
"rm -rf /"         → risk: 1.0 (blocked)
"mkfs /dev/sda1"    → risk: 1.0 (blocked)
```

---

### 9. Multi-Provider Management UI (3-4 days)

**Description:** UI to manage multiple AI providers

**Features:**
- Add/edit/delete providers
- Configure API keys per provider
- Configure models per provider
- Test API key validity
- Set default provider
- Switch between providers

**Supported Providers:**
- OpenAI
- Anthropic (Claude)
- Z.ai
- Custom OpenAI-compatible

---

### 10. MCP Server Management UI (2-3 days)

**Description:** UI to manage MCP servers

**Features:**
- Add/edit MCP servers
- MCP server templates
- Connection testing
- Server status monitoring
- Show available tools per server
- Enable/disable servers

**Templates:**
- Filesystem (npx @modelcontextprotocol/server-filesystem)
- Git (custom)
- Database (custom)

---

### 11. Memory & Context Improvements (2-3 days)

**Status:** ✅ **Core engine completed** (May 4, 2026). Sliding window, pinning, sessions, persistence, search implemented. UI still pending.

**Description:** Enhanced memory and context management

**Features:**
- ✅ Context window management (token-based sliding window)
- ✅ Message summarization (smart compaction)
- ✅ Sliding window for long conversations
- ✅ Persistent memory across sessions
- ✅ Memory search (keyword + role)
- ✅ Message pinning
- ✅ Session isolation
- ✅ Context budget tracking
- [ ] Memory search UI
- [ ] Memory visualization

---

## 🔵 FUTURE (3+ Months)

### 12. Config Backup & Restore (1-2 days)

**Features:**
- Auto-backup on config changes
- Manual backup trigger
- Restore from backup
- Import/export config
- Config versioning

---

### 13. Config Templates & Sharing (1-2 days)

**Features:**
- Template library
- Community templates
- Import/export templates
- Share template URL
- Template preview

---

### 14. Advanced Planner Features (3-4 days)

**Features:**
- Parallel step execution
- Step dependencies
- Conditional branches
- Loop/iteration support
- Plan templates
- Plan history

---

### 15. Vector DB for Memory (5-7 days)

**Description:** Persistent memory with semantic search

**Features:**
- Integrate vector DB (chromadb, pgvector)
- Semantic search
- Memory embeddings
- Retrieval-augmented generation (RAG)
- Memory management UI
- Memory analytics

---

### 16. Multi-Agent System (7-10 days)

**Description:** Specialized agents for different tasks

**Agents:**
- **Planner Agent** - Plans and breaks down tasks
- **Executor Agent** - Executes coding tasks
- **Reviewer Agent** - Reviews code and suggests improvements

**Example Flow:**
```
User: "Add user authentication with OAuth2"

Planner Agent:
  → Plans implementation steps

Executor Agent:
  → Executes coding tasks
  → Creates files
  → Runs commands

Reviewer Agent:
  → Reviews code
  → Suggests improvements
  → Checks for issues
```

---

## 🐛 Testing & Quality

Testing diatur secara terpisah di **[PLAN_TESTING.md](./PLAN_TESTING.md)**.

### Quick Reference
- **Unit Tests** → Phase 1 (bersama feature integration)
- **Integration Tests** → Phase 2 (bersama feature polish)
- **E2E Tests** → Phase 3 (bersama management UIs)
- **Performance & Security** → Phase 4 (advanced)

---

## 📚 Documentation

### User Documentation (3-4 days)
- [ ] Getting started guide
- [ ] Configuration guide
- [ ] Features overview
- [ ] Troubleshooting guide
- [ ] FAQ
- [ ] Video tutorials (optional)

### Developer Documentation (2-3 days)
- [ ] Architecture overview
- [ ] Contribution guide
- [ ] API documentation
- [ ] Testing guide
- [ ] Code examples

---

## 📊 Sprint Planning

### Sprint 1 (1 week): Integration
- [ ] Integrate FirstRunBanner
- [ ] Integrate AgenticPanel
- [ ] Add Agentic Settings
- [x] Safety System Enhancement (core-agentic)
- [x] Memory System Enhancement (core-agentic)
- [x] Unit tests for safety + memory (96 new tests)

### Sprint 2 (1 week): Polish
- [ ] Polish Agentic UI
- [ ] Add Command Palette Actions
- [ ] Resolve TODO items
- [ ] Bug fixes

### Sprint 3 (2 weeks): Advanced Features
- [ ] Planner Agent
- [ ] Safety Improvements
- [ ] Multi-Provider Management
- [ ] MCP Management

### Sprint 4 (2 weeks): Enhancements
- [ ] Memory Improvements
- [ ] Config Backup/Restore
- [ ] Config Templates
- [ ] E2E Testing

---

## 📈 Success Metrics

### Short-term (1 month)
- [ ] Config setup time < 2 minutes
- [ ] First-run wizard completion > 80%
- [ ] Agentic usage > 50% of active users
- [ ] Critical bugs < 5

### Medium-term (3 months)
- [ ] Multi-provider usage > 30%
- [ ] MCP servers configured > 40%
- [ ] Planner agent usage > 20%
- [ ] User satisfaction > 4.5/5

### Long-term (6 months)
- [ ] Multi-agent system production-ready
- [ ] Vector DB memory implementation
- [ ] Community template library
- [ ] 1000+ active users

---

## 🔗 References

| Document | Description |
|-----------|-------------|
| `docs/PLAN_SUMMARY.md` | Modular plan overview |
| `docs/PLAN_TESTING.md` | Testing strategy (unit, integration, E2E) |
| `docs/AGENTIC_PRD.md` | Product Requirements Document |
| `docs/CONFIGURATION.md` | Configuration Schema |
| `docs/CONFIG_AUTOMATION.md` | Config Automation Features |
| `docs/IMPLEMENTATION_SUMMARY.md` | Implementation Summary |
| `core-agentic/docs/TOOL_REFERENCE.md` | Tools Reference |
| `core-agentic/docs/plans/2026-04-22-mcp-client.md` | MCP Implementation |

---

## 📦 Tech Stack

### Frontend
- React 18 + TypeScript
- Tauri 2.0
- Vite
- Tailwind CSS
- shadcn/ui
- Zustand
- Framer Motion
- xterm.js

### Backend
- Rust
- Tauri 2.0
- core-agentic
- tokio (async runtime)
- MCP Protocol

### Testing
- Vitest + Testing Library (unit & component)
- Playwright (E2E)
- cargo test (Rust unit & integration)
- mockall (mock LLM provider)
- ESLint + Husky (pre-commit)

See [PLAN_TESTING.md](./PLAN_TESTING.md) for full strategy.

---

## 🚦 Priority Legend

| Priority | Meaning | Timeline |
|----------|----------|----------|
| 🔴 Critical | Must do for production | 1-2 weeks |
| 🟡 High | Important for good UX | 2-4 weeks |
| 🟢 Medium | Nice to have | 1-2 months |
| 🔵 Low | Future consideration | 3+ months |

---

## 📝 Notes

- Focus on Phase 3 (Integration) tasks first
- This will make automation features actually usable
- Then proceed to Phase 4 based on user feedback
- Continue bug fixes and improvements throughout
- Regularly update roadmap based on progress

---

**Last Updated:** May 4, 2026
**Next Review:** After Sprint 1 completion
