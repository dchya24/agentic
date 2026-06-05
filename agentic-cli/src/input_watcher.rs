//! Input watcher for ESC abort and message queuing during agent processing.
//!
//! When the agent is running, this module:
//! - Detects ESC key presses to abort the current operation
//! - Buffers typed characters and queues complete messages on Enter
//! - Shares the live input buffer so the spinner can render it
//!
//! **This module never writes to the terminal.** All rendering is done by
//! the spinner ticker in `commands.rs`, which reads the shared buffer and
//! draws a two-line transient area (spinner + styled input line).
//!
//! Uses crossterm raw mode for individual key detection, with ONLCR
//! output processing preserved so `println!()` / `\n` still works.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Events produced by the input watcher.
#[derive(Debug)]
pub enum WatcherEvent {
    /// User pressed ESC — agent should be cancelled.
    Abort,
    /// User typed and submitted a message (Enter pressed).
    QueuedMessage(String),
}

/// Shared state between the input watcher thread and the spinner ticker.
pub struct WatcherState {
    /// Current input buffer content (what the user has typed so far).
    pub buffer: String,
    /// Hint shown after queuing (e.g. "✓ Queued: ...").
    pub hint: String,
    /// Pre-rendered left prompt (ANSI-styled directory name + ">").
    pub prompt_left: String,
    /// Pre-rendered right prompt (provider + model + branch).
    pub prompt_right: String,
}

/// Spawns a background thread that watches for ESC and collects queued
/// messages while the agent is processing.
///
/// # Terminal state
///
/// On creation, enters crossterm raw mode but re-enables `OPOST | ONLCR`
/// so that `\n` is still translated to `\r\n` on output.
///
/// On drop or `stop()`, the original terminal state is restored.
pub struct InputWatcher {
    events: Arc<Mutex<Vec<WatcherEvent>>>,
    state: Arc<Mutex<WatcherState>>,
    done: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl InputWatcher {
    /// Start watching. Call `stop()` when the agent finishes.
    pub fn start(
        cancel_flag: Arc<std::sync::atomic::AtomicBool>,
        prompt_left: String,
        prompt_right: String,
    ) -> Self {
        let events: Arc<Mutex<Vec<WatcherEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let state: Arc<Mutex<WatcherState>> = Arc::new(Mutex::new(WatcherState {
            buffer: String::new(),
            hint: String::new(),
            prompt_left,
            prompt_right,
        }));
        let done = Arc::new(AtomicBool::new(false));

        let events_clone = events.clone();
        let state_clone = state.clone();
        let done_clone = done.clone();

        let handle = std::thread::Builder::new()
            .name("agentic-input-watcher".into())
            .spawn(move || {
                run_watcher(&done_clone, &events_clone, &state_clone, &cancel_flag);
            })
            .expect("Failed to spawn input watcher thread");

        Self {
            events,
            state,
            done,
            thread_handle: Some(handle),
        }
    }

    /// Read-only access to the shared input buffer state.
    /// Called by the spinner ticker to render the input line.
    pub fn state(&self) -> &Arc<Mutex<WatcherState>> {
        &self.state
    }

    /// Signal the watcher to stop and wait for the thread to finish.
    /// Returns any events collected since the last call.
    pub fn stop(&mut self) -> Vec<WatcherEvent> {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        std::mem::take(&mut self.events.lock().unwrap())
    }

    /// Drain collected events without stopping the watcher.
    #[allow(dead_code)]
    pub fn drain_events(&self) -> Vec<WatcherEvent> {
        std::mem::take(&mut self.events.lock().unwrap())
    }
}

impl Drop for InputWatcher {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

// ── Implementation ──────────────────────────────────────────

fn run_watcher(
    done: &AtomicBool,
    events: &Mutex<Vec<WatcherEvent>>,
    state: &Mutex<WatcherState>,
    cancel_flag: &std::sync::atomic::AtomicBool,
) {
    // Enter crossterm raw mode (disables ICANON, ECHO, etc.).
    if crossterm::terminal::enable_raw_mode().is_err() {
        while !done.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return;
    }

    // Re-enable OPOST | ONLCR so println!() \n → \r\n still works.
    #[cfg(unix)]
    {
        let stdout = std::io::stdout();
        let fd = stdout.as_raw_fd();
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut termios) } == 0 {
            termios.c_oflag |= libc::OPOST | libc::ONLCR;
            unsafe {
                libc::tcsetattr(fd, libc::TCSANOW, &termios);
            }
        }
    }

    while !done.load(Ordering::Relaxed) {
        let has_event = match crossterm::event::poll(std::time::Duration::from_millis(50)) {
            Ok(b) => b,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
        };

        if !has_event {
            continue;
        }

        let event = match crossterm::event::read() {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            crossterm::event::Event::Key(key) => {
                match key.code {
                    crossterm::event::KeyCode::Esc => {
                        cancel_flag.store(true, Ordering::SeqCst);
                        {
                            let mut s = state.lock().unwrap();
                            s.buffer.clear();
                            s.hint = "⚠ Cancelled".to_string();
                        }
                        events.lock().unwrap().push(WatcherEvent::Abort);
                        // Small delay so spinner can render the cancel state.
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        break;
                    }
                    crossterm::event::KeyCode::Enter => {
                        let line = {
                            let mut s = state.lock().unwrap();
                            let text = s.buffer.trim().to_string();
                            s.buffer.clear();
                            text
                        };
                        if !line.is_empty() {
                            let preview = if line.len() > 50 {
                                format!("{}...", &line[..47])
                            } else {
                                line.clone()
                            };
                            state.lock().unwrap().hint =
                                format!("✓ Queued: {}", preview);
                            events
                                .lock()
                                .unwrap()
                                .push(WatcherEvent::QueuedMessage(line));
                        }
                    }
                    crossterm::event::KeyCode::Char(c) => {
                        // Ctrl+C — abort.
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                            && c == 'c'
                        {
                            cancel_flag.store(true, Ordering::SeqCst);
                            {
                                let mut s = state.lock().unwrap();
                                s.buffer.clear();
                                s.hint = "⚠ Cancelled".to_string();
                            }
                            events.lock().unwrap().push(WatcherEvent::Abort);
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            break;
                        }
                        // Ctrl+U — clear the input buffer.
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                            && c == 'u'
                        {
                            state.lock().unwrap().buffer.clear();
                            continue;
                        }
                        // Normal character — just update buffer, spinner renders it.
                        state.lock().unwrap().buffer.push(c);
                    }
                    crossterm::event::KeyCode::Backspace
                    | crossterm::event::KeyCode::Delete => {
                        state.lock().unwrap().buffer.pop();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Queue any remaining buffer content.
    let remaining = state.lock().unwrap().buffer.trim().to_string();
    if !remaining.is_empty() {
        events
            .lock()
            .unwrap()
            .push(WatcherEvent::QueuedMessage(remaining));
    }

    // Restore normal terminal mode.
    let _ = crossterm::terminal::disable_raw_mode();
}

// ── Unix fd import ──────────────────────────────────────────

#[cfg(unix)]
use std::os::unix::io::AsRawFd;
