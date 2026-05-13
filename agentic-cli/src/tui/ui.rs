//! UI rendering for TUI

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use super::app::{App, MessageRole};
use super::dropdown::DropdownType;
use super::input::{render_input, render_placeholder};
use super::markdown_widget::{MarkdownContent, role_prefix};

/// Padding configuration
const PADDING_HORIZONTAL: u16 = 2;
const PADDING_VERTICAL: u16 = 1;

/// Draw the entire UI
pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Main container with padding
    let padded_area = Rect {
        x: size.x + PADDING_HORIZONTAL,
        y: size.y + PADDING_VERTICAL,
        width: size.width.saturating_sub(PADDING_HORIZONTAL * 2),
        height: size.height.saturating_sub(PADDING_VERTICAL * 2),
    };

    // Background
    let bg_block = Block::default()
        .style(Style::default().bg(Color::Rgb(20, 20, 30)));
    frame.render_widget(bg_block, size);

    // Main layout: header, messages, input
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Messages
            Constraint::Length(3),  // Progress (when loading)
            Constraint::Length(3),  // Input
        ])
        .split(padded_area);

    // Draw components
    draw_header(frame, chunks[0]);
    draw_messages(frame, app, chunks[1]);
    draw_progress(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);

    // Draw dropdown overlay if active
    if app.dropdown.is_some() {
        draw_dropdown(frame, app, chunks[3]);
    }
}

/// Draw header bar
fn draw_header(frame: &mut Frame, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " 🤖 Agentic ",
            Style::default()
                .fg(Color::Rgb(46, 204, 113))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "│",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            " Interactive Mode ",
            Style::default().fg(Color::Rgb(180, 180, 180)),
        ),
        Span::styled(
            "│",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            " /help ",
            Style::default().fg(Color::Rgb(241, 196, 15)),
        ),
        Span::styled(
            "for commands",
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(60, 60, 80)))
            .style(Style::default().bg(Color::Rgb(30, 30, 45))),
    );

    frame.render_widget(header, area);
}

/// Draw messages area
fn draw_messages(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner_area = area.inner(Margin::new(1, 1));
    
    // Build message lines
    let mut all_lines: Vec<Line> = Vec::new();

    for message in &app.messages {
        // Add spacing between messages
        if !all_lines.is_empty() {
            all_lines.push(Line::default());
        }

        // Role header with timestamp
        let (role_span, content_style) = match message.role {
            MessageRole::User => role_prefix("user"),
            MessageRole::Assistant => role_prefix("assistant"),
            MessageRole::System => role_prefix("system"),
            MessageRole::Error => role_prefix("error"),
        };

        let time_str = message.timestamp.format("%H:%M").to_string();
        all_lines.push(Line::from(vec![
            role_span,
            Span::styled(
                format!("  {}", time_str),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        // Parse and render markdown content
        let md_content = MarkdownContent::parse(&message.content);
        for line in md_content.lines {
            // Apply content style to each span if needed
            let styled_line = if content_style != Style::default() {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|span| {
                            if span.style == Style::default() {
                                Span::styled(span.content, content_style)
                            } else {
                                span
                            }
                        })
                        .collect::<Vec<_>>(),
                )
            } else {
                line
            };
            all_lines.push(styled_line);
        }
    }

    // Add current streaming response if any
    if !app.current_response.is_empty() {
        if !all_lines.is_empty() {
            all_lines.push(Line::default());
        }
        
        let (role_span, _) = role_prefix("assistant");
        all_lines.push(Line::from(vec![
            role_span,
            Span::styled(
                "  streaming...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));

        let md_content = MarkdownContent::parse(&app.current_response);
        for line in md_content.lines {
            all_lines.push(line);
        }
    }

    // Calculate scroll
    let visible_height = inner_area.height as usize;
    let total_lines = all_lines.len();
    
    // Auto-scroll to bottom when new content arrives
    let max_scroll = total_lines.saturating_sub(visible_height);
    if app.scroll_offset > max_scroll {
        app.scroll_offset = max_scroll;
    }

    // Create paragraph with scroll
    let messages_widget = Paragraph::new(all_lines.clone())
        .scroll((app.scroll_offset as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(60, 60, 80)))
                .title(Span::styled(
                    " Messages ",
                    Style::default()
                        .fg(Color::Rgb(180, 180, 180))
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(Color::Rgb(25, 25, 35))),
        );

    frame.render_widget(messages_widget, area);

    // Scrollbar
    if total_lines > visible_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(app.scroll_offset)
            .viewport_content_length(visible_height);

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin::new(0, 1)),
            &mut scrollbar_state,
        );
    }
}

/// Draw progress indicator
fn draw_progress(frame: &mut Frame, app: &App, area: Rect) {
    if !app.is_loading {
        // Empty space when not loading
        let empty = Block::default()
            .style(Style::default().bg(Color::Rgb(20, 20, 30)));
        frame.render_widget(empty, area);
        return;
    }

    let _progress_text = app.progress.display();
    let progress_bar = app.progress.progress_bar(area.width.saturating_sub(4) as usize);

    let progress_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                app.progress.spinner(),
                Style::default()
                    .fg(Color::Rgb(52, 152, 219))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                &app.progress.message,
                Style::default().fg(Color::Rgb(180, 180, 180)),
            ),
            Span::raw(" "),
            Span::styled(
                format!("({})", app.progress.elapsed_str()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            progress_bar,
            Style::default().fg(Color::Rgb(52, 152, 219)),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(52, 152, 219)))
            .style(Style::default().bg(Color::Rgb(25, 30, 40))),
    );

    frame.render_widget(progress_widget, area);
}

/// Draw input area
fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_line = if app.input.is_empty() {
        render_placeholder()
    } else {
        render_input(&app.input, app.cursor_pos)
    };

    let input_widget = Paragraph::new(input_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(
                    if app.is_loading {
                        Color::DarkGray
                    } else {
                        Color::Rgb(46, 204, 113)
                    },
                ))
                .title(Span::styled(
                    if app.is_loading {
                        " Input (waiting...) "
                    } else {
                        " Input "
                    },
                    Style::default()
                        .fg(if app.is_loading {
                            Color::DarkGray
                        } else {
                            Color::Rgb(46, 204, 113)
                        })
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(Color::Rgb(30, 30, 45))),
        );

    frame.render_widget(input_widget, area);
}

/// Draw dropdown overlay
fn draw_dropdown(frame: &mut Frame, app: &App, input_area: Rect) {
    let dropdown = match &app.dropdown {
        Some(d) => d,
        None => return,
    };

    if dropdown.is_empty() {
        return;
    }

    // Calculate dropdown position (above input)
    let visible_items = dropdown.visible_items();
    let dropdown_height = (visible_items.len() + 2).min(10) as u16;
    
    let dropdown_area = Rect {
        x: input_area.x + 1,
        y: input_area.y.saturating_sub(dropdown_height),
        width: input_area.width.saturating_sub(2).min(50),
        height: dropdown_height,
    };

    // Clear area behind dropdown
    frame.render_widget(Clear, dropdown_area);

    // Build list items
    let items: Vec<ListItem> = visible_items
        .iter()
        .map(|(_, item, selected)| {
            let style = if *selected {
                Style::default()
                    .bg(Color::Rgb(52, 152, 219))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(200, 200, 200))
            };

            let icon = match dropdown.dropdown_type {
                DropdownType::Command => "⌘ ",
                DropdownType::File => {
                    if item.ends_with('/') {
                        "📁 "
                    } else {
                        "📄 "
                    }
                }
            };

            let mut spans = vec![
                Span::styled(icon, style),
                Span::styled(item.to_string(), style),
            ];

            // Add description for commands
            if let Some(desc) = dropdown.get_description(item) {
                spans.push(Span::styled(
                    format!("  {}", desc),
                    if *selected {
                        Style::default()
                            .bg(Color::Rgb(52, 152, 219))
                            .fg(Color::Rgb(200, 200, 200))
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = format!(" {} {} ", dropdown.icon(), dropdown.title());
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(100, 100, 120)))
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(Color::Rgb(241, 196, 15))
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(Color::Rgb(35, 35, 50))),
        );

    frame.render_widget(list, dropdown_area);
}
