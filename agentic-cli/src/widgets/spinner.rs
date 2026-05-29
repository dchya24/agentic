//! Shared spinner widget — produces ratatui `Line`s for inline or TUI use.
//!
//! Unlike `ProgressState` (which tracks full progress with elapsed time),
//! this module provides simple styled spinner lines for quick rendering.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::progress::ProgressState;

/// Render a spinner line with message and elapsed time.
///
/// Returns a styled `Line` like: `⠹ Thinking... (3s)`
pub fn spinner_line(progress: &ProgressState) -> Line<'static> {
    if !progress.active {
        return Line::default();
    }

    let mut spans = vec![
        Span::styled(
            progress.spinner().to_string(),
            Style::default()
                .fg(Color::Rgb(52, 152, 219))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            if progress.message.is_empty() {
                "Processing...".to_string()
            } else {
                progress.message.clone()
            },
            Style::default().fg(Color::Rgb(180, 180, 180)),
        ),
    ];

    let elapsed = progress.elapsed_str();
    if !elapsed.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("({})", elapsed),
            Style::default().fg(Color::DarkGray),
        ));
    }

    Line::from(spans)
}

/// Render a progress bar line (determinate or indeterminate).
///
/// Returns a styled `Line` with the progress bar characters.
pub fn progress_bar_line(progress: &ProgressState, width: usize) -> Line<'static> {
    let bar = progress.progress_bar(width);

    let style = if progress.percentage.is_some() {
        Style::default().fg(Color::Rgb(46, 204, 113))
    } else {
        Style::default().fg(Color::Rgb(52, 152, 219))
    };

    Line::from(Span::styled(bar, style))
}

/// Render a compact status line: spinner + message + bar.
///
/// Useful for CLI inline rendering where you want a single-line progress.
pub fn compact_progress_line(progress: &ProgressState, bar_width: usize) -> Line<'static> {
    if !progress.active {
        return Line::default();
    }

    let bar = progress.progress_bar(bar_width);

    Line::from(vec![
        Span::styled(
            progress.spinner().to_string(),
            Style::default()
                .fg(Color::Rgb(52, 152, 219))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            if progress.message.is_empty() {
                "Processing...".to_string()
            } else {
                progress.message.clone()
            },
            Style::default().fg(Color::Rgb(180, 180, 180)),
        ),
        Span::raw(" "),
        Span::styled(bar, Style::default().fg(Color::Rgb(52, 152, 219))),
        Span::raw(" "),
        Span::styled(
            progress.elapsed_str(),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// Render a "done" line after progress completes.
///
/// Returns: `✓ Done (2.3s)`
pub fn done_line(elapsed_ms: u128) -> Line<'static> {
    let elapsed_str = if elapsed_ms < 1000 {
        format!("{}ms", elapsed_ms)
    } else {
        format!("{}.{}s", elapsed_ms / 1000, (elapsed_ms % 1000) / 100)
    };

    Line::from(vec![
        Span::styled(
            "✓".to_string(),
            Style::default()
                .fg(Color::Rgb(46, 204, 113))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "Done".to_string(),
            Style::default().fg(Color::Rgb(46, 204, 113)),
        ),
        Span::raw(" "),
        Span::styled(
            format!("({})", elapsed_str),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// Render an error line.
///
/// Returns: `✗ Error: <message>`
pub fn error_line(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "✗".to_string(),
            Style::default()
                .fg(Color::Rgb(231, 76, 60))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("Error: {}", message),
            Style::default().fg(Color::Rgb(231, 76, 60)),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_line_active() {
        let mut progress = ProgressState::new();
        progress.start();
        progress.set_message("Thinking...".to_string());

        let line = spinner_line(&progress);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_spinner_line_inactive() {
        let progress = ProgressState::new();
        let line = spinner_line(&progress);
        assert!(line.spans.is_empty());
    }

    #[test]
    fn test_done_line() {
        let line = done_line(2300);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Done"));
        assert!(text.contains("2.3s"));
    }

    #[test]
    fn test_error_line() {
        let line = error_line("something went wrong");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Error"));
        assert!(text.contains("something went wrong"));
    }

    #[test]
    fn test_compact_progress() {
        let mut progress = ProgressState::new();
        progress.start();
        progress.set_message("Loading".to_string());

        let line = compact_progress_line(&progress, 10);
        assert!(!line.spans.is_empty());
    }
}
