use anyhow::Result;
use core_agentic::{Config, Orchestrator, ToolRegistry};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::{self, Write};
use std::process::Command as ProcessCommand;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::ConfigAction;
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::markdown::render_markdown;
use crate::error::CommandError;

static ALWAYS_CONFIRM: AtomicBool = AtomicBool::new(false);

pub struct Commands {
    config: Config,
    orchestrator: Option<Orchestrator>,
}

impl Commands {
    pub fn new(config: Config) -> Self {
        let provider_config = config
            .to_provider_config()
            .ok_or_else(|| CommandError::Config("No provider configured".to_string()))
            .unwrap();
        let provider = Arc::new(core_agentic::OpenAIProvider::new(provider_config));

        let tools = ToolRegistry::new();
        for tool in core_agentic::tools::builtin_tools() {
            tools.register(tool);
        }

        let mut orchestrator = Orchestrator::new(provider, tools);

        orchestrator.set_confirmation_handler(|request| {
            if ALWAYS_CONFIRM.load(Ordering::Relaxed) {
                return true;
            }
            match prompt_confirmation(&request) {
                Some(ConfirmationResponse::Yes) => true,
                Some(ConfirmationResponse::Always) => {
                    ALWAYS_CONFIRM.store(true, Ordering::Relaxed);
                    true
                }
                Some(ConfirmationResponse::No) | Some(ConfirmationResponse::Quit) | None => false,
            }
        });

        Self {
            config,
            orchestrator: Some(orchestrator),
        }
    }

    pub fn status(&self) -> Result<()> {
        println!("╔══════════════════════════════════════════╗");
        println!("║        🤖 Agentic Status                 ║");
        println!("╠══════════════════════════════════════════╣");

        let config_path = Config::config_path();
        let config_exists = config_path.exists();

        print_info(&format!("Config file: {}", config_path.display()));
        if config_exists {
            print_success("Config file exists");
        } else {
            print_warning("Config file not found");
        }

        // Show provider info from config
        let json = serde_json::to_string_pretty(&self.config)
            .unwrap_or_default();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);

        if let Some(providers) = val.get("providers").and_then(|p| p.as_array()) {
            for (i, p) in providers.iter().enumerate() {
                let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                let ptype = p.get("provider_type").and_then(|t| t.as_str()).unwrap_or("unknown");
                let has_key = p.get("api_key")
                    .and_then(|k| k.as_str())
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);
                println!("  Provider #{}: {} ({})", i + 1, name, ptype);
                if has_key {
                    println!("    API Key: ✓ configured");
                } else {
                    println!("    API Key: ✗ not set");
                }
            }
        } else {
            print_warning("No providers configured");
        }

        println!("╚══════════════════════════════════════════╝");
        Ok(())
    }

    pub fn examples(&self) {
        println!("\n╔══════════════════════════════════════════╗");
        println!("║        📖 Agentic Examples               ║");
        println!("╠══════════════════════════════════════════╣");
        println!("║                                          ║");
        println!("║  # Run a single task                     ║");
        println!("║  agentic run \"list all Rust files\"      ║");
        println!("║  agentic run \"create hello.txt\"         ║");
        println!("║                                          ║");
        println!("║  # Interactive mode                       ║");
        println!("║  agentic interactive                      ║");
        println!("║  agentic i                                ║");
        println!("║                                          ║");
        println!("║  # Config management                      ║");
        println!("║  agentic config init                      ║");
        println!("║  agentic config show                      ║");
        println!("║  agentic config edit                      ║");
        println!("║  agentic config validate                  ║");
        println!("║  agentic config backup                    ║");
        println!("║                                          ║");
        println!("║  # Status                                 ║");
        println!("║  agentic status                           ║");
        println!("║                                          ║");
        println!("║  # Version                                ║");
        println!("║  agentic version                          ║");
        println!("║                                          ║");
        println!("╚══════════════════════════════════════════╝\n");
    }

    pub async fn run(&self, task: &str) -> Result<()> {
        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;

        println!("\n🤖 Running task: {}\n", task);

        let result = orchestrator.run_stream(task, |chunk| {
            print_chunk(&chunk);
        }).await;

        match result {
            Ok(final_result) => {
                println!("\n");
                print_markdown_header("Final Result");
                render_markdown(&final_result).ok();
            }
            Err(e) => {
                print_error(&format!("Error: {}", e));
            }
        }

        Ok(())
    }

    pub fn config(&self, action: &ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show { format: _ } => {
                self.config_show()?;
            }
            ConfigAction::Init { interactive: _, provider: _ } => {
                self.config_init()?;
            }
            ConfigAction::Edit => {
                self.config_edit()?;
            }
            ConfigAction::Validate { verbose: _ } => {
                self.config_validate()?;
            }
            ConfigAction::Reset { force } => {
                self.config_reset(*force)?;
            }
            ConfigAction::Path => {
                self.config_path()?;
            }
            ConfigAction::Backup => {
                self.config_backup()?;
            }
            ConfigAction::Restore { file } => {
                self.config_restore(file)?;
            }
            ConfigAction::Export => {
                self.config_export()?;
            }
            ConfigAction::Import { file } => {
                self.config_import(file)?;
            }
        }

        Ok(())
    }

    fn config_show(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.config)
            .map_err(|e| CommandError::Config(e.to_string()))?;
        println!("{}", json);
        Ok(())
    }

    fn config_init(&self) -> Result<()> {
        let config_path = Config::config_path();

        if config_path.exists() {
            print_warning(&format!("Config file already exists at: {}", config_path.display()));
            print!("Do you want to overwrite it? [y/N]: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().to_lowercase().starts_with('y') {
                println!("Aborted.");
                return Ok(());
            }
        }

        let config = &self.config;

        // Save config
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CommandError::Config(format!("Failed to create directory: {}", e)))?;
        }

        let content = serde_json::to_string_pretty(config)
            .map_err(|e| CommandError::Config(e.to_string()))?;

        std::fs::write(&config_path, content)
            .map_err(|e| CommandError::Config(format!("Failed to write config: {}", e)))?;

        print_success(&format!("Config file created at: {}", config_path.display()));

        // Prompt for API key if empty
        if config.providers.iter().any(|p| p.api_key.is_empty()) {
            print_info("Your API key is currently empty.");
            print!("Would you like to set it now? [Y/n]: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().is_empty() || input.trim().to_lowercase().starts_with('y') {
                print!("Enter your API key: ");
                io::stdout().flush().unwrap();

                let mut api_key = String::new();
                io::stdin().read_line(&mut api_key)?;
                let api_key = api_key.trim();

                if !api_key.is_empty() {
                    // Update config with API key
                    let mut updated_config = config.clone();
                    for provider in &mut updated_config.providers {
                        if provider.api_key.is_empty() {
                            provider.api_key = api_key.to_string();
                        }
                    }

                    let content = serde_json::to_string_pretty(&updated_config)
                        .map_err(|e| CommandError::Config(e.to_string()))?;

                    std::fs::write(&config_path, content)
                        .map_err(|e| CommandError::Config(format!("Failed to write config: {}", e)))?;

                    print_success("API key saved!");
                }
            }
        }

        print_info("You can now use 'agentic run <task>' to start using the AI agent.");
        Ok(())
    }

    fn config_edit(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!(
                "Config file not found. Run 'agentic config init' to create one."
            ));
        }

        // Determine editor to use
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| {
                // Try common editors
                if cfg!(windows) {
                    "notepad".to_string()
                } else if cfg!(target_os = "macos") {
                    "open".to_string()
                } else {
                    "nano".to_string()
                }
            });

        print_info(&format!("Opening {}...", config_path.display()));

        let status = ProcessCommand::new(&editor)
            .arg(&config_path)
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to open editor: {}", e))?;

        if status.success() {
            print_success("Config file updated.");
        } else {
            print_warning("Editor exited with non-zero status.");
        }

        Ok(())
    }

    fn config_validate(&self) -> Result<()> {
        print_info("Validating configuration...");

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check providers
        if self.config.providers.is_empty() {
            errors.push("No providers configured".to_string());
        }

        for (i, provider) in self.config.providers.iter().enumerate() {
            if provider.name.is_empty() {
                errors.push(format!("Provider #{}: name is empty", i + 1));
            }

            if provider.provider_type.is_empty() {
                errors.push(format!("Provider #{}: type is empty", i + 1));
            }

            if provider.api_base.is_empty() {
                errors.push(format!("Provider #{}: API base URL is empty", i + 1));
            }

            if provider.api_key.is_empty() {
                warnings.push(format!("Provider #{}: API key is empty", i + 1));
            }

            if provider.models.is_empty() {
                warnings.push(format!("Provider #{}: No models configured", i + 1));
            }
        }

        // Check safety
        if self.config.safety.blocked_commands.is_empty() {
            warnings.push("No blocked commands configured. Consider adding dangerous commands.".to_string());
        }

        // Print results
        if errors.is_empty() && warnings.is_empty() {
            print_success("Configuration is valid!");
            print_info(&format!("Config file: {}", Config::config_path().display()));
        } else {
            for error in &errors {
                print_error(error);
            }

            for warning in &warnings {
                print_warning(warning);
            }

            if !errors.is_empty() {
                return Err(anyhow::anyhow!("Configuration validation failed with {} error(s)", errors.len()));
            }
        }

        Ok(())
    }

    fn config_reset(&self, force: bool) -> Result<()> {
        let config_path = Config::config_path();

        if config_path.exists() {
            if !force {
                print_warning(&format!("This will delete your existing config file: {}", config_path.display()));
                print!("Are you sure? [y/N]: ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().to_lowercase().starts_with('y') {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            std::fs::remove_file(&config_path)
                .map_err(|e| CommandError::Config(format!("Failed to remove config: {}", e)))?;
            print_success("Config file removed.");
        }

        // Create new default config
        let default_config = Config::fallback();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CommandError::Config(format!("Failed to create directory: {}", e)))?;
        }

        let content = serde_json::to_string_pretty(&default_config)
            .map_err(|e| CommandError::Config(e.to_string()))?;

        std::fs::write(&config_path, content)
            .map_err(|e| CommandError::Config(format!("Failed to write config: {}", e)))?;

        print_success(&format!("Default config created at: {}", config_path.display()));
        print_info("Remember to set your API key in the config file or via environment variables.");
        Ok(())
    }

    fn config_path(&self) -> Result<()> {
        println!("{}", Config::config_path().display());
        Ok(())
    }

    fn config_backup(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!("No config file found at: {}", config_path.display()));
        }

        let backup_dir = Config::config_path()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("backups");

        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| CommandError::Config(format!("Failed to create backup dir: {}", e)))?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_file = backup_dir.join(format!("config_{}.json", timestamp));

        std::fs::copy(&config_path, &backup_file)
            .map_err(|e| CommandError::Config(format!("Failed to create backup: {}", e)))?;

        print_success(&format!("Backup created at: {}", backup_file.display()));
        Ok(())
    }

    fn config_restore(&self, file: &str) -> Result<()> {
        let source = std::path::PathBuf::from(file);

        if !source.exists() {
            return Err(anyhow::anyhow!("Backup file not found: {}", file));
        }

        let config_path = Config::config_path();

        // Backup current config before overwriting
        if config_path.exists() {
            let backup_dir = config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).ok();
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let pre_restore = backup_dir.join(format!("pre_restore_{}.json", ts));
            std::fs::copy(&config_path, &pre_restore).ok();
            print_info(&format!("Current config backed up to: {}", pre_restore.display()));
        }

        std::fs::copy(&source, &config_path)
            .map_err(|e| CommandError::Config(format!("Failed to restore config: {}", e)))?;

        print_success(&format!("Config restored from: {}", file));
        Ok(())
    }

    fn config_export(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!("No config file found. Run 'agentic config init' first."));
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| CommandError::Config(format!("Failed to read config: {}", e)))?;

        // Mask API keys for safe sharing
        let mut json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| CommandError::Config(format!("Invalid JSON: {}", e)))?;

        if let Some(providers) = json.get_mut("providers").and_then(|p| p.as_array_mut()) {
            for provider in providers.iter_mut() {
                if let Some(api_key) = provider.get_mut("api_key") {
                    if let Some(key) = api_key.as_str() {
                        if key.len() > 8 {
                            *api_key = serde_json::Value::String(format!("{}...{}", &key[..4], &key[key.len()-4..]));
                        } else {
                            *api_key = serde_json::Value::String("****".to_string());
                        }
                    }
                }
            }
        }

        let exported = serde_json::to_string_pretty(&json)
            .map_err(|e| CommandError::Config(e.to_string()))?;

        println!("{}", exported);
        print_info("API keys have been masked for safe sharing.");
        Ok(())
    }

    fn config_import(&self, file: &str) -> Result<()> {
        let source = std::path::PathBuf::from(file);

        if !source.exists() {
            return Err(anyhow::anyhow!("Import file not found: {}", file));
        }

        let content = std::fs::read_to_string(&source)
            .map_err(|e| CommandError::Config(format!("Failed to read import file: {}", e)))?;

        // Validate it's valid JSON
        let _: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| CommandError::Config(format!("Invalid JSON in import file: {}", e)))?;

        let config_path = Config::config_path();

        // Backup current config
        if config_path.exists() {
            let backup_dir = config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).ok();
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let pre_import = backup_dir.join(format!("pre_import_{}.json", ts));
            std::fs::copy(&config_path, &pre_import).ok();
            print_info(&format!("Current config backed up to: {}", pre_import.display()));
        }

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CommandError::Config(format!("Failed to create directory: {}", e)))?;
        }

        std::fs::write(&config_path, &content)
            .map_err(|e| CommandError::Config(format!("Failed to write config: {}", e)))?;

        print_success(&format!("Config imported from: {}", file));
        print_info("Run 'agentic config validate' to verify the imported config.");
        Ok(())
    }
}

fn print_chunk(chunk: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    
    if chunk.starts_with('#') || chunk.starts_with("```") || chunk.starts_with('-') || chunk.starts_with('*') {
        let _ = render_markdown(chunk);
    } else {
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(200, 200, 200))));
        print!("{}", chunk);
        let _ = stdout.reset();
    }
    
    stdout.flush().ok();
}

fn print_markdown_header(text: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(
        ColorSpec::new()
            .set_bold(true)
            .set_fg(Some(Color::Rgb(255, 165, 0)))
    );
    println!("\n╔═══════════════════════════════════════╗");
    println!("║ {} ", text);
    println!("╚═══════════════════════════════════════╝");
    let _ = stdout.reset();
}

fn print_success(text: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)));
    println!("✓ {}", text);
    let _ = stdout.reset();
}

fn print_warning(text: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)));
    println!("⚠ {}", text);
    let _ = stdout.reset();
}

fn print_error(text: &str) {
    let mut stdout = StandardStream::stderr(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
    eprintln!("✗ {}", text);
    let _ = stdout.reset();
}

fn print_info(text: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(100, 149, 237))));
    println!("ℹ {}", text);
    let _ = stdout.reset();
}
