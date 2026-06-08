# Ratatui Input Widget — Design Concept

## Latar Belakang

Saat ini REPL menggunakan **reedline** untuk input, dengan **InputWatcher** (crossterm raw mode) untuk menangkap ketikan selama agent processing. Arsitektur ini bermasalah:

1. **Dua mode input berbeda** — reedline (cooked) vs InputWatcher (raw), rawan konflik
2. **Cursor management kompleks** — MoveUp/MoveDown antar fase sering tidak balance
3. **Fleksibilitas layout terbatas** — metadata (model, branch) hanya bisa di atas/kanan prompt, tidak bebas
4. **Rendering terpecah** — reedline punya render sendiri, spinner/output via inline.rs, metadata via components.rs

## Konsep

Ganti reedline dengan **custom input widget berbasis ratatui** yang dirender ke stdout via `inline.rs` (tanpa alternate screen). Satu sistem rendering untuk semua komponen.

```
┌─────────────────────────────────────────────────────────┐
│  ⚡ sumopod/MiniMax (input widget - inline render)       │
│  agentic> █                                              │
│  📌 dev                                                  │
├─────────────────────────────────────────────────────────┤
│  (spinner / output / conversation — juga via inline)     │
└─────────────────────────────────────────────────────────┘
```

## Arsitektur

### Fase

Hanya ada **1 fase input** (tidak seperti sekarang yang pecah):

| Fase | Input | Output |
|------|-------|--------|
| **Idle** | Widget input aktif | Menampilkan prompt + metadata |
| **Processing** | Widget input non-aktif (display-only) | Menampilkan spinner |
| **Streaming** | Widget input non-aktif | Menampilkan teks streaming |

### Komponen

#### 1. `InputBuffer` — state input

```
struct InputBuffer {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
}
```

Method:
- `insert_char(c)`, `delete_backward()`, `delete_forward()`
- `cursor_left()`, `cursor_right()`, `home()`, `end()`
- `history_up()`, `history_down()`
- `submit() -> String` — push ke history, return text

#### 2. `InputRenderer` — render input ke ratatui Line

```
fn render_input_line(buffer: &InputBuffer, metadata: &Metadata) -> Line<'static>
```

Output:
```
Line 1: metadata (model, branch, dll — bisa di atas/bawah)
Line 2: prompt_text + input_text + cursor
```

Syntax highlighting (sudah ada di `tui/input.rs`):
- `/command` → kuning
- `@file` → biru
- Cursor → block putih

#### 3. Event Loop — single source of truth

```
loop {
    // 1. Render semua komponen via inline.rs
    render_layout(&state);
    
    // 2. Baca key event (crossterm raw mode)
    match read_key() {
        Key::Char(c) => state.input.insert_char(c),
        Key::Enter => submit_task(&mut state),
        Key::Backspace => state.input.delete_backward(),
        Key::Esc => if state.is_processing { cancel() },
        Key::Up => state.input.history_up(),
        // ...
    }
}
```

### Layout

Kontrol penuh atas posisi metadata:

```
// Opsi 1: Metadata di bawah input
agentic> who are you?
⚡ sumopod/MiniMax-M2.7-highspeed 📌 dev
─────────────────────────────────────────

// Opsi 2: Metadata di samping kanan
agentic> who are you?  │  ⚡ sumopod/MiniMax 📌 dev

// Opsi 3: Metadata di atas
⚡ sumopod/MiniMax-M2.7-highspeed 📌 dev
agentic> who are you?
```

### Integrasi dengan Existing Code

| Existing | Pengganti |
|----------|-----------|
| `reedline::Reedline` | `InputBuffer` + `InputRenderer` |
| `reedline::Prompt` | `render_input_line()` |
| `AgenticCompleter` | Same — panggil untuk `/` dan `@` completion |
| `AgenticHighlighter` | Same — apply di `render_input_line()` |
| `AgenticHinter` | Optional — tampilkan sebagai ghost text |
| `InputWatcher` | Dihapus — tidak perlu |
| `render_two_line_transient` | Dihapus — spinner standalone |
| `inline.rs` | Same — render semua Line ke stdout |
| `commands.rs` ticker | Simplifikasi — hanya spinner |

## Keuntungan

1. **Satu fase input** — tidak ada konflik reedline vs raw mode
2. **Layout bebas** — metadata bisa di mana saja
3. **Kode lebih sederhana** — hapus InputWatcher, render_two_line_transient, watcher_state
4. **Stabil** — tidak ada cursor management yang rumit antar fase

## Tantangan

1. **History persistence** — perlu rebuild (reedline support SQLite)
2. **Multi-line editing** — perlu handle sendiri
3. **Mouse support** — reedline support click to position cursor
4. **Completion popup** — perlu rebuild dari `AgenticCompleter`
5. **Waktu implementasi** — estimasi 2-3 hari untuk feature parity dasar

## Rekomendasi

Implementasi bertahap:
1. **Phase 1** — Basic input (single line, history in-memory, enter submit)
2. **Phase 2** — Completion popup (`/` dan `@`)
3. **Phase 3** — History persistence (SQLite atau file)
4. **Phase 4** — Syntax highlighting + hints
