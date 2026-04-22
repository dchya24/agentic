# Tasks: Agentic Project Overview

**Overview**: Agentic project task lists  
**Created**: 2026-04-20

---

## Task Files

| File | Description |
|------|-------------|
| `tasks-core-agentic.md` | Core Rust library |
| `tasks-agentic-cli.md` | Standalone CLI binary |
| `tasks-termul-integration.md` | Termul integration |

## Implementation Order

```
1. tasks-core-agentic.md     → Library (foundation)
          │
          ▼
2. tasks-agentic-cli.md      → CLI binary (uses library)
          │
          ▼
3. tasks-termul-integration.md → Termul (uses library)
```

## Quick Reference

### Start Core Agentic
```bash
git checkout -b feature/core-agentic
```

### Start CLI
```bash
git checkout -b feature/agentic-cli
```

### Start Termul Integration
```bash
git checkout -b feature/termul-agentic
```

---

## Notes

- Each task file is independent but has dependencies on previous phases
- Complete tasks in order for best results
- Check off tasks as you complete them