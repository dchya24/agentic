#[cfg(test)]
mod tests;

mod cli;
mod commands;
mod confirmation;
mod error;
mod file_ref;
mod interactive;
mod session;
mod tui;
mod widgets;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, ColorChoice, Command, ConfigAction};
use commands::Commands;
use core_agentic::Config;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Shared cancel flag. The signal handler flips this on the first Ctrl+C;
/// the orchestrator's loop boundary picks it up and returns gracefully.
/// A second Ctrl+C escalates to a hard exit.
static CANCEL_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

pub(crate) fn cancel_flag() -> Arc<AtomicBool> {
    CANCEL_FLAG
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Logging setup ─────────────────────────────────────
    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = if cli.debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(log_level))
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(cli.debug))
        .with(filter)
        .init();

    if cli.verbose > 0 || cli.debug {
        tracing::info!(
            "agentic v{} — verbose:{}, debug:{}, color:{:?}",
            env!("CARGO_PKG_VERSION"),
            cli.verbose,
            cli.debug,
            cli.color,
        );
    }

    // ── Color resolution ───────────────────────────────────
    // Auto leaves the decision to capabilities::should_use_color() which
    // also honors NO_COLOR, TERM=dumb, and the TTY check.
    let color_override = match cli.color {
        ColorChoice::Always => Some(true),
        ColorChoice::Never => Some(false),
        ColorChoice::Auto => None,
    };
    widgets::capabilities::set_color_enabled(color_override);
    let color_enabled = widgets::capabilities::should_use_color();

    // ── Ctrl+C graceful shutdown (background task) ─────────
    // First Ctrl+C: cooperative cancel. The agent loop checks this flag at
    // every turn boundary and returns AgenticError::Cancelled.
    // Second Ctrl+C: force-exit (we may be stuck in a tool that doesn't
    // observe the flag).
    let cancel = cancel_flag();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        if cancel.swap(true, Ordering::SeqCst) {
            // Already cancelled once — hard exit on second Ctrl+C.
            eprintln!("\n⚠ Force-exiting on second Ctrl+C.");
            std::process::exit(130);
        }
        eprintln!("\n⚠ Cancel requested (press Ctrl+C again to force-exit).");
        // Wait for a possible second Ctrl+C.
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\n⚠ Force-exiting on second Ctrl+C.");
        std::process::exit(130);
    });

    // ── Config commands (don't need loaded config) ─────────
    if let Some(Command::Config(action)) = &cli.command {
        if matches!(action, ConfigAction::Path) {
            println!("{}", Config::config_path().display());
            return Ok(());
        }

        if matches!(action, ConfigAction::Init { .. } | ConfigAction::Reset { .. }) {
            let fallback_config = Config::fallback();
            let commands = Commands::new(fallback_config)
                .with_color(color_enabled)
                .with_debug(cli.debug);
            return commands.config(action).map_err(|e| anyhow::anyhow!(e));
        }

        if matches!(action, ConfigAction::Validate { .. }) {
            if !Config::config_exists() {
                eprintln!("✗ Config file not found. Run 'agentic config init' to create one.");
                std::process::exit(1);
            }
        }
    }

    // ── Load config ────────────────────────────────────────
    let config = if let Some(config_path) = &cli.config {
        Config::load_from_path(config_path)
            .ok_or_else(|| anyhow::anyhow!("Failed to load config from: {}", config_path))?
    } else if let Some(loaded) = Config::load() {
        loaded
    } else {
        match &cli.command {
            Some(Command::Run { .. }) | Some(Command::Interactive) | Some(Command::Tui) | None => {
                use ratatui::style::{Color as RColor, Modifier, Style as RStyle};
                use ratatui::text::{Line, Span as RSpan};
                use widgets::{components, inline};

                inline::print_line(&components::warning_badge(&format!(
                    "Config file not found. Creating default at: {}",
                    Config::config_path().display()
                )));
                inline::print_blank();

                let new_config = Config::fallback();
                if let Err(e) = new_config.save() {
                    inline::print_line(&components::error_badge(&format!(
                        "Failed to create config: {}",
                        e
                    )));
                    std::process::exit(1);
                }

                inline::print_line(&components::success_badge("Default config created."));
                inline::print_line(&components::warning_badge(
                    "No providers or models configured yet.",
                ));
                inline::print_blank();
                inline::print_line(&Line::from(RSpan::raw("  Set up a provider with:")));
                let bold = RStyle::default().add_modifier(Modifier::BOLD);
                let dim = RStyle::default().fg(RColor::DarkGray);
                inline::print_line(&Line::from(vec![
                    RSpan::raw("    "),
                    RSpan::styled("agentic config init --interactive", bold),
                    RSpan::styled("   # Guided wizard", dim),
                ]));
                inline::print_line(&Line::from(vec![
                    RSpan::raw("    "),
                    RSpan::styled("agentic config init --provider zai", bold),
                    RSpan::styled("  # Quick setup", dim),
                ]));
                inline::print_line(&Line::from(vec![
                    RSpan::raw("    "),
                    RSpan::styled("agentic config edit", bold),
                    RSpan::styled("                  # Edit manually", dim),
                ]));
                inline::print_blank();

                new_config
            }
            _ => Config::fallback(),
        }
    };

    let mut commands = Commands::new(config)
        .with_color(color_enabled)
        .with_debug(cli.debug)
        .with_permission_mode(cli.mode.into());

    // ── Command dispatch ───────────────────────────────────
    match &cli.command {
        Some(Command::Run { task }) => {
            commands.run(&task).await?;
        }
        Some(Command::Interactive) => {
            interactive::run(commands).await?;
        }
        Some(Command::Tui) => {
            tui::run_tui(commands).await?;
        }
        Some(Command::Config(action)) => {
            commands.config(action)?;
        }
        Some(Command::Status) => {
            commands.status()?;
        }
        Some(Command::Examples) => {
            commands.examples();
        }
        Some(Command::Version) => {
            println!("agentic {}", env!("CARGO_PKG_VERSION"));
        }
        None => {
            interactive::run(commands).await?;
        }
    }

    Ok(())
}
