# TUI Feature Gap — Closing the Gap with Interactive Mode

**Date:** 2026-07-14  
**Branch:** `dev`  
**Scope:** `agentic-cli/src/tui/`, `agentic-cli/src/session.rs`, `agentic-cli/src/commands.rs`  
**Status:** Planned

---

## Goal

Bring the TUI (`agentic tui`) feature-parity with interactive mode  
(`agentic interactive`). The interactive mode currently leads in slash  
commands, completion system, session management, and several UX conveniences.  
This document itemizes every gap and assigns implementation tasks.

---

## Current State

| Category | Interactive | TUI |
|----------|:-----------:|:---:|
| Slash commands | 14 | 6 |
| Tab completion (`/`, `@`) | ✅ | ❌ |
| Session resume | ✅ | ❌ |
| Model switching | ✅ | ❌ |
| Image attachment | ✅ | ❌ |
| MCP status | ✅ | ❌ |
| Conversation search | ✅ | ❌ |
| Plan mode | ✅ | ❌ |
| Context indicator | ✅ | ❌ |
| Multi-pane layout | ❌ | ✅ |
| Real-time streaming | ❌ | ✅ |
| Live token stats | ❌ | ✅ |
| Tool call notifications | ❌ | ✅ |
| Cancel in-flight (Ctrl+C) | ❌ | ✅ |

---

## Gap Items

### G-01: `/` command tab completion

**Priority:** P2 (UX)  
**Files:** `agentic-cli/src/tui/input.rs`, `agentic-cli/src/tui/ui.rs`  
**Estimate:** 1 hari

**Description:**  
Popup completion list saat user ketik `/` lalu tekan `Tab` atau `Ctrl+Space`.  
Completion list menampilkan semua available slash commands beserta deskripsi  
singkat.

**Reference:** `agentic-cli/src/interactive.rs` → `show_slash_completions()`

**Changes:**
- Tambah `Completions` struct ke `input.rs` (mirip `CompletionState` di interactive.rs)
- Handle `Tab` / `Ctrl+Space` di event loop untuk toggle completion popup
- Render completion list sebagai overlay di atas input panel di `ui.rs`
- Support `Enter` untuk select, `Esc` untuk dismiss, `Up/Down` untuk navigate
- Syntax highlighting: `/` commands berwarna kuning, `@` files berwarna biru

---

### G-02: `@` file path completion

**Priority:** P2 (UX)  
**Files:** `agentic-cli/src/tui/input.rs`, `agentic-cli/src/tui/ui.rs`  
**Estimate:** 1 hari  
**Depends on:** G-01

**Description:**  
Popup completion untuk file path saat user ketik `@`. Auto-complete path  
dari current directory, termasuk parent directory navigation (`../`).

**Reference:** `agentic-cli/src/interactive.rs` → `show_file_completions()`

**Changes:**
- Tambah `handle_file_completions()` di `input.rs` (reuse logic dari `interactive.rs`)
- Render file completion overlay di `ui.rs` (share popup renderer dengan G-01)
- Handle path completion di event loop; accept on `Tab` / `Enter`

---

### G-03: `/models <name>` — Switch model

**Priority:** P1 (Core)  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 1 hari

**Description:**  
User bisa switch model dengan ketik `/models <model-name>`. Support fuzzy  
match & update `App` state (provider + model). Jika tanpa arg, tampilkan  
list (sudah ada).

**Reference:** `agentic-cli/src/commands.rs` → `switch_model()` + `pick_model_interactive_inline()`

**Changes:**
- Parse arg di `handle_command("models", args)`: jika ada arg → switch,  
  jika tidak → tampilkan list (existing behavior)
- Tambah helper `fuzzy_match_model(query, model_list)` (reuse logic dari `commands.rs`)
- Update `self.provider` & `self.model` di `App` state
- Tambah visual feedback di output panel:  
  `"Model switched to gpt-4o (openai)"`
- Clear `self.available_models` cache saat switch agar list di-refresh

---

### G-04: `/new` — Proper session reset

**Priority:** P1 (Core)  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 0.5 hari

**Description:**  
`/clear` sudah ada tapi stats gak di-reset. Harus ada `/new` yang reset  
segalanya: messages, tool calls, stats, image attachment, dan session ID.

**Reference:** `agentic-cli/src/interactive.rs` → `handle_new()`

**Changes:**
- Tambah command `/new` (jangan ganti `/clear`, biarkan `/clear` sebagai alias  
  yang hanya wipe messages tanpa reset stats — keep backward compat)
- Reset `self.messages.clear()`
- Reset `self.stream_state = None`
- Reset `self.stats = SessionStats::default()`
- Reset `self.pending_tool_calls.clear()`
- Reset `self.image_attachment = None`
- Generate `self.session_id = Uuid::new_v4()`
- Auto-save session lama ke disk sebelum reset
- Tambah pesan: `"New session started"` di output panel

---

### G-05: `/sessions` — List & Resume

**Priority:** P1 (Core)  
**Files:** `agentic-cli/src/tui/app.rs`, `agentic-cli/src/tui/ui.rs`, `agentic-cli/src/tui/input.rs`  
**Estimate:** 2.5 hari

**Description:**  
List semua saved sessions dan allow resume via `/sessions <id>`.  
Support fuzzy match untuk ID, `Up/Down` navigasi, dan `Enter` untuk resume.

**Reference:** `agentic-cli/src/interactive.rs` → `handle_sessions()`

**Changes:**
- Import `session::{Session, SessionSummary, list, load, format_relative_time}`
- Tambah state ke `App`:
  ```rust
  pub struct SessionView {
      pub summaries: Vec<SessionSummary>,
      pub selected: usize,
      pub filtered: String,
  }
  pub struct App {
      // ...
      pub session_view: Option<SessionView>,
  }
  ```
- `/sessions` tanpa arg → tampilkan tabel session (title, model, time, messages, tokens)
- `/sessions <id>` → resume: load messages via `Session::load()`, replace  
  current conversation, update stats
- `/sessions <partial>` → fuzzy match, jika unik auto-resume
- `/sessions -s <query>` → filter sessions by title/text search
- Handle `Up/Down` navigasi di event loop saat `session_view` aktif
- Render `SessionTable` di `ui.rs` (table widget)
- `Esc` untuk kembali ke normal mode

**UI Layout (pseudo):**
```
┌─ Sessions ─────────────────────────────────────────────┐
│ ID             Title              Model      Time       │
│ > abc123...    Fix login bug      gpt-4o     2h ago     │
│   def456...    Refactor utils     claude-sonnet  5h ago  │
│   ghi789...    Add dark mode      gpt-4o-mini  1d ago    │
│                                                         │
│ ↑/↓ navigate  Enter resume  Esc back                    │
└─────────────────────────────────────────────────────────┘
```

---

### G-06: `/search <query>` — Conversation memory search

**Priority:** P3  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 1 hari

**Description:**  
Search di conversation history (assistant messages) dan tampilkan hasil  
dengan surrounding context.

**Reference:** `agentic-cli/src/commands.rs` → `search_memory_inline()`

**Changes:**
- Search di `self.messages` yang role-nya `"assistant"`
- Case-insensitive substring match
- Tampilkan top 3 hasil dengan surrounding context (±100 chars)
- Render hasil di output panel:
  ```
  ╭─ Search Results for "error handling" ─────────────────╮
  │ Turn 3: ...should handle errors gracefully by         │
  │ using Result types instead of panic...                │
  │ Turn 7: ...error handling is done via custom Error    │
  │ enum in src/error.rs...                               │
  ╰───────────────────────────────────────────────────────╯
  ```
- `/search` tanpa arg → show `"Usage: /search <query>"`

---

### G-07: `/image <path>` — Image attachment

**Priority:** P3  
**Files:** `agentic-cli/src/tui/app.rs`, `agentic-cli/src/tui/ui.rs`  
**Estimate:** 1 hari

**Description:**  
Attach image untuk vision model. Update `image_attachment` state dan show  
indicator di input bar.

**Reference:** `agentic-cli/src/interactive.rs` → `handle_image()` +  
`agentic-cli/src/commands.rs` → `attach_image_inline()`

**Changes:**
- Parse `/image <path>`, validate file exists
- Generate base64 data URL (reuse `attach_image_inline()`)
- Set `self.image_attachment` (state sudah ada)
- Update UI: show attachment badge di input bar (sudah ada di `ui.rs`:  
  `"📷 {basename}"`)
- Support `/image` tanpa arg untuk clear attachment
- Support `/image <partial-path>` dengan file completion (future, depends on G-01)

---

### G-08: `/provider` — Switch provider

**Priority:** P3  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 0.5 hari

**Description:**  
List atau switch provider.

**Reference:** `agentic-cli/src/interactive.rs` → `handle_provider()`

**Changes:**
- `/provider` tanpa arg → tampilkan list provider tersedia + current
- `/provider <name>` → switch provider & reset model ke default model provider
- Update `self.provider` state
- Tambah visual feedback: `"Provider switched to anthropic"`

---

### G-09: `/mcp` — MCP server status

**Priority:** P4  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 0.5 hari

**Description:**  
Show status MCP server yang connected.

**Reference:** `agentic-cli/src/commands.rs` → `show_mcp_status()`

**Changes:**
- Panggil `state.mcp_manager.status()`
- Render MCP server list (name + status) di output panel:
  ```
  ╭─ MCP Servers ─────────────────────────────────────────╮
  │ ✓ filesystem      (connected)                          │
  │ ✓ context7        (connected)                          │
  │ ✗ puppeteer       (not configured)                     │
  ╰───────────────────────────────────────────────────────╯
  ```
- Jika tidak ada MCP configured → show `"No MCP servers configured"`

---

### G-10: `/plan <goal>` — Plan mode

**Priority:** P4  
**Files:** `agentic-cli/src/tui/app.rs`  
**Estimate:** 1 hari

**Description:**  
Quick planning mode — tampilkan structured plan tanpa eksekusi.  
Berguna untuk meminta ide/arahan sebelum implementasi.

**Reference:** `agentic-cli/src/interactive.rs` → `handle_plan()`

**Changes:**
- Buat system prompt untuk planning (mirip di interactive.rs):
  ```
  You are a planning assistant. The user will describe a goal.
  Respond with a structured plan:
  1. Understanding
  2. Approach
  3. Steps (numbered)
  4. Considerations
  Keep it concise.
  ```
- Panggil API dengan streaming (reuse existing streaming flow)
- Render hasil plan di output panel
- Tandai message dengan role `"plan"` atau tambahkan prefix `"📋 Plan:"`

---

### G-11: Context indicator di status bar

**Priority:** P4  
**Files:** `agentic-cli/src/tui/ui.rs`, `agentic-cli/src/tui/app.rs`  
**Estimate:** 0.5 hari

**Description:**  
Tampilkan indikator di status bar jika AGENT.md atau memory.md ada di  
current directory, sama seperti interactive mode.

**Reference:** `agentic-cli/src/interactive.rs` → `render_context_indicator()`

**Changes:**
- Cek `cwd/.agentic.md` + `cwd/AGENT.md` + `cwd/.agentic/memory.md` saat init
- Tambah icon di status bar left section:  
  `"📄 AGENT.md"` / `"🧠 memory.md"`
- Re-check saat `/config` dipanggil (atau saat init)
- Format: `"📄 AGENT.md 🧠 memory.md"` (gabungkan jika keduanya ada)

---

### G-12: Model name auto-complete

**Priority:** P2 (UX)  
**Files:** `agentic-cli/src/tui/input.rs`  
**Estimate:** 0.5 hari  
**Depends on:** G-01

**Description:**  
Saat user ketik `/models gpt-4` → auto-suggest `gpt-4o`, `gpt-4o-mini`, dll.  
Integrasikan dengan completion popup dari G-01.

**Changes:**
- Special handler di completion popup: saat prefix `/models ` → query  
  `self.available_models` untuk fuzzy match
- Render model name + provider info: `"gpt-4o (openai)"`
- Accept on `Tab` / `Enter`

---

## Summary

| ID  | Feature                        | Priority | Est. Hari | Depends On |
|-----|--------------------------------|----------|-----------|------------|
| G-01| `/` command tab completion     | P2 (UX)  | 1.0       | —          |
| G-02| `@` file path completion       | P2 (UX)  | 1.0       | G-01       |
| G-03| `/models <name>` switch        | P1 (Core)| 1.0       | —          |
| G-04| `/new` proper session reset    | P1 (Core)| 0.5       | —          |
| G-05| `/sessions` list & resume      | P1 (Core)| 2.5       | —          |
| G-06| `/search <query>`              | P3       | 1.0       | —          |
| G-07| `/image <path>`                | P3       | 1.0       | —          |
| G-08| `/provider` switch             | P3       | 0.5       | —          |
| G-09| `/mcp` status                  | P4       | 0.5       | —          |
| G-10| `/plan <goal>`                 | P4       | 1.0       | —          |
| G-11| Context indicator              | P4       | 0.5       | —          |
| G-12| Model name auto-complete       | P2 (UX)  | 0.5       | G-01       |
|     | **Total**                      |          | **10.5**  |            |

---

## Recommended Implementation Order

```
Sprint 1 — Core (P1):          G-03 → G-04 → G-05       ~4   hari
Sprint 2 — UX (P2):            G-01 → G-02 → G-12       ~2.5 hari
Sprint 3 — Feature (P3):       G-06 → G-07 → G-08       ~2.5 hari
Sprint 4 — Polish (P4):        G-09 → G-10 → G-11       ~2   hari
```

**Grand Total: ~11 hari** (sekitar 2-3 minggu kerja)

---

## Architectural Notes

### Completion Popup Architecture

The completion system (G-01, G-02, G-12) should share a single popup  
renderer to avoid duplication. Proposed structure:

```rust
// input.rs
pub enum CompletionType {
    SlashCommands(Vec<SlashCompletion>),
    Files(Vec<PathBuf>),
    Models(Vec<(String, String)>),  // (model_name, provider)
}

pub struct CompletionPopup {
    pub r#type: CompletionType,
    pub items: Vec<String>,         // display strings
    pub selected: usize,
    pub prefix: String,             // what triggered it
}
```

```rust
// ui.rs
pub fn render_completion_popup<B: Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    popup: &CompletionPopup,
) {
    // renders a popup overlay at the bottom of the screen,
    // above the input panel
}
```

### SessionView Architecture

The `/sessions` feature (G-05) requires a transient modal-like state in  
`App`. This should NOT block the main event loop — it overlays the output  
panel only:

```rust
// app.rs
impl App {
    pub fn set_session_view(&mut self, view: Option<SessionView>) {
        self.session_view = view;
    }

    pub fn handle_session_selection(&mut self, key: KeyEvent) {
        if let Some(view) = &mut self.session_view {
            match key.code {
                KeyCode::Up => { ... },
                KeyCode::Down => { ... },
                KeyCode::Enter => { /* resume selected */ },
                KeyCode::Esc => { self.session_view = None; },
                _ => {}
            }
        }
    }
}
```

The `handle_key` in `mod.rs` should delegate to `handle_session_selection`  
when `app.session_view.is_some()`, before falling through to normal key  
handlers.

### Reusing Commands Module

Several gaps reuse logic already in `agentic-cli/src/commands.rs`:

| Gap | Reusable Function |
|-----|-------------------|
| G-03 | `switch_model()`, `pick_model_interactive_inline()` |
| G-06 | `search_memory_inline()` |
| G-07 | `attach_image_inline()` |
| G-08 | (uses `switch_model()` internally) |
| G-09 | `show_mcp_status()` |
| G-10 | plan system prompt string |

The TUI should NOT call these functions directly (they're designed for  
interactive mode's I/O). Instead, extract the *data/logic* portions into  
shared helpers and keep the rendering in the TUI.

### State Consistency

When switching models (G-03) or resuming sessions (G-05), the following  
state must stay consistent:

1. `App.provider` ↔ `App.model` ↔ `App.available_models`
2. `App.session_id` ↔ `App.messages` ↔ `App.stats`
3. `App.image_attachment` must be cleared on `/new`

---

## Acceptance Criteria

- [ ] G-01: User dapat ketik `/` + `Tab` untuk melihat semua command
- [ ] G-02: User dapat ketik `@` + `Tab` untuk auto-complete file path
- [ ] G-03: User dapat switch model dengan `/models gpt-4o`
- [ ] G-04: `/new` mereset semua state (messages, stats, attachment, session)
- [ ] G-05: User dapat list dan resume session sebelumnya
- [ ] G-06: User dapat search conversation history dengan `/search`
- [ ] G-07: User dapat attach image dengan `/image <path>`
- [ ] G-08: User dapat switch provider dengan `/provider <name>`
- [ ] G-09: User dapat melihat status MCP server dengan `/mcp`
- [ ] G-10: User dapat membuat plan dengan `/plan <goal>`
- [ ] G-11: Status bar menunjukkan AGENT.md / memory.md yang aktif
- [ ] G-12: `/models ` diikuti partial name memberikan auto-suggest
