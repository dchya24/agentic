//! Input renderer — renders prompt, input, and dropdown to stdout via inline.rs.
//!
//! Uses transient rendering (overwrite in-place) so the prompt stays at the
//! bottom of the terminal while the user types.

use crate::input_buffer::InputBuffer;
use crate::tui::dropdown::{Dropdown, DropdownType};
use crate::tui::input::{render_input, render_placeholder};
use crate::widgets::inline;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

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
                    if b.is_empty() || b == "HEAD" {
                        None
                    } else {
                        Some(b)
                    }
                } else {
                    None
                }
            });

        Self {
            dir_name,
            provider,
            model,
            git_branch,
        }
    }
}

/// Render the prompt line + input to stdout as a transient line.
/// Overwrites the previous prompt render in-place.
///
/// Returns the number of terminal lines rendered (including continuation
/// lines and footer for multi-line mode).  The cursor is left on a NEW
/// line below the prompt so the REPL loop can MoveUp(N) correctly.
pub fn render_prompt_line(
    meta: &PromptMetadata,
    buffer: &InputBuffer,
    _has_dropdown: bool,
) -> usize {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let _cyan = Style::default().fg(Color::Cyan);

    // Build prompt prefix: "dirname> "
    let prompt_prefix = format!("{}> ", meta.dir_name);

    // Check if multi-line mode
    if buffer.is_multiline() {
        // Render multi-line input
        let lines = buffer.lines();
        let current_line = buffer.current_line();

        for (i, line) in lines.iter().enumerate() {
            if i == 0 {
                // First line with prompt
                let input_line = render_input(line, buffer.cursor().min(line.len()));
                let mut spans: Vec<Span<'static>> = vec![Span::styled(prompt_prefix.clone(), dim)];
                spans.extend(input_line.spans);
                inline::print_line(&Line::from(spans));
            } else {
                // Continuation lines with indent
                let indent = "  "; // 2 spaces for continuation
                let line_cursor = if i == current_line {
                    // Calculate cursor position within this line
                    let lines_before: usize = lines[..i].iter().map(|l| l.len() + 1).sum();
                    if buffer.cursor() > lines_before {
                        buffer.cursor() - lines_before
                    } else {
                        0
                    }
                } else {
                    0
                };

                let input_line = render_input(line, line_cursor.min(line.len()));
                let mut spans: Vec<Span<'static>> = vec![
                    Span::styled(indent.to_string(), dim),
                    Span::styled(
                        "│ ".to_string(),
                        Style::default().fg(Color::Rgb(100, 100, 120)),
                    ),
                ];
                spans.extend(input_line.spans);
                inline::print_line(&Line::from(spans));
            }
        }

        // Show multi-line indicator
        inline::print_line(&Line::from(vec![
            Span::styled("  ", dim),
            Span::styled(
                format!(
                    "[{} lines] Shift+Enter: new line, Enter: submit",
                    lines.len()
                ),
                Style::default()
                    .fg(Color::Rgb(100, 100, 120))
                    .add_modifier(Modifier::DIM),
            ),
        ]));

        lines.len() + 1 // input lines + indicator
    } else {
        // Single-line mode
        // NOTE: We use `print_line` (which appends `\r\n`) instead of
        // `print_transient` so that the cursor ends up on the NEXT line
        // after the prompt.  This lets the REPL loop's `MoveUp(1)`
        // correctly return to the prompt line rather than overshooting
        // to the status bar above it.
        let input_line = if buffer.is_empty() {
            render_placeholder()
        } else {
            render_input(buffer.text(), buffer.cursor())
        };

        // Combine: prompt prefix + input content
        let mut spans: Vec<Span<'static>> = vec![Span::styled(prompt_prefix, dim)];
        spans.extend(input_line.spans);

        inline::print_line(&Line::from(spans));
        1 // single-line input
    }
}

/// Render dropdown items below the prompt line.
/// Returns the number of lines printed (for clearing later).
pub fn render_dropdown_lines(dropdown: &Dropdown) -> usize {
    if dropdown.is_empty() {
        return 0;
    }

    let visible = dropdown.visible_items();
    let query = dropdown.query();
    let icon = match dropdown.dropdown_type {
        DropdownType::Command => "⌘",
        DropdownType::File => "📁",
        DropdownType::Model => "🤖",
        DropdownType::Skill => "⚡",
    };

    // Title line
    let title_style = Style::default()
        .fg(Color::Rgb(241, 196, 15))
        .add_modifier(Modifier::BOLD);
    inline::print_line(&Line::from(vec![Span::styled(
        format!("  {} {} ", icon, dropdown.title()),
        title_style,
    )]));

    let mut count = 1;

    for (_, item, selected) in &visible {
        let base_style = if *selected {
            Style::default()
                .bg(Color::Rgb(52, 152, 219))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 200))
        };

        let item_icon = match dropdown.dropdown_type {
            DropdownType::File => {
                if item.ends_with('/') {
                    "📁 "
                } else {
                    "📄 "
                }
            }
            _ => "  ",
        };

        // Build item text with fuzzy highlighting
        let mut spans = vec![Span::styled(format!("  {}", item_icon), base_style)];

        if !query.is_empty() {
            // Apply fuzzy match highlighting
            spans.extend(fuzzy_match_highlight(item, query, *selected));
        } else {
            spans.push(Span::styled(item.to_string(), base_style));
        }

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

/// Highlight fuzzy match characters in text.
///
/// Returns a vector of spans with matched characters highlighted.
/// Case-insensitive matching is used.
fn fuzzy_match_highlight(text: &str, query: &str, is_selected: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    let mut qi = 0;

    let normal_style = if is_selected {
        Style::default()
            .bg(Color::Rgb(52, 152, 219))
            .fg(Color::White)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 200))
    };

    let highlight_style = if is_selected {
        Style::default()
            .bg(Color::Rgb(52, 152, 219))
            .fg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::BOLD)
    };

    for c in text.chars() {
        if qi < query_chars.len() && c.to_lowercase().next() == Some(query_chars[qi]) {
            // Highlight matched character
            spans.push(Span::styled(c.to_string(), highlight_style));
            qi += 1;
        } else {
            // Normal character
            spans.push(Span::styled(c.to_string(), normal_style));
        }
    }

    spans
}
