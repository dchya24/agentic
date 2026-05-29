//! Inline renderer — prints ratatui `Line`/`Text` directly to stdout
//! without entering alternate screen or raw mode.
//!
//! This allows the CLI (non-TUI) mode to reuse the same styled widgets
//! that the full-screen TUI uses.

use crossterm::style::{
    Attribute, Color as CtColor, Print, ResetColor, SetAttribute, SetBackgroundColor,
    SetForegroundColor,
};
use crossterm::ExecutableCommand;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::io::{self, Write};

/// Print a single `Line` to stdout with ANSI styling, followed by a newline.
pub fn print_line(line: &Line<'_>) {
    let mut stdout = io::stdout();
    for span in &line.spans {
        apply_style(&mut stdout, &span.style);
        let _ = stdout.execute(Print(&span.content));
        let _ = stdout.execute(ResetColor);
        let _ = stdout.execute(SetAttribute(Attribute::Reset));
    }
    let _ = writeln!(stdout);
}

/// Print multiple `Line`s to stdout.
pub fn print_lines(lines: &[Line<'_>]) {
    for line in lines {
        print_line(line);
    }
}

/// Print a `Text` block to stdout.
pub fn print_text(text: &Text<'_>) {
    print_lines(&text.lines);
}

/// Print a horizontal rule using a repeated character.
pub fn print_rule(ch: char, style: Style) {
    let width = terminal_width();
    let rule_str: String = std::iter::repeat(ch).take(width).collect();
    let line = Line::from(Span::styled(rule_str, style));
    print_line(&line);
}

/// Print an empty line.
pub fn print_blank() {
    println!();
}

/// Get terminal width, defaulting to 80.
pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(40)
}

/// Apply a ratatui `Style` to stdout using crossterm commands.
fn apply_style(stdout: &mut io::Stdout, style: &Style) {
    if let Some(fg) = style.fg {
        if let Some(ct_color) = to_crossterm_color(fg) {
            let _ = stdout.execute(SetForegroundColor(ct_color));
        }
    }
    if let Some(bg) = style.bg {
        if let Some(ct_color) = to_crossterm_color(bg) {
            let _ = stdout.execute(SetBackgroundColor(ct_color));
        }
    }

    let mods = style.add_modifier;
    if mods.contains(Modifier::BOLD) {
        let _ = stdout.execute(SetAttribute(Attribute::Bold));
    }
    if mods.contains(Modifier::ITALIC) {
        let _ = stdout.execute(SetAttribute(Attribute::Italic));
    }
    if mods.contains(Modifier::UNDERLINED) {
        let _ = stdout.execute(SetAttribute(Attribute::Underlined));
    }
    if mods.contains(Modifier::DIM) {
        let _ = stdout.execute(SetAttribute(Attribute::Dim));
    }
    if mods.contains(Modifier::CROSSED_OUT) {
        let _ = stdout.execute(SetAttribute(Attribute::CrossedOut));
    }
}

/// Convert ratatui `Color` to crossterm `Color`.
fn to_crossterm_color(color: Color) -> Option<CtColor> {
    match color {
        Color::Reset => None,
        Color::Black => Some(CtColor::Black),
        Color::Red => Some(CtColor::DarkRed),
        Color::Green => Some(CtColor::DarkGreen),
        Color::Yellow => Some(CtColor::DarkYellow),
        Color::Blue => Some(CtColor::DarkBlue),
        Color::Magenta => Some(CtColor::DarkMagenta),
        Color::Cyan => Some(CtColor::DarkCyan),
        Color::Gray => Some(CtColor::Grey),
        Color::DarkGray => Some(CtColor::DarkGrey),
        Color::LightRed => Some(CtColor::Red),
        Color::LightGreen => Some(CtColor::Green),
        Color::LightYellow => Some(CtColor::Yellow),
        Color::LightBlue => Some(CtColor::Blue),
        Color::LightMagenta => Some(CtColor::Magenta),
        Color::LightCyan => Some(CtColor::Cyan),
        Color::White => Some(CtColor::White),
        Color::Rgb(r, g, b) => Some(CtColor::Rgb { r, g, b }),
        Color::Indexed(i) => Some(CtColor::AnsiValue(i)),
    }
}
