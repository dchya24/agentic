//! Safety system for agentic AI agent orchestration.
//!
//! Provides risk scoring, configurable thresholds, pattern-based detection,
//! command blocklist, path sandboxing, rate limiting, and audit logging.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Permission Mode
// ---------------------------------------------------------------------------

/// High-level permission mode that gates the safety engine.
///
/// Maps directly to the architecture doc's three modes:
/// - `Default` — ask for writes & commands, allow reads (current behavior).
/// - `Plan`    — read-only mode. All writes/commands are denied.
/// - `Yolo`    — allow everything (dangerous; for trusted automation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    #[default]
    Default,
    Plan,
    Yolo,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::Plan => "plan",
            PermissionMode::Yolo => "yolo",
        }
    }

    /// Parse from a free-form string (case-insensitive). Accepts a few
    /// common synonyms so CLI users don't have to memorize exact spelling.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "default" | "normal" | "ask" => Some(PermissionMode::Default),
            "plan" | "readonly" | "read-only" | "dry-run" => Some(PermissionMode::Plan),
            "yolo" | "auto" | "trust" | "unsafe" => Some(PermissionMode::Yolo),
            _ => None,
        }
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tool actions that mutate state. Used by Plan mode to deny writes
/// without having to enumerate every shell command.
fn is_state_changing_action(action: &str) -> bool {
    matches!(
        action,
        "write_file"
            | "edit_file"
            | "delete_file"
            | "run_command"
            | "run_script"
    )
}

// ---------------------------------------------------------------------------
// Risk Level
// ---------------------------------------------------------------------------

/// Numeric risk score with categorized level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskScore {
    /// 0.0 (safe) to 1.0 (extremely dangerous)
    pub value: f64,
    pub level: RiskLevel,
}

impl RiskScore {
    pub fn new(value: f64, level: RiskLevel) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            level,
        }
    }

    pub fn low() -> Self {
        Self {
            value: 0.1,
            level: RiskLevel::Low,
        }
    }

    pub fn medium() -> Self {
        Self {
            value: 0.5,
            level: RiskLevel::Medium,
        }
    }

    pub fn high() -> Self {
        Self {
            value: 0.8,
            level: RiskLevel::High,
        }
    }

    pub fn critical() -> Self {
        Self {
            value: 1.0,
            level: RiskLevel::Critical,
        }
    }
}

/// Categorized risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }

    pub fn from_score(value: f64) -> Self {
        if value >= 0.8 {
            RiskLevel::Critical
        } else if value >= 0.6 {
            RiskLevel::High
        } else if value >= 0.3 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        }
    }
}

// ---------------------------------------------------------------------------
// Confirmation Request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    pub action: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub risk_score: f64,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Safety Config (serializable)
// ---------------------------------------------------------------------------

/// Fully configurable safety configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// Whether safety checks are enabled at all.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Whether to require confirmation for medium+ risk actions.
    #[serde(default = "default_true")]
    pub require_confirmation: bool,

    /// Auto-approve actions with risk score <= this threshold.
    #[serde(default = "default_auto_approve_threshold")]
    pub auto_approve_threshold: f64,

    /// Risk score threshold for "Medium" classification.
    #[serde(default = "default_medium_threshold")]
    pub medium_threshold: f64,

    /// Risk score threshold for "High" classification.
    #[serde(default = "default_high_threshold")]
    pub high_threshold: f64,

    /// Risk score threshold for "Critical" classification.
    #[serde(default = "default_critical_threshold")]
    pub critical_threshold: f64,

    /// Hard-blocked command patterns (exact + substring match).
    #[serde(default = "default_blocked_commands")]
    pub blocked_commands: Vec<String>,

    /// Regex patterns that trigger automatic risk scoring.
    #[serde(default = "default_risk_patterns")]
    pub risk_patterns: Vec<RiskPattern>,

    /// Allowed command prefixes — if non-empty, only these are permitted.
    #[serde(default)]
    pub allowed_commands: Vec<String>,

    /// Sandbox directory roots. File operations outside these are blocked.
    #[serde(default)]
    pub sandbox_paths: Vec<String>,

    /// Per-tool rate limiting configuration.
    #[serde(default)]
    pub rate_limits: HashMap<String, RateLimit>,

    /// Whether to emit audit log entries.
    #[serde(default = "default_true")]
    pub audit_logging: bool,

    /// Max number of audit entries to keep in memory (ring buffer).
    #[serde(default = "default_audit_capacity")]
    pub audit_capacity: usize,
}

fn default_true() -> bool {
    true
}
fn default_auto_approve_threshold() -> f64 {
    0.3
}
fn default_medium_threshold() -> f64 {
    0.3
}
fn default_high_threshold() -> f64 {
    0.6
}
fn default_critical_threshold() -> f64 {
    0.8
}
fn default_audit_capacity() -> usize {
    1000
}

fn default_blocked_commands() -> Vec<String> {
    vec![
        "rm -rf /".into(),
        "rm -rf /*".into(),
        "rm -rf ~".into(),
        "del /f".into(),
        "format:".into(),
        "mkfs".into(),
        "dd if=/dev/zero".into(),
        "dd if=/dev/random".into(),
        ":(){ :|:& };:".into(),          // fork bomb
        "chmod -R 777 /".into(),
        "chown -R".into(),
        "> /dev/sda".into(),
    ]
}

fn default_risk_patterns() -> Vec<RiskPattern> {
    vec![
        // --- Critical (≥0.8) ---
        RiskPattern::new("rm_recursive_root", r"(?i)\brm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?(/[^\s]*|\*|~)", 0.95)
            .with_reason("Recursive deletion of root/home/wildcard"),
        RiskPattern::new("format_disk", r"(?i)\b(mkfs|format)\b", 0.9)
            .with_reason("Disk formatting"),
        RiskPattern::new("dd_disk", r"(?i)\bdd\s+if=", 0.9)
            .with_reason("Raw disk write with dd"),
        RiskPattern::new("fork_bomb", r":\(\)\{\s*:\|:&\s*\};:", 0.95)
            .with_reason("Fork bomb detected"),
        RiskPattern::new("chmod_777_recursive", r"(?i)\bchmod\s+(-R\s+)?777\s+/", 0.85)
            .with_reason("Recursive 777 permission change on root"),
        // --- High (≥0.6) ---
        RiskPattern::new("force_delete", r"(?i)\brm\s+(-[a-zA-Z]*f[a-zA-Z]*|-r[a-zA-Z]*\s)", 0.7)
            .with_reason("Force/recursive delete"),
        RiskPattern::new("overwrite_device", r"(?i)>\s*/dev/(sd|hd|nvme|vd|loop)", 0.85)
            .with_reason("Writing directly to block device"),
        RiskPattern::new("kill_all", r"(?i)\bkillall?\s+(-9\s+)?(\*|\d+)", 0.65)
            .with_reason("Kill all processes"),
        RiskPattern::new("iptables_flush", r"(?i)\biptables\s+-F", 0.7)
            .with_reason("Flushing all firewall rules"),
        RiskPattern::new("git_reset_hard", r"(?i)\bgit\s+reset\s+--hard", 0.65)
            .with_reason("Hard git reset (uncommitted changes lost)"),
        // --- Medium (≥0.3) ---
        RiskPattern::new("sudo", r"(?i)\bsudo\b", 0.5)
            .with_reason("Elevated privileges via sudo"),
        RiskPattern::new("su_switch", r"(?i)\bsu\s+(-|\w)", 0.55)
            .with_reason("Switching user"),
        RiskPattern::new("network_download", r"(?i)\b(curl|wget)\s+", 0.35)
            .with_reason("Network download"),
        RiskPattern::new("network_upload", r"(?i)\b(curl|wget|scp|rsync)\b.*(-T|--upload-file)", 0.5)
            .with_reason("Network upload"),
        RiskPattern::new("pip_install", r"(?i)\bpip\s+install\b", 0.35)
            .with_reason("Installing Python packages"),
        RiskPattern::new("npm_global", r"(?i)\bnpm\s+install\s+-g\b", 0.35)
            .with_reason("Global npm install"),
        RiskPattern::new("move_rename", r"(?i)\bmv\s+", 0.3)
            .with_reason("Moving/renaming files"),
        RiskPattern::new("delete_file", r"(?i)\b(rm\s+|del\s+)", 0.4)
            .with_reason("Deleting files"),
        RiskPattern::new("git_clean", r"(?i)\bgit\s+clean\s+", 0.45)
            .with_reason("Git clean removes untracked files"),
        RiskPattern::new("docker_rm", r"(?i)\bdocker\s+(rm|rmi)\b", 0.35)
            .with_reason("Removing docker containers/images"),
        // --- Low (<0.3) ---
        RiskPattern::new("read_only", r"(?i)\b(ls|cat|head|tail|less|more|find|grep|wc|file|stat)\b", 0.05)
            .with_reason("Read-only command"),
        RiskPattern::new("git_read", r"(?i)\bgit\s+(log|status|diff|show|branch|tag)\b", 0.05)
            .with_reason("Read-only git command"),
    ]
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_confirmation: true,
            auto_approve_threshold: 0.3,
            medium_threshold: 0.3,
            high_threshold: 0.6,
            critical_threshold: 0.8,
            blocked_commands: default_blocked_commands(),
            risk_patterns: default_risk_patterns(),
            allowed_commands: vec![],
            sandbox_paths: vec![],
            rate_limits: HashMap::new(),
            audit_logging: true,
            audit_capacity: 1000,
        }
    }
}

// ---------------------------------------------------------------------------
// Risk Pattern (regex-based)
// ---------------------------------------------------------------------------

/// A regex pattern that maps to a risk score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskPattern {
    pub name: String,
    pub pattern: String,
    pub score: f64,
    #[serde(default)]
    pub reason: String,
    #[serde(skip)]
    compiled: Option<Regex>,
}

impl RiskPattern {
    pub fn new(name: impl Into<String>, pattern: &str, score: f64) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.to_string(),
            score,
            reason: String::new(),
            compiled: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// Lazy-compile and match. Returns true if the pattern matches.
    pub fn matches(&mut self, text: &str) -> bool {
        if self.compiled.is_none() {
            self.compiled = Regex::new(&self.pattern).ok();
        }
        self.compiled
            .as_ref()
            .map(|re| re.is_match(text))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Rate Limiting
// ---------------------------------------------------------------------------

/// Rate limit configuration per tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Max invocations allowed within the window.
    pub max_calls: u32,
    /// Window duration in seconds.
    pub window_secs: u64,
}

/// Tracks rate-limit state for a single tool.
#[derive(Debug)]
struct RateLimitState {
    calls: Vec<Instant>,
}

impl RateLimitState {
    fn new() -> Self {
        Self { calls: Vec::new() }
    }

    fn check_and_record(&mut self, limit: &RateLimit) -> bool {
        let now = Instant::now();
        let cutoff = now - std::time::Duration::from_secs(limit.window_secs);
        self.calls.retain(|t| *t > cutoff);

        if (self.calls.len() as u32) >= limit.max_calls {
            return false; // rate limited
        }
        self.calls.push(now);
        true
    }
}

// ---------------------------------------------------------------------------
// Audit Log Entry
// ---------------------------------------------------------------------------

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub target: Option<String>,
    pub risk_score: RiskScore,
    pub decision: AuditDecision,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditDecision {
    Approved,
    Blocked,
    RateLimited,
    DeniedByConfirmation,
}

// ---------------------------------------------------------------------------
// Safety Engine
// ---------------------------------------------------------------------------

/// Main safety engine. Thread-safe via interior mutability.
pub struct Safety {
    config: SafetyConfig,
    rate_limit_states: Mutex<HashMap<String, RateLimitState>>,
    audit_log: Mutex<Vec<AuditEntry>>,
    /// Active permission mode. Defaults to `Default` (ask).
    mode: Mutex<PermissionMode>,
}

impl Default for Safety {
    fn default() -> Self {
        Self::new()
    }
}

impl Safety {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    pub fn new() -> Self {
        Self {
            config: SafetyConfig::default(),
            rate_limit_states: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            mode: Mutex::new(PermissionMode::default()),
        }
    }

    pub fn with_config(config: SafetyConfig) -> Self {
        Self {
            config,
            rate_limit_states: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            mode: Mutex::new(PermissionMode::default()),
        }
    }

    pub fn with_blocked_commands(mut self, commands: Vec<String>) -> Self {
        self.config.blocked_commands = commands;
        self
    }

    pub fn with_allowed_commands(mut self, commands: Vec<String>) -> Self {
        self.config.allowed_commands = commands;
        self
    }

    pub fn with_sandbox_paths(mut self, paths: Vec<String>) -> Self {
        self.config.sandbox_paths = paths;
        self
    }

    pub fn set_require_confirmation(&mut self, require: bool) {
        self.config.require_confirmation = require;
    }

    /// Switch the active permission mode at runtime.
    pub fn set_mode(&self, mode: PermissionMode) {
        *self.mode.lock().unwrap() = mode;
    }

    /// Get the active permission mode.
    pub fn mode(&self) -> PermissionMode {
        *self.mode.lock().unwrap()
    }

    // -----------------------------------------------------------------------
    // Core Risk Scoring
    // -----------------------------------------------------------------------

    /// Score a command/action and return a numeric risk assessment.
    pub fn score_command(&self, action: &str, target: Option<&str>) -> RiskScore {
        if !self.config.enabled {
            return RiskScore::low();
        }

        let target_str = target.unwrap_or("");
        let combined = format!("{} {}", action, target_str).to_lowercase();

        // 1) Hard blocklist check → automatic critical
        for blocked in &self.config.blocked_commands {
            if combined.contains(&blocked.to_lowercase()) {
                return RiskScore::new(1.0, RiskLevel::Critical);
            }
        }

        // 2) Allowed commands allowlist check (if configured)
        if !self.config.allowed_commands.is_empty() {
            let cmd_part = target_str
                .split_whitespace()
                .next()
                .unwrap_or(target_str);
            let is_allowed = self
                .config
                .allowed_commands
                .iter()
                .any(|a| cmd_part.eq_ignore_ascii_case(a.trim()));

            if !is_allowed && action == "run_command" {
                return RiskScore::new(0.7, RiskLevel::High);
            }
        }

        // 3) Pattern-based scoring — take the highest match
        let mut best_score = 0.0_f64;

        // We need &mut for lazy regex compilation, so we clone patterns
        // to avoid borrowing issues. The patterns are compiled once.
        let mut patterns = self.config.risk_patterns.clone();
        for pattern in &mut patterns {
            if pattern.matches(&combined) {
                if pattern.score > best_score {
                    best_score = pattern.score;
                }
            }
        }

        // Also score based on action type
        let action_base = match action {
            "run_command" => 0.0, // scored by patterns above
            "write_file" | "edit_file" => 0.2,
            "delete_file" => 0.35,
            _ => 0.0,
        };

        if action_base > best_score {
            best_score = action_base;
        }

        // Classify using configurable thresholds
        let level = if best_score >= self.config.critical_threshold {
            RiskLevel::Critical
        } else if best_score >= self.config.high_threshold {
            RiskLevel::High
        } else if best_score >= self.config.medium_threshold {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        RiskScore::new(best_score, level)
    }

    /// Legacy compatibility: return RiskLevel for an action.
    pub fn check_risk(&self, action: &str, target: Option<&str>) -> RiskLevel {
        self.score_command(action, target).level
    }

    // -----------------------------------------------------------------------
    // Path Sandboxing
    // -----------------------------------------------------------------------

    /// Check if a file path is within allowed sandbox boundaries.
    /// Returns `true` if the path is allowed (or if no sandbox is configured).
    pub fn is_path_allowed(&self, path: &str) -> bool {
        if self.config.sandbox_paths.is_empty() {
            return true; // no sandbox configured → everything allowed
        }

        let resolved = Path::new(path);

        // Block traversal attempts
        if path.contains("..") {
            debug!(path = path, "Path traversal attempt blocked");
            return false;
        }

        self.config
            .sandbox_paths
            .iter()
            .any(|sandbox| resolved.starts_with(sandbox))
    }

    /// Return a RiskScore for a file operation, incorporating sandbox check.
    pub fn score_file_operation(&self, path: &str, operation: &str) -> RiskScore {
        if !self.is_path_allowed(path) {
            return RiskScore::new(0.9, RiskLevel::Critical);
        }

        let mut score = self.score_command(operation, Some(path));

        // Extra risk for sensitive system paths
        let sensitive = [
            "/etc/",
            "/usr/",
            "/boot/",
            "/sys/",
            "/proc/",
            "/dev/",
            "C:\\Windows\\",
            "C:\\Program Files\\",
        ];
        for prefix in &sensitive {
            if path.starts_with(prefix) {
                score.value = (score.value + 0.3).min(1.0);
                score.level = RiskLevel::from_score(score.value);
                break;
            }
        }

        score
    }

    // -----------------------------------------------------------------------
    // Rate Limiting
    // -----------------------------------------------------------------------

    /// Check rate limit for a tool. Returns `true` if the call is allowed.
    pub fn check_rate_limit(&self, tool_name: &str) -> bool {
        let limit = match self.config.rate_limits.get(tool_name) {
            Some(l) => l,
            None => return true, // no limit configured → allowed
        };

        let mut states = self.rate_limit_states.lock().unwrap();
        let state = states
            .entry(tool_name.to_string())
            .or_insert_with(RateLimitState::new);

        state.check_and_record(limit)
    }

    // -----------------------------------------------------------------------
    // Confirmation & Decision
    // -----------------------------------------------------------------------

    /// Should the user be prompted for confirmation?
    pub fn needs_confirmation(&self, action: &str, target: Option<&str>) -> bool {
        let mode = *self.mode.lock().unwrap();
        match mode {
            // Yolo never asks.
            PermissionMode::Yolo => false,
            // Plan denies state-changing actions outright (no confirmation).
            PermissionMode::Plan if is_state_changing_action(action) => false,
            _ => {
                if !self.config.enabled || !self.config.require_confirmation {
                    return false;
                }
                let score = self.score_command(action, target);
                score.value > self.config.auto_approve_threshold
            }
        }
    }

    /// Full safety evaluation: returns a decision without blocking.
    /// This is the primary entry point for the orchestrator.
    pub fn evaluate(&self, action: &str, target: Option<&str>) -> SafetyDecision {
        let mode = *self.mode.lock().unwrap();

        // Plan mode: hard-deny any state-changing tool. Reads still allowed.
        if mode == PermissionMode::Plan && is_state_changing_action(action) {
            let score = RiskScore::new(0.5, RiskLevel::Medium);
            self.audit(action, target, &score, AuditDecision::Blocked);
            return SafetyDecision {
                score,
                allowed: false,
                needs_confirmation: false,
                reason: format!("Blocked by plan mode: {} is a state-changing tool", action),
            };
        }

        // Yolo mode: allow everything except the hard blocklist (which is
        // checked below as part of normal scoring). We bypass confirmation
        // and rate limiting but keep critical-pattern blocking as a final
        // safety net.
        if mode == PermissionMode::Yolo {
            let score = self.score_command(action, target);
            if score.level == RiskLevel::Critical {
                self.audit(action, target, &score, AuditDecision::Blocked);
                return SafetyDecision {
                    score,
                    allowed: false,
                    needs_confirmation: false,
                    reason: "Action blocked: critical risk level (yolo mode still blocks blocklist)"
                        .into(),
                };
            }
            return SafetyDecision {
                score,
                allowed: true,
                needs_confirmation: false,
                reason: "yolo mode: auto-approved".into(),
            };
        }

        if !self.config.enabled {
            return SafetyDecision {
                score: RiskScore::low(),
                allowed: true,
                needs_confirmation: false,
                reason: "Safety system disabled".into(),
            };
        }

        // 1) Score the action
        let score = self.score_command(action, target);

        // 2) Critical actions are always blocked
        if score.level == RiskLevel::Critical {
            self.audit(action, target, &score, AuditDecision::Blocked);
            return SafetyDecision {
                score,
                allowed: false,
                needs_confirmation: false,
                reason: "Action blocked: critical risk level".into(),
            };
        }

        // 3) Path sandboxing for file operations
        if matches!(action, "write_file" | "edit_file" | "delete_file" | "read_file") {
            if let Some(path) = target {
                if !self.is_path_allowed(path) {
                    let file_score = RiskScore::new(0.9, RiskLevel::Critical);
                    self.audit(action, target, &file_score, AuditDecision::Blocked);
                    return SafetyDecision {
                        score: file_score,
                        allowed: false,
                        needs_confirmation: false,
                        reason: "Path outside sandbox boundaries".into(),
                    };
                }
            }
        }

        // 4) Rate limiting
        if !self.check_rate_limit(action) {
            self.audit(action, target, &score, AuditDecision::RateLimited);
            return SafetyDecision {
                score,
                allowed: false,
                needs_confirmation: false,
                reason: "Rate limit exceeded".into(),
            };
        }

        // 5) Medium/High needs confirmation
        let needs_confirm =
            self.config.require_confirmation && score.value > self.config.auto_approve_threshold;

        SafetyDecision {
            score,
            allowed: true,
            needs_confirmation: needs_confirm,
            reason: String::new(),
        }
    }

    /// Create a confirmation request (for the UI / CLI to prompt the user).
    pub fn create_request(&self, action: &str, description: &str) -> ConfirmationRequest {
        let score = self.score_command(action, None);
        ConfirmationRequest {
            action: action.to_string(),
            description: description.to_string(),
            risk_level: score.level,
            risk_score: score.value,
            reason: format!(
                "Risk score: {:.2} ({})",
                score.value,
                score.level.as_str()
            ),
            timestamp: Utc::now(),
        }
    }

    // -----------------------------------------------------------------------
    // Audit Logging
    // -----------------------------------------------------------------------

    fn audit(
        &self,
        action: &str,
        target: Option<&str>,
        score: &RiskScore,
        decision: AuditDecision,
    ) {
        if !self.config.audit_logging {
            return;
        }

        let entry = AuditEntry {
            timestamp: Utc::now(),
            action: action.to_string(),
            target: target.map(|s| s.to_string()),
            risk_score: score.clone(),
            decision,
            reason: String::new(),
        };

        let mut log = self.audit_log.lock().unwrap();
        if log.len() >= self.config.audit_capacity {
            log.remove(0); // ring buffer
        }
        log.push(entry);

        debug!(
            action = action,
            target = target.unwrap_or(""),
            score = score.value,
            level = score.level.as_str(),
            decision = ?decision,
            "Safety audit"
        );
    }

    /// Record that a user confirmed or denied an action.
    pub fn record_confirmation(
        &self,
        action: &str,
        target: Option<&str>,
        score: &RiskScore,
        approved: bool,
    ) {
        let decision = if approved {
            AuditDecision::Approved
        } else {
            AuditDecision::DeniedByConfirmation
        };
        self.audit(action, target, score, decision);

        if approved {
            info!(action = action, "User approved action");
        } else {
            warn!(action = action, "User denied action");
        }
    }

    /// Get recent audit entries (newest first).
    pub fn audit_log(&self) -> Vec<AuditEntry> {
        let log = self.audit_log.lock().unwrap();
        let mut entries: Vec<AuditEntry> = log.clone();
        entries.reverse();
        entries
    }

    /// Clear audit log.
    pub fn clear_audit_log(&self) {
        self.audit_log.lock().unwrap().clear();
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn config(&self) -> &SafetyConfig {
        &self.config
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// ---------------------------------------------------------------------------
// Safety Decision (result of evaluate())
// ---------------------------------------------------------------------------

/// Result of evaluating an action through the safety system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyDecision {
    pub score: RiskScore,
    pub allowed: bool,
    pub needs_confirmation: bool,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn safety() -> Safety {
        Safety::new()
    }

    // --- RiskLevel tests ---

    #[test]
    fn test_risk_level_from_score() {
        assert_eq!(RiskLevel::from_score(0.0), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(0.1), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(0.3), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(0.5), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(0.6), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(0.79), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(0.8), RiskLevel::Critical);
        assert_eq!(RiskLevel::from_score(1.0), RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_requires_confirmation() {
        assert!(!RiskLevel::Low.requires_confirmation());
        assert!(RiskLevel::Medium.requires_confirmation());
        assert!(RiskLevel::High.requires_confirmation());
        assert!(RiskLevel::Critical.requires_confirmation());
    }

    #[test]
    fn test_risk_level_as_str() {
        assert_eq!(RiskLevel::Low.as_str(), "low");
        assert_eq!(RiskLevel::Medium.as_str(), "medium");
        assert_eq!(RiskLevel::High.as_str(), "high");
        assert_eq!(RiskLevel::Critical.as_str(), "critical");
    }

    // --- RiskScore tests ---

    #[test]
    fn test_risk_score_clamped() {
        let score = RiskScore::new(-1.0, RiskLevel::Low);
        assert_eq!(score.value, 0.0);

        let score = RiskScore::new(2.0, RiskLevel::Critical);
        assert_eq!(score.value, 1.0);
    }

    // --- Blocklist tests ---

    #[test]
    fn test_blocked_rm_rf_root() {
        let s = safety();
        let score = s.score_command("run_command", Some("rm -rf /"));
        assert_eq!(score.level, RiskLevel::Critical);
        assert!(!s.evaluate("run_command", Some("rm -rf /")).allowed);
    }

    #[test]
    fn test_blocked_mkfs() {
        let s = safety();
        let score = s.score_command("run_command", Some("mkfs.ext4 /dev/sda1"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    #[test]
    fn test_blocked_dd() {
        let s = safety();
        let score = s.score_command("run_command", Some("dd if=/dev/zero of=/dev/sda"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    #[test]
    fn test_blocked_format() {
        let s = safety();
        let score = s.score_command("run_command", Some("format: C:"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    // --- Pattern-based scoring ---

    #[test]
    fn test_sudo_medium() {
        let s = safety();
        let score = s.score_command("run_command", Some("sudo apt install foo"));
        assert_eq!(score.level, RiskLevel::Medium);
    }

    #[test]
    fn test_curl_medium() {
        let s = safety();
        let score = s.score_command("run_command", Some("curl https://example.com"));
        assert_eq!(score.level, RiskLevel::Medium);
    }

    #[test]
    fn test_ls_low() {
        let s = safety();
        let score = s.score_command("run_command", Some("ls -la"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_git_log_low() {
        let s = safety();
        let score = s.score_command("run_command", Some("git log --oneline"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_git_reset_hard() {
        let s = safety();
        let score = s.score_command("run_command", Some("git reset --hard HEAD~1"));
        assert!(score.value >= 0.6);
    }

    #[test]
    fn test_rm_single_file() {
        let s = safety();
        let score = s.score_command("run_command", Some("rm file.txt"));
        assert!(score.value >= 0.3);
    }

    #[test]
    fn test_write_file_action() {
        let s = safety();
        let score = s.score_command("write_file", None);
        assert!(score.value >= 0.2);
    }

    #[test]
    fn test_delete_file_action() {
        let s = safety();
        let score = s.score_command("delete_file", None);
        assert!(score.value >= 0.3);
    }

    // --- Allowed commands allowlist ---

    #[test]
    fn test_allowed_commands_block_unknown() {
        let s = safety()
            .with_allowed_commands(vec!["ls".into(), "cat".into(), "git".into()]);

        let score = s.score_command("run_command", Some("ls -la"));
        assert_eq!(score.level, RiskLevel::Low);

        let score = s.score_command("run_command", Some("rm file.txt"));
        assert_eq!(score.level, RiskLevel::High);
    }

    #[test]
    fn test_allowed_commands_empty_allows_all() {
        let s = safety(); // default: empty allowed_commands
        let score = s.score_command("run_command", Some("npm install"));
        assert_ne!(score.level, RiskLevel::High); // not blocked by allowlist
    }

    // --- Path sandboxing ---

    #[test]
    fn test_sandbox_allows_within() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(s.is_path_allowed("/home/user/project/src/main.rs"));
    }

    #[test]
    fn test_sandbox_blocks_outside() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(!s.is_path_allowed("/etc/passwd"));
    }

    #[test]
    fn test_sandbox_blocks_traversal() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(!s.is_path_allowed("/home/user/project/../../../etc/passwd"));
    }

    #[test]
    fn test_no_sandbox_allows_all() {
        let s = safety(); // no sandbox configured
        assert!(s.is_path_allowed("/etc/passwd"));
        assert!(s.is_path_allowed("/any/random/path"));
    }

    #[test]
    fn test_sensitive_system_paths() {
        let s = safety();
        let score = s.score_file_operation("/etc/shadow", "write_file");
        assert!(score.value >= 0.3); // extra risk for sensitive path
    }

    // --- Rate limiting ---

    #[test]
    fn test_rate_limit_allows_within() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "run_command".into(),
            RateLimit {
                max_calls: 3,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
    }

    #[test]
    fn test_rate_limit_blocks_over() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "run_command".into(),
            RateLimit {
                max_calls: 2,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
        assert!(!s.check_rate_limit("run_command")); // blocked
    }

    #[test]
    fn test_rate_limit_per_tool_isolation() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "tool_a".into(),
            RateLimit {
                max_calls: 1,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("tool_a"));
        assert!(!s.check_rate_limit("tool_a")); // tool_a limited
        assert!(s.check_rate_limit("tool_b"));   // tool_b unaffected
    }

    #[test]
    fn test_rate_limit_no_config_allows_all() {
        let s = safety();
        for _ in 0..100 {
            assert!(s.check_rate_limit("any_tool"));
        }
    }

    // --- Evaluate (full pipeline) ---

    #[test]
    fn test_evaluate_blocks_critical() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(!decision.allowed);
        assert!(!decision.needs_confirmation); // blocked outright
    }

    #[test]
    fn test_evaluate_allows_low() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("ls -la"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_medium_needs_confirmation() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_disabled_allows_all() {
        let mut config = SafetyConfig::default();
        config.enabled = false;
        let s = Safety::with_config(config);

        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_no_confirmation_required() {
        let mut config = SafetyConfig::default();
        config.require_confirmation = false;
        let s = Safety::with_config(config);

        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation); // no confirmation needed
    }

    #[test]
    fn test_evaluate_sandbox_blocks_file_op() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        let decision = s.evaluate("write_file", Some("/etc/passwd"));
        assert!(!decision.allowed);
    }

    // --- Audit logging ---

    #[test]
    fn test_audit_log_records() {
        let s = safety();
        s.evaluate("run_command", Some("rm -rf /")); // blocked
        let log = s.audit_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].decision, AuditDecision::Blocked);
    }

    #[test]
    fn test_audit_log_clear() {
        let s = safety();
        s.evaluate("run_command", Some("rm -rf /"));
        assert!(!s.audit_log().is_empty());
        s.clear_audit_log();
        assert!(s.audit_log().is_empty());
    }

    #[test]
    fn test_audit_log_ring_buffer() {
        let mut config = SafetyConfig::default();
        config.audit_capacity = 3;
        let s = Safety::with_config(config);

        for i in 0..5 {
            s.evaluate("run_command", Some(&format!("test_cmd_{}", i)));
        }
        // Only last 3 entries kept
        let log = s.audit_log();
        assert!(log.len() <= 3);
    }

    // --- Needs confirmation legacy ---

    #[test]
    fn test_needs_confirmation_low() {
        let s = safety();
        assert!(!s.needs_confirmation("run_command", Some("ls -la")));
    }

    #[test]
    fn test_needs_confirmation_medium() {
        let s = safety();
        assert!(s.needs_confirmation("run_command", Some("sudo apt install foo")));
    }

    // --- Confirmation request ---

    #[test]
    fn test_create_request() {
        let s = safety();
        let req = s.create_request("run_command", "sudo rm file");
        assert_eq!(req.action, "run_command");
        assert_eq!(req.description, "sudo rm file");
        assert!(req.risk_score >= 0.0); // score is always valid
        assert!(!req.timestamp.to_rfc3339().is_empty());
    }

    // --- Configurable thresholds ---

    #[test]
    fn test_custom_thresholds() {
        let mut config = SafetyConfig::default();
        config.medium_threshold = 0.5; // stricter medium
        config.high_threshold = 0.7;
        config.critical_threshold = 0.9;
        let s = Safety::with_config(config);

        // score 0.35 → Low (because medium threshold is 0.5)
        let score = s.score_command("run_command", Some("curl https://example.com"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_command() {
        let s = safety();
        let score = s.score_command("run_command", Some(""));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_none_target() {
        let s = safety();
        let score = s.score_command("run_command", None);
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_unicode_command() {
        let s = safety();
        let score = s.score_command("run_command", Some("echo 'こんにちは'"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_very_long_command() {
        let s = safety();
        let long_cmd = "echo ".to_string() + &"a".repeat(10_000);
        let score = s.score_command("run_command", Some(&long_cmd));
        // Should not panic, should be low risk
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_case_insensitive_blocklist() {
        let s = safety();
        let score = s.score_command("run_command", Some("RM -RF /"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    // --- Permission modes ---

    #[test]
    fn test_permission_mode_default_is_default() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn test_permission_mode_parse() {
        assert_eq!(PermissionMode::parse("default"), Some(PermissionMode::Default));
        assert_eq!(PermissionMode::parse("PLAN"), Some(PermissionMode::Plan));
        assert_eq!(PermissionMode::parse("yolo"), Some(PermissionMode::Yolo));
        assert_eq!(PermissionMode::parse("readonly"), Some(PermissionMode::Plan));
        assert_eq!(PermissionMode::parse("trust"), Some(PermissionMode::Yolo));
        assert_eq!(PermissionMode::parse("bogus"), None);
    }

    #[test]
    fn test_plan_mode_blocks_writes() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        assert!(!s.evaluate("write_file", Some("foo.txt")).allowed);
        assert!(!s.evaluate("edit_file", Some("foo.txt")).allowed);
        assert!(!s.evaluate("run_command", Some("ls")).allowed);
    }

    #[test]
    fn test_plan_mode_allows_reads() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        assert!(s.evaluate("read_file", Some("foo.txt")).allowed);
        assert!(s.evaluate("list_files", Some(".")).allowed);
        assert!(s.evaluate("search_files", Some("pattern")).allowed);
    }

    #[test]
    fn test_yolo_mode_auto_approves() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        // sudo would normally be Medium and require confirmation.
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_yolo_still_blocks_critical_blocklist() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(!decision.allowed); // safety net stays
    }

    #[test]
    fn test_default_mode_unchanged_behavior() {
        let s = safety();
        // Default mode preserves the original gating behavior.
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(decision.needs_confirmation);
    }

    #[test]
    fn test_mode_setter_and_getter() {
        let s = safety();
        assert_eq!(s.mode(), PermissionMode::Default);
        s.set_mode(PermissionMode::Yolo);
        assert_eq!(s.mode(), PermissionMode::Yolo);
    }

    #[test]
    fn test_needs_confirmation_yolo() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        assert!(!s.needs_confirmation("run_command", Some("sudo apt install foo")));
    }

    #[test]
    fn test_needs_confirmation_plan_writes() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        // Plan mode denies outright — no confirmation needed (or possible).
        assert!(!s.needs_confirmation("write_file", Some("foo.txt")));
    }
}
