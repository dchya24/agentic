# Agentic CLI — Diskusi Arsitektur Input & Metadata

Dokumen ini mencatat diskusi teknis seputar arsitektur input, rendering metadata, dan simplifikasi queuing pada CLI agentic.

---

## 1. Latar Belakang

Berdasarkan dokumen mockup (`docs/2026-06-08-mockup.md`), ada 3 pertanyaan desain:

### Q1 — Metrik redundant di status bar

```text
 ⚡ sumopod/MiniMax-M2.7-highspeed  |  💬 0 msgs  |  📊 0 ↑ / 0 ↓  |  ⏱️ 0s
```

Metrik `💬 msgs`, `📊 tokens`, `📦 cache`, `⏱ waktu` muncul di **3 tempat**: banner welcome, status bar sebelum prompt, dan footer response.

**Keputusan:** Hapus `💬 msgs`, `⏱ waktu`, `📦 cache` dari semua status bar. Hanya tampilkan `📊 X↑/X↓` (token) bersama model info.

### Q2 — Metadata di bawah prompt

Mockup menempatkan `sumopod MiniMax-M2.7-highspeed 📌 dev` di sisi kanan prompt. Permintaan: pindahkan ke **bawah** prompt.

**Keputusan:** Implementasi via `print_turn_separator()` yang mencetak model info + git branch di bawah prompt setelah user submit input.

### Q3 — Prompt menghilang saat Thinking

Selama processing, muncul `Thinking...` + spinner, tapi area input seolah menghilang.

**Penyebab:** Ticker menggunakan `print_line` (spinner + newline) + `print_transient` (spinner duplikat) + prompt transient. Balance cursor tidak tepat, menyebabkan:
- Double spinner line
- Cursor creep (setiap tick spinner naik 1 baris, menimpa separator/model info)

---

## 2. Perubahan yang Dilakukan

### Q1 — Hapus Metrik Redundant

**File: `interactive.rs`**

| Fungsi | Sebelum | Sesudah |
|--------|---------|---------|
| `print_status_bar()` | `⚡ model │ 💬 X │ 📊 X↑/X↓ │ 📦 X% │ ⏱ Xs` | `⚡ model │ 📊 X↑/X↓` |
| `print_prompt_status_bar()` | `📂 dir │ ⚡ model │ 💬 X │ 📊 X↑/X↓ │ ⏱ Xs` | `📂 dir │ ⚡ model │ 📊 X↑/X↓` |
| `print_response_summary()` | `✓ done │ ⏱ X.XXs │ 📊 X↑/X↓ │ 📦 X%` | `✓ done │ 📊 X↑/X↓` |

### Q2 — Model Info di Bawah Prompt

**File: `interactive.rs`**

`print_turn_separator()` diubah dari no-op menjadi:
```
  ⚡ sumopod/MiniMax-M2.7-highspeed 📌 dev
```

Ditambahkan dotted separator + blank line setelahnya sebelum spinner:
```
· · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · ·

  ⠇ Thinking...  [████░░░░░░░░░░░░]
```

### Q3 — Perbaikan Ticker & Transient Prompt

**File: `commands.rs`**

| Issue | Penyebab | Fix |
|-------|----------|-----|
| Double spinner | `print_line` + `print_transient` cetak spinner di 2 baris | Hapus `print_transient(spinner_line)` dari `render_two_line_transient` |
| Cursor creep | `MoveUp(1)` tidak balance ketika buffer kosong | `MoveToColumn(0)` tanpa `MoveUp` saat tidak ada konten |
| Prompt tiba-tiba muncul | `render_two_line_transient` mendeteksi buffer tidak kosong | Perbaiki logic agar selalu balance antara `\n` dan `MoveUp` |

---

## 3. Opsi A — Simplifikasi (Dihapus: InputWatcher, Queuing, ESC Abort)

### Masalah

Setelah Q3 fix, masih ada issue:

1. **Double "Thinking..."** — Karena `print_line` + `print_transient` mencetak spinner di 2 baris
2. **Prompt menghilang saat streaming** — `ClearFromCursorDown` menghapus transient prompt
3. **Typing muncul sebagai queued message** — InputWatcher merekam ketikan, tapi tidak visible sampai streaming selesai
4. **Metadata muncul di bawah input setelah Enter** — Lompatan visual saat `print_turn_separator()` dijalankan

### Keputusan: Opsi A — Hapus InputWatcher

**Tidak ada queuing.** User tidak bisa mengetik selama agent processing. Tunggu sampai selesai.

### Perubahan

| File | Perubahan |
|------|-----------|
| `input_watcher.rs` | **Dihapus** (258 baris) |
| `main.rs` | Hapus `mod input_watcher` |
| `commands.rs` | Hapus `watcher_state` field + `with_watcher_state()` |
| `commands.rs` | Hapus `tick_watcher` dari ticker |
| `commands.rs` | Hapus `render_two_line_transient()` |
| `commands.rs` | Hapus `strip_ansi_len()` |
| `interactive.rs` | `process_message()` rewrite — tanpa watcher, cancel, pending queue |
| `interactive.rs` | REPL loop simplifikasi — tanpa `pending` loop |

### Yang Hilang

- ❌ **Queuing** — tidak bisa mengetik selama agent running
- ❌ **ESC abort** — tidak bisa cancel dengan ESC (Ctrl+C tetap berfungsi via cancel_flag)
- ❌ **Live input preview** — spinner standalone, tanpa prompt di bawah

### Yang Tetap

- ✅ **reedline** — REPL input normal sebelum/sesudah agent
- ✅ **Spinner animation** — `⠇ Thinking... [████░░]`
- ✅ **Model info di bawah prompt** — via `print_turn_separator()`
- ✅ **Dotted separator** — antara prompt dan spinner
- ✅ **Response summary** — `✓ done  │  📊 X↑/X↓`

---

## 4. Ratatui Input Widget — Design Concept

Diskusi tentang kemungkinan mengganti reedline dengan custom input widget berbasis ratatui.

### Masalah dengan reedline

1. **Dua mode input** — reedline (cooked) vs InputWatcher (raw), rawan konflik
2. **Cursor management** — MoveUp/MoveDown antar fase tidak balance
3. **Layout terbatas** — metadata hanya bisa di atas/kanan prompt
4. **Rendering terpecah** — reedline, inline.rs, components.rs

### Konsep

Satu sistem rendering (ratatui → inline.rs) untuk semua komponen:

```
  ⚡ sumopod/MiniMax-M2.7-highspeed 📌 dev
  agentic> █
─────────────────────────────────────────────
  (spinner / output / conversation)
```

### Komponen yang Diperlukan

| Komponen | Status |
|----------|--------|
| Render input + cursor | ✅ Ada di `tui/input.rs` |
| Capture keystrokes | ✅ Ada di `InputWatcher` (raw mode) |
| Syntax highlighting | ✅ Ada di `tui/input.rs` |
| History (↑/↓) | ❌ Perlu rebuild (reedline pakai SQLite) |
| Tab completion | ✅ Ada di `AgenticCompleter` |
| Multi-line editing | ❌ Perlu rebuild |
| Completion popup | ❌ Perlu rebuild dari `tui/dropdown.rs` |

### Estimasi

Feature parity dasar dengan reedline: **2-3 hari**.

### Dokumen Terpisah

Lihat `docs/ratatui-input-widget.md` untuk detail desain lebih lanjut.

---

## 5. Status Akhir

```
Opsi A diimplementasikan ✅
- input_watcher.rs dihapus
- commands.rs: watcher_state, render_two_line_transient dihapus
- interactive.rs: process_message disederhanakan
- Kompilasi bersih, 0 error

ratatui-input-widget.md dibuat ✅
- Design concept untuk future reference
- Analisis komponen yang diperlukan
- Estimasi implementasi
```
