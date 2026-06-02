//! Full-screen ratatui model picker modal.
//!
//! Suspends the reedline REPL, opens an alternate-screen TUI with a
//! filterable list of all configured models, and returns the chosen
//! `(provider_name, model_name)` — or `None` on cancel.

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::io::{self};

use crate::commands::Commands;

type Backend = CrosstermBackend<io::Stdout>;

struct PickerItem {
    display: String,
    provider: String,
    model: String,
    is_active: bool,
    vision: bool,
}

struct Picker {
    items: Vec<PickerItem>,
    filtered_indices: Vec<usize>,
    state: ListState,
    filter: String,
}

impl Picker {
    fn build(config: &core_agentic::Config) -> Self {
        let active_provider = config.active_provider().map(|p| p.name.clone());
        let active_model = config.active_model().map(|m| m.model.clone());

        let mut items = Vec::new();
        for (_pi, provider) in config.providers.iter().enumerate() {
            for (_mi, model) in provider.models.iter().enumerate() {
                let display = model.display_name.as_deref().unwrap_or(&model.model);
                let is_active = active_provider.as_deref() == Some(&provider.name)
                    && active_model.as_deref() == Some(&model.model);
                let caps = model.effective_capabilities();
                items.push(PickerItem {
                    display: display.to_string(),
                    provider: provider.name.clone(),
                    model: model.model.clone(),
                    is_active,
                    vision: caps.vision,
                });
            }
        }

        let filtered_indices: Vec<usize> = (0..items.len()).collect();
        let mut state = ListState::default();
        let default_pos = filtered_indices
            .iter()
            .position(|&i| items[i].is_active)
            .unwrap_or(0);
        state.select(Some(default_pos));

        Self {
            items,
            filtered_indices,
            state,
            filter: String::new(),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered_indices.clear();
        for (i, item) in self.items.iter().enumerate() {
            if query.is_empty()
                || item.display.to_lowercase().contains(&query)
                || item.provider.to_lowercase().contains(&query)
                || item.model.to_lowercase().contains(&query)
            {
                self.filtered_indices.push(i);
            }
        }
        if self.filtered_indices.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }

    fn selected_item(&self) -> Option<&PickerItem> {
        let idx = self.filtered_indices.get(self.state.selected()?)?;
        Some(&self.items[*idx])
    }

    fn select_up(&mut self) {
        if let Some(cur) = self.state.selected() {
            if cur > 0 {
                self.state.select(Some(cur - 1));
            }
        }
    }

    fn select_down(&mut self) {
        if let Some(cur) = self.state.selected() {
            if cur + 1 < self.filtered_indices.len() {
                self.state.select(Some(cur + 1));
            }
        }
    }
}

fn render(f: &mut ratatui::Frame, picker: &mut Picker, area: Rect) {
    let max_w = 80u16;
    let max_h = 24u16;
    let w = area.width.min(max_w);
    let h = area.height.min(max_h);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let picker_area = Rect { x, y, width: w, height: h };

    f.render_widget(Clear, picker_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(100, 100, 140)))
        .title(Span::styled(
            " Select Model ",
            Style::default()
                .fg(Color::Rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);

    render_filter(f, picker, chunks[0]);
    render_list(f, picker, chunks[1]);
    render_footer(f, chunks[2]);
}

fn render_filter(f: &mut ratatui::Frame, picker: &Picker, area: Rect) {
    let prompt = Span::styled("> ", Style::default().fg(Color::Cyan));
    let text = if picker.filter.is_empty() {
        Span::styled(
            "type to filter…",
            Style::default().add_modifier(Modifier::DIM),
        )
    } else {
        Span::raw(&picker.filter)
    };
    let line = Line::from(vec![prompt, text]);
    let para = Paragraph::new(line);
    f.render_widget(para, area);
}

fn render_list(f: &mut ratatui::Frame, picker: &mut Picker, area: Rect) {
    let active_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let vision_style = Style::default().fg(Color::Rgb(135, 206, 250));
    let highlight = Style::default()
        .bg(Color::Rgb(60, 60, 90))
        .fg(Color::White);

    let items: Vec<ListItem> = picker
        .filtered_indices
        .iter()
        .map(|&idx| {
            let item = &picker.items[idx];
            let marker = if item.is_active {
                Span::styled("✓ ", active_style)
            } else {
                Span::raw("  ")
            };
            let name = Span::styled(
                item.display.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            );
            let provider = Span::styled(format!(" [{}]", item.provider), dim);
            let eye = if item.vision {
                Span::styled("  👁", vision_style)
            } else {
                Span::raw("")
            };
            ListItem::new(Line::from(vec![marker, name, provider, eye]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(highlight)
        .highlight_spacing(HighlightSpacing::Always);

    f.render_stateful_widget(list, area, &mut picker.state);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect) {
    let dim = Style::default()
        .fg(Color::Rgb(140, 140, 160))
        .add_modifier(Modifier::DIM);
    let key_style = Style::default()
        .fg(Color::Rgb(200, 200, 220))
        .add_modifier(Modifier::BOLD);
    let lines = vec![
        Line::from(vec![
            Span::styled(" ↑/↓", key_style),
            Span::styled(" navigate", dim),
            Span::raw("  "),
            Span::styled("enter", key_style),
            Span::styled(" select", dim),
            Span::raw("  "),
            Span::styled("esc", key_style),
            Span::styled(" cancel", dim),
        ]),
        Line::from(vec![
            Span::styled(" type", key_style),
            Span::styled(" to filter", dim),
            Span::raw("  "),
            Span::styled("ctrl+u", key_style),
            Span::styled(" clear filter", dim),
        ]),
    ];
    let para = Paragraph::new(lines).wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

struct TerminalGuard {
    restored: bool,
}

impl TerminalGuard {
    fn suspend_reedline() -> Self {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        Self { restored: false }
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        let _ = execute!(io::stdout(), EnterAlternateScreen);
        let _ = enable_raw_mode();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

pub fn run(commands: &mut Commands) -> Option<(String, String)> {
    if commands.get_config().providers.is_empty()
        || commands
            .get_config()
            .providers
            .iter()
            .all(|p| p.models.is_empty())
    {
        crate::widgets::inline::print_blank();
        crate::widgets::inline::print_line(&crate::widgets::components::warning_badge(
            "No models configured.",
        ));
        crate::widgets::inline::print_blank();
        return None;
    }

    let _guard = TerminalGuard::suspend_reedline();

    enable_raw_mode().ok()?;
    execute!(io::stdout(), EnterAlternateScreen).ok()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).ok()?;
    terminal.clear().ok()?;

    let result = run_picker_loop(&mut terminal, commands);

    drop(terminal);
    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();

    result
}

fn run_picker_loop(
    terminal: &mut Terminal<Backend>,
    commands: &mut Commands,
) -> Option<(String, String)> {
    let mut picker = Picker::build(commands.get_config());

    loop {
        terminal.draw(|f| render(f, &mut picker, f.area())).ok()?;

        match event::read().ok()? {
            Event::Key(key) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return None;
                }
                KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                    picker.select_up();
                }
                KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                    picker.select_down();
                }
                KeyCode::Enter => {
                    if let Some(item) = picker.selected_item() {
                        let name = item.model.clone();
                        match commands.switch_model(&name) {
                            Ok(result) => return Some(result),
                            Err(_) => return None,
                        }
                    }
                }
                KeyCode::Backspace => {
                    picker.filter.pop();
                    picker.apply_filter();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    picker.filter.clear();
                    picker.apply_filter();
                }
                KeyCode::Char(c) if !c.is_control() => {
                    picker.filter.push(c);
                    picker.apply_filter();
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}
