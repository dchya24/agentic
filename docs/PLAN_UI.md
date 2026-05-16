# 🎨 Plan: Agentic UI Module

Rencana pengembangan untuk semua komponen UI Agentic di layer renderer (React/TypeScript).

---

## 📁 Scope & File Boundaries

```
src/renderer/
├── components/agentic/       ← Semua komponen UI agentic
│   ├── AgenticPanel.tsx
│   ├── AgenticSidebar.tsx
│   ├── AgenticInput.tsx
│   ├── AgenticOutput.tsx
│   ├── AgenticConfigWizard.tsx
│   ├── FirstRunBanner.tsx
│   ├── PlannerPanel.tsx          (NEW)
│   ├── SafetyPanel.tsx           (NEW)
│   ├── ProviderManager.tsx       (NEW)
│   ├── McpManager.tsx            (NEW)
│   └── MemoryPanel.tsx           (NEW)
├── stores/
│   └── agentic-store.ts      ← State management
├── hooks/
│   └── use-agentic-*.ts      (NEW hooks)
├── pages/
│   └── AppPreferences.tsx     ← Settings page
├── layouts/
│   └── WorkspaceLayout.tsx    ← Layout integration
└── lib/
    └── agentic-config-helper.ts
```

---

## 🔴 Phase 1: Core Integration (Week 1) — 3-5 hours

### 1.1 Integrate FirstRunBanner
**File:** `src/renderer/App.tsx`
**Est:** 1-2 hours

```typescript
import { FirstRunBanner } from './components/agentic'

// Add before RouterProvider
<FirstRunBanner />
```

**Acceptance:**
- [ ] Banner tampil saat pertama kali (no config)
- [ ] Quick Setup membuat default config
- [ ] Setup Wizard berjalan lengkap
- [ ] Banner hilang setelah wizard selesai

---

### 1.2 Integrate AgenticPanel
**File:** `src/renderer/layouts/WorkspaceLayout.tsx`
**Est:** 1-2 hours

```typescript
import { AgenticPanel } from '../components/agentic'

// Add to layout
<AgenticPanel />
```

**Acceptance:**
- [ ] Panel terlihat di workspace
- [ ] Message sending berfungsi
- [ ] Streaming response tampil
- [ ] Token usage update real-time

---

### 1.3 Add Agentic Settings
**File:** `src/renderer/pages/AppPreferences.tsx`
**Est:** 1 hour

**Tasks:**
- [ ] Tambah section "Agentic AI" di AppPreferences
- [ ] Display config status (valid/invalid)
- [ ] Tombol "Configure" → buka ConfigWizard
- [ ] Tombol "Edit Config" → buka file editor
- [ ] Tombol "Validate" & "Reset"

---

## 🟡 Phase 2: UI Polish (Week 2) — 2-3 days

### 2.1 AgenticOutput Improvements
**File:** `src/renderer/components/agentic/AgenticOutput.tsx`

**Tasks:**
- [ ] Markdown rendering (react-markdown atau marked)
- [ ] Syntax highlighting (shiki/prism) untuk code blocks
- [ ] Copy button per code block
- [ ] Message timestamps
- [ ] Message dividers
- [ ] Loading/typing indicators
- [ ] Error display styling
- [ ] Empty state design
- [ ] Thought/reasoning collapsible section
- [ ] Tool call display (collapsible)

### 2.2 AgenticInput Improvements
**File:** `src/renderer/components/agentic/AgenticInput.tsx`

**Tasks:**
- [ ] Multi-line input (textarea)
- [ ] Input validation
- [ ] Command history navigation (↑/↓)
- [ ] Auto-resize textarea
- [ ] Attachment support (future)

### 2.3 Command Palette Integration
**File:** `src/renderer/components/CommandPalette.tsx`

**Tasks:**
- [ ] "Open Agentic Chat" action
- [ ] "Open Agentic Settings" action
- [ ] "Clear Chat History" action
- [ ] "Restart Agentic" action
- [ ] Register keyboard shortcuts

---

## 🟢 Phase 3: Management UIs (Week 3-4) — 8-12 days

### 3.1 Provider Manager UI
**File:** `src/renderer/components/agentic/ProviderManager.tsx` (NEW)
**Est:** 3-4 days

**Tasks:**
- [ ] Provider list view
- [ ] Add provider dialog (OpenAI, Anthropic, Z.ai, custom)
- [ ] Edit provider dialog
- [ ] Delete provider
- [ ] API key input (masked) + test validity
- [ ] Model selection per provider
- [ ] Set default provider
- [ ] Switch provider during chat
- [ ] Provider health status indicator

### 3.2 MCP Server Manager UI
**File:** `src/renderer/components/agentic/McpManager.tsx` (NEW)
**Est:** 2-3 days

**Tasks:**
- [ ] MCP server list
- [ ] Add/edit MCP server dialog
- [ ] Template library (filesystem, git, database)
- [ ] Connection testing
- [ ] Server status monitoring
- [ ] Available tools per server
- [ ] Enable/disable toggle

### 3.3 Safety Panel UI
**File:** `src/renderer/components/agentic/SafetyPanel.tsx` (NEW)
**Est:** 1-2 days

**Tasks:**
- [ ] Command risk level display
- [ ] Confirmation dialog (approve/reject)
- [ ] Command preview before execution
- [ ] Safety settings UI
- [ ] Command history with risk levels

### 3.4 Planner Visualization UI
**File:** `src/renderer/components/agentic/PlannerPanel.tsx` (NEW)
**Est:** 2-3 days

**Tasks:**
- [ ] Plan visualization (step list)
- [ ] Plan approval UI (approve/edit/reject)
- [ ] Progress tracking per step
- [ ] Step status indicators (pending/running/done/failed)
- [ ] Re-plan on failure UI

### 3.5 Memory Panel UI
**File:** `src/renderer/components/agentic/MemoryPanel.tsx` (NEW)
**Est:** 1-2 days

**Tasks:**
- [ ] Memory usage visualization
- [ ] Context window status
- [ ] Conversation history search
- [ ] Memory management (clear/pin/delete)
- [ ] Summary view

---

## 🔵 Phase 4: Advanced UI (Month 2+) — 7-10 days

### 4.1 Config Templates UI
**Est:** 1-2 days

- [ ] Template browser
- [ ] Template preview
- [ ] Import/export template
- [ ] Community template gallery

### 4.2 Multi-Agent Visualization UI
**Est:** 3-4 days

- [ ] Agent workflow diagram
- [ ] Agent status cards
- [ ] Inter-agent communication display
- [ ] Agent selection/switching

### 4.3 Vector DB Memory UI
**Est:** 2-3 days

- [ ] Semantic search UI
- [ ] Memory embeddings visualization
- [ ] RAG query interface
- [ ] Memory analytics dashboard

---

## 🧪 Testing

Testing untuk UI komponen diatur di **[PLAN_TESTING.md](./PLAN_TESTING.md)**:
- Phase 1.3: Unit tests untuk komponen agentic
- Phase 3.2: E2E tests untuk user flows

**Rule:** Setiap komponen baru harus menyertakan unit test.

---

## 🖥️ TUI Mode (agentic-cli/src/tui/)

The CLI also has a full TUI mode built with ratatui. The TUI shares the same
`@` file dropdown and `/` command dropdown as the CLI interactive mode.

### Files

| File | Description |
|------|-------------|
| `tui/mod.rs` | Module exports |
| `tui/app.rs` | App state, event loop, `@` dropdown trigger logic |
| `tui/dropdown.rs` | Dropdown: recursive `.gitignore`-aware file listing (uses `ignore` crate) |
| `tui/input.rs` | Input rendering with cursor + `@` highlighting |
| `tui/ui.rs` | Full UI layout: header, messages, progress, input, dropdown overlay |
| `tui/markdown_widget.rs` | Markdown → ratatui styled lines |
| `tui/progress.rs` | Spinner + progress bar state |

### `@` File Dropdown Behavior

Both TUI and CLI interactive modes share the same file listing logic:

| Input | Behavior |
|-------|----------|
| `@` (empty) | All project files recursively (flat list) |
| `@src/` | All files under `src/` recursively |
| `@src/ma` | Files under `src/` matching "ma" |
| `@chat` | All project files matching "chat" |

- Uses `ignore` crate (ripgrep ecosystem) — automatically respects `.gitignore`
- `node_modules`, `target`, `.git`, `dist`, `build`, etc. are excluded automatically
- Paths normalized: Windows backslashes → forward slashes
- Sorted: directories first (with `/`), then files, alphabetically
- Selecting a directory in TUI auto-reopens the dropdown to browse into it

---

## 📦 Dependencies yang Mungkin Dibutuhkan

| Package | Purpose | Phase |
|---------|---------|-------|
| `react-markdown` | Markdown rendering | Phase 2 |
| `remark-gfm` | GitHub-flavored markdown | Phase 2 |
| `rehype-highlight` atau `shiki` | Syntax highlighting | Phase 2 |
| `lucide-react` (existing) | Icons | All phases |

---

## 🚦 Priority Legend

| Priority | Timeline |
|----------|----------|
| 🔴 Critical | 1 week |
| 🟡 High | 2 weeks |
| 🟢 Medium | 1 month |
| 🔵 Low | 2+ months |

---

**Last Updated:** May 15, 2026
