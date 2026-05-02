#[cfg(test)]
mod tests;

mod cli;
mod commands;
mod confirmation;
mod markdown;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, ConfigAction};
use commands::{Commands, CommandError};
use core_agentic::Config;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let cli = Cli::parse();

    if let Some(level) = cli.verbose {
        tracing::info!("Verbose mode enabled: {:?}", level);
    }

    // Handle config commands that don't need a loaded config
    if let Some(Command::Config { action }) = &cli.command {
        // Config path doesn't need config loaded
        if matches!(action, ConfigAction::Path) {
            println!("{}", Config::config_path().display());
            return Ok(());
        }

        // Init and Reset create their own config
        if matches!(action, ConfigAction::Init | ConfigAction::Reset { .. }) {
            let fallback_config = Config::fallback();
            let commands = Commands::new(fallback_config);
            return commands.config(action).map_err(|e| anyhow::anyhow!(e));
        }

        // Validate can work with fallback if file doesn't exist
        if matches!(action, ConfigAction::Validate) {
            if !Config::config_exists() {
                eprintln!("✗ Config file not found. Run 'agentic config init' to create one.");
                std::process::exit(1);
            }
        }
    }

    // Load config for other commands
    let config = if let Some(config_path) = &cli.config {
        Config::load_from_path(config_path)
            .ok_or_else(|| anyhow::anyhow!("Failed to load config from: {}", config_path))?
    } else {
        // Try to load from default path
        if let Some(loaded) = Config::load() {
            loaded
        } else {
            // Check if this is a first run or config is missing
            match &cli.command {
                Some(Command::Run { .. }) | Some(Command::Interactive) => {
                    eprintln!("⚠ Config file not found at: {}", Config::config_path().display());
                    eprintln!();
                    eprintln!("To get started:");
                    eprintln!("  1. Run 'agentic config init' to create a default config");
                    eprintln!("  2. Edit the config to add your API key");
                    eprintln!("  3. Or use environment variables: OPENAI_API_KEY and OPENAI_BASE_URL");
                    eprintln!();
                    eprintln!("Quick setup:");
                    eprintln!("  agentic config init    # Create default config");
                    eprintln!("  agentic config edit     # Edit in your editor");
                    eprintln!("  agentic config show     # Show current config");
                    eprintln!();
                    std::process::exit(1);
                }
                _ => Config::fallback(),
            }
        }
    };

    let commands = Commands::new(config);

    match &cli.command {
        Some(Command::Run { task }) => {
            commands.run(&task).await?;
        }
        Some(Command::Interactive) => {
            interactive::run(commands).await?;
        }
        Some(Command::Config { action }) => {
            commands.config(action)?;
        }
        Some(Command::Version) => {
            println!("agentic {}", env!("CARGO_PKG_VERSION"));
        }
        None => {
            // No command - show help
            Cli::parse_from(["agentic", "--help"]);
        }
    }

    Ok(())
}
