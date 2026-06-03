# Tasks: Agentic Project Overview

**Overview**: Task lists for the agentic CLI + core library
**Created**: 2026-04-20
**Updated**: 2026-06-03

---

## Snapshot

| Crate / surface | Status |
|-----------------|--------|
| `core-agentic` library | Foundation complete; ~95% architecture coverage. Module split (memory/orchestrator/safety) + web_search + URL allowlist + cost tracking + apply_patch + injection scanner + planner + skills + **prompt caching** all on `dev`. **Next:** Phase 12 (TBD). |
| `agentic-cli` binary | Run / interactive / TUI flows complete. Shared widgets stack lands all output through ratatui primitives. `/search` slash command + cost line in status bar + diff preview in confirmation prompt + skills commands + **cache observability**. **Next:** Config wizard cache settings. |

## Task Files

| File | Description |
|------|-------------|
| `tasks-core-agentic.md` | Core Rust library |
| `tasks-agentic-cli.md` | Standalone CLI binary |

## Reading Order

```
1. tasks-core-agentic.md → Library (foundation)
        │
        ▼
2. tasks-agentic-cli.md → CLI binary (uses the library)
```

## Branch Pointers

```bash
# Foundation work landed on `dev`; new work branches off dev.
git checkout -b feature/<your-feature>
```

---

## Notes

- Each task file is independent but the CLI depends on the library
- Check off tasks (`- [ ]` → `- [x]`) as you complete them
- For deep architecture context see `docs/architecture-alignment-overview-25052026.md`
- For roadmap-level priorities see `docs/ROADMAP.md`
