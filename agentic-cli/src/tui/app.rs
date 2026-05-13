//! Main TUI application state and event loop

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;

use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::commands::Commands;
use super::dropdown::{Dropdown, DropdownType};
use super::progress::ProgressState;
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
}

/// Application state
pub struct App {
    /// Input buffer
    pub input: String,
    /// Cursor position in input
    pub cursor_pos: usize,
    /// Output/conversation history
    pub messages: Vec<Message>,
    /// Current streaming response (being built)
    pub current_response: String,
    /// Is currently processing a task
    pub is_loading: bool,
    /// Progress state for animations
    pub progress: ProgressState,
    /// Dropdown state
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
    /// Input history
    pub history: Vec<String>,
    /// History index (-1 = current input)
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

    /// Insert character at cursor position
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_dropdown();
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            let prev_char_boundary = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev_char_boundary);
            self.cursor_pos = prev_char_boundary;
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

    /// Update dropdown based on current input
    fn update_dropdown(&mut self) {
        // Check for `/` command trigger
        if self.input.starts_with('/') {
            let query = &self.input[1..];
            self.dropdown = Some(Dropdown::new(DropdownType::Command, query.to_string()));
        }
        // Check for `@` file trigger
        else if let Some(at_pos) = self.find_at_trigger() {
            let query = &self.input[at_pos + 1..self.cursor_pos];
            self.dropdown = Some(Dropdown::new(DropdownType::File, query.to_string()));
        } else {
            self.dropdown = None;
        }
    }

    /// Find the position of `@` trigger for file completion
    fn find_at_trigger(&self) -> Option<usize> {
        let before_cursor = &self.input[..self.cursor_pos];
        // Find last `@` that's either at start or after whitespace
        for (i, c) in before_cursor.char_indices().rev() {
            if c == '@' {
                if i == 0 || before_cursor[..i].ends_with(char::is_whitespace) {
                    return Some(i);
                }
            }
            if c.is_whitespace() {
                break;
            }
        }
        None
    }

    /// Accept selected dropdown item
    pub fn accept_dropdown(&mut self) {
        if let Some(dropdown) = &self.dropdown {
            if let Some(selected) = dropdown.selected_item() {
                match dropdown.dropdown_type {
                    DropdownType::Command => {
                        self.input = format!("/{}", selected);
                        self.cursor_pos = self.input.len();
                    }
                    DropdownType::File => {
                        if let Some(at_pos) = self.find_at_trigger() {
                            self.input = format!(
                                "{}@{}{}",
                                &self.input[..at_pos],
                                selected,
                                &self.input[self.cursor_pos..]
                            );
                            self.cursor_pos = at_pos + 1 + selected.len();
                        }
                    }
                }
            }
        }
        self.dropdown = None;
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

    /// Navigate history up
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
        }
    }

    /// Navigate history down
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
    }

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

    /// Submit current input
    pub async fn submit(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        // Add to history
        if !input.is_empty() && (self.history.is_empty() || self.history.last() != Some(&input)) {
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
                
                match commands.run_with_callback(&task, |chunk| {
                    let _ = tx.send(AppMessage::StreamChunk(chunk.to_string()));
                }).await {
                    Ok(result) => {
                        let _ = tx.send(AppMessage::TaskComplete(result));
                    }
                    Err(e) => {
                        let _ = tx.send(AppMessage::Error(e.to_string()));
                    }
                }
                
                // Return commands back (we'd need a different approach for real impl)
                commands
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

| Command | Description |
|---------|-------------|
| `/help` | Show this help |
| `/clear` | Clear conversation |
| `/config` | Show configuration |
| `/tools` | List available tools |
| `/history` | Show message history |
| `/quit` | Exit TUI |

**Tips:**
- Type `/` to see command dropdown
- Type `@` to browse files
- Use ↑/↓ to navigate history
- Use PageUp/PageDown to scroll
- Press Ctrl+C to cancel"#.into(),
                    timestamp: chrono::Local::now(),
                });
            }
            "/clear" => {
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
            _ => {
                self.messages.push(Message {
                    role: MessageRole::Error,
                    content: format!("Unknown command: `{}`\nType `/help` for available commands.", cmd),
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
                // Handle dropdown navigation first
                if app.dropdown.is_some() {
                    match key.code {
                        KeyCode::Up => {
                            app.dropdown_up();
                            continue;
                        }
                        KeyCode::Down => {
                            app.dropdown_down();
                            continue;
                        }
                        KeyCode::Tab | KeyCode::Enter => {
                            app.accept_dropdown();
                            continue;
                        }
                        KeyCode::Esc => {
                            app.close_dropdown();
                            continue;
                        }
                        _ => {}
                    }
                }

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
                    KeyCode::Left => app.move_cursor_left(),
                    KeyCode::Right => app.move_cursor_right(),
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
