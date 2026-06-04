//! Input watcher for ESC abort and message queuing during agent processing.
//!
//! When the agent is running, this module:
//! - Detects ESC key presses to abort the current operation
//! - Buffers typed characters and queues complete messages on Enter
//! - Shows a minimal input prompt with visual feedback
//!
//! Uses crossterm raw mode for individual key detection, with ONLCR
//! output processing preserved so `println!()` / `\n` still works.

use std::io::Write;
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

/// Spawns a background thread that watches for ESC and collects queued
/// messages while the agent is processing. Returns a handle that can be
/// polled for events and signaled to stop.
///
/// # Terminal state
///
/// On creation, enters crossterm raw mode but re-enables `OPOST | ONLCR`
/// so that `\n` is still translated to `\r\n` on output. This lets us
/// detect individual key presses without breaking formatted output.
///
/// On drop or `stop()`, the original terminal state is restored.
pub struct InputWatcher {
    events: Arc<Mutex<Vec<WatcherEvent>>>,
    done: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl InputWatcher {
    /// Start watching. Call `stop()` when the agent finishes.
    pub fn start(cancel_flag: Arc<std::sync::atomic::AtomicBool>) -> Self {
        let events: Arc<Mutex<Vec<WatcherEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));

        let events_clone = events.clone();
        let done_clone = done.clone();

        let handle = std::thread::Builder::new()
            .name("agentic-input-watcher".into())
            .spawn(move || {
                run_watcher(&done_clone, &events_clone, &cancel_flag);
            })
            .expect("Failed to spawn input watcher thread");

        Self {
            events,
            done,
            thread_handle: Some(handle),
        }
    }

    /// Signal the watcher to stop and wait for the thread to finish.
    /// Returns any events collected since the last call.
    pub fn stop(&mut self) -> Vec<WatcherEvent> {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            // Give the thread a moment to finish; don't block forever.
            let _ = handle.join();
        }
        std::mem::take(&mut self.events.lock().unwrap())
    }

    /// Drain collected events without stopping the watcher.
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
    cancel_flag: &std::sync::atomic::AtomicBool,
) {
    // Enter crossterm raw mode (disables ICANON, ECHO, etc.).
    if crossterm::terminal::enable_raw_mode().is_err() {
        // If we can't enter raw mode, we can't detect individual keys.
        // Just wait until done.
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

    // Print the queue prompt.
    eprint!("\r\n  \x1b[2m⏳ ESC to cancel · type to queue\x1b[0m\r\n  \x1b[33m>\x1b[0m ");
    let _ = std::io::stderr().flush();

    let mut buffer = String::new();

    while !done.load(Ordering::Relaxed) {
        // Poll for key events (50ms timeout).
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
                        eprint!("\r\n  \x1b[33m⚠ Cancel requested\x1b[0m\r\n");
                        let _ = std::io::stderr().flush();
                        events.lock().unwrap().push(WatcherEvent::Abort);
                        break;
                    }
                    crossterm::event::KeyCode::Enter => {
                        let line = buffer.trim().to_string();
                        if !line.is_empty() {
                            eprint!(
                                "\r\n  \x1b[32m✓ Queued:\x1b[0m {}\r\n  \x1b[33m>\x1b[0m ",
                                if line.len() > 60 {
                                    format!("{}...", &line[..57])
                                } else {
                                    line.clone()
                                }
                            );
                            let _ = std::io::stderr().flush();
                            events.lock().unwrap().push(WatcherEvent::QueuedMessage(line));
                        }
                        buffer.clear();
                    }
                    crossterm::event::KeyCode::Char(c) => {
                        // Ctrl+C — abort (complements the main signal handler).
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                            && c == 'c'
                        {
                            cancel_flag.store(true, Ordering::SeqCst);
                            eprint!("\r\n  \x1b[33m⚠ Cancel requested\x1b[0m\r\n");
                            let _ = std::io::stderr().flush();
                            events.lock().unwrap().push(WatcherEvent::Abort);
                            break;
                        }
                        // Ctrl+U — clear the input buffer.
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                            && c == 'u'
                        {
                            let clear_len = buffer.len();
                            buffer.clear();
                            for _ in 0..clear_len {
                                eprint!("\x1b[1D \x1b[1D");
                            }
                            let _ = std::io::stderr().flush();
                            continue;
                        }
                        // Normal character.
                        buffer.push(c);
                        eprint!("{}", c);
                        let _ = std::io::stderr().flush();
                    }
                    crossterm::event::KeyCode::Backspace
                    | crossterm::event::KeyCode::Delete => {
                        if buffer.pop().is_some() {
                            eprint!("\x1b[1D \x1b[1D");
                            let _ = std::io::stderr().flush();
                        }
                    }
                    _ => {
                        // Ignore other key events (arrow keys, etc.)
                    }
                }
            }
            // Ignore resize / mouse events.
            _ => {}
        }
    }

    // Queue any remaining buffer content.
    let remaining = buffer.trim().to_string();
    if !remaining.is_empty() {
        events.lock().unwrap().push(WatcherEvent::QueuedMessage(remaining));
    }

    // Restore normal terminal mode.
    let _ = crossterm::terminal::disable_raw_mode();
}

// ── Unix fd import ──────────────────────────────────────────

#[cfg(unix)]
use std::os::unix::io::AsRawFd;
