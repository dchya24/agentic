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
pub fn render_prompt_line(meta: &PromptMetadata, buffer: &InputBuffer) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let _cyan = Style::default().fg(Color::Cyan);

    // Build prompt prefix: "dirname> "
    let prompt_prefix = format!("{}> ", meta.dir_name);

    // Build input content (highlighted or placeholder)
    let input_line = if buffer.is_empty() {
        render_placeholder()
    } else {
        render_input(buffer.text(), buffer.cursor())
    };

    // Combine: prompt prefix + input content
    let mut spans: Vec<Span<'static>> = vec![Span::styled(prompt_prefix, dim)];
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
            DropdownType::File => {
                if item.ends_with('/') {
                    "📁 "
                } else {
                    "📄 "
                }
            }
            _ => "  ",
        };

        let mut spans = vec![Span::styled(format!("  {}{}", item_icon, item), style)];

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
