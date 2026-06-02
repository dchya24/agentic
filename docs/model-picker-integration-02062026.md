# Model Picker Integration - ratatui Full-Screen Widget

> **Date:** 2026-06-02  
> **Status:** ✅ Implemented  
> **PR/Issue:** Related to shared widgets architecture alignment

## Overview

Mengganti implementasi `/models` command di CLI interactive mode dari `dialoguer::FuzzySelect` (third-party inline picker) ke `model_picker.rs` (ratatui full-screen TUI widget) yang sudah tersedia namun belum terintegrasi.

## Problem Statement

Sebelum perubahan ini:
- File `model_picker.rs` sudah dibuat dengan full-screen ratatui implementation
- Namun `commands.rs::pick_model_interactive()` masih menggunakan `dialoguer::FuzzySelect`
- Ini menyebabkan:
  - **Inconsistency**: TUI mode menggunakan ratatui, CLI mode menggunakan dialoguer
  - **Code duplication**: Ada 2 implementasi model picker (satu di model_picker.rs yang tidak dipakai, satu di commands.rs yang aktif)
  - **Not aligned**: Melanggar shared widgets architecture yang sudah didefinisikan

## Solution

Mengintegrasikan `model_picker.rs` ke dalam codebase:
1. Register module di `main.rs`
2. Add config accessor method di `Commands`
3. Replace implementation `pick_model_interactive()` untuk call `model_picker::run()`
4. Cleanup unused code dan fix warnings

## Changes Made

### 1. Module Registration

**File:** `agentic-cli/src/main.rs`

```rust
mod cli;
mod commands;
mod confirmation;
mod error;
mod file_ref;
mod interactive;
mod model_picker;  // ← Added
mod tui;
mod widgets;
```

### 2. Config Accessor

**File:** `agentic-cli/src/commands.rs`

```rust
impl Commands {
    // ... existing methods ...
    
    /// Get a reference to the config
    pub(crate) fn get_config(&self) -> &Config {
        &self.config
    }
}
```

**Rationale:** `config` field is private, dan method `config(&self, action)` sudah ada untuk command dispatch. Method `get_config()` memberikan akses read-only tanpa konflik nama.

### 3. Simplified Implementation

**File:** `agentic-cli/src/commands.rs`

**Before (84 lines):**
```rust
pub fn pick_model_interactive(&mut self) -> Option<(String, String)> {
    use dialoguer::{theme::ColorfulTheme, FuzzySelect};
    
    // Build flat list of (rendered_label, provider_idx, model_idx, is_active)
    let mut items: Vec<(String, usize, usize, bool)> = Vec::new();
    // ... 70+ lines of manual list building, ANSI conversion, dialoguer setup ...
    
    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select model")
        .default(default)
        .items(&labels)
        .interact_opt()
        .ok()??;
    
    // ... manual switch logic ...
}
```

**After (3 lines):**
```rust
/// Interactive model picker using ratatui full-screen TUI.
/// Returns (provider_name, model_name) if switched, None if cancelled.
pub fn pick_model_interactive(&mut self) -> Option<(String, String)> {
    crate::model_picker::run(self)
}
```

**Diff:** -81 lines

### 4. Cleanup model_picker.rs

**File:** `agentic-cli/src/model_picker.rs`

**Changes:**
- ✅ Removed unused import: `anyhow::Result`
- ✅ Removed unused import: `Stdout`
- ✅ Removed unused struct fields: `pi: usize, mi: usize` dari `PickerItem`
- ✅ Fixed deprecation warning: `f.size()` → `f.area()`
- ✅ Fixed config access: `commands.config()` → `commands.get_config()`

## Architecture Alignment

### Before
```
interactive.rs
    └─ Commands::pick_model_interactive()
           └─ dialoguer::FuzzySelect (third-party)
                  ├─ ColorfulTheme
                  └─ inline selection dengan ANSI escapes
```

### After
```
interactive.rs
    └─ Commands::pick_model_interactive()
           └─ model_picker::run()
                  └─ ratatui full-screen TUI
                         ├─ Terminal (alternate screen)
                         ├─ List widget
                         ├─ Paragraph widget
                         └─ Block widget
```

**Aligned with:** `docs/shared-widgets-architecture-26052026.md`

## Features

### Full-Screen TUI Experience

- **Alternate screen**: Tidak mengotori terminal history
- **Raw mode**: Keyboard input tanpa line buffering
- **Graceful suspension**: Reedline REPL suspended → restored
- **Terminal restore**: Proper cleanup via `TerminalGuard` drop

### User Interface

**Layout:**
```
╭─ Select Model ────────────────────────────╮
│ > type to filter…                         │
│                                            │
│ ✓ gpt-4o [openai]  👁                     │
│   claude-3-5-sonnet [anthropic]  👁       │
│   gemini-2.0-flash [google]  👁           │
│   llama-3.1-70b [groq]                    │
│                                            │
│ ↑/↓ navigate  enter select  esc cancel    │
│ type to filter  ctrl+u clear filter       │
╰────────────────────────────────────────────╯
```

**Visual Indicators:**
- `✓` Active model marker (green, bold)
- `👁` Vision capability icon (light blue)
- `[provider]` Provider badge (dim gray)

### Keyboard Controls

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `Enter` | Select model and switch |
| `Esc` / `q` | Cancel (return to REPL) |
| Type characters | Filter list in real-time |
| `Ctrl+U` | Clear filter |
| `Backspace` | Remove last filter character |

### Real-Time Filtering

```rust
impl Picker {
    fn apply_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered_indices.clear();
        for (i, item) in self.items.iter().enumerate() {
            if query.is_empty()
                || item.display.to_lowercase().contains(&query)
                || item.provider.to_lowercase().contains(&query)
                || item.model.to_lowercase().contains(&query)
            {
                self.filtered_indices.push(i);
            }
        }
        // Reset selection to first match
        if self.filtered_indices.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }
}
```

## Code Metrics

| Metric | Before | After | Δ |
|--------|--------|-------|---|
| Lines in `pick_model_interactive()` | 84 | 3 | **-81** |
| External picker dependency | dialoguer | ratatui (already used) | **-0** new deps |
| Consistency with TUI mode | ❌ Partial | ✅ Full | ✅ |
| Model picker implementations | 2 (duplicated) | 1 (unified) | ✅ |

## Build Verification

```bash
$ cargo check --package agentic-cli
    Checking agentic-cli v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.13s

$ cargo build --release --package agentic-cli
    Finished `release` profile [optimized] target(s) in 0.14s
```

**Warnings:** 2 unused functions di `widgets/inline.rs` (unrelated, pre-existing)

## Testing

### Manual Test Plan

1. **Start interactive mode:**
   ```bash
   agentic interactive
   ```

2. **Trigger model picker:**
   ```
   > /models
   ```
   or
   ```
   > /m
   ```

3. **Verify full-screen modal:**
   - [ ] Screen clears and shows bordered modal
   - [ ] Active model has `✓` marker
   - [ ] Vision-capable models show `👁` icon
   - [ ] Filter prompt shows "type to filter…"

4. **Test filtering:**
   - Type `gpt` → should filter to OpenAI models
   - Type `claude` → should filter to Anthropic models
   - Press `Ctrl+U` → should clear filter

5. **Test navigation:**
   - Press `↓` or `j` → cursor moves down
   - Press `↑` or `k` → cursor moves up
   - Selection should highlight with darker background

6. **Test selection:**
   - Navigate to different model
   - Press `Enter`
   - Should see success message: "Switched to {provider} / {model}"

7. **Test cancel:**
   - Press `/models` again
   - Press `Esc` or `q`
   - Should return to REPL without switching

8. **Verify terminal state:**
   - After picker exits, terminal should be clean
   - REPL prompt should be intact
   - No leftover UI artifacts

## Benefits

### 1. Consistency
- **Single widget system**: Both TUI mode and CLI mode use ratatui
- **Unified styling**: Same colors, borders, and layout patterns
- **Predictable behavior**: Users get consistent experience

### 2. Maintainability
- **Single source of truth**: Only `model_picker.rs` defines picker behavior
- **Less code duplication**: Removed 81 lines of manual list building
- **Easier to update**: Changes to picker apply everywhere

### 3. Better UX
- **Full-screen focus**: Less distracting than inline list
- **Real-time filtering**: See results as you type
- **Rich visual feedback**: Icons, colors, and clear selection state
- **No history pollution**: Alternate screen keeps terminal clean

### 4. Architecture Alignment
- **Follows shared widgets pattern**: Producer → Widget → Renderer
- **Complies with documented architecture**: See `shared-widgets-architecture-26052026.md`
- **Uses existing infrastructure**: No new dependencies or patterns

### 5. Future-Proof
- **Extensible**: Easy to add features like multi-select, grouping by provider
- **Testable**: Picker logic is self-contained and mockable
- **Portable**: Could be reused for other selection UIs (tools, providers)

## Related Documentation

- **Architecture**: `docs/shared-widgets-architecture-26052026.md`
- **Implementation**: `agentic-cli/src/model_picker.rs`
- **Integration point**: `agentic-cli/src/interactive.rs` (lines 1131-1132)
- **Command handler**: `agentic-cli/src/commands.rs::pick_model_interactive()`

## Future Enhancements (Optional)

### Potential Features
- [ ] **Multi-select mode**: Select multiple models to compare
- [ ] **Provider grouping**: Organize models by provider with collapsible sections
- [ ] **Model details panel**: Show capabilities, context window, pricing on hover
- [ ] **Fuzzy search**: Match models by partial/fuzzy strings (e.g., `gpt4` → `gpt-4o`)
- [ ] **Sorting options**: Sort by name, provider, capabilities
- [ ] **Recent models**: Show recently used models at the top
- [ ] **Favorites**: Pin frequently used models

### Testing
- [ ] Add unit tests for `Picker::build()` logic
- [ ] Add unit tests for `Picker::apply_filter()` edge cases
- [ ] Add integration test for full picker flow
- [ ] Document test scenarios in comments

### Documentation
- [ ] Update help text (`/help`) dengan keyboard shortcuts detail
- [ ] Add GIF/video demo to README showing picker in action
- [ ] Document picker extension points for custom providers

## Dependencies

**Note:** `dialoguer` is still used elsewhere and cannot be removed:
- `config_init_wizard()` - Provider selection
- `config_init_provider()` - API key input
- Various confirmation prompts throughout CLI

This is expected and acceptable. The goal was consistency, not dependency removal.

## Conclusion

✅ **Implementation complete and verified.**

The model picker is now fully integrated with the ratatui-based shared widgets architecture. Users get a consistent, polished full-screen experience when selecting models in interactive mode, and the codebase is simpler and more maintainable.

---

**Implemented by:** Kiro  
**Date:** 2026-06-02  
**Verified:** Build success, no errors
