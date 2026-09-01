# Aliran (Flow) `@earendil-works/pi-agent-core` dan `@earendil-works/pi-coding-agent`

Dokumen ini menganalisis aliran kerja dua package di dalam monorepo ini:

- **`packages/agent`** — `@earendil-works/pi-agent-core`: *stateful agent core* yang generik, provider-agnostic, tanpa tahu apa pun soal "coding" atau "CLI". Menyediakan `Agent`, `agentLoop()`, tipe `AgentMessage`/`AgentTool`/`AgentEvent`, serta harness tambahan (session, compaction, skills, prompt templates).
- **`packages/coding-agent`** — `@earendil-works/pi-coding-agent`: implementasi **CLI `pi`** yang dibangun di atas agent core. Menambahkan mode interactive/print/rpc, session persistence, tools built-in (read/bash/edit/write/grep/find/ls), model runtime, dan extension system.

Bacaan pendukung: `packages/agent/README.md`, `packages/coding-agent/README.md`, dan source kedua package.

---

## 1. Gambaran Arsitektur

```
┌──────────────────────────────────────────────────────────────────┐
│                     pi CLI (packages/coding-agent)                │
│                                                                  │
│  main.ts ──parse args──► AgentSessionRuntime ──► AgentSession    │
│                             │                                      │
│                             └── AgentSessionServices               │
│                                 ├─ ModelRuntime (provider/auth)    │
│                                 ├─ SettingsManager                 │
│                                 └─ ResourceLoader (ext/skills)     │
│                                                                  │
│  Modes:  InteractiveMode / runPrintMode / runRpcMode               │
└────────────────────────────────────┬─────────────────────────────┘
                                     │ session.prompt(text)
                                     ▼
┌──────────────────────────────────────────────────────────────────┐
│                AgentSession (coding-agent, per-mode shared)       │
│  - expansion: extension commands, skill (/skill:), templates      │
│  - steering/followUp queue saat streaming                          │
│  - convertToLlm (messages.ts)                                      │
│  - auto-retry, auto-compaction, bash pendings                      │
└────────────────────────────────────┬─────────────────────────────┘
                                     │ agent.prompt(messages)
                                     ▼
┌──────────────────────────────────────────────────────────────────┐
│            Agent core (packages/agent, provider-agnostic)          │
│                                                                  │
│  Agent ─► runAgentLoop / runAgentLoopContinue                      │
│              │                                                     │
│              ├─ transformContext()?  AgentMessage[]→AgentMessage[] │
│              ├─ convertToLlm()       AgentMessage[]→Message[]      │
│              ├─ streamFn()           Message[]→AssistantMessageStream
│              ├─ beforeToolCall / afterToolCall                     │
│              └─ steering/followUp queue polling                    │
│                                                                  │
│  events: agent_start → turn_start → message_* → tool_execution_*  │
│          → turn_end → agent_end                                    │
└──────────────────────────────────────────────────────────────────┘
```

Kunci pembagian tanggung jawab:

- **Agent core** hanya tahu: message/event lifecycle, tool execution loop, queueing, dan lifecycle prompt/continue/abort. Ia tidak mengimpor `pi-ai/compat` sendiri — `StreamFn` di-inject (lihat `packages/agent/src/stream-fn.ts`).
- **Coding-agent** menyediakan seluruh konteks nyata: model+auth, tools, session di disk, sistem prompt, extension, dan TUI/print/RPC. Ia memasang `setDefaultStreamFn(streamSimple)` agar fallback tersedia untuk extension yang memakai agent core langsung (`packages/coding-agent/src/core/sdk.ts:36`).

---

## 2. Package `packages/agent` — Agent Core

### 2.1 File utama

| File | Isi |
|---|---|
| `src/types.ts` | `AgentMessage`, `AgentTool`, `AgentEvent`, `StreamFn`, `AgentLoopConfig`, `AgentState`, `ToolExecutionMode`, `QueueMode` |
| `src/agent.ts` | Kelas `Agent`: state, subscribe, prompt/continue/steer/followUp/abort/waitForIdle/reset |
| `src/agent-loop.ts` | Loop level rendah: `runAgentLoop()`, `runAgentLoopContinue()`, `agentLoop()`, `agentLoopContinue()` (observasional) |
| `src/stream-fn.ts` | `setDefaultStreamFn()` / `getDefaultStreamFn()` |
| `src/index.ts` | Export semua hal di atas + re-export tipe telemetry |
| `src/harness/` | `agent-harness.ts` (tipe kelas error & interface `AgentLane`), `session/`, `compaction/`, `tools/`, `skills.ts`, `prompt-templates.ts`, `system-prompt.ts`, `messages.ts`, `telemetry.ts` |

### 2.2 Tipe inti

- **`AgentMessage`** — message internal agent. Role standar: `user`, `assistant`, `toolResult`. Bisa diperluas via declaration merging `CustomAgentMessages` (coding-agent menambah `bashExecution`, `custom`, `branchSummary`, `compactionSummary`).
- **`AgentTool<T>`** — `name`, `label`, `description`, `parameters` (JSON Schema/typebox), `executionMode`, `execute(ctx, input)`, plus fungsi pendukung opsional.
- **`AgentEvent`** — event stream: `agent_start`, `turn_start`, `message_start`, `message_update`, `message_end`, `tool_execution_start`, `tool_execution_update`, `tool_execution_end`, `turn_end`, `agent_end`.
- **`StreamFn`** — `(model, context, options) => AssistantMessageEventStream`. **Kontrak penting: tidak boleh throw/reject**; semua kegagalan dikodekan sebagai `stopReason: "error"` atau `"aborted"` beserta `errorMessage`.
- **`ToolExecutionMode`** — `"parallel"` (default) atau `"sequential"`.
- **`QueueMode`** — `"all"` atau `"one-at-a-time"` (untuk steering/follow-up).

### 2.3 Alur `prompt()` → `agent_end`

Kelas `Agent` (`packages/agent/src/agent.ts`):

1. `prompt(input, images?)` menormalisasi input menjadi `AgentMessage[]`, menolak bila sudah ada `activeRun`.
2. `runPromptMessages()` memanggil `runAgentLoop(messages, contextSnapshot, loopConfig, processEvents, signal, streamFn)` di dalam `runWithLifecycle()`.
3. `runAgentLoop` (di `agent-loop.ts`):
   - Emit `agent_start`, `turn_start`.
   - Ambil pesan `user` (atau lanjutkan dari transcript via `runAgentLoopContinue`).
   - Emit `message_start` → jalankan `streamFn` → alirkan `message_update` → `message_end` untuk **assistant message**.
   - Bila assistant berakhir dengan `stopReason` yang menandakan tool call: siapkan batch tool.
   - **Preflight**: untuk tiap tool, `beforeToolCall` (bisa mengubah/block). Dalam mode `"parallel"`, preflight berjalan sekuensial tapi eksekusi paralel.
   - Emit `tool_execution_start` → `tool_execution_update` → `tool_execution_end`; lalu `message_start/end` untuk pesan `toolResult`.
   - Setelah batch selesai: poll `getSteeringMessages()` (steering queue) dan `getFollowUpMessages()` (follow-up queue). Bila ada, jalankan sebagai turn lanjutan (menjadi `user` message berikutnya). Bila tidak, lanjut ke LLM call berikutnya (tool result context) — kecuali `terminate` dari seluruh tool menyebabkannya berhenti.
   - Emit `turn_end`, dan loop ke turn berikutnya selama masih ada kerja. Akhirnya emit `agent_end`.
4. Hook `shouldStopAfterTurn` dipanggil setelah `turn_end` sebelum polling queue, sehingga host bisa menghentikan loop dari luar.
5. `processEvents()` di kelas `Agent` mereduksi state dan meng-await semua listener `subscribe()`. `agent_end` berarti "tidak ada event loop lagi", sedangkan idle total tercapai setelah semua listener `agent_end` selesai dan `finishRun()` membersihkan state runtime.

### 2.4 Tool execution: `"parallel"` vs `"sequential"`

- **`"parallel"`** (default): preflight semua tool secara sekuensial dulu, lalu eksekusi tool secara paralel. Event `tool_execution_end` keluar sesuai urutan tool **selesai**; message `toolResult` tetap dalam urutan sumber (sumber = urutan tool call).
- **`"sequential"`**: tool dieksekusi satu per satu; hasil masuk transcript sebelum tool berikutnya.
- `executionMode` di tiap tool memaksa batch tertentu menjadi sequential meskipun default-nya parallel.
- **`terminate: true`** dari `execute()` atau `afterToolCall` menghentikan loop **hanya jika semua tool dalam batch** menghasilkan terminate.

### 2.5 Steering vs Follow-up

- **Steering** (antrean pesan yang "memotong" pekerjaan saat agent sedang jalan) — dipakai mis. Enter di interactive mode.
- **Follow-up** (antrean pesan yang ditambahkan setelah pekerjaan selesai) — dipakai Alt+Enter.
- Keduanya dikontrol `QueueMode`: `"one-at-a-time"` (default) mengambil satu pesan per polling, `"all"` mengambil semuanya.
- Di kelas `Agent`: `steer()`/`followUp()` menambah ke queue; loop mengambil via `getSteeringMessages`/`getFollowUpMessages` di antara turn. Ada `nextRun()` yang menunda ke run berikutnya.
- Sinkronisasi via `hasQueuedMessages()`, dipakai `AgentSession` untuk memutuskan `agent.continue()`.

### 2.6 Low-level API observasional

`agentLoop()` / `agentLoopContinue()` di `src/agent-loop.ts` menerima handler event dan **tidak** menunggu handler async settle (bersifat "fire and forget"). Kelas `Agent` memakai versi `runAgentLoop`/`runAgentLoopContinue` yang meng-await `processEvents` sehingga lifecycle `promise` menunggu semua listener.

### 2.7 Harness (opsional, di `src/harness/`)

- `agent-harness.ts` mendefinisikan tipe/kelas error bertag (`LaneBusy`, `MissingIdentities`, `NoActiveRun`, `NoActiveOperation`, `NothingToResume`, `InvalidMessage`, `UnknownSkill`, `UnknownTemplate`, `UnknownTarget`, `UnknownQueueItem`, `LaneExists`, `InvalidLane`, `NothingToCompact`, `Closed`, `HarnessFault`, `HarnessClosed`, `HarnessNotImplemented`) dan interface `AgentLane` (prompt/skill/compact/navigateTree/steer/followUp/nextRun/abort/resume/waitForIdle/dll). Implementasi `AgentHarness` saat ini banyak memakai `unavailable()` (belum diimplementasikan penuh).
- `src/harness/compaction/` — `compact`, `prepareCompaction`, `generateSummary`, `calculateContextTokens`, `estimateContextTokens`, `shouldCompact`, `findCutPoint`, dst.
- `src/harness/session/` — session tree / entry persistence generik (dipakai bersama coding-agent? lihat §3).
- `src/harness/` lainnya: `skills.ts`, `prompt-templates.ts`, `system-prompt.ts`, `messages.ts`, `telemetry.ts`, `tools/`, `env/`, `utils/`.

---

## 3. Package `packages/coding-agent` — Implementasi CLI `pi`

### 3.1 Entry point: `src/main.ts`

Alur `main(args)`:

1. Inisialisasi env offline, Windows self-update cleanup, cwd/agentDir, bootstrap `SettingsManager`.
2. **Short-circuit commands**: `pi update`/package commands, `pi config`, credential-print (`--credential*`), `--version`, `--export`.
3. `parseArgs` → `Args` (mode, print, model/provider/thinking, tools, session, resume, fork, dst).
4. Tentukan `appMode`: `rpc` → `json` → `print` (bila `-p` atau stdin/stdout bukan TTY) → `interactive`.
5. Migrations + first-time setup + session resolution (`SessionManager.create/open/forkFrom/continueRecent`).
6. Bangun `createRuntime` (factory) → `createAgentSessionServices(...)` → `createAgentSessionFromServices(...)` → `createAgentSessionRuntime(...)`.
7. Berdasarkan `appMode`, jalankan salah satu:
   - `runRpcMode(runtime)` — protokol JSON-RPC di stdio.
   - `InteractiveMode(runtime, {...}).run()` — TUI.
   - `runPrintMode(runtime, { mode: "text" | "json", ... })` — one-shot.

### 3.2 `createAgentSession()` (SDK, `src/core/sdk.ts`)

- Menentukan `cwd`, `agentDir`, `modelRuntime` (default `ModelRuntime.create`), `settingsManager`, `sessionManager` (default `SessionManager.create(cwd, getDefaultSessionDir(...))`).
- Memuat `resourceLoader` (DefaultResourceLoader) bila belum ada.
- Restore model & thinking level dari session bila ada data; bila tidak, `findInitialModel`.
- **Membangun `Agent` core** dengan `initialState` (`systemPrompt: ""`, model, thinkingLevel, tools: []) dan wiring:
  - `convertToLlm: convertToLlmWithBlockImages` — membungkus `convertToLlm` dari `messages.ts` plus filter gambar bila setting `blockImages`.
  - `streamFn` — memanggil `modelRuntime.streamSimple(...)` dengan pengaturan timeout/retry dari settings, transformasi headers (`mergeProviderAttributionHeaders` + event extension `before_provider_headers`).
  - `onPayload` / `onResponse` — event extension `before_provider_request` / `after_provider_response`.
  - `transformContext` — meneruskan ke event extension `context` (runner.emitContext).
  - `steeringMode`, `followUpMode`, `transport`, `thinkingBudgets`, `maxRetryDelayMs` dari settings.
- Memasang `setDefaultStreamFn(streamSimple)` (fallback).
- Restore/init session entries: `appendModelChange`, `appendThinkingLevelChange`.
- Membuat `AgentSession` dan mengembalikan `{ session, extensionsResult, modelFallbackMessage }`.

### 3.3 `AgentSession` (`src/core/agent-session.ts`) — otak semua mode

`AgentSession` membungkus `Agent` core + `SessionManager` + services, dan dipakai sama oleh interactive, print, dan rpc.

**Pembuatan**: `_unsubscribeAgent = agent.subscribe(_handleAgentEvent)`.

**Alur `prompt(text, options?)`** (`agent-session.ts:1116`):

1. Bila dimulai `/` dan bukan command yang dikenal → `_tryExecuteExtensionCommand` (command extension dieksekusi langsung, `preflightResult(true)`).
2. Tolak bila sedang compaction (`_compactionAbortController` aktif).
3. **Emit event extension `input`** — extension bisa `handled` (return), atau `transform` teks/gambar.
4. Ekspansi skill (`/skill:name args` → blok `<skill>`) dan prompt template (`expandPromptTemplate`).
5. Bila **streaming**:
   - Tanpa `streamingBehavior` → throw.
   - `followUp` → `_queueFollowUp`, `steer` → `_queueSteer`. (Interactive memanggil `prompt(text, { streamingBehavior: "steer" })` saat streaming.)
6. Flush pending bash messages; validasi model + auth (`hasConfiguredAuth`/`checkAuth`).
7. Cek compaction pra-prompt (`_checkCompaction(lastAssistant, false)`).
8. Bangun array messages: user message (teks+images) → pesan `_pendingNextTurnMessages` → custom messages dari event extension `before_agent_start`.
9. Terapkan `systemPromptOverride` bila extension memodifikasi system prompt.
10. `_runAgentPrompt(messages)` → `agent.prompt(messages)`; lalu loop `while (await _handlePostAgentRun()) { await agent.continue(); }`.

**`_handlePostAgentRun()`** memeriksa: auto-retry bila `_isRetryableError`, compaction (`_checkCompaction`), lalu `agent.hasQueuedMessages()` (pesan yang di-queue oleh handler `agent_end` extension).

**`_handleAgentEvent(event)`** (shared subscribe handler):

- Saat `message_start` user: lepas dari queue steering/follow-up (update UI queue state) sebelum emit.
- Emit ke extension (`_emitExtensionEvent`) lalu ke listener (`_emit`); `agent_end` diberi `willRetry` bila auto-retry akan terjadi.
- **Persistence**: pada `message_end`, pesan `custom` → `CustomMessageEntry`; `user`/`assistant`/`toolResult` → `SessionMessageEntry`. Bash/compaction/branch summary dipersist di tempat lain.
- `_lastAssistantMessage` di-set untuk pengecekan compaction; counter `_retryAttempt` di-reset saat sukses.

**Lainnya**: `steer()`, `_queueSteer`, `_queueFollowUp`, model scoped cycling, retry (`_prepareRetry`), compaction manual (`/compact`) dan auto (`_checkCompaction` → `_runAutoCompaction`), `navigateTree`, `fork`, `switchSession`, bash handling (`recordBashResult`, defer ke `agent_end` bila streaming agar tidak merusak urutan tool_use/tool_result), `subscribe(listener)` → `AgentSessionEvent`.

### 3.4 Session persistence: `SessionManager` (`src/core/session-manager.ts`)

- Format: JSONL file per session (`*.jsonl`), dengan **header** `{type:"session", id, timestamp, cwd, parentSession}` dan entry-entry berikutnya.
- **Entry types**: `message`, `thinking_level_change`, `model_change`, `compaction`, `branch_summary`, `custom`, `label`, `session_info`, `bash` (dst). Tiap entry ber-ID dan `parentId` (membentuk **tree/branch** — mendukung fork, navigateTree, branch summary).
- **Branch/leaf**: session menyimpan `leafId`; `buildSessionContext()` me-resolve leafId → daftar entry → `sessionEntryToContextMessages` → `AgentMessage[]` untuk LLM.
- Operasi: `create`, `open`, `inMemory`, `forkFrom`, `continueRecent`, `createBranchedSession`, `importFromJsonl`/export, `list`, `listAll`.
- **Compaction** menyimpan `CompactionEntry` (summary, firstKeptEntryId, tokensBefore) sehingga LLM context menampilkan summary sebagai pengganti riwayat lama.
- Versi format: `CURRENT_SESSION_VERSION = 3`.

### 3.5 Model runtime (`src/core/model-runtime.ts`)

- `ModelRuntime` mengimplementasikan `Models` dan dibangun dari `createModels({ credentials, modelsStore })` (katalog provider `@earendil-works/pi-ai/providers/all`).
- Menyediakan `createModels`, `getModel`, `checkAuth`, `refresh` (availability refresh), `streamSimple` (meneruskan ke `prepared.provider.streamSimple` dengan timeout/retry), dan `setRuntimeApiKey`.
- Auth disimpan di `auth.json`; OAuth/login via `/login`; provider-composer; model scope (scoped models) untuk Ctrl+P cycling.

### 3.6 Custom messages & `convertToLlm` (`src/core/messages.ts`)

- Mendeklarasikan via declaration merging `CustomAgentMessages`: `bashExecution`, `custom`, `branchSummary`, `compactionSummary`.
- `convertToLlm(messages)` memetakan tiap `AgentMessage` → `Message` LLM:
  - `bashExecution` → user text (`Ran \`cmd\`...`); **`excludeFromContext` (`!!`) di-skip**.
  - `custom` → user text/content.
  - `branchSummary`/`compactionSummary` → user text dibungkus `<summary>...</summary>`.
  - `user`/`assistant`/`toolResult` → diteruskan apa adanya.
- Dipakai di: `Agent.convertToLlm`, compaction `generateSummary`, dan tool/extension.

### 3.7 Tools built-in (`src/core/tools/`)

Factory `createXxxTool(cwd, options)` dan definition:

- **`read`** — baca file (truncation `truncateHead`/`truncateLine`/`truncateTail`).
- **`bash`** — eksekusi shell (spawn, `BashSpawnHook`, detached children tracking, kill).
- **`edit`** / **`write`** — mutasi file (diantrekan via `withFileMutationQueue`).
- **`grep`**, **`find`**, **`ls`** — pencarian/listing (read-only).
- Tool definition berisi `parameters` JSON Schema + konteks (cwd). Diaktifkan default: `read`, `bash`, `edit`, `write`. CLI `--no-tools`, `--no-builtin-tools`, `--tools`, `--exclude-tools` mengubah daftar.

### 3.8 Modes

| Mode | File | Deskripsi |
|---|---|---|
| Interactive | `modes/interactive/interactive-mode.ts` | TUI (terminal UI). `init()` membangun layout, memuat fd/rg, setup keybindings + editor submit handler; `run()` memulai model refresh, versi check, lalu **loop**: `getUserInput()` → `session.prompt(userInput)`. |
| Print | `modes/print-mode.ts` | One-shot. `-p "prompt"` → teks final; `--mode json` → stream JSON event (`toJsonEvent`). |
| RPC | `modes/rpc/rpc-mode.ts` | Protokol JSON-RPC over stdio (`runRpcMode`): command seperti `prompt`, `get_state`, `subscribe`, dst; output `{type:"response", command, success, data|error}` + event `extension_ui_request`. |

Interactive mode key handling:
- Submit editor → `onSubmit(text)` (`setupEditorSubmitHandler`): command `/settings`, `/model`, `/export`, `/fork`, `/compact`, dll; perintah `!bash` / `!!bash` (exclude context); queue saat compaction; **saat streaming** → `session.prompt(text, { streamingBehavior: "steer" })`; normal → callback/pending.
- Streaming & normal path tetap satu pintu: `session.prompt()`.

### 3.9 Extension system (`src/core/extensions/`)

- `runner.ts`: `ExtensionRunner` mengeksekusi lifecycle/hooks. Event: `input`, `context`, `before_agent_start`, `before_provider_headers`, `before_provider_request`, `after_provider_response`, `message_end`, `tool_call`, `tool_result`, `user_bash`, `project_trust`, `resources_discover`, `session_before_switch/fork/tree/compact`, `session_start`, `session_shutdown`, `agent_settled`, dsb.
- Extension dapat: menambah tools (`customTools`), slash commands (`registerCommand`), UI components, markdown transformers, message renderers, provider config, flags CLI, dan data session (`CustomEntry`).
- `bindExtensions()` memanggil factory extension dengan `ExtensionContext`; interactive mode menyediakan `ExtensionUIContext` (dialogs, toast, dst).

### 3.10 Wiring AgentSession → Agent core

Ringkasan pemetaan:

| Agent core option | Sumber di coding-agent |
|---|---|
| `convertToLlm` | `convertToLlmWithBlockImages` (membungkus `core/messages.ts:convertToLlm`) |
| `streamFn` | `modelRuntime.streamSimple` + retry/timeout/headers dari settings |
| `transformContext` | `extensionRunner.emitContext` |
| `onPayload` / `onResponse` | event extension `before_provider_request` / `after_provider_response` |
| `steeringMode` / `followUpMode` | `settingsManager.getSteeringMode()/getFollowUpMode()` |
| `sessionId` | `sessionManager.getSessionId()` |
| `transport`, `thinkingBudgets`, `maxRetryDelayMs` | settings |

---

## 4. Alur End-to-End (Satu Prompt di Interactive Mode)

```
User mengetik + Enter di editor TUI
  └─ InteractiveMode.setupEditorSubmitHandler → onSubmit(text)
       └─ session.prompt(text, { streamingBehavior: "steer" })   [jika streaming]
          └─ AgentSession.prompt
             ├─ extension input event (dapat transform)
             ├─ ekspansi skill/template
             ├─ (streaming) → _queueSteer  → agent.steer()
             │                    loop agent yang berjalan akan menariknya via getSteeringMessages
             └─ (idle) → validasi model/auth → cek compaction
                  └─ _runAgentPrompt([user msg, ...pendingNextTurn, ...customMsgs])
                       └─ agent.prompt(messages)
                            └─ runAgentLoop
                               ├─ emit agent_start, turn_start, message_start(user)
                               ├─ transformContext (extension)
                               ├─ convertToLlm → Message[]
                               ├─ streamFn (modelRuntime.streamSimple) → assistant stream
                               │    └─ message_update → message_end (assistant)
                               ├─ [tool call?]
                               │    ├─ preflight: beforeToolCall (tool_execution_start)
                               │    ├─ execute tools (parallel/sequential) (tool_execution_update)
                               │    └─ tool_execution_end + message_end (toolResult)
                               ├─ poll steering/follow-up queue → lanjut turn atau berhenti
                               ├─ turn_end → agent_end
                               └─ AgentSession._handleAgentEvent
                                    ├─ emit ke extension + listener TUI
                                    └─ persist ke SessionManager (message_end)
              └─ _handlePostAgentRun → continue() (auto-retry/compaction/queued)
```

---

## 5. Catatan Perbedaan Utama antara Core dan Implementasi

1. **Provider-agnostic vs provider-aware**: agent core tidak pernah menyentuh `pi-ai/compat`/provider; semua lewat `StreamFn` yang di-inject. Coding-agent memasang `streamSimple` sebagai fallback global.
2. **Message set**: core hanya `user`/`assistant`/`toolResult`; coding-agent menambah 4 tipe custom + `convertToLlm` yang mengubahnya jadi user message untuk LLM.
3. **Session**: core punya `harness/session` generik + `AgentHarness` (masih banyak `unavailable()`); coding-agent memakai `SessionManager` JSONL sendiri dengan tree/branch, compaction, dan persistence — ini yang benar-benar dipakai runtime.
4. **Queueing**: `Agent.steer/followUp/nextRun` adalah mekanisme inti; `AgentSession` menambahkan lapisan penjadwalan (queue pesan, compaction lock, preflight) serta queue UI di interactive mode.
5. **Kegagalan**: core meng-enkode error via `stopReason: "error"|"aborted"` + `errorMessage`; coding-agent menambahkan auto-retry dan auto-compaction di atasnya.

---

## 6. File Kunci untuk Navigasi

- `packages/agent/src/agent.ts` — kelas `Agent`
- `packages/agent/src/agent-loop.ts` — `runAgentLoop` / `runAgentLoopContinue` / `agentLoop`
- `packages/agent/src/types.ts` — semua tipe inti
- `packages/agent/src/harness/agent-harness.ts` — tipe `AgentLane` + error bertag
- `packages/coding-agent/src/main.ts` — entry CLI
- `packages/coding-agent/src/core/sdk.ts` — `createAgentSession()` (membangun `Agent` core)
- `packages/coding-agent/src/core/agent-session.ts` — `AgentSession` (loop tinggi)
- `packages/coding-agent/src/core/agent-session-runtime.ts` — `AgentSessionRuntime` (switch/fork/new/import session)
- `packages/coding-agent/src/core/agent-session-services.ts` — services terikat cwd
- `packages/coding-agent/src/core/session-manager.ts` — persistence JSONL + branch tree
- `packages/coding-agent/src/core/model-runtime.ts` — provider/auth/streamSimple
- `packages/coding-agent/src/core/messages.ts` — custom messages + `convertToLlm`
- `packages/coding-agent/src/core/tools/index.ts` — factory tools
- `packages/coding-agent/src/modes/interactive/interactive-mode.ts` — TUI
- `packages/coding-agent/src/modes/print-mode.ts` — print/json mode
- `packages/coding-agent/src/modes/rpc/rpc-mode.ts` — JSON-RPC
- `packages/coding-agent/src/core/extensions/runner.ts` — extension lifecycle/hooks
