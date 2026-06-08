//! Interactive REPL mode using custom ratatui input widget
//!
//! Provides an interactive CLI with:
//! - `/` command completion dropdown (auto-activates on `/`)
//! - `@` file path completion dropdown (auto-activates on `@`)
//! - Syntax highlighting for `/` (yellow) and `@` (blue)
//! - In-memory input history with ↑/↓ navigation
//! - Session statistics, conversation history, save/load
//!
//! Uses crossterm raw mode for key capture and ratatui inline rendering
//! (no alternate screen). Replaces the previous reedline-based input.

use anyhow::Result;
use crossterm::{
    cursor::MoveUp,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    style::{Color, Modifier, Style as RStyle},
    text::{Line, Span as RSpan},
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::cli::SkillAction;
use crate::commands::Commands;
use crate::input_buffer::InputBuffer;
use crate::input_renderer::{self, PromptMetadata};
use crate::tui::dropdown::{Dropdown, DropdownType};
use crate::widgets::components;
use crate::widgets::inline;

// ── Session statistics ──────────────────────────────────────

#[derive(Clone)]
struct SessionStats {
    messages_sent: Arc<AtomicU32>,
    tool_calls: Arc<AtomicU32>,
    total_input_tokens: Arc<AtomicU32>,
    total_output_tokens: Arc<AtomicU32>,
    total_cache_read_tokens: Arc<AtomicU32>,
    total_cache_creation_tokens: Arc<AtomicU32>,
    session_start: Instant,
}

impl SessionStats {
    fn new() -> Self {
        Self {
            messages_sent: Arc::new(AtomicU32::new(0)),
            tool_calls: Arc::new(AtomicU32::new(0)),
            total_input_tokens: Arc::new(AtomicU32::new(0)),
            total_output_tokens: Arc::new(AtomicU32::new(0)),
            total_cache_read_tokens: Arc::new(AtomicU32::new(0)),
            total_cache_creation_tokens: Arc::new(AtomicU32::new(0)),
            session_start: Instant::now(),
        }
    }

    fn increment_messages(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    fn reset(&self) {
        self.messages_sent.store(0, Ordering::Relaxed);
        self.tool_calls.store(0, Ordering::Relaxed);
        self.total_input_tokens.store(0, Ordering::Relaxed);
        self.total_output_tokens.store(0, Ordering::Relaxed);
        self.total_cache_read_tokens.store(0, Ordering::Relaxed);
        self.total_cache_creation_tokens.store(0, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn increment_tool_calls(&self) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn add_input_tokens(&self, n: u32) {
        self.total_input_tokens.fetch_add(n, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn add_output_tokens(&self, n: u32) {
        self.total_output_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn add_cache_read_tokens(&self, n: u32) {
        self.total_cache_read_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn add_cache_creation_tokens(&self, n: u32) {
        self.total_cache_creation_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn total_cache_read_tokens(&self) -> u32 {
        self.total_cache_read_tokens.load(Ordering::Relaxed)
    }

    fn total_cache_creation_tokens(&self) -> u32 {
        self.total_cache_creation_tokens.load(Ordering::Relaxed)
    }

    fn cache_hit_ratio(&self) -> f64 {
        let read = self.total_cache_read_tokens() as f64;
        let created = self.total_cache_creation_tokens() as f64;
        let total = read + created;
        if total > 0.0 {
            read / total
        } else {
            0.0
        }
    }

    fn messages_sent(&self) -> u32 {
        self.messages_sent.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    fn tool_calls(&self) -> u32 {
        self.tool_calls.load(Ordering::Relaxed)
    }

    fn total_input_tokens(&self) -> u32 {
        self.total_input_tokens.load(Ordering::Relaxed)
    }

    fn total_output_tokens(&self) -> u32 {
        self.total_output_tokens.load(Ordering::Relaxed)
    }

    fn elapsed_secs(&self) -> u64 {
        self.session_start.elapsed().as_secs()
    }

    fn elapsed_str(&self) -> String {
        let secs = self.elapsed_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }

    fn format_tokens(&self, n: u32) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.1}K", n as f64 / 1_000.0)
        } else {
            format!("{}", n)
        }
    }
}

// ── Slash command definitions with aliases ──────────────────

const SLASH_COMMANDS: &[(&str, &[&str], &str)] = &[
    ("help", &["h", "?"], "Show help message"),
    ("new", &["n", "clear", "cls"], "Start a new session"),
    ("config", &["cfg"], "Show current configuration"),
    ("history", &["hist"], "Show conversation history"),
    ("tools", &["t"], "List available tools"),
    ("models", &["m"], "List all models from all providers"),
    ("provider", &["prov"], "Switch or show provider"),
    ("sessions", &["ss"], "List and resume previous sessions"),
    ("mcp", &[], "Show MCP server status"),
    ("plan", &["p"], "Create a plan for a goal"),
    ("skills", &[], "List all indexed skills"),
    ("search", &["find"], "Search conversation memory"),
    ("image", &["img"], "Attach an image for the next turn"),
    ("stats", &[], "Show session statistics"),
    ("quit", &["q", "exit"], "Exit interactive mode"),
];

// ── Conversation entry ──────────────────────────────────────

#[derive(Debug)]
struct ConversationEntry {
    role: String,
    content: String,
    timestamp: chrono::DateTime<chrono::Local>,
}

// ── REPL loop ───────────────────────────────────────────────

pub async fn run(mut commands: Commands) -> Result<()> {
    let stats = SessionStats::new();
    let mut model_info = get_model_info(&commands);

    // Initialize session
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();
    let mut current_session = crate::session::create(
        &cwd,
        &model_info.provider,
        &model_info.model,
    );

    print_banner(&model_info, &stats);

    let mut buffer = InputBuffer::new();
    let mut dropdown: Option<Dropdown> = None;
    let mut conversation: Vec<ConversationEntry> = Vec::new();

    // Enter raw mode for key capture
    enable_raw_mode()?;

    // Guard: ensure raw mode is disabled even on panic
    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }
    let _raw_guard = RawModeGuard;

    let result = repl_loop(
        &mut buffer,
        &mut dropdown,
        &mut commands,
        &mut conversation,
        &mut current_session,
        &stats,
        &mut model_info,
    )
    .await;

    // Explicitly disable (guard also does this on drop)
    drop(_raw_guard);
    disable_raw_mode().ok();

    // Auto-save session on exit
    if !current_session.messages.is_empty() {
        let _ = crate::session::save(&current_session);
    }

    print_goodbye(&stats);
    result
}

// ── Inner REPL loop ─────────────────────────────────────────
//
// Rendering architecture:
//   1. Status bar: printed ONCE per prompt cycle (permanent, part of scrollback)
//   2. Input area (dropdown + prompt): transient block, cleared & re-rendered
//      on every keystroke. Track newline count for proper cursor movement.
//   3. On submit: clear input area, print submitted text as permanent.

async fn repl_loop(
    buffer: &mut InputBuffer,
    dropdown: &mut Option<Dropdown>,
    commands: &mut Commands,
    conversation: &mut Vec<ConversationEntry>,
    current_session: &mut crate::session::Session,
    stats: &SessionStats,
    model_info: &mut ModelInfo,
) -> Result<()> {
    // Number of newlines currently rendered in the input area.
    // This includes dropdown title + dropdown items. The prompt line is
    // transient (no \n) so it doesn't count as a newline, but it occupies
    // the same row as the cursor after the last \n.
    let mut area_newlines: u16 = 0;
    // Whether we need to print the status bar for this prompt cycle.
    let mut needs_status_bar = true;

    loop {
        // ── Print status bar once per prompt cycle ──
        if needs_status_bar {
            print_prompt_status_bar(model_info, stats);
            needs_status_bar = false;
        }

        // ── Clear previous input area ──
        clear_input_area(area_newlines);
        area_newlines = 0;

        // ── Render dropdown (if active) + prompt line ──
        let meta = PromptMetadata::new(
            model_info.provider.clone(),
            model_info.model.clone(),
        );

        // Dropdown lines (each with \n)
        if let Some(ref dd) = dropdown {
            if !dd.is_empty() {
                area_newlines = input_renderer::render_dropdown_lines(dd) as u16;
            }
        }

        // Prompt line (transient, no \n)
        input_renderer::render_prompt_line(&meta, buffer);

        // ── Wait for key event ──
        let event = event::read()?;

        match event {
            Event::Key(key) => {
                // If dropdown is open, handle dropdown-specific keys first
                if dropdown.is_some() {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::NONE, KeyCode::Up) => {
                            if let Some(ref mut dd) = dropdown {
                                dd.select_prev();
                            }
                            continue;
                        }
                        (KeyModifiers::NONE, KeyCode::Down) => {
                            if let Some(ref mut dd) = dropdown {
                                dd.select_next();
                            }
                            continue;
                        }
                        (KeyModifiers::NONE, KeyCode::Tab)
                        | (KeyModifiers::NONE, KeyCode::Enter) => {
                            accept_dropdown(buffer, dropdown);
                            area_newlines = 0;
                            continue;
                        }
                        (KeyModifiers::NONE, KeyCode::Esc) => {
                            *dropdown = None;
                            area_newlines = 0;
                            continue;
                        }
                        // Any other key: close dropdown and fall through to input handling
                        _ => {
                            *dropdown = None;
                            area_newlines = 0;
                        }
                    }
                }

                // Handle normal input keys
                match (key.modifiers, key.code) {
                    // ── Submit ──
                    (KeyModifiers::NONE, KeyCode::Enter) => {
                        // Clear the input area
                        clear_input_area(area_newlines);
                        area_newlines = 0;
                        *dropdown = None;

                        let input = buffer.submit();
                        if input.is_empty() {
                            inline::print_blank();
                            continue;
                        }

                        // Handle input
                        let should_break = handle_input(
                            &input,
                            commands,
                            conversation,
                            current_session,
                            stats,
                            model_info,
                        )
                        .await;

                        // Refresh model_info in case provider changed
                        *model_info = get_model_info(commands);
                        needs_status_bar = true;

                        if should_break {
                            return Ok(());
                        }
                    }

                    // ── Exit ──
                    (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                        clear_input_area(area_newlines);
                        return Ok(());
                    }

                    // ── Cancel ──
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                        clear_input_area(area_newlines);
                        area_newlines = 0;
                        *dropdown = None;
                        buffer.clear();
                        inline::print_blank();
                        inline::print_line(&components::info_badge(
                            "Use /quit or Ctrl+D to exit.",
                        ));
                        inline::print_blank();
                        needs_status_bar = true;
                    }

                    // ── Character input ──
                    (KeyModifiers::NONE, KeyCode::Char(c)) => {
                        buffer.insert_char(c);
                        buffer.reset_history_browse();
                        update_dropdown(buffer, dropdown);
                    }

                    // ── Backspace ──
                    (KeyModifiers::NONE, KeyCode::Backspace) => {
                        buffer.delete_backward();
                        buffer.reset_history_browse();
                        update_dropdown(buffer, dropdown);
                    }

                    // ── Ctrl+Backspace / Ctrl+W ──
                    (KeyModifiers::CONTROL, KeyCode::Backspace)
                    | (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                        buffer.delete_word_backward();
                        buffer.reset_history_browse();
                        update_dropdown(buffer, dropdown);
                    }

                    // ── Delete ──
                    (KeyModifiers::NONE, KeyCode::Delete) => {
                        buffer.delete_forward();
                        buffer.reset_history_browse();
                        update_dropdown(buffer, dropdown);
                    }

                    // ── Cursor movement ──
                    (KeyModifiers::NONE, KeyCode::Left) => {
                        buffer.cursor_left();
                    }
                    (KeyModifiers::NONE, KeyCode::Right) => {
                        buffer.cursor_right();
                    }
                    (KeyModifiers::CONTROL, KeyCode::Left) => {
                        buffer.cursor_word_left();
                    }
                    (KeyModifiers::CONTROL, KeyCode::Right) => {
                        buffer.cursor_word_right();
                    }
                    (KeyModifiers::NONE, KeyCode::Home) => {
                        buffer.cursor_home();
                    }
                    (KeyModifiers::NONE, KeyCode::End) => {
                        buffer.cursor_end();
                    }

                    // ── History navigation (only when no dropdown) ──
                    (KeyModifiers::NONE, KeyCode::Up) => {
                        buffer.history_up();
                    }
                    (KeyModifiers::NONE, KeyCode::Down) => {
                        buffer.history_down();
                    }

                    // ── Ctrl+L — clear screen ──
                    (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                        clear_input_area(area_newlines);
                        area_newlines = 0;
                        crossterm::execute!(
                            std::io::stdout(),
                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                            crossterm::cursor::MoveTo(0, 0)
                        )
                        .ok();
                        print_banner(model_info, stats);
                        needs_status_bar = true;
                    }

                    // Ignore other key combinations
                    _ => {}
                }
            }
            Event::Resize(_, _) => {
                // Terminal resized — just re-render on next iteration
            }
            _ => {}
        }
    }
}

// ── Dropdown trigger & accept logic ─────────────────────────

/// Update dropdown state based on current input and cursor position.
fn update_dropdown(buffer: &InputBuffer, dropdown: &mut Option<Dropdown>) {
    let text = buffer.text();
    let cursor = buffer.cursor();

    // 1) Check for `/` command trigger
    if text.starts_with('/') {
        let before_cursor = &text[..cursor];
        if !before_cursor.contains(' ') {
            let query = &text[1..cursor];
            *dropdown = Some(Dropdown::new(DropdownType::Command, query.to_string()));
            return;
        }

        // 1b) Check for `/models <partial>` model trigger
        if let Some(space_pos) = before_cursor.find(' ') {
            let cmd = &text[1..space_pos];
            if cmd == "models" || cmd == "m" {
                let query = &text[space_pos + 1..cursor];
                *dropdown = Some(Dropdown::new(DropdownType::Model, query.to_string()));
                return;
            }
        }
    }

    // 2) Check for `@` file trigger
    if let Some(at_pos) = find_at_trigger(text, cursor) {
        let query = &text[at_pos + 1..cursor];
        *dropdown = Some(Dropdown::new(DropdownType::File, query.to_string()));
        return;
    }

    // 3) No trigger
    *dropdown = None;
}

/// Find the byte position of the `@` trigger that the cursor is inside.
fn find_at_trigger(text: &str, cursor: usize) -> Option<usize> {
    let before_cursor = &text[..cursor];

    for (i, c) in before_cursor.char_indices().rev() {
        match c {
            '@' => {
                let at_start = i == 0;
                let after_space = i > 0 && text[..i].ends_with(char::is_whitespace);
                if at_start || after_space {
                    let after_at = &before_cursor[i + 1..];
                    if !after_at.contains(char::is_whitespace) {
                        return Some(i);
                    }
                }
                return None;
            }
            w if w.is_whitespace() => return None,
            _ => continue,
        }
    }
    None
}

/// Accept the currently selected dropdown item and insert into buffer.
fn accept_dropdown(buffer: &mut InputBuffer, dropdown: &mut Option<Dropdown>) {
    let dd = match dropdown.take() {
        Some(d) => d,
        None => return,
    };

    let selected_text = match dd.selected_item() {
        Some(s) => s.to_string(),
        None => return,
    };

    let is_dir = selected_text.ends_with('/');

    match dd.dropdown_type {
        DropdownType::Command => {
            // Replace entire input with /command
            buffer.set_text(format!("/{} ", selected_text));
        }
        DropdownType::File => {
            // Replace from @ to cursor with the selected file path
            if let Some(at_pos) = find_at_trigger(buffer.text(), buffer.cursor()) {
                let suffix = if is_dir { "" } else { " " };
                let replacement = format!("{}{}", selected_text, suffix);
                buffer.replace_range(at_pos + 1, &replacement);
            }
        }
        DropdownType::Model => {
            // Extract model ID from display string
            let model_id = dd
                .get_model_id(&selected_text)
                .unwrap_or_else(|| selected_text.clone());
            // Replace from after "/models " to cursor with model ID
            let space_pos = buffer.text().find(' ').unwrap_or(buffer.text().len());
            let prefix = buffer.text()[..=space_pos].to_string();
            let after_cursor = buffer.text()[buffer.cursor()..].to_string();
            buffer.set_text(format!("{}{} {}", prefix, model_id, after_cursor));
        }
    }

    // For directories, re-trigger dropdown to show contents
    if is_dir {
        let mut new_dd = None;
        update_dropdown(buffer, &mut new_dd);
        *dropdown = new_dd;
    }
}

/// Clear the input area (dropdown lines + prompt line) from the terminal.
///
/// After rendering, cursor is `area_newlines` rows below the start of the area
/// (the prompt line is on the same row as the cursor, without a `\n`).
/// To clear: move cursor up `area_newlines` rows, then clear from cursor down.
fn clear_input_area(area_newlines: u16) {
    use crossterm::cursor::MoveToColumn;
    use crossterm::terminal::{Clear, ClearType};
    use crossterm::ExecutableCommand;
    use std::io::Write;

    let mut stdout = std::io::stdout();
    if area_newlines > 0 {
        let _ = stdout.execute(MoveUp(area_newlines));
    }
    let _ = stdout.execute(MoveToColumn(0));
    let _ = stdout.execute(Clear(ClearType::FromCursorDown));
    let _ = stdout.flush();
}

// ── Input handling ──────────────────────────────────────────

/// Handle submitted input. Returns true if the loop should break (quit).
async fn handle_input(
    input: &str,
    commands: &mut Commands,
    conversation: &mut Vec<ConversationEntry>,
    current_session: &mut crate::session::Session,
    stats: &SessionStats,
    model_info: &ModelInfo,
) -> bool {
    // Handle slash commands
    if input.starts_with('/') {
        if let Some(action) = handle_slash_command(input) {
            return handle_repl_action(
                &action,
                commands,
                conversation,
                current_session,
                stats,
                model_info,
            )
            .await;
        }
        return false;
    }

    // Handle plain text shortcuts
    match input.to_lowercase().as_str() {
        "exit" | "quit" | "q" => return true,
        "help" | "h" => print_help(),
        "new" | "n" => {
            if !current_session.messages.is_empty() {
                let _ = crate::session::save(current_session);
            }
            let cwd = std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string();
            *current_session = crate::session::create(
                &cwd,
                &model_info.provider,
                &model_info.model,
            );
            conversation.clear();
            stats.reset();
            commands.restart_session();

            crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                crossterm::cursor::MoveTo(0, 0)
            )
            .ok();
            print_banner(model_info, stats);
            inline::print_blank();
            inline::print_line(&components::success_badge("New session started."));
            inline::print_blank();
            print_status_bar(model_info, stats);
        }
        _ => {
            process_message(input, commands, conversation, current_session, stats, model_info)
                .await;
        }
    }

    false
}

// ── REPL actions ────────────────────────────────────────────

enum ReplAction {
    Quit,
    NewSession,
    Config,
    History,
    Tools,
    Stats,
    Provider(String),
    Models,
    ModelsSwitch(String),
    Sessions,
    SessionsResume(String),
    Mcp,
    Plan(String),
    Skills,
    SkillsLoad(String),
    Search(String),
    Image(String),
}

fn handle_slash_command(input: &str) -> Option<ReplAction> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts
        .get(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    match cmd {
        "/quit" | "/q" | "/exit" => Some(ReplAction::Quit),
        "/help" | "/h" => {
            print_help();
            None
        }
        "/new" | "/n" | "/clear" | "/cls" => Some(ReplAction::NewSession),
        "/config" | "/cfg" => Some(ReplAction::Config),
        "/history" | "/hist" => Some(ReplAction::History),
        "/tools" | "/t" => Some(ReplAction::Tools),
        "/stats" => Some(ReplAction::Stats),
        "/mcp" => Some(ReplAction::Mcp),
        "/provider" if !arg.is_empty() => Some(ReplAction::Provider(arg)),
        "/provider" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Usage: /provider <name>"));
            inline::print_blank();
            None
        }
        "/models" | "/m" if !arg.is_empty() => Some(ReplAction::ModelsSwitch(arg)),
        "/models" | "/m" => Some(ReplAction::Models),
        "/sessions" | "/ss" if !arg.is_empty() => Some(ReplAction::SessionsResume(arg)),
        "/sessions" | "/ss" => Some(ReplAction::Sessions),
        "/plan" if !arg.is_empty() => Some(ReplAction::Plan(arg)),
        "/plan" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Usage: /plan <goal>"));
            inline::print_blank();
            None
        }
        "/skills" if !arg.is_empty() => Some(ReplAction::SkillsLoad(arg)),
        "/skills" => Some(ReplAction::Skills),
        "/search" | "/find" if !arg.is_empty() => Some(ReplAction::Search(arg)),
        "/search" | "/find" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge(
                "Usage: /search <query>  (case-insensitive substring match over conversation memory)",
            ));
            inline::print_blank();
            None
        }
        "/image" | "/img" if !arg.is_empty() => Some(ReplAction::Image(arg)),
        "/image" | "/img" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge(
                "Usage: /image <path | data: url | http(s) url>",
            ));
            inline::print_blank();
            None
        }
        _ => {
            inline::print_blank();
            inline::print_line(&components::error_badge(&format!(
                "Unknown command: {}",
                cmd
            )));
            inline::print_line(&Line::from(vec![
                RSpan::raw("  Type "),
                RSpan::styled(
                    "/help",
                    RStyle::default()
                        .fg(Color::Rgb(255, 215, 0))
                        .add_modifier(Modifier::BOLD),
                ),
                RSpan::raw(" for available commands."),
            ]));
            inline::print_blank();
            None
        }
    }
}

/// Handle a ReplAction. Returns true if the loop should break (quit).
async fn handle_repl_action(
    action: &ReplAction,
    commands: &mut Commands,
    conversation: &mut Vec<ConversationEntry>,
    current_session: &mut crate::session::Session,
    stats: &SessionStats,
    model_info: &ModelInfo,
) -> bool {
    match action {
        ReplAction::Quit => return true,

        ReplAction::NewSession => {
            if !current_session.messages.is_empty() {
                if let Err(e) = crate::session::save(current_session) {
                    inline::print_line(&components::warning_badge(&format!(
                        "Could not auto-save session: {}",
                        e
                    )));
                }
            }
            let cwd = std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string();
            *current_session = crate::session::create(
                &cwd,
                &model_info.provider,
                &model_info.model,
            );
            conversation.clear();
            stats.reset();
            commands.restart_session();

            crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                crossterm::cursor::MoveTo(0, 0)
            )
            .ok();
            print_banner(model_info, stats);
            inline::print_blank();
            inline::print_line(&components::success_badge("New session started."));
            inline::print_blank();
            print_status_bar(model_info, stats);
        }

        ReplAction::Config => {
            commands.config_show_inline();
        }

        ReplAction::History => {
            show_history(conversation);
        }

        ReplAction::Tools => {
            commands.list_tools();
        }

        ReplAction::Stats => {
            show_stats(stats, model_info);
        }

        ReplAction::Sessions => {
            show_sessions();
        }

        ReplAction::SessionsResume(id) => {
            match crate::session::load(id) {
                Ok(loaded) => {
                    if !current_session.messages.is_empty() {
                        let _ = crate::session::save(current_session);
                    }
                    conversation.clear();
                    for msg in &loaded.messages {
                        conversation.push(ConversationEntry {
                            role: msg.role.clone(),
                            content: msg.content.clone(),
                            timestamp: chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
                                .map(|dt| dt.with_timezone(&chrono::Local))
                                .unwrap_or_else(|_| chrono::Local::now()),
                        });
                    }
                    *current_session = loaded;
                    commands.restart_session();
                    stats.reset();

                    crossterm::execute!(
                        std::io::stdout(),
                        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                        crossterm::cursor::MoveTo(0, 0)
                    )
                    .ok();
                    print_banner(model_info, stats);
                    inline::print_blank();
                    inline::print_line(&components::success_badge(&format!(
                        "Resumed: {} ({} messages)",
                        current_session.title,
                        current_session.messages.len()
                    )));
                    inline::print_blank();
                    print_status_bar(model_info, stats);
                }
                Err(e) => {
                    inline::print_blank();
                    inline::print_line(&components::error_badge(&format!(
                        "Failed to load session: {}",
                        e
                    )));
                    inline::print_blank();
                }
            }
        }

        ReplAction::Provider(name) => {
            inline::print_blank();
            inline::print_line(&components::warning_badge(
                "Provider switching not yet supported in REPL.",
            ));
            inline::print_line(&Line::from(vec![
                RSpan::raw("  Use: "),
                RSpan::styled(
                    "agentic config edit",
                    RStyle::default().add_modifier(Modifier::BOLD),
                ),
                RSpan::raw(" to change providers."),
            ]));
            inline::print_blank();
            let _ = name;
        }

        ReplAction::Models => {
            if let Some((provider, model)) = commands.pick_model_interactive_inline() {
                inline::print_blank();
                inline::print_line(&components::success_badge(&format!(
                    "Switched to {} / {}",
                    provider, model
                )));
                inline::print_blank();
                print_status_bar(model_info, stats);
            }
        }

        ReplAction::ModelsSwitch(name) => {
            match commands.switch_model(name) {
                Ok((provider, model)) => {
                    inline::print_blank();
                    inline::print_line(&components::success_badge(&format!(
                        "Switched to {} / {}",
                        provider, model
                    )));
                    inline::print_blank();
                }
                Err(e) => {
                    inline::print_blank();
                    inline::print_line(&components::error_badge(&e.to_string()));
                    inline::print_line(&Line::from(vec![
                        RSpan::raw("  Use "),
                        RSpan::styled(
                            "/models",
                            RStyle::default().add_modifier(Modifier::BOLD),
                        ),
                        RSpan::raw(" to see available models."),
                    ]));
                    inline::print_blank();
                }
            }
        }

        ReplAction::Mcp => {
            commands.show_mcp_status();
        }

        ReplAction::Plan(goal) => {
            conversation.push(ConversationEntry {
                role: "user".into(),
                content: format!("[plan] {}", goal),
                timestamp: chrono::Local::now(),
            });
            stats.increment_messages();

            print_turn_separator(model_info);
            let start = Instant::now();
            if let Err(e) = commands.plan_inline(goal).await {
                inline::print_blank();
                inline::print_line(&components::error_badge(&e.to_string()));
                inline::print_blank();
            } else {
                let elapsed = start.elapsed();
                conversation.push(ConversationEntry {
                    role: "assistant".into(),
                    content: format!("(plan executed in {:.1}s)", elapsed.as_secs_f64()),
                    timestamp: chrono::Local::now(),
                });
                print_response_summary(stats);
            }
        }

        ReplAction::Skills => {
            commands.skill_command(&SkillAction::List).ok();
        }

        ReplAction::SkillsLoad(name) => {
            use ratatui::style::{Color, Modifier, Style};
            use ratatui::text::{Line, Span};

            inline::print_blank();
            inline::print_line(&components::section_header(
                "⚡",
                &format!("Loading skill: {}", name),
                Color::Rgb(255, 215, 0),
            ));
            inline::print_blank();

            let discovery_config: core_agentic::DiscoveryConfig =
                core_agentic::DiscoveryConfig::from(&commands.get_config().skills);
            let index = core_agentic::discover_skills(&discovery_config);

            if let Some(skill) = index.get(name) {
                inline::print_line(&Line::from(vec![
                    Span::styled("  📦 ", Style::default()),
                    Span::styled(
                        format!("{} — {}", skill.name(), skill.description()),
                        Style::default()
                            .fg(Color::Rgb(255, 215, 0))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                inline::print_line(&Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        format!("Path: {}", skill.dir.display()),
                        Style::default()
                            .fg(Color::Rgb(100, 100, 120))
                            .add_modifier(Modifier::DIM),
                    ),
                ]));
                inline::print_blank();

                let preview: Vec<&str> = skill.body.lines().take(5).collect();
                for line in &preview {
                    inline::print_line(&Line::from(vec![
                        Span::raw("     "),
                        Span::styled(
                            *line,
                            Style::default().fg(Color::Rgb(180, 180, 200)),
                        ),
                    ]));
                }
                if skill.body.lines().count() > 5 {
                    inline::print_line(&Line::from(vec![
                        Span::raw("     "),
                        Span::styled(
                            "...",
                            Style::default().fg(Color::Rgb(100, 100, 120)),
                        ),
                    ]));
                }
            } else {
                inline::print_line(&components::warning_badge(&format!(
                    "Skill '{}' not found. Use /skills to list available skills.",
                    name
                )));
            }
            inline::print_blank();
        }

        ReplAction::Search(query) => {
            commands.search_memory_inline(query);
        }

        ReplAction::Image(path) => {
            commands.attach_image_inline(path);
        }
    }

    false
}

// ── Message processing ────────────────────────────────────

/// Process a single user message through the agent.
async fn process_message(
    input: &str,
    commands: &mut Commands,
    conversation: &mut Vec<ConversationEntry>,
    current_session: &mut crate::session::Session,
    stats: &SessionStats,
    model_info: &ModelInfo,
) {
    conversation.push(ConversationEntry {
        role: "user".into(),
        content: input.to_string(),
        timestamp: chrono::Local::now(),
    });
    stats.increment_messages();

    crate::session::push_message(current_session, "user", input);

    print_turn_separator(model_info);
    inline::print_line(&components::dotted_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();

    let start = Instant::now();
    let result = commands.run(input).await;

    if let Err(e) = result {
        inline::print_blank();
        inline::print_line(&components::error_badge(&e.to_string()));
        inline::print_blank();
    } else {
        let elapsed = start.elapsed();
        let estimated_input = (input.len() as f32 / 4.0) as u32;
        stats.add_input_tokens(estimated_input);

        conversation.push(ConversationEntry {
            role: "assistant".into(),
            content: format!("(response in {:.1}s)", elapsed.as_secs_f64()),
            timestamp: chrono::Local::now(),
        });

        crate::session::push_message(
            current_session,
            "assistant",
            &format!("(response in {:.1}s)", elapsed.as_secs_f64()),
        );
        let _ = crate::session::save(current_session);

        print_response_summary(stats);
    }
}

// ── Model info ──────────────────────────────────────────────

struct ModelInfo {
    provider: String,
    model: String,
    api_base: String,
    agent_md_name: Option<String>,
    memory_md_loaded: bool,
    vision_capable: bool,
    active_skill: Option<String>,
}

fn get_model_info(commands: &Commands) -> ModelInfo {
    let (provider, model, api_base) = commands.model_info();
    let agent_md_name = commands
        .agent_md_path()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    ModelInfo {
        provider,
        model,
        api_base,
        agent_md_name,
        memory_md_loaded: commands.memory_md_loaded(),
        vision_capable: commands.active_model_capabilities().vision,
        active_skill: core_agentic::active_skill(),
    }
}

// ── Print helpers ───────────────────────────────────────────

fn print_banner(model_info: &ModelInfo, stats: &SessionStats) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    inline::print_blank();

    let title = components::banner_title(
        "  █▀▀█ █▀▀ █▀▀█ █▀█ ▀█▀ █ █▀▀   ▇ ▅ ▃",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&title);
    let subtitle = components::banner_title(
        "  █▒░█ █▀▀ █▀▀█ █ █  █  █ █   ▉ ▅ ▁",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&subtitle);
    inline::print_blank();

    let info_lines = vec![
        Line::from(vec![
            RSpan::styled("📂 ", RStyle::default()),
            RSpan::styled("cwd  ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(cwd, RStyle::default().fg(Color::Rgb(180, 180, 200))),
        ]),
        Line::from(vec![
            RSpan::styled("⚡ ", RStyle::default()),
            RSpan::styled("model", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::raw("  "),
            RSpan::styled(
                model_info.provider.clone(),
                RStyle::default()
                    .fg(Color::Rgb(64, 224, 208))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" / ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                model_info.model.clone(),
                RStyle::default()
                    .fg(Color::Rgb(255, 215, 0))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("💡 ", RStyle::default()),
            RSpan::styled("tip  ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled("type ", RStyle::default()),
            RSpan::styled(
                "/help",
                RStyle::default()
                    .fg(Color::Rgb(255, 215, 0))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" for commands, ", RStyle::default()),
            RSpan::styled(
                "@",
                RStyle::default()
                    .fg(Color::Rgb(135, 206, 250))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" to reference files", RStyle::default()),
        ]),
    ];

    let mut info_lines = info_lines;
    if model_info.agent_md_name.is_some()
        || model_info.memory_md_loaded
        || model_info.active_skill.is_some()
    {
        let mut spans: Vec<RSpan<'static>> = vec![
            RSpan::styled("🔗 ", RStyle::default()),
            RSpan::styled("ctx  ", RStyle::default().add_modifier(Modifier::DIM)),
        ];
        let mut first = true;
        if let Some(ref name) = model_info.agent_md_name {
            spans.push(RSpan::styled(
                format!("📄 {}", name),
                RStyle::default()
                    .fg(Color::Rgb(176, 196, 222))
                    .add_modifier(Modifier::BOLD),
            ));
            first = false;
        }
        if model_info.memory_md_loaded {
            if !first {
                spans.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.push(RSpan::styled(
                "🧠 memory.md",
                RStyle::default()
                    .fg(Color::Rgb(176, 196, 222))
                    .add_modifier(Modifier::BOLD),
            ));
            first = false;
        }
        if let Some(ref skill) = model_info.active_skill {
            if !first {
                spans.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.push(RSpan::styled(
                format!("⚡ skill:{}", skill),
                RStyle::default()
                    .fg(Color::Rgb(241, 196, 15))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        info_lines.push(Line::from(spans));
    }

    let skills = core_agentic::list_skills();
    if !skills.is_empty() {
        const MAX_SKILL_NAMES: usize = 5;
        let mut skill_names: Vec<String> = skills
            .iter()
            .take(MAX_SKILL_NAMES)
            .map(|(name, _)| name.clone())
            .collect();
        let remaining = skills.len().saturating_sub(MAX_SKILL_NAMES);
        if remaining > 0 {
            skill_names.push(format!("+{} more", remaining));
        }
        let skill_str = skill_names.join(" · ");
        info_lines.push(Line::from(vec![
            RSpan::styled("📦 ", RStyle::default()),
            RSpan::styled("skills", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::raw(" "),
            RSpan::styled(skill_str, RStyle::default().fg(Color::Rgb(241, 196, 15))),
        ]));
    }

    let panel_lines = components::panel(
        "Welcome",
        &info_lines,
        components::BoxStyle::Rounded,
        Color::Rgb(100, 100, 140),
    );
    inline::print_lines(&panel_lines);
    inline::print_blank();

    print_status_bar(model_info, stats);
}

fn print_turn_separator(model_info: &ModelInfo) {
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

    let mut spans = vec![
        RSpan::raw("  "),
        RSpan::styled(
            format!("⚡ {}/{}", model_info.provider, model_info.model),
            RStyle::default().fg(Color::Rgb(255, 215, 0)),
        ),
    ];

    if let Some(ref branch) = git_branch {
        spans.push(RSpan::styled(
            format!(" 📌 {}", branch),
            RStyle::default()
                .fg(Color::Rgb(135, 206, 250))
                .add_modifier(Modifier::DIM),
        ));
    }

    inline::print_line(&Line::from(spans));
}

fn print_status_bar(model_info: &ModelInfo, stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    let sep = RSpan::styled("  │  ", RStyle::default().fg(Color::Rgb(60, 60, 80)));

    inline::print_line(&Line::from(vec![
        RSpan::raw("  "),
        RSpan::styled(
            format!("⚡ {}", model_info.provider),
            RStyle::default().fg(Color::Rgb(255, 215, 0)),
        ),
        RSpan::styled(
            format!("/{}", model_info.model),
            RStyle::default()
                .fg(Color::Rgb(241, 196, 15))
                .add_modifier(Modifier::DIM),
        ),
        if model_info.vision_capable {
            RSpan::styled(
                "  👁",
                RStyle::default().fg(Color::Rgb(135, 206, 250)),
            )
        } else {
            RSpan::raw("")
        },
        sep.clone(),
        RSpan::styled(
            format!("📊 {} ↑ / {} ↓", in_tok, out_tok),
            RStyle::default().fg(Color::Rgb(186, 85, 211)),
        ),
    ]));

    let has_agent_md = model_info.agent_md_name.is_some();
    let has_skill = model_info.active_skill.is_some();
    if has_agent_md || model_info.memory_md_loaded || has_skill {
        let mut chips: Vec<RSpan<'static>> = vec![RSpan::raw("  ")];
        if let Some(ref name) = model_info.agent_md_name {
            chips.push(RSpan::styled(
                format!("📄 {}", name),
                RStyle::default().fg(Color::Rgb(176, 196, 222)),
            ));
        }
        if model_info.memory_md_loaded {
            if has_agent_md {
                chips.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().fg(Color::Rgb(60, 60, 80)),
                ));
            }
            chips.push(RSpan::styled(
                "🧠 memory.md",
                RStyle::default().fg(Color::Rgb(176, 196, 222)),
            ));
        }
        if let Some(ref skill) = model_info.active_skill {
            if has_agent_md || model_info.memory_md_loaded {
                chips.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().fg(Color::Rgb(60, 60, 80)),
                ));
            }
            chips.push(RSpan::styled(
                format!("⚡ skill:{}", skill),
                RStyle::default()
                    .fg(Color::Rgb(241, 196, 15))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        inline::print_line(&Line::from(chips));
    }

    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}

fn print_prompt_status_bar(model_info: &ModelInfo, stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    let sep = RSpan::styled(" \u{2502} ", RStyle::default().fg(Color::Rgb(60, 60, 80)));

    let cwd = std::env::current_dir()
        .unwrap_or_default()
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

    let mut spans: Vec<RSpan<'static>> = vec![
        RSpan::raw("  "),
        RSpan::styled(
            format!("\u{1f4c2} {}", cwd),
            RStyle::default().fg(Color::Rgb(180, 180, 200)),
        ),
    ];

    if let Some(ref branch) = git_branch {
        spans.push(RSpan::styled(
            format!(" \u{b7} {}", branch),
            RStyle::default().fg(Color::Rgb(135, 206, 250)),
        ));
    }

    spans.push(sep.clone());
    spans.push(RSpan::styled(
        format!("\u{26a1} {}/{}", model_info.provider, model_info.model),
        RStyle::default().fg(Color::Rgb(255, 215, 0)),
    ));

    if model_info.vision_capable {
        spans.push(RSpan::styled(
            " \u{1f441}",
            RStyle::default().fg(Color::Rgb(135, 206, 250)),
        ));
    }

    spans.push(sep.clone());
    spans.push(RSpan::styled(
        format!(
            "\u{1f4ca}{}\u{2191}/{}\u{2193}",
            in_tok, out_tok
        ),
        RStyle::default().fg(Color::Rgb(186, 85, 211)),
    ));

    inline::print_line(&Line::from(spans));
}

fn print_response_summary(stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    let sep = RSpan::styled(
        " │ ",
        RStyle::default().fg(Color::Rgb(60, 60, 80)),
    );

    inline::print_blank();
    inline::print_line(&components::rounded_dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_line(&Line::from(vec![
        RSpan::raw("  "),
        RSpan::styled(
            " ✓ done ",
            RStyle::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(Color::Rgb(39, 174, 96))
                .add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
        RSpan::styled(
            format!("📊 {}↑/{}↓", in_tok, out_tok),
            RStyle::default().fg(Color::Rgb(186, 85, 211)),
        ),
    ]));
    inline::print_line(&components::rounded_dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}

fn print_help() {
    let help_md = r#"## 📖 Commands

**Slash commands:**
- `/help`              Show this help
- `/new`               Start a new session (clears conversation)
- `/config`            Show current configuration
- `/history`           Show conversation history
- `/tools`             List available tools
- `/stats`             Show session statistics
- `/mcp`               Show MCP server status
- `/skills`            List all indexed skills
- `/skills <name>`     Load and display a skill
- `/sessions`          List previous sessions
- `/sessions <id>`     Resume a previous session
- `/plan <goal>`       Create a plan for a goal
- `/search <query>`    Search conversation memory (case-insensitive)
- `/image <path>`      Attach image for next turn (path | data: | http(s) URL)
- `/provider <name>`   Switch provider (not yet supported)
- `/models`            Pick model interactively
- `/models <name>`     Switch to model by name (supports auto-complete)
- `/quit`              Exit interactive mode

**Shortcuts:**
- `help`, `h`          Show help
- `new`, `n`           New session
- `exit`, `q`          Exit

**Completion & Hints:**
- `/` → Dropdown with command list + descriptions
- `@` → Dropdown with file list + icons
- Tab/Enter → Accept dropdown selection
- ↑/↓ → Navigate dropdown or history
- Esc → Close dropdown

**Tips:**
- Type any text to send as a task to the AI agent
- Use `/skills <name>` to load a skill before starting a task
- Ctrl+C to cancel, Ctrl+D to exit
"#;

    inline::print_blank();
    inline::print_line(&components::section_header(
        "📖",
        "Help",
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    let md = crate::widgets::markdown::MarkdownContent::parse(help_md);
    inline::print_lines(&md.lines);
    inline::print_blank();
}

fn show_stats(stats: &SessionStats, model_info: &ModelInfo) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    let in_tok = stats.total_input_tokens();
    let out_tok = stats.total_output_tokens();
    let total_tok = in_tok + out_tok;

    inline::print_blank();
    inline::print_line(&components::section_header(
        "📊",
        "Session Statistics",
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    inline::print_line(&components::subsection_header(
        "Session",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Duration",
        &stats.elapsed_str(),
        12,
        Color::Rgb(46, 204, 113),
    ));
    inline::print_line(&components::kv_line(
        "Messages",
        &format!("{}", stats.messages_sent()),
        12,
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Tool calls",
        &format!("{}", stats.tool_calls()),
        12,
        Color::Rgb(135, 206, 250),
    ));
    inline::print_blank();

    inline::print_line(&components::subsection_header(
        "Model",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_badge(
        "Provider",
        &model_info.provider,
        12,
        Color::Rgb(255, 255, 255),
        Color::Rgb(155, 89, 182),
    ));
    inline::print_line(&components::kv_badge(
        "Model",
        &model_info.model,
        12,
        Color::Rgb(255, 255, 255),
        Color::Rgb(52, 152, 219),
    ));
    inline::print_line(&components::kv_line(
        "API Base",
        &model_info.api_base,
        12,
        Color::Rgb(180, 180, 200),
    ));
    inline::print_blank();

    inline::print_line(&components::subsection_header(
        "Token Usage",
        Color::Rgb(255, 215, 0),
    ));

    if total_tok > 0 {
        let in_ratio = in_tok as f32 / total_tok as f32;
        let out_ratio = out_tok as f32 / total_tok as f32;
        inline::print_line(&components::labeled_bar(
            "Input",
            in_ratio,
            30,
            Color::Rgb(46, 204, 113),
            Color::Rgb(50, 50, 60),
        ));
        inline::print_line(&components::labeled_bar(
            "Output",
            out_ratio,
            30,
            Color::Rgb(231, 76, 60),
            Color::Rgb(50, 50, 60),
        ));
    } else {
        inline::print_line(&Line::from(vec![
            RSpan::styled(
                "  Input:        ",
                RStyle::default().add_modifier(Modifier::DIM),
            ),
            RSpan::styled(
                "— no data yet —",
                RStyle::default().add_modifier(Modifier::DIM),
            ),
        ]));
    }
    inline::print_line(&Line::from(vec![
        RSpan::styled(
            "  Total:        ",
            RStyle::default().add_modifier(Modifier::DIM),
        ),
        RSpan::styled(
            format!("{} tokens", stats.format_tokens(total_tok)),
            RStyle::default()
                .fg(Color::Rgb(255, 215, 0))
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    inline::print_blank();

    let cache_read = stats.total_cache_read_tokens();
    let cache_created = stats.total_cache_creation_tokens();
    if cache_read > 0 || cache_created > 0 {
        inline::print_line(&components::subsection_header(
            "Prompt Cache",
            Color::Rgb(46, 204, 113),
        ));
        inline::print_line(&components::kv_line(
            "Cache read",
            &format!("{} tokens", stats.format_tokens(cache_read)),
            14,
            Color::Rgb(46, 204, 113),
        ));
        inline::print_line(&components::kv_line(
            "Cache created",
            &format!("{} tokens", stats.format_tokens(cache_created)),
            14,
            Color::Rgb(52, 152, 219),
        ));
        let ratio = stats.cache_hit_ratio();
        inline::print_line(&components::kv_line(
            "Hit ratio",
            &format!("{:.0}%", ratio * 100.0),
            14,
            Color::Rgb(241, 196, 15),
        ));
        inline::print_blank();
    }

    inline::print_line(&components::subsection_header(
        "Environment",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Working dir",
        &cwd,
        12,
        Color::Rgb(180, 180, 200),
    ));
    inline::print_blank();
    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}

fn show_history(conversation: &[ConversationEntry]) {
    inline::print_blank();
    if conversation.is_empty() {
        inline::print_line(&components::warning_badge(
            "No messages in this session yet.",
        ));
        inline::print_blank();
        return;
    }

    inline::print_line(&components::section_header(
        "📜",
        &format!("Conversation History ({} messages)", conversation.len()),
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    for (i, entry) in conversation.iter().enumerate() {
        let time = entry.timestamp.format("%H:%M:%S");
        let (icon, badge_bg) = match entry.role.as_str() {
            "user" => ("👤", Color::Rgb(52, 152, 219)),
            "assistant" => ("🤖", Color::Rgb(46, 204, 113)),
            _ => ("💬", Color::Rgb(241, 196, 15)),
        };
        let content_preview = if entry.content.len() > 120 {
            format!("{}...", &entry.content[..117])
        } else {
            entry.content.clone()
        };
        inline::print_line(&Line::from(vec![
            RSpan::raw("  "),
            RSpan::styled(
                format!(" {} ", icon),
                RStyle::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(badge_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" "),
            RSpan::styled(
                format!("#{:02}", i + 1),
                RStyle::default()
                    .fg(Color::Rgb(180, 180, 200))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" "),
            RSpan::styled(
                format!("[{}]", time),
                RStyle::default().add_modifier(Modifier::DIM),
            ),
            RSpan::raw("  "),
            RSpan::raw(content_preview),
        ]));
    }
    inline::print_blank();
}

fn print_goodbye(stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    inline::print_blank();

    let cache_read = stats.total_cache_read_tokens();
    let cache_created = stats.total_cache_creation_tokens();
    let has_cache = cache_read > 0 || cache_created > 0;

    let mut summary_lines = vec![
        Line::from(vec![
            RSpan::styled("💬 ", RStyle::default()),
            RSpan::styled("Messages ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("{}", stats.messages_sent()),
                RStyle::default()
                    .fg(Color::Rgb(135, 206, 250))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("⏱ ", RStyle::default()),
            RSpan::styled("Duration ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                stats.elapsed_str(),
                RStyle::default()
                    .fg(Color::Rgb(46, 204, 113))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("📊 ", RStyle::default()),
            RSpan::styled("Tokens   ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("{} ↑", in_tok),
                RStyle::default().fg(Color::Rgb(46, 204, 113)),
            ),
            RSpan::raw(" / "),
            RSpan::styled(
                format!("{} ↓", out_tok),
                RStyle::default().fg(Color::Rgb(231, 76, 60)),
            ),
        ]),
    ];

    if has_cache {
        summary_lines.push(Line::from(vec![
            RSpan::styled("📦 ", RStyle::default()),
            RSpan::styled("Cache    ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("💰 {} rd", stats.format_tokens(cache_read)),
                RStyle::default()
                    .fg(Color::Rgb(46, 204, 113))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" / "),
            RSpan::styled(
                format!("✏️ {} cr", stats.format_tokens(cache_created)),
                RStyle::default()
                    .fg(Color::Rgb(52, 152, 219))
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::raw("  "),
            RSpan::styled(
                format!("({:.0}% hit)", stats.cache_hit_ratio() * 100.0),
                RStyle::default().fg(Color::Rgb(241, 196, 15)),
            ),
        ]));
    }

    let panel_lines = components::panel(
        "Session Summary",
        &summary_lines,
        components::BoxStyle::Rounded,
        Color::Rgb(100, 100, 140),
    );
    inline::print_lines(&panel_lines);
    inline::print_blank();

    let goodbye = components::gradient_text(
        "  👋 See you next time!",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&goodbye);
    inline::print_blank();
}

fn show_sessions() {
    inline::print_blank();

    let sessions = match crate::session::list() {
        Ok(s) => s,
        Err(e) => {
            inline::print_line(&components::error_badge(&format!(
                "Failed to list sessions: {}",
                e
            )));
            inline::print_blank();
            return;
        }
    };

    if sessions.is_empty() {
        inline::print_line(&components::warning_badge("No previous sessions found."));
        inline::print_blank();
        return;
    }

    inline::print_line(&components::section_header(
        "📜",
        &format!("Sessions ({})", sessions.len()),
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    let bold = RStyle::default().add_modifier(Modifier::BOLD);
    let dim = RStyle::default().add_modifier(Modifier::DIM);

    for (i, s) in sessions.iter().enumerate().take(20) {
        let time = crate::session::format_relative_time(&s.updated_at);

        let title = if s.title.is_empty() {
            "Untitled"
        } else {
            &s.title
        };

        inline::print_line(&Line::from(vec![
            RSpan::styled(format!("  {:2}. ", i + 1), dim.clone()),
            RSpan::styled(title.to_string(), bold.clone()),
            RSpan::styled(format!("  {} msgs", s.message_count), dim.clone()),
            RSpan::raw("  "),
            RSpan::styled(
                time,
                RStyle::default().fg(Color::Rgb(135, 206, 250)),
            ),
        ]));
        inline::print_line(&Line::from(vec![
            RSpan::styled("      ", RStyle::default()),
            RSpan::styled(format!("{}", s.id), dim.clone()),
            RSpan::styled(
                format!(" · {} · {}/{}", s.directory, s.provider, s.model),
                dim.clone(),
            ),
        ]));
        inline::print_blank();
    }

    if sessions.len() > 20 {
        inline::print_line(&Line::from(vec![RSpan::styled(
            format!("  ... and {} more", sessions.len() - 20),
            dim.clone(),
        )]));
        inline::print_blank();
    }

    inline::print_line(&Line::from(vec![
        RSpan::styled("💡 ", RStyle::default()),
        RSpan::styled("Tip: ", RStyle::default().add_modifier(Modifier::DIM)),
        RSpan::raw("Use "),
        RSpan::styled(
            "/sessions <id>",
            RStyle::default()
                .fg(Color::Rgb(255, 215, 0))
                .add_modifier(Modifier::BOLD),
        ),
        RSpan::raw(" to resume a session"),
    ]));
    inline::print_blank();
}
