# Ratatui Input Widget Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace reedline with a custom ratatui-based input widget for the interactive REPL, unifying all rendering through `inline.rs`.

**Architecture:** Single event loop using crossterm raw mode for key capture, `InputBuffer` for state, `tui/input.rs` + `tui/dropdown.rs` for rendering, `inline.rs` for output. No more dual-mode (reedline cooked + InputWatcher raw) — one input system for all phases.

**Tech Stack:** crossterm (raw mode + key events), ratatui (Line/Span rendering), existing `tui/dropdown.rs` + `tui/input.rs` modules.

---

### Task 1: Create `InputBuffer` struct

**Files:**
- Create: `agentic-cli/src/input_buffer.rs`
- Modify: `agentic-cli/src/main.rs` (add `mod input_buffer`)

**Step 1: Create `InputBuffer` with core methods**

```rust
//! Custom input buffer replacing reedline for interactive REPL mode.
//!
//! Provides single-line text editing, cursor management, and in-memory history.

/// Maximum input length (single-line mode)
const MAX_INPUT_LEN: usize = 4096;

/// Input buffer with cursor position and in-memory history.
#[derive(Debug)]
pub struct InputBuffer {
    /// Current input text
    text: String,
    /// Cursor position (byte offset within `text`)
    cursor: usize,
    /// Submitted input history (most recent last)
    history: Vec<String>,
    /// Current history browse index (None = not browsing)
    history_idx: Option<usize>,
    /// Saved input when user started browsing history
    saved_input: String,
}

impl InputBuffer {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_idx: None,
            saved_input: String::new(),
        }
    }

    /// Current input text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Current cursor position (byte offset)
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Is input empty?
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Insert a character at cursor position
    pub fn insert_char(&mut self, c: char) {
        if self.text.len() + c.len_utf8() > MAX_INPUT_LEN {
            return;
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete character before cursor (Backspace)
    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete character at cursor (Delete key)
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.text.drain(self.cursor..next);
    }

    /// Delete from cursor to start of previous word
    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Skip trailing whitespace
        let mut pos = self.cursor;
        while pos > 0 && self.text[..pos].ends_with(char::is_whitespace) {
            let prev = self.text[..pos].char_indices().last().map(|(i,_)| i).unwrap_or(0);
            pos = prev;
        }
        // Skip word characters
        while pos > 0 {
            let prev = self.text[..pos].char_indices().last().map(|(i,_)| i).unwrap_or(0);
            if self.text[prev..pos].chars().next().map_or(false, |c| c.is_whitespace()) {
                break;
            }
            pos = prev;
        }
        self.text.drain(pos..self.cursor);
        self.cursor = pos;
    }

    /// Move cursor left one character
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right one character
    pub fn cursor_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Move cursor to start (Home)
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end (End)
    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Move cursor to start of previous word (Ctrl+Left)
    pub fn cursor_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut pos = self.cursor;
        // Skip whitespace
        while pos > 0 {
            let prev = self.text[..pos].char_indices().last().map(|(i,_)| i).unwrap_or(0);
            if !self.text[prev..pos].chars().next().map_or(true, |c| c.is_whitespace()) {
                break;
            }
            pos = prev;
        }
        // Skip word
        while pos > 0 {
            let prev = self.text[..pos].char_indices().last().map(|(i,_)| i).unwrap_or(0);
            if self.text[prev..pos].chars().next().map_or(false, |c| c.is_whitespace()) {
                break;
            }
            pos = prev;
        }
        self.cursor = pos;
    }

    /// Move cursor to start of next word (Ctrl+Right)
    pub fn cursor_word_right(&mut self) {
        let len = self.text.len();
        if self.cursor >= len {
            return;
        }
        let mut pos = self.cursor;
        // Skip word
        while pos < len {
            let next = self.text[pos..].char_indices().nth(1).map(|(i,_)| pos + i).unwrap_or(len);
            if self.text[pos..next].chars().next().map_or(true, |c| c.is_whitespace()) {
                break;
            }
            pos = next;
        }
        // Skip whitespace
        while pos < len {
            let next = self.text[pos..].char_indices().nth(1).map(|(i,_)| pos + i).unwrap_or(len);
            if !self.text[pos..next].chars().next().map_or(true, |c| c.is_whitespace()) {
                break;
            }
            pos = next;
        }
        self.cursor = pos;
    }

    /// Clear entire input
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Submit input — pushes to history, returns the text, clears buffer
    pub fn submit(&mut self) -> String {
        let input = self.text.trim().to_string();
        if !input.is_empty() {
            // Don't push duplicate of last history entry
            if self.history.last() != Some(&input) {
                self.history.push(input.clone());
            }
        }
        self.text.clear();
        self.cursor = 0;
        self.history_idx = None;
        input
    }

    /// Navigate history up (older entries)
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_idx.is_none() {
            self.saved_input = self.text.clone();
            self.history_idx = Some(self.history.len() - 1);
        } else if let Some(idx) = self.history_idx {
            if idx > 0 {
                self.history_idx = Some(idx - 1);
            }
        }
        if let Some(idx) = self.history_idx {
            self.text = self.history[idx].clone();
            self.cursor = self.text.len();
        }
    }

    /// Navigate history down (newer entries)
    pub fn history_down(&mut self) {
        if let Some(idx) = self.history_idx {
            if idx + 1 >= self.history.len() {
                // Back to saved input
                self.history_idx = None;
                self.text = self.saved_input.clone();
                self.cursor = self.text.len();
            } else {
                self.history_idx = Some(idx + 1);
                self.text = self.history[idx + 1].clone();
                self.cursor = self.text.len();
            }
        }
    }
}
```

**Step 2: Run `cargo check`**

Run: `cargo check -p agentic-cli 2>&1 | tail -5`
Expected: 0 errors (just warnings about unused code)

**Step 3: Add unit tests to `input_buffer.rs`**

Add test module at bottom of file with tests for:
- `insert_char`, `delete_backward`, `delete_forward`
- `cursor_left`, `cursor_right`, `cursor_home`, `cursor_end`
- `cursor_word_left`, `cursor_word_right`, `delete_word_backward`
- `history_up`, `history_down`, `submit`
- Edge cases: empty buffer, cursor at boundaries

**Step 4: Run tests**

Run: `cargo test -p agentic-cli input_buffer 2>&1 | tail -20`
Expected: All tests pass

**Step 5: Commit**

```bash
git add agentic-cli/src/input_buffer.rs agentic-cli/src/main.rs
git commit -m "feat: add InputBuffer for custom ratatui input widget"
```

---

### Task 2: Create `InputRenderer` — render input + prompt + dropdown

**Files:**
- Create: `agentic-cli/src/input_renderer.rs`
- Modify: `agentic-cli/src/main.rs` (add `mod input_renderer`)

**Step 1: Create `InputRenderer`**

This module ties together:
- `input_buffer::InputBuffer` (state)
- `tui/input.rs` (syntax highlighting + cursor)
- `tui/dropdown.rs` (dropdown overlay)
- `widgets/inline.rs` (terminal output)

It provides:
- `render_prompt_input()` — render the prompt + input line via inline.rs transient
- `render_dropdown()` — render dropdown items below the prompt line
- `clear_render()` — clear the transient area before finalizing

```rust
//! Input renderer — renders prompt, input, and dropdown to stdout via inline.rs.
//!
//! Uses transient rendering (overwrite in-place) so the prompt stays at the
//! bottom of the terminal while the user types.

use crate::input_buffer::InputBuffer;
use crate::tui::dropdown::{Dropdown, DropdownType};
use crate::tui::input::{render_input, render_placeholder};
use crate::widgets::inline;
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Modifier, Style};

/// Metadata shown in the prompt area
pub struct PromptMetadata {
    pub dir_name: String,
    pub provider: String,
    pub model: String,
    pub git_branch: Option<String>,
}

impl PromptMetadata {
    pub fn new(provider: String, model: String) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let dir_name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        let git_branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if b.is_empty() || b == "HEAD" { None } else { Some(b) }
                } else { None }
            });

        Self { dir_name, provider, model, git_branch }
    }
}

/// Render the prompt line + input to stdout as a transient line.
/// Overwrites the previous prompt render in-place.
pub fn render_prompt_line(meta: &PromptMetadata, buffer: &InputBuffer) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let cyan = Style::default().fg(Color::Cyan);

    // Build prompt prefix: "dirname> "
    let prompt_prefix = format!("{}> ", meta.dir_name);

    // Build input content (highlighted or placeholder)
    let input_line = if buffer.is_empty() {
        render_placeholder()
    } else {
        render_input(buffer.text(), buffer.cursor())
    };

    // Combine: prompt prefix + input content
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(prompt_prefix, dim),
    ];
    spans.extend(input_line.spans.into_iter());

    inline::print_transient(&Line::from(spans));
}

/// Render dropdown items below the prompt line.
/// Returns the number of lines printed (for clearing later).
pub fn render_dropdown_lines(dropdown: &Dropdown) -> usize {
    if dropdown.is_empty() {
        return 0;
    }

    let visible = dropdown.visible_items();
    let icon = match dropdown.dropdown_type {
        DropdownType::Command => "⌘",
        DropdownType::File => "📁",
        DropdownType::Model => "🤖",
    };

    // Title line
    let title_style = Style::default()
        .fg(Color::Rgb(241, 196, 15))
        .add_modifier(Modifier::BOLD);
    inline::print_line(&Line::from(vec![
        Span::styled(format!("  {} {} ", icon, dropdown.title()), title_style),
    ]));

    let mut count = 1;

    for (_, item, selected) in &visible {
        let style = if *selected {
            Style::default()
                .bg(Color::Rgb(52, 152, 219))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 200))
        };

        let item_icon = match dropdown.dropdown_type {
            DropdownType::File => if item.ends_with('/') { "📁 " } else { "📄 " },
            _ => "  ",
        };

        let mut spans = vec![
            Span::styled(format!("  {}{}", item_icon, item), style),
        ];

        if let Some(desc) = dropdown.get_description(item) {
            let desc_style = if *selected {
                Style::default()
                    .bg(Color::Rgb(52, 152, 219))
                    .fg(Color::Rgb(200, 200, 200))
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!("  {}", desc), desc_style));
        }

        inline::print_line(&Line::from(spans));
        count += 1;
    }

    count
}
```

**Step 2: Run `cargo check`**

Run: `cargo check -p agentic-cli 2>&1 | tail -5`
Expected: 0 errors

**Step 3: Commit**

```bash
git add agentic-cli/src/input_renderer.rs agentic-cli/src/main.rs
git commit -m "feat: add InputRenderer for prompt + dropdown rendering"
```

---

### Task 3: Rewrite `interactive.rs` — replace reedline with custom event loop

**Files:**
- Modify: `agentic-cli/src/interactive.rs` (complete rewrite of REPL loop)
- Modify: `agentic-cli/Cargo.toml` (reedline → optional/dev dependency)

This is the core task. The REPL loop changes from:

```
loop { line_editor.read_line(&prompt) → process }
```

to:

```
enable_raw_mode();
loop {
    render_prompt_line();
    event::read() → match key → update buffer / submit
}
disable_raw_mode();
```

**Key changes:**
1. Remove all reedline imports and types (Reedline, ReedlineEvent, Signal, Prompt, Completer, Highlighter, Hinter, Validator, SqliteBackedHistory, DescriptionMenu, Emacs, etc.)
2. Remove `AgenticCompleter`, `AgenticHighlighter`, `AgenticHinter`, `AgenticValidator`, `AgenticPrompt` structs
3. Remove reedline keybinding setup
4. Add `crossterm::terminal::{enable_raw_mode, disable_raw_mode}`
5. Add `crossterm::event::{read, Event, KeyCode, KeyModifiers}`
6. New REPL loop using raw mode + `InputBuffer` + `InputRenderer` + `Dropdown`
7. Keep ALL other functionality: session management, banner, slash commands, status bar, process_message, etc.

**Step 1: Rewrite the REPL loop in `interactive.rs`**

Key structure:
```rust
pub async fn run(mut commands: Commands) -> Result<()> {
    let stats = SessionStats::new();
    let mut model_info = get_model_info(&commands);
    let mut buffer = InputBuffer::new();
    let mut dropdown: Option<Dropdown> = None;

    // Session setup (same as before)
    let cwd = ...;
    let mut current_session = ...;

    print_banner(&model_info, &stats);

    // Enter raw mode for key capture
    crossterm::terminal::enable_raw_mode()?;

    let result = repl_loop(
        &mut buffer, &mut dropdown, &mut commands,
        &mut conversation, &mut current_session,
        &stats, &model_info,
    ).await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;

    // Save session, print goodbye (same as before)
    ...
    result
}
```

The inner loop:
```rust
async fn repl_loop(...) -> Result<()> {
    let meta = PromptMetadata::new(...);

    loop {
        // Render prompt + input
        render_prompt_line(&meta, &buffer);
        if let Some(ref dd) = dropdown {
            // render dropdown below
        }

        // Read key event
        let event = crossterm::event::read()?;
        match event {
            Event::Key(key) => handle_key(key, &mut buffer, &mut dropdown, ...),
            _ => {}
        }
    }
}
```

Key handling:
- Char → `buffer.insert_char(c)` + `update_dropdown()`
- Backspace → `buffer.delete_backward()` + `update_dropdown()`
- Delete → `buffer.delete_forward()` + `update_dropdown()`
- Left/Right → `buffer.cursor_left()` / `cursor_right()`
- Home/End → `buffer.cursor_home()` / `cursor_end()`
- Ctrl+Left/Right → `buffer.cursor_word_left()` / `cursor_word_right()`
- Ctrl+W → `buffer.delete_word_backward()`
- Up/Down (no dropdown) → `buffer.history_up()` / `history_down()`
- Up/Down (with dropdown) → `dropdown.select_prev()` / `select_next()`
- Tab/Enter (with dropdown) → `accept_dropdown()`
- Esc → close dropdown
- Enter (no dropdown) → submit input
- Ctrl+C → cancel (or show exit hint)
- Ctrl+D → break loop

**Step 2: Wire up dropdown logic**

Copy dropdown trigger/accept logic from `tui/app.rs`:
- `update_dropdown()` — check for `/` or `@` triggers
- `find_at_trigger()` — find `@` position
- `accept_dropdown()` — insert selected item into buffer

**Step 3: Keep slash command handling**

The existing `handle_slash_command()` and `ReplAction` enum stay the same. Only the REPL loop changes.

**Step 4: During processing, show spinner**

When agent is running, render spinner via `inline::print_transient()` (same as current `commands.rs` ticker does). User cannot type — just wait.

**Step 5: Run `cargo check` and fix errors**

Run: `cargo check -p agentic-cli 2>&1 | tail -20`
Expected: 0 errors (may need iterative fixes)

**Step 6: Run `cargo test`**

Run: `cargo test -p agentic-cli 2>&1 | tail -20`
Expected: All tests pass

**Step 7: Commit**

```bash
git add agentic-cli/src/interactive.rs
git commit -m "feat: replace reedline with custom ratatui input widget"
```

---

### Task 4: Remove reedline dependency

**Files:**
- Modify: `agentic-cli/Cargo.toml` (remove reedline, nu-ansi-term)

**Step 1: Remove reedline from Cargo.toml**

Remove:
```toml
reedline = { version = "0.47", features = ["sqlite"] }
nu-ansi-term = "0.50"
```

Note: `nu-ansi-term` was only used by `AgenticHighlighter` which is now removed.

**Step 2: Clean up any remaining reedline imports**

Search for `reedline` across all source files and remove any remaining references.

**Step 3: Run `cargo check`**

Run: `cargo check -p agentic-cli 2>&1 | tail -10`
Expected: 0 errors

**Step 4: Run full test suite**

Run: `cargo test -p agentic-cli 2>&1 | tail -20`
Expected: All tests pass

**Step 5: Commit**

```bash
git add agentic-cli/Cargo.toml agentic-cli/Cargo.lock
git commit -m "chore: remove reedline and nu-ansi-term dependencies"
```

---

### Task 5: Smoke test and fix edge cases

**Step 1: Build release binary**

Run: `cargo build -p agentic-cli --release 2>&1 | tail -5`
Expected: Build succeeds

**Step 2: Manual smoke test checklist**

Test these scenarios:
1. Start REPL → see banner + prompt
2. Type text → see input with cursor
3. Press Backspace → character deleted
4. Press Left/Right → cursor moves
5. Press Home/End → cursor at start/end
6. Press Up/Down → history navigation
7. Type `/` → dropdown appears with commands
8. Navigate dropdown with ↑/↓ → selection moves
9. Press Tab/Enter on dropdown → command inserted
10. Type `@` → dropdown appears with files
11. Type `@src/` → files filtered to src/
12. Select file → path inserted
13. Type `/help` → help message shown
14. Submit text → agent processes, spinner shown
15. Ctrl+C during processing → cancel
16. Ctrl+D → exit
17. `/quit` → exit

**Step 3: Fix any issues found**

**Step 4: Final commit**

```bash
git add -A
git commit -m "fix: address edge cases in ratatui input widget"
```
