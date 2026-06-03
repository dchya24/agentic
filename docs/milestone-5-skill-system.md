# Milestone 5 — Skill System

**Date:** 2026-06-03
**Scope:** `core-agentic`, `agentic-cli`
**Status:** Design approved, implementation planned

## Goal

Add a skill system that allows the agent to load domain-specific instructions on demand, following the [Agent Skills standard](https://agentskills.io/specification) for cross-agent compatibility with pi, opencode, codex, and similar tools.

## Design

### Skill Locations (Discovery)

Skills are discovered from these directories (scanned in order, first-found wins on name collision):

| Path | Scope | Compat |
|------|-------|--------|
| `~/.agents/skills/` | Global | pi, opencode, codex |
| `~/.config/agentic/skills/` | Global | agentic-specific |
| `.agents/skills/` (walk-up from cwd) | Project | pi, opencode, codex |
| `.agentic/skills/` (walk-up from cwd) | Project | agentic-specific |

Additionally, `skills.compat_dirs` in config allows adding extra paths (e.g. `~/.claude/skills/`, `~/.codex/skills/`).

### SKILL.md Format

Per Agent Skills standard:

```markdown
---
name: my-skill
description: What this skill does and when to use it. Max 1024 chars.
---

# My Skill

## Setup
Optional setup instructions.

## Usage
Instructions the agent follows when this skill is loaded.
```

Name rules: lowercase a-z, 0-9, hyphens only, 1-64 chars. Must match parent directory.

### Selection Model — Hybrid (A + B)

**Default (Opsi A):** All discovered skills are indexed. The system prompt lists available skills by name + description. The agent loads skills on demand via the `skill` tool.

**Optional blocklist (Opsi B):** Users can disable specific skills via `skills.blocklist` in config. Blocked skills are excluded from the index.

### Config (in existing `config.json`)

```json
{
  "skills": {
    "blocklist": ["unwanted-skill"],
    "compat_dirs": ["~/.claude/skills"]
  }
}
```

### Runtime Flow

1. **Startup:** Scan skill directories → build `SkillIndex`
2. **System prompt:** Append `📦 Skills: <name> (<description>)` line for each indexed skill
3. **Agent decides** a task matches a skill → calls `skill("skill-name")`
4. **`skill` tool:**
   - Looks up skill in index
   - Reads `SKILL.md` + any referenced files (relative paths from skill dir)
   - Returns full content as tool output
   - Optionally appends instructions to system prompt for session duration
5. **Model absorbs instructions** → follows them for subsequent turns

### CLI Commands

| Command | Description |
|---------|-------------|
| `agentic skill list` | List all indexed skills with name, description, source path |
| `agentic skill info <name>` | Show skill details (SKILL.md preview, files) |
| `agentic skill create <name>` | Scaffold new skill directory + SKILL.md template |
| `/skills` (REPL) | Same as `agentic skill list` |
| `/skills <name>` (REPL) | Load skill inline |

### Status Bar Indicators

- Banner panel: `📄 AGENT.md · 🧠 memory.md · ⚡ skill:<active-skill>` when a skill is loaded
- Status bar: skill chip with name when active

## Implementation Plan

### Phase 1 — Core library (`core-agentic`)

| # | Task | Files |
|---|------|-------|
| 1.1 | Skill format types: `Skill`, `SkillIndex`, `SkillMetadata` | `core-agentic/src/skills/mod.rs` |
| 1.2 | Skill discovery: walk directories, build index | `core-agentic/src/skills/discovery.rs` |
| 1.3 | `skill` tool: `SkillLoader` trait + tool impl | `core-agentic/src/skills/tool.rs` |
| 1.4 | Config wiring: `SkillsConfig` in `config.rs` | `core-agentic/src/config.rs` |
| 1.5 | Skill index injected into system prompt | `core-agentic/src/prompts.rs` |
| 1.6 | Unit tests: discovery, load, missing skill, invalid SKILL.md | — |
| 1.7 | Integration tests: skill tool end-to-end | `core-agentic/tests/skills_loop.rs` |

### Phase 2 — CLI (`agentic-cli`)

| # | Task | Files |
|---|------|-------|
| 2.1 | `agentic skill list` command handler | `agentic-cli/src/commands.rs` |
| 2.2 | `agentic skill info <name>` command handler | `agentic-cli/src/commands.rs` |
| 2.3 | `agentic skill create <name>` scaffold wizard | `agentic-cli/src/commands.rs` |
| 2.4 | `/skills` REPL slash command | `agentic-cli/src/interactive.rs` |
| 2.5 | Status bar + banner chips for active skill | `agentic-cli/src/widgets/` |
| 2.6 | `SkillResolver` trait (following QuestionHandler callback pattern) | `agentic-cli/src/commands.rs` |

## Skill System Implementation Plan

Saved as `docs/plans/2026-06-03-skill-system.md`
