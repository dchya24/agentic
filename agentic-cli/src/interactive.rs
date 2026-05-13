//! Interactive REPL mode using reedline
//!
//! Provides an interactive CLI with:
//! - `/` command completion with description popup (auto-activates on `/`)
//! - `@` file path completion with popup (auto-activates on `@`)
//! - Syntax highlighting for `/` (yellow) and `@` (blue)
//! - Fish-style inline hints
//! - Session statistics, conversation history, save/load

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use nu_ansi_term::{Color as AnsiColor, Style};
use reedline::{
    default_emacs_keybindings, Completer, DescriptionMenu, EditCommand, Emacs, FileBackedHistory,
    Highlighter, Hinter, KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode,
    PromptHistorySearch, PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal,
    Span, StyledText, Suggestion, ValidationResult, Validator,
};
use std::borrow::Cow;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::commands::Commands;

// ── Session statistics ──────────────────────────────────────

#[derive(Clone)]
struct SessionStats {
    messages_sent: Arc<AtomicU32>,
    tool_calls: Arc<AtomicU32>,
    total_input_tokens: Arc<AtomicU32>,
    total_output_tokens: Arc<AtomicU32>,
    session_start: Instant,
}

impl SessionStats {
    fn new() -> Self {
        Self {
            messages_sent: Arc::new(AtomicU32::new(0)),
            tool_calls: Arc::new(AtomicU32::new(0)),
            total_input_tokens: Arc::new(AtomicU32::new(0)),
            total_output_tokens: Arc::new(AtomicU32::new(0)),
            session_start: Instant::now(),
        }
    }

    fn increment_messages(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
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

    fn messages_sent(&self) -> u32 {
        self.messages_sent.load(Ordering::Relaxed)
    }

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
    ("clear", &["cls", "c"], "Clear screen"),
    ("config", &["cfg"], "Show current configuration"),
    ("history", &["hist"], "Show conversation history"),
    ("tools", &["t"], "List available tools"),
    ("model", &["m"], "Switch or show model"),
    ("provider", &["prov"], "Switch or show provider"),
    ("save", &["s"], "Save conversation to file"),
    ("load", &["l"], "Load conversation from file"),
    ("mcp", &[], "Show MCP server status"),
    ("plan", &["p"], "Create a plan for a goal"),
    ("stats", &[], "Show session statistics"),
    ("quit", &["q", "exit"], "Exit interactive mode"),
];

// ── Spinner frames ──────────────────────────────────────────

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn start_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(SPINNER_FRAMES)
            .template("{spinner:.cyan} {msg:.dim}")
            .unwrap(),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

// ── Agentic Completer ───────────────────────────────────────

/// Custom completer that handles both `/` commands and `@` file paths.
struct AgenticCompleter;

impl Completer for AgenticCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        // Guard: reedline may pass pos > line.len() in some edge cases
        let pos = pos.min(line.len());
        let before_cursor = &line[..pos];

        // ── Case 1: `/` command completion ──
        if line.starts_with('/') {
            if let Some(space_pos) = line.find(' ') {
                if pos <= space_pos {
                    return complete_slash_command_suggestions(&line[..pos]);
                }
            } else {
                return complete_slash_command_suggestions(before_cursor);
            }
        }

        // ── Case 2: `@` file completion ──
        if let Some(at_pos) = find_at_trigger(before_cursor) {
            let query = &before_cursor[at_pos + 1..];
            return complete_file_path_suggestions(query, at_pos);
        }

        Vec::new()
    }
}

/// Find the `@` trigger position in text before cursor.
fn find_at_trigger(text: &str) -> Option<usize> {
    for (i, c) in text.char_indices().rev() {
        match c {
            '@' => {
                let at_start = i == 0;
                let after_space = i > 0 && text[..i].ends_with(char::is_whitespace);
                if at_start || after_space {
                    let after_at = &text[i + 1..];
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

/// Complete slash commands and return Suggestions with descriptions.
fn complete_slash_command_suggestions(partial: &str) -> Vec<Suggestion> {
    let partial_lower = partial.to_lowercase();

    SLASH_COMMANDS
        .iter()
        .filter(|(cmd, aliases, _)| {
            let full = format!("/{}", cmd);
            if full.starts_with(&partial_lower) || full.starts_with(partial) {
                return true;
            }
            aliases.iter().any(|a| {
                let alias_full = format!("/{}", a);
                alias_full.starts_with(&partial_lower) || alias_full.starts_with(partial)
            })
        })
        .map(|(cmd, aliases, desc)| {
            let display = if aliases.is_empty() {
                format!("/{}", cmd)
            } else {
                format!("/{} ({})", cmd, aliases.join(", "))
            };
            Suggestion {
                value: format!("/{}", cmd),
                display_override: Some(display),
                description: Some(desc.to_string()),
                style: None,
                extra: None,
                span: Span::new(0, partial.len()),
                append_whitespace: true,
                match_indices: None,
            }
        })
        .collect()
}

/// Complete file paths and return Suggestions.
fn complete_file_path_suggestions(query: &str, at_pos: usize) -> Vec<Suggestion> {
    let mut results = Vec::new();

    let (base_path, search_pattern) = if query.contains('/') {
        let path = PathBuf::from(query);
        if query.ends_with('/') {
            (path, String::new())
        } else {
            let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let file_part = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_string();
            (parent, file_part)
        }
    } else {
        (PathBuf::from("."), query.to_string())
    };

    if let Ok(entries) = std::fs::read_dir(&base_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with('.') && !search_pattern.starts_with('.') {
                continue;
            }

            if matches!(file_name.as_str(), "target" | "node_modules" | ".git")
                && !search_pattern.starts_with(&file_name[..2.min(file_name.len())])
            {
                continue;
            }

            let matches = if search_pattern.is_empty() {
                true
            } else {
                let fl = file_name.to_lowercase();
                let pl = search_pattern.to_lowercase();
                fl.starts_with(&pl) || fl.contains(&pl)
            };

            if matches {
                let base_str = base_path.to_string_lossy();
                let full_path = if base_str == "." {
                    file_name.clone()
                } else {
                    let clean = base_str.trim_end_matches('/');
                    format!("{}/{}", clean, file_name)
                };

                let is_dir = entry.path().is_dir();
                let display = if is_dir {
                    format!("{}/", full_path)
                } else {
                    full_path.clone()
                };

                let icon = if is_dir { "📁" } else { "📄" };

                results.push(Suggestion {
                    value: display.clone(),
                    display_override: Some(format!("{} {}", icon, display)),
                    description: Some(if is_dir {
                        "Directory".to_string()
                    } else {
                        let ext_str = entry
                            .path()
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_string();
                        let ext_ref = ext_str.as_str();
                        format!("File ({})", if ext_str.is_empty() { "?" } else { ext_ref })
                    }),
                    style: None,
                    extra: None,
                    span: Span::new(at_pos, at_pos + 1 + query.len()),
                    append_whitespace: !is_dir,
                    match_indices: None,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        let a_dir = a.value.ends_with('/');
        let b_dir = b.value.ends_with('/');
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.value.to_lowercase().cmp(&b.value.to_lowercase()),
        }
    });

    results.truncate(20);
    results
}

// ── Agentic Highlighter ─────────────────────────────────────

/// Syntax highlighter for `/` commands and `@` file references.
struct AgenticHighlighter;

impl Highlighter for AgenticHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled = StyledText::new();

        if line.is_empty() {
            return styled;
        }

        // Slash command highlighting (yellow)
        if line.starts_with('/') {
            if let Some(space_pos) = line.find(' ') {
                styled.push((
                    Style::new().fg(AnsiColor::Yellow).bold(),
                    line[..space_pos].to_string(),
                ));
                styled.push((Style::new(), line[space_pos..].to_string()));
            } else {
                styled.push((
                    Style::new().fg(AnsiColor::Yellow).bold(),
                    line.to_string(),
                ));
            }
            return styled;
        }

        // @ file reference highlighting (blue)
        if line.contains('@') {
            let mut i = 0;
            let mut in_at_ref = false;
            let mut at_start = 0;

            for (pos, c) in line.char_indices() {
                if c == '@' && (pos == 0 || line[..pos].ends_with(char::is_whitespace)) {
                    // Flush previous
                    if i < pos {
                        styled.push((Style::new(), line[i..pos].to_string()));
                    }
                    in_at_ref = true;
                    at_start = pos;
                    i = pos;
                } else if in_at_ref && c.is_whitespace() {
                    styled.push((
                        Style::new().fg(AnsiColor::Rgb(52, 152, 219)).bold(),
                        line[at_start..pos].to_string(),
                    ));
                    in_at_ref = false;
                    i = pos;
                }
            }

            // Flush remaining
            if i < line.len() {
                if in_at_ref {
                    styled.push((
                        Style::new().fg(AnsiColor::Rgb(52, 152, 219)).bold(),
                        line[i..].to_string(),
                    ));
                } else {
                    styled.push((Style::new(), line[i..].to_string()));
                }
            }

            return styled;
        }

        // Default
        styled.push((Style::new(), line.to_string()));
        styled
    }
}

// ── Agentic Hinter ──────────────────────────────────────────

/// Fish-style hinter that shows first match inline.
struct AgenticHinter {
    last_hint: String,
}

impl AgenticHinter {
    fn new() -> Self {
        Self {
            last_hint: String::new(),
        }
    }
}

impl Hinter for AgenticHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        _history: &dyn reedline::History,
        _use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.last_hint.clear();

        // Slash command hints
        if line.starts_with('/') && !line.contains(' ') {
            let partial = &line[..pos];
            let matches: Vec<&&str> = SLASH_COMMANDS
                .iter()
                .map(|(cmd, _, _)| cmd)
                .filter(|cmd| {
                    let full = format!("/{}", **cmd);
                    full != partial && full.starts_with(partial)
                })
                .collect();

            if !matches.is_empty() {
                let first = format!("/{}", matches[0]);
                let remainder = first[partial.len()..].to_string();
                if !remainder.is_empty() {
                    let hint = if matches.len() > 1 {
                        format!("{} [{}+]", remainder, matches.len() - 1)
                    } else {
                        remainder
                    };
                    self.last_hint = hint.clone();
                    return AnsiColor::DarkGray.paint(hint).to_string();
                }
            }
        }

        // @ file path hints
        if line.contains('@') {
            let before = &line[..pos];
            if let Some(at_pos) = find_at_trigger(before) {
                let query = &before[at_pos + 1..];
                let completions = complete_file_path_suggestions(query, at_pos);

                if !completions.is_empty() {
                    let comp = &completions[0].value;
                    if let Some(remaining) = comp.get(query.len()..) {
                        if !remaining.is_empty() {
                            let hint = if completions.len() > 1 {
                                format!("{} [{}+]", remaining, completions.len() - 1)
                            } else {
                                remaining.to_string()
                            };
                            self.last_hint = hint.clone();
                            return AnsiColor::DarkGray.paint(hint).to_string();
                        }
                    }
                }
            }
        }

        String::new()
    }

    fn complete_hint(&self) -> String {
        if let Some(space_pos) = self.last_hint.find(" [") {
            self.last_hint[..space_pos].to_string()
        } else {
            self.last_hint.clone()
        }
    }

    fn next_hint_token(&self) -> String {
        let hint = self.complete_hint();
        if let Some(slash_pos) = hint.find('/') {
            hint[..slash_pos + 1].to_string()
        } else if let Some(space_pos) = hint.find(' ') {
            hint[..space_pos].to_string()
        } else {
            hint
        }
    }
}

// ── Agentic Validator ───────────────────────────────────────

struct AgenticValidator;

impl Validator for AgenticValidator {
    fn validate(&self, _line: &str) -> ValidationResult {
        ValidationResult::Complete
    }
}

// ── Agentic Prompt ──────────────────────────────────────────

struct AgenticPrompt {
    dir_name: String,
}

impl AgenticPrompt {
    fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let dir_name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        Self { dir_name }
    }
}

impl Prompt for AgenticPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let styled = format!(
            "{}{}{}agentic> ",
            AnsiColor::DarkGray.prefix(),
            self.dir_name,
            Style::new().prefix()
        );
        Cow::Owned(styled)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "FAILED ",
        };
        Cow::Owned(format!("{}({})", prefix, history_search.term))
    }
}

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
    let model_info = get_model_info(&commands);

    print_banner(&model_info, &stats);

    // Build reedline with all features
    let completer = Box::new(AgenticCompleter);
    let highlighter = Box::new(AgenticHighlighter);
    let hinter = Box::new(AgenticHinter::new());
    let validator = Box::new(AgenticValidator);
    let prompt = AgenticPrompt::new();

    // File-backed history
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("agentic")
        .join("history.txt");

    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let history = Box::new(
        FileBackedHistory::with_file(1000, history_path.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create history: {}", e))?,
    );

    // Description menu — only_buffer_difference: false so completer gets full buffer
    let completion_menu = Box::new(
        DescriptionMenu::default()
            .with_name("completion_menu")
            .with_marker("\u{25bc} ")
            .with_columns(1)
            .with_selection_rows(8)
            .with_description_rows(4)
            .with_only_buffer_difference(false),
    );

    // Custom keybindings for Tab completion + auto-trigger on `/` and `@`
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    // Arrow up/down: navigate menu items when active, history when not
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Down,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuNext,
            ReedlineEvent::Down,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Up,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuPrevious,
            ReedlineEvent::Up,
        ]),
    );
    // Auto-activate completion menu when typing `/`
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Char('/'),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('/')]),
            ReedlineEvent::Menu("completion_menu".to_string()),
        ]),
    );
    // Auto-activate completion menu when typing `@`
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Char('@'),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('@')]),
            ReedlineEvent::Menu("completion_menu".to_string()),
        ]),
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut line_editor = Reedline::create()
        .with_completer(completer)
        .with_highlighter(highlighter)
        .with_hinter(hinter)
        .with_validator(validator)
        .with_history(history)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_quick_completions(true)
        .with_partial_completions(true);

    let mut conversation: Vec<ConversationEntry> = Vec::new();

    loop {
        let sig = line_editor.read_line(&prompt);

        match sig {
            Ok(Signal::Success(input)) => {
                let input = input.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                // Handle slash commands
                if input.starts_with('/') {
                    if let Some(action) = handle_slash_command(&input) {
                        match action {
                            ReplAction::Quit => break,
                            ReplAction::Clear => {
                                print!("\x1b[2J\x1b[H");
                                std::io::stdout().flush()?;
                                print_status_bar(&model_info, &stats);
                            }
                            ReplAction::ClearHistory => {
                                conversation.clear();
                                commands.clear_memory();
                                println!("\n  \x1b[32m\u{2713} Conversation cleared.\x1b[0m\n");
                                print_status_bar(&model_info, &stats);
                            }
                            ReplAction::Config => {
                                commands.config_show_inline();
                            }
                            ReplAction::History => {
                                show_history(&conversation);
                            }
                            ReplAction::Tools => {
                                commands.list_tools();
                            }
                            ReplAction::Stats => {
                                show_stats(&stats, &model_info);
                            }
                            ReplAction::Save(file) => {
                                save_conversation(&conversation, &file);
                            }
                            ReplAction::Load(file) => {
                                if let Ok(entries) = load_conversation(&file) {
                                    conversation = entries;
                                    println!(
                                        "\n  \x1b[32m\u{2713} Conversation loaded from: {}\x1b[0m\n",
                                        file
                                    );
                                }
                            }
                            ReplAction::Provider(name) => {
                                println!(
                                    "\n  \x1b[33m\u{26a0} Provider switching not yet supported in REPL.\x1b[0m"
                                );
                                println!("  Use: agentic config edit to change providers.\n");
                                let _ = &name;
                            }
                            ReplAction::Model(name) => {
                                println!(
                                    "\n  \x1b[33m\u{26a0} Model switching not yet supported in REPL.\x1b[0m"
                                );
                                println!("  Use: agentic config edit to change models.\n");
                                let _ = &name;
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

                                let pb = start_spinner("Planning...");
                                let start = Instant::now();
                                if let Err(e) =
                                    commands.run(&format!("Create a plan for: {}", goal)).await
                                {
                                    pb.finish_and_clear();
                                    eprintln!("\n  \x1b[31m\u{2717} Error: {}\x1b[0m\n", e);
                                } else {
                                    pb.finish_and_clear();
                                    let elapsed = start.elapsed();
                                    conversation.push(ConversationEntry {
                                        role: "assistant".into(),
                                        content: format!(
                                            "(plan created in {:.1}s)",
                                            elapsed.as_secs_f64()
                                        ),
                                        timestamp: chrono::Local::now(),
                                    });
                                    print_response_summary(&stats, elapsed.as_millis());
                                }
                            }
                        }
                    }
                    continue;
                }

                // Handle plain text as task
                match input.to_lowercase().as_str() {
                    "exit" | "quit" | "q" => break,
                    "help" | "h" => print_help(),
                    "clear" => {
                        print!("\x1b[2J\x1b[H");
                        std::io::stdout().flush()?;
                        print_status_bar(&model_info, &stats);
                    }
                    _ => {
                        conversation.push(ConversationEntry {
                            role: "user".into(),
                            content: input.clone(),
                            timestamp: chrono::Local::now(),
                        });
                        stats.increment_messages();

                        let pb = start_spinner("Thinking...");
                        let start = Instant::now();

                        if let Err(e) = commands.run(&input).await {
                            pb.finish_and_clear();
                            eprintln!("\n  \x1b[31m\u{2717} Error: {}\x1b[0m\n", e);
                        } else {
                            pb.finish_and_clear();
                            let elapsed = start.elapsed();

                            let estimated_input = (input.len() as f32 / 4.0) as u32;
                            stats.add_input_tokens(estimated_input);

                            conversation.push(ConversationEntry {
                                role: "assistant".into(),
                                content: format!(
                                    "(response in {:.1}s)",
                                    elapsed.as_secs_f64()
                                ),
                                timestamp: chrono::Local::now(),
                            });
                            print_response_summary(&stats, elapsed.as_millis());
                        }
                    }
                }
            }
            Ok(Signal::CtrlC) => {
                println!("\n  \x1b[33mUse /quit or Ctrl+D to exit.\x1b[0m\n");
                continue;
            }
            Ok(Signal::CtrlD) => {
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error: {:?}", e);
                break;
            }
        }
    }

    print_goodbye(&stats);
    Ok(())
}

// ── Model info ──────────────────────────────────────────────

struct ModelInfo {
    provider: String,
    model: String,
    api_base: String,
}

fn get_model_info(commands: &Commands) -> ModelInfo {
    let (provider, model, api_base) = commands.model_info();
    ModelInfo {
        provider,
        model,
        api_base,
    }
}

// ── REPL actions ────────────────────────────────────────────

enum ReplAction {
    Quit,
    Clear,
    ClearHistory,
    Config,
    History,
    Tools,
    Stats,
    Save(String),
    Load(String),
    Provider(String),
    Model(String),
    Mcp,
    Plan(String),
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
        "/clear" | "/c" | "/cls" => Some(ReplAction::Clear),
        "/config" | "/cfg" => Some(ReplAction::Config),
        "/history" | "/hist" => Some(ReplAction::History),
        "/tools" | "/t" => Some(ReplAction::Tools),
        "/stats" | "/s" => Some(ReplAction::Stats),
        "/mcp" => Some(ReplAction::Mcp),
        "/save" if !arg.is_empty() => Some(ReplAction::Save(arg)),
        "/save" => {
            println!("\n  \x1b[33mUsage: /save <file>\x1b[0m\n");
            None
        }
        "/load" if !arg.is_empty() => Some(ReplAction::Load(arg)),
        "/load" => {
            println!("\n  \x1b[33mUsage: /load <file>\x1b[0m\n");
            None
        }
        "/provider" if !arg.is_empty() => Some(ReplAction::Provider(arg)),
        "/provider" => {
            println!("\n  \x1b[33mUsage: /provider <name>\x1b[0m\n");
            None
        }
        "/model" if !arg.is_empty() => Some(ReplAction::Model(arg)),
        "/model" => {
            println!("\n  \x1b[33mUsage: /model <name>\x1b[0m\n");
            None
        }
        "/plan" if !arg.is_empty() => Some(ReplAction::Plan(arg)),
        "/plan" => {
            println!("\n  \x1b[33mUsage: /plan <goal>\x1b[0m\n");
            None
        }
        _ => {
            println!("\n  \x1b[33mUnknown command: {}\x1b[0m", cmd);
            println!("  Type \x1b[1m/help\x1b[0m for available commands.\n");
            None
        }
    }
}

// ── Print helpers ───────────────────────────────────────────

fn print_banner(model_info: &ModelInfo, stats: &SessionStats) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    println!();
    println!("  \x1b[1m\x1b[36m\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}\x1b[0m");
    println!("  \x1b[1m\x1b[36m\u{2551}            \u{1f916} Agentic Interactive Mode               \u{2551}\x1b[0m");
    println!("  \x1b[1m\x1b[36m\u{2560}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2563}\x1b[0m");
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  \x1b[2m\u{1f4c2} {}\x1b[0m",
        pad_str(&cwd, 52)
    );
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  \x1b[33m\u{26a1} {} \x1b[2m/ {}\x1b[0m",
        pad_str(&format!("Provider: {}", model_info.provider), 25),
        pad_str(&format!("Model: {}", model_info.model), 25)
    );
    println!("  \x1b[1m\x1b[36m\u{2560}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2563}\x1b[0m");
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  /help    Show commands                              \x1b[1m\x1b[36m\u{2551}\x1b[0m"
    );
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  /tools   List available tools                        \x1b[1m\x1b[36m\u{2551}\x1b[0m"
    );
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  /stats   Show session statistics                     \x1b[1m\x1b[36m\u{2551}\x1b[0m"
    );
    println!(
        "  \x1b[1m\x1b[36m\u{2551}\x1b[0m  /quit    Exit (Ctrl+D)                               \x1b[1m\x1b[36m\u{2551}\x1b[0m"
    );
    println!("  \x1b[1m\x1b[36m\u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}\x1b[0m");
    println!();

    print_status_bar(model_info, stats);
}

fn print_status_bar(model_info: &ModelInfo, stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    println!(
        "  \x1b[2m\u{250c}\u{2500} \x1b[33m\u{26a1} {} {}\x1b[2m \u{2500}\u{2500}\u{2500} \x1b[36m\u{1f4ac} {} msgs\x1b[2m \u{2500}\u{2500}\u{2500} \x1b[35m\u{1f4ca} tokens: {} in / {} out\x1b[2m \u{2500}\u{2500}\u{2500} \x1b[32m\u{23f1} {}\x1b[2m \u{2500}\u{2510}\x1b[0m",
        model_info.provider,
        model_info.model,
        stats.messages_sent(),
        in_tok,
        out_tok,
        stats.elapsed_str(),
    );
    println!();
}

fn print_response_summary(stats: &SessionStats, ms: u128) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    println!();
    println!(
        "  \x1b[2m\u{250c}\u{2500} \x1b[32m\u{2713} Done\x1b[2m \u{2500}\u{2500}\u{2500} \u{23f1} {}.{:03}s \u{2500}\u{2500}\u{2500} \u{1f4ac} {} msgs \u{2500}\u{2500}\u{2500} \u{1f4ca} {} in / {} out \u{2500}\u{2500}\u{2500} \u{23f1} session: {} \u{2500}\u{2510}\x1b[0m",
        ms / 1000,
        ms % 1000,
        stats.messages_sent(),
        in_tok,
        out_tok,
        stats.elapsed_str(),
    );
    println!();
}

fn print_help() {
    println!();
    println!("  \x1b[1m\x1b[36m\u{1f4d6} Commands:\x1b[0m");
    println!();
    println!("  \x1b[33mSlash commands:\x1b[0m");
    println!("  /help              Show this help");
    println!("  /clear             Clear screen");
    println!("  /config            Show current configuration");
    println!("  /history           Show conversation history");
    println!("  /tools             List available tools");
    println!("  /stats             Show session statistics");
    println!("  /mcp               Show MCP server status");
    println!("  /save <file>       Export conversation to file");
    println!("  /load <file>       Load conversation from file");
    println!("  /plan <goal>       Create a plan for a goal");
    println!("  /provider <name>   Switch provider (not yet supported)");
    println!("  /model <name>      Switch model (not yet supported)");
    println!("  /quit              Exit interactive mode");
    println!();
    println!("  \x1b[33mShortcuts:\x1b[0m");
    println!("  help, h            Show help");
    println!("  clear              Clear screen");
    println!("  exit, q            Exit");
    println!();
    println!("  \x1b[33mCompletion & Hints:\x1b[0m");
    println!("  \x1b[1;33m/\x1b[0m                Popup with command list + descriptions");
    println!("  \x1b[1;34m@\x1b[0m                Popup with file list + icons");
    println!("  Tab               Navigate/open completion menu");
    println!("  \u{2192} (Right Arrow)   Accept inline hint");
    println!("  \u{2022} Type \x1b[1;33m/\x1b[0m \u{2192} popup shows all commands with descriptions");
    println!("  \u{2022} Type \x1b[1;34m@src/\x1b[0m \u{2192} popup shows files in src/");
    println!("  \u{2022} Arrow keys \u{2191}\u{2193} to navigate, Enter to select");
    println!();
    println!("  \x1b[33mTips:\x1b[0m");
    println!("  \u{2022} Type any text to send as a task to the AI agent");
    println!("  \u{2022} Ctrl+R to search command history");
    println!("  \u{2022} Ctrl+C to cancel, Ctrl+D to exit");
    println!();
}

fn show_stats(stats: &SessionStats, model_info: &ModelInfo) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    println!();
    println!("  \x1b[1m\x1b[36m\u{1f4ca} Session Statistics:\x1b[0m");
    println!();
    println!("  \x1b[2m\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[0m");
    println!();

    println!("  \x1b[1mSession:\x1b[0m");
    println!("    Duration:     \x1b[32m{}\x1b[0m", stats.elapsed_str());
    println!("    Messages:     \x1b[33m{}\x1b[0m", stats.messages_sent());
    println!("    Tool calls:   \x1b[36m{}\x1b[0m", stats.tool_calls());
    println!();

    println!("  \x1b[1mModel:\x1b[0m");
    println!("    Provider:     \x1b[33m{}\x1b[0m", model_info.provider);
    println!("    Model:        \x1b[33m{}\x1b[0m", model_info.model);
    println!("    API Base:     \x1b[2m{}\x1b[0m", model_info.api_base);
    println!();

    let in_tok = stats.total_input_tokens();
    let out_tok = stats.total_output_tokens();
    let total_tok = in_tok + out_tok;

    println!("  \x1b[1mToken Usage:\x1b[0m");

    let bar_width = 30;
    if total_tok > 0 {
        let in_ratio = (in_tok as f32 / total_tok as f32 * bar_width as f32) as usize;
        let out_ratio = bar_width - in_ratio;
        println!(
            "    Input:        \x1b[32m{}\x1b[31m{}\x1b[0m {} tokens",
            "\u{2588}".repeat(in_ratio),
            "\u{2588}".repeat(out_ratio),
            stats.format_tokens(in_tok)
        );
        println!(
            "    Output:       \x1b[31m{}\x1b[0m {} tokens",
            "\u{2588}".repeat(
                bar_width.min(out_tok as usize / (total_tok as usize / bar_width + 1))
            ),
            stats.format_tokens(out_tok)
        );
    } else {
        println!("    Input:        \x1b[2m0\x1b[0m");
        println!("    Output:       \x1b[2m0\x1b[0m");
    }
    println!(
        "    Total:        \x1b[1m{}\x1b[0m tokens",
        stats.format_tokens(total_tok)
    );
    println!();

    println!("  \x1b[1mEnvironment:\x1b[0m");
    println!("    Working dir:  \x1b[2m{}\x1b[0m", cwd);
    println!();

    println!("  \x1b[2m\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[0m");
    println!();
}

fn show_history(conversation: &[ConversationEntry]) {
    println!();
    if conversation.is_empty() {
        println!("  \x1b[33mNo messages in this session yet.\x1b[0m\n");
        return;
    }

    println!(
        "  \x1b[1m\x1b[36m\u{1f4dc} Conversation History ({} messages):\x1b[0m\n",
        conversation.len()
    );
    for (i, entry) in conversation.iter().enumerate() {
        let time = entry.timestamp.format("%H:%M:%S");
        let icon = match entry.role.as_str() {
            "user" => "\x1b[1;34m\u{1f464}\x1b[0m",
            "assistant" => "\x1b[1;32m\u{1f916}\x1b[0m",
            _ => "\x1b[1;33m\u{1f4ac}\x1b[0m",
        };
        let content_preview = if entry.content.len() > 120 {
            format!("{}...", &entry.content[..117])
        } else {
            entry.content.clone()
        };
        println!(
            "  {} \x1b[2m[{}] #{}\x1b[0m {}",
            icon, time, i + 1, content_preview
        );
    }
    println!();
}

fn print_goodbye(stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    println!();
    println!("  \x1b[1m\x1b[36m\u{1f4ca} Session Summary:\x1b[0m");
    println!("  \x1b[2m\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[0m");
    println!(
        "  \u{1f4ac} Messages: {}  \u{2502}  \u{23f1} Duration: {}  \u{2502}  \u{1f4ca} Tokens: {} in / {} out",
        stats.messages_sent(),
        stats.elapsed_str(),
        in_tok,
        out_tok,
    );
    println!("  \x1b[2m\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[0m");
    println!();
    println!("  \x1b[36m\u{1f44b} Goodbye!\x1b[0m\n");
}

fn pad_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("...{}", &s[s.len() - max + 3..])
    } else {
        format!("{}{}", s, " ".repeat(max - s.len()))
    }
}

// ── Save/Load conversation ──────────────────────────────────

fn save_conversation(conversation: &[ConversationEntry], file: &str) {
    let data = serde_json::json!({
        "version": 1,
        "exported_at": chrono::Local::now().to_rfc3339(),
        "message_count": conversation.len(),
        "messages": conversation.iter().map(|e| {
            serde_json::json!({
                "role": e.role,
                "content": e.content,
                "timestamp": e.timestamp.to_rfc3339(),
            })
        }).collect::<Vec<_>>(),
    });

    let content = match serde_json::to_string_pretty(&data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\n  \x1b[31m\u{2717} Failed to serialize: {}\x1b[0m\n", e);
            return;
        }
    };

    match std::fs::write(file, content) {
        Ok(_) => {
            println!(
                "\n  \x1b[32m\u{2713} Conversation saved to: {} ({} messages)\x1b[0m\n",
                file,
                conversation.len()
            );
        }
        Err(e) => {
            eprintln!("\n  \x1b[31m\u{2717} Failed to save: {}\x1b[0m\n", e);
        }
    }
}

fn load_conversation(file: &str) -> Result<Vec<ConversationEntry>> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

    let data: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| anyhow::anyhow!("Invalid JSON: {}", e))?;

    let messages = data
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| anyhow::anyhow!("No 'messages' array found in file"))?;

    let mut entries = Vec::new();
    for msg in messages {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown")
            .to_string();
        let content = msg
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let timestamp = msg
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.with_timezone(&chrono::Local))
            .unwrap_or_else(chrono::Local::now);

        entries.push(ConversationEntry {
            role,
            content,
            timestamp,
        });
    }

    Ok(entries)
}
