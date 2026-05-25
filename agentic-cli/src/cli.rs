use clap::{Parser, Subcommand};

/// AI agent orchestration command-line interface
#[derive(Parser)]
#[command(
    name = "agentic",
    version,
    about = "AI agent orchestration CLI — run tasks, manage config, chat interactively",
    long_about = "agentic — AI agent orchestration CLI\n\
        \n\
        Run coding tasks through an AI agent with access to shell tools.\n\
        Supports multiple LLM providers (OpenAI, Anthropic, Z.ai),\n\
        streaming output, interactive mode, and MCP servers.\n\
        \n\
        Quick start:\n\
        \n  \
          agentic init                          # Interactive setup wizard\n  \
          agentic run \"list files\"              # Run a single task\n  \
          agentic interactive                   # Start REPL\n\
        \n\
        Documentation: https://github.com/nutec/termul"
)]
pub struct Cli {
    /// Set a custom config file path
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<String>,

    /// Verbose output (repeat for more: -v, -vv, -vvv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Enable debug output (shows tool calls, raw API requests)
    #[arg(long, global = true)]
    pub debug: bool,

    /// Control colored output
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,

    /// Permission mode for tool execution
    ///
    /// - default: ask for medium+ risk actions, allow reads (recommended)
    /// - plan:    read-only mode; deny all writes and commands
    /// - yolo:    auto-approve everything except hard-blocked patterns
    #[arg(long, global = true, value_name = "MODE", default_value = "default")]
    pub mode: PermissionModeArg,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// CLI representation of [`core_agentic::PermissionMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PermissionModeArg {
    Default,
    Plan,
    Yolo,
}

impl From<PermissionModeArg> for core_agentic::PermissionMode {
    fn from(arg: PermissionModeArg) -> Self {
        match arg {
            PermissionModeArg::Default => core_agentic::PermissionMode::Default,
            PermissionModeArg::Plan => core_agentic::PermissionMode::Plan,
            PermissionModeArg::Yolo => core_agentic::PermissionMode::Yolo,
        }
    }
}

/// When to use colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorChoice {
    /// Auto-detect based on terminal
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run a single task through the AI agent
    #[command(long_about = "Run a single task through the AI agent.\n\
        \n\
        The agent will analyze the task, select appropriate tools,\n\
        and execute the plan. Streaming output is shown in real-time.\n\
        \n\
        Examples:\n  \
          agentic run \"list all Rust files\"\n  \
          agentic run \"create a hello.txt with 'hello world'\"\n  \
          agentic run \"explain the codebase structure\"")]
    Run {
        /// The task description
        task: String,
    },

    /// Start interactive mode (REPL)
    #[command(alias = "i", long_about = "Start an interactive REPL session.\n\
        \n\
        Enter a persistent chat session where you can send multiple\n\
        messages, switch providers, manage config, and more.\n\
        \n\
        Slash commands: /help, /config, /provider, /model, /clear, /save, /tools, /quit")]
    Interactive,

    /// Start TUI mode (full-screen interactive)
    #[command(long_about = "Start a full-screen TUI session.\n\
        \n\
        Features:\n\
        - Rich markdown rendering\n\
        - Animated progress indicators\n\
        - Dropdown for / commands\n\
        - Dropdown for @ file completion\n\
        - Scrollable message history\n\
        \n\
        Keybindings: /help, Ctrl+C to cancel, Ctrl+D to quit")]
    Tui,

    /// Show current status (provider, model, connection)
    #[command(long_about = "Show current agent status.\n\
        \n\
        Displays active provider, model, config file location,\n\
        and whether the config is valid.")]
    Status,

    /// Show usage examples
    #[command(long_about = "Show usage examples for common tasks.\n\
        \n\
        Displays a curated list of example commands to help\n\
        you get started with the CLI.")]
    Examples,

    /// Configuration management
    #[command(subcommand)]
    Config(ConfigAction),

    /// Show version information
    #[command(alias = "v")]
    Version,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Initialize a new configuration file
    #[command(long_about = "Create a new configuration file.\n\
        \n\
        Without flags, creates a default OpenAI-compatible config.\n\
        Use --interactive for a guided setup wizard.\n\
        Use --provider for quick setup with a specific provider.\n\
        \n\
        Examples:\n  \
          agentic config init                    # Default config\n  \
          agentic config init --interactive      # Guided wizard\n  \
          agentic config init --provider openai  # Quick OpenAI setup")]
    Init {
        /// Run interactive setup wizard
        #[arg(long)]
        interactive: bool,

        /// Quick setup for a specific provider (openai, anthropic, zai)
        #[arg(long, value_name = "NAME")]
        provider: Option<String>,
    },

    /// Show current configuration
    #[command(long_about = "Display the current configuration.\n\
        \n\
        Supports multiple output formats for scripting and inspection.")]
    Show {
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "json")]
        format: OutputFormat,
    },

    /// Edit configuration in default editor ($EDITOR)
    Edit,

    /// Validate configuration file
    #[command(long_about = "Check the configuration for errors and warnings.\n\
        \n\
        Use --verbose for detailed information about each check.")]
    Validate {
        /// Show detailed validation output
        #[arg(long)]
        verbose: bool,
    },

    /// Create a timestamped backup of the config file
    #[command(long_about = "Create a backup of the current config file.\n\
        \n\
        Backups are stored in ~/.config/agentic/backups/ with timestamps.")]
    Backup,

    /// Restore configuration from a backup file
    #[command(long_about = "Restore config from a backup file.\n\
        \n\
        Provide the path to a backup file to restore from.")]
    Restore {
        /// Path to backup file to restore
        file: String,
    },

    /// Export configuration (secrets masked)
    #[command(long_about = "Export configuration in a shareable format.\n\
        \n\
        API keys and other secrets are masked for safe sharing.")]
    Export,

    /// Import configuration from a file
    #[command(long_about = "Import a configuration from a JSON file.\n\
        \n\
        The file should follow the agentic config schema.")]
    Import {
        /// Path to config file to import
        file: String,
    },

    /// Get configuration file path
    Path,

    /// Reset configuration to defaults
    #[command(long_about = "Reset the configuration file to defaults.\n\
        \n\
        This will overwrite your current config. Use --force to skip confirmation.")]
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

/// Output format for config show
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Table,
}

impl ConfigAction {
    pub fn needs_config_file(&self) -> bool {
        matches!(
            self,
            Self::Show { .. }
                | Self::Validate { .. }
                | Self::Edit
                | Self::Reset { .. }
        )
    }
}
