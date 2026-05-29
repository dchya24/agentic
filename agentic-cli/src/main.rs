#[cfg(test)]
mod tests;

mod cli;
mod commands;
mod config;
mod confirmation;
mod error;
mod file_ref;
mod interactive;
mod markdown;
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
    let color_enabled = match cli.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => atty_check(),
    };

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
                eprintln!("\x1b[33m⚠ Config file not found.\x1b[0m Creating default config at:");
                eprintln!("  {}\n", Config::config_path().display());

                let new_config = Config::fallback();
                if let Err(e) = new_config.save() {
                    eprintln!("\x1b[31m✗ Failed to create config: {}\x1b[0m", e);
                    std::process::exit(1);
                }

                eprintln!("\x1b[32m✓ Default config created.\x1b[0m");
                eprintln!("\x1b[33m⚠ No providers or models configured yet.\x1b[0m");
                eprintln!("  Set up a provider with:");
                eprintln!("    \x1b[1magentic config init --interactive\x1b[0m    # Guided wizard");
                eprintln!("    \x1b[1magentic config init --provider zai\x1b[0m  # Quick setup");
                eprintln!("    \x1b[1magentic config edit\x1b[0m             # Edit manually");
                eprintln!();

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

/// Check if stdout is a terminal (for auto color detection)
fn atty_check() -> bool {
    std::io::stdout().is_terminal()
}

// Polyfill for is_terminal on older Rust
trait IsTerminal {
    fn is_terminal(&self) -> bool;
}

impl<T: std::io::Write> IsTerminal for T {
    fn is_terminal(&self) -> bool {
        std::env::var("TERM").is_ok() || std::env::var("COLORTERM").is_ok()
    }
}
