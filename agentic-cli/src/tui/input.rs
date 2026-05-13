//! Input handling utilities

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render input with syntax highlighting for special characters
pub fn render_input(input: &str, cursor_pos: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let _char_index = 0;

    for (byte_pos, c) in input.char_indices() {
        let new_style = get_char_style(c, input, byte_pos);
        
        // Check if we need to insert cursor
        if byte_pos == cursor_pos {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            // Cursor character
            spans.push(Span::styled(
                c.to_string(),
                Style::default()
                    .bg(Color::White)
                    .fg(Color::Black),
            ));
            continue;
        }

        if new_style != current_style {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            current_style = new_style;
        }
        
        current_text.push(c);
    }

    // Push remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    // If cursor is at end, add cursor block
    if cursor_pos >= input.len() {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::White),
        ));
    }

    Line::from(spans)
}

/// Get style for a character based on context
fn get_char_style(c: char, input: &str, pos: usize) -> Style {
    // Slash command highlighting
    if input.starts_with('/') {
        if pos == 0 {
            return Style::default()
                .fg(Color::Rgb(241, 196, 15))
                .add_modifier(Modifier::BOLD);
        }
        // Command name (until space)
        if !input[..pos].contains(' ') {
            return Style::default()
                .fg(Color::Rgb(241, 196, 15));
        }
    }

    // @ file reference highlighting
    if c == '@' {
        return Style::default()
            .fg(Color::Rgb(52, 152, 219))
            .add_modifier(Modifier::BOLD);
    }

    // Check if we're in an @reference
    let before = &input[..pos];
    if let Some(at_pos) = before.rfind('@') {
        // Check if @ is at start or after whitespace
        if at_pos == 0 || before[..at_pos].ends_with(char::is_whitespace) {
            // Check if no space between @ and current position
            if !before[at_pos..].contains(' ') {
                return Style::default()
                    .fg(Color::Rgb(52, 152, 219));
            }
        }
    }

    Style::default().fg(Color::White)
}

/// Render placeholder text when input is empty
pub fn render_placeholder() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "Type a message, ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "/",
            Style::default()
                .fg(Color::Rgb(241, 196, 15))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " for commands, ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "@",
            Style::default()
                .fg(Color::Rgb(52, 152, 219))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " for files",
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty_input() {
        let line = render_input("", 0);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_render_slash_command() {
        let line = render_input("/help", 5);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_render_at_reference() {
        let line = render_input("read @src/main.rs", 17);
        assert!(!line.spans.is_empty());
    }
}
