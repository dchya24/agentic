//! Core `Memory` struct and its primary operations.
//!
//! Holds messages, tracks tokens, manages pinning, builds context
//! windows. Compaction logic lives in `super::compaction`; persistence
//! lives in `super::persist`.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::tool::{ToolCall, ToolResultValue};

use super::types::{
    config_default_max_tokens, estimate_tokens, ContextWindow, MemoryConfig, Message,
    MessageMetadata, MessageRole, SessionInfo,
};

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

    #[serde(default = "config_default_max_tokens")]
    pub max_tokens: u32,

    #[serde(default)]
    pub session: SessionInfo,

    #[serde(default)]
    pub config: MemoryConfig,

    /// IDs of pinned messages (never compacted away).
    /// `pub(super)` so sibling modules in `memory/` (compaction,
    /// persist) can mutate it without exposing it crate-wide.
    #[serde(default)]
    pub(super) pinned_ids: HashSet<String>,

    /// Compaction summary, if any.
    #[serde(default)]
    pub(super) summary: Option<String>,
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

    /// Total token budget for this memory's context window.
    pub fn budget(&self) -> u32 {
        self.max_tokens
    }

    /// Effective request budget: `max_tokens * config.context_budget_ratio`.
    /// This is the value the orchestrator should pass to
    /// `get_context_for_request` so the rest of `max_tokens` is reserved
    /// for the system prompt, tool definitions, and the response.
    pub fn request_budget(&self) -> u32 {
        let ratio = self.config.context_budget_ratio.clamp(0.1, 0.95);
        (self.max_tokens as f64 * ratio) as u32
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
        let recent_slice: Vec<&Message> = self.messages.iter().rev().take(limit).collect();

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

    /// Production-grade context builder for the LLM main loop.
    ///
    /// Strategy (closer to what Claude Code, Aider, and Continue.dev do):
    ///
    /// 1. **Walk turns, not messages.** A "turn" is a self-contained
    ///    group: `user` followed by zero or more `assistant`/`tool`
    ///    pairs until the next `user`. We never cut a turn in half,
    ///    which means we never split an assistant's `tool_calls` from
    ///    its `tool` results.
    ///
    /// 2. **Token budget, not message count.** Walk turns from newest
    ///    to oldest, accumulating estimated tokens, and stop just
    ///    before exceeding `token_budget`. The most recent turn is
    ///    always included even if it alone exceeds budget — the model
    ///    will then truncate, but at least we send a valid request
    ///    rather than nothing.
    ///
    /// 3. **Anchored to user.** Because we walk turn boundaries, the
    ///    result always starts with a `user` message (or system + user
    ///    if a summary is present), satisfying provider requirements.
    ///
    /// 4. **Summary prepended.** If a compaction summary exists
    ///    (`Memory::summary`), it's prepended as a system message at
    ///    the start of the slice so the model has high-level context
    ///    even after older turns are evicted.
    ///
    /// `token_budget` should typically be ~70% of the model's context
    /// window to leave headroom for the system prompt, tool definitions,
    /// and the response itself.
    pub fn get_context_for_request(&self, token_budget: u32) -> Vec<Message> {
        if self.messages.is_empty() {
            return Vec::new();
        }

        // Walk backwards finding turn boundaries (each `user` message
        // starts a new turn). Collect (start_idx, end_idx_exclusive)
        // ranges for each turn.
        let mut turn_starts: Vec<usize> = Vec::new();
        for (i, msg) in self.messages.iter().enumerate() {
            if matches!(msg.role, MessageRole::User) {
                turn_starts.push(i);
            }
        }

        if turn_starts.is_empty() {
            // No user messages exist (shouldn't happen in practice;
            // means orchestrator was misused). Fall back to anchor.
            return self.get_context_with_user_anchor(20, Some(200));
        }

        let total = self.messages.len();
        let mut turns: Vec<(usize, usize)> = Vec::with_capacity(turn_starts.len());
        for (idx, &start) in turn_starts.iter().enumerate() {
            let end = turn_starts.get(idx + 1).copied().unwrap_or(total);
            turns.push((start, end));
        }

        // Walk turns newest-first, accumulating tokens.
        let mut earliest_kept: usize = turns.last().map(|(s, _)| *s).unwrap_or(0);
        let mut used: u32 = 0;
        // Reserve space for an optional summary at the head.
        let summary_tokens: u32 = self.summary.as_deref().map(estimate_tokens).unwrap_or(0);
        let effective_budget = token_budget.saturating_sub(summary_tokens);

        for (start, end) in turns.iter().rev() {
            let turn_tokens: u32 = self.messages[*start..*end]
                .iter()
                .map(|m| {
                    if m.metadata.token_count > 0 {
                        m.metadata.token_count
                    } else {
                        estimate_tokens(&m.content)
                    }
                })
                .sum();

            // Always include the most recent turn, even if it alone
            // blows the budget. Otherwise we'd send an empty request.
            let is_most_recent = *start == turns.last().unwrap().0;

            if !is_most_recent && used + turn_tokens > effective_budget {
                // This turn would overflow; stop and use the previous
                // (newer) earliest_kept.
                break;
            }

            used += turn_tokens;
            earliest_kept = *start;
        }

        // Build the output: optional summary + kept turns in chronological order.
        let mut out: Vec<Message> = Vec::with_capacity(total - earliest_kept + 1);
        if let Some(ref summary) = self.summary {
            out.push(Message {
                id: "__summary__".to_string(),
                role: MessageRole::System,
                content: summary.clone(),
                timestamp: Utc::now(),
                pinned: true,
                metadata: MessageMetadata {
                    token_count: summary_tokens,
                    ..Default::default()
                },
            });
        }
        out.extend(self.messages[earliest_kept..].iter().cloned());
        out
    }

    /// Get all messages.
    pub fn get_messages(&self) -> &[Message] {
        &self.messages
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
