//! Inline renderer — prints ratatui `Line`/`Text` directly to stdout
//! without entering alternate screen or raw mode.
//!
//! This allows the CLI (non-TUI) mode to reuse the same styled widgets
//! that the full-screen TUI uses.

use crossterm::cursor::MoveToColumn;
use crossterm::style::{
    Attribute, Color as CtColor, Print, ResetColor, SetAttribute, SetBackgroundColor,
    SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use crossterm::Command;
use crossterm::ExecutableCommand;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::io::{self, Write};

use super::capabilities::{is_stdout_tty, should_use_color};

/// Print a single `Line` to stdout. Styling is dropped automatically when
/// the terminal does not support color (NO_COLOR, TERM=dumb, piped output,
/// or `--color=never`).
pub fn print_line(line: &Line<'_>) {
    let mut stdout = io::stdout();
    let styled = should_use_color();
    for span in &line.spans {
        if styled {
            apply_style(&mut stdout, &span.style);
        }
        let _ = stdout.execute(Print(&span.content));
        if styled {
            let _ = stdout.execute(ResetColor);
            let _ = stdout.execute(SetAttribute(Attribute::Reset));
        }
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

/// Print a `Line` as a transient status line that overwrites itself on each
/// update. Uses `\r` + clear-line so the cursor stays parked at the start of
/// the line, ready to be overwritten by the next call or finalized by
/// [`clear_transient`] / [`print_line`].
///
/// When stdout is not a TTY (piped output, dumb terminal), this is a no-op so
/// progress noise doesn't pollute logs. Callers should still emit a final
/// `print_line` for the completed state in that case.
pub fn print_transient(line: &Line<'_>) {
    if !is_stdout_tty() {
        return;
    }
    let mut stdout = io::stdout();
    let _ = stdout.execute(MoveToColumn(0));
    let _ = stdout.execute(Clear(ClearType::CurrentLine));
    let styled = should_use_color();
    for span in &line.spans {
        if styled {
            apply_style(&mut stdout, &span.style);
        }
        let _ = stdout.execute(Print(&span.content));
        if styled {
            let _ = stdout.execute(ResetColor);
            let _ = stdout.execute(SetAttribute(Attribute::Reset));
        }
    }
    let _ = stdout.flush();
}

/// Clear the current transient status line. Pair with [`print_transient`]
/// before printing a finalized message.
pub fn clear_transient() {
    if !is_stdout_tty() {
        return;
    }
    let mut stdout = io::stdout();
    let _ = stdout.execute(MoveToColumn(0));
    let _ = stdout.execute(Clear(ClearType::CurrentLine));
    let _ = stdout.flush();
}

/// Get terminal width, defaulting to 80.
pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(40)
}

/// Render a `Line` to an ANSI-encoded `String`. Useful when a third-party
/// library (dialoguer's `Select`/`FuzzySelect`, indicatif templates, etc.)
/// expects a raw styled string but we still want widget-driven styling.
///
/// Honors [`should_use_color`] — when color is disabled, returns plain
/// text with all styling stripped.
pub fn line_to_ansi(line: &Line<'_>) -> String {
    let styled = should_use_color();
    // Most lines are short; pre-allocate a reasonable lower bound.
    let mut out = String::with_capacity(
        line.spans.iter().map(|s| s.content.len()).sum::<usize>() + 16,
    );
    for span in &line.spans {
        if styled {
            write_style_ansi(&mut out, &span.style);
        }
        out.push_str(&span.content);
        if styled {
            // Reset after each span so adjacent spans don't bleed styling.
            let _ = SetAttribute(Attribute::Reset).write_ansi(&mut out);
            let _ = ResetColor.write_ansi(&mut out);
        }
    }
    out
}

/// Write SGR escapes for `style` into `out`. Used by [`line_to_ansi`].
fn write_style_ansi(out: &mut String, style: &Style) {
    if let Some(fg) = style.fg {
        if let Some(ct) = to_crossterm_color(fg) {
            let _ = SetForegroundColor(ct).write_ansi(out);
        }
    }
    if let Some(bg) = style.bg {
        if let Some(ct) = to_crossterm_color(bg) {
            let _ = SetBackgroundColor(ct).write_ansi(out);
        }
    }
    let mods = style.add_modifier;
    if mods.contains(Modifier::BOLD) {
        let _ = SetAttribute(Attribute::Bold).write_ansi(out);
    }
    if mods.contains(Modifier::DIM) {
        let _ = SetAttribute(Attribute::Dim).write_ansi(out);
    }
    if mods.contains(Modifier::ITALIC) {
        let _ = SetAttribute(Attribute::Italic).write_ansi(out);
    }
    if mods.contains(Modifier::UNDERLINED) {
        let _ = SetAttribute(Attribute::Underlined).write_ansi(out);
    }
    if mods.contains(Modifier::CROSSED_OUT) {
        let _ = SetAttribute(Attribute::CrossedOut).write_ansi(out);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_crossterm_color_maps_known_variants() {
        assert_eq!(to_crossterm_color(Color::Reset), None);
        assert!(matches!(to_crossterm_color(Color::Red), Some(CtColor::DarkRed)));
        assert!(matches!(
            to_crossterm_color(Color::Rgb(10, 20, 30)),
            Some(CtColor::Rgb { r: 10, g: 20, b: 30 })
        ));
        assert!(matches!(
            to_crossterm_color(Color::Indexed(42)),
            Some(CtColor::AnsiValue(42))
        ));
    }

    #[test]
    fn terminal_width_has_minimum_floor() {
        // The width helper enforces a 40-col floor so layout math stays sane
        // even on tiny or unmeasurable terminals.
        assert!(terminal_width() >= 40);
    }

    #[test]
    fn line_to_ansi_emits_styling_when_color_on() {
        use super::super::capabilities::set_color_enabled;
        set_color_enabled(Some(true));
        let line = Line::from(vec![
            Span::styled("red", Style::default().fg(Color::Red)),
            Span::raw(" plain"),
            Span::styled("bold", Style::default().add_modifier(Modifier::BOLD)),
        ]);
        let ansi = line_to_ansi(&line);
        assert!(ansi.contains("red"));
        assert!(ansi.contains(" plain"));
        assert!(ansi.contains("bold"));
        // Should contain at least one SGR escape ([).
        assert!(ansi.contains('\u{1b}'));
        set_color_enabled(None);
    }

    #[test]
    fn line_to_ansi_strips_styling_when_color_off() {
        use super::super::capabilities::set_color_enabled;
        set_color_enabled(Some(false));
        let line = Line::from(vec![
            Span::styled("red", Style::default().fg(Color::Red)),
            Span::raw(" plain"),
        ]);
        let ansi = line_to_ansi(&line);
        assert_eq!(ansi, "red plain");
        assert!(!ansi.contains('\u{1b}'));
        set_color_enabled(None);
    }
}
