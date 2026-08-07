# Design: Agent Runtime Decoupling (Runtime + Protocol + CLI Migration)

**Status:** Draft v1 — disetujui brainstorming 2026-08-07
**Scope:** Sub-proyek #1 (fondasi) dari goal besar: core-agentic bisa dipakai Node CLI, VS Code extension, dan desktop.

---

## 1. Konteks & Tujuan

`core-agentic` (Rust library) berisi seluruh business logic agent. Saat ini satu-satunya konsumen adalah `agentic-cli` yang **menjalankan orchestrator in-process** dan mencampur rendering dengan wiring runtime (`commands.rs` 3922 baris, `ensure_orchestrator` membangun provider/tools/prompt).

Goal: membuka `core-agentic` untuk consumer non-Rust (Node/Bun CLI, VS Code extension, desktop) tanpa menduplikasi logika agent.

PRD acuan: `docs/PRD_Runtime_CLI_Decoupling.md`.

## 2. Keputusan Hasil Brainstorming

| Keputusan | Pilihan | Alasan |
|---|---|---|
| Scope | Fondasi: `agentic-runtime` + protocol + migrasi Rust CLI | semua consumer lain bergantung pada ini |
| Transport | stdio JSON Lines (A) | standar child process, cross-platform, simpel |
| Boundary | Transport-agnostic | napi-rs = drop-in `NativeClient` nanti tanpa nulis ulang engine |
| Lifecycle | Long-lived daemon | REPL, extension, desktop butuh konteks antar-turn |
| Flow interaktif | confirmation + question + todo, full request/response | ketiganya wajib menjadi message bolak-balik |
| Session | Single session v1 | VS Code extension masih rencana, belum keputusan |
| Config | Hybrid: runtime load dari disk, client override lewat `init` | v1 cepat, fleksibel untuk nanti |
| Validasi | Proof-of-protocol Node script + migrasi Rust CLI (Phase 4 PRD) | 2 sisi bahasa membuktikan protocol stabil |
| Engine location | **Engine di core** (`core::runtime`), binary tipis | testable tanpa spawn, napi-rs tinggal bind |

## 3. Arsitektur Target

```text
agentic/
├── core-agentic/
│   ├── src/runtime/          ← BARU
│   │   ├── protocol.rs       ← tipe Request/Response/ProtocolEvent + framing + versioning
│   │   ├── engine.rs         ← RuntimeEngine: daemon loop, single session, dispatch
│   │   └── transport.rs      ← trait Transport { read_request, write_event }
│   ├── src/events.rs         ← rename + variant baru (Bagian 5)
│   ├── src/tools/            ← global handler → ToolDeps (Bagian 5)
│   └── src/orchestrator/     ← minimal perubahan (run_stream tetap)
│
├── agentic-runtime/          ← BARU (binary, workspace member)
│   └── src/main.rs           ← StdioTransport + RuntimeEngine + signal handling
│
├── agentic-cli/              ← refactor (Bagian 7)
│   └── src/client.rs         ← BARU: StdioClient
│
└── scripts/protocol-demo.js  ← BARU: proof-of-protocol Node script
```

### 3.1 Komponen kunci

- **`Transport` trait** — `read_request() -> Option<Request>`, `write_event(&ProtocolEvent)`. Implementasi: `StdioTransport` (di `agentic-runtime`), `MemoryTransport` (`#[cfg(test)]` di core). napi-rs nanti: bind langsung ke `RuntimeEngine`.
- **`RuntimeEngine`** — state: `Config` (merge disk + override init), satu `Orchestrator` (single session), `current_request_id`, kanal response untuk confirmation/question yang menunggu. Loop: baca request → dispatch → tulis event.
- **`agentic-runtime` binary** — hanya `RuntimeEngine::new(StdioTransport).run()`. Tidak ada clap, tidak ada UI, tidak ada `println!`; log → stderr via tracing.
- **`StdioClient`** (CLI) — spawn `agentic-runtime`, API: `send(Request)`, `on_event(cb)`, `await_response(request_id)`, `shutdown()`. Konsep yang sama akan dipakai Node CLI.

## 4. Protocol

### 4.1 Transport & framing

- JSON Lines: satu objek JSON per baris (`\n`), stdin → request, stdout → event.
- Versioning: tiap pesan bawa `v: 1`; `ready`/`init_ok` menyebut `"protocol":"agentic","version":1`. Client menolak mismatch.
- Korelasi: tiap request punya `request_id`; semua event dari run membawa `request_id` sama.

### 4.2 Request (client → runtime)

```json
{"v":1,"id":"r1","type":"init","overrides":{"configPath":"~/...","permissionMode":"default","model":"glm-4.7"}}
{"v":1,"id":"r2","type":"run","task":"fix login bug","attachments":[]}
{"v":1,"id":"r3","type":"cancel"}
{"v":1,"id":"r4","type":"confirm_response","requestId":"r2","approved":true}
{"v":1,"id":"r5","type":"question_response","requestId":"r2","answers":[]}
{"v":1,"id":"r6","type":"shutdown"}
```

### 4.3 Event (runtime → client)

| Event | Asal | Keterangan |
|---|---|---|
| `thinking` | rename `thought` | reasoning model |
| `planning` | BARU | fase plan dimulai `{task}` |
| `tool_call` | existing | model minta tool |
| `tool_started` | rename `tool_start` | tool mulai eksekusi |
| `tool_delta` | existing | output streaming tool |
| `tool_output` | existing | hasil tool (success/error/truncated/duration) |
| `tool_finished` | BARU | penanda tool selesai |
| `assistant_delta` | BARU | chunk text streaming model (menggantikan jalur `on_chunk`) |
| `warning` | BARU | System yang bersifat warning |
| `confirmation_request` | existing | + nunggu `confirm_response` |
| `question_request` | BARU | + nunggu `question_response` |
| `todo_changed` | BARU | pengganti global todo handler |
| `error` | existing | fatal run error |
| `done` | rename `completed` | run selesai, `{result}` |
| `system`, `plan_progress`, `plan_replanned` | existing | tetap |

### 4.4 Alur hidup

1. Client spawn runtime → runtime langsung kirim `ready` (tanpa diminta).
2. Client kirim `init` (boleh kosong) → runtime load config disk, merge override, bangun session, balas `init_ok`.
3. Client kirim `run` → stream event → `done` / `error`. Memory session bertahan.
4. `cancel` membatalkan run aktif. `shutdown` keluar graceful (exit 0).

### 4.5 Edge cases

- Baris tidak valid → `error{type:"protocol_error"}`, daemon lanjut.
- Client mati / pipe putus → runtime keluar (exit 1, log stderr).
- SIGINT/SIGTERM → cancel run aktif dulu, lalu shutdown graceful.
- Run kedua saat run aktif → `error{type:"busy"}` (single session).

## 5. Refactor Core

### 5a. Streaming → `assistant_delta`

`Orchestrator::run_stream_with_attachments` tetap; engine memakai `on_chunk` closure yang meng-emit `assistant_delta`. Jalur paralel `chunk_tx` di CLI mati. Tidak ada perubahan di dalam orchestrator.

### 5b. Hapus global handler → `ToolDeps`

| Sekarang (global singleton) | Menjadi |
|---|---|
| `set_question_handler` / `clear_question_handler` | `QuestionHandler` jadi field di `QuestionTool` |
| `set_todo_change_handler` | `TodoChangeHandler` jadi field di `TodowriteTool` |
| `set_confirmation_handler` (sudah instance-based ✅) | tetap, closure dari engine |

```rust
struct ToolDeps {
    tracker: Arc<FileTracker>,
    url_policy: UrlPolicy,
    question_handler: Option<Box<dyn QuestionHandler>>, // default: skip-all
    todo_handler: Option<Box<dyn TodoChangeHandler>>,   // default: no-op
}
```

`builtin_tools_with(tracker, url_policy)` → menerima `ToolDeps`.

### 5c. Handler engine

Handler yang di-inject engine menulis event + blok nunggu response:

```rust
// QuestionHandler impl (di engine):
fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer> {
    let rid = engine.current_request_id();
    transport.write_event(Event::QuestionRequest { request_id: rid, questions });
    engine.wait_response::<QuestionResponse>(rid)   // blok di channel pending-response map milik engine
}
```

Pola sama untuk confirmation. `wait_response` di-resolve oleh engine ketika `question_response`/`confirm_response` datang dari stdin. Blok di thread eksekusi tool OK — CLI sekarang juga blok (dialoguer sinkron).

### 5d. Envelope: `request_id` lewat pembungkus, bukan per-variant

```rust
struct ProtocolEvent {
    v: u32,
    request_id: Option<String>,   // None untuk ready
    #[serde(flatten)]
    event: Event,
}
```

Wire format tetap flat:
```json
{"v":1,"requestId":"r2","type":"tool_output","toolName":"grep","output":{...}}
```

### 5e. Perubahan `Event`

- Rename: `Thought`→`Thinking`, `ToolStart`→`ToolStarted`, `Completed`→`Done`
- Baru: `Planning { task }`, `ToolFinished { tool_call_id, tool_name, success }`, `AssistantDelta { content }`, `Warning { message }`, `QuestionRequest { questions }`, `TodoChanged { todos }`
- `QuestionPrompt`/`QuestionAnswer`/`TodoItem` perlu derive `Serialize` (sebagian mungkin sudah)

### 5f. Yang TIDAK berubah

- Algoritma planner, provider, workflow agent, safety, memory (non-goals PRD)
- `run_stream`, `EventEmitter`, `Orchestrator::on_event` tetap API publik
- `Orchestrator` tetap bisa dipakai langsung (Rust embedding)

## 6. Strategi Testing

| Lapisan | Metode | Isi |
|---|---|---|
| 1. Golden protocol | test serialisasi di core | setiap event/request dibandingkan dengan snapshot; round-trip parse→reserialize |
| 2. Engine test | `RuntimeEngine` + `MemoryTransport` + `ScriptedProvider` (sudah ada) | happy path, bidirectional (confirmation/question), cancel, busy, malformed line, config merge |
| 3. Integration binary | spawn `agentic-runtime` (`std::process::Command`) | smoke (ready→shutdown, exit 0), round-trip JSONL; config tempdir + `AGENTIC_RUNTIME_SCRIPTED=1` (debug-only) |
| 4. CLI migration | adapt `with_mock_provider` | CLI spawn runtime (mock mode), drive via `StdioClient`, assertion output tetap |

Golden test berfungsi sebagai pengunci wire format — mitigasi risiko "protocol berubah terlalu sering" dari PRD.

## 7. Migrasi Rust CLI (Phase 4 PRD)

### Tetap di CLI
- `cli.rs` + dispatch `main.rs` (clap)
- Semua `widgets/` (rendering)
- Perintah config file (init/edit/show/validate/backup/restore/export/import/path/reset)
- `skill list/info/create`, `update`, `examples`, `version`, `status`
- `file_ref.rs` (@file expansion + attachments, hasil dikirim di request `run`)

### Pindah ke runtime
- Seluruh isi `ensure_orchestrator()` → `RuntimeEngine::build_session()`: provider, ToolRegistry + ToolDeps, skill discovery + SkillTool, assembly system prompt (AGENT.md + skills + config override + memory section), permission mode, autocompact knobs, cancel wiring, tiga handler
- `CliQuestionHandler` + `CliTodoRenderer` dihapus

### StdioClient (baru)

```rust
struct StdioClient {
    child: tokio::process::Child,
    writer: BufWriter<ChildStdin>,
    reader_task: JoinHandle,
    event_handlers: Vec<Box<dyn Fn(ProtocolEvent)>>,
    pending: HashMap<request_id, oneshot::Sender<Response>>,
    current_run: Option<request_id>,
}
```

### Adaptasi jalur rendering

| Jalur sekarang | Menjadi |
|---|---|
| `commands::run` — spinner thread + chunk_tx + event_rx + final_result | renderer konsumsi `ProtocolEvent` (satu stream) |
| `interactive.rs` — orchestrator langsung | client: kirim `run`, render event |
| `tui/app.rs` — `run_with_callbacks` | client: `on_event` → render |
| confirmation via closure | event `confirmation_request` → dialoguer → `confirm_response` |
| question via global handler | event `question_request` → dialoguer → `question_response` |
| `/model` switch | init override `{model}` → runtime rebuild session |
| `--mode plan` | init override `permissionMode` |

### Refactor file (`commands.rs` 3922 baris dipecah)

```
agentic-cli/src/
├── client.rs              ← StdioClient
├── commands/
│   ├── mod.rs             ← dispatch + run
│   ├── config.rs          ← operasi config file
│   ├── skill.rs           ← skill list/info/create
│   ├── status.rs          ← status/examples/version/update
│   └── run.rs             ← driver: client + rendering
├── interactive.rs         ← refactor ke client
├── tui/                   ← refactor ke client
└── widgets/               ← tidak berubah
```

### Parity yang dijaga
- `@file` expansion + attachment di request `run`
- AGENT.md: runtime di-spawn dengan cwd sama dengan CLI → walk-up discovery tetap benar
- REPL multi-turn: memory hidup di daemon
- `--mode yolo/plan/default` tetap jalan

## 8. Out of Scope v1 (di-defer)

- napi-rs / `NativeClient`
- Multi-session; session persistence lintas-restart daemon (protocol export/import memory)
- VS Code extension, desktop
- Node/Bun CLI (sub-proyek #3 berikutnya)
- `session.rs` resume (memory daemon tidak di-resume darinya)
- Perubahan algoritma planner/provider/workflow (non-goals PRD)

## 9. Urutan Implementasi

| # | Langkah | Definition of Done |
|---|---|---|
| 1 | Core: tipe protocol + rename/new `Event` variants | golden test protocol lolos |
| 2 | Core: `ToolDeps` — hapus global handler | tidak ada `set_question_handler`/`set_todo_change_handler` di lib.rs |
| 3 | Core: `RuntimeEngine` + transports + wiring | engine tests (memory transport + ScriptedProvider) lolos |
| 4 | `agentic-runtime` binary + integration test spawn | smoke + round-trip test lolos |
| 5 | Proof-of-protocol Node script (`scripts/protocol-demo.js`) | kirim run + confirmation round-trip terbukti |
| 6 | CLI: `StdioClient` + migrasi jalur `run` | `agentic run` jalan lewat runtime |
| 7 | CLI: migrasi `interactive` + `tui` | REPL & TUI jalan lewat runtime |
| 8 | CLI: pecah `commands.rs`, hapus kode mati, update exports + docs | build clean, semua test lolos |

## 10. Risiko & Mitigasi

| Risiko | Mitigasi |
|---|---|
| serde `flatten` + internal tag tidak berperilaku | golden test di langkah 1 membuktikan wire format sebelum dibangun di atasnya |
| Busy / race antar run | policy single-run di protocol (`error{type:"busy"}`) |
| Renderer regresi (cursor race TUI yang pernah ada) | event-driven menghapus dual-channel — lebih sederhana dari sekarang |
| Protocol berubah diam-diam | golden test + versioning `v:1` |
| CLI migrasi terlalu besar | urutan bertahap: run dulu, lalu interactive/tui |

## 11. Success Criteria (v1 fondasi)

- `agentic-runtime` menerima request via stdin, mengeluarkan event via stdout (JSONL, `v:1`)
- confirmation + question + todo round-trip berfungsi lewat protocol
- `agentic run`, `agentic interactive`, `agentic tui` berjalan lewat runtime (CLI = renderer, tidak menjalankan orchestrator langsung)
- Proof-of-protocol Node script bekerja tanpa perubahan runtime
- Semua business logic di `core-agentic`; CLI tidak lagi memegang wiring agent
- Golden test mengunci wire format
