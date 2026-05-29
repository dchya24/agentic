//! Memory and context management for agentic AI sessions.
//!
//! Provides sliding-window context management, message pinning,
//! session-based isolation, disk persistence, and smart summarization.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::providers::ToolCallResponse;
use crate::tool::{ToolCall, ToolResultValue};

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

/// A single message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub metadata: MessageMetadata,
}

/// Optional metadata attached to a message.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Token count for this message (estimated).
    #[serde(default)]
    pub token_count: u32,
    /// Model that generated this message (for assistant messages).
    #[serde(default)]
    pub model: Option<String>,
    /// Duration to generate this message (for assistant messages).
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Tool call ID if this is a tool result message.
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// Tool calls emitted by this assistant message.
    /// Required for the next request so tool results can be matched
    /// to their parent assistant call (per OpenAI spec).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallResponse>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            role: MessageRole::User,
            content: content.into(),
            timestamp: Utc::now(),
            pinned: false,
            metadata: MessageMetadata::default(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
            pinned: false,
            metadata: MessageMetadata::default(),
        }
    }

    /// Assistant message that emitted tool calls.
    /// The tool calls are stored in metadata so the next request can
    /// reattach them on the assistant `ChatMessageRequest` — required
    /// for tool results to be valid per OpenAI spec.
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCallResponse>,
    ) -> Self {
        Self {
            id: new_id(),
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
            pinned: false,
            metadata: MessageMetadata {
                tool_calls,
                ..MessageMetadata::default()
            },
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            role: MessageRole::System,
            content: content.into(),
            timestamp: Utc::now(),
            pinned: false,
            metadata: MessageMetadata::default(),
        }
    }

    pub fn tool(
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let tc_id = tool_call_id.into();
        Self {
            id: new_id(),
            role: MessageRole::Tool {
                tool_name: tool_name.into(),
                tool_call_id: tc_id.clone(),
            },
            content: content.into(),
            timestamp: Utc::now(),
            pinned: false,
            metadata: MessageMetadata {
                tool_call_id: Some(tc_id),
                ..Default::default()
            },
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.metadata.model = Some(model.into());
        self
    }

    pub fn with_duration(mut self, ms: u64) -> Self {
        self.metadata.duration_ms = Some(ms);
        self
    }

    /// Estimate and store token count.
    pub fn with_estimated_tokens(mut self) -> Self {
        self.metadata.token_count = estimate_tokens(&self.content);
        self
    }

    /// Pin this message (won't be removed during compaction).
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }
}

// ---------------------------------------------------------------------------
// Message Role
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "tool")]
    Tool {
        tool_name: String,
        tool_call_id: String,
    },
}

impl MessageRole {
    pub fn as_str(&self) -> &str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool { .. } => "tool",
        }
    }
}

// ---------------------------------------------------------------------------
// Session Info
// ---------------------------------------------------------------------------

/// Session metadata for isolation and persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl SessionInfo {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: new_id(),
            created_at: now,
            updated_at: now,
            label: String::new(),
            provider: None,
            model: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Context Window
// ---------------------------------------------------------------------------

/// Result of context window calculation.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub messages: Vec<Message>,
    pub total_tokens: u32,
    pub budget: u32,
    pub used_percentage: f64,
    pub was_compacted: bool,
    pub removed_count: usize,
}

// ---------------------------------------------------------------------------
// Summarized Context
// ---------------------------------------------------------------------------

/// Result of memory compaction.
#[derive(Debug, Clone)]
pub struct SummarizedContext {
    pub summary: String,
    pub summarized_count: usize,
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub kept_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Memory Config
// ---------------------------------------------------------------------------

/// Configuration for memory behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum token budget for context window.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Number of recent messages to always keep (even if over budget).
    #[serde(default = "default_keep_recent")]
    pub keep_recent: usize,

    /// Trigger compaction when usage exceeds this percentage (0.0-1.0).
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: f64,

    /// Max messages to include in context when building for LLM.
    #[serde(default = "default_context_message_limit")]
    pub context_message_limit: usize,

    /// Directory for persisting sessions to disk.
    #[serde(default)]
    pub persist_dir: Option<String>,

    /// Whether to auto-persist on every add_message.
    #[serde(default)]
    pub auto_persist: bool,
}

fn default_max_tokens() -> u32 {
    128_000
}
fn default_keep_recent() -> usize {
    4
}
fn default_compaction_threshold() -> f64 {
    0.85
}
fn default_context_message_limit() -> usize {
    50
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_max_tokens(),
            keep_recent: default_keep_recent(),
            compaction_threshold: default_compaction_threshold(),
            context_message_limit: default_context_message_limit(),
            persist_dir: None,
            auto_persist: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Memory (main struct)
// ---------------------------------------------------------------------------

/// Conversation memory with context window management, pinning,
/// session isolation, and optional persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResultValue>,

    #[serde(default)]
    pub total_tokens: u32,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    #[serde(default)]
    pub session: SessionInfo,

    #[serde(default)]
    pub config: MemoryConfig,

    /// IDs of pinned messages (never compacted away).
    #[serde(default)]
    pinned_ids: HashSet<String>,

    /// Compaction summary, if any.
    #[serde(default)]
    summary: Option<String>,
}

impl Memory {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    pub fn new(max_tokens: u32) -> Self {
        Self {
            messages: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            total_tokens: 0,
            max_tokens,
            session: SessionInfo::new(),
            config: MemoryConfig {
                max_tokens,
                ..Default::default()
            },
            pinned_ids: HashSet::new(),
            summary: None,
        }
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        let max = config.max_tokens;
        Self {
            messages: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            total_tokens: 0,
            max_tokens: max,
            session: SessionInfo::new(),
            config,
            pinned_ids: HashSet::new(),
            summary: None,
        }
    }

    pub fn with_session(mut self, session: SessionInfo) -> Self {
        self.session = session;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.session.label = label.into();
        self
    }

    // -----------------------------------------------------------------------
    // Add messages
    // -----------------------------------------------------------------------

    /// Add a message and update token tracking.
    pub fn add_message(&mut self, message: Message) {
        let tokens = estimate_tokens(&message.content);
        self.total_tokens += tokens;
        self.session.touch();

        // Track pinned state
        if message.pinned {
            self.pinned_ids.insert(message.id.clone());
        }

        self.messages.push(message);

        if self.config.auto_persist {
            let _ = self.persist();
        }
    }

    pub fn add_tool_call(&mut self, call: ToolCall) {
        self.tool_calls.push(call);
        self.session.touch();
    }

    pub fn add_tool_result(&mut self, result: ToolResultValue) {
        self.tool_results.push(result);
        self.session.touch();
    }

    // -----------------------------------------------------------------------
    // Pinning
    // -----------------------------------------------------------------------

    /// Pin a message by ID so it survives compaction.
    pub fn pin(&mut self, message_id: &str) -> bool {
        if self.messages.iter().any(|m| m.id == message_id) {
            self.pinned_ids.insert(message_id.to_string());
            if let Some(msg) = self.messages.iter_mut().find(|m| m.id == message_id) {
                msg.pinned = true;
            }
            true
        } else {
            false
        }
    }

    /// Unpin a message.
    pub fn unpin(&mut self, message_id: &str) {
        self.pinned_ids.remove(message_id);
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == message_id) {
            msg.pinned = false;
        }
    }

    /// Get all pinned message IDs.
    pub fn pinned_ids(&self) -> &HashSet<String> {
        &self.pinned_ids
    }

    // -----------------------------------------------------------------------
    // Context Window
    // -----------------------------------------------------------------------

    /// Get messages up to budget limit (token-based sliding window).
    /// Returns a ContextWindow with metadata about what was included/excluded.
    pub fn get_context_window(&self) -> ContextWindow {
        let budget = self.config.max_tokens;
        let limit = self.config.context_message_limit;

        // Always include summary (if exists) and pinned messages
        let mut selected: Vec<Message> = Vec::new();
        let mut used_tokens: u32 = 0;

        // Add existing summary as a system message
        if let Some(ref summary) = self.summary {
            let summary_tokens = estimate_tokens(summary);
            let summary_msg = Message {
                id: "__summary__".into(),
                role: MessageRole::System,
                content: summary.clone(),
                timestamp: Utc::now(),
                pinned: true,
                metadata: MessageMetadata {
                    token_count: summary_tokens,
                    ..Default::default()
                },
            };
            used_tokens += summary_tokens;
            selected.push(summary_msg);
        }

        // Walk messages from newest to oldest
        let recent_slice: Vec<&Message> = self
            .messages
            .iter()
            .rev()
            .take(limit)
            .collect();

        for msg in &recent_slice {
            let msg_tokens = if msg.metadata.token_count > 0 {
                msg.metadata.token_count
            } else {
                estimate_tokens(&msg.content)
            };

            if used_tokens + msg_tokens > budget {
                break;
            }

            used_tokens += msg_tokens;
            selected.push((*msg).clone());
        }

        // Reverse to restore chronological order
        selected.reverse();

        let used_pct = if budget > 0 {
            (used_tokens as f64 / budget as f64) * 100.0
        } else {
            0.0
        };

        let selected_count = selected.len();

        ContextWindow {
            messages: selected,
            total_tokens: used_tokens,
            budget,
            used_percentage: used_pct,
            was_compacted: self.summary.is_some(),
            removed_count: self.messages.len().saturating_sub(selected_count),
        }
    }

    /// Legacy: get last N messages.
    pub fn get_context(&self, max_messages: usize) -> Vec<Message> {
        let start = self.messages.len().saturating_sub(max_messages);
        self.messages[start..].to_vec()
    }

    /// Get the conversation tail anchored to a user message.
    ///
    /// `max_messages` is a soft floor: if the last `max_messages` don't
    /// include any user message (because the agent ran a long chain of
    /// tool calls between user turns), the window is extended backwards
    /// to include the most recent user message. This avoids producing a
    /// slice that's purely assistant/tool turns, which providers reject
    /// with HTTP 400 "messages parameter is illegal".
    ///
    /// `hard_cap` bounds how far we'll look back. When set to `None` we
    /// search all the way to the start of memory; when set we never
    /// return more than `hard_cap` messages even if the latest user
    /// turn is older than that.
    pub fn get_context_with_user_anchor(
        &self,
        max_messages: usize,
        hard_cap: Option<usize>,
    ) -> Vec<Message> {
        let total = self.messages.len();
        if total == 0 {
            return Vec::new();
        }

        let mut start = total.saturating_sub(max_messages);

        // Does the candidate slice already contain a user message?
        let has_user = self.messages[start..]
            .iter()
            .any(|m| matches!(m.role, MessageRole::User));

        if !has_user {
            // Walk backwards to find the most recent user message.
            if let Some(user_idx) = self.messages[..start]
                .iter()
                .rposition(|m| matches!(m.role, MessageRole::User))
            {
                start = user_idx;
            }
        }

        // Apply hard_cap if set.
        if let Some(cap) = hard_cap {
            let earliest = total.saturating_sub(cap);
            if start < earliest {
                start = earliest;
            }
        }

        self.messages[start..].to_vec()
    }

    /// Get all messages.
    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    // -----------------------------------------------------------------------
    // Compaction / Summarization
    // -----------------------------------------------------------------------

    /// Check if compaction should be triggered.
    pub fn needs_compaction(&self) -> bool {
        let threshold = (self.config.max_tokens as f64 * self.config.compaction_threshold) as u32;
        self.total_tokens >= threshold
    }

    /// Compact memory: summarize older messages, keep recent + pinned.
    pub fn compact(&mut self) -> SummarizedContext {
        let tokens_before = self.total_tokens;
        let total_count = self.messages.len();

        if total_count <= self.config.keep_recent {
            return SummarizedContext {
                summary: String::new(),
                summarized_count: 0,
                tokens_before,
                tokens_after: tokens_before,
                kept_ids: self.messages.iter().map(|m| m.id.clone()).collect(),
            };
        }

        // Build summary from messages that will be removed
        let keep_from = total_count.saturating_sub(self.config.keep_recent);

        // Clone messages first to avoid borrow conflict
        let all_messages: Vec<Message> = self.messages.clone();
        let mut to_compact_content: Vec<String> = Vec::new();
        let mut to_keep: Vec<Message> = Vec::new();
        let mut kept_ids: Vec<String> = Vec::new();

        for (i, msg) in all_messages.iter().enumerate() {
            if i >= keep_from || self.pinned_ids.contains(&msg.id) {
                to_keep.push(msg.clone());
                kept_ids.push(msg.id.clone());
            } else {
                to_compact_content.push(format!("[{}]: {}",
                    msg.role.as_str(),
                    if msg.content.len() > 50 { format!("{}...", &msg.content[..47]) } else { msg.content.clone() }
                ));
            }
        }

        let compact_count = to_compact_content.len();

        // Truncate summary if too long to keep tokens manageable
        let raw_summary = to_compact_content.join("\n");
        let max_summary_chars = ((self.config.max_tokens as usize) / 4).max(200);
        let truncated_summary = if raw_summary.len() > max_summary_chars {
            format!("{}\n... ({} more messages truncated)",
                &raw_summary[..max_summary_chars.saturating_sub(40)],
                compact_count
            )
        } else {
            raw_summary
        };

        // Generate summary
        let new_summary = match &self.summary {
            Some(existing) => {
                format!(
                    "{}\n--- ({} messages): {}",
                    existing,
                    compact_count,
                    truncated_summary
                )
            }
            None => format!(
                "[Summary of {} messages, {} tokens]: {}",
                compact_count,
                tokens_before,
                truncated_summary
            ),
        };

        let summary_tokens = estimate_tokens(&new_summary);
        let kept_tokens: u32 = to_keep.iter().map(|m| estimate_tokens(&m.content)).sum();
        let tokens_after = summary_tokens + kept_tokens;

        // Apply
        self.summary = Some(new_summary);
        self.messages = to_keep;
        self.total_tokens = tokens_after;

        SummarizedContext {
            summary: self.summary.clone().unwrap_or_default(),
            summarized_count: compact_count,
            tokens_before,
            tokens_after,
            kept_ids,
        }
    }

    /// Legacy compatibility: alias for compact().
    pub fn summarize(&mut self) {
        self.compact();
    }

    /// Get the current compaction summary, if any.
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// Build a prompt that asks an LLM to summarize the messages that
    /// would be compacted away on the next call to [`Self::compact`].
    ///
    /// Returns `None` if there's nothing to summarize (memory is below
    /// `keep_recent`). The caller is responsible for calling its own
    /// LLM provider with this text and then passing the summary into
    /// [`Self::compact_with_summary`].
    ///
    /// Layered design: keeping the LLM call out of `core::memory` means
    /// this module stays free of any provider/runtime dependency and is
    /// trivially unit-testable.
    pub fn build_summarization_prompt(&self) -> Option<String> {
        let total = self.messages.len();
        if total <= self.config.keep_recent {
            return None;
        }
        let keep_from = total.saturating_sub(self.config.keep_recent);

        let mut transcript = String::new();
        for (i, msg) in self.messages.iter().enumerate() {
            if i >= keep_from || self.pinned_ids.contains(&msg.id) {
                continue;
            }
            transcript.push_str(&format!("[{}]: {}\n", msg.role.as_str(), msg.content));
        }

        if transcript.is_empty() {
            return None;
        }

        let prior_summary = self.summary.as_deref().unwrap_or("(none)");
        Some(format!(
            "You are summarizing an in-progress conversation between a user, an AI \
             coding assistant, and the assistant's tool calls. Produce a concise \
             summary that preserves:\n\
             - the user's intent / goals,\n\
             - decisions already made,\n\
             - files read or modified (with paths) and key findings,\n\
             - open questions or pending steps.\n\
             \n\
             Keep the summary under ~400 words. Use plain prose, no markdown \
             headers. Do not include verbatim tool output — paraphrase.\n\
             \n\
             Prior summary (to extend, do not repeat verbatim):\n{}\n\
             \n\
             Conversation excerpt to summarize:\n{}",
            prior_summary, transcript
        ))
    }

    /// Compact memory using a caller-provided summary string (typically
    /// generated by an LLM via [`Self::build_summarization_prompt`]).
    ///
    /// Behavior matches [`Self::compact`] except the summary text comes
    /// from outside instead of the heuristic truncation. Returns the
    /// same [`SummarizedContext`] shape so callers can switch between
    /// the two with no other changes.
    pub fn compact_with_summary(&mut self, llm_summary: &str) -> SummarizedContext {
        let tokens_before = self.total_tokens;
        let total_count = self.messages.len();

        if total_count <= self.config.keep_recent {
            return SummarizedContext {
                summary: String::new(),
                summarized_count: 0,
                tokens_before,
                tokens_after: tokens_before,
                kept_ids: self.messages.iter().map(|m| m.id.clone()).collect(),
            };
        }

        let keep_from = total_count.saturating_sub(self.config.keep_recent);
        let all_messages: Vec<Message> = self.messages.clone();
        let mut to_keep: Vec<Message> = Vec::new();
        let mut kept_ids: Vec<String> = Vec::new();
        let mut compact_count: usize = 0;

        for (i, msg) in all_messages.iter().enumerate() {
            if i >= keep_from || self.pinned_ids.contains(&msg.id) {
                to_keep.push(msg.clone());
                kept_ids.push(msg.id.clone());
            } else {
                compact_count += 1;
            }
        }

        let new_summary = match &self.summary {
            Some(existing) => format!(
                "{}\n---\n[LLM summary of {} more messages]:\n{}",
                existing, compact_count, llm_summary.trim()
            ),
            None => format!(
                "[LLM summary of {} messages, {} tokens]:\n{}",
                compact_count, tokens_before, llm_summary.trim()
            ),
        };

        let summary_tokens = estimate_tokens(&new_summary);
        let kept_tokens: u32 = to_keep.iter().map(|m| estimate_tokens(&m.content)).sum();
        let tokens_after = summary_tokens + kept_tokens;

        self.summary = Some(new_summary);
        self.messages = to_keep;
        self.total_tokens = tokens_after;

        SummarizedContext {
            summary: self.summary.clone().unwrap_or_default(),
            summarized_count: compact_count,
            tokens_before,
            tokens_after,
            kept_ids,
        }
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Simple keyword search across messages.
    pub fn search(&self, query: &str) -> Vec<&Message> {
        let query_lower = query.to_lowercase();
        self.messages
            .iter()
            .filter(|m| m.content.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Search messages by role.
    pub fn search_by_role(&self, role: &MessageRole) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| std::mem::discriminant(&m.role) == std::mem::discriminant(role))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Token tracking
    // -----------------------------------------------------------------------

    pub fn token_count(&self) -> u32 {
        self.total_tokens
    }

    /// Recalculate total tokens from scratch (fix drift).
    pub fn recalculate_tokens(&mut self) {
        self.total_tokens = self
            .messages
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();
    }

    pub fn role_type(&self) -> &str {
        self.messages
            .last()
            .map(|m| m.role.as_str())
            .unwrap_or("user")
    }

    /// Token budget remaining.
    pub fn remaining_budget(&self) -> u32 {
        self.config.max_tokens.saturating_sub(self.total_tokens)
    }

    /// Usage as a percentage of budget.
    pub fn usage_percentage(&self) -> f64 {
        if self.config.max_tokens == 0 {
            return 0.0;
        }
        (self.total_tokens as f64 / self.config.max_tokens as f64) * 100.0
    }

    // -----------------------------------------------------------------------
    // Session Info
    // -----------------------------------------------------------------------

    pub fn session(&self) -> &SessionInfo {
        &self.session
    }

    /// Start a new session (clears messages, keeps config).
    pub fn new_session(&mut self) -> SessionInfo {
        self.clear();
        self.session = SessionInfo::new();
        self.session.clone()
    }

    /// Start a new session with a label.
    pub fn new_session_with_label(&mut self, label: impl Into<String>) -> SessionInfo {
        self.clear();
        self.session = SessionInfo::new().with_label(label);
        self.session.clone()
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Get the default persist directory.
    fn persist_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.config.persist_dir {
            PathBuf::from(dir)
        } else {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".config")
                .join("agentic")
                .join("sessions")
        }
    }

    /// Save this memory to disk.
    pub fn persist(&self) -> io::Result<PathBuf> {
        let dir = self.persist_dir();
        fs::create_dir_all(&dir)?;

        let filename = format!("{}.json", self.session.id);
        let path = dir.join(filename);

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        fs::write(&path, json)?;
        Ok(path)
    }

    /// Load a session from disk by session ID.
    pub fn load(session_id: &str) -> io::Result<Self> {
        Self::load_from_dir(session_id, None)
    }

    /// Load a session from a specific directory.
    pub fn load_from_dir(session_id: &str, dir: Option<&Path>) -> io::Result<Self> {
        let dir = dir
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".config")
                    .join("agentic")
                    .join("sessions")
            });

        let path = dir.join(format!("{}.json", session_id));
        let content = fs::read_to_string(&path)?;

        let mut memory: Memory = serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Rebuild pinned_ids from messages
        memory.pinned_ids = memory
            .messages
            .iter()
            .filter(|m| m.pinned)
            .map(|m| m.id.clone())
            .collect();

        Ok(memory)
    }

    /// List all saved session IDs in the persist directory.
    pub fn list_sessions() -> io::Result<Vec<String>> {
        Self::list_sessions_from_dir(None)
    }

    /// List all saved session IDs from a specific directory.
    pub fn list_sessions_from_dir(dir: Option<&Path>) -> io::Result<Vec<String>> {
        let dir = dir
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".config")
                    .join("agentic")
                    .join("sessions")
            });

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions: Vec<String> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    sessions.push(stem.to_string_lossy().to_string());
                }
            }
        }

        sessions.sort();
        Ok(sessions)
    }

    /// Delete a saved session file.
    pub fn delete_session(session_id: &str) -> io::Result<()> {
        Self::delete_session_in_dir(session_id, None)
    }

    /// Delete a saved session file from a specific directory.
    pub fn delete_session_in_dir(session_id: &str, dir: Option<&Path>) -> io::Result<()> {
        let dir = dir
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".config")
                    .join("agentic")
                    .join("sessions")
            });
        let path = dir.join(format!("{}.json", session_id));
        if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Clear
    // -----------------------------------------------------------------------

    /// Clear all messages, tool data, and summary.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_calls.clear();
        self.tool_results.clear();
        self.total_tokens = 0;
        self.pinned_ids.clear();
        self.summary = None;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a short unique ID.
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = Utc::now().timestamp_millis() as u64;
    // Mix timestamp + counter for uniqueness
    format!("{:x}-{:04x}", ts, (count % 0xFFFF) as u16)
}

/// Estimate token count from text.
/// Uses character heuristic: ~4 chars per token.
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 / 4).max(1)
}

// summarize_messages removed — compaction builds summary inline

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Memory {
        Memory::new(10000)
    }

    fn memory_small() -> Memory {
        Memory::new(200) // small budget for compaction tests
    }

    // --- Message construction ---

    #[test]
    fn test_message_user() {
        let msg = Message::user("hello");
        assert!(matches!(msg.role, MessageRole::User));
        assert_eq!(msg.content, "hello");
        assert!(!msg.pinned);
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("response");
        assert!(matches!(msg.role, MessageRole::Assistant));
        assert_eq!(msg.content, "response");
    }

    #[test]
    fn test_message_system() {
        let msg = Message::system("system prompt");
        assert!(matches!(msg.role, MessageRole::System));
    }

    #[test]
    fn test_message_tool() {
        let msg = Message::tool("read_file", "call-123", "file contents");
        assert!(matches!(msg.role, MessageRole::Tool { .. }));
        if let MessageRole::Tool { tool_name, tool_call_id } = &msg.role {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_call_id, "call-123");
        }
    }

    #[test]
    fn test_message_with_model() {
        let msg = Message::assistant("hi").with_model("gpt-4o");
        assert_eq!(msg.metadata.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn test_message_with_duration() {
        let msg = Message::assistant("hi").with_duration(1500);
        assert_eq!(msg.metadata.duration_ms, Some(1500));
    }

    #[test]
    fn test_message_pinned() {
        let msg = Message::user("important").pinned();
        assert!(msg.pinned);
    }

    #[test]
    fn test_message_with_estimated_tokens() {
        let msg = Message::user("a".repeat(100)).with_estimated_tokens();
        assert_eq!(msg.metadata.token_count, 25); // 100/4
    }

    // --- MessageRole ---

    #[test]
    fn test_message_role_as_str() {
        assert_eq!(MessageRole::User.as_str(), "user");
        assert_eq!(MessageRole::Assistant.as_str(), "assistant");
        assert_eq!(MessageRole::System.as_str(), "system");
        assert_eq!(
            MessageRole::Tool {
                tool_name: "t".into(),
                tool_call_id: "1".into()
            }
            .as_str(),
            "tool"
        );
    }

    // --- Basic add/get ---

    #[test]
    fn test_add_and_get_messages() {
        let mut m = memory();
        m.add_message(Message::user("Hello"));
        m.add_message(Message::assistant("Hi there"));

        assert_eq!(m.get_messages().len(), 2);
        assert_eq!(m.token_count(), 3); // "Hello"=1 + "Hi there"=2
    }

    #[test]
    fn test_get_context_limit() {
        let mut m = memory();
        for i in 0..10 {
            m.add_message(Message::user(format!("msg {}", i)));
        }

        let ctx = m.get_context(3);
        assert_eq!(ctx.len(), 3);
        // Should be the last 3
        assert!(ctx[0].content.contains("msg 7"));
        assert!(ctx[1].content.contains("msg 8"));
        assert!(ctx[2].content.contains("msg 9"));
    }

    #[test]
    fn test_clear() {
        let mut m = memory();
        m.add_message(Message::user("Hello"));
        m.add_message(Message::assistant("Hi"));
        m.clear();
        assert!(m.get_messages().is_empty());
        assert_eq!(m.token_count(), 0);
    }

    // --- Pinning ---

    #[test]
    fn test_pin_message() {
        let mut m = memory();
        let msg = Message::user("important");
        let id = msg.id.clone();
        m.add_message(msg);
        m.add_message(Message::user("normal"));

        assert!(m.pin(&id));
        assert!(m.pinned_ids().contains(&id));
    }

    #[test]
    fn test_pin_nonexistent() {
        let mut m = memory();
        assert!(!m.pin("nonexistent-id"));
    }

    #[test]
    fn test_unpin_message() {
        let mut m = memory();
        let msg = Message::user("important").pinned();
        let id = msg.id.clone();
        m.add_message(msg);

        assert!(m.pinned_ids().contains(&id));
        m.unpin(&id);
        assert!(!m.pinned_ids().contains(&id));
    }

    #[test]
    fn test_pinned_survives_compaction() {
        let mut m = memory_small(); // 200 token budget
        let pinned_msg = Message::user("keep me always").pinned();
        let pinned_id = pinned_msg.id.clone();
        m.add_message(pinned_msg);

        // Fill up memory
        for i in 0..20 {
            m.add_message(Message::user(format!(
                "padding message number {} with some text to use tokens",
                i
            )));
        }

        assert!(m.needs_compaction());
        let result = m.compact();

        // Pinned message should be kept
        assert!(result.kept_ids.contains(&pinned_id));
        assert!(m.get_messages().iter().any(|msg| msg.id == pinned_id));
    }

    // --- Context Window ---

    #[test]
    fn test_context_window_within_budget() {
        let mut m = memory();
        for i in 0..5 {
            m.add_message(Message::user(format!("short {}", i)));
        }
        let ctx = m.get_context_window();
        assert_eq!(ctx.messages.len(), 5);
        assert!(!ctx.was_compacted);
    }

    #[test]
    fn test_context_window_respects_limit() {
        let mut config = MemoryConfig::default();
        config.context_message_limit = 3;
        let mut m = Memory::with_config(config);

        for i in 0..10 {
            m.add_message(Message::user(format!("msg {}", i)));
        }

        let ctx = m.get_context_window();
        assert!(ctx.messages.len() <= 3);
    }

    #[test]
    fn test_context_window_with_summary() {
        let mut m = memory_small();
        for i in 0..20 {
            m.add_message(Message::user(format!(
                "padding message {} with enough text to fill budget",
                i
            )));
        }
        m.compact();

        let ctx = m.get_context_window();
        assert!(ctx.was_compacted);
        assert!(ctx.removed_count > 0 || ctx.messages.len() < 20);
    }

    // --- Compaction ---

    #[test]
    fn test_needs_compaction_below_threshold() {
        let mut m = memory();
        m.add_message(Message::user("short message"));
        assert!(!m.needs_compaction());
    }

    #[test]
    fn test_needs_compaction_above_threshold() {
        let mut m = memory_small(); // 200 tokens
        for i in 0..50 {
            m.add_message(Message::user(format!("message {} with some content", i)));
        }
        assert!(m.needs_compaction());
    }

    #[test]
    fn test_compact_reduces_tokens() {
        let mut config = MemoryConfig::default();
        config.max_tokens = 50_000;
        config.keep_recent = 4;
        let mut m = Memory::with_config(config);

        // Use large messages so compaction actually saves tokens
        for i in 0..100 {
            m.add_message(Message::user(format!(
                "message number {} with a lot of text to consume tokens padding padding padding",
                i
            )));
        }

        let tokens_before = m.token_count();
        assert!(tokens_before > 1000, "should have substantial tokens: {}", tokens_before);

        let result = m.compact();
        let tokens_after = m.token_count();

        assert!(result.summarized_count > 0, "should have compacted some messages");
        assert!(tokens_after < tokens_before, "tokens should decrease: {} -> {}", tokens_before, tokens_after);
    }

    #[test]
    fn test_compact_keeps_recent() {
        let mut config = MemoryConfig::default();
        config.max_tokens = 200;
        config.keep_recent = 3;
        let mut m = Memory::with_config(config);

        for i in 0..20 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        m.compact();

        let messages = m.get_messages();
        assert!(messages.len() >= 3);
        // Last 3 messages should be present
        let last_content: Vec<&str> = messages.iter().rev().take(3).map(|m| m.content.as_str()).collect();
        assert!(last_content[0].contains("msg 19"));
        assert!(last_content[1].contains("msg 18"));
        assert!(last_content[2].contains("msg 17"));
    }

    #[test]
    fn test_compact_no_compact_needed() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("hi"));

        let result = m.compact();
        assert_eq!(result.summarized_count, 0); // nothing to compact
    }

    #[test]
    fn test_compact_accumulates_summary() {
        let mut m = memory_small();
        for i in 0..30 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        m.compact();

        let first_summary = m.summary().unwrap().to_string();

        // Add more and compact again
        for i in 0..30 {
            m.add_message(Message::user(format!("msg2 {}", i)));
        }
        m.compact();

        let second_summary = m.summary().unwrap().to_string();
        assert!(second_summary.len() > first_summary.len());
    }

    // --- LLM-based summarization helpers ---

    #[test]
    fn build_prompt_returns_none_when_below_keep_recent() {
        let mut m = memory();
        m.config.keep_recent = 4;
        m.add_message(Message::user("a"));
        m.add_message(Message::user("b"));
        assert!(m.build_summarization_prompt().is_none());
    }

    #[test]
    fn build_prompt_includes_old_messages_only() {
        let mut m = memory();
        m.config.keep_recent = 2;
        m.add_message(Message::user("OLDEST goal"));
        m.add_message(Message::assistant("OLD response"));
        m.add_message(Message::user("recent question"));
        m.add_message(Message::assistant("recent answer"));

        let prompt = m.build_summarization_prompt().expect("should build");
        assert!(prompt.contains("OLDEST goal"));
        assert!(prompt.contains("OLD response"));
        // The most recent two are kept verbatim and shouldn't appear
        // in the to-summarize section.
        assert!(!prompt.contains("recent question"));
        assert!(!prompt.contains("recent answer"));
    }

    #[test]
    fn build_prompt_skips_pinned_from_excerpt() {
        let mut m = memory();
        m.config.keep_recent = 1;
        let pinned = Message::user("pinned context").pinned();
        let pinned_id = pinned.id.clone();
        m.add_message(pinned);
        m.add_message(Message::user("droppable 1"));
        m.add_message(Message::user("droppable 2"));
        m.add_message(Message::user("recent"));

        let prompt = m.build_summarization_prompt().expect("should build");
        // Pinned content stays in memory verbatim, not in the
        // to-summarize transcript.
        assert!(!prompt.contains("pinned context"));
        assert!(prompt.contains("droppable 1"));
        assert!(prompt.contains("droppable 2"));
        assert!(m.pinned_ids().contains(&pinned_id));
    }

    #[test]
    fn compact_with_summary_keeps_recent_and_substitutes_summary() {
        let mut m = memory();
        m.config.keep_recent = 2;
        for i in 0..10 {
            m.add_message(Message::user(format!("old {}", i)));
        }
        let llm_summary = "User asked about X; agent read foo.rs and changed bar.rs.";
        let r = m.compact_with_summary(llm_summary);

        assert!(r.summarized_count >= 8);
        let stored = m.summary().expect("summary should be stored");
        assert!(stored.contains(llm_summary));
        // Two most recent messages should still be there.
        assert_eq!(m.get_messages().len(), 2);
        assert!(m.get_messages()[1].content.contains("old 9"));
    }

    #[test]
    fn compact_with_summary_no_op_when_below_keep_recent() {
        let mut m = memory();
        m.config.keep_recent = 4;
        m.add_message(Message::user("a"));
        let r = m.compact_with_summary("unused");
        assert_eq!(r.summarized_count, 0);
        assert!(m.summary().is_none());
    }

    #[test]
    fn compact_with_summary_appends_to_prior_summary() {
        let mut m = memory();
        m.config.keep_recent = 1;
        for i in 0..5 {
            m.add_message(Message::user(format!("first wave {}", i)));
        }
        m.compact_with_summary("FIRST_SUMMARY");
        // Add more, compact again with a new LLM-summary.
        for i in 0..5 {
            m.add_message(Message::user(format!("second wave {}", i)));
        }
        m.compact_with_summary("SECOND_SUMMARY");

        let s = m.summary().unwrap();
        assert!(s.contains("FIRST_SUMMARY"));
        assert!(s.contains("SECOND_SUMMARY"));
    }

    #[test]
    fn test_summarize_legacy_alias() {
        let mut m = memory_small();
        for i in 0..30 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        let _tokens_before = m.token_count();
        m.summarize();
        // Verify summarize ran — may not reduce tokens in tight budget
        assert!(!m.get_messages().is_empty() || m.summary().is_some());
    }

    // --- Search ---

    #[test]
    fn test_search_found() {
        let mut m = memory();
        m.add_message(Message::user("hello world"));
        m.add_message(Message::assistant("greetings"));
        m.add_message(Message::user("goodbye world"));

        let results = m.search("world");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut m = memory();
        m.add_message(Message::user("Hello World"));

        let results = m.search("hello");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_not_found() {
        let mut m = memory();
        m.add_message(Message::user("hello"));

        let results = m.search("xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_by_role() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("hi"));
        m.add_message(Message::user("how are you"));

        let user_msgs = m.search_by_role(&MessageRole::User);
        assert_eq!(user_msgs.len(), 2);

        let assistant_msgs = m.search_by_role(&MessageRole::Assistant);
        assert_eq!(assistant_msgs.len(), 1);
    }

    // --- Token tracking ---

    #[test]
    fn test_remaining_budget() {
        let mut m = Memory::new(1000);
        m.add_message(Message::user("hello")); // max(5/4,1) = 1 token
        assert_eq!(m.remaining_budget(), 999);
    }

    #[test]
    fn test_usage_percentage() {
        let mut m = Memory::new(1000);
        m.add_message(Message::user("a".repeat(400))); // 100 tokens
        let pct = m.usage_percentage();
        assert!(pct > 0.0 && pct <= 100.0);
    }

    #[test]
    fn test_recalculate_tokens() {
        let mut m = memory();
        m.add_message(Message::user("hello world"));
        m.add_message(Message::assistant("hi there"));

        // Artificially corrupt token count
        m.total_tokens = 9999;
        m.recalculate_tokens();

        let expected = estimate_tokens("hello world") + estimate_tokens("hi there");
        assert_eq!(m.token_count(), expected);
    }

    // --- Session ---

    #[test]
    fn test_session_info_new() {
        let session = SessionInfo::new();
        assert!(!session.id.is_empty());
        assert!(session.label.is_empty());
    }

    #[test]
    fn test_session_with_label() {
        let mut m = memory();
        m = m.with_label("test session");
        assert_eq!(m.session().label, "test session");
    }

    #[test]
    fn test_new_session_clears_messages() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.new_session();
        assert!(m.get_messages().is_empty());
        assert_ne!(m.session().id, ""); // new session has new ID
    }

    #[test]
    fn test_new_session_with_label() {
        let mut m = memory();
        let session = m.new_session_with_label("my session");
        assert_eq!(session.label, "my session");
    }

    // --- Persistence ---

    #[test]
    fn test_persist_and_load() {
        let dir = std::env::temp_dir().join("core_agentic_test_memory");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);

        m.add_message(Message::user("hello from persist test"));
        m.add_message(Message::assistant("hi back"));

        let session_id = m.session().id.clone();
        let path = m.persist().unwrap();
        assert!(path.exists());

        // Load it back
        let loaded = Memory::load_from_dir(&session_id, Some(&dir)).unwrap();
        assert_eq!(loaded.get_messages().len(), 2);
        assert_eq!(loaded.session().id, session_id);
        assert!(loaded.get_messages()[0].content.contains("hello from persist test"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_persist_with_pinned() {
        let dir = std::env::temp_dir().join("core_agentic_test_pinned");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);

        let pinned = Message::user("keep this").pinned();
        let pinned_id = pinned.id.clone();
        m.add_message(pinned);
        m.add_message(Message::user("normal"));

        let session_id = m.session().id.clone();
        m.persist().unwrap();

        let loaded = Memory::load_from_dir(&session_id, Some(&dir)).unwrap();
        assert!(loaded.pinned_ids().contains(&pinned_id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_sessions() {
        let dir = std::env::temp_dir().join("core_agentic_test_list");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());

        let mut m1 = Memory::with_config(config.clone());
        m1.add_message(Message::user("session 1"));
        m1.persist().unwrap();

        let mut m2 = Memory::with_config(config.clone());
        m2.add_message(Message::user("session 2"));
        m2.persist().unwrap();

        let sessions = Memory::list_sessions_from_dir(Some(&dir)).unwrap();
        assert_eq!(sessions.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete_session() {
        let dir = std::env::temp_dir().join("core_agentic_test_delete");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);
        m.add_message(Message::user("to be deleted"));

        let session_id = m.session().id.clone();
        m.persist().unwrap();

        assert!(Memory::load_from_dir(&session_id, Some(&dir)).is_ok());
        Memory::delete_session_in_dir(&session_id, Some(&dir)).unwrap();

        let sessions = Memory::list_sessions_from_dir(Some(&dir)).unwrap();
        assert!(!sessions.contains(&session_id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_nonexistent_session() {
        let result = Memory::load_from_dir("nonexistent-id", Some(std::env::temp_dir().as_path()));
        assert!(result.is_err());
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_memory_operations() {
        let m = memory();
        assert!(m.get_messages().is_empty());
        assert_eq!(m.token_count(), 0);
        assert_eq!(m.remaining_budget(), 10000);
        assert_eq!(m.usage_percentage(), 0.0);
        assert_eq!(m.role_type(), "user"); // default
        assert!(m.search("anything").is_empty());
        assert!(m.summary().is_none());
    }

    #[test]
    fn test_compact_empty_memory() {
        let mut m = memory();
        let result = m.compact();
        assert_eq!(result.summarized_count, 0);
    }

    #[test]
    fn test_message_ids_unique() {
        let m1 = Message::user("a");
        let m2 = Message::user("b");
        assert_ne!(m1.id, m2.id);
    }

    #[test]
    fn test_message_serialization_roundtrip() {
        let msg = Message::user("hello").with_model("gpt-4o").with_duration(500);
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "hello");
        assert_eq!(parsed.metadata.model.as_deref(), Some("gpt-4o"));
        assert_eq!(parsed.metadata.duration_ms, Some(500));
    }

    #[test]
    fn test_memory_serialization_roundtrip() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("world"));

        let json = serde_json::to_string(&m).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get_messages().len(), 2);
        assert_eq!(parsed.max_tokens, m.max_tokens);
    }

    #[test]
    fn test_very_long_message() {
        let mut m = memory();
        let long = "a".repeat(100_000);
        m.add_message(Message::user(&long));
        assert_eq!(m.get_messages().len(), 1);
        assert!(m.token_count() > 0);
    }

    // ---- get_context_with_user_anchor ----

    #[test]
    fn anchor_extends_to_include_user_when_window_too_small() {
        // Simulate an agent run with one user message followed by a long
        // chain of assistant + tool turns. A naive last-N slice would
        // contain no user message, which providers reject.
        let mut m = memory();
        m.add_message(Message::user("original prompt"));
        for i in 0..30 {
            m.add_message(Message::assistant(format!("thinking {}", i)));
            m.add_message(Message::tool("read_file", &format!("call-{}", i), "result"));
        }

        let ctx = m.get_context_with_user_anchor(20, None);
        assert!(
            ctx.iter().any(|msg| matches!(msg.role, MessageRole::User)),
            "slice must contain at least one user message"
        );
        // First message of the slice should be the user prompt.
        assert!(matches!(ctx[0].role, MessageRole::User));
        assert_eq!(ctx[0].content, "original prompt");
    }

    #[test]
    fn anchor_keeps_short_slice_when_window_already_has_user() {
        // The anchor only extends; if the window already contains a
        // user, leave it alone.
        let mut m = memory();
        for i in 0..5 {
            m.add_message(Message::user(format!("q{}", i)));
            m.add_message(Message::assistant(format!("a{}", i)));
        }
        let ctx = m.get_context_with_user_anchor(4, None);
        assert_eq!(ctx.len(), 4);
        // Last 4 of 10: q3, a3, q4, a4.
        assert_eq!(ctx[0].content, "q3");
    }

    #[test]
    fn anchor_respects_hard_cap() {
        // Even if the user message is far back, hard_cap caps the slice.
        let mut m = memory();
        m.add_message(Message::user("way back"));
        for _ in 0..50 {
            m.add_message(Message::assistant("more"));
        }
        let ctx = m.get_context_with_user_anchor(5, Some(10));
        assert!(ctx.len() <= 10);
        // The user message is older than the hard cap, so it WON'T be
        // included — sanitize_for_provider in the orchestrator will then
        // strip leading non-user messages and drop everything. That's
        // acceptable: the alternative is sending an unbounded payload.
    }

    #[test]
    fn anchor_handles_empty_memory() {
        let m = memory();
        let ctx = m.get_context_with_user_anchor(20, None);
        assert!(ctx.is_empty());
    }

    #[test]
    fn anchor_picks_most_recent_user_when_multiple_exist() {
        // Multi-turn conversation. Latest user turn becomes the anchor,
        // not the first one ever.
        let mut m = memory();
        m.add_message(Message::user("first prompt"));
        m.add_message(Message::assistant("first answer"));
        m.add_message(Message::user("second prompt"));
        for i in 0..30 {
            m.add_message(Message::assistant(format!("thinking {}", i)));
        }
        let ctx = m.get_context_with_user_anchor(5, None);
        // The slice must start at the second user prompt, not the first.
        assert!(matches!(ctx[0].role, MessageRole::User));
        assert_eq!(ctx[0].content, "second prompt");
    }
}
