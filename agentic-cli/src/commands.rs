use anyhow::Result;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color as TColor, Table};
use core_agentic::{Config, Orchestrator, ToolRegistry};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::{ConfigAction, OutputFormat};
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::error::CommandError;
use crate::markdown::render_markdown;

static ALWAYS_CONFIRM: AtomicBool = AtomicBool::new(false);
static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Provider presets for quick setup
struct ProviderPreset {
    name: &'static str,
    provider_type: &'static str,
    api_base: &'static str,
    models: &'static [(&'static str, &'static str)],
}

const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        provider_type: "openai-compatible",
        api_base: "https://api.openai.com/v1",
        models: &[
            ("gpt-4o", "GPT-4o"),
            ("gpt-4o-mini", "GPT-4o Mini"),
            ("gpt-4-turbo", "GPT-4 Turbo"),
        ],
    },
    ProviderPreset {
        name: "anthropic",
        provider_type: "openai-compatible",
        api_base: "https://api.anthropic.com/v1",
        models: &[
            ("claude-sonnet-4-20250514", "Claude Sonnet 4"),
            ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet"),
        ],
    },
    ProviderPreset {
        name: "zai",
        provider_type: "openai-compatible",
        api_base: "https://api.z.ai/v1",
        models: &[
            ("glm-4.7", "GLM-4.7"),
            ("glm-4", "GLM-4"),
        ],
    },
    ProviderPreset {
        name: "custom",
        provider_type: "openai-compatible",
        api_base: "",
        models: &[],
    },
];

pub struct Commands {
    config: Config,
    orchestrator: Option<Orchestrator>,
    color_enabled: bool,
    debug_enabled: bool,
}

impl Commands {
    /// Create Commands without initializing the orchestrator (for config/status/examples)
    pub fn new(config: Config) -> Self {
        Self {
            config,
            orchestrator: None,
            color_enabled: true,
            debug_enabled: false,
        }
    }

    pub fn with_color(mut self, enabled: bool) -> Self {
        self.color_enabled = enabled;
        COLOR_ENABLED.store(enabled, Ordering::Relaxed);
        self
    }

    pub fn with_debug(mut self, enabled: bool) -> Self {
        self.debug_enabled = enabled;
        self
    }

    /// Lazily initialize the orchestrator when needed (run/interactive)
    fn ensure_orchestrator(&mut self) -> Result<()> {
        if self.orchestrator.is_some() {
            return Ok(());
        }

        let provider_config = self
            .config
            .to_provider_config()
            .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;
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

        self.orchestrator = Some(orchestrator);
        Ok(())
    }

    // ── Status ──────────────────────────────────────────────

    pub fn status(&self) -> Result<()> {
        let config_path = Config::config_path();
        let config_exists = config_path.exists();

        println!();
        print_info("Agentic CLI Status");
        println!("  Config file: {}", config_path.display());
        if config_exists {
            print_success("  Config file: exists");
        } else {
            print_warning("  Config file: not found");
        }

        if self.config.providers.is_empty() {
            print_warning("  No providers configured");
        } else {
            for (i, p) in self.config.providers.iter().enumerate() {
                println!();
                print_info(&format!("Provider #{}: {}", i + 1, p.name));
                println!("    Type:     {}", p.provider_type);
                println!("    API Base: {}", p.api_base);
                if p.api_key.is_empty() {
                    print_warning("    API Key:  ✗ not set");
                } else {
                    print_success("    API Key:  ✓ configured");
                }
                if !p.models.is_empty() {
                    println!("    Models:");
                    for m in &p.models {
                        let display = m
                            .display_name
                            .as_deref()
                            .unwrap_or(&m.model);
                        println!("      • {} ({})", display, m.model);
                    }
                }
            }
        }
        println!();
        Ok(())
    }

    // ── Examples ────────────────────────────────────────────

    pub fn examples(&self) {
        println!();
        println!("  \x1b[1m\x1b[36m📖 Agentic CLI — Usage Examples\x1b[0m");
        println!();
        println!("  \x1b[33m# Run a single task\x1b[0m");
        println!("  agentic run \"list all Rust files\"");
        println!("  agentic run \"create hello.txt with 'hello world'\"");
        println!("  agentic run \"explain the codebase structure\"");
        println!();
        println!("  \x1b[33m# Interactive mode\x1b[0m");
        println!("  agentic interactive");
        println!("  agentic i");
        println!();
        println!("  \x1b[33m# Config management\x1b[0m");
        println!("  agentic config init                    # Default config");
        println!("  agentic config init --interactive      # Guided wizard");
        println!("  agentic config init --provider openai  # Quick setup");
        println!("  agentic config show");
        println!("  agentic config show --format table");
        println!("  agentic config edit");
        println!("  agentic config validate");
        println!("  agentic config backup");
        println!("  agentic config export                  # Masked secrets");
        println!();
        println!("  \x1b[33m# Status & info\x1b[0m");
        println!("  agentic status");
        println!("  agentic version");
        println!();
    }

    // ── Run task ────────────────────────────────────────────

    pub async fn run(&mut self, task: &str) -> Result<()> {
        self.ensure_orchestrator()?;

        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;

        println!("\n🤖 Running task: {}\n", task);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.dim} {msg}")
                .unwrap(),
        );
        pb.set_message("Thinking...");

        let result = orchestrator
            .run_stream(task, |chunk| {
                // Stop spinner on first real content
                if !pb.is_finished() {
                    pb.finish_and_clear();
                }
                print_chunk(&chunk, self.color_enabled);
            })
            .await;

        match result {
            Ok(final_result) => {
                println!("\n");
                print_markdown_header("Final Result");
                render_markdown(&final_result).ok();
            }
            Err(e) => {
                pb.finish_and_clear();
                print_error(&format!("Error: {}", e), self.color_enabled);
            }
        }

        Ok(())
    }

    // ── Config dispatch ─────────────────────────────────────

    pub fn config(&self, action: &ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show { format } => self.config_show(*format)?,
            ConfigAction::Init { interactive, provider } => {
                self.config_init(*interactive, provider.as_deref())?
            }
            ConfigAction::Edit => self.config_edit()?,
            ConfigAction::Validate { verbose } => self.config_validate(*verbose)?,
            ConfigAction::Reset { force } => self.config_reset(*force)?,
            ConfigAction::Path => self.config_path()?,
            ConfigAction::Backup => self.config_backup()?,
            ConfigAction::Restore { file } => self.config_restore(file)?,
            ConfigAction::Export => self.config_export()?,
            ConfigAction::Import { file } => self.config_import(file)?,
        }
        Ok(())
    }

    // ── Config show (json or table) ─────────────────────────

    fn config_show(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&self.config)
                    .map_err(|e| CommandError::Config(e.to_string()))?;
                println!("{}", json);
            }
            OutputFormat::Table => {
                self.config_show_table()?;
            }
        }
        Ok(())
    }

    fn config_show_table(&self) -> Result<()> {
        println!();

        // Providers table
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec![
                Cell::new("Provider").fg(TColor::Cyan),
                Cell::new("Type").fg(TColor::Cyan),
                Cell::new("API Base").fg(TColor::Cyan),
                Cell::new("Key").fg(TColor::Cyan),
                Cell::new("Models").fg(TColor::Cyan),
            ]);

        for p in &self.config.providers {
            let key_status = if p.api_key.is_empty() {
                Cell::new("✗").fg(TColor::Red)
            } else if p.api_key.starts_with('$') {
                Cell::new(format!("env:{}", &p.api_key)).fg(TColor::Yellow)
            } else {
                Cell::new(format!("{}...{}", &p.api_key[..4.min(p.api_key.len())], &p.api_key[p.api_key.len().saturating_sub(4)..])).fg(TColor::Green)
            };

            let models: String = p
                .models
                .iter()
                .map(|m| {
                    m.display_name
                        .as_deref()
                        .unwrap_or(&m.model)
                })
                .collect::<Vec<_>>()
                .join(", ");

            table.add_row(vec![
                Cell::new(&p.name),
                Cell::new(&p.provider_type),
                Cell::new(&p.api_base),
                key_status,
                Cell::new(if models.is_empty() { "none" } else { &models }),
            ]);
        }

        println!("  \x1b[1mProviders:\x1b[0m");
        println!("{table}");

        // Safety table
        let mut safety_table = Table::new();
        safety_table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec![
                Cell::new("Setting").fg(TColor::Cyan),
                Cell::new("Value").fg(TColor::Cyan),
            ]);

        safety_table.add_row(vec![
            Cell::new("Auto-approve low risk"),
            Cell::new(if self.config.safety.auto_approve_low_risk {
                "yes"
            } else {
                "no"
            }),
        ]);

        let blocked = if self.config.safety.blocked_commands.is_empty() {
            "none".to_string()
        } else {
            self.config
                .safety
                .blocked_commands
                .join(", ")
        };
        safety_table.add_row(vec![Cell::new("Blocked commands"), Cell::new(blocked)]);

        println!("  \x1b[1mSafety:\x1b[0m");
        println!("{safety_table}");

        // Output table
        let mut output_table = Table::new();
        output_table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec![
                Cell::new("Setting").fg(TColor::Cyan),
                Cell::new("Value").fg(TColor::Cyan),
            ]);

        output_table.add_row(vec![
            Cell::new("Color"),
            Cell::new(if self.config.output.color { "yes" } else { "no" }),
        ]);
        output_table.add_row(vec![
            Cell::new("Stream"),
            Cell::new(if self.config.output.stream {
                "yes"
            } else {
                "no"
            }),
        ]);
        output_table.add_row(vec![
            Cell::new("Show thoughts"),
            Cell::new(if self.config.output.show_thoughts {
                "yes"
            } else {
                "no"
            }),
        ]);
        output_table.add_row(vec![
            Cell::new("Show tool calls"),
            Cell::new(if self.config.output.show_tool_calls {
                "yes"
            } else {
                "no"
            }),
        ]);

        println!("  \x1b[1mOutput:\x1b[0m");
        println!("{output_table}");

        // MCP servers
        if !self.config.mcp_servers.is_empty() {
            let mut mcp_table = Table::new();
            mcp_table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_header(vec![
                    Cell::new("Name").fg(TColor::Cyan),
                    Cell::new("Command").fg(TColor::Cyan),
                ]);
            for (name, srv) in &self.config.mcp_servers {
                mcp_table.add_row(vec![Cell::new(name), Cell::new(srv.command.as_deref().unwrap_or(""))]);
            }
            println!("  \x1b[1mMCP Servers:\x1b[0m");
            println!("{mcp_table}");
        }

        Ok(())
    }

    // ── Config init (default / --interactive / --provider) ──

    fn config_init(&self, interactive: bool, provider_name: Option<&str>) -> Result<()> {
        let config_path = Config::config_path();

        if config_path.exists() {
            let overwrite = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!(
                    "Config file already exists at {}. Overwrite?",
                    config_path.display()
                ))
                .default(false)
                .interact()
                .unwrap_or(false);
            if !overwrite {
                println!("Aborted.");
                return Ok(());
            }
        }

        let new_config = if interactive {
            self.config_init_wizard()?
        } else if let Some(name) = provider_name {
            self.config_init_provider(name)?
        } else {
            // Default: create fallback config
            print_info("Creating default config (OpenAI-compatible)...");
            Config::fallback()
        };

        // Ensure directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CommandError::Config(format!("Failed to create directory: {}", e))
            })?;
        }

        let content = serde_json::to_string_pretty(&new_config)
            .map_err(|e| CommandError::Config(e.to_string()))?;

        std::fs::write(&config_path, content).map_err(|e| {
            CommandError::Config(format!("Failed to write config: {}", e))
        })?;

        print_success(&format!(
            "Config file created at: {}",
            config_path.display()
        ));

        // Next steps guidance
        if new_config
            .providers
            .iter()
            .any(|p| p.api_key.is_empty())
        {
            println!();
            print_info("Next steps:");
            println!("  1. Set your API key:");
            println!("     agentic config edit");
            println!("     # or set env var: OPENAI_API_KEY=sk-...");
            println!("  2. Verify: agentic config validate");
            println!("  3. Try it: agentic run \"list files\"");
        } else {
            println!();
            print_info("You're all set! Try: agentic run \"hello world\"");
        }

        Ok(())
    }

    /// Full interactive wizard using dialoguer
    fn config_init_wizard(&self) -> Result<Config> {
        println!();
        println!("  \x1b[1m\x1b[36m╔══════════════════════════════════════════╗\x1b[0m");
        println!("  \x1b[1m\x1b[36m║      🤖 Agentic Config Wizard            ║\x1b[0m");
        println!("  \x1b[1m\x1b[36m╚══════════════════════════════════════════╝\x1b[0m");
        println!();

        // Step 1: Choose provider
        let provider_names: Vec<&str> = PROVIDER_PRESETS.iter().map(|p| p.name).collect();
        let provider_idx = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Step 1: Choose your LLM provider")
            .items(&provider_names)
            .default(0)
            .interact()
            .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;

        let preset = &PROVIDER_PRESETS[provider_idx];

        // Step 2: API base URL
        let default_base = if preset.api_base.is_empty() {
            String::new()
        } else {
            preset.api_base.to_string()
        };
        let api_base: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Step 2: API base URL")
            .default(default_base)
            .interact()
            .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;

        // Step 3: API key
        let env_key = if provider_idx == 0 {
            "OPENAI_API_KEY"
        } else if provider_idx == 1 {
            "ANTHROPIC_API_KEY"
        } else {
            "API_KEY"
        };

        let env_value = std::env::var(env_key).unwrap_or_default();
        let api_key_default = if env_value.is_empty() {
            String::new()
        } else {
            env_value
        };

        let api_key: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Step 3: API key (or env:${})", env_key))
            .default(api_key_default)
            .interact()
            .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;

        // Resolve if user typed env reference
        let resolved_key = if api_key.starts_with('$') {
            std::env::var(&api_key[1..]).unwrap_or_else(|_| api_key.clone())
        } else {
            api_key
        };

        // Step 4: Choose model
        let model = if preset.models.is_empty() {
            // Custom provider — ask for model name
            let model_name: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Step 4: Model name (e.g. gpt-4o)")
                .interact()
                .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;
            core_agentic::ModelConfig {
                model: model_name,
                display_name: None,
                temperature: 0.7,
                max_tokens: 8192,
            }
        } else {
            let model_labels: Vec<String> = preset
                .models
                .iter()
                .map(|(id, name)| format!("{} ({})", name, id))
                .collect();

            let model_idx = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Step 4: Choose default model")
                .items(&model_labels)
                .default(0)
                .interact()
                .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;

            core_agentic::ModelConfig {
                model: preset.models[model_idx].0.to_string(),
                display_name: Some(preset.models[model_idx].1.to_string()),
                temperature: 0.7,
                max_tokens: 8192,
            }
        };

        // Step 5: Safety options
        let auto_approve = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Step 5: Auto-approve low-risk tool calls?")
            .default(true)
            .interact()
            .unwrap_or(true);

        // Build the config
        let config = Config {
            providers: vec![core_agentic::ProviderConfig {
                name: preset.name.to_string(),
                provider_type: preset.provider_type.to_string(),
                api_base,
                api_key: resolved_key.clone(),
                models: vec![model],
            }],
            safety: core_agentic::SafetyConfig {
                auto_approve_low_risk: auto_approve,
                blocked_commands: vec![
                    "rm -rf /".to_string(),
                    "mkfs".to_string(),
                    "dd if=".to_string(),
                ],
            },
            output: core_agentic::OutputConfig {
                color: true,
                stream: true,
                show_thoughts: true,
                show_tool_calls: true,
            },
            mcp_servers: std::collections::HashMap::new(),
        };

        // Summary
        println!();
        print_success("Configuration summary:");
        println!("  Provider: {}", config.providers[0].name);
        println!("  API Base: {}", config.providers[0].api_base);
        println!(
            "  Model:    {}",
            config.providers[0].models[0].model
        );
        println!(
            "  API Key:  {}",
            if resolved_key.is_empty() {
                "(empty)".to_string()
            } else {
                format!("{}...{}", &resolved_key[..4.min(resolved_key.len())], &resolved_key[resolved_key.len().saturating_sub(4)..])
            }
        );

        Ok(config)
    }

    /// Quick provider preset setup
    fn config_init_provider(&self, name: &str) -> Result<Config> {
        let preset = PROVIDER_PRESETS
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                let available: Vec<&str> = PROVIDER_PRESETS.iter().map(|p| p.name).collect();
                CommandError::not_found_with_suggestion(
                    format!("provider '{}'", name),
                    format!("available: {}", available.join(", ")),
                )
            })?;

        if preset.name == "custom" {
            // Redirect to interactive wizard for custom
            return self.config_init_wizard();
        }

        // Try to get API key from env
        let env_var = format!("{}_API_KEY", preset.name.to_uppercase());
        let api_key = std::env::var(&env_var)
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();

        let default_model = preset
            .models
            .first()
            .map(|(id, display)| core_agentic::ModelConfig {
                model: id.to_string(),
                display_name: Some(display.to_string()),
                temperature: 0.7,
                max_tokens: 8192,
            })
            .unwrap_or(core_agentic::ModelConfig {
                model: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                temperature: 0.7,
                max_tokens: 8192,
            });

        print_info(&format!(
            "Creating config for {} ({})",
            preset.name, preset.api_base
        ));

        let config = Config {
            providers: vec![core_agentic::ProviderConfig {
                name: preset.name.to_string(),
                provider_type: preset.provider_type.to_string(),
                api_base: preset.api_base.to_string(),
                api_key,
                models: vec![default_model],
            }],
            safety: core_agentic::SafetyConfig::default(),
            output: core_agentic::OutputConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
        };

        Ok(config)
    }

    // ── Config edit ─────────────────────────────────────────

    fn config_edit(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!(
                "Config file not found. Run 'agentic config init' to create one."
            ));
        }

        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| {
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

    // ── Config validate ─────────────────────────────────────

    fn config_validate(&self, verbose: bool) -> Result<()> {
        print_info("Validating configuration...");
        println!();

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check providers
        if self.config.providers.is_empty() {
            errors.push("No providers configured".to_string());
        }

        for (i, provider) in self.config.providers.iter().enumerate() {
            let label = if provider.name.is_empty() {
                format!("Provider #{}", i + 1)
            } else {
                format!("Provider '{}{}'\x1b[0m'", "\x1b[1m", provider.name)
            };

            if verbose {
                println!("  Checking {}...", label);
            }

            if provider.name.is_empty() {
                errors.push(format!("{}: name is empty", label));
            }
            if provider.provider_type.is_empty() {
                errors.push(format!("{}: type is empty", label));
            }
            if provider.api_base.is_empty() {
                errors.push(format!("{}: API base URL is empty", label));
            }
            if provider.api_key.is_empty() {
                warnings.push(format!("{}: API key is empty", label));
            }
            if provider.models.is_empty() {
                warnings.push(format!("{}: No models configured", label));
            }

            if verbose {
                println!("    API Base: {}", provider.api_base);
                println!(
                    "    API Key:  {}",
                    if provider.api_key.is_empty() {
                        "(empty)".to_string()
                    } else {
                        format!(
                            "{}...{}",
                            &provider.api_key[..4.min(provider.api_key.len())],
                            &provider.api_key[provider.api_key.len().saturating_sub(4)..]
                        )
                    }
                );
                println!("    Models:   {}", provider.models.len());
                for m in &provider.models {
                    println!(
                        "      • {} (temp: {}, max_tokens: {})",
                        m.model, m.temperature, m.max_tokens
                    );
                }
            }
        }

        // Check safety
        if self.config.safety.blocked_commands.is_empty() {
            warnings.push(
                "No blocked commands configured. Consider adding dangerous commands.".to_string(),
            );
        }

        if verbose {
            println!("  Checking safety config...");
            println!(
                "    Auto-approve low risk: {}",
                self.config.safety.auto_approve_low_risk
            );
            println!(
                "    Blocked commands: {}",
                self.config.safety.blocked_commands.join(", ")
            );
        }

        // Print results
        println!();
        if errors.is_empty() && warnings.is_empty() {
            print_success("Configuration is valid!");
            print_info(&format!("Config file: {}", Config::config_path().display()));
        } else {
            for error in &errors {
                print_error(error, self.color_enabled);
            }
            for warning in &warnings {
                print_warning(warning);
            }

            if !errors.is_empty() {
                return Err(anyhow::anyhow!(
                    "Configuration validation failed with {} error(s)",
                    errors.len()
                ));
            }
            print_warning(&format!(
                "Configuration valid with {} warning(s)",
                warnings.len()
            ));
        }

        Ok(())
    }

    // ── Config reset ────────────────────────────────────────

    fn config_reset(&self, force: bool) -> Result<()> {
        let config_path = Config::config_path();

        if config_path.exists() {
            if !force {
                let confirm = Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(format!(
                        "This will reset your config at {}. Continue?",
                        config_path.display()
                    ))
                    .default(false)
                    .interact()
                    .unwrap_or(false);
                if !confirm {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            std::fs::remove_file(&config_path).map_err(|e| {
                CommandError::Config(format!("Failed to remove config: {}", e))
            })?;
        }

        let default_config = Config::fallback();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CommandError::Config(format!("Failed to create directory: {}", e))
            })?;
        }

        let content = serde_json::to_string_pretty(&default_config)
            .map_err(|e| CommandError::Config(e.to_string()))?;

        std::fs::write(&config_path, content).map_err(|e| {
            CommandError::Config(format!("Failed to write config: {}", e))
        })?;

        print_success(&format!(
            "Default config created at: {}",
            config_path.display()
        ));
        print_info(
            "Remember to set your API key in the config file or via environment variables.",
        );
        Ok(())
    }

    // ── Config path ─────────────────────────────────────────

    fn config_path(&self) -> Result<()> {
        println!("{}", Config::config_path().display());
        Ok(())
    }

    // ── Config backup ───────────────────────────────────────

    fn config_backup(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!(
                "No config file found at: {}",
                config_path.display()
            ));
        }

        let backup_dir = config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("backups");

        std::fs::create_dir_all(&backup_dir).map_err(|e| {
            CommandError::Config(format!("Failed to create backup dir: {}", e))
        })?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_file = backup_dir.join(format!("config_{}.json", timestamp));

        std::fs::copy(&config_path, &backup_file).map_err(|e| {
            CommandError::Config(format!("Failed to create backup: {}", e))
        })?;

        print_success(&format!("Backup created at: {}", backup_file.display()));
        Ok(())
    }

    // ── Config restore ──────────────────────────────────────

    fn config_restore(&self, file: &str) -> Result<()> {
        let source = std::path::PathBuf::from(file);

        if !source.exists() {
            return Err(anyhow::anyhow!("Backup file not found: {}", file));
        }

        let config_path = Config::config_path();

        if config_path.exists() {
            let backup_dir = config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).ok();
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let pre_restore = backup_dir.join(format!("pre_restore_{}.json", ts));
            std::fs::copy(&config_path, &pre_restore).ok();
            print_info(&format!(
                "Current config backed up to: {}",
                pre_restore.display()
            ));
        }

        std::fs::copy(&source, &config_path).map_err(|e| {
            CommandError::Config(format!("Failed to restore config: {}", e))
        })?;

        print_success(&format!("Config restored from: {}", file));
        Ok(())
    }

    // ── Config export ───────────────────────────────────────

    fn config_export(&self) -> Result<()> {
        let config_path = Config::config_path();

        if !config_path.exists() {
            return Err(anyhow::anyhow!(
                "No config file found. Run 'agentic config init' first."
            ));
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| CommandError::Config(format!("Failed to read config: {}", e)))?;

        let mut json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| CommandError::Config(format!("Invalid JSON: {}", e)))?;

        if let Some(providers) = json.get_mut("providers").and_then(|p| p.as_array_mut()) {
            for provider in providers.iter_mut() {
                if let Some(api_key) = provider.get_mut("api_key") {
                    if let Some(key) = api_key.as_str() {
                        if key.len() > 8 {
                            *api_key = serde_json::Value::String(format!(
                                "{}...{}",
                                &key[..4],
                                &key[key.len() - 4..]
                            ));
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

    // ── Config import ───────────────────────────────────────

    fn config_import(&self, file: &str) -> Result<()> {
        let source = std::path::PathBuf::from(file);

        if !source.exists() {
            return Err(anyhow::anyhow!("Import file not found: {}", file));
        }

        let content = std::fs::read_to_string(&source)
            .map_err(|e| CommandError::Config(format!("Failed to read import file: {}", e)))?;

        let _: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            CommandError::Config(format!("Invalid JSON in import file: {}", e))
        })?;

        let config_path = Config::config_path();

        if config_path.exists() {
            let backup_dir = config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).ok();
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let pre_import = backup_dir.join(format!("pre_import_{}.json", ts));
            std::fs::copy(&config_path, &pre_import).ok();
            print_info(&format!(
                "Current config backed up to: {}",
                pre_import.display()
            ));
        }

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CommandError::Config(format!("Failed to create directory: {}", e))
            })?;
        }

        std::fs::write(&config_path, &content).map_err(|e| {
            CommandError::Config(format!("Failed to write config: {}", e))
        })?;

        print_success(&format!("Config imported from: {}", file));
        print_info("Run 'agentic config validate' to verify the imported config.");
        Ok(())
    }
}

// ── Print helpers ───────────────────────────────────────────

fn print_chunk(chunk: &str, _color_enabled: bool) {
    let use_color = COLOR_ENABLED.load(Ordering::Relaxed);
    if chunk.starts_with('#')
        || chunk.starts_with("```")
        || chunk.starts_with('-')
        || chunk.starts_with('*')
    {
        let _ = render_markdown(chunk);
    } else if use_color {
        let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(200, 200, 200))));
        print!("{}", chunk);
        let _ = stdout.reset();
        stdout.flush().ok();
    } else {
        print!("{}", chunk);
        io::stdout().flush().ok();
    }
}

fn print_markdown_header(text: &str) {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(
            ColorSpec::new()
                .set_bold(true)
                .set_fg(Some(Color::Rgb(255, 165, 0))),
        );
        println!("\n╔═══════════════════════════════════════╗");
        println!("║ {} ", text);
        println!("╚═══════════════════════════════════════╝");
        let _ = stdout.reset();
    } else {
        println!("\n[ {} ]", text);
    }
}

fn print_success(text: &str) {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)));
        println!("✓ {}", text);
        let _ = stdout.reset();
    } else {
        println!("✓ {}", text);
    }
}

fn print_warning(text: &str) {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)));
        println!("⚠ {}", text);
        let _ = stdout.reset();
    } else {
        println!("⚠ {}", text);
    }
}

fn print_error(text: &str, _color_enabled: bool) {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        let mut stdout = StandardStream::stderr(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
        eprintln!("✗ {}", text);
        let _ = stdout.reset();
    } else {
        eprintln!("✗ {}", text);
    }
}

fn print_info(text: &str) {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(100, 149, 237))));
        println!("ℹ {}", text);
        let _ = stdout.reset();
    } else {
        println!("ℹ {}", text);
    }
}
