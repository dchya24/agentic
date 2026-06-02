# Model Picker Integration — Inline Fuzzy Select

> **Date:** 2026-06-02  
> **Status:** ✅ Implemented (revised from ratatui modal)  
> **Related:** `docs/shared-widgets-architecture-26052026.md`

## Overview

`/models` command menggunakan inline fuzzy-select picker (dialoguer) yang berjalan langsung di REPL tanpa alternate screen. Dipilih daripada ratatui full-screen modal karena lebih ringan, konsisten dengan UX inline, dan tidak menyebabkan masalah screen clearing di split terminal.

## Design Decision

### ratatui Full-Screen Modal (sebelumnya)
- ❌ Menggunakan alternate screen → screen clearing issues
- ❌ Berantakan di split terminal
- ❌ Context REPL hilang setelah exit
- ❌ 348 lines of code

### dialoguer Inline Fuzzy Select (sekarang)
- ✅ Inline di REPL, tidak clear screen
- ✅ Berfungsi baik di split terminal
- ✅ Fuzzy search real-time
- ✅ ~50 lines of code
- ✅ Auto-complete `/models <name>` via reedline completer

## Implementation

### 1. Interactive Picker (`commands.rs`)

```rust
pub fn pick_model_interactive_inline(&mut self) -> Option<(String, String)> {
    use dialoguer::{FuzzySelect, theme::ColorfulTheme};
    
    // Build list with provider, vision icon, active marker
    // ...
    
    FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("🤖 Select Model")
        .items(&items)
        .default(active_idx)
        .interact_opt()
}
```

### 2. Auto-Complete (`interactive.rs`)

```rust
fn complete_model_suggestions(query: &str) -> Vec<Suggestion> {
    // Returns filtered models from config
    // Supports partial match on display_name, model ID, provider
}

// In AgenticCompleter:
if line.starts_with("/models ") || line.starts_with("/m ") {
    return complete_model_suggestions(query);
}
```

## User Experience

### Option 1: Interactive Picker
```
/models<Enter>

🤖 Select Model (type to filter, ↑↓ to navigate, Enter to select, Esc to cancel)
❯ kimi-k2.6 👁 [sumopod] ●
  qwen3.6-27b [Alibaba Cloud]
  kr/claude-sonnet-4.5 [devchya.id]
```

### Option 2: Direct Switch
```
/models claude-sonnet<Enter>
✓ Switched to devchya.id / kr/claude-sonnet-4.5
```

### Option 3: Auto-Complete
```
/models gpt<Tab>
┌─────────────────────────────────────────┐
│ GPT-4o 👁 [openai] ●                   │
│ GPT-4 Turbo [openai]                    │
└─────────────────────────────────────────┘
```

## Visual Indicators

| Indicator | Meaning |
|-----------|---------|
| `●` | Active model (green) |
| `👁` | Vision capability (light blue) |
| `[provider]` | Provider badge |

## Keyboard Controls (Interactive Picker)

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate |
| Type | Fuzzy filter |
| `Enter` | Select & switch |
| `Esc` | Cancel |

## Code Metrics

| Metric | ratatui Modal | dialoguer Inline |
|--------|--------------|-----------------|
| Lines of code | 348 | ~50 |
| External dep | ratatui (alternate screen) | dialoguer (already used) |
| Split terminal | ❌ Issues | ✅ Works |
| Screen clearing | ❌ Required | ✅ Not needed |
| Auto-complete | ❌ No | ✅ Yes |

## Files

- `agentic-cli/src/commands.rs` — `pick_model_interactive_inline()`
- `agentic-cli/src/interactive.rs` — `complete_model_suggestions()`, `AgenticCompleter`
