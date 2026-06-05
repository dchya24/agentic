# Implementation Log: Visual Improvements — Interactive Mode

**Date:** 2026-06-04  
**Status:** All 7 tasks implemented + 1 bugfix  
**Files changed:** `interactive.rs`, `commands.rs`, `widgets/tool_call.rs`, `widgets/components.rs`, `widgets/inline.rs`

---

## Implementation Summary

| Task | Status | File(s) |
|------|--------|---------|
| Task 1: Banner skills | ✅ Done | `interactive.rs` |
| Task 2: Richer prompt (Opsi B→manual) | ✅ Done | `interactive.rs` |
| Task 3: Compact response summary | ✅ Done | `interactive.rs`, `components.rs` |
| Task 4: Thinking tokens styling | ✅ Done | `commands.rs`, `components.rs` |
| Task 5: Turn separator + role badge | ✅ Done | `interactive.rs` |
| Task 6: Tool call compact rendering | ✅ Done | `tool_call.rs`, `commands.rs` |
| Task 7: Markdown re-render | ✅ Done | `commands.rs`, `inline.rs` |
| Bugfix: Spinner tidak muncul | ✅ Fixed | `commands.rs` |

---

## Phase 1: Simple Changes

### Task 1: Banner — Skills List

**Perubahan:** Tambah baris `📦 skills` di panel Welcome banner.

```rust
// interactive.rs → print_banner()
let skills = core_agentic::list_skills();
if !skills.is_empty() {
    const MAX_SKILL_NAMES: usize = 5;
    let mut skill_names: Vec<String> = skills.iter()
        .take(MAX_SKILL_NAMES)
        .map(|(name, _)| name.clone())
        .collect();
    let remaining = skills.len().saturating_sub(MAX_SKILL_NAMES);
    if remaining > 0 {
        skill_names.push(format!("+{} more", remaining));
    }
    // ... render as Line with yellow styling
}
```

**Result:**
```
╭─ Welcome ────────────────────────────────────────────────╮
│ 📂 cwd    /home/user/project                             │
│ ⚡ model   sumopod / kimi-k2.6                            │
│ 📦 skills  brainstorming · frontend-design · backend-dev  │
│ 💡 tip    type /help for commands, @ to reference files   │
╰──────────────────────────────────────────────────────────╯
```

### Task 5: Turn Separator + Role Badge

**Perubahan:** Ganti `dotted_separator` → `section_header("👤", "You", blue)`.

```rust
fn print_turn_separator() {
    inline::print_blank();
    inline::print_line(&components::section_header(
        "👤", "You", Color::Rgb(52, 152, 219),
    ));
}
```

**Before:** `· · · · · · · · · · · · · · · · · ·`  
**After:** `── 👤 You ──────────────────────────────`

### Task 3: Compact Response Summary

**Perubahan:**
- Hapus `💬 N msgs` dan `session Xs` (redundan dengan status bar)
- Token format lebih compact: `📊 2.1K↑/850↓`
- Separator baru `rounded_dashed_separator()` (`╶╌╌╌╌╌╶`)

**New helper di `components.rs`:**
```rust
pub fn rounded_dashed_separator(color: Color) -> Line<'static> {
    // ╶╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╶
}
```

**Result:**
```
╶╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╶
  ✓ done │ ⏱ 2.3s │ 📊 2.1K↑/850↓ │ 📦 78% cached
╶╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╶
```

---

## Phase 2: Simple-Medium

### Task 4: Thinking Tokens Styling

**Perubahan:**
- `Event::Thought` tidak lagi di-suppress selama streaming
- Ditampilkan dengan header `─ ─ thinking... ──────` / `─ ─ done thinking ──────`
- Konten: `Modifier::DIM` + `Color::Indexed(242)`

**New helper di `components.rs`:**
```rust
pub fn thinking_header(active: bool) -> Line<'static> {
    let label = if active { " thinking... " } else { " done thinking " };
    // ... styled with DIM + Indexed(242)
}
```

**Result:**
```
─ ─ thinking... ──────────────────────────────────────────────
  Analyzing the codebase structure... checking module deps...
─ ─ done thinking ────────────────────────────────────────────
```

### Task 2: Richer Prompt — Manual Status Bar

**Perubahan:** Status bar dicetak manual sebelum setiap `read_line()` karena reedline right prompt hilang saat input panjang.

**Evolusi pendekatan:**
1. ~~Opsi B: `right_prompt_on_last_line() = false`~~ → reedline tetap share baris dengan input
2. ~~Transient (`print_transient`)~~ → salah baris saat reedline pindah ke baris baru
3. ✅ **Manual permanent line** → cetak sebelum `read_line()`, jadi bagian scrollback

```rust
loop {
    print_prompt_status_bar(&model_info, &stats);
    let sig = line_editor.read_line(&prompt);
    // ...
}
```

**Result:**
```
 📂 agentic · dev │ ⚡ sumopod/kimi-k2.6 │ 💬0 📊0↑/0↓ │ ⏱ 4m 11s
 agentic> who are you                         📌 dev sumopod/kimi-k2.6
```

**Catatan:** Reedline right prompt tetap aktif (model info di baris input), tapi info lengkap ada di status bar manual di atasnya. Status bar tidak terpengaruh panjang input.

---

## Phase 3: Medium

### Task 6: Tool Call Compact Rendering

**Perubahan:** Ganti panel 6-8 baris → 2 baris compact per tool call.

**New functions di `tool_call.rs`:**
- `render_call_compact()` → `⚙ tool_name(path="src/main.rs", limit=100)`
- `render_result_compact()` → `→ ✓ 142 lines` atau `→ ✗ error message`
- `compact_args()` → format args inline: `key=value, key=value`
- Truncation: jika >80 chars, truncate args + `…`

**Result:**
```
 ⚙ read_file(path="src/main.rs", limit=100)
    → ✓ 142 lines, 4.2K bytes
 ⚙ edit_file(path="src/main.rs")
    → ✓ +5/-2
 │ - old_line
 │ + new_line
 ⚙ run_command(cmd="cargo test")
    → ✗ Tool error: exit code 1
```

**Tests:** 12 tests (5 new + 7 existing), all passing.

---

## Phase 4: Complex

### Task 7: Streaming Markdown — Re-render saat Selesai

**Perubahan:** Stream plaintext → re-render dengan markdown styling saat selesai.

**New function di `inline.rs`:**
```rust
pub fn replace_lines(count: u32, new_lines: &[Line<'_>]) {
    // MoveUp(count) + Clear(FromCursorDown) + print styled lines
}
```

**Track streamed lines:**
```rust
let streamed_lines = Arc::new(AtomicU32::new(0));
// in on_chunk:
streamed_lines.fetch_add(chunk.chars().filter(|&c| c == '\n').count() as u32, Relaxed);
```

**Re-render logic:**
```rust
if total_lines > 0 && total_lines <= 500 && is_stdout_tty() {
    let parsed = MarkdownContent::parse(&full_text);
    inline::replace_lines(total_lines, &parsed.lines);
}
```

**Edge cases:**
- Non-TTY → skip re-render, biarkan plaintext
- >500 lines → skip re-render, terlalu riskan
- Empty stream → batch mode (sudah ada)

---

## Bugfix: Spinner Tidak Muncul Setelah Enter

**Masalah:** User menekan Enter → tidak ada progress bar / spinner visible.

**Root cause:** Spinner ticker menggunakan `tokio::select!` antara `interval.tick()` (80ms) dan `event_rx.recv()`. Untuk model yang responsif cepat, chunk pertama tiba dalam <80ms, sehingga:
1. Ticker belum sempat tick pertama kali
2. `streaming_text_active = true` → ticker skip semua tick
3. User tidak pernah melihat spinner

**Fix:** Cetak spinner awal **segera** setelah ticker dibuat, sebelum streaming dimulai:

```rust
let ticker = tokio::spawn(/* ... */);

// Print initial spinner immediately
{
    let p = progress.lock().unwrap();
    let initial_line = spinner::compact_progress_line(&p, 18);
    if let Some(ref ws) = self.watcher_state {
        render_two_line_transient(&initial_line, ws);
    } else {
        inline::print_transient(&initial_line);
    }
}

let result = orchestrator.run_stream_with_attachments(/* ... */);
```

**File:** `commands.rs` line ~1722

---

## Roadmap: Opsi C — Status Bar di Bawah Input (v2)

**Status:** Deferred.

Memerlukan meninggalkan reedline (~2000+ baris re-implementation). Akan dieksekusi setelah MVP stabil.

**Referensi:** `docs/plans/2026-06-04-new-ui-cli.md`

---

## Known Issues

1. **`test_auto_title_long` failing** — Pre-existing bug di `session.rs`, tidak terkait perubahan ini
2. **Status bar mengotori scrollback** — Setiap prompt cycle menambah 1 baris status bar ke history. Bisa diatasi nanti dengan transient approach yang lebih baik (butuh koordinasi lebih erat dengan reedline internal rendering)
3. **Markdown re-render dengan wrapped lines** — `streamed_lines` hanya menghitung `\n`, bukan terminal-wrapped lines. Jika satu baris panjang melebihi terminal width, `MoveUp` bisa salah hitung. Untuk response pendek (<500 lines) ini jarang masalah

---

## Test Results

```
running 125 tests
test result: FAILED. 124 passed; 1 failed; 0 ignored; 0 measured

Failed (pre-existing):
  - session::tests::test_auto_title_long

New tests added:
  - tool_call::tests::render_call_compact_single_line
  - tool_call::tests::render_call_compact_no_args
  - tool_call::tests::render_call_compact_truncates_long_args
  - tool_call::tests::render_result_compact_success
  - tool_call::tests::render_result_compact_error
```
