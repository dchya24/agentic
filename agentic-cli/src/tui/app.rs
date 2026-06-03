//! Main TUI application state and event loop
//!
//! Handles:
//! - Input editing with cursor support
//! - `@` file search dropdown (type @ anywhere to search files)
//! - `/` command dropdown (type / at start to see commands)
//! - Arrow key navigation for both dropdowns
//! - Tab/Enter to accept, Esc to dismiss
//! - History navigation with ↑/↓

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::commands::Commands;
use crate::widgets::progress::ProgressState;
use super::dropdown::{Dropdown, DropdownType};
use super::ui;

/// Messages from async tasks to the UI
pub enum AppMessage {
    /// Streaming chunk from LLM
    StreamChunk(String),
    /// Task completed
    TaskComplete(String),
    /// Error occurred
    Error(String),
    /// Progress update
    Progress(String),
    /// Agent invoked a tool. Renders as a yellow panel before the result.
    ToolCall {
        name: String,
        arguments: serde_json::Value,
    },
    /// Agent received a tool result. Renders as a green/red notification
    /// + optional unified-diff body when the tool is edit_file/write_file.
    ToolResult {
        name: String,
        output: serde_json::Value,
        is_error: bool,
    },
    /// Plan progress update from the planner agent.
    PlanProgress {
        goal: String,
        current_step: String,
        step_status: String,
        total: usize,
        completed: usize,
        failed: usize,
        pending: usize,
    },
}

/// Application state
pub struct App {
    /// Input buffer
    pub input: String,
    /// Cursor position (byte offset) in input
    pub cursor_pos: usize,
    /// Output/conversation history
    pub messages: Vec<Message>,
    /// Current streaming response (being built)
    pub current_response: String,
    /// Is currently processing a task
    pub is_loading: bool,
    /// Progress state for animations
    pub progress: ProgressState,
    /// Dropdown state (None when hidden)
    pub dropdown: Option<Dropdown>,
    /// Scroll offset for messages
    pub scroll_offset: usize,
    /// Should quit
    pub should_quit: bool,
    /// Commands handler
    commands: Option<Commands>,
    /// Message channel receiver
    rx: Option<mpsc::UnboundedReceiver<AppMessage>>,
    /// Message channel sender (for async tasks)
    tx: mpsc::UnboundedSender<AppMessage>,
    /// Last tick for animations
    last_tick: Instant,
    /// Input history (submitted inputs)
    pub history: Vec<String>,
    /// History index (-1 = not browsing history)
    pub history_index: i32,
    /// Saved input when browsing history
    pub saved_input: String,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Error,
    /// Tool invocation: rendered as a panel with the tool name and args.
    Tool,
    /// Tool result (success path): rendered as a green notification with
    /// optional unified-diff body for edit_file/write_file outputs.
    ToolResult,
    /// Tool result (error / blocked / skipped): rendered as a red
    /// notification with the body always shown.
    ToolError,
}

impl App {
    pub fn new(commands: Commands) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            input: String::new(),
            cursor_pos: 0,
            messages: vec![Message {
                role: MessageRole::System,
                content: "Welcome to Agentic TUI! Type a message or use /help for commands.".into(),
                timestamp: chrono::Local::now(),
            }],
            current_response: String::new(),
            is_loading: false,
            progress: ProgressState::new(),
            dropdown: None,
            scroll_offset: 0,
            should_quit: false,
            commands: Some(commands),
            rx: Some(rx),
            tx,
            last_tick: Instant::now(),
            history: Vec::new(),
            history_index: -1,
            saved_input: String::new(),
        }
    }

    // ── Input editing ────────────────────────────────────────

    /// Insert character at cursor position and update dropdown
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_dropdown();
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev);
            self.cursor_pos = prev;
            self.update_dropdown();
        }
    }

    /// Delete character at cursor (Delete key)
    #[allow(dead_code)]
    pub fn delete_char_forward(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
            // cursor_pos stays the same
            self.update_dropdown();
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Move cursor to start
    pub fn move_cursor_start(&mut self) {
        self.cursor_pos = 0;
    }

    /// Move cursor to end
    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.input.len();
    }

    // ── Dropdown: @ file search & / commands ────────────────

    /// Update dropdown state based on current input and cursor position.
    ///
    /// Rules:
    /// - If input starts with `/` and cursor is in the command part → command dropdown
    /// - If cursor is right after or within an `@...` sequence → file dropdown
    /// - Otherwise → no dropdown
    fn update_dropdown(&mut self) {
        // 1) Check for `/` command trigger
        if self.input.starts_with('/') {
            // Only show command dropdown if cursor is still in the command part
            // (before any space — the command itself)
            let before_cursor = &self.input[..self.cursor_pos];
            if !before_cursor.contains(' ') {
                let query = &self.input[1..self.cursor_pos];
                self.dropdown = Some(Dropdown::new(DropdownType::Command, query.to_string()));
                return;
            }
        }

        // 2) Check for `@` file trigger
        if let Some(at_pos) = self.find_at_trigger() {
            let query = &self.input[at_pos + 1..self.cursor_pos];
            self.dropdown = Some(Dropdown::new(DropdownType::File, query.to_string()));
            return;
        }

        // 3) No trigger found
        self.dropdown = None;
    }

    /// Find the byte position of the `@` trigger that the cursor is currently inside.
    ///
    /// Returns Some(byte_pos) if:
    /// - There's an `@` at the start of input or after whitespace
    /// - There's no whitespace between the `@` and the cursor
    /// - The cursor is at or after the `@`
    fn find_at_trigger(&self) -> Option<usize> {
        let before_cursor = &self.input[..self.cursor_pos];

        // Walk backwards from cursor looking for `@`
        for (i, c) in before_cursor.char_indices().rev() {
            match c {
                '@' => {
                    // `@` must be at start of input or preceded by whitespace
                    let at_start = i == 0;
                    let after_space = i > 0 && self.input[..i].ends_with(char::is_whitespace);
                    if at_start || after_space {
                        // Check no whitespace between @ and cursor
                        let after_at = &before_cursor[i + 1..];
                        if !after_at.contains(char::is_whitespace) {
                            return Some(i);
                        }
                    }
                    // If `@` is not valid trigger position, stop searching
                    // (we hit a non-whitespace char that isn't `@`)
                    return None;
                }
                w if w.is_whitespace() => {
                    // Hit whitespace going backwards — stop
                    return None;
                }
                _ => {
                    // Regular character, keep going back
                    continue;
                }
            }
        }
        None
    }

    /// Accept the currently selected dropdown item and insert it into input.
    pub fn accept_dropdown(&mut self) {
        let selected_text = match &self.dropdown {
            Some(d) => d.selected_item().map(|s| s.to_string()),
            None => None,
        };

        let mut is_dir = false;

        if let Some(text) = selected_text {
            is_dir = text.ends_with('/');
            if let Some(dropdown) = &self.dropdown {
                match dropdown.dropdown_type {
                    DropdownType::Command => {
                        // Replace entire input with /command
                        self.input = format!("/{} ", text);
                        self.cursor_pos = self.input.len();
                    }
                    DropdownType::File => {
                        // Replace from @ to cursor with the selected file path
                        if let Some(at_pos) = self.find_at_trigger() {
                            let before_at = &self.input[..at_pos];
                            let after_cursor = &self.input[self.cursor_pos..];
                            
                            // Add trailing space for files, keep slash for dirs
                            let suffix = if is_dir {
                                // Directory — keep it for further navigation
                                ""
                            } else {
                                // File — add space after
                                " "
                            };
                            
                            self.input = format!("{}@{}{}{}", before_at, text, suffix, after_cursor);
                            self.cursor_pos = at_pos + 1 + text.len() + suffix.len();
                        }
                    }
                }
            }
        }

        // After accepting a file dropdown:
        // - For directories: re-trigger dropdown to show contents of selected dir
        // - For files/commands: close dropdown
        if is_dir {
            self.update_dropdown();
        } else {
            self.dropdown = None;
        }
    }

    /// Move dropdown selection up
    pub fn dropdown_up(&mut self) {
        if let Some(dropdown) = &mut self.dropdown {
            dropdown.select_prev();
        }
    }

    /// Move dropdown selection down
    pub fn dropdown_down(&mut self) {
        if let Some(dropdown) = &mut self.dropdown {
            dropdown.select_next();
        }
    }

    /// Close dropdown
    pub fn close_dropdown(&mut self) {
        self.dropdown = None;
    }

    // ── History navigation ──────────────────────────────────

    /// Navigate history up (older entries)
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index == -1 {
            self.saved_input = self.input.clone();
        }
        if (self.history_index as usize) < self.history.len() - 1 {
            self.history_index += 1;
            self.input = self.history[self.history.len() - 1 - self.history_index as usize].clone();
            self.cursor_pos = self.input.len();
            self.dropdown = None;
        }
    }

    /// Navigate history down (newer entries)
    pub fn history_down(&mut self) {
        if self.history_index > 0 {
            self.history_index -= 1;
            self.input = self.history[self.history.len() - 1 - self.history_index as usize].clone();
            self.cursor_pos = self.input.len();
        } else if self.history_index == 0 {
            self.history_index = -1;
            self.input = self.saved_input.clone();
            self.cursor_pos = self.input.len();
        }
        self.dropdown = None;
    }

    // ── Scroll ──────────────────────────────────────────────

    /// Scroll messages up
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(3);
        }
    }

    /// Scroll messages down
    pub fn scroll_down(&mut self) {
        self.scroll_offset += 3;
    }

    // ── Submit ──────────────────────────────────────────────

    /// Submit current input
    pub async fn submit(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        // Add to history
        if !self.history.is_empty() || self.history.last() != Some(&input) {
            self.history.push(input.clone());
        }
        self.history_index = -1;

        // Clear input
        self.input.clear();
        self.cursor_pos = 0;
        self.dropdown = None;

        // Handle slash commands
        if input.starts_with('/') {
            self.handle_slash_command(&input).await;
            return;
        }

        // Add user message
        self.messages.push(Message {
            role: MessageRole::User,
            content: input.clone(),
            timestamp: chrono::Local::now(),
        });

        // Start loading
        self.is_loading = true;
        self.progress.start();
        self.current_response.clear();

        // Run task in background
        let tx = self.tx.clone();
        let task = input.clone();

        if let Some(mut commands) = self.commands.take() {
            tokio::spawn(async move {
                let _ = tx.send(AppMessage::Progress("Thinking...".into()));

                // Pipe orchestrator events to the same channel so the
                // message log shows tool calls / results inline. Cloning
                // the sender into the closure is cheap (Arc internally).
                let event_tx = tx.clone();

                match commands
                    .run_with_callbacks(
                        &task,
                        |chunk| {
                            let _ = tx.send(AppMessage::StreamChunk(chunk.to_string()));
                        },
                        move |event| match event {
                            core_agentic::Event::ToolCall { tool_name, arguments } => {
                                let _ = event_tx.send(AppMessage::ToolCall {
                                    name: tool_name,
                                    arguments,
                                });
                            }
                            core_agentic::Event::ToolOutput { tool_name, output } => {
                                // Heuristic matching the inline mode: orchestrator
                                // records denied/skipped/errored outcomes as plain
                                // strings with these prefixes. Surface them as errors
                                // so the UI uses the red accent.
                                let body = match &output {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                let is_error = body.starts_with("Tool error")
                                    || body.starts_with("Blocked:")
                                    || body.starts_with("Skipped:");
                                let _ = event_tx.send(AppMessage::ToolResult {
                                    name: tool_name,
                                    output,
                                    is_error,
                                });
                            }
                            core_agentic::Event::Error { message } => {
                                let _ = event_tx.send(AppMessage::Error(message));
                            }
                            // Other event types aren't surfaced in TUI for now.
                            _ => {}
                        },
                    )
                    .await
                {
                    Ok(result) => {
                        let _ = tx.send(AppMessage::TaskComplete(result));
                    }
                    Err(e) => {
                        let _ = tx.send(AppMessage::Error(e.to_string()));
                    }
                }

                // We can't put commands back via channel easily, so we leak it.
                // In a real implementation, use Arc<Mutex<Commands>> or similar.
                let _ = commands;
            });
        }
    }

    /// Handle slash commands
    async fn handle_slash_command(&mut self, input: &str) {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let _arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd {
            "/help" | "/h" => {
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: r#"**Available Commands:**

| Command | Alias | Description |
|---------|-------|-------------|
| `/help` | `/h` | Show this help |
| `/clear` | `/c` | Clear conversation |
| `/config` | `/cfg` | Show configuration |
| `/tools` | `/t` | List available tools |
| `/history` | `/hist` | Show message history |
| `/save` | `/s` | Save conversation |
| `/load` | `/l` | Load conversation |
| `/mcp` | | Show MCP status |
| `/plan` | `/p` | Create a plan |
| `/model` | `/m` | Switch model |
| `/provider` | | Switch provider |
| `/stats` | | Show statistics |
| `/quit` | `/q` | Exit TUI |

**Tips:**
- Type `/` to see command dropdown
- Type `@` anywhere to browse files
- Use ↑/↓ to navigate history
- Use PageUp/PageDown to scroll
- Press Ctrl+C to cancel"#.into(),
                    timestamp: chrono::Local::now(),
                });
            }
            "/clear" | "/c" | "/cls" => {
                self.messages.clear();
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: "Conversation cleared.".into(),
                    timestamp: chrono::Local::now(),
                });
                self.scroll_offset = 0;
            }
            "/quit" | "/q" | "/exit" => {
                self.should_quit = true;
            }
            "/config" | "/cfg" => {
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: "Configuration display coming soon...".into(),
                    timestamp: chrono::Local::now(),
                });
            }
            "/tools" | "/t" => {
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: "Tools list coming soon...".into(),
                    timestamp: chrono::Local::now(),
                });
            }
            "/history" | "/hist" => {
                let history_text = if self.history.is_empty() {
                    "No command history yet.".to_string()
                } else {
                    self.history
                        .iter()
                        .enumerate()
                        .map(|(i, h)| format!("{}. {}", i + 1, h))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: format!("**Command History:**\n\n{}", history_text),
                    timestamp: chrono::Local::now(),
                });
            }
            "/stats" => {
                self.messages.push(Message {
                    role: MessageRole::System,
                    content: format!(
                        "**Session Statistics:**\n\n- Messages: {}\n- History entries: {}",
                        self.messages.len(),
                        self.history.len()
                    ),
                    timestamp: chrono::Local::now(),
                });
            }
            _ => {
                self.messages.push(Message {
                    role: MessageRole::Error,
                    content: format!(
                        "Unknown command: `{}`\nType `/help` for available commands.",
                        cmd
                    ),
                    timestamp: chrono::Local::now(),
                });
            }
        }
    }

    /// Process pending messages from async tasks
    pub fn process_messages(&mut self) {
        if let Some(rx) = &mut self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    AppMessage::StreamChunk(chunk) => {
                        self.current_response.push_str(&chunk);
                    }
                    AppMessage::TaskComplete(result) => {
                        self.is_loading = false;
                        self.progress.stop();

                        let content = if self.current_response.is_empty() {
                            result
                        } else {
                            std::mem::take(&mut self.current_response)
                        };

                        self.messages.push(Message {
                            role: MessageRole::Assistant,
                            content,
                            timestamp: chrono::Local::now(),
                        });
                    }
                    AppMessage::Error(err) => {
                        self.is_loading = false;
                        self.progress.stop();
                        self.current_response.clear();

                        self.messages.push(Message {
                            role: MessageRole::Error,
                            content: format!("Error: {}", err),
                            timestamp: chrono::Local::now(),
                        });
                    }
                    AppMessage::Progress(msg) => {
                        self.progress.set_message(msg);
                    }
                    AppMessage::ToolCall { name, arguments } => {
                        // Stash the structured payload as JSON in the
                        // message content so the renderer can decode it.
                        // We use a sentinel envelope so render code can
                        // tell tool messages apart by the role enum.
                        let payload = serde_json::json!({
                            "name": name,
                            "arguments": arguments,
                        });
                        self.messages.push(Message {
                            role: MessageRole::Tool,
                            content: payload.to_string(),
                            timestamp: chrono::Local::now(),
                        });
                    }
                    AppMessage::ToolResult { name, output, is_error } => {
                        let payload = serde_json::json!({
                            "name": name,
                            "output": output,
                        });
                        self.messages.push(Message {
                            role: if is_error {
                                MessageRole::ToolError
                            } else {
                                MessageRole::ToolResult
                            },
                            content: payload.to_string(),
                            timestamp: chrono::Local::now(),
                        });
                    }
                    AppMessage::PlanProgress {
                        goal: _,
                        current_step,
                        step_status,
                        total: _,
                        completed: _,
                        failed: _,
                        pending: _,
                    } => {
                        self.progress.set_message(format!(
                            "Plan: {} — {}",
                            current_step, step_status,
                        ));
                    }
                }
            }
        }
    }

    /// Tick for animations
    pub fn tick(&mut self) {
        if self.last_tick.elapsed() >= Duration::from_millis(100) {
            self.progress.tick();
            self.last_tick = Instant::now();
        }
    }
}

// ── Main TUI event loop ──────────────────────────────────────

/// Run the TUI application
pub async fn run_tui(commands: Commands) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(commands);

    // Main loop
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw(f, &mut app))?;

        // Handle events
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(&mut app, key).await;
            }
        }

        // Tick for animations
        if last_tick.elapsed() >= tick_rate {
            app.tick();
            app.process_messages();
            last_tick = Instant::now();
        }

        // Check quit
        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Handle a single key event
async fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    // ── Dropdown-specific key handling ──
    // When a dropdown is open, intercept navigation keys
    if app.dropdown.is_some() {
        match key.code {
            KeyCode::Up => {
                app.dropdown_up();
                return;
            }
            KeyCode::Down => {
                app.dropdown_down();
                return;
            }
            KeyCode::Tab | KeyCode::Enter => {
                app.accept_dropdown();
                return;
            }
            KeyCode::Esc => {
                app.close_dropdown();
                return;
            }
            // All other keys fall through to normal input handling
            // so typing continues to filter the dropdown
            _ => {}
        }
    }

    // ── Normal key handling ──
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.is_loading {
                app.is_loading = false;
                app.progress.stop();
                app.messages.push(Message {
                    role: MessageRole::System,
                    content: "Task cancelled.".into(),
                    timestamp: chrono::Local::now(),
                });
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Enter => {
            if !app.is_loading {
                app.submit().await;
            }
        }
        KeyCode::Char(c) => {
            if !app.is_loading {
                app.insert_char(c);
            }
        }
        KeyCode::Backspace => {
            if !app.is_loading {
                app.delete_char();
            }
        }
        KeyCode::Delete => {
            if !app.is_loading {
                app.delete_char_forward();
            }
        }
        KeyCode::Left => {
            // If Shift is held, scroll instead of moving cursor
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.scroll_up();
            } else {
                app.move_cursor_left();
            }
        }
        KeyCode::Right => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.scroll_down();
            } else {
                app.move_cursor_right();
            }
        }
        KeyCode::Home => app.move_cursor_start(),
        KeyCode::End => app.move_cursor_end(),
        KeyCode::Up => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.scroll_up();
            } else {
                app.history_up();
            }
        }
        KeyCode::Down => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.scroll_down();
            } else {
                app.history_down();
            }
        }
        KeyCode::PageUp => app.scroll_up(),
        KeyCode::PageDown => app.scroll_down(),
        KeyCode::Esc => {
            if app.dropdown.is_some() {
                app.close_dropdown();
            }
        }
        _ => {}
    }
}
