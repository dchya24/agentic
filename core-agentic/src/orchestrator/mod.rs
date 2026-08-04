//! Orchestrator — the agent loop that coordinates LLM calls, tools,
//! safety checks, and memory.
//!
//! Module layout:
//! - [`messages`] — pure helpers for shaping the request slice
//!   (`build_request_messages`, `truncate_tool_result`,
//!   `sanitize_for_provider`, `CLEARED_TOOL_RESULT_PLACEHOLDER`).
//! - [`tool_exec`] — `handle_tool_calls` and the parallel async variant.
//! - [`compaction`] — `maybe_autocompact`, `summarize_via_provider`,
//!   `build_messages`.
//! - [`run`] — the top-level `run` and `run_stream` agent loops.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::events::EventEmitter;
use crate::memory::Memory;
use crate::providers::LLMProvider;
use crate::safety::{ConfirmationRequest, Safety};
use crate::tool_registry::ToolRegistry;

mod compaction;
mod messages;
mod run;
mod tool_exec;

/// Default safety cap on agent iterations to avoid runaway loops.
///
/// 50 is a balance: real agentic codebase work (explore → read → edit →
/// verify) routinely needs 20–40 turns, and the loop now *gracefully
/// finalizes* at the cap rather than hard-aborting, so the cost of
/// touching the limit is low (you still get an answer). Override with
/// [`Orchestrator::set_max_iterations`].
pub const DEFAULT_MAX_ITERATIONS: u32 = 50;

/// Number of consecutive *identical* tool calls (same tool **and** same
/// arguments) before loop detection triggers. Calling the same tool with
/// *different* arguments is legitimate progress and does not count.
const LOOP_DETECTION_THRESHOLD: usize = 3;

/// How many recent tool-call signatures loop detection remembers. Kept
/// `>= LOOP_DETECTION_THRESHOLD` so a full repeat-run always fits inside
/// the window and is never fragmented by the trim.
const LOOP_DETECTION_WINDOW: usize = 8;

/// Default tool-result truncation limit (chars). Layer 1 of context compression.
pub const DEFAULT_TOOL_RESULT_MAX_CHARS: usize = 25_000;

/// Default number of most-recent tool results kept verbatim. Older tool
/// results are replaced with a `[Cleared]` placeholder. Layer 2 of context
/// compression.
pub const DEFAULT_KEEP_RECENT_TOOL_RESULTS: usize = 6;

/// A tool call reduced to the fields that identify duplicate work.
///
/// Two calls with equal [`ToolCallSignature`]s are doing the *exact same
/// thing* — same tool, same arguments — the hallmark of an agent stuck in
/// a loop. Calls that share only a tool name but differ in arguments
/// (e.g. `skill("brainstorming")` vs `skill("debugging")`) are distinct
/// signatures and do **not** count toward loop detection, so legitimate
/// multi-step tool usage is never falsely flagged.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ToolCallSignature {
    tool: String,
    /// Stable hash of the JSON arguments string. Hashed rather than stored
    /// verbatim so the recent-call window stays small even when args are
    /// large (file contents, command strings).
    args_hash: u64,
    /// Truncated argument preview, kept only so the loop-detected error
    /// can show *what* was repeated.
    args_preview: String,
}

impl ToolCallSignature {
    fn new(tool: &str, args: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        args.hash(&mut hasher);
        Self {
            tool: tool.to_string(),
            args_hash: hasher.finish(),
            args_preview: truncate_args_preview(args, 120),
        }
    }
}

/// Truncate `s` to `max` bytes on a UTF-8 boundary, appending an ellipsis
/// when trimmed. Used for compact diagnostic previews in log/error output.
fn truncate_args_preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorState {
    Idle,
    Planning,
    Executing,
    Completed,
}

/// Confirmation handler: a boxed closure deciding whether a risky
/// state-changing tool call may proceed.
type ConfirmationHandler = Box<dyn Fn(ConfirmationRequest) -> bool + Send + Sync>;

pub struct Orchestrator {
    provider: Arc<dyn LLMProvider>,
    tools: ToolRegistry,
    memory: Mutex<Memory>,
    safety: Safety,
    state: Mutex<OrchestratorState>,
    events: EventEmitter,
    confirmation_handler: Mutex<Option<ConfirmationHandler>>,
    system_prompt: Option<String>,
    model: String,
    /// Hard cap on the agent loop. Prevents runaway tool-call loops.
    max_iterations: u32,
    /// Recent tool-call signatures for loop detection (sliding window).
    recent_tool_calls: Mutex<Vec<ToolCallSignature>>,
    /// Cap individual tool result strings (Layer 1 of context compression).
    tool_result_max_chars: usize,
    /// Auto-compact memory when token usage exceeds the configured threshold.
    auto_compact: bool,
    /// Number of most-recent tool results to keep verbatim. Older tool
    /// results are replaced with `CLEARED_TOOL_RESULT_PLACEHOLDER` when
    /// building the request. Layer 2 of context compression.
    keep_recent_tool_results: usize,
    /// Cooperative cancel flag. When set, the next loop iteration / tool
    /// batch boundary returns `AgenticError::Cancelled`. Shared via Arc so
    /// callers (CLI signal handlers) can flip it asynchronously.
    cancel: Arc<AtomicBool>,
    /// When `true`, autocompact will ask the LLM to summarize older
    /// messages instead of using the heuristic string truncation.
    /// Falls back to the heuristic on provider error.
    auto_compact_with_llm: bool,
    /// Optional model name used for summarization calls. Defaults to the
    /// orchestrator's main `model` when unset.
    summarizer_model: Option<String>,
}

impl Orchestrator {
    pub fn new(provider: Arc<dyn LLMProvider>, tools: ToolRegistry) -> Self {
        Self {
            provider,
            tools,
            memory: Mutex::new(Memory::new(128000)),
            safety: Safety::new(),
            state: Mutex::new(OrchestratorState::Idle),
            events: EventEmitter::new(),
            confirmation_handler: Mutex::new(None),
            system_prompt: None,
            model: "glm-4.7".to_string(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            recent_tool_calls: Mutex::new(Vec::new()),
            tool_result_max_chars: DEFAULT_TOOL_RESULT_MAX_CHARS,
            auto_compact: true,
            keep_recent_tool_results: DEFAULT_KEEP_RECENT_TOOL_RESULTS,
            cancel: Arc::new(AtomicBool::new(false)),
            auto_compact_with_llm: false,
            summarizer_model: None,
        }
    }

    /// Subscribe to runtime events (tool calls, results, errors, system
    /// messages). Multiple subscribers are supported. Handlers must be
    /// `Send + Sync` because emissions can happen from blocking-pool tasks.
    pub fn on_event<F>(&self, handler: F)
    where
        F: Fn(crate::events::Event) + Send + Sync + 'static,
    {
        self.events.on(handler);
    }

    /// Drop all event handlers. Call between runs if you re-subscribe each
    /// invocation to avoid handler accumulation.
    pub fn clear_event_handlers(&self) {
        self.events.clear();
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Override the maximum number of agent loop iterations.
    pub fn set_max_iterations(&mut self, max: u32) {
        self.max_iterations = max.max(1);
    }

    /// Record this turn's tool calls and return the offending signature
    /// + run length if loop detection trips.
    ///
    /// Two rules keep legitimate work from being flagged:
    ///
    /// 1. **Arguments are part of the signature.** `skill("brainstorming")`
    ///    and `skill("debugging")` are *different* calls — loading several
    ///    skills is progress, not a loop. Only the *exact same* call
    ///    (same tool **and** same arguments) counts as a repeat.
    /// 2. **Identical calls within one assistant turn are de-duplicated.**
    ///    A turn that batch-requests `slow_read({})` four times in parallel
    ///    is one model decision, not a loop — the loop is the model
    ///    *re-deciding* the same thing across separate turns.
    ///
    /// A loop is then `LOOP_DETECTION_THRESHOLD` consecutive identical
    /// signatures in the recent window. Scattered repeats (e.g. legitimately
    /// re-reading a file after editing it) don't form a consecutive run and
    /// won't trip the guard.
    fn record_tool_calls_for_loop_detection(
        &self,
        calls: &[(&str, &str)],
    ) -> Option<(ToolCallSignature, usize)> {
        // De-dup within this turn (rule 2). N is tiny, so a linear
        // `contains` is cheaper than a HashSet and keeps first-seen order.
        let mut new_sigs: Vec<ToolCallSignature> = Vec::with_capacity(calls.len());
        for (name, args) in calls {
            let sig = ToolCallSignature::new(name, args);
            if !new_sigs.contains(&sig) {
                new_sigs.push(sig);
            }
        }

        let mut recent = self.recent_tool_calls.lock().unwrap();
        recent.extend(new_sigs.iter().cloned());

        // Trim to a sliding window. Must be >= threshold so a full
        // repeat-run is never fragmented by the trim itself.
        if recent.len() > LOOP_DETECTION_WINDOW {
            let split = recent.len() - LOOP_DETECTION_WINDOW;
            recent.drain(0..split);
        }

        // Consecutive-run detection: longest back-to-back run of an
        // identical signature. Only newly-added signatures can be the ones
        // that just crossed the threshold, so we only scan for those.
        for sig in &new_sigs {
            let run = Self::longest_consecutive_run(&recent, sig);
            if run >= LOOP_DETECTION_THRESHOLD {
                return Some((sig.clone(), run));
            }
        }
        None
    }

    /// Length of the longest run of `target` appearing in consecutive
    /// positions of `recent`. O(n) over a tiny window.
    fn longest_consecutive_run(recent: &[ToolCallSignature], target: &ToolCallSignature) -> usize {
        let mut best = 0;
        let mut cur = 0;
        for s in recent {
            if s == target {
                cur += 1;
                if cur > best {
                    best = cur;
                }
            } else {
                cur = 0;
            }
        }
        best
    }

    /// Clear the loop detection history (e.g. after a successful non-tool response).
    fn clear_loop_detection(&self) {
        self.recent_tool_calls.lock().unwrap().clear();
    }

    /// Check if we're approaching the iteration limit (>= 80%).
    fn approaching_limit(&self, iteration: u32) -> bool {
        iteration >= (self.max_iterations * 4 / 5).max(1)
    }

    /// Override the per-tool-result character cap.
    pub fn set_tool_result_max_chars(&mut self, max: usize) {
        self.tool_result_max_chars = max;
    }

    /// Enable or disable automatic memory compaction.
    pub fn set_auto_compact(&mut self, enabled: bool) {
        self.auto_compact = enabled;
    }

    /// Override how many recent tool results are kept verbatim before
    /// older ones are cleared (Layer 2 of context compression).
    /// `0` disables clearing (everything kept until autocompact runs).
    pub fn set_keep_recent_tool_results(&mut self, n: usize) {
        self.keep_recent_tool_results = n;
    }

    /// Get a clone of the cancel flag. Wire this to your signal handler
    /// (e.g. Ctrl+C) so the agent can shut down gracefully between turns
    /// rather than being killed mid-flight.
    ///
    /// ```ignore
    /// let cancel = orchestrator.cancel_handle();
    /// tokio::spawn(async move {
    ///     tokio::signal::ctrl_c().await.ok();
    ///     cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    /// });
    /// ```
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Replace the orchestrator's internal cancel flag with an externally
    /// owned one (e.g. a process-global handle shared with the signal
    /// handler). Use this when you want a single Ctrl+C handler to drive
    /// multiple orchestrator instances.
    pub fn set_cancel_handle(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
    }

    /// Enable LLM-based summarization for autocompact. When enabled, the
    /// orchestrator asks the provider to summarize older messages on
    /// compaction; on provider error it falls back to the heuristic.
    pub fn set_auto_compact_with_llm(&mut self, enabled: bool) {
        self.auto_compact_with_llm = enabled;
    }

    /// Override the model used for summarization. Defaults to the main
    /// model. Setting a cheaper/faster model here is recommended.
    pub fn set_summarizer_model(&mut self, model: impl Into<String>) {
        self.summarizer_model = Some(model.into());
    }

    /// Reset the cancel flag (e.g. between REPL inputs).
    pub fn reset_cancel(&self) {
        self.cancel
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn set_confirmation_handler<F>(&mut self, handler: F)
    where
        F: Fn(ConfirmationRequest) -> bool + Send + Sync + 'static,
    {
        let mut h = self.confirmation_handler.lock().unwrap();
        *h = Some(Box::new(handler));
    }

    pub fn add_system_message(&self, content: String) {
        self.memory
            .lock()
            .unwrap()
            .add_message(crate::memory::Message::system(content));
    }

    /// Set a custom system prompt for all LLM requests.
    /// If not set, the provider's default system prompt is used.
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Set the active permission mode (Default / Plan / Yolo).
    pub fn set_permission_mode(&self, mode: crate::safety::PermissionMode) {
        self.safety.set_mode(mode);
    }

    /// Get the active permission mode.
    pub fn permission_mode(&self) -> crate::safety::PermissionMode {
        self.safety.mode()
    }

    pub(super) fn require_confirmation(&self, request: ConfirmationRequest) -> bool {
        let handler = self.confirmation_handler.lock().unwrap();
        if let Some(ref h) = *handler {
            h(request)
        } else {
            false
        }
    }

    pub fn get_state(&self) -> OrchestratorState {
        *self.state.lock().unwrap()
    }

    pub fn clear_memory(&self) {
        self.memory.lock().unwrap().clear();
    }

    /// Borrow the orchestrator's tool registry. Useful for callers that
    /// want to reuse the same builtin set + URL policy + tracker (e.g.
    /// the planner's `execute_plan`).
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Search the conversation memory for messages whose content contains
    /// the given query (case-insensitive). Returns owned snippets so the
    /// caller doesn't need to hold the memory lock.
    ///
    /// Each result is `(role, content)`. The newest matches come last,
    /// matching insertion order in `Memory`.
    pub fn search_memory(&self, query: &str) -> Vec<(crate::memory::MessageRole, String)> {
        let mem = self.memory.lock().unwrap();
        mem.search(query)
            .into_iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect()
    }
}

#[cfg(test)]
mod cancel_handle_tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn cancel_handle_default_is_unset() {
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(!cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_handle_signals_through_clone() {
        // The same Arc<AtomicBool> shared between threads should observe
        // the flag flip. This mirrors how main.rs and the orchestrator
        // share state.
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();

        std::thread::spawn(move || {
            cancel_clone.store(true, Ordering::SeqCst);
        })
        .join()
        .unwrap();

        assert!(cancel.load(Ordering::SeqCst));
    }
}
