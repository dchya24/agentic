#[cfg(test)]
mod tests;

mod cli;
mod commands;
mod confirmation;
mod interactive;
mod output;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use commands::Commands;
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

    let config = if let Some(config_path) = &cli.config {
        core_agentic::Config::load_from_path(config_path).ok_or_else(|| anyhow::anyhow!("Failed to load config"))?
    } else {
        core_agentic::Config::default()
    };

    let commands = Commands::new(config);

    match &cli.command {
        Some(cli::Command::Run { task }) => {
            commands.run(&task)?;
        }
        Some(cli::Command::Interactive) => {
            interactive::run(commands)?;
        }
        Some(cli::Command::Config { action }) => {
            commands.config(&action)?;
        }
        Some(cli::Command::Version) => {
            println!("agentic {}", env!("CARGO_PKG_VERSION"));
        }
        None => {}
    }

    Ok(())
}