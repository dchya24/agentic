use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agentic")]
#[command(about = "AI agent orchestration command-line interface", long_about = None)]
pub struct Cli {
    /// Set a custom config file path
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Verbose output level (can be used multiple times)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: Option<u8>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run a single task
    Run {
        /// The task description
        task: String,
    },

    /// Start interactive mode
    Interactive,

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show version information
    Version,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show,

    /// Initialize a new configuration file
    Init,

    /// Edit configuration in default editor
    Edit,

    /// Validate configuration file
    Validate,

    /// Reset configuration to defaults
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// Get configuration file path
    Path,
}

impl ConfigAction {
    pub fn needs_config_file(&self) -> bool {
        matches!(self, Self::Show | Self::Validate | Self::Edit | Self::Reset { .. })
    }
}
