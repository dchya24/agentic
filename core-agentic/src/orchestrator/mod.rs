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
pub const DEFAULT_MAX_ITERATIONS: u32 = 30;

/// Default tool-result truncation limit (chars). Layer 1 of context compression.
pub const DEFAULT_TOOL_RESULT_MAX_CHARS: usize = 25_000;

/// Default number of most-recent tool results kept verbatim. Older tool
/// results are replaced with a `[Cleared]` placeholder. Layer 2 of context
/// compression.
pub const DEFAULT_KEEP_RECENT_TOOL_RESULTS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorState {
    Idle,
    Planning,
    Executing,
    Completed,
}

pub struct Orchestrator {
    provider: Arc<dyn LLMProvider>,
    tools: ToolRegistry,
    memory: Mutex<Memory>,
    safety: Safety,
    state: Mutex<OrchestratorState>,
    events: EventEmitter,
    confirmation_handler:
        Mutex<Option<Box<dyn Fn(ConfirmationRequest) -> bool + Send + Sync>>>,
    system_prompt: Option<String>,
    model: String,
    /// Hard cap on the agent loop. Prevents runaway tool-call loops.
    max_iterations: u32,
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
    /// Soft USD budget cap. When set, the orchestrator cancels the next
    /// iteration once `cumulative_cost_usd` exceeds this value.
    budget_usd: Option<f64>,
    /// Cumulative cost in USD since this orchestrator was constructed.
    /// `None` if any provider call had an unknown model price.
    cumulative_cost_usd: Mutex<Option<f64>>,
    /// Optional per-model pricing overrides. Consulted before the
    /// built-in `pricing::lookup` table.
    pricing_overrides: std::collections::HashMap<String, crate::pricing::ModelPricing>,
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
            tool_result_max_chars: DEFAULT_TOOL_RESULT_MAX_CHARS,
            auto_compact: true,
            keep_recent_tool_results: DEFAULT_KEEP_RECENT_TOOL_RESULTS,
            cancel: Arc::new(AtomicBool::new(false)),
            auto_compact_with_llm: false,
            summarizer_model: None,
            budget_usd: None,
            cumulative_cost_usd: Mutex::new(Some(0.0)),
            pricing_overrides: std::collections::HashMap::new(),
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

    /// Set a soft USD budget cap. When the cumulative provider cost
    /// exceeds this value the agent loop cancels at the next iteration
    /// boundary. Pass `None` to disable.
    pub fn set_budget_usd(&mut self, budget: Option<f64>) {
        self.budget_usd = budget;
    }

    /// Cumulative provider cost in USD since this orchestrator was
    /// constructed. `None` if any turn used a model without pricing
    /// data and no override was supplied.
    pub fn cumulative_cost_usd(&self) -> Option<f64> {
        *self.cumulative_cost_usd.lock().unwrap()
    }

    /// Reset the cumulative-cost running total to zero. Useful when
    /// restarting a session in-place so the budget cap and the
    /// status-bar segment start fresh.
    pub fn reset_cumulative_cost(&self) {
        *self.cumulative_cost_usd.lock().unwrap() = Some(0.0);
    }

    /// Replace the per-model pricing override map.
    pub fn set_pricing_overrides(
        &mut self,
        overrides: std::collections::HashMap<String, crate::pricing::ModelPricing>,
    ) {
        self.pricing_overrides = overrides;
    }

    /// Resolve a price for `model`: overrides first, then the built-in
    /// table.
    pub(super) fn lookup_pricing(&self, model: &str) -> Option<crate::pricing::ModelPricing> {
        if let Some(p) = self.pricing_overrides.get(model) {
            return Some(*p);
        }
        crate::pricing::lookup(model)
    }

    /// Record a provider usage report. Updates the cumulative-cost
    /// running total and emits an `Event::Usage`. Returns the cost in
    /// USD for this single call (`None` if pricing unavailable).
    pub fn record_usage(&self, input_tokens: u32, output_tokens: u32) -> Option<f64> {
        let pricing = self.lookup_pricing(&self.model);
        let call_cost = pricing.map(|p| p.cost_usd(input_tokens, output_tokens));

        let cumulative = {
            let mut total = self.cumulative_cost_usd.lock().unwrap();
            match (*total, call_cost) {
                (Some(t), Some(c)) => {
                    *total = Some(t + c);
                }
                _ => {
                    // Any unknown turn poisons the cumulative total.
                    *total = None;
                }
            }
            *total
        };

        self.events.emit(crate::events::Event::Usage {
            model: self.model.clone(),
            input_tokens,
            output_tokens,
            cost_usd: call_cost,
            cumulative_cost_usd: cumulative,
        });

        call_cost
    }

    /// Returns true if the configured budget has been exceeded.
    pub(super) fn budget_exceeded(&self) -> bool {
        match (self.budget_usd, self.cumulative_cost_usd()) {
            (Some(cap), Some(spent)) => spent > cap,
            _ => false,
        }
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

#[cfg(test)]
mod cost_tracking_tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use crate::providers::{
        ChatChunk, ChatMessageResponse, ChatRequest, ChatResponse, ChatUsage, LLMProvider,
        ProviderError, ProviderResult, StreamResult,
    };
    use crate::tool_registry::ToolRegistry;

    use super::Orchestrator;

    /// Provider that returns scripted `ChatResponse`s with usage attached.
    struct ScriptedProvider {
        responses: Mutex<Vec<ChatResponse>>,
    }

    impl ScriptedProvider {
        fn new(responses: Vec<ChatResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    impl LLMProvider for ScriptedProvider {
        fn provider_type(&self) -> &str { "fake" }
        fn provider_id(&self) -> &str { "fake" }
        fn chat(&self, _req: ChatRequest) -> ProviderResult<ChatResponse> {
            let mut q = self.responses.lock().unwrap();
            if q.is_empty() {
                return Err(ProviderError::new("no scripted response"));
            }
            Ok(q.remove(0))
        }
        fn chat_stream(&self, _req: ChatRequest) -> StreamResult<ChatChunk, ProviderError> {
            Err(ProviderError::new("streaming not supported in test"))
        }
    }

    fn text_response_with_usage(s: &str, in_tok: u32, out_tok: u32) -> ChatResponse {
        ChatResponse {
            id: "test".into(),
            model: "gpt-4o-mini".into(),
            message: ChatMessageResponse {
                role: "assistant".into(),
                content: Some(s.into()),
                tool_calls: vec![],
            },
            finish_reason: Some("stop".into()),
            usage: Some(ChatUsage {
                prompt_tokens: in_tok,
                completion_tokens: out_tok,
                total_tokens: in_tok + out_tok,
            }),
        }
    }

    fn make_orch(model: &str, responses: Vec<ChatResponse>) -> Orchestrator {
        let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(responses));
        let mut o = Orchestrator::new(provider, ToolRegistry::new());
        o.set_model(model);
        o
    }

    #[test]
    fn record_usage_emits_event_with_cost() {
        let o = make_orch("gpt-4o-mini", vec![]);

        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        o.on_event(move |event| {
            if let crate::events::Event::Usage {
                model,
                input_tokens,
                output_tokens,
                cost_usd,
                cumulative_cost_usd,
            } = event
            {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((model, input_tokens, output_tokens, cost_usd, cumulative_cost_usd));
            }
        });

        // 1M in + 1M out at gpt-4o-mini ($0.15 / $0.60 per M) = $0.75.
        let cost = o.record_usage(1_000_000, 1_000_000);
        let cost = cost.expect("gpt-4o-mini has known pricing");
        assert!((cost - 0.75).abs() < 1e-9);

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        let (ref model, in_tok, out_tok, c, cum) = events[0];
        assert_eq!(model, "gpt-4o-mini");
        assert_eq!(in_tok, 1_000_000);
        assert_eq!(out_tok, 1_000_000);
        assert!((c.unwrap() - 0.75).abs() < 1e-9);
        assert!((cum.unwrap() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn cumulative_cost_accumulates_across_calls() {
        let o = make_orch("gpt-4o-mini", vec![]);
        o.record_usage(100_000, 50_000);
        o.record_usage(200_000, 100_000);
        // (100k * 0.15 + 50k * 0.60) / 1M = 0.015 + 0.030 = 0.045
        // (200k * 0.15 + 100k * 0.60) / 1M = 0.030 + 0.060 = 0.090
        let total = o.cumulative_cost_usd().expect("known model");
        assert!((total - 0.135).abs() < 1e-6);
    }

    #[test]
    fn unknown_model_poisons_cumulative_cost() {
        let o = make_orch("some-unlisted-model", vec![]);
        o.record_usage(1_000, 1_000);
        // Cost for one call is unknown — cumulative becomes None.
        assert!(o.cumulative_cost_usd().is_none());
    }

    #[test]
    fn pricing_override_used_when_set() {
        use crate::pricing::ModelPricing;
        let mut o = make_orch("my-custom-model", vec![]);
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("my-custom-model".to_string(), ModelPricing::new(1.00, 2.00));
        o.set_pricing_overrides(overrides);
        let cost = o.record_usage(1_000_000, 1_000_000).expect("override");
        assert!((cost - 3.00).abs() < 1e-9);
    }

    #[test]
    fn budget_exceeded_reports_correctly() {
        let mut o = make_orch("gpt-4o-mini", vec![]);
        o.set_budget_usd(Some(0.05));

        // Under budget.
        o.record_usage(100_000, 50_000); // = $0.045
        assert!(!o.budget_exceeded());

        // Push over.
        o.record_usage(50_000, 0); // + $0.0075 = $0.0525
        assert!(o.budget_exceeded());
    }

    #[test]
    fn budget_disabled_when_unset() {
        let o = make_orch("gpt-4o-mini", vec![]);
        // Big spend, no budget — still allowed.
        o.record_usage(10_000_000, 10_000_000);
        assert!(!o.budget_exceeded());
    }

    #[test]
    fn reset_cumulative_cost_zeroes_total() {
        let o = make_orch("gpt-4o-mini", vec![]);
        o.record_usage(1_000_000, 1_000_000);
        assert!(o.cumulative_cost_usd().unwrap() > 0.0);
        o.reset_cumulative_cost();
        assert_eq!(o.cumulative_cost_usd(), Some(0.0));
    }

    #[test]
    fn reset_cumulative_cost_clears_unknown_pricing_state() {
        let o = make_orch("some-unlisted-model", vec![]);
        o.record_usage(1_000, 1_000);
        // Unknown model poisoned the total.
        assert!(o.cumulative_cost_usd().is_none());
        o.reset_cumulative_cost();
        // After reset, back to a known zero.
        assert_eq!(o.cumulative_cost_usd(), Some(0.0));
    }

    #[test]
    fn run_loop_returns_error_when_budget_exceeded() {
        // Pre-load cumulative cost over budget; the loop should bail at
        // the next iteration before calling the provider.
        let mut o = make_orch("gpt-4o-mini", vec![text_response_with_usage("hi", 0, 0)]);
        o.set_budget_usd(Some(0.001));
        o.record_usage(1_000_000, 1_000_000); // $0.75 — way over.

        let err = o.run("please respond").expect_err("budget should block");
        let msg = err.to_string();
        assert!(msg.contains("Budget exceeded"), "got: {}", msg);
    }
}
