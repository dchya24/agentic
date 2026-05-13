use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Config, Context, Editor, Helper};
use std::borrow::Cow::{self, Borrowed, Owned};
use std::io::Write;
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

    fn increment_tool_calls(&self) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn add_input_tokens(&self, n: u32) {
        self.total_input_tokens.fetch_add(n, Ordering::Relaxed);
    }

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

// ── Slash command definitions ───────────────────────────────

const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/clear",
    "/config",
    "/history",
    "/tools",
    "/model",
    "/provider",
    "/save",
    "/load",
    "/mcp",
    "/plan",
    "/stats",
    "/quit",
];

// ── Spinner frames ──────────────────────────────────────────

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── Progress bar helper ─────────────────────────────────────

/// Run a background spinner animation that updates the progress bar.
/// Returns a handle that stops the spinner when dropped.
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

// ── REPL Helper (completer + highlighter + hinter) ──────────

#[derive(Helper)]
struct ReplHelper {
    file_completer: FilenameCompleter,
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        if line.starts_with('/') {
            let slash_cmds: Vec<Pair> = SLASH_COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(line))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect();
            if !slash_cmds.is_empty() {
                return Ok((0, slash_cmds));
            }
        }

        self.file_completer.complete(line, pos, ctx)
    }
}

impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Owned(format!("\x1b[1;36m{}\x1b[0m", prompt))
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned(format!("\x1b[2m{}\x1b[0m", hint))
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.starts_with('/') {
            Owned(format!("\x1b[1;33m{}\x1b[0m", line))
        } else {
            Borrowed(line)
        }
    }

    fn highlight_char(&self, line: &str, _pos: usize, _forced: bool) -> bool {
        !line.is_empty()
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        if line.starts_with('/') {
            let matches: Vec<&&str> = SLASH_COMMANDS
                .iter()
                .filter(|cmd| **cmd != line && cmd.starts_with(line))
                .collect();
            if matches.len() == 1 {
                let remainder = matches[0][line.len()..].to_string();
                if !remainder.is_empty() {
                    return Some(remainder);
                }
            }
        }
        None
    }
}

impl Validator for ReplHelper {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        Ok(ValidationResult::Valid(None))
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

    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .edit_mode(rustyline::EditMode::Emacs)
        .build();

    let helper = ReplHelper {
        file_completer: FilenameCompleter::new(),
    };

    let mut rl = Editor::with_history(config, DefaultHistory::new())?;
    rl.set_helper(Some(helper));

    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("agentic")
        .join("history.txt");

    if history_path.exists() {
        let _ = rl.load_history(&history_path);
    }

    let mut conversation: Vec<ConversationEntry> = Vec::new();

    loop {
        // Build dynamic prompt with cwd
        let prompt = build_prompt();
        let readline = rl.readline(&prompt);

        match readline {
            Ok(line) => {
                let input = line.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&input);

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
                                println!("\n  \x1b[32m✓ Conversation cleared.\x1b[0m\n");
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
                                    println!("\n  \x1b[32m✓ Conversation loaded from: {}\x1b[0m\n", file);
                                }
                            }
                            ReplAction::Provider(name) => {
                                println!("\n  \x1b[33m⚠ Provider switching not yet supported in REPL.\x1b[0m");
                                println!("  Use: agentic config edit to change providers.\n");
                                let _ = &name;
                            }
                            ReplAction::Model(name) => {
                                println!("\n  \x1b[33m⚠ Model switching not yet supported in REPL.\x1b[0m");
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
                                if let Err(e) = commands.run(&format!("Create a plan for: {}", goal)).await {
                                    pb.finish_and_clear();
                                    eprintln!("\n  \x1b[31m✗ Error: {}\x1b[0m\n", e);
                                } else {
                                    pb.finish_and_clear();
                                    let elapsed = start.elapsed();
                                    conversation.push(ConversationEntry {
                                        role: "assistant".into(),
                                        content: format!("(plan created in {:.1}s)", elapsed.as_secs_f64()),
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
                            eprintln!("\n  \x1b[31m✗ Error: {}\x1b[0m\n", e);
                        } else {
                            pb.finish_and_clear();
                            let elapsed = start.elapsed();

                            // Estimate tokens (rough: ~4 chars per token)
                            let estimated_input = (input.len() as f32 / 4.0) as u32;
                            stats.add_input_tokens(estimated_input);

                            conversation.push(ConversationEntry {
                                role: "assistant".into(),
                                content: format!("(response in {:.1}s)", elapsed.as_secs_f64()),
                                timestamp: chrono::Local::now(),
                            });
                            print_response_summary(&stats, elapsed.as_millis());
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("\n  \x1b[33mUse /quit or Ctrl+D to exit.\x1b[0m\n");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    // Save history on exit
    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = rl.save_history(&history_path);

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
    ModelInfo { provider, model, api_base }
}

// ── Dynamic prompt builder ──────────────────────────────────

fn build_prompt() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let dir_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");
    format!("\x1b[2m{}\x1b[0m \x1b[1;36magentic>\x1b[0m ", dir_name)
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
        "/clear" => Some(ReplAction::Clear),
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
    println!("  \x1b[1m\x1b[36m╔══════════════════════════════════════════════════════╗\x1b[0m");
    println!("  \x1b[1m\x1b[36m║            🤖 Agentic Interactive Mode               ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m╠══════════════════════════════════════════════════════╣\x1b[0m");
    println!("  \x1b[1m\x1b[36m║\x1b[0m  \x1b[2m📂 {}\x1b[0m", pad_str(&cwd, 52));
    println!("  \x1b[1m\x1b[36m║\x1b[0m  \x1b[33m⚡ {} \x1b[2m/ {}\x1b[0m", pad_str(&format!("Provider: {}", model_info.provider), 25), pad_str(&format!("Model: {}", model_info.model), 25));
    println!("  \x1b[1m\x1b[36m╠══════════════════════════════════════════════════════╣\x1b[0m");
    println!("  \x1b[1m\x1b[36m║\x1b[0m  /help    Show commands                              \x1b[1m\x1b[36m║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║\x1b[0m  /tools   List available tools                        \x1b[1m\x1b[36m║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║\x1b[0m  /stats   Show session statistics                     \x1b[1m\x1b[36m║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║\x1b[0m  /quit    Exit (Ctrl+D)                               \x1b[1m\x1b[36m║\x1b[0m");
    println!("  \x1b[1m\x1b[36m╚══════════════════════════════════════════════════════╝\x1b[0m");
    println!();

    print_status_bar(model_info, stats);
}

fn print_status_bar(model_info: &ModelInfo, stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    println!(
        "  \x1b[2m┌─ \x1b[33m⚡ {} {}\x1b[2m ─── \x1b[36m💬 {} msgs\x1b[2m ─── \x1b[35m📊 tokens: {} in / {} out\x1b[2m ─── \x1b[32m⏱ {}\x1b[2m ─┐\x1b[0m",
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
        "  \x1b[2m┌─ \x1b[32m✓ Done\x1b[2m ─── ⏱ {}.{:03}s ─── 💬 {} msgs ─── 📊 {} in / {} out ─── ⏱ session: {} ─┐\x1b[0m",
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
    println!("  \x1b[1m\x1b[36m📖 Commands:\x1b[0m");
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
    println!("  \x1b[33mTips:\x1b[0m");
    println!("  • Type any text to send as a task to the AI agent");
    println!("  • Ctrl+R to search command history");
    println!("  • Tab to auto-complete commands and file paths");
    println!("  • Ctrl+C to cancel, Ctrl+D to exit");
    println!();
}

fn show_stats(stats: &SessionStats, model_info: &ModelInfo) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    println!();
    println!("  \x1b[1m\x1b[36m📊 Session Statistics:\x1b[0m");
    println!();
    println!("  \x1b[2m────────────────────────────────────────────────\x1b[0m");
    println!();

    // Session
    println!("  \x1b[1mSession:\x1b[0m");
    println!("    Duration:     \x1b[32m{}\x1b[0m", stats.elapsed_str());
    println!("    Messages:     \x1b[33m{}\x1b[0m", stats.messages_sent());
    println!("    Tool calls:   \x1b[36m{}\x1b[0m", stats.tool_calls());
    println!();

    // Model
    println!("  \x1b[1mModel:\x1b[0m");
    println!("    Provider:     \x1b[33m{}\x1b[0m", model_info.provider);
    println!("    Model:        \x1b[33m{}\x1b[0m", model_info.model);
    println!("    API Base:     \x1b[2m{}\x1b[0m", model_info.api_base);
    println!();

    // Token usage
    let in_tok = stats.total_input_tokens();
    let out_tok = stats.total_output_tokens();
    let total_tok = in_tok + out_tok;

    println!("  \x1b[1mToken Usage:\x1b[0m");

    // Mini progress bar showing in vs out ratio
    let bar_width = 30;
    if total_tok > 0 {
        let in_ratio = (in_tok as f32 / total_tok as f32 * bar_width as f32) as usize;
        let out_ratio = bar_width - in_ratio;
        println!(
            "    Input:        \x1b[32m{}\x1b[31m{}\x1b[0m {} tokens",
            "█".repeat(in_ratio),
            "█".repeat(out_ratio),
            stats.format_tokens(in_tok)
        );
        println!(
            "    Output:       \x1b[31m{}\x1b[0m {} tokens",
            "█".repeat(bar_width.min(out_tok as usize / (total_tok as usize / bar_width + 1))),
            stats.format_tokens(out_tok)
        );
    } else {
        println!("    Input:        \x1b[2m0\x1b[0m");
        println!("    Output:       \x1b[2m0\x1b[0m");
    }
    println!("    Total:        \x1b[1m{}\x1b[0m tokens", stats.format_tokens(total_tok));
    println!();

    // Directory
    println!("  \x1b[1mEnvironment:\x1b[0m");
    println!("    Working dir:  \x1b[2m{}\x1b[0m", cwd);
    println!();

    println!("  \x1b[2m────────────────────────────────────────────────\x1b[0m");
    println!();
}

fn show_history(conversation: &[ConversationEntry]) {
    println!();
    if conversation.is_empty() {
        println!("  \x1b[33mNo messages in this session yet.\x1b[0m\n");
        return;
    }

    println!(
        "  \x1b[1m\x1b[36m📜 Conversation History ({} messages):\x1b[0m\n",
        conversation.len()
    );
    for (i, entry) in conversation.iter().enumerate() {
        let time = entry.timestamp.format("%H:%M:%S");
        let icon = match entry.role.as_str() {
            "user" => "\x1b[1;34m👤\x1b[0m",
            "assistant" => "\x1b[1;32m🤖\x1b[0m",
            _ => "\x1b[1;33m💬\x1b[0m",
        };
        let content_preview = if entry.content.len() > 120 {
            format!("{}...", &entry.content[..117])
        } else {
            entry.content.clone()
        };
        println!("  {} \x1b[2m[{}] #{}\x1b[0m {}", icon, time, i + 1, content_preview);
    }
    println!();
}

fn print_goodbye(stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    println!();
    println!("  \x1b[1m\x1b[36m📊 Session Summary:\x1b[0m");
    println!("  \x1b[2m──────────────────────────────────────\x1b[0m");
    println!(
        "  💬 Messages: {}  │  ⏱ Duration: {}  │  📊 Tokens: {} in / {} out",
        stats.messages_sent(),
        stats.elapsed_str(),
        in_tok,
        out_tok,
    );
    println!("  \x1b[2m──────────────────────────────────────\x1b[0m");
    println!();
    println!("  \x1b[36m👋 Goodbye!\x1b[0m\n");
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
            eprintln!("\n  \x1b[31m✗ Failed to serialize: {}\x1b[0m\n", e);
            return;
        }
    };

    match std::fs::write(file, content) {
        Ok(_) => {
            println!(
                "\n  \x1b[32m✓ Conversation saved to: {} ({} messages)\x1b[0m\n",
                file,
                conversation.len()
            );
        }
        Err(e) => {
            eprintln!("\n  \x1b[31m✗ Failed to save: {}\x1b[0m\n", e);
        }
    }
}

fn load_conversation(file: &str) -> Result<Vec<ConversationEntry>> {
    let content =
        std::fs::read_to_string(file).map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

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
