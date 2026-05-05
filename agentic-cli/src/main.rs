#[cfg(test)]
mod tests;

mod cli;
mod commands;
mod config;
mod confirmation;
mod error;
mod interactive;
mod markdown;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, ColorChoice, Command, ConfigAction};
use commands::Commands;
use core_agentic::Config;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

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
    tokio::spawn(async {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\n⚠ Interrupted.");
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
            Some(Command::Run { .. }) | Some(Command::Interactive) => {
                eprintln!("⚠ Config file not found at: {}", Config::config_path().display());
                eprintln!();
                eprintln!("To get started:");
                eprintln!("  agentic config init                  # Default config");
                eprintln!("  agentic config init --interactive    # Guided wizard");
                eprintln!("  agentic config init --provider zai   # Quick setup");
                eprintln!();
                std::process::exit(1);
            }
            _ => Config::fallback(),
        }
    };

    let mut commands = Commands::new(config)
        .with_color(color_enabled)
        .with_debug(cli.debug);

    // ── Command dispatch ───────────────────────────────────
    match &cli.command {
        Some(Command::Run { task }) => {
            commands.run(&task).await?;
        }
        Some(Command::Interactive) => {
            interactive::run(commands).await?;
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
            Cli::parse_from(["agentic", "--help"]);
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
