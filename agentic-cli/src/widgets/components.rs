//! Rich inline UI components for CLI mode.
//!
//! Provides higher-level visual components built on ratatui primitives:
//! - Bordered panels/boxes
//! - Status badges
//! - Key-value displays
//! - Gradient text
//! - Section headers
//! - Notification banners

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::inline::terminal_width;

// ── Box drawing characters ──────────────────────────────────

/// Box style for panels
#[derive(Clone, Copy)]
pub enum BoxStyle {
    /// Single line: ┌─┐│└─┘
    Single,
    /// Double line: ╔═╗║╚═╝
    Double,
    /// Rounded: ╭─╮│╰─╯
    Rounded,
    /// Heavy: ┏━┓┃┗━┛
    Heavy,
}

impl BoxStyle {
    fn chars(&self) -> (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str) {
        match self {
            // (tl, tr, bl, br, h, v, cross)
            BoxStyle::Single => ("┌", "┐", "└", "┘", "─", "│", "┼"),
            BoxStyle::Double => ("╔", "╗", "╚", "╝", "═", "║", "╬"),
            BoxStyle::Rounded => ("╭", "╮", "╰", "╯", "─", "│", "┼"),
            BoxStyle::Heavy => ("┏", "┓", "┗", "┛", "━", "┃", "╋"),
        }
    }
}

// ── Panel / Box ─────────────────────────────────────────────

/// Render content inside a bordered box.
///
/// ```text
/// ╭─ Title ──────────────────────╮
/// │  content line 1              │
/// │  content line 2              │
/// ╰──────────────────────────────╯
/// ```
pub fn panel(title: &str, content: &[Line<'static>], style: BoxStyle, border_color: Color) -> Vec<Line<'static>> {
    let width = terminal_width().min(100);
    let (tl, tr, bl, br, h, v, _) = style.chars();
    let border_style = Style::default().fg(border_color);
    let inner_width = width.saturating_sub(4); // 2 border + 2 padding

    let mut lines = Vec::new();

    // Top border with title
    let title_display = if title.is_empty() {
        String::new()
    } else {
        format!(" {} ", title)
    };
    let title_len = title_display.chars().count();
    let remaining = inner_width.saturating_sub(title_len + 1);

    lines.push(Line::from(vec![
        Span::styled(format!("{}{}", tl, h), border_style),
        Span::styled(
            title_display,
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(h.repeat(remaining), border_style),
        Span::styled(tr.to_string(), border_style),
    ]));

    // Content lines
    for line in content {
        let content_str: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        let content_len = content_str.chars().count();
        let padding = inner_width.saturating_sub(content_len);

        let mut spans = vec![
            Span::styled(format!("{} ", v), border_style),
        ];
        spans.extend(line.spans.iter().cloned());
        spans.push(Span::raw(" ".repeat(padding)));
        spans.push(Span::styled(format!(" {}", v), border_style));

        lines.push(Line::from(spans));
    }

    // Bottom border
    lines.push(Line::from(vec![
        Span::styled(format!("{}{}{}", bl, h.repeat(inner_width + 2), br), border_style),
    ]));

    lines
}

/// Render a compact panel with no title.
pub fn box_content(content: &[Line<'static>], style: BoxStyle, border_color: Color) -> Vec<Line<'static>> {
    panel("", content, style, border_color)
}

// ── Section Header ──────────────────────────────────────────

/// Render a section header with decorative line.
///
/// ```text
/// ── 📊 Statistics ──────────────────────
/// ```
pub fn section_header(icon: &str, title: &str, color: Color) -> Line<'static> {
    let width = terminal_width().min(100);
    let prefix = format!("── {} {} ", icon, title);
    let prefix_len = prefix.chars().count();
    let remaining = width.saturating_sub(prefix_len);

    Line::from(vec![
        Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(remaining),
            Style::default().fg(color).add_modifier(Modifier::DIM),
        ),
    ])
}

/// Render a sub-section header (lighter weight).
pub fn subsection_header(title: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {} ", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

// ── Key-Value Display ───────────────────────────────────────

/// Render a key-value pair with aligned formatting.
///
/// ```text
///   Provider:     openai
///   Model:        gpt-4
/// ```
pub fn kv_line(key: &str, value: &str, key_width: usize, value_color: Color) -> Line<'static> {
    let padded_key = format!("  {:width$}", format!("{}:", key), width = key_width + 1);
    Line::from(vec![
        Span::styled(padded_key, Style::default().add_modifier(Modifier::DIM)),
        Span::styled(value.to_string(), Style::default().fg(value_color)),
    ])
}

/// Render a key-value pair with a badge-style value.
pub fn kv_badge(key: &str, value: &str, key_width: usize, badge_fg: Color, badge_bg: Color) -> Line<'static> {
    let padded_key = format!("  {:width$}", format!("{}:", key), width = key_width + 1);
    Line::from(vec![
        Span::styled(padded_key, Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            format!(" {} ", value),
            Style::default().fg(badge_fg).bg(badge_bg),
        ),
    ])
}

// ── Status Badges ───────────────────────────────────────────

/// Success badge: ` ✓ message `
pub fn success_badge(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(" ✓ {} ", message),
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(Color::Rgb(39, 174, 96)),
        ),
    ])
}

/// Error badge: ` ✗ message `
pub fn error_badge(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(" ✗ {} ", message),
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(Color::Rgb(192, 57, 43)),
        ),
    ])
}

/// Warning badge: ` ⚠ message `
pub fn warning_badge(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(" ⚠ {} ", message),
            Style::default()
                .fg(Color::Rgb(0, 0, 0))
                .bg(Color::Rgb(241, 196, 15)),
        ),
    ])
}

/// Info badge: ` ℹ message `
pub fn info_badge(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(" ℹ {} ", message),
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(Color::Rgb(52, 152, 219)),
        ),
    ])
}

// ── Gradient / Decorative Text ──────────────────────────────

/// Render text with a horizontal gradient between two colors.
pub fn gradient_text(text: &str, from: Color, to: Color) -> Line<'static> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len().max(1);

    let (r1, g1, b1) = color_to_rgb(from);
    let (r2, g2, b2) = color_to_rgb(to);

    let spans: Vec<Span<'static>> = chars
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let t = i as f32 / (len - 1).max(1) as f32;
            let r = lerp(r1, r2, t);
            let g = lerp(g1, g2, t);
            let b = lerp(b1, b2, t);
            Span::styled(
                c.to_string(),
                Style::default().fg(Color::Rgb(r, g, b)),
            )
        })
        .collect();

    Line::from(spans)
}

/// Render a decorative banner title with gradient.
pub fn banner_title(text: &str, from: Color, to: Color) -> Line<'static> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len().max(1);

    let (r1, g1, b1) = color_to_rgb(from);
    let (r2, g2, b2) = color_to_rgb(to);

    let spans: Vec<Span<'static>> = chars
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let t = i as f32 / (len - 1).max(1) as f32;
            let r = lerp(r1, r2, t);
            let g = lerp(g1, g2, t);
            let b = lerp(b1, b2, t);
            Span::styled(
                c.to_string(),
                Style::default()
                    .fg(Color::Rgb(r, g, b))
                    .add_modifier(Modifier::BOLD),
            )
        })
        .collect();

    Line::from(spans)
}

// ── Separator Variants ──────────────────────────────────────

/// Dotted separator
pub fn dotted_separator(color: Color) -> Line<'static> {
    let width = terminal_width().min(100);
    let dots: String = "· ".repeat(width / 2);
    Line::from(Span::styled(dots, Style::default().fg(color)))
}

/// Dashed separator
pub fn dashed_separator(color: Color) -> Line<'static> {
    let width = terminal_width().min(100);
    let dashes: String = "╌".repeat(width);
    Line::from(Span::styled(dashes, Style::default().fg(color)))
}

/// Double-line separator
pub fn double_separator(color: Color) -> Line<'static> {
    let width = terminal_width().min(100);
    let line: String = "═".repeat(width);
    Line::from(Span::styled(line, Style::default().fg(color)))
}

// ── Progress / Bar Charts ───────────────────────────────────

/// Render a labeled progress bar.
///
/// ```text
///   Input:  ████████████░░░░░░░░  60%
/// ```
pub fn labeled_bar(label: &str, value: f32, width: usize, filled_color: Color, empty_color: Color) -> Line<'static> {
    let filled = (width as f32 * value.clamp(0.0, 1.0)) as usize;
    let empty = width.saturating_sub(filled);
    let pct = (value * 100.0) as u8;

    Line::from(vec![
        Span::styled(
            format!("  {:12}", format!("{}:", label)),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::styled("█".repeat(filled), Style::default().fg(filled_color)),
        Span::styled("░".repeat(empty), Style::default().fg(empty_color)),
        Span::styled(
            format!("  {}%", pct),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ])
}

/// Render a mini sparkline from values.
pub fn sparkline(values: &[f32], color: Color) -> Line<'static> {
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let range = (max - min).max(0.001);

    let chars: String = values
        .iter()
        .map(|v| {
            let normalized = ((v - min) / range * 7.0) as usize;
            blocks[normalized.min(7)]
        })
        .collect();

    Line::from(Span::styled(chars, Style::default().fg(color)))
}

// ── Table ───────────────────────────────────────────────────

/// Render a simple table with headers and rows.
pub fn table(headers: &[&str], rows: &[Vec<String>], header_color: Color, border_color: Color) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let col_widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let max_row = rows
                .iter()
                .map(|r| r.get(i).map(|c| c.chars().count()).unwrap_or(0))
                .max()
                .unwrap_or(0);
            h.chars().count().max(max_row) + 2
        })
        .collect();

    let border_style = Style::default().fg(border_color);
    let header_style = Style::default().fg(header_color).add_modifier(Modifier::BOLD);

    // Header
    let mut header_spans = vec![Span::styled("  ", border_style)];
    for (i, h) in headers.iter().enumerate() {
        header_spans.push(Span::styled(
            format!("{:width$}", h, width = col_widths[i]),
            header_style,
        ));
    }
    lines.push(Line::from(header_spans));

    // Separator
    let sep: String = col_widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─");
    lines.push(Line::from(Span::styled(format!("  {}", sep), border_style)));

    // Rows
    for row in rows {
        let mut row_spans = vec![Span::styled("  ", border_style)];
        for (i, cell) in row.iter().enumerate() {
            let width = col_widths.get(i).copied().unwrap_or(10);
            row_spans.push(Span::raw(format!("{:width$}", cell, width = width)));
        }
        lines.push(Line::from(row_spans));
    }

    lines
}

// ── Notification Banner ─────────────────────────────────────

/// Full-width notification banner.
///
/// ```text
/// ┃ ✓ Operation completed successfully
/// ```
pub fn notification(icon: &str, message: &str, accent_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("  ┃ ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} ", icon), Style::default().fg(accent_color)),
        Span::raw(message.to_string()),
    ])
}

// ── Helpers ─────────────────────────────────────────────────

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Red => (231, 76, 60),
        Color::Green => (46, 204, 113),
        Color::Yellow => (241, 196, 15),
        Color::Blue => (52, 152, 219),
        Color::Magenta => (155, 89, 182),
        Color::Cyan => (26, 188, 156),
        Color::White => (255, 255, 255),
        Color::DarkGray => (100, 100, 100),
        _ => (200, 200, 200),
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_basic() {
        let content = vec![
            Line::from("Hello world"),
            Line::from("Second line"),
        ];
        let result = panel("Test", &content, BoxStyle::Rounded, Color::Cyan);
        assert_eq!(result.len(), 4); // top + 2 content + bottom
    }

    #[test]
    fn test_section_header() {
        let line = section_header("📊", "Statistics", Color::Cyan);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Statistics"));
    }

    #[test]
    fn test_kv_line() {
        let line = kv_line("Provider", "openai", 12, Color::Yellow);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Provider"));
        assert!(text.contains("openai"));
    }

    #[test]
    fn test_success_badge() {
        let line = success_badge("Done");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("✓"));
        assert!(text.contains("Done"));
    }

    #[test]
    fn test_gradient_text() {
        let line = gradient_text("Hello", Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 255));
        assert_eq!(line.spans.len(), 5); // one per char
    }

    #[test]
    fn test_labeled_bar() {
        let line = labeled_bar("CPU", 0.75, 20, Color::Green, Color::DarkGray);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("CPU"));
        assert!(text.contains("75%"));
    }

    #[test]
    fn test_sparkline() {
        let line = sparkline(&[0.1, 0.5, 0.8, 0.3, 1.0], Color::Cyan);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_table() {
        let headers = &["Name", "Value"];
        let rows = vec![
            vec!["foo".to_string(), "bar".to_string()],
            vec!["baz".to_string(), "qux".to_string()],
        ];
        let result = table(headers, &rows, Color::Cyan, Color::DarkGray);
        assert_eq!(result.len(), 4); // header + sep + 2 rows
    }

    #[test]
    fn test_notification() {
        let line = notification("✓", "All good", Color::Green);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("All good"));
    }
}
