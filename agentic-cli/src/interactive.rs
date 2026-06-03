//! Interactive REPL mode using reedline
//!
//! Provides an interactive CLI with:
//! - `/` command completion with description popup (auto-activates on `/`)
//! - `@` file path completion with popup (auto-activates on `@`)
//! - Syntax highlighting for `/` (yellow) and `@` (blue)
//! - Fish-style inline hints
//! - Session statistics, conversation history, save/load

use anyhow::Result;
use nu_ansi_term::{Color as AnsiColor, Style};
use ratatui::{
    style::{Color, Modifier, Style as RStyle},
    text::{Line, Span as RSpan},
};
use reedline::{
    default_emacs_keybindings, Completer, DescriptionMenu, EditCommand, Emacs,
    Highlighter, Hinter, KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode,
    PromptHistorySearch, PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal,
    Span, StyledText, Suggestion, ValidationResult, Validator,
};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::cli::SkillAction;
use crate::commands::Commands;
use crate::widgets::inline;
use crate::widgets::components;

// ── Session statistics ──────────────────────────────────────

#[derive(Clone)]
struct SessionStats {
    messages_sent: Arc<AtomicU32>,
    tool_calls: Arc<AtomicU32>,
    total_input_tokens: Arc<AtomicU32>,
    total_output_tokens: Arc<AtomicU32>,
    /// Tokens read from provider prompt cache.
    total_cache_read_tokens: Arc<AtomicU32>,
    /// Tokens written to provider prompt cache.
    total_cache_creation_tokens: Arc<AtomicU32>,
    session_start: Instant,
}

impl SessionStats {
    fn new() -> Self {
        Self {
            messages_sent: Arc::new(AtomicU32::new(0)),
            tool_calls: Arc::new(AtomicU32::new(0)),
            total_input_tokens: Arc::new(AtomicU32::new(0)),
            total_output_tokens: Arc::new(AtomicU32::new(0)),
            total_cache_read_tokens: Arc::new(AtomicU32::new(0)),
            total_cache_creation_tokens: Arc::new(AtomicU32::new(0)),
            session_start: Instant::now(),
        }
    }

    fn increment_messages(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset all counters in place. Used by `/restart` so the status bar
    /// reflects the fresh session immediately.
    fn reset(&self) {
        self.messages_sent.store(0, Ordering::Relaxed);
        self.tool_calls.store(0, Ordering::Relaxed);
        self.total_input_tokens.store(0, Ordering::Relaxed);
        self.total_output_tokens.store(0, Ordering::Relaxed);
        self.total_cache_read_tokens.store(0, Ordering::Relaxed);
        self.total_cache_creation_tokens.store(0, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn increment_tool_calls(&self) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn add_input_tokens(&self, n: u32) {
        self.total_input_tokens.fetch_add(n, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn add_output_tokens(&self, n: u32) {
        self.total_output_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn add_cache_read_tokens(&self, n: u32) {
        self.total_cache_read_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn add_cache_creation_tokens(&self, n: u32) {
        self.total_cache_creation_tokens.fetch_add(n, Ordering::Relaxed);
    }

    fn total_cache_read_tokens(&self) -> u32 {
        self.total_cache_read_tokens.load(Ordering::Relaxed)
    }

    fn total_cache_creation_tokens(&self) -> u32 {
        self.total_cache_creation_tokens.load(Ordering::Relaxed)
    }

    /// Cache hit ratio (0.0–1.0). When zero cache reads, returns 0.0.
    fn cache_hit_ratio(&self) -> f64 {
        let read = self.total_cache_read_tokens() as f64;
        let created = self.total_cache_creation_tokens() as f64;
        let total = read + created;
        if total > 0.0 { read / total } else { 0.0 }
    }

    fn messages_sent(&self) -> u32 {
        self.messages_sent.load(Ordering::Relaxed)
    }

    fn tool_calls(&self) -> u32 {
        self.tool_calls.load(Ordering::Relaxed)
    }

    fn total_input_tokens(&self) -> u32 {
        self.total_input_tokens.load(Ordering::Relaxed)
    }

    fn total_output_tokens(&self) -> u32 {
        self.total_output_tokens.load(Ordering::Relaxed)
    }

    fn elapsed_secs(&self) -> u64 {
        self.session_start.elapsed().as_secs()
    }

    fn elapsed_str(&self) -> String {
        let secs = self.elapsed_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }

    fn format_tokens(&self, n: u32) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.1}K", n as f64 / 1_000.0)
        } else {
            format!("{}", n)
        }
    }
}

// ── Slash command definitions with aliases ──────────────────

const SLASH_COMMANDS: &[(&str, &[&str], &str)] = &[
    ("help", &["h", "?"], "Show help message"),
    ("new", &["n", "clear", "cls"], "Start a new session"),
    ("config", &["cfg"], "Show current configuration"),
    ("history", &["hist"], "Show conversation history"),
    ("tools", &["t"], "List available tools"),
    ("models", &["m"], "List all models from all providers"),
    ("provider", &["prov"], "Switch or show provider"),
    ("sessions", &["ss"], "List and resume previous sessions"),
    ("mcp", &[], "Show MCP server status"),
    ("plan", &["p"], "Create a plan for a goal"),
    ("skills", &[], "List all indexed skills"),
    ("search", &["find"], "Search conversation memory"),
    ("image", &["img"], "Attach an image for the next turn"),
    ("stats", &[], "Show session statistics"),
    ("quit", &["q", "exit"], "Exit interactive mode"),
];

// ── Layout helpers ──────────────────────────────────────────

/// Detect terminal width, falling back to 80 cols.
fn term_width() -> usize {
    crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(40)
}

// ── Agentic Completer ───────────────────────────────────────

/// Custom completer that handles both `/` commands and `@` file paths.
struct AgenticCompleter;

impl Completer for AgenticCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        // Guard: reedline may pass pos > line.len() in some edge cases
        let pos = pos.min(line.len());
        let before_cursor = &line[..pos];

        // ── Case 1: `/models <query>` - complete model names ──
        if line.starts_with("/models ") || line.starts_with("/m ") {
            let query_start = line.find(' ').unwrap() + 1;
            if pos >= query_start {
                let query = &line[query_start..pos];
                return complete_model_suggestions(query);
            }
        }

        // ── Case 1b: `/skills <query>` - complete skill names ──
        if line.starts_with("/skills ") {
            let query_start = line.find(' ').unwrap() + 1;
            if pos >= query_start {
                let query = &line[query_start..pos];
                return complete_skill_suggestions(query);
            }
        }

        // ── Case 2: `/` command completion ──
        if line.starts_with('/') {
            if let Some(space_pos) = line.find(' ') {
                if pos <= space_pos {
                    return complete_slash_command_suggestions(&line[..pos]);
                }
            } else {
                return complete_slash_command_suggestions(before_cursor);
            }
        }

        // ── Case 3: `@` file completion ──
        if let Some(at_pos) = find_at_trigger(before_cursor) {
            let query = &before_cursor[at_pos + 1..];
            return complete_file_path_suggestions(query, at_pos);
        }

        Vec::new()
    }
}

/// Find the `@` trigger position in text before cursor.
fn find_at_trigger(text: &str) -> Option<usize> {
    for (i, c) in text.char_indices().rev() {
        match c {
            '@' => {
                let at_start = i == 0;
                let after_space = i > 0 && text[..i].ends_with(char::is_whitespace);
                if at_start || after_space {
                    let after_at = &text[i + 1..];
                    if !after_at.contains(char::is_whitespace) {
                        return Some(i);
                    }
                }
                return None;
            }
            w if w.is_whitespace() => return None,
            _ => continue,
        }
    }
    None
}

/// Complete slash commands and return Suggestions with descriptions.
fn complete_slash_command_suggestions(partial: &str) -> Vec<Suggestion> {
    let partial_lower = partial.to_lowercase();

    SLASH_COMMANDS
        .iter()
        .filter(|(cmd, aliases, _)| {
            let full = format!("/{}", cmd);
            if full.starts_with(&partial_lower) || full.starts_with(partial) {
                return true;
            }
            aliases.iter().any(|a| {
                let alias_full = format!("/{}", a);
                alias_full.starts_with(&partial_lower) || alias_full.starts_with(partial)
            })
        })
        .map(|(cmd, aliases, desc)| {
            let display = if aliases.is_empty() {
                format!("/{}", cmd)
            } else {
                format!("/{} ({})", cmd, aliases.join(", "))
            };
            Suggestion {
                value: format!("/{}", cmd),
                display_override: Some(display),
                description: Some(desc.to_string()),
                style: None,
                extra: None,
                span: Span::new(0, partial.len()),
                append_whitespace: true,
                match_indices: None,
            }
        })
        .collect()
}

/// Complete model names for `/models <query>`.
/// Returns suggestions from all configured providers.
fn complete_model_suggestions(query: &str) -> Vec<Suggestion> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Load config to get all available models
    let config = match core_agentic::Config::load() {
        Some(c) => c,
        None => return results,
    };

    let active_provider = config.active_provider().map(|p| p.name.clone());
    let active_model = config.active_model().map(|m| m.model.clone());

    for provider in &config.providers {
        for model in &provider.models {
            let display_name = model.display_name.as_deref().unwrap_or(&model.model);
            let model_name = &model.model;

            // Filter by query (match display name or model ID)
            if !query.is_empty()
                && !display_name.to_lowercase().contains(&query_lower)
                && !model_name.to_lowercase().contains(&query_lower)
                && !provider.name.to_lowercase().contains(&query_lower)
            {
                continue;
            }

            let is_active = active_provider.as_deref() == Some(&provider.name)
                && active_model.as_deref() == Some(model_name);

            let caps = model.effective_capabilities();
            let vision_icon = if caps.vision { " 👁" } else { "" };
            let active_marker = if is_active { " ●" } else { "" };

            let display = format!(
                "{}{} [{}]{}",
                display_name, vision_icon, provider.name, active_marker
            );

            let description = format!(
                "{} ({}){}",
                model_name,
                provider.name,
                if is_active { " - active" } else { "" }
            );

            results.push(Suggestion {
                value: model_name.clone(),
                display_override: Some(display),
                description: Some(description),
                style: None,
                extra: None,
                span: Span::new(0, query.len()),
                append_whitespace: false,
                match_indices: None,
            });
        }
    }

    // Sort: active first, then alphabetically
    results.sort_by(|a, b| {
        let a_active = a.description.as_ref().map_or(false, |d| d.contains("active"));
        let b_active = b.description.as_ref().map_or(false, |d| d.contains("active"));

        match (a_active, b_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.value.cmp(&b.value),
        }
    });

    results
}

/// Complete skill names for `/skills <query>`.
/// Returns suggestions from the discovered skill index.
fn complete_skill_suggestions(query: &str) -> Vec<Suggestion> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Use the global skill loader to list available skills
    let skills = core_agentic::list_skills();

    for (name, desc) in &skills {
        if !query.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }

        results.push(Suggestion {
            value: name.clone(),
            display_override: Some(format!("📦 {} — {}", name, desc)),
            description: Some(desc.clone()),
            style: None,
            extra: None,
            span: Span::new(0, query.len()),
            append_whitespace: true,
            match_indices: None,
        });
    }

    results.sort_by(|a, b| a.value.cmp(&b.value));
    results
}

/// Complete file paths and return Suggestions.
///
/// Uses `ignore` crate for `.gitignore`-aware recursive file listing.
///
/// Behavior:
/// - `@` (empty query) → all project files recursively (flat list)
/// - `@src/` → all files under src/ recursively
/// - `@src/ma` → files under src/ matching "ma"
/// - `@chat` → all project files matching "chat"
fn complete_file_path_suggestions(query: &str, at_pos: usize) -> Vec<Suggestion> {
    let mut results = Vec::new();

    // Parse query into (path_prefix, name_filter)
    let (path_prefix, name_filter) = if query.is_empty() {
        (String::new(), String::new())
    } else if query.ends_with('/') {
        (query.to_string(), String::new())
    } else if query.contains('/') {
        let last_slash = query.rfind('/').unwrap();
        (
            query[..=last_slash].to_string(),
            query[last_slash + 1..].to_string(),
        )
    } else {
        (String::new(), query.to_string())
    };

    let filter_lower = name_filter.to_lowercase();

    // Walk the project recursively, respecting .gitignore
    let mut builder = ignore::WalkBuilder::new(".");
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false);

    for entry in builder.build().filter_map(|e| e.ok()) {
        let path = entry.path();
        let path_str = path.to_string_lossy();

        // Skip `.` itself
        if path_str == "." || path_str == "./" {
            continue;
        }

        // Normalize: backslashes → forward slashes, strip leading "./"
        let normalized = path_str.replace('\\', "/");
        let clean = normalized.strip_prefix("./").unwrap_or(&normalized);

        // If a path prefix was given, only include files under that prefix
        if !path_prefix.is_empty() {
            if !clean.starts_with(&path_prefix)
                && !clean.starts_with(path_prefix.trim_end_matches('/'))
            {
                continue;
            }
        }

        // If a name filter was given, match against filename and full path
        if !filter_lower.is_empty() {
            let fname = std::path::Path::new(clean)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let fname_lower = fname.to_lowercase();
            let clean_lower = clean.to_lowercase();

            if !fname_lower.starts_with(&filter_lower)
                && !fname_lower.contains(&filter_lower)
                && !clean_lower.contains(&filter_lower)
            {
                continue;
            }
        }

        let is_dir = path.is_dir();
        let display = if is_dir {
            format!("{}/", clean)
        } else {
            clean.to_string()
        };

        let icon = if is_dir { "📁" } else { "📄" };

        let description = if is_dir {
            "Directory".to_string()
        } else {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("?");
            format!("File ({})", ext)
        };

        results.push(Suggestion {
            value: format!("@{}", display),
            display_override: Some(format!("{} {}", icon, display)),
            description: Some(description),
            style: None,
            extra: None,
            span: Span::new(at_pos, at_pos + 1 + query.len()),
            append_whitespace: !is_dir,
            match_indices: None,
        });
    }

    // Sort: directories first, then files — both alphabetically
    results.sort_by(|a, b| {
        let a_dir = a.value.ends_with('/');
        let b_dir = b.value.ends_with('/');
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.value.to_lowercase().cmp(&b.value.to_lowercase()),
        }
    });

    results.truncate(30);
    results
}

// ── Agentic Highlighter ─────────────────────────────────────

/// Syntax highlighter for `/` commands and `@` file references.
struct AgenticHighlighter;

impl Highlighter for AgenticHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled = StyledText::new();

        if line.is_empty() {
            return styled;
        }

        // Slash command highlighting (yellow)
        if line.starts_with('/') {
            if let Some(space_pos) = line.find(' ') {
                styled.push((
                    Style::new().fg(AnsiColor::Yellow).bold(),
                    line[..space_pos].to_string(),
                ));
                styled.push((Style::new(), line[space_pos..].to_string()));
            } else {
                styled.push((
                    Style::new().fg(AnsiColor::Yellow).bold(),
                    line.to_string(),
                ));
            }
            return styled;
        }

        // @ file reference highlighting (blue)
        if line.contains('@') {
            let mut i = 0;
            let mut in_at_ref = false;
            let mut at_start = 0;

            for (pos, c) in line.char_indices() {
                if c == '@' && (pos == 0 || line[..pos].ends_with(char::is_whitespace)) {
                    // Flush previous
                    if i < pos {
                        styled.push((Style::new(), line[i..pos].to_string()));
                    }
                    in_at_ref = true;
                    at_start = pos;
                    i = pos;
                } else if in_at_ref && c.is_whitespace() {
                    styled.push((
                        Style::new().fg(AnsiColor::Rgb(52, 152, 219)).bold(),
                        line[at_start..pos].to_string(),
                    ));
                    in_at_ref = false;
                    i = pos;
                }
            }

            // Flush remaining
            if i < line.len() {
                if in_at_ref {
                    styled.push((
                        Style::new().fg(AnsiColor::Rgb(52, 152, 219)).bold(),
                        line[i..].to_string(),
                    ));
                } else {
                    styled.push((Style::new(), line[i..].to_string()));
                }
            }

            return styled;
        }

        // Default
        styled.push((Style::new(), line.to_string()));
        styled
    }
}

// ── Agentic Hinter ──────────────────────────────────────────

/// Fish-style hinter that shows first match inline.
struct AgenticHinter {
    last_hint: String,
}

impl AgenticHinter {
    fn new() -> Self {
        Self {
            last_hint: String::new(),
        }
    }
}

impl Hinter for AgenticHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        _history: &dyn reedline::History,
        _use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.last_hint.clear();

        // Slash command hints
        if line.starts_with('/') && !line.contains(' ') {
            let partial = &line[..pos];
            let matches: Vec<&&str> = SLASH_COMMANDS
                .iter()
                .map(|(cmd, _, _)| cmd)
                .filter(|cmd| {
                    let full = format!("/{}", **cmd);
                    full != partial && full.starts_with(partial)
                })
                .collect();

            if !matches.is_empty() {
                let first = format!("/{}", matches[0]);
                let remainder = first[partial.len()..].to_string();
                if !remainder.is_empty() {
                    let hint = if matches.len() > 1 {
                        format!("{} [{}+]", remainder, matches.len() - 1)
                    } else {
                        remainder
                    };
                    self.last_hint = hint.clone();
                    return AnsiColor::DarkGray.paint(hint).to_string();
                }
            }
        }

        // @ file path hints
        if line.contains('@') {
            let before = &line[..pos];
            if let Some(at_pos) = find_at_trigger(before) {
                let query = &before[at_pos + 1..];
                let completions = complete_file_path_suggestions(query, at_pos);

                if !completions.is_empty() {
                    let comp = &completions[0].value;
                    if let Some(remaining) = comp.get(query.len()..) {
                        if !remaining.is_empty() {
                            let hint = if completions.len() > 1 {
                                format!("{} [{}+]", remaining, completions.len() - 1)
                            } else {
                                remaining.to_string()
                            };
                            self.last_hint = hint.clone();
                            return AnsiColor::DarkGray.paint(hint).to_string();
                        }
                    }
                }
            }
        }

        String::new()
    }

    fn complete_hint(&self) -> String {
        if let Some(space_pos) = self.last_hint.find(" [") {
            self.last_hint[..space_pos].to_string()
        } else {
            self.last_hint.clone()
        }
    }

    fn next_hint_token(&self) -> String {
        let hint = self.complete_hint();
        if let Some(slash_pos) = hint.find('/') {
            hint[..slash_pos + 1].to_string()
        } else if let Some(space_pos) = hint.find(' ') {
            hint[..space_pos].to_string()
        } else {
            hint
        }
    }
}

// ── Agentic Validator ───────────────────────────────────────

struct AgenticValidator;

impl Validator for AgenticValidator {
    fn validate(&self, _line: &str) -> ValidationResult {
        ValidationResult::Complete
    }
}

// ── Agentic Prompt ──────────────────────────────────────────

struct AgenticPrompt {
    dir_name: String,
    git_branch: Option<String>,
    model: String,
    provider: String,
}

impl AgenticPrompt {
    fn new(model_info: &ModelInfo) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let dir_name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        let git_branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if branch.is_empty() || branch == "HEAD" {
                        None
                    } else {
                        Some(branch)
                    }
                } else {
                    None
                }
            });

        Self {
            dir_name,
            git_branch,
            model: model_info.model.clone(),
            provider: model_info.provider.clone(),
        }
    }

    /// Build the prompt right-side info string, truncating to fit
    /// within `max_width` visible characters.
    fn build_right_info(&self, max_width: usize) -> String {
        let branch_part = match &self.git_branch {
            Some(b) => format!(" \u{1f4cc}{}", b),
            None => String::new(),
        };
        let info = format!(
            "{} {}{}",
            self.provider, self.model, branch_part
        );

        if info.len() > max_width {
            // Progressive truncation: drop branch, then truncate model
            let no_branch = format!("{} {}", self.provider, self.model);
            if no_branch.len() <= max_width {
                no_branch
            } else if self.model.len() + 3 <= max_width {
                format!("...{}", &self.model[self.model.len() - (max_width - 3)..])
            } else {
                format!("{:.w$}", info, w = max_width)
            }
        } else {
            info
        }
    }
}

impl Prompt for AgenticPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let dim = AnsiColor::DarkGray.prefix().to_string();
        let reset = Style::new().prefix().to_string();
        let cyan = AnsiColor::Cyan.prefix().to_string();

        // Single-line prompt: dirname>
        // Responsive: uses current terminal width to decide how much to show.
        let w = term_width();
        // Reserve ~40 chars for the right-side info + input area
        let max_dir = w.saturating_sub(40).max(10).min(self.dir_name.len());
        let dir_display = if self.dir_name.len() > max_dir {
            format!("...{}", &self.dir_name[self.dir_name.len() - max_dir + 3..])
        } else {
            self.dir_name.clone()
        };

        let left = format!(
            "{}{}{}{}>{} ",
            dim, dir_display, reset, cyan, reset
        );
        Cow::Owned(left)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        let w = term_width();
        // Reserve space for left prompt + some input area
        let left_len = self.dir_name.len().min(w.saturating_sub(40).max(10)) + 3; // "dir> "
        let max_right = w.saturating_sub(left_len).saturating_sub(2).max(0);

        let info = self.build_right_info(max_right);
        if info.is_empty() {
            return Cow::Borrowed("");
        }

        let dim = AnsiColor::DarkGray.prefix().to_string();
        let reset = Style::new().prefix().to_string();

        let right = format!(
            "{}{}{}",
            dim, info, reset
        );
        Cow::Owned(right)
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        // Left empty so that left + right layout is used instead.
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        let styled = format!(
            "{}   {}",
            AnsiColor::DarkGray.prefix(),
            Style::new().prefix()
        );
        Cow::Owned(styled)
    }

    fn right_prompt_on_last_line(&self) -> bool {
        true
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "FAILED ",
        };
        Cow::Owned(format!("{}({})", prefix, history_search.term))
    }
}

// ── Conversation entry ──────────────────────────────────────

#[derive(Debug)]
struct ConversationEntry {
    role: String,
    content: String,
    timestamp: chrono::DateTime<chrono::Local>,
}

// ── REPL loop ───────────────────────────────────────────────

pub async fn run(mut commands: Commands) -> Result<()> {
    let stats = SessionStats::new();
    let mut model_info = get_model_info(&commands);

    // Initialize session
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display().to_string();
    let mut current_session = crate::session::create(
        &cwd,
        &model_info.provider,
        &model_info.model,
    );

    print_banner(&model_info, &stats);

    // Build reedline with all features
    let completer = Box::new(AgenticCompleter);
    let highlighter = Box::new(AgenticHighlighter);
    let hinter = Box::new(AgenticHinter::new());
    let validator = Box::new(AgenticValidator);
    let prompt = AgenticPrompt::new(&model_info);

    // In-memory command history (no file)
    let history = Box::new(
        reedline::SqliteBackedHistory::in_memory()
            .map_err(|e| anyhow::anyhow!("Failed to create history: {}", e))?,
    );

    // Description menu — only_buffer_difference: false so completer gets full buffer
    let completion_menu = Box::new(
        DescriptionMenu::default()
            .with_name("completion_menu")
            .with_marker("\u{25bc} ")
            .with_columns(1)
            .with_selection_rows(8)
            .with_description_rows(4)
            .with_only_buffer_difference(false),
    );

    // Custom keybindings for Tab completion + auto-trigger on `/` and `@`
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    // Arrow up/down: navigate menu items when active, history when not
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Down,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuNext,
            ReedlineEvent::Down,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Up,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuPrevious,
            ReedlineEvent::Up,
        ]),
    );
    // Auto-activate completion menu when typing `/`
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Char('/'),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('/')]),
            ReedlineEvent::Menu("completion_menu".to_string()),
        ]),
    );
    // Auto-activate completion menu when typing `@`
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Char('@'),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('@')]),
            ReedlineEvent::Menu("completion_menu".to_string()),
        ]),
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut line_editor = Reedline::create()
        .with_completer(completer)
        .with_highlighter(highlighter)
        .with_hinter(hinter)
        .with_validator(validator)
        .with_history(history)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_quick_completions(true)
        .with_partial_completions(true);

    let mut conversation: Vec<ConversationEntry> = Vec::new();

    loop {
        let sig = line_editor.read_line(&prompt);

        match sig {
            Ok(Signal::Success(input)) => {
                let input = input.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                // Handle slash commands
                if input.starts_with('/') {
                    if let Some(action) = handle_slash_command(&input) {
                        match action {
                            ReplAction::Quit => break,
                            ReplAction::NewSession => {
                                // Save current session before clearing
                                if !current_session.messages.is_empty() {
                                    if let Err(e) = crate::session::save(&current_session) {
                                        inline::print_line(&components::warning_badge(
                                            &format!("Could not auto-save session: {}", e),
                                        ));
                                    }
                                }
                                // Start fresh
                                let cwd = std::env::current_dir()
                                    .unwrap_or_default()
                                    .display().to_string();
                                current_session = crate::session::create(
                                    &cwd,
                                    &model_info.provider,
                                    &model_info.model,
                                );
                                conversation.clear();
                                stats.reset();
                                commands.restart_session();

                                crossterm::execute!(
                                    std::io::stdout(),
                                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                                    crossterm::cursor::MoveTo(0, 0)
                                ).ok();
                                let model_info = get_model_info(&commands);
                                print_banner(&model_info, &stats);
                                inline::print_blank();
                                inline::print_line(&components::success_badge(
                                    "New session started.",
                                ));
                                inline::print_blank();
                                print_status_bar(&model_info, &stats);
                            }
                            ReplAction::Config => {
                                commands.config_show_inline();
                            }
                            ReplAction::History => {
                                show_history(&conversation);
                            }
                            ReplAction::Tools => {
                                commands.list_tools();
                            }
                            ReplAction::Stats => {
                                show_stats(&stats, &model_info);
                            }
                            ReplAction::Sessions => {
                                show_sessions();
                            }
                            ReplAction::SessionsResume(id) => {
                                match crate::session::load(&id) {
                                    Ok(loaded) => {
                                        // Save current first
                                        if !current_session.messages.is_empty() {
                                            let _ = crate::session::save(&current_session);
                                        }
                                        // Restore loaded session
                                        conversation.clear();
                                        for msg in &loaded.messages {
                                            conversation.push(ConversationEntry {
                                                role: msg.role.clone(),
                                                content: msg.content.clone(),
                                                timestamp: chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
                                                    .map(|dt| dt.with_timezone(&chrono::Local))
                                                    .unwrap_or_else(|_| chrono::Local::now()),
                                            });
                                        }
                                        current_session = loaded;
                                        commands.restart_session();
                                        stats.reset();

                                        crossterm::execute!(
                                            std::io::stdout(),
                                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                                            crossterm::cursor::MoveTo(0, 0)
                                        ).ok();
                                        let model_info = get_model_info(&commands);
                                        print_banner(&model_info, &stats);
                                        inline::print_blank();
                                        inline::print_line(&components::success_badge(
                                            &format!("Resumed: {} ({} messages)", current_session.title, current_session.messages.len()),
                                        ));
                                        inline::print_blank();
                                        print_status_bar(&model_info, &stats);
                                    }
                                    Err(e) => {
                                        inline::print_blank();
                                        inline::print_line(&components::error_badge(
                                            &format!("Failed to load session: {}", e),
                                        ));
                                        inline::print_blank();
                                    }
                                }
                            }
                            ReplAction::Provider(name) => {
                                inline::print_blank();
                                inline::print_line(&components::warning_badge(
                                    "Provider switching not yet supported in REPL.",
                                ));
                                inline::print_line(&Line::from(vec![
                                    RSpan::raw("  Use: "),
                                    RSpan::styled(
                                        "agentic config edit",
                                        RStyle::default().add_modifier(Modifier::BOLD),
                                    ),
                                    RSpan::raw(" to change providers."),
                                ]));
                                inline::print_blank();
                                let _ = &name;
                            }
                            ReplAction::Models => {
                                if let Some((provider, model)) = commands.pick_model_interactive_inline() {
                                    // Update model_info after switch
                                    let model_info = get_model_info(&commands);
                                    inline::print_blank();
                                    inline::print_line(&components::success_badge(
                                        &format!("Switched to {} / {}", provider, model),
                                    ));
                                    inline::print_blank();
                                    print_status_bar(&model_info, &stats);
                                }
                            }
                            ReplAction::ModelsSwitch(name) => {
                                match commands.switch_model(&name) {
                                    Ok((provider, model)) => {
                                        inline::print_blank();
                                        inline::print_line(&components::success_badge(
                                            &format!("Switched to {} / {}", provider, model),
                                        ));
                                        inline::print_blank();
                                    }
                                    Err(e) => {
                                        inline::print_blank();
                                        inline::print_line(&components::error_badge(&e.to_string()));
                                        inline::print_line(&Line::from(vec![
                                            RSpan::raw("  Use "),
                                            RSpan::styled(
                                                "/models",
                                                RStyle::default().add_modifier(Modifier::BOLD),
                                            ),
                                            RSpan::raw(" to see available models."),
                                        ]));
                                        inline::print_blank();
                                    }
                                }
                            }
                            ReplAction::Mcp => {
                                commands.show_mcp_status();
                            }
                            ReplAction::Plan(goal) => {
                                conversation.push(ConversationEntry {
                                    role: "user".into(),
                                    content: format!("[plan] {}", goal),
                                    timestamp: chrono::Local::now(),
                                });
                                stats.increment_messages();

                                print_turn_separator();
                                let start = Instant::now();
                                if let Err(e) = commands.plan_inline(&goal).await {
                                    inline::print_blank();
                                    inline::print_line(&components::error_badge(&e.to_string()));
                                    inline::print_blank();
                                } else {
                                    let elapsed = start.elapsed();
                                    conversation.push(ConversationEntry {
                                        role: "assistant".into(),
                                        content: format!(
                                            "(plan executed in {:.1}s)",
                                            elapsed.as_secs_f64()
                                        ),
                                        timestamp: chrono::Local::now(),
                                    });
                                    print_response_summary(&stats, elapsed.as_millis());
                                }
                            }
                            ReplAction::Skills => {
                                commands.skill_command(&SkillAction::List).ok();
                            }
                            ReplAction::SkillsLoad(name) => {
                                use ratatui::style::{Color, Modifier, Style};
                                use ratatui::text::{Line, Span};

                                inline::print_blank();
                                inline::print_line(&components::section_header(
                                    "⚡",
                                    &format!("Loading skill: {}", name),
                                    Color::Rgb(255, 215, 0),
                                ));
                                inline::print_blank();

                                let discovery_config: core_agentic::DiscoveryConfig =
                                    core_agentic::DiscoveryConfig::from(&commands.get_config().skills);
                                let index = core_agentic::discover_skills(&discovery_config);

                                if let Some(skill) = index.get(&name) {
                                    inline::print_line(&Line::from(vec![
                                        Span::styled(
                                            "  📦 ",
                                            Style::default(),
                                        ),
                                        Span::styled(
                                            format!("{} — {}", skill.name(), skill.description()),
                                            Style::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD),
                                        ),
                                    ]));
                                    inline::print_line(&Line::from(vec![
                                        Span::raw("     "),
                                        Span::styled(
                                            format!("Path: {}", skill.dir.display()),
                                            Style::default().fg(Color::Rgb(100, 100, 120)).add_modifier(Modifier::DIM),
                                        ),
                                    ]));
                                    inline::print_blank();

                                    // Show first few lines of the skill body
                                    let preview: Vec<&str> = skill.body.lines().take(5).collect();
                                    for line in &preview {
                                        inline::print_line(&Line::from(vec![
                                            Span::raw("     "),
                                            Span::styled(
                                                *line,
                                                Style::default().fg(Color::Rgb(180, 180, 200)),
                                            ),
                                        ]));
                                    }
                                    if skill.body.lines().count() > 5 {
                                        inline::print_line(&Line::from(vec![
                                            Span::raw("     "),
                                            Span::styled(
                                                "...",
                                                Style::default().fg(Color::Rgb(100, 100, 120)),
                                            ),
                                        ]));
                                    }
                                } else {
                                    inline::print_line(&components::warning_badge(
                                        &format!("Skill '{}' not found. Use /skills to list available skills.", name),
                                    ));
                                }
                                inline::print_blank();
                            }
                            ReplAction::Search(query) => {
                                commands.search_memory_inline(&query);
                            }
                            ReplAction::Image(path) => {
                                commands.attach_image_inline(&path);
                            }
                        }
                    }
                    continue;
                }

                // Handle plain text as task
                match input.to_lowercase().as_str() {
                    "exit" | "quit" | "q" => break,
                    "help" | "h" => print_help(),
                    "new" | "n" => {
                        // Save current session before clearing
                        if !current_session.messages.is_empty() {
                            let _ = crate::session::save(&current_session);
                        }
                        let cwd = std::env::current_dir()
                            .unwrap_or_default()
                            .display().to_string();
                        current_session = crate::session::create(
                            &cwd,
                            &model_info.provider,
                            &model_info.model,
                        );
                        conversation.clear();
                        stats.reset();
                        commands.restart_session();

                        crossterm::execute!(
                            std::io::stdout(),
                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                            crossterm::cursor::MoveTo(0, 0)
                        ).ok();
                        model_info = get_model_info(&commands);
                        print_banner(&model_info, &stats);
                        inline::print_blank();
                        inline::print_line(&components::success_badge(
                            "New session started.",
                        ));
                        inline::print_blank();
                        print_status_bar(&model_info, &stats);
                    }
                    _ => {
                        conversation.push(ConversationEntry {
                            role: "user".into(),
                            content: input.clone(),
                            timestamp: chrono::Local::now(),
                        });
                        stats.increment_messages();

                        // Push to session and auto-save
                        crate::session::push_message(
                            &mut current_session,
                            "user",
                            &input,
                        );

                        // Visual break between user input (drawn by reedline)
                        // and the assistant turn.
                        print_turn_separator();

                        // commands.run() owns its own transient spinner now,
                        // so we just time the call here.
                        let start = Instant::now();

                        if let Err(e) = commands.run(&input).await {
                            inline::print_blank();
                            inline::print_line(&components::error_badge(&e.to_string()));
                            inline::print_blank();
                        } else {
                            let elapsed = start.elapsed();

                            let estimated_input = (input.len() as f32 / 4.0) as u32;
                            stats.add_input_tokens(estimated_input);

                            conversation.push(ConversationEntry {
                                role: "assistant".into(),
                                content: format!(
                                    "(response in {:.1}s)",
                                    elapsed.as_secs_f64()
                                ),
                                timestamp: chrono::Local::now(),
                            });

                            // Push assistant response to session
                            crate::session::push_message(
                                &mut current_session,
                                "assistant",
                                &format!("(response in {:.1}s)", elapsed.as_secs_f64()),
                            );

                            // Auto-save session
                            let _ = crate::session::save(&current_session);

                            print_response_summary(&stats, elapsed.as_millis());
                        }
                    }
                }
            }
            Ok(Signal::CtrlC) => {
                inline::print_blank();
                inline::print_line(&components::info_badge("Use /quit or Ctrl+D to exit."));
                inline::print_blank();
                continue;
            }
            Ok(Signal::CtrlD) => {
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error: {:?}", e);
                break;
            }
        }
    }

    // Auto-save session on exit
    if !current_session.messages.is_empty() {
        let _ = crate::session::save(&current_session);
    }

    print_goodbye(&stats);
    Ok(())
}

// ── Model info ──────────────────────────────────────────────

struct ModelInfo {
    provider: String,
    model: String,
    api_base: String,
    /// File name of the loaded AGENT.md (e.g. `AGENT.md`). `None` when
    /// the walk-up didn't find one.
    agent_md_name: Option<String>,
    /// `true` when at least one persistent memory file was folded into
    /// the system prompt.
    memory_md_loaded: bool,
    /// `true` when the active model supports image input.
    vision_capable: bool,
    /// Name of the currently active skill, if any.
    active_skill: Option<String>,
}

fn get_model_info(commands: &Commands) -> ModelInfo {
    let (provider, model, api_base) = commands.model_info();
    let agent_md_name = commands
        .agent_md_path()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    ModelInfo {
        provider,
        model,
        api_base,
        agent_md_name,
        memory_md_loaded: commands.memory_md_loaded(),
        vision_capable: commands.active_model_capabilities().vision,
        active_skill: core_agentic::active_skill(),
    }
}

// ── REPL actions ────────────────────────────────────────────

enum ReplAction {
    Quit,
    NewSession,
    Config,
    History,
    Tools,
    Stats,
    Provider(String),
    Models,
    ModelsSwitch(String),
    Sessions,
    SessionsResume(String),
    Mcp,
    Plan(String),
    Skills,
    SkillsLoad(String),
    Search(String),
    Image(String),
}

fn handle_slash_command(input: &str) -> Option<ReplAction> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts
        .get(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    match cmd {
        "/quit" | "/q" | "/exit" => Some(ReplAction::Quit),
        "/help" | "/h" => {
            print_help();
            None
        }
        "/new" | "/n" | "/clear" | "/cls" => Some(ReplAction::NewSession),
        "/config" | "/cfg" => Some(ReplAction::Config),
        "/history" | "/hist" => Some(ReplAction::History),
        "/tools" | "/t" => Some(ReplAction::Tools),
        "/stats" => Some(ReplAction::Stats),
        "/mcp" => Some(ReplAction::Mcp),
        "/provider" if !arg.is_empty() => Some(ReplAction::Provider(arg)),
        "/provider" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Usage: /provider <name>"));
            inline::print_blank();
            None
        }
        "/models" | "/m" if !arg.is_empty() => Some(ReplAction::ModelsSwitch(arg)),
        "/models" | "/m" => Some(ReplAction::Models),
        "/sessions" | "/ss" if !arg.is_empty() => Some(ReplAction::SessionsResume(arg)),
        "/sessions" | "/ss" => Some(ReplAction::Sessions),
        "/plan" if !arg.is_empty() => Some(ReplAction::Plan(arg)),
        "/plan" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge("Usage: /plan <goal>"));
            inline::print_blank();
            None
        }
        "/skills" if !arg.is_empty() => Some(ReplAction::SkillsLoad(arg)),
        "/skills" => Some(ReplAction::Skills),
        "/search" | "/find" if !arg.is_empty() => Some(ReplAction::Search(arg)),
        "/search" | "/find" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge(
                "Usage: /search <query>  (case-insensitive substring match over conversation memory)",
            ));
            inline::print_blank();
            None
        }
        "/image" | "/img" if !arg.is_empty() => Some(ReplAction::Image(arg)),
        "/image" | "/img" => {
            inline::print_blank();
            inline::print_line(&components::warning_badge(
                "Usage: /image <path | data: url | http(s) url>",
            ));
            inline::print_blank();
            None
        }
        _ => {
            inline::print_blank();
            inline::print_line(&components::error_badge(
                &format!("Unknown command: {}", cmd),
            ));
            inline::print_line(&Line::from(vec![
                RSpan::raw("  Type "),
                RSpan::styled(
                    "/help",
                    RStyle::default()
                        .fg(Color::Rgb(255, 215, 0))
                        .add_modifier(Modifier::BOLD),
                ),
                RSpan::raw(" for available commands."),
            ]));
            inline::print_blank();
            None
        }
    }
}

// ── Print helpers (using shared widgets) ────────────────────

fn print_banner(model_info: &ModelInfo, stats: &SessionStats) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    inline::print_blank();

    // Gradient banner title
    let title = components::banner_title(
        "  █▀▀█ █▀▀ █▀▀█ █▀█ ▀█▀ █ █▀▀   ▇ ▅ ▃",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&title);
    let subtitle = components::banner_title(
        "  █▒░█ █▀▀ █▀▀█ █ █  █  █ █   ▉ ▅ ▁",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&subtitle);
    inline::print_blank();

    // Info panel
    let info_lines = vec![
        Line::from(vec![
            RSpan::styled("📂 ", RStyle::default()),
            RSpan::styled("cwd  ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(cwd, RStyle::default().fg(Color::Rgb(180, 180, 200))),
        ]),
        Line::from(vec![
            RSpan::styled("⚡ ", RStyle::default()),
            RSpan::styled("model", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::raw("  "),
            RSpan::styled(
                model_info.provider.clone(),
                RStyle::default().fg(Color::Rgb(64, 224, 208)).add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" / ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                model_info.model.clone(),
                RStyle::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("💡 ", RStyle::default()),
            RSpan::styled("tip  ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled("type ", RStyle::default()),
            RSpan::styled(
                "/help",
                RStyle::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" for commands, ", RStyle::default()),
            RSpan::styled(
                "@",
                RStyle::default().fg(Color::Rgb(135, 206, 250)).add_modifier(Modifier::BOLD),
            ),
            RSpan::styled(" to reference files", RStyle::default()),
        ]),
    ];

    // Add a 'context' line when AGENT.md or persistent memory is loaded
    // so users see at boot which extra sources are influencing the agent.
    let mut info_lines = info_lines;
    if model_info.agent_md_name.is_some() || model_info.memory_md_loaded || model_info.active_skill.is_some() {
        let mut spans: Vec<RSpan<'static>> = vec![
            RSpan::styled("🔗 ", RStyle::default()),
            RSpan::styled("ctx  ", RStyle::default().add_modifier(Modifier::DIM)),
        ];
        let mut first = true;
        if let Some(ref name) = model_info.agent_md_name {
            spans.push(RSpan::styled(
                format!("📄 {}", name),
                RStyle::default()
                    .fg(Color::Rgb(176, 196, 222))
                    .add_modifier(Modifier::BOLD),
            ));
            first = false;
        }
        if model_info.memory_md_loaded {
            if !first {
                spans.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.push(RSpan::styled(
                "🧠 memory.md",
                RStyle::default()
                    .fg(Color::Rgb(176, 196, 222))
                    .add_modifier(Modifier::BOLD),
            ));
            first = false;
        }
        if let Some(ref skill) = model_info.active_skill {
            if !first {
                spans.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.push(RSpan::styled(
                format!("⚡ skill:{}", skill),
                RStyle::default()
                    .fg(Color::Rgb(241, 196, 15))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        info_lines.push(Line::from(spans));
    }

    let panel_lines = components::panel(
        "Welcome",
        &info_lines,
        components::BoxStyle::Rounded,
        Color::Rgb(100, 100, 140),
    );
    inline::print_lines(&panel_lines);
    inline::print_blank();

    print_status_bar(model_info, stats);
}

/// Light separator drawn between a user turn and the assistant response.
fn print_turn_separator() {
    inline::print_blank();
    inline::print_line(&components::dotted_separator(Color::Rgb(80, 80, 100)));
    inline::print_blank();
}

fn print_status_bar(model_info: &ModelInfo, stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    let sep = RSpan::styled(
        "  │  ",
        RStyle::default().fg(Color::Rgb(60, 60, 80)),
    );

    let cache_ratio = stats.cache_hit_ratio();

    inline::print_line(&Line::from(vec![
        RSpan::raw("  "),
        RSpan::styled(
            format!("⚡ {}", model_info.provider),
            RStyle::default().fg(Color::Rgb(255, 215, 0)),
        ),
        RSpan::styled(
            format!("/{}", model_info.model),
            RStyle::default().fg(Color::Rgb(241, 196, 15)).add_modifier(Modifier::DIM),
        ),
        if model_info.vision_capable {
            RSpan::styled(
                "  👁",
                RStyle::default().fg(Color::Rgb(135, 206, 250)),
            )
        } else {
            RSpan::raw("")
        },
        sep.clone(),
        RSpan::styled(
            format!("💬 {} msgs", stats.messages_sent()),
            RStyle::default().fg(Color::Rgb(135, 206, 250)),
        ),
        sep.clone(),
        RSpan::styled(
            format!("📊 {} ↑ / {} ↓", in_tok, out_tok),
            RStyle::default().fg(Color::Rgb(186, 85, 211)),
        ),
        if cache_ratio > 0.0 {
            let pct = (cache_ratio * 100.0) as u8;
            RSpan::styled(
                format!("  📦 {}% cached", pct),
                RStyle::default().fg(Color::Rgb(46, 204, 113)),
            )
        } else {
            RSpan::raw("")
        },
        sep,
        RSpan::styled(
            format!("⏱ {}", stats.elapsed_str()),
            RStyle::default().fg(Color::Rgb(46, 204, 113)),
        ),
    ]));

    // Context-source indicator row. Only printed when at least one of
    // AGENT.md / persistent memory / active skill — keeps the main status
    // bar uncluttered for plain runs.
    let has_agent_md = model_info.agent_md_name.is_some();
    let has_skill = model_info.active_skill.is_some();
    if has_agent_md || model_info.memory_md_loaded || has_skill {
        let mut chips: Vec<RSpan<'static>> = vec![RSpan::raw("  ")];
        if let Some(ref name) = model_info.agent_md_name {
            chips.push(RSpan::styled(
                format!("📄 {}", name),
                RStyle::default().fg(Color::Rgb(176, 196, 222)),
            ));
        }
        if model_info.memory_md_loaded {
            if has_agent_md {
                chips.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().fg(Color::Rgb(60, 60, 80)),
                ));
            }
            chips.push(RSpan::styled(
                "🧠 memory.md",
                RStyle::default().fg(Color::Rgb(176, 196, 222)),
            ));
        }
        if let Some(ref skill) = model_info.active_skill {
            if has_agent_md || model_info.memory_md_loaded {
                chips.push(RSpan::styled(
                    "  ·  ",
                    RStyle::default().fg(Color::Rgb(60, 60, 80)),
                ));
            }
            chips.push(RSpan::styled(
                format!("⚡ skill:{}", skill),
                RStyle::default()
                    .fg(Color::Rgb(241, 196, 15))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        inline::print_line(&Line::from(chips));
    }

    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}

fn print_response_summary(stats: &SessionStats, ms: u128) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    let sep = RSpan::styled(
        "  │  ",
        RStyle::default().fg(Color::Rgb(60, 60, 80)),
    );

    let cache_read = stats.total_cache_read_tokens();
    let cache_created = stats.total_cache_creation_tokens();
    let has_cache = cache_read > 0 || cache_created > 0;

    inline::print_blank();
    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_line(&Line::from(vec![
        RSpan::raw("  "),
        RSpan::styled(
            " ✓ done ",
            RStyle::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(Color::Rgb(39, 174, 96))
                .add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
        RSpan::styled(
            format!("⏱ {}.{:03}s", ms / 1000, ms % 1000),
            RStyle::default().fg(Color::Rgb(180, 180, 200)),
        ),
        sep.clone(),
        RSpan::styled(
            format!("💬 {} msgs", stats.messages_sent()),
            RStyle::default().fg(Color::Rgb(180, 180, 200)),
        ),
        sep.clone(),
        RSpan::styled(
            format!("📊 {} ↑ / {} ↓", in_tok, out_tok),
            RStyle::default().fg(Color::Rgb(180, 180, 200)),
        ),
        if has_cache {
            let pct = (stats.cache_hit_ratio() * 100.0) as u8;
            RSpan::styled(
                format!("  📦 {}% cached", pct),
                RStyle::default().fg(Color::Rgb(46, 204, 113)),
            )
        } else {
            RSpan::raw("")
        },
        sep,
        RSpan::styled(
            format!("session {}", stats.elapsed_str()),
            RStyle::default().fg(Color::Rgb(180, 180, 200)),
        ),
    ]));
    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}


fn print_help() {
    let help_md = r#"## 📖 Commands

**Slash commands:**
- `/help`              Show this help
- `/new`               Start a new session (clears conversation)
- `/config`            Show current configuration
- `/history`           Show conversation history
- `/tools`             List available tools
- `/stats`             Show session statistics
- `/mcp`               Show MCP server status
- `/skills`            List all indexed skills
- `/skills <name>`     Load and display a skill
- `/sessions`          List previous sessions
- `/sessions <id>`     Resume a previous session
- `/plan <goal>`       Create a plan for a goal
- `/search <query>`    Search conversation memory (case-insensitive)
- `/image <path>`      Attach image for next turn (path | data: | http(s) URL)
- `/provider <name>`   Switch provider (not yet supported)
- `/models`            Pick model interactively
- `/models <name>`     Switch to model by name (supports auto-complete)
- `/quit`              Exit interactive mode

**Shortcuts:**
- `help`, `h`          Show help
- `new`, `n`           New session
- `exit`, `q`          Exit

**Completion & Hints:**
- `/` → Popup with command list + descriptions
- `@` → Popup with file list + icons
- Tab → Navigate/open completion menu
- → (Right Arrow) Accept inline hint

**Tips:**
- Type any text to send as a task to the AI agent
- Use `/skills <name>` to load a skill before starting a task
- Ctrl+R to search command history
- Ctrl+C to cancel, Ctrl+D to exit
"#;

    inline::print_blank();
    inline::print_line(&components::section_header(
        "📖",
        "Help",
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    let md = crate::widgets::markdown::MarkdownContent::parse(help_md);
    inline::print_lines(&md.lines);
    inline::print_blank();
}

fn show_stats(stats: &SessionStats, model_info: &ModelInfo) {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    let in_tok = stats.total_input_tokens();
    let out_tok = stats.total_output_tokens();
    let total_tok = in_tok + out_tok;

    inline::print_blank();
    inline::print_line(&components::section_header(
        "📊",
        "Session Statistics",
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    // Session subsection
    inline::print_line(&components::subsection_header(
        "Session",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Duration",
        &stats.elapsed_str(),
        12,
        Color::Rgb(46, 204, 113),
    ));
    inline::print_line(&components::kv_line(
        "Messages",
        &format!("{}", stats.messages_sent()),
        12,
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Tool calls",
        &format!("{}", stats.tool_calls()),
        12,
        Color::Rgb(135, 206, 250),
    ));
    inline::print_blank();

    // Model subsection
    inline::print_line(&components::subsection_header(
        "Model",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_badge(
        "Provider",
        &model_info.provider,
        12,
        Color::Rgb(255, 255, 255),
        Color::Rgb(155, 89, 182),
    ));
    inline::print_line(&components::kv_badge(
        "Model",
        &model_info.model,
        12,
        Color::Rgb(255, 255, 255),
        Color::Rgb(52, 152, 219),
    ));
    inline::print_line(&components::kv_line(
        "API Base",
        &model_info.api_base,
        12,
        Color::Rgb(180, 180, 200),
    ));
    inline::print_blank();

    // Token usage subsection
    inline::print_line(&components::subsection_header(
        "Token Usage",
        Color::Rgb(255, 215, 0),
    ));

    if total_tok > 0 {
        let in_ratio = in_tok as f32 / total_tok as f32;
        let out_ratio = out_tok as f32 / total_tok as f32;
        inline::print_line(&components::labeled_bar(
            "Input",
            in_ratio,
            30,
            Color::Rgb(46, 204, 113),
            Color::Rgb(50, 50, 60),
        ));
        inline::print_line(&components::labeled_bar(
            "Output",
            out_ratio,
            30,
            Color::Rgb(231, 76, 60),
            Color::Rgb(50, 50, 60),
        ));
    } else {
        inline::print_line(&Line::from(vec![
            RSpan::styled(
                "  Input:        ",
                RStyle::default().add_modifier(Modifier::DIM),
            ),
            RSpan::styled("— no data yet —", RStyle::default().add_modifier(Modifier::DIM)),
        ]));
    }
    inline::print_line(&Line::from(vec![
        RSpan::styled(
            "  Total:        ",
            RStyle::default().add_modifier(Modifier::DIM),
        ),
        RSpan::styled(
            format!("{} tokens", stats.format_tokens(total_tok)),
            RStyle::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD),
        ),
    ]));
    inline::print_blank();

    // Cache subsection (only shown when cache metrics exist)
    let cache_read = stats.total_cache_read_tokens();
    let cache_created = stats.total_cache_creation_tokens();
    if cache_read > 0 || cache_created > 0 {
        inline::print_line(&components::subsection_header(
            "Prompt Cache",
            Color::Rgb(46, 204, 113),
        ));
        inline::print_line(&components::kv_line(
            "Cache read",
            &format!("{} tokens", stats.format_tokens(cache_read)),
            14,
            Color::Rgb(46, 204, 113),
        ));
        inline::print_line(&components::kv_line(
            "Cache created",
            &format!("{} tokens", stats.format_tokens(cache_created)),
            14,
            Color::Rgb(52, 152, 219),
        ));
        let ratio = stats.cache_hit_ratio();
        inline::print_line(&components::kv_line(
            "Hit ratio",
            &format!("{:.0}%", ratio * 100.0),
            14,
            Color::Rgb(241, 196, 15),
        ));
        inline::print_blank();
    }

    // Environment subsection
    inline::print_line(&components::subsection_header(
        "Environment",
        Color::Rgb(255, 215, 0),
    ));
    inline::print_line(&components::kv_line(
        "Working dir",
        &cwd,
        12,
        Color::Rgb(180, 180, 200),
    ));
    inline::print_blank();
    inline::print_line(&components::dashed_separator(Color::Rgb(60, 60, 80)));
    inline::print_blank();
}

fn show_history(conversation: &[ConversationEntry]) {
    inline::print_blank();
    if conversation.is_empty() {
        inline::print_line(&components::warning_badge("No messages in this session yet."));
        inline::print_blank();
        return;
    }

    inline::print_line(&components::section_header(
        "📜",
        &format!("Conversation History ({} messages)", conversation.len()),
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    for (i, entry) in conversation.iter().enumerate() {
        let time = entry.timestamp.format("%H:%M:%S");
        let (icon, badge_bg) = match entry.role.as_str() {
            "user" => ("👤", Color::Rgb(52, 152, 219)),
            "assistant" => ("🤖", Color::Rgb(46, 204, 113)),
            _ => ("💬", Color::Rgb(241, 196, 15)),
        };
        let content_preview = if entry.content.len() > 120 {
            format!("{}...", &entry.content[..117])
        } else {
            entry.content.clone()
        };
        inline::print_line(&Line::from(vec![
            RSpan::raw("  "),
            RSpan::styled(
                format!(" {} ", icon),
                RStyle::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(badge_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" "),
            RSpan::styled(
                format!("#{:02}", i + 1),
                RStyle::default().fg(Color::Rgb(180, 180, 200)).add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" "),
            RSpan::styled(
                format!("[{}]", time),
                RStyle::default().add_modifier(Modifier::DIM),
            ),
            RSpan::raw("  "),
            RSpan::raw(content_preview),
        ]));
    }
    inline::print_blank();
}

fn print_goodbye(stats: &SessionStats) {
    let in_tok = stats.format_tokens(stats.total_input_tokens());
    let out_tok = stats.format_tokens(stats.total_output_tokens());

    inline::print_blank();

    let cache_read = stats.total_cache_read_tokens();
    let cache_created = stats.total_cache_creation_tokens();
    let has_cache = cache_read > 0 || cache_created > 0;

    let mut summary_lines = vec![
        Line::from(vec![
            RSpan::styled("💬 ", RStyle::default()),
            RSpan::styled("Messages ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("{}", stats.messages_sent()),
                RStyle::default().fg(Color::Rgb(135, 206, 250)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("⏱ ", RStyle::default()),
            RSpan::styled("Duration ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                stats.elapsed_str(),
                RStyle::default().fg(Color::Rgb(46, 204, 113)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            RSpan::styled("📊 ", RStyle::default()),
            RSpan::styled("Tokens   ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("{} ↑", in_tok),
                RStyle::default().fg(Color::Rgb(46, 204, 113)),
            ),
            RSpan::raw(" / "),
            RSpan::styled(
                format!("{} ↓", out_tok),
                RStyle::default().fg(Color::Rgb(231, 76, 60)),
            ),
        ]),
    ];

    if has_cache {
        summary_lines.push(Line::from(vec![
            RSpan::styled("📦 ", RStyle::default()),
            RSpan::styled("Cache    ", RStyle::default().add_modifier(Modifier::DIM)),
            RSpan::styled(
                format!("💰 {} rd", stats.format_tokens(cache_read)),
                RStyle::default().fg(Color::Rgb(46, 204, 113)).add_modifier(Modifier::BOLD),
            ),
            RSpan::raw(" / "),
            RSpan::styled(
                format!("✏️ {} cr", stats.format_tokens(cache_created)),
                RStyle::default().fg(Color::Rgb(52, 152, 219)).add_modifier(Modifier::BOLD),
            ),
            RSpan::raw("  "),
            RSpan::styled(
                format!("({:.0}% hit)", stats.cache_hit_ratio() * 100.0),
                RStyle::default().fg(Color::Rgb(241, 196, 15)),
            ),
        ]));
    }

    let panel_lines = components::panel(
        "Session Summary",
        &summary_lines,
        components::BoxStyle::Rounded,
        Color::Rgb(100, 100, 140),
    );
    inline::print_lines(&panel_lines);
    inline::print_blank();

    let goodbye = components::gradient_text(
        "  👋 See you next time!",
        Color::Rgb(255, 105, 180),
        Color::Rgb(64, 224, 208),
    );
    inline::print_line(&goodbye);
    inline::print_blank();
}

// ── Save/Load conversation ──────────────────────────────────

fn show_sessions() {
    inline::print_blank();

    let sessions = match crate::session::list() {
        Ok(s) => s,
        Err(e) => {
            inline::print_line(&components::error_badge(
                &format!("Failed to list sessions: {}", e),
            ));
            inline::print_blank();
            return;
        }
    };

    if sessions.is_empty() {
        inline::print_line(&components::warning_badge("No previous sessions found."));
        inline::print_blank();
        return;
    }

    inline::print_line(&components::section_header(
        "📜",
        &format!("Sessions ({})", sessions.len()),
        Color::Rgb(64, 224, 208),
    ));
    inline::print_blank();

    let bold = RStyle::default().add_modifier(Modifier::BOLD);
    let dim = RStyle::default().add_modifier(Modifier::DIM);

    for (i, s) in sessions.iter().enumerate().take(20) {
        let time = crate::session::format_relative_time(&s.updated_at);

        let title = if s.title.is_empty() {
            "Untitled"
        } else {
            &s.title
        };

        inline::print_line(&Line::from(vec![
            RSpan::styled(format!("  {:2}. ", i + 1), dim.clone()),
            RSpan::styled(title.to_string(), bold.clone()),
            RSpan::styled(format!("  {} msgs", s.message_count), dim.clone()),
            RSpan::raw("  "),
            RSpan::styled(time, RStyle::default().fg(Color::Rgb(135, 206, 250))),
        ]));
        inline::print_line(&Line::from(vec![
            RSpan::styled("      ", RStyle::default()),
            RSpan::styled(format!("{}", s.id), dim.clone()),
            RSpan::styled(format!(" · {} · {}/{}", s.directory, s.provider, s.model), dim.clone()),
        ]));
        inline::print_blank();
    }

    if sessions.len() > 20 {
        inline::print_line(&Line::from(vec![
            RSpan::styled(
                format!("  ... and {} more", sessions.len() - 20),
                dim.clone(),
            ),
        ]));
        inline::print_blank();
    }

    inline::print_line(&Line::from(vec![
        RSpan::styled("💡 ", RStyle::default()),
        RSpan::styled("Tip: ", RStyle::default().add_modifier(Modifier::DIM)),
        RSpan::raw("Use "),
        RSpan::styled(
            "/sessions <id>",
            RStyle::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD),
        ),
        RSpan::raw(" to resume a session"),
    ]));
    inline::print_blank();
}
