use anyhow::Result;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color as TColor, Table};
use core_agentic::{Config, Orchestrator, ToolRegistry};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use ratatui::style::{Color as RColor, Modifier as RModifier, Style as RStyle};
use ratatui::text::{Line as RLine, Span as RSpan};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::{ConfigAction, OutputFormat};
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::error::CommandError;
use crate::widgets::capabilities;
use crate::widgets::{components, inline};

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
    permission_mode: core_agentic::PermissionMode,
    /// Path to the `AGENT.md` discovered by walk-up at orchestrator init,
    /// or `None` when no project instructions were found.
    agent_md_path: Option<std::path::PathBuf>,
    /// `true` when at least one persistent memory file (user-global or
    /// project-local) was loaded and folded into the system prompt.
    memory_md_loaded: bool,
}

impl Commands {
    /// Create Commands without initializing the orchestrator (for config/status/examples)
    pub fn new(config: Config) -> Self {
        Self {
            config,
            orchestrator: None,
            color_enabled: true,
            debug_enabled: false,
            permission_mode: core_agentic::PermissionMode::Default,
            agent_md_path: None,
            memory_md_loaded: false,
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

    pub fn with_permission_mode(mut self, mode: core_agentic::PermissionMode) -> Self {
        self.permission_mode = mode;
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
        let model_name = provider_config.default_model.clone();
        let provider: Arc<dyn core_agentic::LLMProvider> =
            Arc::new(core_agentic::OpenAIProvider::new(provider_config));

        // Build URL allowlist policy from the user config (defaults to
        // unrestricted when neither `safety.allowed_domains` nor
        // `safety.block_ip_urls` is set).
        let url_policy = self.config.url_policy();
        if !url_policy.is_unrestricted() {
            tracing::info!(
                domains = ?url_policy.allowed_domains,
                block_ip_urls = url_policy.block_ip_urls,
                "URL allowlist active"
            );
        }

        let tracker = Arc::new(core_agentic::file_tracker::FileTracker::new());
        let tools = ToolRegistry::new();
        for tool in core_agentic::tools::builtin_tools_with(tracker, url_policy) {
            tools.register(tool);
        }

        // Register the subagent tool. It needs the provider + tool registry
        // (so subagents inherit the same toolset) and the parent's cancel
        // flag (so a Ctrl+C kills children too).
        let subagent = core_agentic::SpawnSubagentTool::new(
            provider.clone(),
            tools.clone(),
            model_name.clone(),
        )
        .with_mode(self.permission_mode)
        .with_cancel(crate::cancel_flag());
        tools.register(Box::new(subagent));

        let mut orchestrator = Orchestrator::new(provider, tools);
        orchestrator.set_model(model_name);

        // Wire the process-global cancel flag so Ctrl+C in main.rs flips
        // the same atomic the orchestrator polls between turns.
        orchestrator.set_cancel_handle(crate::cancel_flag());

        // Assemble effective system prompt:
        //   default baseline  +  AGENT.md from cwd  +  config-provided override
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let project_instructions =
            core_agentic::load_project_instructions(&cwd).map(|(path, content)| {
                tracing::info!(
                    path = %path.display(),
                    bytes = content.len(),
                    "Loaded project instructions"
                );
                self.agent_md_path = Some(path);
                content
            });

        let assembled = core_agentic::assemble_system_prompt(
            None, // use DEFAULT_SYSTEM_PROMPT
            project_instructions.as_deref(),
            self.config.system_prompt.as_deref(),
        );

        // Append cross-session memory (user-global + project-local) if present.
        let memory_section = core_agentic::assemble_memory_section(&cwd);
        self.memory_md_loaded = memory_section.is_some();
        let final_prompt = match memory_section {
            Some(mem) => format!("{}\n\n---\n# Persistent Memory\n\n{}", assembled, mem),
            None => assembled,
        };
        orchestrator.set_system_prompt(final_prompt);

        // Apply permission mode (Default / Plan / Yolo).
        orchestrator.set_permission_mode(self.permission_mode);
        if self.permission_mode != core_agentic::PermissionMode::Default {
            tracing::info!(
                mode = %self.permission_mode,
                "Permission mode active"
            );
        }

        // Apply agent-loop knobs from config: LLM-based autocompact +
        // optional summarizer model override. When neither is set the
        // orchestrator's compiled-in defaults (heuristic compaction,
        // main model as summarizer) apply.
        if self.config.agent.auto_compact_with_llm {
            orchestrator.set_auto_compact_with_llm(true);
            tracing::info!("LLM-based autocompact enabled");
        }
        if let Some(ref summarizer) = self.config.agent.summarizer_model {
            orchestrator.set_summarizer_model(summarizer.clone());
            tracing::info!(model = %summarizer, "Summarizer model override");
        }
        if let Some(budget) = self.config.agent.budget_usd {
            orchestrator.set_budget_usd(Some(budget));
            tracing::info!(budget_usd = budget, "Cost budget cap active");
        }
        if !self.config.agent.pricing.is_empty() {
            orchestrator.set_pricing_overrides(self.config.agent.pricing.clone());
            tracing::info!(
                count = self.config.agent.pricing.len(),
                "Loaded pricing overrides"
            );
        }

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

    // ── Model info (for REPL prompt) ────────────────────────

    pub fn model_info(&self) -> (String, String, String) {
        if let Some(p) = self.config.active_provider() {
            let model = p.models
                .first()
                .map(|m| m.display_name.as_deref().unwrap_or(&m.model))
                .unwrap_or("unknown")
                .to_string();
            (p.name.clone(), model, p.api_base.clone())
        } else {
            ("none".into(), "none".into(), "-".into())
        }
    }

    // ── Inline config display (for REPL /config) ──────────

    pub fn config_show_inline(&self) {
        inline::print_blank();
        let key_w = 10;
        if let Some(p) = self.config.active_provider() {
            inline::print_line(&components::kv_line(
                "Provider",
                &format!("{} ({})", p.name, p.provider_type),
                key_w,
                RColor::Reset,
            ));
            inline::print_line(&components::kv_line(
                "API Base",
                &p.api_base,
                key_w,
                RColor::Reset,
            ));
            if p.api_key.is_empty() {
                inline::print_line(&components::kv_line(
                    "API Key",
                    "not set",
                    key_w,
                    RColor::Red,
                ));
            } else {
                let masked = format!(
                    "{}...{}",
                    &p.api_key[..4.min(p.api_key.len())],
                    &p.api_key[p.api_key.len().saturating_sub(4)..]
                );
                inline::print_line(&components::kv_line(
                    "API Key",
                    &masked,
                    key_w,
                    RColor::Green,
                ));
            }
            if let Some(m) = p.models.first() {
                inline::print_line(&components::kv_line(
                    "Model",
                    &format!(
                        "{} (temp: {}, max_tokens: {})",
                        m.model, m.temperature, m.max_tokens
                    ),
                    key_w,
                    RColor::Reset,
                ));
            }
        } else {
            inline::print_line(&components::warning_badge("No provider configured."));
        }
        inline::print_line(&components::kv_line(
            "Config",
            &Config::config_path().display().to_string(),
            key_w,
            RColor::Reset,
        ));
        inline::print_blank();
    }

    // ── List tools (for REPL /tools) ────────────────────────

    /// Cumulative provider cost in USD since the orchestrator was
    /// initialized for this `Commands` instance. Returns `None` if the
    /// orchestrator hasn't run yet, or if any turn used a model with
    /// unknown pricing and no override was supplied.
    pub fn cumulative_cost_usd(&self) -> Option<f64> {
        self.orchestrator
            .as_ref()
            .and_then(|o| o.cumulative_cost_usd())
    }

    /// Path to the `AGENT.md` that was loaded into the system prompt for
    /// this session, or `None` when no project instructions were found.
    /// Set during the first `ensure_orchestrator()` call.
    pub fn agent_md_path(&self) -> Option<&std::path::Path> {
        self.agent_md_path.as_deref()
    }

    /// `true` when at least one persistent memory file (user-global or
    /// project-local) was loaded into the system prompt.
    pub fn memory_md_loaded(&self) -> bool {
        self.memory_md_loaded
    }

    /// Restart the agent session in-place: drops the conversation memory,
    /// clears any pending cancel flag, drops accumulated event handlers,
    /// and resets cumulative cost. The provider, tool registry, system
    /// prompt, AGENT.md and persistent memory section all stay loaded —
    /// the user does NOT pay the re-init cost (or another wizard prompt).
    ///
    /// No-op when the orchestrator hasn't been initialized yet.
    pub fn restart_session(&mut self) {
        if let Some(orch) = self.orchestrator.as_ref() {
            orch.clear_memory();
            orch.reset_cumulative_cost();
            orch.reset_cancel();
            orch.clear_event_handlers();
        }
    }

    pub fn list_tools(&self) {
        let tools = core_agentic::ToolRegistry::new();
        for tool in core_agentic::tools::builtin_tools() {
            tools.register(tool);
        }

        let tool_list = tools.list();

        inline::print_blank();
        inline::print_line(&components::section_header(
            "🔧",
            &format!("Available Tools ({})", tool_list.len()),
            RColor::Cyan,
        ));
        inline::print_blank();

        for t in &tool_list {
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  "),
                RSpan::styled(
                    t.name.to_string(),
                    RStyle::default().add_modifier(RModifier::BOLD),
                ),
            ]));
            inline::print_line(&RLine::from(vec![
                RSpan::raw("    "),
                RSpan::raw(t.description.to_string()),
            ]));
            if !t.parameters.is_empty() {
                let params: Vec<String> = t
                    .parameters
                    .keys()
                    .map(|p| {
                        let is_required = t.required.contains(p);
                        format!(
                            "{}{}{}",
                            p,
                            if is_required { "*" } else { "" },
                            if is_required { " (required)" } else { " (optional)" }
                        )
                    })
                    .collect();
                inline::print_line(&RLine::from(vec![
                    RSpan::raw("    "),
                    RSpan::styled(
                        "Params: ",
                        RStyle::default().add_modifier(RModifier::DIM),
                    ),
                    RSpan::raw(params.join(", ")),
                ]));
            }
            inline::print_blank();
        }
    }

    // ── Switch model ────────────────────────────────────────

    /// Switch to a model by name (partial match ok).
    /// Returns (provider_name, model_name) if switched, or error string.
    pub fn switch_model(&mut self, name: &str) -> Result<(String, String), String> {
        let name_lower = name.to_lowercase();

        // Find matching (provider_idx, model_idx)
        let mut found: Option<(usize, usize)> = None;
        for (pi, provider) in self.config.providers.iter().enumerate() {
            for (mi, model) in provider.models.iter().enumerate() {
                let display = model.display_name.as_deref().unwrap_or(&model.model);
                if model.model.to_lowercase().contains(&name_lower)
                    || display.to_lowercase().contains(&name_lower)
                {
                    found = Some((pi, mi));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }

        let (pi, mi) = found.ok_or_else(|| format!("Model '{}' not found", name))?;

        // Reorder: move selected provider to front, selected model to front
        let mut providers = self.config.providers.clone();
        let mut provider = providers.remove(pi);
        let model = provider.models.remove(mi);
        provider.models.insert(0, model);
        providers.insert(0, provider);
        self.config.providers = providers;

        // Save config
        self.config.save().map_err(|e| format!("Failed to save config: {}", e))?;

        // Reset orchestrator so it reinitializes with new model
        self.orchestrator = None;

        let active_provider = self.config.providers[0].name.clone();
        let active_model = self.config.providers[0].models[0]
            .display_name
            .clone()
            .unwrap_or_else(|| self.config.providers[0].models[0].model.clone());

        Ok((active_provider, active_model))
    }

    /// Interactive model picker using dialoguer.
    /// Returns (provider_name, model_name) if switched, None if cancelled.
    pub fn pick_model_interactive(&mut self) -> Option<(String, String)> {
        use dialoguer::{theme::ColorfulTheme, FuzzySelect};

        // Build flat list of (rendered_label, provider_idx, model_idx, is_active).
        //
        // Each label is composed as a ratatui `Line<Span>` so styling stays
        // consistent with the rest of the CLI, then converted to an ANSI
        // string for dialoguer (which only accepts `&str`). `line_to_ansi`
        // honors capability detection, so labels stay plain when stdout
        // isn't a TTY or `--color=never` was passed.
        let mut items: Vec<(String, usize, usize, bool)> = Vec::new();
        let active_provider = self.config.active_provider().map(|p| p.name.clone());
        let active_model = self.config.active_model().map(|m| m.model.clone());

        let active_marker_style = RStyle::default()
            .fg(RColor::Green)
            .add_modifier(RModifier::BOLD);
        let dim = RStyle::default().add_modifier(RModifier::DIM);

        for (pi, provider) in self.config.providers.iter().enumerate() {
            for (mi, model) in provider.models.iter().enumerate() {
                let display = model.display_name.as_deref().unwrap_or(&model.model);
                let is_active = active_provider.as_deref() == Some(&provider.name)
                    && active_model.as_deref() == Some(&model.model);

                let marker = if is_active {
                    RSpan::styled("✓ ", active_marker_style)
                } else {
                    RSpan::raw("  ")
                };

                let line = RLine::from(vec![
                    marker,
                    RSpan::raw(display.to_string()),
                    RSpan::raw(" "),
                    RSpan::styled(format!("[{}]", provider.name), dim),
                ]);

                items.push((inline::line_to_ansi(&line), pi, mi, is_active));
            }
        }

        if items.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("No models configured."));
            inline::print_blank();
            return None;
        }

        // Default to the active model's row — by structured flag, not by
        // string scanning the (possibly ANSI-prefixed) label.
        let default = items
            .iter()
            .position(|(_, _, _, is_active)| *is_active)
            .unwrap_or(0);

        let labels: Vec<&str> = items.iter().map(|(l, _, _, _)| l.as_str()).collect();

        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("Select model")
            .default(default)
            .items(&labels)
            .interact_opt()
            .ok()??;

        let (_, pi, mi, _) = &items[selection];
        let name = self.config.providers[*pi].models[*mi].model.clone();
        match self.switch_model(&name) {
            Ok(result) => Some(result),
            Err(e) => {
                inline::print_blank();
                inline::print_line(&components::error_badge(&e));
                inline::print_blank();
                None
            }
        }
    }

    // ── MCP status (for REPL /mcp) ──────────────────────────

    pub fn show_mcp_status(&self) {
        inline::print_blank();

        if self.config.mcp_servers.is_empty() {
            inline::print_line(&components::warning_badge("No MCP servers configured."));
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  Add servers in your config file: "),
                RSpan::raw(Config::config_path().display().to_string()),
            ]));
            inline::print_blank();
            return;
        }

        inline::print_line(&components::section_header(
            "📡",
            &format!("MCP Servers ({})", self.config.mcp_servers.len()),
            RColor::Cyan,
        ));
        inline::print_blank();

        let bold = RStyle::default().add_modifier(RModifier::BOLD);
        for (name, srv) in &self.config.mcp_servers {
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  "),
                RSpan::styled(name.clone(), bold),
            ]));
            if let Some(cmd) = &srv.command {
                inline::print_line(&RLine::from(format!("    Command: {}", cmd)));
            }
            if let Some(args) = &srv.args {
                if !args.is_empty() {
                    inline::print_line(&RLine::from(format!(
                        "    Args:    {}",
                        args.join(" ")
                    )));
                }
            }
            if let Some(url) = &srv.url {
                inline::print_line(&RLine::from(format!("    URL:     {}", url)));
            }
            inline::print_blank();
        }
    }

    // ── Memory search (for REPL /search) ────────────────────

    /// Render a `/search <query>` result against the current orchestrator's
    /// conversation memory. No-op (with a hint) if the orchestrator hasn't
    /// been initialized yet — i.e. before the first turn.
    pub fn search_memory_inline(&self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Search query is empty."));
            inline::print_blank();
            return;
        }

        let orch = match self.orchestrator.as_ref() {
            Some(o) => o,
            None => {
                inline::print_blank();
                inline::print_line(&components::warning_badge(
                    "No conversation history yet — send a message first.",
                ));
                inline::print_blank();
                return;
            }
        };

        let hits = orch.search_memory(query);

        inline::print_blank();
        inline::print_line(&components::section_header(
            "🔍",
            &format!("Memory search: \"{}\" — {} match(es)", query, hits.len()),
            RColor::Cyan,
        ));
        inline::print_blank();

        if hits.is_empty() {
            inline::print_line(&components::warning_badge(
                "No messages match. Try a shorter query or different keyword.",
            ));
            inline::print_blank();
            return;
        }

        let bold = RStyle::default().add_modifier(RModifier::BOLD);
        let dim = RStyle::default().add_modifier(RModifier::DIM);

        for (i, (role, content)) in hits.iter().enumerate() {
            let (role_label, role_color) = match role {
                core_agentic::MessageRole::User => ("user", RColor::Cyan),
                core_agentic::MessageRole::Assistant => ("assistant", RColor::Green),
                core_agentic::MessageRole::System => ("system", RColor::Yellow),
                core_agentic::MessageRole::Tool { tool_name, .. } => {
                    (tool_name.as_str(), RColor::Magenta)
                }
            };

            inline::print_line(&RLine::from(vec![
                RSpan::styled(format!("  [{}] ", i + 1), dim),
                RSpan::styled(
                    role_label.to_string(),
                    RStyle::default()
                        .fg(role_color)
                        .add_modifier(RModifier::BOLD),
                ),
            ]));

            for snippet in extract_match_snippets(content, query, 80, 2) {
                inline::print_line(&RLine::from(vec![
                    RSpan::raw("      "),
                    RSpan::styled(snippet, bold),
                ]));
            }
            inline::print_blank();
        }
    }

    // ── Plan (for REPL /plan) ───────────────────────────

    /// Plan a goal through `core_agentic::PlannerAgent`: ask the model
    /// for a step-by-step plan, render it for the operator, and ask for
    /// approval before executing the steps.
    ///
    /// This replaces the older 'just-prefix-the-prompt' fallback. The
    /// orchestrator and tool registry are reused from the current
    /// session so plan steps run with the same safety + permissions as
    /// regular turns.
    pub async fn plan_inline(&mut self, goal: &str) -> anyhow::Result<()> {
        let goal = goal.trim();
        if goal.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Plan goal is empty."));
            inline::print_blank();
            return Ok(());
        }

        self.ensure_orchestrator()?;

        // Build a fresh PlannerAgent against the same provider.
        let provider_config = self
            .config
            .to_provider_config()
            .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;
        let provider: std::sync::Arc<dyn core_agentic::LLMProvider> =
            std::sync::Arc::new(core_agentic::OpenAIProvider::new(provider_config));
        let planner = core_agentic::PlannerAgent::new(provider);

        // Reuse the orchestrator's tool registry so steps see the same
        // tool surface (including allowlist + tracker).
        let tools = match self.orchestrator.as_ref() {
            Some(o) => o.tool_registry().clone(),
            None => {
                let tracker = std::sync::Arc::new(
                    core_agentic::file_tracker::FileTracker::new(),
                );
                let registry = ToolRegistry::new();
                for t in core_agentic::tools::builtin_tools_with(
                    tracker,
                    self.config.url_policy(),
                ) {
                    registry.register(t);
                }
                registry
            }
        };

        inline::print_blank();
        inline::print_line(&components::section_header(
            "🗺️",
            &format!("Planning: {}", goal),
            RColor::Cyan,
        ));
        inline::print_blank();

        let plan = match planner.create_plan(goal, &tools) {
            Ok(p) => p,
            Err(e) => {
                inline::print_line(&components::error_badge(&format!(
                    "Failed to create plan: {}",
                    e
                )));
                inline::print_blank();
                return Ok(());
            }
        };

        // Render the plan as a numbered list.
        let dim = RStyle::default().add_modifier(RModifier::DIM);
        let bold = RStyle::default().add_modifier(RModifier::BOLD);
        for (i, step) in plan.steps.iter().enumerate() {
            let mut spans = vec![
                RSpan::styled(format!("  {:>2}. ", i + 1), dim),
                RSpan::styled(step.description.clone(), bold),
            ];
            if let Some(ref tool) = step.tool {
                spans.push(RSpan::styled(
                    format!("  [↳ {}]", tool),
                    RStyle::default().fg(RColor::Cyan),
                ));
            }
            inline::print_line(&RLine::from(spans));
            if !step.depends_on.is_empty() {
                inline::print_line(&RLine::from(vec![
                    RSpan::raw("      "),
                    RSpan::styled(
                        format!("depends on steps: {:?}", step.depends_on),
                        dim,
                    ),
                ]));
            }
        }
        inline::print_blank();

        // Approval prompt. Re-use the existing dialoguer Confirm
        // since the planner's approval is binary.
        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Execute this plan?")
            .default(false)
            .interact()
            .unwrap_or(false);
        if !proceed {
            inline::print_line(&components::warning_badge("Plan rejected. Nothing executed."));
            inline::print_blank();
            return Ok(());
        }

        // Execute. The planner emits ToolCall/ToolOutput events; we don't
        // hook them up here for inline mode (keeping output simple for
        // the first cut). Future work can pipe them through the same
        // event renderer the run() path uses.
        let mut plan = plan;
        match planner.execute_plan(&mut plan, &tools) {
            Ok(result) => {
                inline::print_blank();
                inline::print_line(&components::section_header(
                    "✅",
                    &format!(
                        "Plan complete — {} succeeded, {} failed",
                        result.steps_completed, result.steps_failed
                    ),
                    RColor::Green,
                ));
                inline::print_blank();
            }
            Err(e) => {
                inline::print_blank();
                inline::print_line(&components::error_badge(&format!(
                    "Plan execution failed: {}",
                    e
                )));
                inline::print_blank();
            }
        }
        Ok(())
    }

    // ── Examples ────────────────────────────────────────────

    pub fn examples(&self) {
        let yellow_comment = RStyle::default().fg(RColor::Yellow);

        inline::print_blank();
        inline::print_line(&components::section_header(
            "📖",
            "Agentic CLI — Usage Examples",
            RColor::Cyan,
        ));
        inline::print_blank();

        let groups: &[(&str, &[&str])] = &[
            (
                "# Run a single task",
                &[
                    "agentic run \"list all Rust files\"",
                    "agentic run \"create hello.txt with 'hello world'\"",
                    "agentic run \"explain the codebase structure\"",
                ],
            ),
            (
                "# Interactive mode",
                &["agentic interactive", "agentic i"],
            ),
            (
                "# Config management",
                &[
                    "agentic config init                    # Default config",
                    "agentic config init --interactive      # Guided wizard",
                    "agentic config init --provider openai  # Quick setup",
                    "agentic config show",
                    "agentic config show --format table",
                    "agentic config edit",
                    "agentic config validate",
                    "agentic config backup",
                    "agentic config export                  # Masked secrets",
                ],
            ),
            (
                "# Status & info",
                &["agentic status", "agentic version"],
            ),
        ];

        for (heading, lines) in groups {
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  "),
                RSpan::styled((*heading).to_string(), yellow_comment),
            ]));
            for cmd in *lines {
                inline::print_line(&RLine::from(format!("  {}", cmd)));
            }
            inline::print_blank();
        }
    }

    // ── Run task ────────────────────────────────────────────

    pub async fn run(&mut self, task: &str) -> Result<()> {
        use crate::widgets::{components, inline, markdown as md_widget, progress, spinner};
        use ratatui::style::Color as RColor;

        // Expand @file references before sending to AI
        let expanded = crate::file_ref::expand_file_refs(task);
        let task = &expanded;

        self.ensure_orchestrator()?;

        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;

        // Fresh run: clear any pending cancel from a previous invocation.
        orchestrator.reset_cancel();
        // Drop event handlers from any previous run so we don't accumulate
        // subscribers across invocations.
        orchestrator.clear_event_handlers();

        // Subscribe to runtime events (tool calls, results) so we can
        // render them between spinner ticks. Gated by config.output.show_tool_calls.
        let show_tool_calls = self.config.output.show_tool_calls;
        // Verbose body for tool results piggybacks on `show_thoughts`:
        // it's the same intent (render the agent's internal trace, not
        // just the final answer).
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        if show_tool_calls {
            let tx = event_tx.clone();
            orchestrator.on_event(move |event| {
                // Best-effort send: if the receiver is dropped (run finished),
                // silently ignore.
                let _ = tx.send(event);
            });
        }
        // Drop our local sender so the receiver shuts down once the
        // orchestrator's handler is also gone (after `clear_event_handlers`).
        drop(event_tx);

        // Live transient spinner: a background task ticks the frame and
        // redraws via `inline::print_transient` (no-op when not a TTY).
        // The same task drains incoming events and renders them inline,
        // briefly clearing the spinner so the events land in scrollback.
        let progress = std::sync::Arc::new(std::sync::Mutex::new({
            let mut p = progress::ProgressState::new();
            p.start();
            p.set_message("Thinking…".to_string());
            p
        }));
        let stop_flag = std::sync::Arc::new(AtomicBool::new(false));

        let tick_progress = progress.clone();
        let tick_stop = stop_flag.clone();
        let ticker = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(80));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tick_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let line = {
                            let mut p = tick_progress.lock().unwrap();
                            p.tick();
                            // Compact line = spinner + message + animated bar.
                            // No elapsed seconds — motion comes from the bar.
                            spinner::compact_progress_line(&p, 18)
                        };
                        inline::print_transient(&line);
                    }
                    maybe_event = event_rx.recv() => {
                        match maybe_event {
                            Some(event) => {
                                // Pause the spinner, render the event, then
                                // let the next tick redraw the spinner.
                                inline::clear_transient();
                                render_event(&event);
                            }
                            None => {
                                // Channel closed and drained. Keep ticking
                                // (or break if stop is set) without panic.
                                if tick_stop.load(Ordering::Relaxed) {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            // Drain anything still queued so we don't lose late events.
            while let Ok(event) = event_rx.try_recv() {
                inline::clear_transient();
                render_event(&event);
            }
        });

        let result = orchestrator
            .run_stream(task, |_chunk| {
                // Discard chunks; we render the final result once at the end.
            })
            .await;

        // Stop ticker. Dropping orchestrator's handler is what eventually
        // closes the receiver — do that by clearing handlers (the sender
        // captured by the closure goes away with the closure).
        orchestrator.clear_event_handlers();
        stop_flag.store(true, Ordering::Relaxed);
        let _ = ticker.await;
        progress.lock().unwrap().stop();
        inline::clear_transient();

        match result {
            Ok(final_result) => {
                inline::print_blank();
                inline::print_line(&components::section_header(
                    "🤖",
                    "Response",
                    RColor::Rgb(64, 224, 208),
                ));
                inline::print_blank();

                let parsed = md_widget::MarkdownContent::parse(&final_result);
                inline::print_lines(&parsed.lines);
                inline::print_blank();
            }
            Err(e) => {
                inline::print_blank();
                inline::print_line(&components::error_badge(&e.to_string()));
                inline::print_blank();
            }
        }

        Ok(())
    }


    /// Run task with separate callbacks for streaming chunks and runtime
    /// events (tool calls + results). Used by the TUI to surface tool
    /// activity in the message log alongside the streaming response.
    ///
    /// `on_chunk` receives token deltas as the model generates them.
    /// `on_event` receives `core_agentic::Event` for every tool call and
    /// result emitted by the orchestrator.
    ///
    /// Both callbacks must be `Send + Sync + 'static` because the event
    /// stream is dispatched on blocking-pool threads.
    pub async fn run_with_callbacks<C, E>(
        &mut self,
        task: &str,
        mut on_chunk: C,
        on_event: E,
    ) -> Result<String>
    where
        C: FnMut(&str),
        E: Fn(core_agentic::Event) + Send + Sync + 'static,
    {
        let expanded = crate::file_ref::expand_file_refs(task);
        let task = &expanded;

        self.ensure_orchestrator()?;

        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;
        orchestrator.reset_cancel();
        // Subscribers from a previous run shouldn't leak into this one.
        orchestrator.clear_event_handlers();
        orchestrator.on_event(on_event);

        let result = orchestrator
            .run_stream(task, |chunk| on_chunk(&chunk))
            .await;

        // Drop our subscriber once the run completes so its captured
        // sender doesn't keep the receiver open forever.
        orchestrator.clear_event_handlers();

        Ok(result?)
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

        inline::print_line(&RLine::from(vec![
            RSpan::raw("  "),
            RSpan::styled(
                "Providers:",
                RStyle::default().add_modifier(RModifier::BOLD),
            ),
        ]));
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

        inline::print_line(&RLine::from(vec![
            RSpan::raw("  "),
            RSpan::styled(
                "Safety:",
                RStyle::default().add_modifier(RModifier::BOLD),
            ),
        ]));
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

        inline::print_line(&RLine::from(vec![
            RSpan::raw("  "),
            RSpan::styled(
                "Output:",
                RStyle::default().add_modifier(RModifier::BOLD),
            ),
        ]));
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
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  "),
                RSpan::styled(
                    "MCP Servers:",
                    RStyle::default().add_modifier(RModifier::BOLD),
                ),
            ]));
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
        inline::print_blank();
        inline::print_line(&components::banner_title(
            "🤖 Agentic Config Wizard",
            RColor::Cyan,
            RColor::Magenta,
        ));
        inline::print_blank();

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
                allowed_domains: vec![],
                block_ip_urls: false,
            },
            output: core_agentic::OutputConfig {
                color: true,
                stream: true,
                show_thoughts: true,
                show_tool_calls: true,
            },
            mcp_servers: std::collections::HashMap::new(),
            system_prompt: None,
            agent: core_agentic::AgentLoopConfig::default(),
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
            system_prompt: None,
            agent: core_agentic::AgentLoopConfig::default(),
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
                format!("Provider '{}'", provider.name)
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

// ── Inline event rendering helper (used by run()'s ticker task) ───────────
//
// Free function so it can be called from inside the spawned tokio task
// without moving a closure (and therefore without capturing &self).

fn render_event(event: &core_agentic::Event) {
    use crate::widgets::{components, diff as diff_widget, inline, tool_call};

    // Compact rendering by default: success notifications show only the
    // headline (tool name + numeric summary). Errors always include their
    // message body so users can debug without flipping a flag. The full
    // raw output is still in memory for the model.
    //
    // Special case: edit_file / write_file results carry a `diff` field
    // (unified-diff string) and `lines_added` / `lines_removed` counts.
    // We render those through `widgets::diff` so the user sees a real
    // colored diff inline instead of the raw JSON blob.
    const MAX_TOOL_OUTPUT_LINES: usize = 12;
    match event {
        core_agentic::Event::ToolCall { tool_name, arguments } => {
            let lines = tool_call::render_call(tool_name, arguments);
            inline::print_lines(&lines);
        }
        core_agentic::Event::ToolOutput { tool_name, output } => {
            // Heuristic: orchestrator records denied/skipped/error outcomes
            // as plain string output with these prefixes. We render those
            // with the red error accent.
            let body = match output {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let is_error = body.starts_with("Tool error")
                || body.starts_with("Blocked:")
                || body.starts_with("Skipped:");

            // Try to extract a unified diff from the embedded JSON. The
            // orchestrator wraps tool results as Value::String containing
            // pretty-printed JSON; we parse it back to look for our
            // structured fields. If we find a non-empty diff, render it
            // through the diff widget; the headline notification still
            // appears so the user gets the summary line.
            let parsed: Option<serde_json::Value> = if !is_error {
                serde_json::from_str(&body).ok()
            } else {
                None
            };

            let diff_text = parsed
                .as_ref()
                .and_then(|v| v.get("diff"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());

            // Always print the headline (success/error badge).
            let lines = tool_call::render_result(
                tool_name,
                output,
                is_error,
                MAX_TOOL_OUTPUT_LINES,
                /*verbose=*/ false,
            );
            inline::print_lines(&lines);

            // If there's a diff, render the summary line + the diff body.
            if let Some(diff) = diff_text {
                inline::print_line(&diff_widget::summary_line(diff));
                let diff_lines = diff_widget::render(diff);
                let max_diff_lines = 40;
                if diff_lines.len() > max_diff_lines {
                    inline::print_lines(&diff_lines[..max_diff_lines]);
                    let remaining = diff_lines.len() - max_diff_lines;
                    inline::print_line(&ratatui::text::Line::from(
                        ratatui::text::Span::styled(
                            format!("    … {} more diff line(s) hidden", remaining),
                            ratatui::style::Style::default()
                                .add_modifier(ratatui::style::Modifier::DIM),
                        ),
                    ));
                } else {
                    inline::print_lines(&diff_lines);
                }
            }
        }
        core_agentic::Event::Error { message } => {
            inline::print_line(&components::error_badge(message));
        }
        core_agentic::Event::System { message } => {
            inline::print_line(&components::info_badge(message));
        }
        // Thought / ConfirmationRequest / Completed are surfaced via other
        // channels (memory, the confirmation prompt, the final markdown).
        _ => {}
    }
}

// ── Print helpers (shared widgets) ──────────────────────────
//  - CLI and TUI share one styling vocabulary
//  - Color/TTY decisions live in one place (`widgets::capabilities`)
fn print_success(text: &str) {
    inline::print_line(&components::success_badge(text));
}

fn print_warning(text: &str) {
    inline::print_line(&components::warning_badge(text));
}

fn print_error(text: &str, _color_enabled: bool) {
    // Errors go to stderr in plain styled text. We keep a simple format here
    // because `inline::print_line` writes to stdout; for stderr we render
    // directly with optional ANSI based on capabilities.
    if capabilities::should_use_color() {
        let mut stderr = StandardStream::stderr(termcolor::ColorChoice::Always);
        let _ = stderr.set_color(
            ColorSpec::new()
                .set_fg(Some(Color::White))
                .set_bg(Some(Color::Red)),
        );
        eprint!("  ✗ {} ", text);
        let _ = stderr.reset();
        eprintln!();
    } else {
        eprintln!("  ✗ {}", text);
    }
}

fn print_info(text: &str) {
    inline::print_line(&components::info_badge(text));
}

/// Extract up to `max_snippets` short windows around occurrences of
/// `query` in `content`. Each window is bounded by `max_chars`, breaks
/// on UTF-8 boundaries, and is prefixed/suffixed with an ellipsis when
/// truncated.
///
/// The match itself is preserved verbatim; we don't attempt to highlight
/// the matched substring inside the snippet (that would require a richer
/// span builder than the rest of the inline renderer expects).
fn extract_match_snippets(
    content: &str,
    query: &str,
    max_chars: usize,
    max_snippets: usize,
) -> Vec<String> {
    if query.is_empty() {
        return vec![truncate_one_line(content, max_chars)];
    }

    let lower_content = content.to_lowercase();
    let lower_query = query.to_lowercase();

    let mut out = Vec::with_capacity(max_snippets);
    let mut search_from = 0usize;
    let context = max_chars / 2;

    while out.len() < max_snippets {
        let Some(rel) = lower_content[search_from..].find(&lower_query) else {
            break;
        };
        let match_start = search_from + rel;
        let match_end = match_start + lower_query.len();

        // Window: pad `context` chars before/after the match, snapped to
        // UTF-8 boundaries.
        let start = floor_char_boundary(content, match_start.saturating_sub(context));
        let end = ceil_char_boundary(content, (match_end + context).min(content.len()));
        let mut snippet = String::new();
        if start > 0 {
            snippet.push_str("…");
        }
        snippet.push_str(content[start..end].replace('\n', " ").trim());
        if end < content.len() {
            snippet.push('…');
        }
        out.push(snippet);

        // Advance past this match so we don't loop on the same hit.
        search_from = match_end;
    }

    if out.is_empty() {
        out.push(truncate_one_line(content, max_chars));
    }
    out
}

fn truncate_one_line(s: &str, max_chars: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() <= max_chars {
        return one;
    }
    let cut = floor_char_boundary(&one, max_chars);
    format!("{}…", &one[..cut])
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    let len = s.len();
    if i >= len {
        return len;
    }
    while i < len && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod search_snippet_tests {
    use super::*;

    #[test]
    fn extract_finds_substring_case_insensitive() {
        let s = "Hello WORLD, this is a test. The world is round.";
        let snips = extract_match_snippets(s, "world", 40, 5);
        assert_eq!(snips.len(), 2);
        assert!(snips[0].to_lowercase().contains("world"));
        assert!(snips[1].to_lowercase().contains("world"));
    }

    #[test]
    fn extract_respects_max_snippets() {
        let s = "foo ".repeat(50);
        let snips = extract_match_snippets(&s, "foo", 40, 3);
        assert_eq!(snips.len(), 3);
    }

    #[test]
    fn extract_falls_back_to_truncated_line_when_no_match() {
        let s = "some content with no needle";
        let snips = extract_match_snippets(s, "needle", 10, 2);
        // "needle" *is* in s; ensure positive case still works.
        assert_eq!(snips.len(), 1);
        assert!(snips[0].to_lowercase().contains("needle"));

        let snips = extract_match_snippets("unrelated text", "missing", 8, 2);
        assert_eq!(snips.len(), 1);
        // Truncated to ~8 chars + ellipsis.
        assert!(snips[0].chars().count() <= 9);
    }

    #[test]
    fn extract_handles_utf8_boundaries() {
        let s = "你好世界 你好世界";
        let snips = extract_match_snippets(s, "世界", 6, 5);
        assert!(!snips.is_empty());
        // Must not panic and must contain the query.
        for snip in &snips {
            assert!(snip.contains("世界"));
        }
    }
}
