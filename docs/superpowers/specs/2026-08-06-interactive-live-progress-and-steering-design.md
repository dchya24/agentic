# Design: Interactive Live Tool Progress + Steering (+ Parallel Lanes)

**Date**: 2026-08-06
**Status**: Approved (brainstorming)
**Next step**: implementation plan (`writing-plans`)

## Ringkasan

Meningkatkan interactive mode `agentic` dari "blokir sampai selesai" menjadi **responsif**: per-tool progress dengan live output, dan steering (menyela agent yang sedang jalan). Terinspirasi dari paritas dengan `pi` (lihat `docs/COMPARISON_PI_VS_AGENTIC.md`) — gap nomor 1 (steering) dan gap event-granularity.

Tiga fase, masing-masing rilis sendiri:

1. **Fase 1 — Tool lifecycle & live output** (prioritas C: progress per-tool). Orchestrator emit `ToolStart`/`ToolDelta`/`ToolOutput`(kaya); `run_command`/`run_script` di-streaming; renderer inline menggambar tool card.
2. **Fase 2 — Steering & REPL non-blokir** (prioritas A: menyela). REPL tidak blokir saat run; prompt line hidup; Enter → steer queue; Ctrl+C → cancel graceful; slash command ditunda.
3. **Fase 3 — Paralel lanes** (prioritas D). Sketsa; multi-lane via subagent; fondasi dari Fase 2.

Motivasi ganda: kenyamanan pemakaian harian (A) + kedalaman arsitektur sejajar pi (B).

---

## Konteks arsitektur saat ini

```
interactive.rs::process_message
  → conversation.push(user)
  → commands.run(input).await        ← BLOKIR: REPL mati sampai run selesai
       → orchestrator.run_stream(input, on_chunk)
            → loop: LLM call → tool batch → next LLM call ...
       → renderer thread (single-writer stdout):
            spinner + tool panel (Event::ToolCall/ToolOutput) + teks streamed
```

Fakta kunci dari kode:

- `Tool` trait (`core-agentic/src/tool.rs`): `execute(&self, args) -> ToolResult<Value>` — atomik, tanpa konteks.
- `run_command` (`tools/run_command.rs`): `wait_with_output()` — semua output ditangkap setelah proses selesai.
- `tool_exec.rs`: emit `Event::ToolCall` sebelum eksekusi, `Event::ToolOutput` sesudah. Tanpa durasi/progress.
- `Event` (`events.rs`): enum flat — `Thought/ToolCall/ToolOutput/ConfirmationRequest/Error/Completed/System/PlanProgress/PlanReplanned`.
- Renderer (`commands.rs::run`, ~1584+): thread `agentic-renderer`, single-writer, state machine "spinner selalu baris terakhir"; konsumsi `chunk_rx` + `event_rx` via channel.
- Interactive (`interactive.rs`): `repl_loop` baca crossterm keys (raw mode), gambar prompt via `input_buffer`/`input_renderer`; `process_message` meng-await run.
- Cancel: `CANCEL_FLAG` global (`Arc<AtomicBool>`), dipakai orchestrator di setiap turn boundary, `AgenticError::Cancelled`.
- `Commands` menyimpan `orchestrator: Option<Orchestrator>` (by value).

---

## Fase 1 — Tool lifecycle & live output

### 1.1 `core-agentic` — Event baru

Perluas `Event` di `events.rs`:

```rust
ToolStart  { tool_call_id: String, tool_name: String, arguments: Value } // baru
ToolDelta  { tool_call_id: String, tool_name: String, delta: String }    // baru
ToolOutput { tool_name, output, error,                                    // ada,
             tool_call_id: String,      duration_ms: u64,                 // + baru
             success: bool, truncated: bool }                             // + baru
```

- `ToolOutput` diperkaya (penambahan field, bukan breaking untuk pemakaian kita).
- Tiap event membawa `tool_call_id` supaya renderer memisahkan buffer per tool (penting untuk eksekusi paralel).

### 1.2 `core-agentic` — Hook streaming di `Tool` trait

```rust
fn execute(&self, args: Value) -> ToolResult<Value>;   // tetap (fallback)

/// Default: panggil `execute`, abaikan callback — semua tool lama TIDAK
/// berubah perilaku.
fn execute_streaming(&self, args: Value,
                     on_progress: &dyn Fn(&str)) -> ToolResult<Value> {
    self.execute(args)
}
```

- `run_command` dan `run_script` override: ganti `wait_with_output()` → spawn child dengan stdout/stderr piped, baca per baris (atau per chunk saat tidak ada newline), tiap bagian → `on_progress(line)`, akumulasi buffer, kembalikan `{stdout, stderr}` (kontrak JSON lama dipertahankan).
- Binary output / no-newline: kirim per chunk tetap (mis. 4KB) sebagai delta.
- Tool lain (read/grep/list/dll): tetap atomik, pakai default.

### 1.3 `core-agentic` — `tool_exec.rs` (path sync + async)

```
Sebelum eksekusi : emit ToolStart(tool_call_id, name, args)
Saat eksekusi    : on_progress = |delta| emit ToolDelta(...)   // di-rate-limit
Setelah eksekusi : ukur durasi → emit ToolOutput(+duration_ms, success, truncated)
```

- Berlaku di `handle_tool_calls` (sync) dan `handle_tool_calls_parallel` (async).
- Kontrak LLM/memory TIDAK berubah: hasil final tetap di-truncate oleh `tool_result_max_chars`; delta hanyalah observability.
- **Rate-limit delta**: coalesce ~80ms per tool (akumulasi, flush per interval) — melindungi channel dan renderer dari spam log (mis. `tail -f`).

### 1.4 `agentic-cli` — renderer inline (Opsi 2)

Renderer thread yang sudah ada diperluas:

```
ToolStart   → "⟳ run_command cargo test"
ToolDelta   → delta (indent, warna stdout/stderr), akumulasi per tool_call_id
ToolOutput  → "✓ run_command cargo test — 42.1s" / "✗ … (partial output)"
              truncation guard: buffer > ~4000 chars → "… +N lines truncated"
```

- State machine single-writer dipertahankan (spinner baris terakhir; output tool dicetak di atas, spinner di-re-arm sesudahnya).
- Interleaving paralel: delta di-buffer per `tool_call_id`; dicetak dengan prefix nama tool saat tiba; output final tetap urutan sumber.
- Gated oleh `output.show_tool_calls` yang sudah ada (tidak ada config baru).
- Tidak ada perubahan REPL loop di Fase 1.

### 1.5 Scope Fase 1

- ✅ Tool status + durasi + output live untuk `run_command`/`run_script`
- ✅ Renderer inline
- ❌ Steering/REPL non-blokir (Fase 2), paralel lane (Fase 3)
- ❌ Live output untuk tool selain command/script

---

## Fase 2 — Steering & REPL non-blokir

### 2.1 `core-agentic` — steering queue di `Orchestrator`

```rust
steer_queue: Mutex<VecDeque<String>>,

pub fn steer(&self, text: String) { /* thread-safe push */ }
```

- Di loop `run`/`run_stream`: **setelah tool batch selesai, sebelum build request berikutnya** → drain queue → `memory.add_message(Message::user(text))` per pesan.
- Efek: pesan steering masuk context LLM + memory + session (persist) — sejajar `getSteeringMessages` di pi.
- Berlaku di kedua path (sync `run` dan async `run_stream`).
- Semantik lengkap `steer()`: selalu enqueue. Run yang sedang aktif mendrain di setiap turn boundary; bila tidak ada run aktif (atau pesan tiba setelah turn terakhir), pesan didrain **di awal run berikutnya sebagai user message pertama**. Tidak pernah hilang, tidak pernah duplikat. (Di interactive, `steer()` hanya dipanggil saat state Running, jadi jalur idle tidak terpakai dari UI.)

### 2.2 `agentic-cli` — REPL tidak blokir

`process_message` dipecah:

```
process_message(input):
  1. prepare_run()               // &mut, sinkron:
     - ensure_orchestrator, expand @refs, drain attachments, cek vision
     - buat channel events + subscribe orchestrator
     - spawn RunRenderer (2.3) dengan RunInputState
     - snapshot Arc<Orchestrator>
  2. tokio::spawn { orchestrator.run_stream(input, on_chunk) }  // tidak di-await
  3. return → REPL loop lanjut baca keys, state = Running
```

**`Commands` menyimpan `Arc<Orchestrator>`** (bukan `Option<Orchestrator>`), supaya task run punya clone sementara REPL bisa `commands.steer()` (`&self`, tanpa konflik `&mut`).

### 2.3 Terminal ownership (keputusan paling menentukan)

Pola yang ada dipertahankan (idle → REPL gambar prompt; running → renderer gambar semua), plus: **selama running, renderer juga menggambar prompt line**.

- **`RunRenderer`** diekstrak dari `commands::run` ke modul baru `agentic-cli/src/run_ui.rs`. Dipakai bareng: `run` one-shot (tanpa prompt line) dan interactive (dengan prompt line).
- **`RunInputState`** (Arc\<Mutex\>): `buffer`, `cursor`, `pending_steer: Vec<String>`, `pending_commands: VecDeque<String>`, `running: bool`.
- REPL **hanya membaca keys + mutasi shared state** — tidak pernah menulis stdout selama run. Tidak ada signal redraw eksplisit: renderer membaca `RunInputState` pada tick 80ms yang sudah ada, sehingga buffer/cursor/pending yang diubah REPL otomatis terlihat di redraw berikutnya.
- Renderer: setiap menulis output → redraw prompt line dari `RunInputState` (pada tick 80ms).

### 2.4 Interaksi saat Running

| Input | Perilaku |
|---|---|
| Teks + **Enter** | push `pending_steer` → `orchestrator.steer()`, buffer dikosongkan, redraw |
| Enter (kosong) | abaikan |
| **Ctrl+C** | cancel graceful (turn boundary); hasil parsial disimpan; prompt kembali |
| **Esc** | kosongkan buffer |
| **Slash command** | masuk `pending_commands` — **ditunda** |

### 2.4b Slash command ditunda (keputusan user)

- Input diawali `/` + dikenali parser → `pending_commands` (bukan steer, bukan ditolak). Teks biasa → steer.
- Run selesai (atau di-cancel) → REPL kembali Idle → drain `pending_commands` FIFO lewat jalur slash command normal (aman, karena run sudah tidak memegang `&mut Commands`).
- Bekerja untuk semua command existing (`/models`, `/new`, `/quit`, `/search`, dst).
- V2 (opsional): command read-only (`/help`, `/stats`, `/tools`) dieksekusi seketika — v1 mendahulukan kesederhanaan: semua ditunda.

### 2.5 Persistensi partial (bonus)

Saat cancel mid-run: simpan konversasi parsial ke session (user prompt, tool results, teks partial dari accumulator `on_chunk`) — sekaligus memperbaiki crash-safety (pi persist per-message; kita sekarang menyimpan lebih awal).

### 2.6 Race & error handling Fase 2

- Steer di akhir run → jadi prompt run berikutnya (lihat 2.1).
- Cancel mid-tool → output parsial tool + teks partial disimpan; prompt dikembalikan; pending commands tetap dieksekusi.
- Deferred command gagal (`/models unknown`) → badge error normal, lanjut, bukan crash.

---

## Fase 3 — Paralel lanes (sketsa)

- **Model**: multi-lane, bukan multi-tool. Tiap lane = subagent (`SpawnSubagentTool` yang sudah ada) dengan context terisolasi, share tool registry + cancel flag.
- **Trigger** (belum diputuskan):
  - a) Perintah baru `/parallel <task1> | <task2>`, atau
  - b) Deteksi otomatis dari satu prompt via planner.
- **Rendering**: tiap lane prefiks warna + buffer sendiri; satu lane "fokus" default, yang lain kolaps; renderer sudah punya kemampuan prompt line + status tool dari Fase 1–2.
- **Non-goals**: tanpa sinkronisasi antar-lane (menunggu hasil lane lain = urusan planner); tanpa memory sharing/prioritization v1.

Fondasi dari Fase 2 (run non-blokir + `RunRenderer`) membuat penambahan N lane = N task + N buffer event.

---

## Error handling (lintas fase)

| Kasus | Perilaku |
|---|---|
| Delta flood (log spam) | Coalesce ~80ms per tool di orchestrator |
| Tool gagal di tengah streaming | `ToolOutput` dengan `error` + output parsial; `✗ tool — 12.4s (partial output)` |
| Delta raksasa tanpa newline | Renderer pecah per chunk 4KB + truncation guard |
| Steer di akhir run | Diteruskan sebagai prompt run berikutnya |
| Cancel mid-tool | Partial disimpan ke session; prompt kembali; pending commands dieksekusi |
| Deferred command gagal | Badge error, lanjut |
| Terjadi di tool yang tidak stream | Default `execute`, tanpa delta — tidak ada regresi |

## Backward compatibility

| Perubahan | Dampak |
|---|---|
| `Event` +variant baru, `ToolOutput` +field | Exhaustiveness di kompilator memaksa match diperiksa; kita satu-satunya consumer |
| `Tool::execute_streaming` default = `execute` | Semua tool lain tidak berubah; JSON `run_command` identik |
| `Commands`: `Option<Orchestrator>` → `Arc<Orchestrator>` | Refactor internal; permukaan CLI tidak berubah |
| Tidak ada flag CLI baru | Live output di-gate `output.show_tool_calls` |

## Rollout

```
Fase 1 (C: progress + live output)  → release v0.4.0
Fase 2 (A: steer + REPL non-blokir) → release v0.5.0
Fase 3 (D: paralel)                 → release v0.6.0
```

Tiap fase independen; regression dicek test suite `orchestrator_loop.rs` existing + test baru per fase.

## Testing strategy

- **Fase 1**
  - Unit: default `execute_streaming` = `execute` (tool lama tidak berubah); `run_command` streaming menghasilkan delta + hasil JSON sama.
  - Integration (`tests/orchestrator_loop.rs`): mock tool dengan `execute_streaming` → urutan event `ToolStart → ToolDelta* → ToolOutput(+duration_ms, truncated)`.
  - Renderer: unit test aliran event → output yang diharapkan.
- **Fase 2**
  - Unit: drain FIFO; steer saat tidak ada run aktif → pesan menjadi user message pertama run berikutnya; steer di tengah run → muncul di memory sebagai user message antar-turn.
  - Integration: mock provider dengan tool calls; `steer()` di tengah run → request LLM berikutnya berisi pesan steering → run selesai normal.
  - Renderer + interactive: smoke test manual.
- **Fase 3**: test E2E per lane (reuse `planner_loop.rs` / `orchestrator_loop.rs` patterns).

## Dokumen yang akan di-update

- `docs/ROADMAP.md` — tandai fase-fase.
- `tasks/tasks-core-agentic.md` + `tasks/tasks-agentic-cli.md` — task baru per fase.
- `docs/COMPARISON_PI_VS_AGENTIC.md` — tandai gap yang tertutup (steering, event granularity, persist parsial).
