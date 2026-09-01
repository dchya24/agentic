# PRD: Decoupling Agent Runtime from CLI

## Status
Draft v1 — Implementation largely landed on
`feature/runtime-cli-decoupling` (rebased onto dev post-P0-P2):
Phase 1-3 done (protocol events, headless runtime engine +
`agentic-runtime` stdio JSONL daemon, Node demo). Phase 4 (CLI as pure
renderer) is in progress: run/callback paths consume the headless
runtime; remaining async migration completed from the recovered WIP.
Phase 5 (Node/Bun CLI) is a demo only (`scripts/protocol-demo.js`).

## Goal

Memisahkan **Agent Runtime** dari implementasi CLI sehingga runtime dapat digunakan oleh:
- Rust CLI (existing)
- Node.js/Bun CLI (new)
- VS Code Extension (future)
- Desktop/Web UI (future)

Target utama adalah **mempertahankan investasi pada `agentic-cli`** sambil membuka kemungkinan frontend lain tanpa menduplikasi logika agent.

---

# Existing Structure

```text
agentic/
├── core-agentic/
└── agentic-cli/
```

Masalah:
- Sebagian logika runtime dan rendering CLI masih bercampur.
- Sulit membuat frontend baru tanpa menyalin logika.

---

# Target Architecture

```text
agentic/

├── core-agentic/        # Pure library
│   ├── planner/
│   ├── orchestrator/
│   ├── provider/
│   ├── tools/
│   ├── memory/
│   └── context/
│
├── agentic-runtime/     # Headless executable
│
├── agentic-cli/         # Rust CLI
│
└── agentic-node-cli/    # Node/Bun CLI
```

---

# Responsibilities

## core-agentic

Berisi seluruh business logic agent.

### Harus ada

- Planner
- Workflow
- Orchestrator
- Tool Registry
- Provider
- Memory
- Context Builder
- Retry
- Reflection

### Tidak boleh ada

- clap
- ratatui
- crossterm
- println!
- terminal color
- spinner

---

## agentic-runtime

Runtime tanpa UI.

Input:

- stdin
- JSON Lines

Output:

- Event Stream (stdout)

Contoh request:

```json
{"type":"run","task":"fix login bug"}
```

Contoh event:

```json
{"event":"thinking"}
{"event":"tool_started","tool":"grep"}
{"event":"assistant_delta","content":"Searching..."}
{"event":"tool_finished"}
{"event":"done"}
```

---

## Rust CLI

Tanggung jawab:

- Parse argument
- Spawn runtime
- Render event
- Spinner
- Progress
- Markdown rendering

Tidak boleh menjalankan orchestrator langsung.

---

## Node/Bun CLI

Tanggung jawab sama dengan Rust CLI.

Node hanya:

- spawn runtime
- kirim request
- render event

Tidak mengetahui implementasi internal runtime.

---

# Runtime Protocol

Transport awal:

- stdin
- stdout
- JSON Lines

Message:

```json
{
  "id":"task-1",
  "type":"run",
  "task":"implement oauth"
}
```

Event minimum:

- thinking
- planning
- tool_started
- tool_output
- tool_finished
- assistant_delta
- warning
- error
- done

---

# Migration Plan

## Phase 1

Audit `agentic-cli`.

Identifikasi:

- rendering
- orchestrator
- provider
- tools

Output:
Daftar komponen yang harus dipindahkan.

---

## Phase 2

Pindahkan seluruh runtime ke `core-agentic`.

Definition of Done:

- CLI tidak lagi memiliki business logic agent.

---

## Phase 3

Tambahkan `agentic-runtime`.

Definition of Done:

- Runtime menerima request melalui stdin.
- Runtime mengeluarkan event melalui stdout.

---

## Phase 4

Refactor Rust CLI.

Definition of Done:

- Rust CLI hanya menjadi renderer.

---

## Phase 5

Bangun Node/Bun CLI.

Definition of Done:

- Fitur setara Rust CLI.
- Menggunakan protocol yang sama.

---

# Non Goals

- Mengubah algoritma planner.
- Mengubah provider.
- Mengubah workflow agent.
- Menambahkan fitur AI baru.

Fokus hanya pada pemisahan runtime dan presentation.

---

# Risks

- Tight coupling yang masih tersisa di CLI.
- Protocol berubah terlalu sering.
- Event tidak cukup granular untuk UI.

Mitigasi:

- Versioning protocol.
- Snapshot test event.
- Golden test untuk output runtime.

---

# Success Criteria

- Rust CLI tetap berfungsi.
- Node/Bun CLI dapat berjalan tanpa perubahan runtime.
- Runtime dapat dipakai frontend lain.
- Tidak ada duplikasi orchestrator.
- Semua business logic berada di `core-agentic`.
