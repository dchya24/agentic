use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "agentic")]
#[command(about = "AI agent orchestration CLI", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short, long, value_name = "PATH", global = true)]
    pub config: Option<String>,

    #[arg(short, long, action)]
    pub verbose: Option<VerboseLevel>,
}

#[derive(Parser, Debug, Clone)]
pub enum Command {
    #[command(about = "Run a single task")]
    Run { task: String },

    #[command(about = "Start interactive mode")]
    Interactive,

    #[command(about = "Manage configuration")]
    Config { action: ConfigAction },

    #[command(about = "Show version")]
    Version,
}

#[derive(Parser, Debug, Clone, ValueEnum)]
pub enum ConfigAction {
    Show,
    Edit,
    Reset,
}

#[derive(ValueEnum, Debug, Clone)]
pub enum VerboseLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}
