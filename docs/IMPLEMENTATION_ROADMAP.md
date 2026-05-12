# 🚀 Implementation Roadmap

Rencana implementasi pengembangan selanjutnya untuk Termul Manager dan Agentic AI.

---

## 📊 Current Status

### ✅ Completed (Phase 1-2)

| Feature | Status | Notes |
|---------|--------|-------|
| Core Terminal Management | ✅ Done | Termul Manager basic features |
| Tabbed Interface | ✅ Done | Windows Terminal-style |
| Multiple Shell Support | ✅ Done | PowerShell, CMD, Git Bash, WSL |
| Session Persistence | ✅ Done | Cross restart sessions |
| Workspace Snapshots | ✅ Done | Save/restore states |
| Git Integration | ✅ Done | Branch/status in status bar |
| File Explorer | ✅ Done | Tree view with operations |
| Code Editor | ✅ Done | Markdown + code editing |
| LLM Providers | ✅ Done | OpenAI, Anthropic |
| Tool System | ✅ Done | 7 builtin tools |
| MCP Client | ✅ Done | stdio + HTTP transports |
| Config Automation | ✅ Done | Wizard + CLI commands |
| Agentic Integration | ✅ Done | Store + Tauri commands |

### ✅ Completed (Phase 3 — Integration)

| Feature | Status | Notes |
|---------|--------|-------|
| First-Run Banner Integration | ✅ Done | Mounted in App.tsx, auto-checks config |
| Agentic Panel in UI | ✅ Done | AgenticPanel in PaneContent, agentic tab via workspace store |
| Agentic Settings in Preferences | ✅ Done | Config status, wizard, validate, edit in AppPreferences |
| Command Palette Agentic Actions | ✅ Done | Open Chat, Open Settings, Clear History |

---

## 🎯 Phase 3: Integration & Polish (Next Immediate)

### Priority: 🔴 Critical

#### 1. Integrate Config Automation (1-2 days)

**Status:** ✅ Done

**Tasks:**
- [x] Add FirstRunBanner to App.tsx
- [x] Add hook to auto-check config on app start
- [x] Show banner if config not set up
- [x] Wire up Quick Setup button
- [x] Wire up Setup Wizard button
- [ ] Test first-run flow end-to-end
- [ ] Test config creation flow
- [ ] Test banner dismiss behavior

**Files:**
- `src/renderer/App.tsx` - Add FirstRunBanner
- `src/renderer/components/agentic/FirstRunBanner.tsx` - Test integration
- `src/renderer/lib/agentic-config-helper.ts` - Test functions

**Acceptance Criteria:**
- Banner shows on first app launch with no config
- Banner doesn't show after wizard completes
- Quick Setup creates default config
- Setup Wizard opens and completes successfully
- Config file is created at correct path

---

#### 2. Integrate Agentic Panel in WorkspaceLayout (1-2 days)

**Status:** ✅ Done

**Tasks:**
- [x] Add AgenticPanel to WorkspaceLayout
- [x] Add AgenticSidebar to WorkspaceLayout
- [x] Wire up message sending
- [x] Wire up streaming display
- [x] Wire up token usage display
- [x] Wire up status display
- [ ] Add keyboard shortcuts for agentic
- [ ] Test chat flow
- [ ] Test streaming responses
- [ ] Test tool invocation display

**Files:**
- `src/renderer/layouts/WorkspaceLayout.tsx` - Add agentic panels
- `src/renderer/components/agentic/AgenticPanel.tsx` - Test chat UI
- `src/renderer/components/agentic/AgenticSidebar.tsx` - Test status UI

**Acceptance Criteria:**
- Agentic panel visible in workspace
- User can send messages
- Streaming responses display correctly
- Token usage updates in real-time
- Status shows correct model and provider
- Keyboard shortcuts work (if implemented)

---

### Priority: 🟡 High

#### 3. Add Agentic Settings to AppPreferences (1 day)

**Status:** ✅ Done

**Tasks:**
- [x] Add "Agentic AI" section to AppPreferences
- [x] Display current provider and model
- [x] Add button to open config wizard
- [x] Add button to open config file
- [x] Add button to validate config
- [ ] Add button to reset config
- [x] Display config status (valid/invalid)
- [ ] Show API key status (masked)

**Files:**
- `src/renderer/pages/AppPreferences.tsx` - Add agentic section
- `src/renderer/lib/agentic-config-helper.ts` - Use existing helpers

**Acceptance Criteria:**
- User can access agentic settings from Preferences
- Can view current config status
- Can open config wizard to update settings
- Can manually edit config file
- Can validate and reset config

---

#### 4. Add Agentic Command Palette Actions (1 day)

**Status:** ✅ Done

**Tasks:**
- [x] Add "Open Agentic Chat" action
- [x] Add "Open Agentic Settings" action
- [x] Add "Clear Agentic History" action
- [ ] Add "Restart Agentic" action
- [x] Register actions in command palette
- [ ] Add keyboard shortcuts

**Files:**
- Find command palette implementation
- Add agentic actions

**Acceptance Criteria:**
- Can open agentic chat via Ctrl+Shift+A (example)
- Can open agentic settings via palette
- Can clear chat history via palette
- All actions show in palette search

---

### Priority: 🟢 Medium

#### 5. Polish Agentic UI (1-2 days)

**Status:** Basic UI exists, needs polish

**Tasks:**
- [ ] Add markdown rendering to AI responses
- [ ] Add syntax highlighting for code blocks
- [ ] Add copy button for code blocks
- [ ] Add message timestamps
- [ ] Add message dividers
- [ ] Add loading indicators
- [ ] Add error display
- [ ] Add empty state
- [ ] Add message input validation
- [ ] Add message history sidebar (collapsible)

**Files:**
- `src/renderer/components/agentic/AgenticOutput.tsx` - Improve display
- `src/renderer/components/agentic/AgenticInput.tsx` - Improve input

**Acceptance Criteria:**
- Markdown renders correctly (headers, lists, code blocks)
- Code has syntax highlighting
- Code blocks have copy buttons
- Messages have timestamps
- Clear visual hierarchy between messages
- Smooth loading animations
- Helpful empty states

---

## 🚀 Phase 4: Advanced Features (2-4 weeks)

### Priority: 🔴 Critical for Production

#### 6. Planner Agent (5-7 days)

**Reference:** PRD Future Scope - Multi-agent system

**Status:** Not implemented

**Tasks:**
- [ ] Design planner agent architecture
- [ ] Implement planner agent in core-agentic
- [ ] Add task decomposition logic
- [ ] Add step planning algorithm
- [ ] Add plan execution tracking
- [ ] Add replanning on failure
- [ ] Add plan visualization UI
- [ ] Add plan approval UI
- [ ] Add plan edit capability
- [ ] Test complex multi-step tasks

**Files:**
- `core-agentic/src/planner.rs` - New module
- `core-agentic/src/agent.rs` - Update to support planner
- `src/renderer/components/agentic/PlannerPanel.tsx` - New UI

**Features:**
- Agent breaks down complex tasks into steps
- Shows plan to user before execution
- Allows user to approve/edit plan
- Tracks progress through steps
- Replans if steps fail
- Shows step-by-step execution

**Example Flow:**
```
User: "Refactor the authentication module"

Planner:
  1. Read auth module files
  2. Analyze current structure
  3. Design new structure
  4. Create refactoring plan
  5. Execute refactoring step by step
  6. Run tests after each step
  7. Roll back if tests fail
```

---

#### 7. Safety & Sandbox Improvements (3-4 days)

**Reference:** PRD Safety Requirements

**Status:** ✅ Core safety enhanced (May 4, 2026). Risk scoring, patterns, sandbox, rate limiting, audit logging done. UI pending.

**Tasks:**
- [x] Implement command risk scoring
- [x] Add dynamic approval based on risk
- [ ] Add command preview before execution
- [ ] Add undo capability for file operations
- [x] Implement basic sandbox mode
- [ ] Add command history review UI
- [ ] Add safety settings UI
- [x] Test dangerous command blocking
- [x] Test risk scoring accuracy

**Files:**
- `core-agentic/src/safety.rs` - Enhance
- `src/renderer/components/agentic/SafetyPanel.tsx` - New UI
- `src-tauri/src/sandbox.rs` - New module (optional)

**Features:**
- Risk scoring algorithm (low/medium/high)
- Auto-approve low risk
- Require confirmation for medium/high risk
- Block dangerous commands entirely
- Preview commands before execution
- Undo file operations (write, edit)
- Show command history with risk levels

**Risk Scoring Example:**
```rust
// Low risk
"ls -la" → risk: 0.1 (auto-approve)
"npm install" → risk: 0.3 (auto-approve)

// Medium risk
"rm file.txt" → risk: 0.6 (confirm)
"git reset --hard" → risk: 0.7 (confirm)

// High risk (blocked)
"rm -rf /" → risk: 1.0 (blocked)
"mkfs /dev/sda1" → risk: 1.0 (blocked)
```

---

### Priority: 🟡 High

#### 8. Multi-Provider Management UI (3-4 days)

**Status:** Only one provider supported in UI

**Tasks:**
- [ ] Add provider list UI
- [ ] Add add provider dialog
- [ ] Add edit provider dialog
- [ ] Add delete provider capability
- [ ] Add provider testing (API key validation)
- [ ] Add model selection per provider
- [ ] Add default provider selection
- [ ] Add provider switch UI
- [ ] Save multi-provider config

**Files:**
- `src/renderer/pages/AppPreferences.tsx` - Add provider management section
- `src/renderer/components/agentic/ProviderManager.tsx` - New UI
- `src-tauri/src/agentic/commands.rs` - Add provider commands

**Features:**
- Add multiple providers (OpenAI, Anthropic, custom)
- Configure API keys per provider
- Configure models per provider
- Test API key validity
- Set default provider
- Switch between providers during chat

---

#### 9. MCP Server Management UI (2-3 days)

**Status:** MCP servers configured in JSON only

**Tasks:**
- [ ] Add MCP server list UI
- [ ] Add add MCP server dialog
- [ ] Add edit MCP server dialog
- [ ] Add MCP server template library
- [ ] Add MCP server testing
- [ ] Add MCP server status monitoring
- [ ] Show available tools per MCP server
- [ ] Enable/disable MCP servers

**Files:**
- `src/renderer/components/agentic/McpManager.tsx` - New UI
- `src-tauri/src/agentic/commands.rs` - Add MCP commands

**Features:**
- Visual MCP server management
- Pre-configured templates (filesystem, git, etc.)
- Test MCP server connection
- Show available tools from each server
- Monitor server status
- Enable/disable individual servers

---

#### 10. Memory & Context Improvements (2-3 days)

**Reference:** PRD Memory & Context Management

**Status:** ✅ Core memory enhanced (May 4, 2026). Sliding window, pinning, sessions, persistence, search done. UI pending.

**Tasks:**
- [x] Implement context window management
- [x] Add message summarization
- [x] Add sliding window for long conversations
- [x] Add memory persistence across sessions
- [ ] Add memory search UI
- [ ] Add memory visualization
- [x] Add context optimization

**Files:**
- `core-agentic/src/memory.rs` - Enhance
- `src/renderer/components/agentic/MemoryPanel.tsx` - New UI

**Features:**
- Automatically summarize old messages
- Keep important context in memory
- Search conversation history
- Visualize memory usage
- Optimize context for API calls
- Persistent memory across app restarts

---

## 🔮 Phase 5: Future Enhancements (Ongoing)

### Priority: 🟢 Medium

#### 11. Config Backup & Restore (1-2 days)

**Tasks:**
- [ ] Auto-backup config on changes
- [ ] Manual backup trigger
- [ ] Restore from backup
- [ ] Import config from file
- [ ] Export config to file
- [ ] Config versioning

---

#### 12. Config Templates & Sharing (1-2 days)

**Tasks:**
- [ ] Template library
- [ ] Community templates
- [ ] Template import/export
- [ ] Share template URL
- [ ] Template preview

---

#### 13. Advanced Planner Features (3-4 days)

**Tasks:**
- [ ] Parallel step execution
- [ ] Step dependencies
- [ ] Conditional branches
- [ ] Loop/iteration support
- [ ] Plan templates
- [ ] Plan history

---

#### 14. Vector DB for Memory (5-7 days)

**Reference:** PRD Future Scope - Persistent memory (vector DB)

**Tasks:**
- [ ] Integrate vector DB (chromadb, pgvector, etc.)
- [ ] Add semantic search
- [ ] Add memory embeddings
- [ ] Add retrieval-augmented generation (RAG)
- [ ] Add memory management UI
- [ ] Add memory analytics

---

#### 15. Multi-Agent System (7-10 days)

**Reference:** PRD Future Scope - Multi-agent system

**Tasks:**
- [ ] Design multi-agent architecture
- [ ] Implement planner agent
- [ ] Implement executor agent
- [ ] Implement reviewer agent
- [ ] Add agent communication
- [ ] Add agent coordination
- [ ] Add agent visualization UI
- [ ] Test complex workflows

**Example Multi-Agent Flow:**
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

## 🐛 Bug Fixes & Improvements (Ongoing)

### Priority: 🟡 High

#### 16. Resolve TODO Items (2-3 days)

**Current TODOs from codebase:**

1. **WorkspaceLayout.tsx**
   - [ ] Pass actual system env from backend for variable expansion
   - [ ] Route to active terminal pane via context

2. **FileExplorer.tsx**
   - [ ] Implement batch delete with multi-select

3. **env-parser.ts**
   - [ ] Add system env expansion for $VAR references

4. **use-terminal-restore.ts**
   - [ ] Pass actual system env from backend for variable expansion

5. **use-projects-persistence.ts**
   - [ ] Store secret values in secure OS storage (keyring/secureStore)

6. **use-snapshots.ts**
   - [ ] Pass actual system env from backend for variable expansion

---

## 📋 Testing & Quality Assurance

### Priority: 🔴 Critical

#### 17. E2E Testing (3-5 days)

**Tasks:**
- [ ] Set up E2E test framework (Playwright/Cypress)
- [ ] Write tests for config setup flow
- [ ] Write tests for agentic chat flow
- [ ] Write tests for file operations
- [ ] Write tests for terminal operations
- [ ] Write tests for project management
- [ ] Set up CI/CD pipeline

---

#### 18. Performance Optimization (2-3 days)

**Tasks:**
- [ ] Profile application startup time
- [ ] Optimize large file operations
- [ ] Optimize terminal rendering
- [ ] Optimize config loading
- [ ] Add lazy loading for components
- [ ] Optimize bundle size

---

#### 19. Security Audit (2-3 days)

**Tasks:**
- [ ] Audit API key handling
- [ ] Audit file access permissions
- [ ] Audit command execution safety
- [ ] Audit MCP server communication
- [ ] Audit data storage
- [ ] Implement rate limiting

---

## 📚 Documentation

### Priority: 🟢 Medium

#### 20. User Documentation (3-4 days)

**Tasks:**
- [ ] Write getting started guide
- [ ] Write configuration guide
- [ ] Write features overview
- [ ] Write troubleshooting guide
- [ ] Write FAQ
- [ ] Add video tutorials (optional)

---

#### 21. Developer Documentation (2-3 days)

**Tasks:**
- [ ] Write architecture overview
- [ ] Write contribution guide
- [ ] Write API documentation
- [ ] Write testing guide
- [ ] Add code examples

---

## 🎯 Sprint Planning (Recommended)

### Sprint 1 (1 week): Integration ✅
- [x] Integrate Config Automation
- [x] Integrate Agentic Panel
- [x] Add Agentic Settings
- [ ] Basic testing

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
- [ ] Config setup time < 2 minutes (currently requires manual editing)
- [ ] First-run wizard completion rate > 80%
- [ ] Agentic usage > 50% of active users
- [ ] Critical bugs < 5

### Medium-term (3 months)
- [ ] Multi-provider usage > 30%
- [ ] MCP servers configured > 40% of users
- [ ] Planner agent usage > 20%
- [ ] User satisfaction score > 4.5/5

### Long-term (6 months)
- [ ] Multi-agent system production-ready
- [ ] Vector DB memory implementation
- [ ] Community template library
- [ ] 1000+ active users

---

## 🔗 References

- **PRD:** `docs/AGENTIC_PRD.md`
- **Config Schema:** `docs/CONFIGURATION.md`
- **Config Automation:** `docs/CONFIG_AUTOMATION.md`
- **Implementation Summary:** `docs/IMPLEMENTATION_SUMMARY.md`
- **MCP Plan:** `core-agentic/docs/plans/2026-04-22-mcp-client.md`
- **Tool Reference:** `core-agentic/docs/TOOL_REFERENCE.md`

---

## 🚦 Priority Legend

| Priority | Meaning | Timeline |
|----------|----------|-----------|
| 🔴 Critical | Must do for production | 1-2 weeks |
| 🟡 High | Important for good UX | 2-4 weeks |
| 🟢 Medium | Nice to have | 1-2 months |
| 🔵 Low | Future consideration | 3+ months |

---

## 📝 Notes

- Focus on completing Phase 3 (Integration) first
- This will make the automation features actually usable
- Then move to Phase 4 based on user feedback
- Continue bug fixes and improvements throughout
- Regularly update roadmap based on progress
