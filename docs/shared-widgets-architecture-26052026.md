# Shared Widgets Architecture

> Date: 2026-05-26 (last update: 2026-05-29)
> Status: Implemented

## Current Integration Status

| Module | Uses Shared Widgets | Notes |
|--------|--------------------|---------|
| `tui/app.rs` | ✓ `widgets::progress::ProgressState` | Full-screen mode |
| `tui/ui.rs` | ✓ `widgets::markdown::MarkdownContent` | Full-screen mode |
| `interactive.rs` | ✓ `widgets::inline`, `widgets::components`, `widgets::markdown` | All output uses shared widgets, zero raw ANSI |
| `commands.rs` | ✓ `widgets::inline`, `widgets::components`, `widgets::markdown`, `widgets::capabilities` | All `println!`-based ANSI replaced with shared widgets (one ANSI escape kept inside a dialoguer label, gated by capability detection) |
| `main.rs` | ✓ `widgets::inline`, `widgets::components`, `widgets::capabilities` | Bootstrap messages use badges; `--color` flag drives `capabilities::set_color_enabled` |

## Overview

The `agentic-cli` uses `ratatui` in two modes:

1. **TUI mode** (`agentic tui`) — full-screen alternate screen with raw mode
2. **CLI mode** (`agentic run`, `agentic interactive`) — inline terminal output, no alternate screen

Both modes share a common `widgets` module that produces ratatui primitives (`Line`, `Span`, `Style`) as a structured styled-text layer.

## Directory Structure

```
src/
├── widgets/              # Shared components (ratatui primitives)
│   ├── mod.rs            # Module root
│   ├── capabilities.rs   # Color/TTY detection (NO_COLOR, TERM=dumb, isatty, --color override)
│   ├── inline.rs         # Inline renderer: Line/Text → stdout (no raw mode)
│   ├── markdown.rs       # Markdown parser → Vec<Line<'static>>
│   ├── progress.rs       # Progress state (spinner frames, bars, elapsed time)
│   ├── spinner.rs        # Higher-level Line builders for spinners/status
│   └── components.rs     # Rich UI components: panels, badges, headers, gradients
├── tui/                  # Full-screen TUI (consumes widgets via Frame)
│   ├── app.rs            # App state, imports widgets::progress::ProgressState
│   ├── ui.rs             # Rendering, imports widgets::markdown::MarkdownContent
│   ├── dropdown.rs       # Dropdown widget (TUI-specific)
│   └── input.rs          # Input rendering (TUI-specific)
├── interactive.rs        # Reedline REPL (uses all widgets for output)
└── main.rs              # Registers `mod widgets`, drives capability override
```

## How It Works

```
┌──────────────────────────────────────────────────────┐
│  widgets/markdown.rs   →  Vec<Line<'static>>         │
│  widgets/progress.rs   →  ProgressState              │
│  widgets/spinner.rs    →  Line<'static>              │
├──────────────────────────────────────────────────────┤
│  TUI mode:   render Lines into Frame (ratatui)       │
│  CLI mode:   render Lines into stdout (inline.rs)    │
└──────────────────────────────────────────────────────┘
```

### TUI Mode

Uses ratatui's `Terminal` with `CrosstermBackend`, alternate screen, and raw mode. Widgets are rendered into a `Frame` via the standard ratatui draw loop.

```rust
// tui/ui.rs
use crate::widgets::markdown::MarkdownContent;

let md = MarkdownContent::parse(&message.content);
// Render md.lines into a Paragraph widget inside Frame
```

### CLI Mode (Inline)

Uses `widgets::inline` to print styled `Line`s directly to stdout. No alternate screen, no raw mode — output goes into terminal scrollback.

```rust
use crate::widgets::inline;
use crate::widgets::markdown::MarkdownContent;
use crate::widgets::spinner;

// Render markdown inline
let md = MarkdownContent::parse("## Hello\n\n- item 1\n- item 2");
inline::print_lines(&md.lines);

// Status lines
inline::print_line(&spinner::done_line(1500));
inline::print_line(&spinner::error_line("connection failed"));

// Horizontal rule
use ratatui::style::{Color, Style};
inline::print_rule('─', Style::default().fg(Color::DarkGray));
```

## API Reference

### `widgets::capabilities`

Centralizes color/TTY decisions so widgets and helpers don't each re-check
environment state.

| Function | Description |
|----------|-------------|
| `set_color_enabled(Option<bool>)` | Force color on/off; `None` = auto-detect |
| `should_use_color()` | Master decision: override > NO_COLOR > TERM=dumb > stdout TTY |
| `is_stdout_tty()` | Cached `std::io::stdout().is_terminal()` check |

Precedence:
1. Explicit override from `--color always|never` (set in `main.rs`).
2. `NO_COLOR` env var (any non-empty value disables color).
3. `TERM=dumb` disables color.
4. Falls back to TTY check on stdout (so piped output is plain).

### `widgets::inline`

| Function | Description |
|----------|-------------|
| `print_line(line)` | Print a single `Line` with ANSI styling + newline |
| `print_lines(lines)` | Print multiple `Line`s |
| `print_text(text)` | Print a `Text` block |
| `print_rule(ch, style)` | Print a full-width horizontal rule |
| `print_blank()` | Print an empty line |
| `terminal_width()` | Get terminal width (default 80) |

### `widgets::markdown`

| Item | Description |
|------|-------------|
| `MarkdownContent::parse(md)` | Parse markdown into `Vec<Line<'static>>` |
| `role_prefix(role)` | Get styled prefix span for a message role |

### `widgets::progress`

| Item | Description |
|------|-------------|
| `ProgressState::new()` | Create inactive progress state |
| `.start()` | Begin progress (resets frame, starts timer) |
| `.stop()` | End progress |
| `.tick()` | Advance spinner frame |
| `.spinner()` | Get current spinner character |
| `.elapsed_str()` | Get formatted elapsed time |
| `.progress_bar(width)` | Render progress bar string |
| `.display()` | Full progress display string (plain text) |

### `widgets::spinner`

| Function | Description |
|----------|-------------|
| `spinner_line(progress)` | Styled spinner + message + elapsed |
| `progress_bar_line(progress, width)` | Styled progress bar |
| `compact_progress_line(progress, width)` | Single-line: spinner + message + bar + time |
| `done_line(elapsed_ms)` | `✓ Done (1.5s)` |
| `error_line(message)` | `✗ Error: <message>` |

### `widgets::components`

Rich visual components for beautiful CLI output.

| Function | Description |
|----------|-------------|
| `panel(title, content, style, color)` | Bordered box with title |
| `box_content(content, style, color)` | Bordered box (no title) |
| `section_header(icon, title, color)` | `── 📊 Statistics ────────` |
| `subsection_header(title, color)` | Bold sub-section title |
| `kv_line(key, value, width, color)` | Aligned `Key:  value` |
| `kv_badge(key, value, width, fg, bg)` | Aligned `Key: [badge]` |
| `success_badge(msg)` | ` ✓ message ` (green bg) |
| `error_badge(msg)` | ` ✗ message ` (red bg) |
| `warning_badge(msg)` | ` ⚠ message ` (yellow bg) |
| `info_badge(msg)` | ` ℹ message ` (blue bg) |
| `gradient_text(text, from, to)` | Horizontal RGB gradient |
| `banner_title(text, from, to)` | Bold gradient title |
| `dotted_separator(color)` | `· · · · · ·` |
| `dashed_separator(color)` | `╌╌╌╌╌╌` |
| `double_separator(color)` | `══════` |
| `labeled_bar(label, value, width, fill, empty)` | `Input: ███░░  60%` |
| `sparkline(values, color)` | `▁▂▅▇█▅▃` mini chart |
| `table(headers, rows, h_color, b_color)` | Simple aligned table |
| `notification(icon, msg, color)` | `┃ ✓ message` accent line |

**Box styles:** `Single`, `Double`, `Rounded`, `Heavy`

## Constraints & Considerations

### What ratatui gives us in CLI mode

- **Structured styled text** — `Line<'static>` / `Span` / `Style` as composable data
- **Shared types** — same color/style definitions across TUI and CLI
- **Consistent rendering** — markdown looks the same in both modes

### What ratatui does NOT give us in CLI mode

- **No diffing** — every print is append-only, no partial redraws
- **No Layout system** — `Layout` needs a `Rect` (fixed area), not applicable inline
- **No Widget trait** — `Paragraph`, `Table`, `Block` render into `Frame`, not stdout
- **No live updates** — for in-place spinners, use `\r` + clear-line or `indicatif`

### Things to watch

| Concern | Mitigation |
|---------|------------|
| Color support detection | ✓ Implemented in `widgets::capabilities` (NO_COLOR, TERM=dumb, isatty, --color override). `inline::print_line` strips styling automatically when disabled. |
| Terminal width changes | Query `crossterm::terminal::size()` at render time, not cached |
| Unicode display width | Emoji/CJK chars have variable width; use `unicode-width` if needed |
| Piped output | ✓ Auto-stripped via `capabilities::is_stdout_tty()` check in `should_use_color()` |
| Scrollback pollution | Be intentional — inline output stays in history |
| Live-updating lines | Use `\r` + ANSI clear for single-line updates (spinners) |

## Adding New Shared Widgets

1. Create a new file in `src/widgets/` (e.g., `table.rs`)
2. Have it produce `Vec<Line<'static>>` or similar ratatui primitives
3. Register it in `src/widgets/mod.rs`
4. Consume from TUI via `Frame` rendering
5. Consume from CLI via `inline::print_lines()`

Keep widget logic **output-agnostic** — they produce data, not side effects. The rendering target (Frame vs stdout) is decided by the caller.
