# Rencana Pematangan core-agentic

> Target: `core-agentic` menjadi `core` yang dapat dipakai agentic-cli, apps
> CLI/TUI TypeScript, desktop apps, API server, dan Vibe Kanban — tanpa
> duplikasi logika agent.
>
> Basis: `docs/21-08-2026-review-core-agentic.md` (review) + audit kode aktual
> (2026-08-21). Setiap item mencatat status aktual, bukan asumsi review.

## Prinsip

- **Memory menyimpan informasi. Context menentukan apa yang dikirim ke model.**
- **Runtime mengelola lifecycle. Loop mengelola decision.**
- **Events adalah satu-satunya jalan keluar dari core menuju frontend.**
- Setiap refactor harus backward-compatible dulu, alias pembersihan menyusul
  setelah semua pemakai (agentic-cli) dimigrasi — clean cutover, tanpa shim.

---

## Prioritas P0 — fondasi

### P0-1: Context Engine (subsystem first-class)

**Status aktual:** context management tersebar di 4 tempat:
- `memory/store.rs` — `get_context*` (penyusunan konteks) bercampur dengan
  penyimpanan (`add_message`, `persist`)
- `orchestrator/messages.rs` — `build_request_messages`, `sanitize_for_provider`
- `orchestrator/compaction.rs` — `maybe_autocompact`, `build_messages`
- `prompts.rs`, `skills/`

**Perubahan:**
1. Buat `src/context/` baru:
   - `mod.rs` — `ContextEngine` struct: menyusun slice request
   - `builder.rs` — pindahkan `build_request_messages` + `sanitize_for_provider`
     dari `orchestrator/messages.rs`
   - `budget.rs` — alokasi budget: system prompt / tools / conversation /
     response (gantikan `request_budget()` di `Memory`)
   - `sources.rs` — agregasi sumber: system prompt, conversation window,
     memory (persistent), skills aktif, plan aktif, env/task state
2. `Memory` tetap sebagai penyimpanan murni (CRUD + pin + search + persist);
   hapus `get_context*` dari `store.rs` — pindah logika windowing ke
   `ContextEngine`.
3. Orchestrator memanggil `ContextEngine::build(session)` per-turn, bukan
   `memory.get_context*` + helper di `messages.rs`.

**Verifikasi:** unit test builder (window, pin, orphan tool call, sanitize)
ikut pindah; `tests/orchestrator_loop.rs` tetap hijau tanpa perubahan
assertion.

---

### P0-2: Tool Capability Model

**Status aktual:** `Tool` trait hanya punya `is_read_only()` dan
`execute_streaming()` (default = `execute`). Scheduler di `tool_exec.rs`
meng-hardcode: read-only batch paralel, mutating sekuensial.

**Perubahan:**
1. Tambah `ToolMetadata` di `tool.rs`:

   ```rust
   pub struct ToolMetadata {
       pub mutability: Mutability,      // ReadOnly | Mutating
       pub concurrency: Concurrency,    // ParallelSafe | Exclusive
       pub idempotent: bool,
       pub risk: RiskLevel,             // Low | Medium | High (dari safety)
       pub side_effects: SideEffects,   // None | Fs | Shell | Network | UserFacing
   }
   ```

2. Trait baru `fn metadata(&self) -> ToolMetadata` (default dari `is_read_only`).
   Migrasi 18 builtin + MCP adapter + subagent tool.
3. `ToolRegistry::execute_batch` menggantikan if/else di `tool_exec.rs`:
   analisis capability → kelompokkan parallel-safe / exclusive / butuh
   confirmation → eksekusi.

**Verifikasi:** test `execute_batch` (campuran read-only + mutating, urutan
benar, konflik exclusive); `tool_exec.rs` kehilangan cabang if/else.

---

### P0-3: Event lifecycle session

**Status aktual:** 11 event ada, semua `Serialize` UI-agnostic. Belum ada
event lifecycle agent session.

**Perubahan:** tambah variant ke `Event` enum:
`SessionStarted`, `ModelRequest`, `ModelChunk`, `ToolCallCompleted`
(ada `ToolOutput`, tambah eksplisit), `SkillActivated`, `PlanCreated`,
`StepStarted`, `StepCompleted`, `CompactionStarted`, `WaitingForUser`,
`SessionCompleted`, `SessionFailed`, `StateChanged(new_state)`.

**Verifikasi:** serialization test; CLI lama tetap render (event baru
ignored/fallback di renderer).

---

## Prioritas P1 — runtime & session

> **Status (2026-08-31): SELESAI.**

### P1-1: Pisahkan Runtime dari Loop — ✅ selesai

Implementasi aktual:
- `runtime.rs` — trait `AgentLoop` (sync `run` + streaming `run_stream`),
  `StandardLoop` (delegasi ke loop LLM→tool→observation teruji),
  `AgentRuntime` (lifecycle envelope: session begin/terminal, cancel,
  pause/resume, status), `ChildSpawn` + `AgentRuntime::spawn` untuk
  subagent.
- `spawn_subagent` dimigrasi ke `AgentRuntime::spawn` — subagent kini
  lewat jalur runtime yang sama, bukan orchestrator manual.
- Iteration boundary di `run.rs` menghormati `pause` (park + Condvar)
  lalu `cancel`; `Orchestrator` dapat `cancel()/pause()/resume()/is_paused()`.
- Test: `tests/runtime_loop.rs` (6) — end-to-end, dispatch loop kustom,
  streaming, park/resume, cancel saat park, `on_state_change`.

### P1-2: State machine formal + observable — ✅ selesai

Implementasi aktual:
- `OrchestratorState` diperluas: `Created, Idle, WaitingForModel,
  ExecutingTools, WaitingForUser, Compacting, Completed, Failed,
  Cancelled` dengan `as_str()`/`from_wire()` dan guard
  `can_transition_to()` (matriks di-pin test dua arah).
- Semua penulisan state lewat `set_state()` (guard + log transisi
  ilegal + emit `Event::StateChanged`); handler tiped
  `on_state_change()` tersedia.
- Loop: `WaitingForModel` per-iterasi; tool turn → `ExecutingTools`
  (+ `WaitingForUser` sekitar konfirmasi); autocompact → `Compacting`;
  terminal `Completed/Failed/Cancelled` di lifecycle envelope.

### P1-3: Session checkpoint & resume — ✅ selesai

Implementasi aktual:
- `session.rs` — `AgentSession` (versioned JSON: state, model,
  messages, active_skill, plan/task_state slot, checkpoint_count) +
  `SessionStore` (save atomik via rename, load, list newest-first,
  delete, validasi id anti path-traversal).
- `orchestrator/checkpoint.rs` — `set_session_store/clear/reset_session`,
  `session_begin`/`session_terminal` di lifecycle envelope, checkpoint
  otomatis per tool-boundary (di luar memory lock — hindari deadlock),
  `checkpoint()` manual, `resume_session()` (attach store + restore
  history + adopsi session id).
- Tanpa store = no-op penuh (kompatibilitas perilaku lama).
- Test: 7 unit store + 3 integration crash→resume/failure-postmortem.

---

### P1-1: Pisahkan Runtime dari Loop

**Status aktual:** semua di `orchestrator/run.rs` (loop + lifecycle +
streaming + compaction + tool exec bercampur).

**Perubahan:**
- `AgentRuntime` (lifecycle): `start()`, `run()`, `cancel()`, `pause()`,
  `resume()`, `status()`, `session()` — menangani state, checkpoint,
  event lifecycle.
- `AgentLoop` (decision): trait dengan implementasi `StandardLoop`
  (loop LLM→tool→observation), `PlanningLoop`, `InteractiveLoop`,
  `SubagentLoop`. Subagent memakai `AgentRuntime::spawn()` yang sama.

**Verifikasi:** `tests/orchestrator_loop.rs` + `tests/subagent_loop.rs`
hijau; tidak ada conditional `if planner else standard` di loop.

---

### P1-2: State machine formal + observable

**Status aktual:** `OrchestratorState {Idle, Planning, Executing, Completed}`
di-set manual via `Mutex` di `run.rs`, tidak emit event.

**Perubahan:**
1. Perluas enum: `Created, Running, WaitingForModel, ExecutingTools,
   WaitingForUser, Compacting, Completed, Failed, Cancelled`.
2. Definisikan transisi legal (guard) + `StateChanged` event di tiap
   transisi.
3. `get_state()` tetap, tambah `on_state_change` handler.

**Verifikasi:** unit test transisi (legal/illegal); event `StateChanged`
terobservasi di test integration.

---

### P1-3: Session checkpoint & resume

**Status aktual:** `memory/store.rs` + `persist.rs` persist hanya messages
(+ tool_calls/results). Tidak ada state/plan/task_state.

**Perubahan:**
1. `AgentSession` struct: `session_id`, `state`, `messages`,
   `tool_calls/results`, `active_skills`, `plan`, `task_state`,
   `model_state` (usage, cost), `checkpoints`.
2. `SessionStore` (baru, di `session.rs`): save/load/list/delete
   checkpoint; format JSON versioned.
3. Orchestrator auto-checkpoint per-tool-boundary + `resume(session_id)`.

**Verifikasi:** test crash→resume (simulasi: tulis session, buat baru, load,
lanjut dari state `WaitingForModel`/`ExecutingTools`).

---

## Prioritas P2 — policy & ekstensi

> **Status (2026-08-31): SELESAI.**

### P2-1: Safety policy layer terpusat + hapus leak stdout — ✅ selesai

- `Safety::evaluate_tool(PolicyRequest) -> PolicyDecision` — satu jalur
  untuk builtin/MCP/external: skor per-argumen + risk floor tool
  (`ToolMetadata::risk`) menaikkan *effective risk* untuk gate
  konfirmasi. Floor tidak pernah memberi izin (denial tetap dominan);
  Yolo/Plan tidak pernah di-prompt oleh floor.
- Kedua pre-pass `tool_exec.rs` kini lewat pipeline yang sama.
- `println!` di core = **0** — denial/skip kini `Event::System`
  (renderer CLI punya catch-all).
- Test: 5 unit policy (allowed/denied/confirm/floor/floor-tak-mengizinkan).

### P2-2: Skills metadata kaya + progressive loading — ✅ selesai

- `SkillMetadata` + `tags: Vec<String>` (lowercased, deduped) +
  `version: Option<String>`; `parse_frontmatter` mendukung `tags: a, b`
  dan `version:`.
- `SkillRegistry` — candidate scoring query-aware: exact name +100,
  partial name +50, tag hit +30, word hits +10; deterministic
  (relevance → alphabetical).
- Progressive loading: scoring memakai metadata murni; konten penuh
  dimuat saat aktivasi (via tool `skill`).
- Test: 4 unit (scoring/tag parsing/determinisme).

### P2-3: Subagent policy tree — ✅ selesai

- `SubagentPolicy { max_depth: 3, max_iterations: 12, max_tokens:
  64_000, max_children: 4, timeout: 600s }` + `SubagentTool::with_depth/
  with_policy`.
- Enforcement di `SpawnSubagentTool::execute`: guard depth (child
  registry di-patch dengan tool spawn depth+1 → guard mengganda ke
  bawah pohon), guard max_children (counter atomic), cap iterations
  (`min(requested, policy)`), token budget child
  (`ChildSpawn::with_memory_token_budget` → `Memory::max_tokens`), dan
  timeout wall-clock (thread + `recv_timeout`; cancel flag child
  di-flip saat timeout; mirror cancel parent via watchdog).
- Test: 2+ unit (defaults, depth refusal, proceeds-at-depth-0).

### P2-4: Eliminasi global mutable state — ✅ selesai

- `QuestionTool` punya handler per-instance
  (`with_handler`); `install_question_handler(&ToolRegistry, …)`
  mengganti tool di registry. CLI (`ensure_orchestrator`) dimigrasi —
  tidak lagi memanggil global setter.
- Global slots (`QUESTION_HANDLER`, `SKILL_LOADER`) tetap berfungsi
  sebagai jalur deprecated (`#[deprecated]` pada `set_*`/`clear_*`/
  `activate_skill`/`resolve_skill`/`deactivate_skill`); internal shim
  non-deprecated (`*_global`, `global_question_handler`) dipakai jalur
  fallback `SkillTool`/`QuestionTool` — core bebas lint deprecated.
- Test: 2 unit per-instance (handler instance + swap registry).

---

### P2-1: Safety policy layer terpusat + hapus leak stdout

**Status aktual:** komponen safety lengkap (risk, confirmation, allow/deny,
path sandbox, URL, rate limit, audit, injection) tapi integrasi
percabangan if/else di `tool_exec.rs` + ada `println!` langsung ke stdout
(`tool_exec.rs:146,168,438,460`) — melanggar boundary core.

**Perubahan:**
1. `PolicyEngine` (di `safety/engine.rs`): input = tool request →
   output `{allowed, denied, confirmation_required, sanitized}` — satu
   jalur untuk builtin, MCP, external.
2. Ganti `println!` dengan `Event::ToolOutput`/`Event::System`.

**Verifikasi:** test policy engine (allowed/denied/confirm/sanitized);
grep `println!` di `core-agentic/src` = 0 (selain test).

### P2-2: Skills metadata kaya + progressive loading

**Status aktual:** `SkillLoader` + discovery 5 lokasi + `activate` ada;
frontmatter hanya `name`+`description`.

**Perubahan:** tambah `tags`, `version`; `SkillRegistry` dengan
candidate scoring (query-aware) → activation → load → context; loading
bertahap (metadata dulu, isi saat diaktifkan).

### P2-3: Subagent policy tree

**Status aktual:** `spawn_subagent` memakai `Orchestrator` baru (sudah
benar secara arsitektur); tidak ada policy kedalaman.

**Perubahan:** `SubagentPolicy {max_depth, max_iterations, max_tokens,
max_children, timeout}`; enforce di `SpawnSubagentTool`.

### P2-4: Eliminasi global mutable state (multi-session readiness)

**Status aktual:** `skills/mod.rs` `static SKILL_LOADER` +
`question.rs` handler = callback global per-process. Untuk API server /
kanban dengan banyak session paralel ini menjadi masalah.

**Perubahan:** pindahkan handler ke per-`Orchestrator`/per-`Session`
(setter di struct, bukan static); pertahankan fungsi global sebagai
deprecation sementara (jika dipakai CLI) → migrasi CLI → hapus.

---

## Urutan pengerjaan yang disarankan

```text
Fase A (P0)   Context Engine → Tool Capability → Event lifecycle
Fase B (P1)   Runtime/Loop split → State machine → Session checkpoint
Fase C (P2)   Policy terpusat → Skills metadata → Subagent policy → global state
Fase D        Migrasi agentic-cli ke API baru; hapus jalur lama
              (clean cutover, tanpa shim)
```

## Catatan tambahan audit

- Dependency tree `core-agentic` sudah bersih (tanpa clap/ratatui/crossterm/
  dialoguer/indicatif) — boundary UI sudah baik.
- `diff_util.rs`, `mcp/transport.rs`, `git_query.rs`, `run_command.rs`
  memakai IO — itu wajar (tool/MCP), bukan leak.
- `tests/` integration: `orchestrator_loop.rs`, `planner_loop.rs`,
  `skills_loop.rs` — jadi safety net utama refactor; jaga tetap hijau di
  tiap fase.
