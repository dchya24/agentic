use anyhow::Result;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color as TColor, Table};
use core_agentic::{Config, Orchestrator, ToolRegistry};
use dialoguer::{Confirm, Input, MultiSelect, Select, theme::ColorfulTheme};
use ratatui::style::{Color as RColor, Modifier as RModifier, Style as RStyle};
use ratatui::text::{Line as RLine, Span as RSpan};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::{ConfigAction, OutputFormat, SkillAction};
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::error::CommandError;
use crate::widgets::capabilities;
use crate::widgets::{components, inline};

static ALWAYS_CONFIRM: AtomicBool = AtomicBool::new(false);
static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);

// ── Interactive tool handlers ───────────────────────────────

/// Handles `question` tool calls from the agent during interactive mode.
///
/// Renders each question with a styled prompt, presents options via
/// `dialoguer::Select` / `MultiSelect` when choices exist, and falls back
/// to free-text `Input` otherwise. The handler is synchronous (matches
/// the `Tool::execute` contract) and reads directly from stdin.
struct CliQuestionHandler;

impl core_agentic::QuestionHandler for CliQuestionHandler {
    fn handle(
        &self,
        questions: &[core_agentic::QuestionPrompt],
    ) -> Vec<core_agentic::QuestionAnswer> {
        let mut answers = Vec::with_capacity(questions.len());

        for q in questions {
            // Render a visual separator + question header.
            inline::print_blank();

            let header = q.header.as_deref().unwrap_or("Question");
            inline::print_line(&components::section_header(
                "❓",
                header,
                RColor::Rgb(241, 196, 15),
            ));
            inline::print_blank();

            // Print the question text.
            inline::print_line(&RLine::from(vec![
                RSpan::styled("  ", RStyle::default()),
                RSpan::raw(&q.question),
            ]));
            inline::print_blank();

            let answer = if !q.options.is_empty() {
                // Agent provided pre-defined choices.
                if q.multiple {
                    // Multi-select.
                    let defaults: Vec<bool> = vec![false; q.options.len()];
                    let selection = MultiSelect::with_theme(&ColorfulTheme::default())
                        .with_prompt("Select (space to toggle, enter to confirm)")
                        .items(&q.options)
                        .defaults(&defaults)
                        .interact();

                    match selection {
                        Ok(indices) => {
                            let chosen: Vec<String> = indices
                                .iter()
                                .map(|&i| q.options[i].clone())
                                .collect();

                            if chosen.is_empty() && q.custom {
                                // Nothing selected but custom allowed.
                                free_text_fallback(&q.question)
                            } else if chosen.is_empty() {
                                // Nothing selected and no custom — skip.
                                skip_answer(&q.question)
                            } else {
                                render_answer(&format!(
                                    "{}",
                                    chosen.join(", ")
                                ));
                                vec![core_agentic::QuestionAnswer {
                                    question: q.question.clone(),
                                    answer: chosen,
                                    skipped: false,
                                }]
                            }
                        }
                        Err(_) => skip_answer(&q.question),
                    }
                } else {
                    // Single-select.
                    let mut items: Vec<String> = q.options.clone();
                    if q.custom {
                        items.push("✏️  Custom (type your own)".to_string());
                    }
                    items.push("⏭  Skip".to_string());

                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Choose")
                        .items(&items)
                        .default(0)
                        .interact();

                    match selection {
                        Ok(idx) => {
                            // Last item is always "Skip".
                            let skip_idx = items.len() - 1;
                            if idx == skip_idx {
                                skip_answer(&q.question)
                            } else if q.custom && idx == items.len() - 2 {
                                // "Custom" was selected.
                                free_text_fallback(&q.question)
                            } else {
                                let chosen = q.options[idx].clone();
                                render_answer(&chosen);
                                vec![core_agentic::QuestionAnswer {
                                    question: q.question.clone(),
                                    answer: vec![chosen],
                                    skipped: false,
                                }]
                            }
                        }
                        Err(_) => skip_answer(&q.question),
                    }
                }
            } else {
                // No options — free-text input.
                free_text_fallback(&q.question)
            };

            answers.extend(answer);
        }

        answers
    }
}

/// Render a free-text input prompt for the given question.
fn free_text_fallback(question: &str) -> Vec<core_agentic::QuestionAnswer> {
    let result = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Your answer (enter to submit, Ctrl+C to skip)")
        .allow_empty(true)
        .interact();

    match result {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                skip_answer(question)
            } else {
                render_answer(&text);
                vec![core_agentic::QuestionAnswer {
                    question: question.to_string(),
                    answer: vec![text],
                    skipped: false,
                }]
            }
        }
        Err(_) => skip_answer(question),
    }
}

/// Produce a single skipped answer.
fn skip_answer(question: &str) -> Vec<core_agentic::QuestionAnswer> {
    inline::print_blank();
    inline::print_line(&components::warning_badge("Skipped."));
    inline::print_blank();
    vec![core_agentic::QuestionAnswer {
        question: question.to_string(),
        answer: vec![],
        skipped: true,
    }]
}

/// Echo the chosen answer back to the user.
fn render_answer(text: &str) {
    inline::print_blank();
    inline::print_line(&components::success_badge(&format!("Answered: {}", text)));
    inline::print_blank();
}

/// Renders todo list changes inline. Fires after every `todowrite` call.
/// Shows a compact progress summary so the user sees task progress even
/// in non-interactive `agentic run` mode.
struct CliTodoRenderer;

impl core_agentic::TodoChangeHandler for CliTodoRenderer {
    fn on_change(&self, todos: &[core_agentic::TodoItem]) {
        if todos.is_empty() {
            return;
        }

        let total = todos.len();
        let completed = todos
            .iter()
            .filter(|t| t.status == core_agentic::TodoStatus::Completed)
            .count();
        let in_progress = todos
            .iter()
            .filter(|t| t.status == core_agentic::TodoStatus::InProgress)
            .count();
        let pct = if total > 0 {
            (completed as f64 / total as f64 * 100.0) as u32
        } else {
            0
        };

        // Build a compact status bar.
        //    📋 Tasks: 3/7 (43%)  ● 1 active  ○ 3 pending
        let mut spans: Vec<RSpan<'static>> = vec![
            RSpan::styled(
                "  📋 ",
                RStyle::default(),
            ),
            RSpan::styled(
                format!("Tasks: {}/{} ({}%)", completed, total, pct),
                RStyle::default()
                    .fg(RColor::Rgb(241, 196, 15))
                    .add_modifier(RModifier::BOLD),
            ),
        ];

        if in_progress > 0 {
            spans.push(RSpan::styled(
                format!("  ● {} active", in_progress),
                RStyle::default().fg(RColor::Rgb(135, 206, 250)),
            ));
        }

        let pending = total - completed - in_progress;
        if pending > 0 {
            spans.push(RSpan::styled(
                format!("  ○ {} pending", pending),
                RStyle::default().fg(RColor::Rgb(120, 120, 140)),
            ));
        }

        // Render individual items (compact, truncated).
        let mut item_lines: Vec<RLine<'static>> = Vec::new();
        for todo in todos {
            let (icon, color) = match todo.status {
                core_agentic::TodoStatus::Completed => ("✓", RColor::Rgb(46, 204, 113)),
                core_agentic::TodoStatus::InProgress => ("●", RColor::Rgb(135, 206, 250)),
                core_agentic::TodoStatus::Pending => ("○", RColor::Rgb(120, 120, 140)),
                core_agentic::TodoStatus::Cancelled => ("✗", RColor::Rgb(120, 120, 140)),
            };

            let priority_marker = match todo.priority {
                core_agentic::TodoPriority::High => " ❗",
                core_agentic::TodoPriority::Low => " ↓",
                _ => "",
            };

            let mut content = todo.content.clone();
            if content.len() > 80 {
                content.truncate(77);
                content.push_str("...");
            }

            item_lines.push(RLine::from(vec![
                RSpan::raw("    "),
                RSpan::styled(
                    format!("{} ", icon),
                    RStyle::default().fg(color).add_modifier(RModifier::BOLD),
                ),
                RSpan::styled(
                    content,
                    RStyle::default().fg(RColor::Rgb(200, 200, 210)),
                ),
                RSpan::styled(
                    priority_marker.to_string(),
                    RStyle::default()
                        .fg(RColor::Rgb(231, 76, 60))
                        .add_modifier(RModifier::DIM),
                ),
            ]));
        }

        inline::print_blank();
        inline::print_line(&RLine::from(spans));
        for line in &item_lines {
            inline::print_line(line);
        }
        inline::print_blank();
    }
}

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
    /// Image attachments queued by the `/image <path>` slash command,
    /// to ride along with the next user turn. Drained by `run()` when
    /// the next message is sent.
    pending_attachments: Vec<core_agentic::Attachment>,
    /// `true` when running in interactive REPL mode. Controls whether
    /// the `question` tool handler is registered (stdin-based prompts)
    /// vs returning skip-all fallback (non-interactive `agentic run`).
    interactive_mode: bool,
    /// Shared skill index for the skill tool, populated at orchestrator init.
    skill_index: Option<std::sync::Arc<std::sync::RwLock<core_agentic::SkillIndex>>>,
    /// Mock provider for testing. When set, `ensure_orchestrator` uses
    /// this instead of constructing a real provider from config.
    /// Only settable via `with_mock_provider` (gated to `#[cfg(test)]`).
    mock_provider: Option<std::sync::Arc<dyn core_agentic::LLMProvider>>,
    /// Shared input watcher state. When set, the spinner ticker renders
    /// a two-line transient area (spinner + input buffer).
    watcher_state:
        Option<std::sync::Arc<std::sync::Mutex<crate::input_watcher::WatcherState>>>,
}

impl Default for Commands {
    fn default() -> Self {
        Self::new(Config::fallback())
    }
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
            pending_attachments: Vec::new(),
            interactive_mode: false,
            skill_index: None,
            mock_provider: None,
            watcher_state: None,
        }
    }

    pub fn with_color(mut self, enabled: bool) -> Self {
        self.color_enabled = enabled;
        COLOR_ENABLED.store(enabled, Ordering::Relaxed);
        self
    }

    /// Get a reference to the config
    pub(crate) fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn with_interactive_mode(mut self, enabled: bool) -> Self {
        self.interactive_mode = enabled;
        self
    }

    /// Inject a mock provider for end-to-end testing. Only available
    /// in test builds.
    #[cfg(test)]
    pub(crate) fn with_mock_provider(
        mut self,
        provider: std::sync::Arc<dyn core_agentic::LLMProvider>,
    ) -> Self {
        self.mock_provider = Some(provider);
        self
    }

    pub fn with_debug(mut self, enabled: bool) -> Self {
        self.debug_enabled = enabled;
        self
    }

    /// Attach the input watcher's shared state so the spinner ticker
    /// can render the live input buffer below the progress line.
    pub fn with_watcher_state(
        mut self,
        state: Option<std::sync::Arc<std::sync::Mutex<crate::input_watcher::WatcherState>>>,
    ) -> Self {
        self.watcher_state = state;
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

        let provider: Arc<dyn core_agentic::LLMProvider>;
        let model_name: String;

        if let Some(mock) = self.mock_provider.clone() {
            provider = mock;
            model_name = self
                .config
                .active_model()
                .map(|m| m.model.clone())
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
        } else {
            let provider_config = self
                .config
                .to_provider_config()
                .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;
            model_name = provider_config.default_model.clone();
            provider = Arc::new(core_agentic::OpenAIProvider::new(provider_config));
        }

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

        // Discover skills and build the shared skill index before registering
        // the skill tool (so the tool has the index available immediately).
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let discovery_config: core_agentic::DiscoveryConfig = (&self.config.skills).into();
        let skill_index = core_agentic::discover_skills(&discovery_config);
        let skill_index_arc = std::sync::Arc::new(std::sync::RwLock::new(skill_index));
        self.skill_index = Some(skill_index_arc.clone());

        // Register the skill tool.
        tools.register(Box::new(core_agentic::SkillTool::new(skill_index_arc)));

        let mut orchestrator = Orchestrator::new(provider, tools);
        orchestrator.set_model(model_name);

        // Wire the process-global cancel flag so Ctrl+C in main.rs flips
        // the same atomic the orchestrator polls between turns.
        orchestrator.set_cancel_handle(crate::cancel_flag());

        // Assemble effective system prompt:
        //   default baseline  +  AGENT.md from cwd  +  skills section  +  config-provided override
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

        // Generate skills section for system prompt from the discovered index.
        let skills_section = {
            let idx = self.skill_index.as_ref().unwrap().read().unwrap();
            let skill_pairs: Vec<(&str, &str)> = idx
                .all()
                .iter()
                .map(|s| (s.name(), s.description()))
                .collect();
            core_agentic::skills_system_section(&skill_pairs)
        };

        let assembled = core_agentic::assemble_system_prompt(
            None, // use DEFAULT_SYSTEM_PROMPT
            project_instructions.as_deref(),
            skills_section.as_deref(),
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
        if let Some(max_iter) = self.config.agent.max_iterations {
            orchestrator.set_max_iterations(max_iter);
            tracing::info!(max_iterations = max_iter, "Max iterations override");
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

        // ── Interactive tool handlers ─────────────────────
        // Only register the question handler in interactive mode.
        // In non-interactive `agentic run`, the tool returns skip-all
        // so the agent proceeds without blocking on stdin.
        if self.interactive_mode {
            core_agentic::set_question_handler(Box::new(CliQuestionHandler));
        }

        // Register the todo change handler in all modes. Even in
        // non-interactive `agentic run`, the user benefits from seeing
        // task progress rendered inline.
        core_agentic::set_todo_change_handler(Box::new(CliTodoRenderer));

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

    /// Resolve the active model's capabilities (vision / tools /
    /// streaming). Reads the per-model override first, then falls back
    /// to the built-in lookup.
    pub fn active_model_capabilities(&self) -> core_agentic::ModelCapabilities {
        if let Some(model) = self.config.active_model() {
            return model.effective_capabilities();
        }
        core_agentic::ModelCapabilities::default()
    }

    /// Drain the queue of `/image`-attached payloads. The queue is
    /// per-command-run — once attached to a turn, the buffer empties.
    pub fn drain_pending_attachments(&mut self) -> Vec<core_agentic::Attachment> {
        std::mem::take(&mut self.pending_attachments)
    }

    /// Number of images queued for the next turn (for status display).
    #[allow(dead_code)]
    pub fn pending_attachment_count(&self) -> usize {
        self.pending_attachment_count_inner()
    }

    fn pending_attachment_count_inner(&self) -> usize {
        self.pending_attachments.len()
    }

    /// Queue an already-loaded attachment for the next turn.
    /// Used by the TUI `/image` handler that loads the file itself.
    pub fn queue_attachment(&mut self, att: core_agentic::Attachment) {
        self.pending_attachments.push(att);
    }

    /// Read-only reference to the current configuration.
    /// Used by TUI commands that need provider/model lists.
    pub fn config_ref(&self) -> &Config {
        &self.config
    }

    /// Render the `/image <path>` slash command: load the file, validate
    /// it (size cap + MIME), and queue it for the next user turn. The
    /// next call to `run()` will attach all queued images to the
    /// outgoing message.
    pub fn attach_image_inline(&mut self, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Usage: /image <path>"));
            inline::print_blank();
            return;
        }

        // Pre-flight capability check so the user sees the failure now,
        // not on the next turn.
        let caps = self.active_model_capabilities();
        if !caps.vision {
            inline::print_blank();
            inline::print_line(&components::error_badge(
                "Active model does not support image input.",
            ));
            inline::print_line(&RLine::from(vec![
                RSpan::raw("  Switch with "),
                RSpan::styled(
                    "/models",
                    RStyle::default().add_modifier(RModifier::BOLD),
                ),
                RSpan::raw(" to a vision-capable model first."),
            ]));
            inline::print_blank();
            return;
        }

        let limits = core_agentic::AttachmentLimits::default();
        let result = if path.starts_with("http://")
            || path.starts_with("https://")
            || path.starts_with("data:")
        {
            core_agentic::attachments::load_image_from_url(path, limits)
        } else {
            core_agentic::attachments::load_image_from_path(path, limits)
        };

        match result {
            Ok(att) => {
                let bytes = att.size_bytes;
                let mime = att.mime_type.clone();
                let source = format!("{}", att.source);
                self.pending_attachments.push(att);
                inline::print_blank();
                inline::print_line(&components::success_badge(&format!(
                    "Attached: {} ({} bytes, {})",
                    source,
                    bytes,
                    if mime.is_empty() { "remote" } else { mime.as_str() }
                )));
                inline::print_line(&RLine::from(vec![
                    RSpan::raw("  "),
                    RSpan::styled(
                        format!(
                            "{} image(s) queued for next turn.",
                            self.pending_attachments.len()
                        ),
                        RStyle::default().add_modifier(RModifier::DIM),
                    ),
                ]));
                inline::print_blank();
            }
            Err(e) => {
                inline::print_blank();
                inline::print_line(&components::error_badge(&format!(
                    "Failed to attach: {}",
                    e
                )));
                inline::print_blank();
            }
        }
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

    /// Interactive model picker using ratatui full-screen TUI.
    /// Returns (provider_name, model_name) if switched, None if cancelled.
    /// Display available models inline (non-modal).
    pub fn list_models_inline(&self) {
        inline::print_blank();
        inline::print_line(&components::section_header(
            "🤖",
            "Available Models",
            RColor::Cyan,
        ));
        inline::print_blank();

        let active_provider = self.config.active_provider().map(|p| p.name.clone());
        let active_model = self.config.active_model().map(|m| m.model.clone());

        for provider in &self.config.providers {
            let provider_style = RStyle::default()
                .fg(RColor::Rgb(255, 215, 0))
                .add_modifier(RModifier::BOLD);

            inline::print_line(&RLine::from(vec![
                RSpan::raw("  "),
                RSpan::styled(format!("📡 {}", provider.name), provider_style),
            ]));

            for model in &provider.models {
                let is_active = active_provider.as_deref() == Some(&provider.name)
                    && active_model.as_deref() == Some(&model.model);

                let display = model.display_name.as_deref().unwrap_or(&model.model);
                let caps = model.effective_capabilities();

                let mut spans = vec![RSpan::raw("    ")];

                if is_active {
                    spans.push(RSpan::styled(
                        "● ",
                        RStyle::default().fg(RColor::Green),
                    ));
                } else {
                    spans.push(RSpan::raw("  "));
                }

                spans.push(RSpan::styled(
                    display.to_string(),
                    RStyle::default().fg(RColor::Rgb(180, 180, 200)),
                ));

                if caps.vision {
                    spans.push(RSpan::styled(
                        "  👁",
                        RStyle::default().fg(RColor::Rgb(135, 206, 250)),
                    ));
                }

                spans.push(RSpan::styled(
                    format!("  ({})", model.model),
                    RStyle::default().fg(RColor::Rgb(100, 100, 120)).add_modifier(RModifier::DIM),
                ));

                inline::print_line(&RLine::from(spans));
            }
            inline::print_blank();
        }

        inline::print_line(&RLine::from(vec![
            RSpan::styled("💡 ", RStyle::default()),
            RSpan::styled("Tip: ", RStyle::default().add_modifier(RModifier::DIM)),
            RSpan::raw("Use "),
            RSpan::styled(
                "/models <name>",
                RStyle::default().fg(RColor::Rgb(255, 215, 0)).add_modifier(RModifier::BOLD),
            ),
            RSpan::raw(" to switch. Type "),
            RSpan::styled(
                "/models ",
                RStyle::default().fg(RColor::Rgb(255, 215, 0)),
            ),
            RSpan::raw("then press "),
            RSpan::styled(
                "Tab",
                RStyle::default().fg(RColor::Rgb(135, 206, 250)).add_modifier(RModifier::BOLD),
            ),
            RSpan::raw(" for completion"),
        ]));
        inline::print_blank();
    }

    /// Interactive model picker using dialoguer (fuzzy searchable).
    pub fn pick_model_interactive_inline(&mut self) -> Option<(String, String)> {
        use dialoguer::{FuzzySelect, theme::ColorfulTheme};

        let active_provider = self.config.active_provider().map(|p| p.name.clone());
        let active_model = self.config.active_model().map(|m| m.model.clone());

        // Build list of (display_string, provider_name, model_name)
        let mut items: Vec<(String, String, String)> = Vec::new();
        let mut default_idx = 0;

        for provider in &self.config.providers {
            for model in &provider.models {
                let display_name = model.display_name.as_deref().unwrap_or(&model.model);
                let caps = model.effective_capabilities();
                let vision_icon = if caps.vision { " 👁" } else { "" };

                let is_active = active_provider.as_deref() == Some(&provider.name)
                    && active_model.as_deref() == Some(&model.model);

                if is_active {
                    default_idx = items.len();
                }

                let display = format!(
                    "{}{} [{}]{}",
                    display_name,
                    vision_icon,
                    provider.name,
                    if is_active { " ●" } else { "" }
                );

                items.push((display, provider.name.clone(), model.model.clone()));
            }
        }

        if items.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("No models configured."));
            inline::print_blank();
            return None;
        }

        inline::print_blank();

        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("🤖 Select Model (type to filter, ↑↓ to navigate, Enter to select, Esc to cancel)")
            .items(&items.iter().map(|(d, _, _)| d.as_str()).collect::<Vec<_>>())
            .default(default_idx)
            .interact_opt();

        match selection {
            Ok(Some(idx)) => {
                let (_, _, model) = &items[idx];
                match self.switch_model(model) {
                    Ok(result) => Some(result),
                    Err(_) => None,
                }
            }
            Ok(None) | Err(_) => None,
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
        self.plan_and_execute(goal, true).await
    }

    /// `agentic run --plan <task>`: one-shot plan-then-execute without
    /// entering interactive mode. Respects the `require_approval` config
    /// setting (defaults to true).
    pub async fn plan_run(&mut self, task: &str) -> anyhow::Result<()> {
        let task = task.trim();
        if task.is_empty() {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Task is empty."));
            inline::print_blank();
            return Ok(());
        }

        self.ensure_orchestrator()?;
        // Use require_approval from config (default: true)
        self.plan_and_execute(task, self.config.agent.planner.require_approval).await
    }

    /// Shared planner logic: create a plan, optionally ask for approval,
    /// render live progress, and execute.
    ///
    /// `ask_approval`: when true, render the plan and prompt with
    /// dialoguer. When false, skip the prompt (auto-approve).
    async fn plan_and_execute(
        &mut self,
        goal: &str,
        ask_approval: bool,
    ) -> anyhow::Result<()> {
        // Build a fresh PlannerAgent against the same provider.
        let provider_config = self
            .config
            .to_provider_config()
            .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;
        let provider: std::sync::Arc<dyn core_agentic::LLMProvider> =
            std::sync::Arc::new(core_agentic::OpenAIProvider::new(provider_config));
        let planner = core_agentic::PlannerAgent::from_config(
            provider,
            &self.config.agent.planner,
        );

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
                    format!("  [↳{}]", tool),
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

        // Approval: either prompt with dialoguer, or auto-approve.
        let proceed = if ask_approval {
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Execute this plan?")
                .default(false)
                .interact()
                .unwrap_or(false)
        } else {
            inline::print_line(&RLine::from(vec![
                RSpan::styled(
                    "  Auto-approved (require_approval = false)\n".to_string(),
                    RStyle::default().fg(RColor::DarkGray),
                ),
            ]));
            true
        };

        if !proceed {
            inline::print_line(&components::warning_badge("Plan rejected. Nothing executed."));
            inline::print_blank();
            return Ok(());
        }

        // Execute with live progress updates via the planner's event bus.
        let mut plan = plan;
        planner.on({
            move |event: core_agentic::Event| {
                match event {
                    core_agentic::Event::PlanProgress {
                        step_description,
                        step_status,
                        steps_total,
                        steps_completed,
                        steps_failed,
                        ..
                    } => {
                        let icon = match step_status.as_str() {
                            "in_progress" => "▶",
                            "completed" => "✅",
                            "failed" => "❌",
                            _ => "⏳",
                        };
                        let bar = components::labeled_bar(
                            &format!("{}/{}", steps_completed, steps_total),
                            if steps_total > 0 { steps_completed as f32 / steps_total as f32 } else { 0.0 },
                            30,
                            if steps_failed > 0 { RColor::Red } else { RColor::Green },
                            RColor::DarkGray,
                        );
                        inline::print_line(&bar);
                        inline::print_line(&RLine::from(vec![
                            RSpan::raw(format!("       {}  {}", icon, step_description)),
                        ]));
                    }
                    core_agentic::Event::PlanReplanned {
                        reason,
                        steps_carried_over,
                        steps_total,
                        ..
                    } => {
                        inline::print_blank();
                        inline::print_line(&RLine::from(vec![
                            RSpan::styled("       🔄 ".to_string(), RStyle::default().fg(RColor::Yellow)),
                            RSpan::styled("Re-planning: ".to_string(), RStyle::default().add_modifier(RModifier::BOLD)),
                            RSpan::styled(reason.clone(), RStyle::default().fg(RColor::Yellow)),
                        ]));
                        inline::print_line(&RLine::from(vec![
                            RSpan::raw(format!(
                                "          {} carried over, {} total revised steps",
                                steps_carried_over, steps_total
                            )),
                        ]));
                        inline::print_blank();
                    }
                    _ => {}
                }
            }
        });

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

        // When --mode plan is active, route through the planner agent
        // instead of the regular orchestrator loop. This creates a real
        // plan, shows it, and executes approved steps.
        if self.permission_mode == core_agentic::PermissionMode::Plan {
            return self.plan_run(task).await;
        }

        // Expand @file references and extract any image attachments
        // (`@photo.png`) for the vision channel. Pending attachments
        // queued by `/image` are drained here too.
        let expanded = crate::file_ref::expand_with_attachments(task);
        let mut attachments = self.drain_pending_attachments();
        attachments.extend(expanded.attachments);
        let task = &expanded.text;

        if !attachments.is_empty() {
            // Pre-flight capability check so we fail fast with a clear,
            // user-actionable error instead of waiting on a provider 4xx.
            let caps = self.active_model_capabilities();
            if !caps.vision {
                inline::print_blank();
                inline::print_line(&components::error_badge(&format!(
                    "Active model does not support image input."
                )));
                inline::print_line(&RLine::from(vec![
                    RSpan::raw("  Switch with "),
                    RSpan::styled(
                        "/models",
                        RStyle::default().add_modifier(RModifier::BOLD),
                    ),
                    RSpan::raw(" to a vision-capable model (e.g. gpt-4o, claude-3-5-sonnet)."),
                ]));
                inline::print_blank();
                return Ok(());
            }
        }

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

        // ── Live rendering strategy ────────────────────────────
        //
        // We render streaming text in real-time (like pi, codex, opencode)
        // instead of batch-rendering at the end. The flow:
        //
        //  1. Spinner ticks while the model is "thinking".
        //  2. When text chunks arrive, stop spinner and print text directly.
        //  3. When tool calls arrive (after text), render tool panels,
        //     then restart the spinner for the next iteration.
        //  4. For the final response (no tool calls), text is already
        //     streamed — just print a completion marker.
        //
        // Thought events from the orchestrator are suppressed because
        // we already stream the text in real-time via on_chunk.

        // Shared state between the chunk callback and the event ticker.
        // When `true`, the ticker skips spinner ticks because the chunk
        // callback is actively printing text.
        let streaming_text_active = std::sync::Arc::new(AtomicBool::new(false));
        let streaming_text_active_clone = streaming_text_active.clone();

        // Collect the streamed text so we can re-render it as styled
        // markdown once streaming completes.
        let streamed_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let streamed_text_clone = streamed_text.clone();
        // Track how many terminal lines were printed during streaming
        // so we can MoveUp + replace them with styled markdown.
        let streamed_lines = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let streamed_lines_clone = streamed_lines.clone();

        let progress = std::sync::Arc::new(std::sync::Mutex::new({
            let mut p = progress::ProgressState::new();
            p.start();
            p.set_message("Thinking…".to_string());
            p
        }));
        let stop_flag = std::sync::Arc::new(AtomicBool::new(false));

        let tick_progress = progress.clone();
        let tick_stop = stop_flag.clone();
        let tick_watcher = self.watcher_state.clone();
        let ticker = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(80));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tick_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        // Don't overwrite text that the chunk callback
                        // is actively streaming.
                        if streaming_text_active_clone.load(Ordering::Relaxed) {
                            continue;
                        }
                        let line = {
                            let mut p = tick_progress.lock().unwrap();
                            p.tick();
                            spinner::compact_progress_line(&p, 18)
                        };

                        // Render spinner + optional input line.
                        if let Some(ref ws) = tick_watcher {
                            render_two_line_transient(&line, ws);
                        } else {
                            inline::print_transient(&line);
                        }
                    }
                    maybe_event = event_rx.recv() => {
                        match maybe_event {
                            Some(event) => {
                                // Pause the spinner, render the event, then
                                // let the next tick redraw the spinner.
                                inline::clear_transient();
                                // Thought events are rendered with DIM styling
                                // (the LLM's reasoning before tool execution).
                                // They are NOT the streamed final text.
                                streaming_text_active_clone.store(false, Ordering::Relaxed);
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

        // Print initial spinner immediately so the user sees activity
        // right away, even before the first ticker tick (80ms).
        {
            let p = progress.lock().unwrap();
            let initial_line = spinner::compact_progress_line(&p, 18);
            if let Some(ref ws) = self.watcher_state {
                render_two_line_transient(&initial_line, ws);
            } else {
                inline::print_transient(&initial_line);
            }
        }

        let result = orchestrator
            .run_stream_with_attachments(task, attachments, |chunk| {
                // Stream text in real-time. When the first chunk arrives,
                // clear the spinner and start printing directly.
                if !chunk.is_empty() {
                    if !streaming_text_active.load(Ordering::Relaxed) {
                        // First chunk — transition from spinner to text.
                        inline::clear_transient();
                        streaming_text_active.store(true, Ordering::Relaxed);
                    }
                    // Print the chunk directly to stdout.
                    // We use print! instead of inline::print_line because
                    // streaming is character-by-character, not line-by-line.
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(chunk.as_bytes());
                    let _ = std::io::stdout().flush();
                    streamed_text_clone
                        .lock()
                        .unwrap()
                        .push_str(&chunk);
                    // Count newlines for re-render tracking.
                    let newlines = chunk.chars().filter(|&c| c == '\n').count() as u32;
                    if newlines > 0 {
                        streamed_lines_clone.fetch_add(newlines, Ordering::Relaxed);
                    }
                }
            })
            .await;

        // Streaming is done — reset the flag so the spinner can tick again
        // if the ticker hasn't stopped yet.
        streaming_text_active.store(false, Ordering::Relaxed);

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
                let already_streamed = streamed_text.lock().unwrap().clone();
                let lines_printed = streamed_lines.load(Ordering::Relaxed);

                if already_streamed.is_empty() {
                    // No text was streamed (e.g. model returned empty
                    // before tool calls, or only tool calls). Render the
                    // final result in batch mode.
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
                } else {
                    // Text was already streamed in real-time as plaintext.
                    // Re-render with full markdown styling by replacing
                    // the streamed lines in-place.
                    let total_lines = if already_streamed.ends_with('\n') {
                        lines_printed
                    } else {
                        lines_printed + 1 // last line without trailing newline
                    };

                    if total_lines > 0 && total_lines <= 500 && inline::is_stdout_tty() {
                        let full_text = if final_result.len() > already_streamed.len() {
                            final_result.clone()
                        } else {
                            already_streamed.clone()
                        };
                        let parsed = md_widget::MarkdownContent::parse(&full_text);
                        inline::replace_lines(total_lines, &parsed.lines);
                    } else {
                        // Too many lines or non-TTY — just ensure newline.
                        if !already_streamed.ends_with('\n') {
                            println!();
                        }
                    }
                    inline::print_blank();
                }
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

    // ── Skill command dispatch ────────────────────────────────

    pub fn skill_command(&self, action: &SkillAction) -> Result<()> {
        match action {
            SkillAction::List => self.skill_list()?,
            SkillAction::Info { name } => self.skill_info(name)?,
            SkillAction::Create { name, global } => self.skill_create(name, *global)?,
        }
        Ok(())
    }

    fn skill_list(&self) -> Result<()> {
        let discovery_config: core_agentic::DiscoveryConfig =
            core_agentic::DiscoveryConfig::from(&self.config.skills);
        let index = core_agentic::discover_skills(&discovery_config);

        if index.is_empty() {
            println!();
            println!("  No skills found.");
            println!();
            println!("  Create one:  agentic skill create <name>");
            println!();
            return Ok(());
        }

        println!();
        println!("  Skills");

        let mut skills: Vec<_> = index.all().into_iter().collect();
        skills.sort_by(|a, b| a.name().cmp(b.name()));

        for skill in &skills {
            println!();
            println!("  📦 {}", skill.name());
            println!("     {}", skill.description());
            println!("     Path: {}", skill.dir.display());
        }

        if !index.blocked().is_empty() {
            println!();
            println!("  Blocked:");
            for name in index.blocked() {
                println!("     ✗ {}", name);
            }
        }

        println!();
        Ok(())
    }

    fn skill_info(&self, name: &str) -> Result<()> {
        let discovery_config: core_agentic::DiscoveryConfig =
            core_agentic::DiscoveryConfig::from(&self.config.skills);
        let index = core_agentic::discover_skills(&discovery_config);

        let skill = match index.get(name) {
            Some(s) => s,
            None => {
                eprintln!("✗ Skill '{}' not found.", name);
                std::process::exit(1);
            }
        };

        println!();
        println!("  📦 {} — {}", skill.name(), skill.description());
        println!("     Path: {}", skill.dir.display());
        println!("     SKILL.md size: {} bytes", skill.content.len());
        println!();

        // Show preview (first 20 lines of body)
        let preview_lines: Vec<&str> = skill.body.lines().take(20).collect();
        if !preview_lines.is_empty() {
            println!("  Preview:");
            for line in &preview_lines {
                println!("    {}", line);
            }
            if skill.body.lines().count() > 20 {
                println!("    ... ({} more lines)", skill.body.lines().count() - 20);
            }
        }

        // List other files in the skill directory
        if let Ok(entries) = std::fs::read_dir(&skill.dir) {
            let files: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("SKILL.md"))
                .collect();
            if !files.is_empty() {
                println!();
                println!("  Referenced files:");
                for f in &files {
                    println!("     📄 {}", f.file_name().unwrap_or_default().to_string_lossy());
                }
            }
        }

        println!();
        Ok(())
    }

    fn skill_create(&self, name: &str, global: bool) -> Result<()> {
        // Validate name
        let name_re = regex::Regex::new(r"^[a-z0-9-]{1,64}$").unwrap();
        if !name_re.is_match(name) {
            eprintln!("✗ Invalid skill name '{}': must be 1-64 chars, lowercase a-z, 0-9, hyphens only", name);
            std::process::exit(1);
        }

        let base_dir = if global {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home)
                .join(".config")
                .join("agentic")
                .join("skills")
        } else {
            // Project-local: .agentic/skills/ in cwd (or walk-up)
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let mut dir = find_or_create_dir(&cwd, ".agentic");
            dir.push("skills");
            dir
        };

        let skill_dir = base_dir.join(name);
        if skill_dir.exists() {
            eprintln!("✗ Skill '{}' already exists at {}", name, skill_dir.display());
            std::process::exit(1);
        }

        std::fs::create_dir_all(&skill_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create skill directory: {}", e))?;

        let template = format!(
            r"---
name: {name}
description: What this skill does and when to use it.
---

# {name}

## Setup
(optional setup instructions)

## Usage
Instructions the agent follows when this skill is loaded.
"
        );

        std::fs::write(skill_dir.join("SKILL.md"), &template)
            .map_err(|e| anyhow::anyhow!("Failed to write SKILL.md: {}", e))?;

        println!();
        println!("  ✅ Skill '{}' created at:", name);
        println!("     {}", skill_dir.display());
        println!();
        println!("  Edit SKILL.md to add instructions, then run:");
        println!("     agentic skill list");
        println!();

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
                    capabilities: None,
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
                    capabilities: None,
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
                cache: core_agentic::CacheConfig::default(),
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
            skills: core_agentic::SkillsConfig::default(),
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
                    capabilities: None,
            })
            .unwrap_or(core_agentic::ModelConfig {
                model: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                temperature: 0.7,
                max_tokens: 8192,
                    capabilities: None,
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
                cache: core_agentic::CacheConfig::default(),
            }],
            safety: core_agentic::SafetyConfig::default(),
            output: core_agentic::OutputConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
            system_prompt: None,
            agent: core_agentic::AgentLoopConfig::default(),
            skills: core_agentic::SkillsConfig::default(),
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

/// Render a two-line transient area:
///   Line 1: spinner progress
///   Line 2: styled input line (same look as the normal REPL prompt)
///
/// Uses ANSI escape codes to manage two lines without scrolling.
fn render_two_line_transient(
    spinner_line: &ratatui::text::Line<'_>,
    watcher_state: &std::sync::Mutex<crate::input_watcher::WatcherState>,
) {
    use std::io::Write;
    use crossterm::ExecutableCommand;

    let mut stdout = std::io::stdout();

    // Render the spinner line.
    inline::print_transient(spinner_line);

    // Read the current state.
    let s = watcher_state.lock().unwrap();
    let has_hint = !s.hint.is_empty();
    let has_content = has_hint || !s.buffer.is_empty();

    // Always render the input line so the prompt is always visible.
    // Move to next line, clear it.
    let _ = stdout.execute(crossterm::cursor::MoveToColumn(0));
    let _ = stdout.execute(crossterm::terminal::Clear(
        crossterm::terminal::ClearType::CurrentLine,
    ));
    print!("\n");
    let _ = stdout.execute(crossterm::cursor::MoveToColumn(0));

    if has_hint {
        // Show hint in green after the prompt.
        print!("{}\x1b[32m{}\x1b[0m", s.prompt_left, s.hint);
    } else {
        // Show live input buffer after the prompt.
        print!("{}{}", s.prompt_left, s.buffer);
    }

    // Print right-side info (model / provider / branch) padded
    // to fill the terminal width, just like the normal prompt.
    // Calculate visible width of what we already printed.
    let left_visible = strip_ansi_len(&s.prompt_left)
        + if has_hint {
            s.hint.len()
        } else {
            s.buffer.len()
        };
    let term_w = inline::terminal_width();
    let right_visible = strip_ansi_len(&s.prompt_right);
    let gap = term_w.saturating_sub(left_visible).saturating_sub(right_visible);
    if gap > 2 && !s.prompt_right.is_empty() {
        print!("\x1b[2m{}\x1b[0m{}", " ".repeat(gap), s.prompt_right);
    }

    let _ = stdout.flush();

    // Move cursor back up to the spinner line so next tick
    // overwrites correctly.
    print!("\x1b[1A");
    let _ = stdout.flush();
}

/// Calculate visible length of a string (strips ANSI escape sequences).
fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            len += c.len_utf8();
        }
    }
    len
}

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
            inline::print_line(&tool_call::render_call_compact(tool_name, arguments));
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

            // Compact result line: → ✓ summary  or  → ✗ error
            inline::print_line(&tool_call::render_result_compact(output, is_error));

            // If there's a diff, render it inline without borders.
            if let Some(diff) = diff_text {
                inline::print_line(&diff_widget::summary_line(diff));
                let diff_lines = diff_widget::render(diff);
                let max_diff_lines = 20;
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
            let _ = tool_name;
        }
        core_agentic::Event::Thought { content } => {
            // Display the LLM's thinking/reasoning before tool execution
            // with DIM styling so it's visually distinct from the final response.
            if !content.is_empty() {
                inline::print_line(&components::thinking_header(true));
                // Print thinking content with DIM style
                for line in content.lines() {
                    inline::print_line(&RLine::from(RSpan::styled(
                        format!("  {}", line),
                        RStyle::default()
                            .fg(RColor::Indexed(242))
                            .add_modifier(RModifier::DIM),
                    )));
                }
                inline::print_line(&components::thinking_header(false));
            }
        }
        core_agentic::Event::Error { message } => {
            inline::print_line(&components::error_badge(message));
        }
        core_agentic::Event::System { message } => {
            inline::print_line(&components::info_badge(message));
        }
        // ConfirmationRequest / Completed are surfaced via other
        // channels (the confirmation prompt, the final markdown).
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

/// Walk up from `start` looking for `target` directory, creating it if not found.
/// If none exists up to the filesystem root, falls back to `start`.
fn find_or_create_dir(start: &std::path::Path, target: &str) -> std::path::PathBuf {
    let mut current: Option<&std::path::Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(target);
        if candidate.is_dir() {
            return candidate;
        }
        current = dir.parent();
    }
    // Create in cwd
    let fallback = start.join(target);
    let _ = std::fs::create_dir_all(&fallback);
    fallback
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

// ── End-to-end smoke tests ─────────────────────────────────
//
// These tests exercise the full `Commands::run` pipeline against a
// scripted mock provider so refactors in the CLI wiring are caught
// before they reach a release. The orchestrator integration tests in
// core-agentic cover the loop itself; here we validate that the CLI
// layer (orchestrator init, tool registration, event plumbing, output
// rendering) works end to end.

#[cfg(test)]
mod e2e_tests {
    use super::*;
    use core_agentic::providers::{
        ChatChunk, ChatMessageResponse, ChatRequest, ChatResponse, LLMProvider,
        ProviderError, ProviderResult, StreamResult, ToolCallDelta, ToolCallFunction,
        ToolCallResponse,
    };
    use futures::stream;
    use std::sync::Mutex;

    /// Provider returning scripted responses in order.
    struct ScriptedProvider {
        responses: Mutex<Vec<ChatResponse>>,
    }

    impl ScriptedProvider {
        fn new(responses: Vec<ChatResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }

        /// Convert a `ChatResponse` into a list of streaming `ChatChunk`s.
        fn response_to_chunks(response: ChatResponse) -> Vec<Result<ChatChunk, ProviderError>> {
            let mut chunks: Vec<Result<ChatChunk, ProviderError>> = Vec::new();
            let id = response.id.clone();

            // Text deltas (one chunk per response).
            if let Some(content) = &response.message.content {
                chunks.push(Ok(ChatChunk {
                    id: id.clone(),
                    delta: content.clone(),
                    finish_reason: None,
                    tool_calls: vec![],
                    usage: None,
                }));
            }

            // Tool-call deltas.
            for tc in &response.message.tool_calls {
                chunks.push(Ok(ChatChunk {
                    id: id.clone(),
                    delta: String::new(),
                    finish_reason: None,
                    tool_calls: vec![ToolCallDelta {
                        index: 0,
                        id: Some(tc.id.clone()),
                        function_name: Some(tc.function.name.clone()),
                        function_arguments: Some(tc.function.arguments.clone()),
                    }],
                    usage: None,
                }));
            }

            // Final chunk with finish_reason.
            chunks.push(Ok(ChatChunk {
                id: id.clone(),
                delta: String::new(),
                finish_reason: response.finish_reason.clone(),
                tool_calls: vec![],
                usage: None,
            }));

            chunks
        }
    }

    impl LLMProvider for ScriptedProvider {
        fn provider_type(&self) -> &str {
            "test"
        }
        fn provider_id(&self) -> &str {
            "test"
        }
        fn chat(&self, _req: ChatRequest) -> ProviderResult<ChatResponse> {
            let mut q = self.responses.lock().unwrap();
            if q.is_empty() {
                return Err(ProviderError::new(
                    "ScriptedProvider: no more responses",
                ));
            }
            Ok(q.remove(0))
        }
        fn chat_stream(
            &self,
            _req: ChatRequest,
        ) -> StreamResult<ChatChunk, ProviderError> {
            let mut q = self.responses.lock().unwrap();
            if q.is_empty() {
                return Err(ProviderError::new(
                    "ScriptedProvider: no more responses",
                ));
            }
            let response = q.remove(0);
            let chunks = Self::response_to_chunks(response);
            Ok(Box::pin(stream::iter(chunks)))
        }
    }

    #[tokio::test]
    async fn smoke_run_executes_tool_and_returns_final_answer() {
        // ── Provider: tool call → final text ──────────────
        let provider = Arc::new(ScriptedProvider::new(vec![
            ChatResponse {
                id: "resp-1".into(),
                model: "test".into(),
                message: ChatMessageResponse {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: vec![ToolCallResponse {
                        id: "call-1".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "list_files".into(),
                            arguments: r#"{"path": "."}"#.into(),
                        },
                    }],
                },
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            ChatResponse {
                id: "resp-2".into(),
                model: "test".into(),
                message: ChatMessageResponse {
                    role: "assistant".into(),
                    content: Some(
                        "Here are the files I found.".into(),
                    ),
                    tool_calls: vec![],
                },
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]));

        let config = Config::fallback();
        let mut commands = Commands::new(config)
            .with_mock_provider(provider)
            .with_permission_mode(core_agentic::PermissionMode::Yolo);

        let result = commands
            .run_with_callbacks("list current directory", |_| {}, |_| {})
            .await
            .expect("run_with_callbacks should succeed");

        assert!(
            result.contains("Here are the files"),
            "Expected final answer in output, got: {result}"
        );
    }

    #[tokio::test]
    async fn smoke_run_emits_events_for_tool_calls() {
        // ── Provider: one tool call → final text ──────────
        let provider = Arc::new(ScriptedProvider::new(vec![
            ChatResponse {
                id: "resp-1".into(),
                model: "test".into(),
                message: ChatMessageResponse {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: vec![ToolCallResponse {
                        id: "call-1".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "list_files".into(),
                            arguments: r#"{"path": "."}"#.into(),
                        },
                    }],
                },
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            ChatResponse {
                id: "resp-2".into(),
                model: "test".into(),
                message: ChatMessageResponse {
                    role: "assistant".into(),
                    content: Some("Done.".into()),
                    tool_calls: vec![],
                },
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]));

        let config = Config::fallback();
        let mut commands = Commands::new(config)
            .with_mock_provider(provider)
            .with_permission_mode(core_agentic::PermissionMode::Yolo);

        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();

        let _ = commands
            .run_with_callbacks(
                "list files",
                |_| {},
                move |evt| {
                    captured.lock().unwrap().push(format!("{evt:?}"));
                },
            )
            .await;

        let logged = events.lock().unwrap();
        let all: String = logged.join(" ");
        assert!(
            all.contains("ToolCall") || all.contains("list_files"),
            "Expected tool-call events, got: {all}"
        );
        assert!(
            all.contains("ToolOutput"),
            "Expected tool-output events, got: {all}"
        );
    }
}
