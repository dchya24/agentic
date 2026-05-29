//! Shared markdown renderer — produces ratatui `Line`s from markdown text.
//!
//! Used by both:
//! - TUI mode (rendered into ratatui widgets)
//! - CLI mode (rendered inline via `widgets::inline`)

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Parsed markdown content ready for rendering
#[derive(Clone, Debug)]
pub struct MarkdownContent {
    pub lines: Vec<Line<'static>>,
}

impl MarkdownContent {
    /// Parse markdown string into styled lines
    pub fn parse(markdown: &str) -> Self {
        let mut renderer = MarkdownRenderer::new();
        renderer.render(markdown);
        Self {
            lines: renderer.lines,
        }
    }
}

/// Internal markdown renderer
struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    code_lang: String,
    list_depth: usize,
    in_table: bool,
    table_row: Vec<String>,
}

impl MarkdownRenderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_line: Vec::new(),
            style_stack: vec![Style::default()],
            in_code_block: false,
            code_lang: String::new(),
            list_depth: 0,
            in_table: false,
            table_row: Vec::new(),
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, style: Style) {
        let combined = self.current_style().patch(style);
        self.style_stack.push(combined);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn push_span(&mut self, text: &str) {
        if !text.is_empty() {
            self.current_line
                .push(Span::styled(text.to_string(), self.current_style()));
        }
    }

    fn finish_line(&mut self) {
        if !self.current_line.is_empty() {
            self.lines
                .push(Line::from(std::mem::take(&mut self.current_line)));
        } else {
            self.lines.push(Line::default());
        }
    }

    fn render(&mut self, markdown: &str) {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let parser = Parser::new_ext(markdown, options);

        for event in parser {
            self.handle_event(event);
        }

        // Finish any remaining content
        if !self.current_line.is_empty() {
            self.finish_line();
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.handle_start_tag(tag),
            Event::End(tag_end) => self.handle_end_tag(tag_end),
            Event::Text(text) => self.handle_text(&text),
            Event::Code(code) => self.handle_inline_code(&code),
            Event::SoftBreak => self.push_span(" "),
            Event::HardBreak => self.finish_line(),
            Event::Rule => {
                self.finish_line();
                self.current_line.push(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                ));
                self.finish_line();
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑ " } else { "☐ " };
                self.push_span(marker);
            }
            _ => {}
        }
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.finish_line();
                let (color, prefix) = match level {
                    pulldown_cmark::HeadingLevel::H1 => (Color::Rgb(255, 165, 0), "# "),
                    pulldown_cmark::HeadingLevel::H2 => (Color::Rgb(135, 206, 235), "## "),
                    pulldown_cmark::HeadingLevel::H3 => (Color::Rgb(144, 238, 144), "### "),
                    pulldown_cmark::HeadingLevel::H4 => (Color::Rgb(221, 160, 221), "#### "),
                    _ => (Color::White, "##### "),
                };
                self.push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
                self.push_span(prefix);
            }
            Tag::Paragraph => {
                if !self.lines.is_empty() && !self.in_code_block {
                    // Add blank line before paragraph (unless first)
                }
            }
            Tag::Strong => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Emphasis => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::CodeBlock(kind) => {
                self.finish_line();
                self.in_code_block = true;
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };

                // Code block header
                let lang_display = if self.code_lang.is_empty() {
                    "code".to_string()
                } else {
                    self.code_lang.clone()
                };
                self.current_line.push(Span::styled(
                    format!("┌─ {} ", lang_display),
                    Style::default().fg(Color::Rgb(46, 204, 113)),
                ));
                self.current_line.push(Span::styled(
                    "─".repeat(30),
                    Style::default().fg(Color::Rgb(46, 204, 113)),
                ));
                self.finish_line();

                self.push_style(Style::default().fg(Color::Rgb(200, 200, 200)));
            }
            Tag::Link { dest_url, .. } => {
                self.push_style(
                    Style::default()
                        .fg(Color::Rgb(52, 152, 219))
                        .add_modifier(Modifier::UNDERLINED),
                );
                let _ = dest_url;
            }
            Tag::BlockQuote => {
                self.finish_line();
                self.push_style(Style::default().fg(Color::Rgb(149, 165, 166)));
                self.push_span("│ ");
            }
            Tag::List(_) => {
                self.list_depth += 1;
            }
            Tag::Item => {
                self.finish_line();
                let indent = "  ".repeat(self.list_depth.saturating_sub(1));
                self.current_line.push(Span::styled(
                    format!("{}• ", indent),
                    Style::default().fg(Color::Rgb(155, 89, 182)),
                ));
            }
            Tag::Table(_) => {
                self.finish_line();
                self.in_table = true;
            }
            Tag::TableHead => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::TableRow => {
                self.table_row.clear();
            }
            Tag::TableCell => {}
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Heading(_) => {
                self.pop_style();
                self.finish_line();
            }
            TagEnd::Paragraph => {
                self.finish_line();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                self.pop_style();
                self.in_code_block = false;
                self.current_line.push(Span::styled(
                    format!("└{}", "─".repeat(35)),
                    Style::default().fg(Color::Rgb(46, 204, 113)),
                ));
                self.finish_line();
            }
            TagEnd::Link => {
                self.pop_style();
            }
            TagEnd::BlockQuote => {
                self.pop_style();
                self.finish_line();
            }
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
            }
            TagEnd::Item => {
                self.finish_line();
            }
            TagEnd::Table => {
                self.in_table = false;
                self.finish_line();
            }
            TagEnd::TableHead => {
                self.pop_style();
                self.finish_line();
                // Add separator line
                self.current_line.push(Span::styled(
                    "├".to_string() + &"─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                ));
                self.finish_line();
            }
            TagEnd::TableRow => {
                self.finish_line();
            }
            TagEnd::TableCell => {
                self.push_span(" │ ");
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str) {
        if self.in_code_block {
            // Handle code block text line by line
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    self.finish_line();
                }
                if !line.is_empty() {
                    self.current_line.push(Span::styled(
                        format!("│ {}", line),
                        Style::default().fg(Color::Rgb(200, 200, 200)),
                    ));
                } else if i > 0 {
                    self.current_line.push(Span::styled(
                        "│".to_string(),
                        Style::default().fg(Color::Rgb(46, 204, 113)),
                    ));
                }
            }
        } else {
            self.push_span(text);
        }
    }

    fn handle_inline_code(&mut self, code: &str) {
        self.current_line.push(Span::styled(
            format!("`{}`", code),
            Style::default()
                .fg(Color::Rgb(241, 196, 15))
                .bg(Color::Rgb(40, 40, 40)),
        ));
    }
}

/// Convert message role to styled prefix
pub fn role_prefix(role: &str) -> (Span<'static>, Style) {
    match role {
        "user" => (
            Span::styled(
                "👤 You".to_string(),
                Style::default()
                    .fg(Color::Rgb(52, 152, 219))
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default(),
        ),
        "assistant" => (
            Span::styled(
                "🤖 Assistant".to_string(),
                Style::default()
                    .fg(Color::Rgb(46, 204, 113))
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default(),
        ),
        "system" => (
            Span::styled(
                "ℹ System".to_string(),
                Style::default()
                    .fg(Color::Rgb(155, 89, 182))
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default().fg(Color::Rgb(180, 180, 180)),
        ),
        "error" => (
            Span::styled(
                "✗ Error".to_string(),
                Style::default()
                    .fg(Color::Rgb(231, 76, 60))
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default().fg(Color::Rgb(231, 76, 60)),
        ),
        _ => (
            Span::styled(
                format!("💬 {}", role),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Style::default(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let content = MarkdownContent::parse("Hello **world**!");
        assert!(!content.lines.is_empty());
    }

    #[test]
    fn test_parse_heading() {
        let content = MarkdownContent::parse("# Title\n\nParagraph");
        assert!(content.lines.len() >= 2);
    }

    #[test]
    fn test_parse_code_block() {
        let content = MarkdownContent::parse("```rust\nfn main() {}\n```");
        assert!(content.lines.len() >= 3);
    }

    #[test]
    fn test_parse_list() {
        let content = MarkdownContent::parse("- Item 1\n- Item 2\n- Item 3");
        assert!(content.lines.len() >= 3);
    }

    #[test]
    fn test_parse_inline_code() {
        let content = MarkdownContent::parse("Use `cargo build` to compile");
        assert!(!content.lines.is_empty());
        // Should contain the backtick-wrapped code
        let all_text: String = content.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("`cargo build`"));
    }

    #[test]
    fn test_parse_nested_list() {
        let content = MarkdownContent::parse("- Item 1\n  - Nested\n- Item 2");
        assert!(content.lines.len() >= 3);
    }

    #[test]
    fn test_parse_blockquote() {
        let content = MarkdownContent::parse("> This is a quote");
        assert!(!content.lines.is_empty());
        let all_text: String = content.lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("│"));
    }
}
