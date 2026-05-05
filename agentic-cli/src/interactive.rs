use anyhow::Result;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Config, Context, Editor, Helper};
use std::borrow::Cow::{self, Borrowed, Owned};
use std::time::Instant;

use crate::commands::Commands;

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
    "/quit",
];

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
        // If line starts with /, complete slash commands
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

        // Otherwise try filename completion
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
    print_banner();

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

    // Persist history across sessions
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("agentic")
        .join("history.txt");

    if history_path.exists() {
        let _ = rl.load_history(&history_path);
    }

    let mut conversation: Vec<ConversationEntry> = Vec::new();
    let mut session_start = Instant::now();

    loop {
        let readline = rl.readline("agentic> ");

        match readline {
            Ok(line) => {
                let input = line.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                // Add to history
                let _ = rl.add_history_entry(&input);

                // Handle slash commands
                if input.starts_with('/') {
                    if let Some(action) = handle_slash_command(&input) {
                        match action {
                            ReplAction::Quit => break,
                            ReplAction::Clear => {
                                print!("\x1b[2J\x1b[H");
                                std::io::Write::flush(&mut std::io::stdout())?;
                            }
                            ReplAction::ClearHistory => {
                                conversation.clear();
                                session_start = Instant::now();
                                commands.clear_memory();
                                println!("\n  \x1b[32m✓ Conversation cleared.\x1b[0m\n");
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
                            ReplAction::Save(file) => {
                                save_conversation(&conversation, &file, session_start);
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
                                let start = Instant::now();
                                if let Err(e) = commands.run(&format!("Create a plan for: {}", goal)).await {
                                    eprintln!("\n  \x1b[31m✗ Error: {}\x1b[0m\n", e);
                                } else {
                                    let elapsed = start.elapsed();
                                    conversation.push(ConversationEntry {
                                        role: "assistant".into(),
                                        content: format!("(plan created in {:.1}s)", elapsed.as_secs_f64()),
                                        timestamp: chrono::Local::now(),
                                    });
                                    print_timing(elapsed.as_millis());
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
                        std::io::Write::flush(&mut std::io::stdout())?;
                    }
                    _ => {
                        conversation.push(ConversationEntry {
                            role: "user".into(),
                            content: input.clone(),
                            timestamp: chrono::Local::now(),
                        });

                        let start = Instant::now();
                        if let Err(e) = commands.run(&input).await {
                            eprintln!("\n  \x1b[31m✗ Error: {}\x1b[0m\n", e);
                        } else {
                            let elapsed = start.elapsed();
                            conversation.push(ConversationEntry {
                                role: "assistant".into(),
                                content: format!("(response in {:.1}s)", elapsed.as_secs_f64()),
                                timestamp: chrono::Local::now(),
                            });
                            print_timing(elapsed.as_millis());
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

    println!(
        "\n  \x1b[36m👋 Goodbye! Session had {} message(s).\x1b[0m\n",
        conversation.len()
    );
    Ok(())
}

// ── REPL actions ────────────────────────────────────────────

enum ReplAction {
    Quit,
    Clear,
    ClearHistory,
    Config,
    History,
    Tools,
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

fn print_banner() {
    println!();
    println!("  \x1b[1m\x1b[36m╔══════════════════════════════════════════╗\x1b[0m");
    println!("  \x1b[1m\x1b[36m║        🤖 Agentic Interactive Mode       ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m╠══════════════════════════════════════════╣\x1b[0m");
    println!("  \x1b[1m\x1b[36m║  /help    Show commands                  ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║  /tools   List available tools            ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║  /config  Show configuration              ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m║  /quit    Exit (Ctrl+D)                   ║\x1b[0m");
    println!("  \x1b[1m\x1b[36m╚══════════════════════════════════════════╝\x1b[0m");
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

fn print_timing(ms: u128) {
    println!();
    println!(
        "  \x1b[2m📊 Completed in {}.{:03}s\x1b[0m",
        ms / 1000,
        ms % 1000
    );
    println!();
}

// ── Save/Load conversation ──────────────────────────────────

fn save_conversation(
    conversation: &[ConversationEntry],
    file: &str,
    session_start: Instant,
) {
    let data = serde_json::json!({
        "version": 1,
        "exported_at": chrono::Local::now().to_rfc3339(),
        "session_duration_secs": session_start.elapsed().as_secs(),
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
