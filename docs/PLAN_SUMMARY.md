# 🎯 Plan Summary

Ringkasan rencana pengembangan Termul — dibagi per modul.

---

## 📁 Modular Plan Structure

| Module | File | Description |
|--------|------|-------------|
| 🎨 **UI** | [docs/PLAN_UI.md](./PLAN_UI.md) | Komponen React, layout, settings, polish |
| 🧠 **Core** | [docs/PLAN_CORE.md](./PLAN_CORE.md) | Engine Rust: agent loop, planner, safety, memory |
| 🖥️ **CLI** | [docs/PLAN_CLI.md](./PLAN_CLI.md) | Standalone binary: REPL, config, commands |
| 🧪 **Testing** | [docs/PLAN_TESTING.md](./PLAN_TESTING.md) | Unit, integration, E2E, performance, security |

---

## 🏗️ Architecture

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│  Termul UI  │────▶│  Tauri APIs  │────▶│ core-agentic │
│  (React)    │     │  (Rust)      │     │  (Rust lib)  │
│  PLAN_UI    │     │              │     │  PLAN_CORE   │
└─────────────┘     └──────────────┘     └──────┬───────┘
                                                 │
                                                 │ shared lib
                                                 │
                    ┌──────────────┐     ┌───────┴──────┐
                    │ Agentic CLI  │────▶│ core-agentic │
                    │ (Binary)     │     │  (Rust lib)  │
                    │  PLAN_CLI    │     │  PLAN_CORE   │
                    └──────────────┘     └──────────────┘
```

---

## 🚦 Cross-Module Priority Timeline

### 🔴 Week 1-2: Integration (UI + Core)

| Module | Task | Est |
|--------|------|-----|
| UI | Integrate FirstRunBanner in App.tsx | 1-2h | ✅ |
| UI | Integrate AgenticPanel in WorkspaceLayout | 1-2h | ✅ |
| UI | Add Agentic Settings in AppPreferences | 1h | ✅ |
| Core | ~~Safety system enhancement~~ ✅ Done | 2-3d |
| CLI | Config commands polish | 1-2d |

### 🟡 Week 3-4: Polish & Enhancement

| Module | Task | Est |
|--------|------|-----|
| UI | Markdown rendering + syntax highlighting | 2-3d |
| UI | Command palette actions | 1d |
| Core | ~~Memory system enhancement~~ ✅ Done | 2d |
| CLI | Interactive mode (REPL) improvements | 2-3d |
| CLI | Output rendering enhancement | 2d |

### 🟢 Month 2: Advanced Features

| Module | Task | Est |
|--------|------|-----|
| Core | Planner Agent | 5-7d |
| Core | Provider enhancements (Z.ai, generic) | 3-4d |
| UI | Provider Manager UI | 3-4d |
| UI | MCP Manager UI | 2-3d |
| UI | Safety Panel UI | 1-2d |
| CLI | Non-interactive / pipe mode | 2-3d |
| CLI | Session management | 2d |

### 🔵 Month 3+: Enterprise

| Module | Task | Est |
|--------|------|-----|
| Core | Multi-Agent System | 7-10d |
| Core | Vector DB Memory | 5-7d |
| UI | Planner Visualization | 2-3d |
| UI | Multi-Agent Visualization | 3-4d |
| CLI | Remote execution | 2-3d |
| CLI | Plugin system | 2-3d |

---

## ✅ Global Checklist

### Week 1-2
- [x] FirstRunBanner integrated
- [x] AgenticPanel integrated
- [x] Agentic Settings added
- [x] Safety scoring implemented
- [x] Memory enhancement implemented
- [ ] Config CLI polished

### Week 3-4
- [ ] Markdown rendering in UI
- [x] Command palette actions
- [ ] Memory compaction in core
- [ ] REPL improvements in CLI

### Month 2
- [ ] Planner agent (core + UI)
- [ ] Provider management (core + UI + CLI)
- [ ] MCP management (UI + CLI)

### Month 3+
- [ ] Multi-agent system
- [ ] Vector DB memory
- [ ] Advanced planner features

---

## 🔗 Reference Documents

| Document | Description |
|-----------|-------------|
| [PLAN_UI.md](./PLAN_UI.md) | UI module plan (React components) |
| [PLAN_CORE.md](./PLAN_CORE.md) | Core module plan (Rust engine) |
| [PLAN_CLI.md](./PLAN_CLI.md) | CLI module plan (standalone binary) |
| [PLAN_TESTING.md](./PLAN_TESTING.md) | Testing strategy (unit, integration, E2E) |
| [AGENTIC_PRD.md](./AGENTIC_PRD.md) | Product Requirements Document |
| [CONFIGURATION.md](./CONFIGURATION.md) | Config schema reference |
| [CONFIG_AUTOMATION.md](./CONFIG_AUTOMATION.md) | Config automation features |
| [IMPLEMENTATION_ROADMAP.md](./IMPLEMENTATION_ROADMAP.md) | Detailed roadmap |
| [PLAN.md](./PLAN.md) | Original full plan (superseded by modular plans) |

---

**Last Updated:** May 12, 2026
