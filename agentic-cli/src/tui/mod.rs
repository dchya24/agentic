//! TUI module for interactive mode using ratatui
//!
//! Features:
//! - Padded UI layout
//! - Rich markdown rendering
//! - Animated progress indicators
//! - Dropdown for `/` commands
//! - Dropdown for `@` file completion

mod app;
mod dropdown;
mod input;
mod markdown_widget;
mod progress;
mod ui;

pub use app::run_tui;
