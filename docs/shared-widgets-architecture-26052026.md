# Shared Widgets Architecture

> Date: 2026-05-26 (last update: 2026-05-29)
> Status: Implemented

## Current Integration Status

| Module | Uses Shared Widgets | Notes |
|--------|--------------------|---------|
| `tui/app.rs` | ✓ `widgets::progress::ProgressState` | Full-screen mode |
| `tui/ui.rs` | ✓ `widgets::markdown`, `widgets::spinner`, `widgets::tool_call`, `widgets::diff` | Tool calls + results + diffs render through the same widgets as inline mode |
| `interactive.rs` | ✓ `widgets::inline`, `widgets::components`, `widgets::markdown` | All output uses shared widgets, zero raw ANSI |
| `commands.rs` | ✓ full widgets stack including transient spinner via `widgets::spinner` + `inline::print_transient` | All `println!`-based ANSI replaced |
| `main.rs` | ✓ `widgets::inline`, `widgets::components`, `widgets::capabilities` | Bootstrap messages use badges; `--color` flag drives `capabilities::set_color_enabled` |
| `confirmation.rs` | ✓ `widgets::components::panel` | Risk colour leaks through panel border; old hard-coded fixed-width box removed |

Zero raw `\x1b[` escapes remain in `agentic-cli/src/`.

## Overview

The `agentic-cli` uses `ratatui` in two modes:

1. **TUI mode** (`agentic tui`) — full-screen alternate screen with raw mode
2. **CLI mode** (`agentic run`, `agentic interactive`) — inline terminal output, no alternate screen

Both modes share a common `widgets` module that produces ratatui primitives (`Line`, `Span`, `Style`) as a structured styled-text layer.

## Directory Structure

```
agentic-cli/src/
├── widgets/              # Shared components (ratatui primitives)
│   ├── mod.rs            # Module root
│   ├── capabilities.rs   # Color/TTY detection (NO_COLOR, TERM=dumb, isatty, --color override)
│   ├── inline.rs         # Inline renderer: Line/Text → stdout, transient updates, line_to_ansi
│   ├── markdown.rs       # Markdown parser → Vec<Line<'static>>
│   ├── progress.rs       # Progress state (spinner frames, bars, elapsed time)
│   ├── spinner.rs        # Higher-level Line builders for spinners/status
│   ├── components.rs     # Rich UI components: panels, badges, headers, gradients
│   ├── tool_call.rs      # Tool call panel + tool result notification
│   └── diff.rs           # Unified-diff renderer + summary line
├── tui/                  # Full-screen TUI (consumes widgets via Frame)
│   ├── app.rs            # App state, AppMessage event channel, MessageRole variants
│   ├── ui.rs             # Rendering — special-cases tool roles to use widget renderers
│   ├── dropdown.rs       # Dropdown widget (TUI-specific)
│   └── input.rs          # Input rendering (TUI-specific)
├── confirmation.rs       # Risk-coloured panel for tool confirmations
├── interactive.rs        # Reedline REPL (uses all widgets for output)
└── main.rs               # Registers `mod widgets`, drives capability override

core-agentic/src/
├── diff_util.rs          # Unified-diff producer (similar=2.6) consumed by widgets::diff
├── events.rs             # Event enum + EventEmitter (Mutex<Vec<...>> for &self.on())
└── orchestrator.rs       # on_event() / clear_event_handlers() public surface
```

## How It Works

```
Producers (core-agentic)            Widgets (agentic-cli/src/widgets)
─────────────────────────           ──────────────────────────────────
Orchestrator emits Event ───┐
                            ├─►  tool_call::render_call    ──┐
Tool result JSON {diff: …} ─┘    tool_call::render_result    │
                                 diff::render / summary_line │
                                 markdown::MarkdownContent   ├─► Vec<Line<'static>>
                                 components::*               │
                                 spinner::*                  │
                                                             ▼
                            Renderers
                            ─────────
                            TUI mode:    ratatui Frame  (Paragraph + scroll)
                            CLI mode:    inline::print_lines (stdout + ANSI)
```

### TUI Mode

Uses ratatui's `Terminal` with `CrosstermBackend`, alternate screen, and raw mode. Widgets are rendered into a `Frame` via the standard ratatui draw loop.

```rust
// tui/ui.rs
match message.role {
    MessageRole::Tool => {
        let (name, args) = parse_tool_call_payload(&message.content)?;
        all_lines.extend(tool_call::render_call(&name, &args));
    }
    MessageRole::ToolResult => {
        let (name, output) = parse_tool_result_payload(&message.content)?;
        all_lines.extend(tool_call::render_result(&name, &output, false, 12, false));
        if let Some(diff) = extract_diff_string(&output) {
            all_lines.push(diff::summary_line(&diff));
            all_lines.extend(diff::render(&diff));
        }
    }
    // …
}
```

### CLI Mode (Inline)

Uses `widgets::inline` to print styled `Line`s directly to stdout. No alternate screen, no raw mode — output goes into terminal scrollback.

```rust
use crate::widgets::{inline, tool_call, diff};

let lines = tool_call::render_call("read_file", &args);
inline::print_lines(&lines);

let result_lines = tool_call::render_result("read_file", &output, false, 12, false);
inline::print_lines(&result_lines);

if let Some(diff_text) = extract_diff(&output) {
    inline::print_line(&diff::summary_line(diff_text));
    inline::print_lines(&diff::render(diff_text));
}
```

## API Reference

### `widgets::tool_call`

Renders agent tool invocations and their results.

| Item | Description |
|------|-------------|
| `render_call(tool_name, arguments)` | Bordered panel titled `tool · <name>` with `key = value` arg rows |
| `render_result(tool_name, output, is_error, max_body_lines, verbose)` | Notification accent + truncated output body. `verbose=false` shows only the headline for success, full body for errors. |

Long outputs are clipped after `max_body_lines` and the suffix `… N more line(s) truncated` is rendered in dim style.

### `widgets::diff`

Styled renderer for unified-diff text (no diff *computation* — that's done in `core_agentic::diff_util`).

| Item | Description |
|------|-------------|
| `render(diff)` | One styled `Line` per input line; recognises `--- ` / `+++ ` / `@@` / `+` / `-` |
| `summary_line(diff)` | `+12 −3  in 2 hunks` summary span |

Producers: `tools/edit_file.rs` and `tools/write_file.rs` attach a `diff` field to their JSON result. The CLI parses the embedded JSON in `render_event` and routes it through `widgets::diff`.

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
| `print_line(line)` | Print a single `Line` with ANSI styling + newline. Auto-strips styling when `should_use_color()` is false. |
| `print_lines(lines)` | Print multiple `Line`s |
| `print_text(text)` | Print a `Text` block |
| `print_rule(ch, style)` | Print a full-width horizontal rule |
| `print_blank()` | Print an empty line |
| `print_transient(line)` | In-place update via `\r` + clear-line. No-op when stdout isn't a TTY. |
| `clear_transient()` | Clear the current transient line |
| `line_to_ansi(line)` | Render a `Line` to an ANSI-encoded `String`. Used to feed widget output into third-party APIs that take `&str` (dialoguer, indicatif). Honors capability detection. |
| `terminal_width()` | Get terminal width (default 80, floor 40) |

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
| `spinner_line(progress)` | Styled spinner glyph + message. **No elapsed time** — motion comes from frame advance. |
| `progress_bar_line(progress, width)` | Styled progress bar |
| `compact_progress_line(progress, width)` | Single-line: spinner + message + animated bar. Used by `commands.rs::run` so the user sees motion without numeric noise. |
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
- **No live updates** — `inline::print_transient` works around this with `\r` + clear-line for single-line spinners

### Things to watch

| Concern | Mitigation |
|---------|------------|
| Color support detection | ✓ Implemented in `widgets::capabilities` (NO_COLOR, TERM=dumb, isatty, --color override). `inline::print_line` and `inline::line_to_ansi` strip styling automatically when disabled. |
| Terminal width changes | Query `crossterm::terminal::size()` at render time, not cached |
| Unicode display width | Emoji/CJK chars have variable width; use `unicode-width` if needed |
| Piped output | ✓ Auto-stripped via `capabilities::is_stdout_tty()` check in `should_use_color()` |
| Scrollback pollution | Be intentional — inline output stays in history. Tool result rendering is compact (headline only) by default. |
| Live-updating lines | ✓ `inline::print_transient` + `inline::clear_transient` (no-op on non-TTY) |

## Event Subscriptions

The `Orchestrator` exposes `on_event(handler)` and `clear_event_handlers()`
so the CLI can subscribe to runtime events without coupling rendering to
orchestrator internals. Both CLI inline mode and TUI mode now subscribe.

Emitted events:

| Event | Where it fires | Consumed by |
|-------|----------------|-------------|
| `Event::ToolCall { tool_name, arguments }` | Before each tool call (also for denied/skipped calls so the operator sees the attempt) | `widgets::tool_call::render_call` |
| `Event::ToolOutput { tool_name, output }` | After each tool result, including blocked/skipped strings | `widgets::tool_call::render_result` + `widgets::diff` when payload carries a `diff` field |
| `Event::Error { message }` | Provider/tool error surfacing | `components::error_badge` |
| `Event::System { message }` | Status notifications | `components::info_badge` |

`EventEmitter` uses `Mutex<Vec<Box<dyn Fn(Event) + Send + Sync>>>` so handlers can be registered through `&self`.

### CLI inline subscription (`commands.rs::run`)

Subscription is gated by `config.output.show_tool_calls`.

1. `orchestrator.clear_event_handlers()` to drop subscribers from any previous run.
2. Open a `tokio::mpsc::unbounded_channel`; register a handler that pushes events into the sender.
3. The spinner ticker uses `tokio::select!` to interleave spinner ticks with event drains. On each event it calls `inline::clear_transient` then `inline::print_lines` of the rendered widget; the next tick redraws the spinner.
4. After `run_stream` returns, `clear_event_handlers()` releases the sender, the channel closes, and the ticker loop exits.

The free function `render_event(event)` decodes the embedded JSON in `Event::ToolOutput` to extract a `diff` field when present, so `edit_file`/`write_file` results render with inline colored diffs.

### TUI subscription (`tui/app.rs`)

The TUI uses the existing `mpsc::UnboundedSender<AppMessage>` channel that already drives streaming chunks. Tool events are mapped to two new variants:

```rust
pub enum AppMessage {
    StreamChunk(String),
    TaskComplete(String),
    Error(String),
    Progress(String),
    ToolCall { name: String, arguments: serde_json::Value },
    ToolResult { name: String, output: serde_json::Value, is_error: bool },
}
```

`Commands::run_with_callbacks(task, on_chunk, on_event)` subscribes the event handler before kicking off the agent loop and clears it on completion. The handler clones the channel sender into the closure.

In the message log, three new `MessageRole` variants drive distinct rendering:

```rust
pub enum MessageRole {
    User, Assistant, System, Error,
    Tool,        // → widgets::tool_call::render_call
    ToolResult,  // → widgets::tool_call::render_result (success path) + widgets::diff
    ToolError,   // → widgets::tool_call::render_result (error path, body always shown)
}
```

Tool messages bypass the markdown renderer; the structured payload is stashed as a JSON envelope in `Message::content` and decoded by `parse_tool_call_payload` / `parse_tool_result_payload` helpers in `tui/ui.rs`.

## Diff Producer

`core_agentic::diff_util` produces unified diffs from before/after content using `similar=2.6`.

| Item | Description |
|------|-------------|
| `unified_diff(path, before, after, ctx)` | Standard `--- a/path / +++ b/path / @@` format |
| `change_summary(before, after) → ChangeStats { added, removed }` | Line counts |

Producers:
- `tools/edit_file.rs::apply_replacement` reads the buffer, runs the replacement, then attaches `diff`, `lines_added`, `lines_removed` to the JSON result.
- `tools/write_file.rs::execute` does the same and adds a `created: bool` flag (treats "file does not exist" as empty before-state so creates show purely additions).

Consumer: `agentic-cli/src/commands.rs::render_event` parses the embedded JSON, pulls the `diff` field if non-empty, and renders via `widgets::diff::render` + `summary_line`. Caps at 40 lines with a `… diff truncated` marker beyond that. The TUI's `tui/ui.rs::extract_diff_string` does the same dance.

## Token-Budget Context Builder

`Memory::get_context_for_request(token_budget)` is the production-grade context slicer the orchestrator uses on every turn. Replaces the legacy `get_context(20)` message-count slice that produced HTTP 400 from providers when the slice cut mid-pair.

Properties:

1. **Walk turns, not messages.** A turn = one `user` message followed by zero or more `assistant`/`tool` pairs until the next user. Tool_call/result pairs are never split.
2. **Token budget, not message count.** Walks turns newest-first, accumulates estimated tokens, stops at the next turn boundary that would exceed budget. Most recent turn is always included even if it alone exceeds budget.
3. **Anchored to user.** Output always starts with `user` (or system summary + user), satisfying provider requirements.
4. **Summary prepended.** If `Memory::summary` is set (compaction has run), it's prepended as a system message.

`Memory::request_budget()` exposes the configured budget: `max_tokens * config.context_budget_ratio` (default `0.7`, clamped to `[0.1, 0.95]`). Anthropic-style providers may want a lower ratio (0.5–0.6) since their tool definitions tend to be verbose.

`sanitize_for_provider` in `orchestrator.rs` remains as defense-in-depth, dropping any malformed message that slips through (orphan tool results, dangling tool_calls, empty assistant turns, leading non-user messages, duplicate system messages). It is no-op on the happy path.

### Tokenizer backends

| Mode | Default | With `tiktoken` feature |
|------|---------|------------------------|
| Encoder | `len() / 4` heuristic | `cl100k_base` BPE via `tiktoken-rs` |
| Accuracy | ±30% | ±2% |
| Cost | Zero deps | ~10MB binary, ~50ms cold start |

Enable: `cargo build --features core-agentic/tiktoken`

## Adding New Shared Widgets

1. Create a new file in `agentic-cli/src/widgets/` (e.g. `table.rs`).
2. Have it produce `Vec<Line<'static>>` or similar ratatui primitives.
3. Register it in `agentic-cli/src/widgets/mod.rs`.
4. Consume from TUI via `Frame` rendering inside `tui/ui.rs::draw_messages`.
5. Consume from CLI via `inline::print_lines()` in `commands.rs` or `interactive.rs`.

Keep widget logic **output-agnostic** — they produce data, not side effects. The rendering target (Frame vs stdout) is decided by the caller. If the data needs to come from `core-agentic` (e.g. structured tool results), add a producer there and parse it from the `Event` payload at the rendering boundary.
