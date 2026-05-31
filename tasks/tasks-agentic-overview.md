# Tasks: Agentic Project Overview

**Overview**: Agentic project task lists
**Created**: 2026-04-20
**Updated**: 2026-05-31

---

## Snapshot

| Crate / surface | Status |
|-----------------|--------|
| `core-agentic` library | Foundation complete; ~95% architecture coverage. Module split (memory/orchestrator/safety) + `web_search` + summarizer config landed on `dev`. |
| `agentic-cli` binary | Run / interactive / TUI flows complete. Shared widgets stack lands all output through ratatui primitives. `/search` slash command added. |
| Termul integration | Backend Tauri commands + React panels complete. Advanced UI surfaces (planner, MCP manager, memory search UI) still open. |

## Task Files

| File | Description |
|------|-------------|
| `tasks-core-agentic.md` | Core Rust library |
| `tasks-agentic-cli.md` | Standalone CLI binary |
| `tasks-termul-integration.md` | Termul integration |

## Reading Order

```
1. tasks-core-agentic.md     → Library (foundation)
          │
          ▼
2. tasks-agentic-cli.md      → CLI binary (uses library)
          │
          ▼
3. tasks-termul-integration.md → Termul (uses library)
```

## Branch Pointers (historical)

```bash
# Foundation work landed on `dev`; new work branches off dev.
git checkout -b feature/<your-feature>
```

---

## Notes

- Each task file is independent but has dependencies on previous phases
- Check off tasks (`- [ ]` → `- [x]`) as you complete them
- For deep architecture context see `docs/architecture-alignment-overview-25052026.md`
- For roadmap-level priorities see `docs/IMPLEMENTATION_ROADMAP.md`
