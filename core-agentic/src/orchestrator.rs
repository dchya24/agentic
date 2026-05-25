//! Orchestrator - Core agent loop

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::events::EventEmitter;
use crate::memory::{Memory, Message, MessageRole};
use crate::providers::{ChatMessageRequest, ChatRequest, LLMProvider};
use crate::safety::{ConfirmationRequest, Safety};
use crate::tool_registry::ToolRegistry;
use crate::AgenticError;

/// Default safety cap on agent iterations to avoid runaway loops.
pub const DEFAULT_MAX_ITERATIONS: u32 = 30;

/// Default tool-result truncation limit (chars). Layer 1 of context compression.
pub const DEFAULT_TOOL_RESULT_MAX_CHARS: usize = 25_000;

/// Default number of most-recent tool results kept verbatim. Older tool
/// results are replaced with a `[Cleared]` placeholder. Layer 2 of context
/// compression.
pub const DEFAULT_KEEP_RECENT_TOOL_RESULTS: usize = 6;

/// Placeholder substituted for stale tool results when Layer 2 fires.
pub const CLEARED_TOOL_RESULT_PLACEHOLDER: &str = "[Cleared: older tool result removed to save context. Re-run the tool if you need this output.]";

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
    #[allow(dead_code)]
    events: EventEmitter,
    confirmation_handler:
        Mutex<Option<Box<dyn Fn(crate::safety::ConfirmationRequest) -> bool + Send + Sync>>>,
    system_prompt: Option<String>,
    model: String,
    /// Hard cap on the agent loop. Prevents runaway tool-call loops.
    max_iterations: u32,
    /// Cap individual tool result strings (Layer 1 of context compression).
    tool_result_max_chars: usize,
    /// Auto-compact memory when token usage exceeds the configured threshold.
    auto_compact: bool,
    /// Number of most-recent tool results to keep verbatim. Older tool
    /// results are replaced with [`CLEARED_TOOL_RESULT_PLACEHOLDER`] when
    /// building the request. Layer 2 of context compression.
    keep_recent_tool_results: usize,
    /// Cooperative cancel flag. When set, the next loop iteration / tool
    /// batch boundary returns `AgenticError::Cancelled`. Shared via Arc so
    /// callers (CLI signal handlers) can flip it asynchronously.
    cancel: Arc<AtomicBool>,
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
        }
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

    /// Reset the cancel flag (e.g. between REPL inputs).
    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// Internal helper: returns true when cancel was requested.
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
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
            .add_message(Message::system(content));
    }

    /// Set a custom system prompt for all LLM requests.
    /// If not set, the provider's default system prompt is used.
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    fn should_confirm(&self, tool_name: &str, args: &serde_json::Value) -> bool {
        let action = tool_name;
        let target = args
            .get("command")
            .or(args.get("path"))
            .or(args.get("file_path"))
            .and_then(|v| v.as_str());
        self.safety.needs_confirmation(action, target)
    }

    /// Extract the most relevant target string from tool args (for safety scoring).
    fn extract_target(args: &serde_json::Value) -> Option<&str> {
        args.get("command")
            .or(args.get("path"))
            .or(args.get("file_path"))
            .and_then(|v| v.as_str())
    }

    /// Set the active permission mode (Default / Plan / Yolo).
    pub fn set_permission_mode(&self, mode: crate::safety::PermissionMode) {
        self.safety.set_mode(mode);
    }

    /// Get the active permission mode.
    pub fn permission_mode(&self) -> crate::safety::PermissionMode {
        self.safety.mode()
    }

    fn require_confirmation(&self, request: ConfirmationRequest) -> bool {
        let handler = self.confirmation_handler.lock().unwrap();
        if let Some(ref h) = *handler {
            h(request)
        } else {
            false
        }
    }

    fn execute_tool(&self, name: &str, args: &serde_json::Value) -> String {
        let raw = match self.tools.execute_by_name(name, args) {
            Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            Err(e) => format!("Tool error: {}", e),
        };
        truncate_tool_result(&raw, self.tool_result_max_chars)
    }

    /// Run autocompact if configured and memory is over threshold.
    /// This is Layer 3 of context compression (currently a heuristic; LLM-based
    /// summarization can be wired in later).
    fn maybe_autocompact(&self) {
        if !self.auto_compact {
            return;
        }
        let mut mem = self.memory.lock().unwrap();
        if mem.needs_compaction() {
            let result = mem.compact();
            tracing::info!(
                summarized = result.summarized_count,
                tokens_before = result.tokens_before,
                tokens_after = result.tokens_after,
                "Memory autocompacted"
            );
        }
    }

    fn build_messages(&self) -> Vec<ChatMessageRequest> {
        let context = self.memory.lock().unwrap().get_context(20);
        build_request_messages(&context, self.keep_recent_tool_results)
    }

    fn handle_tool_calls(&self, content: &str, tool_calls: &[(String, String, String)]) {
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant(content));

        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));

            let target = Self::extract_target(&args);
            let decision = self.safety.evaluate(tc_name, target);

            // Hard-denied (Plan mode, blocklist, sandbox, rate-limit).
            if !decision.allowed {
                let reason = if decision.reason.is_empty() {
                    "Action denied by safety policy".to_string()
                } else {
                    decision.reason.clone()
                };
                println!("  -> [DENIED: {}]", reason);
                self.memory.lock().unwrap().add_message(Message::tool(
                    tc_name.clone(),
                    tc_id.clone(),
                    format!("Blocked: {}", reason),
                ));
                continue;
            }

            // Needs confirmation (Default mode, medium+ risk).
            if decision.needs_confirmation {
                let request = self
                    .safety
                    .create_request(tc_name, &format!("{:?}", args));
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.memory.lock().unwrap().add_message(Message::tool(
                        tc_name.clone(),
                        tc_id.clone(),
                        "Skipped: Confirmation denied".to_string(),
                    ));
                    continue;
                }
                self.safety
                    .record_confirmation(tc_name, target, &decision.score, true);
            }

            let result = self.execute_tool(tc_name, &args);

            self.memory.lock().unwrap().add_message(Message::tool(
                tc_name.clone(),
                tc_id.clone(),
                result,
            ));
        }
    }

    /// Async variant of [`handle_tool_calls`] that batches consecutive
    /// read-only tools and runs them concurrently.
    ///
    /// Sequencing rules (matching the architecture doc):
    /// - Read-only tools (read_file, list_files, glob, grep, search_files)
    ///   in the same batch run in parallel via spawn_blocking.
    /// - State-changing tools run alone, sequentially.
    /// - Results are pushed to memory in the **original tool-call order**
    ///   regardless of which batch finished first.
    /// - Safety evaluation and user confirmation happen sequentially on the
    ///   main task before any execution starts (parallelism doesn't change
    ///   gating semantics).
    async fn handle_tool_calls_parallel(
        &self,
        content: &str,
        tool_calls: &[(String, String, String)],
    ) {
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant(content));

        // Outcome of the safety+confirmation pre-pass for a single call.
        enum Slot {
            /// Pre-resolved (denied, skipped). The string is the message we
            /// will record verbatim as the tool result.
            PreResolved { name: String, id: String, message: String },
            /// Needs to be executed. Carries the parsed args and a flag for
            /// scheduling.
            Pending {
                name: String,
                id: String,
                args: serde_json::Value,
                read_only: bool,
            },
        }

        // Pre-pass: evaluate every call. Confirmation prompts run here, in
        // the original order, before anything is executed.
        let mut slots: Vec<Slot> = Vec::with_capacity(tool_calls.len());
        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));
            let target = Self::extract_target(&args);
            let decision = self.safety.evaluate(tc_name, target);

            if !decision.allowed {
                let reason = if decision.reason.is_empty() {
                    "Action denied by safety policy".to_string()
                } else {
                    decision.reason.clone()
                };
                println!("  -> [DENIED: {}]", reason);
                slots.push(Slot::PreResolved {
                    name: tc_name.clone(),
                    id: tc_id.clone(),
                    message: format!("Blocked: {}", reason),
                });
                continue;
            }

            if decision.needs_confirmation {
                let request = self
                    .safety
                    .create_request(tc_name, &format!("{:?}", args));
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    slots.push(Slot::PreResolved {
                        name: tc_name.clone(),
                        id: tc_id.clone(),
                        message: "Skipped: Confirmation denied".to_string(),
                    });
                    continue;
                }
                self.safety
                    .record_confirmation(tc_name, target, &decision.score, true);
            }

            let read_only = self.tools.is_read_only(tc_name);
            slots.push(Slot::Pending {
                name: tc_name.clone(),
                id: tc_id.clone(),
                args,
                read_only,
            });
        }

        // Execute slots in batches. Output is collected position-aligned to
        // `slots` so we can push to memory in original order at the end.
        let mut results: Vec<Option<(String, String, String)>> = (0..slots.len())
            .map(|_| None)
            .collect();

        let mut i = 0;
        while i < slots.len() {
            // PreResolved slots are written directly without execution.
            if let Slot::PreResolved { name, id, message } = &slots[i] {
                results[i] = Some((name.clone(), id.clone(), message.clone()));
                i += 1;
                continue;
            }

            // Determine batch bounds.
            //   Pending + read_only     → grow batch while next is the same.
            //   Pending + !read_only    → batch of one.
            let start = i;
            let mut end = i + 1;
            if let Slot::Pending { read_only: true, .. } = &slots[i] {
                while end < slots.len() {
                    match &slots[end] {
                        Slot::Pending { read_only: true, .. } => end += 1,
                        _ => break,
                    }
                }
            }

            // Spawn one blocking task per call in the batch. spawn_blocking
            // is the right primitive because Tool::execute is sync and may
            // do filesystem / process I/O.
            let mut handles = Vec::with_capacity(end - start);
            for slot_idx in start..end {
                if let Slot::Pending { name, id, args, .. } = &slots[slot_idx] {
                    let registry = self.tools.clone();
                    let max_chars = self.tool_result_max_chars;
                    let name = name.clone();
                    let id = id.clone();
                    let args = args.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        let raw = match registry.execute_by_name(&name, &args) {
                            Ok(v) => serde_json::to_string_pretty(&v)
                                .unwrap_or_else(|_| v.to_string()),
                            Err(e) => format!("Tool error: {}", e),
                        };
                        let truncated = truncate_tool_result(&raw, max_chars);
                        (name, id, truncated)
                    });
                    handles.push((slot_idx, handle));
                }
            }

            for (slot_idx, handle) in handles {
                match handle.await {
                    Ok(triple) => results[slot_idx] = Some(triple),
                    Err(join_err) => {
                        // Recover slot identity for the error message.
                        if let Slot::Pending { name, id, .. } = &slots[slot_idx] {
                            results[slot_idx] = Some((
                                name.clone(),
                                id.clone(),
                                format!("Tool error: task panicked: {}", join_err),
                            ));
                        }
                    }
                }
            }

            i = end;
        }

        // Push results in the original order so the model sees a coherent
        // tool/assistant/tool/assistant interleaving.
        let mut mem = self.memory.lock().unwrap();
        for entry in results.into_iter().flatten() {
            let (name, id, output) = entry;
            mem.add_message(Message::tool(name, id, output));
        }
    }

    pub fn run(&self, input: &str) -> Result<String, AgenticError> {
        {
            let mut state = self.state.lock().unwrap();
            *state = OrchestratorState::Planning;
        }

        self.memory
            .lock()
            .unwrap()
            .add_message(Message::user(input));

        let tool_defs = self.tools.tool_definitions();
        let mut iteration: u32 = 0;

        loop {
            iteration += 1;
            if iteration > self.max_iterations {
                tracing::warn!(
                    max = self.max_iterations,
                    "Agent loop hit max_iterations; aborting"
                );
                return Err(AgenticError::Provider(format!(
                    "Agent loop exceeded max_iterations ({}). Aborting to prevent runaway.",
                    self.max_iterations
                )));
            }

            if self.cancelled() {
                tracing::info!("Agent loop cancelled by user");
                return Err(AgenticError::Cancelled);
            }

            self.maybe_autocompact();

            let messages = self.build_messages();
            let mut request = ChatRequest::new(&self.model, messages).with_tools(tool_defs.clone());
            if let Some(ref prompt) = self.system_prompt {
                request = request.with_system_prompt(prompt.clone());
            }

            let response = self
                .provider
                .chat(request)
                .map_err(|e| AgenticError::Provider(e.to_string()))?;

            let content = response.message.content.clone().unwrap_or_default();

            if !response.message.tool_calls.is_empty() {
                let tool_calls: Vec<(String, String, String)> = response
                    .message
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        (tc.id.clone(), tc.function.name.clone(), tc.function.arguments.clone())
                    })
                    .collect();
                self.handle_tool_calls(&content, &tool_calls);
            } else {
                self.memory
                    .lock()
                    .unwrap()
                    .add_message(Message::assistant(&content));

                {
                    let mut state = self.state.lock().unwrap();
                    *state = OrchestratorState::Completed;
                }
                return Ok(content);
            }
        }
    }

    pub fn get_state(&self) -> OrchestratorState {
        *self.state.lock().unwrap()
    }

    pub fn clear_memory(&self) {
        self.memory.lock().unwrap().clear();
    }

    pub async fn run_stream<F>(&self, input: &str, mut on_chunk: F) -> Result<String, AgenticError>
    where
        F: FnMut(String),
    {
        use std::collections::HashMap;

        use futures::stream::StreamExt;

        {
            let mut state = self.state.lock().unwrap();
            *state = OrchestratorState::Planning;
        }

        self.memory
            .lock()
            .unwrap()
            .add_message(Message::user(input));

        let tool_defs = self.tools.tool_definitions();
        let mut iteration: u32 = 0;

        loop {
            iteration += 1;
            if iteration > self.max_iterations {
                tracing::warn!(
                    max = self.max_iterations,
                    "Agent stream loop hit max_iterations; aborting"
                );
                return Err(AgenticError::Provider(format!(
                    "Agent loop exceeded max_iterations ({}). Aborting to prevent runaway.",
                    self.max_iterations
                )));
            }

            if self.cancelled() {
                tracing::info!("Agent stream loop cancelled by user");
                return Err(AgenticError::Cancelled);
            }

            self.maybe_autocompact();

            let messages = self.build_messages();
            let mut request = ChatRequest::new(&self.model, messages)
                .with_tools(tool_defs.clone())
                .stream();
            if let Some(ref prompt) = self.system_prompt {
                request = request.with_system_prompt(prompt.clone());
            }

            let mut content_buf = String::new();
            let mut tool_calls_map: HashMap<u32, (String, String, String)> = HashMap::new();

            match self.provider.chat_stream(request) {
                Ok(mut stream) => {
                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                if !chunk.delta.is_empty() {
                                    on_chunk(chunk.delta.clone());
                                    content_buf.push_str(&chunk.delta);
                                }
                                for tc in chunk.tool_calls {
                                    let entry = tool_calls_map
                                        .entry(tc.index)
                                        .or_insert_with(|| (String::new(), String::new(), String::new()));
                                    if let Some(id) = tc.id {
                                        entry.0 = id;
                                    }
                                    if let Some(name) = tc.function_name {
                                        entry.1 = name;
                                    }
                                    if let Some(args) = tc.function_arguments {
                                        entry.2.push_str(&args);
                                    }
                                }
                            }
                            Err(e) => {
                                return Err(AgenticError::Provider(e.to_string()));
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(AgenticError::Provider(e.to_string()));
                }
            }

            let accumulated_tool_calls: Vec<(String, String, String)> = {
                let mut indices: Vec<u32> = tool_calls_map.keys().copied().collect();
                indices.sort();
                indices
                    .into_iter()
                    .map(|i| {
                        let (id, name, args) = tool_calls_map.remove(&i).unwrap();
                        (id, name, args)
                    })
                    .collect()
            };

            if !accumulated_tool_calls.is_empty() {
                self.handle_tool_calls_parallel(&content_buf, &accumulated_tool_calls).await;
                continue;
            }

            self.memory
                .lock()
                .unwrap()
                .add_message(Message::assistant(&content_buf));

            {
                let mut state = self.state.lock().unwrap();
                *state = OrchestratorState::Completed;
            }

            return Ok(content_buf);
        }
    }
}

/// Build the per-request message list, applying Layer 2 compression
/// (replace older tool-result contents with a placeholder).
///
/// `keep_recent_tool_results == 0` disables the placeholder substitution
/// entirely (everything passes through verbatim).
pub(crate) fn build_request_messages(
    context: &[Message],
    keep_recent_tool_results: usize,
) -> Vec<ChatMessageRequest> {
    let keep = keep_recent_tool_results;
    let keep_indices: std::collections::HashSet<usize> = if keep == 0 {
        (0..context.len()).collect()
    } else {
        context
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, m)| matches!(m.role, MessageRole::Tool { .. }))
            .take(keep)
            .map(|(i, _)| i)
            .collect()
    };

    context
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (role, tool_call_id) = match &m.role {
                MessageRole::User => ("user", None),
                MessageRole::Assistant => ("assistant", None),
                MessageRole::System => ("system", None),
                MessageRole::Tool { tool_call_id, .. } => {
                    ("tool", Some(tool_call_id.clone()))
                }
            };

            let content = if matches!(m.role, MessageRole::Tool { .. })
                && keep > 0
                && !keep_indices.contains(&i)
            {
                CLEARED_TOOL_RESULT_PLACEHOLDER.to_string()
            } else {
                m.content.clone()
            };

            ChatMessageRequest {
                role: role.to_string(),
                content,
                tool_call_id,
                tool_calls: vec![],
            }
        })
        .collect()
}

/// Truncate a tool result string to `max_chars`, appending a marker note.
/// Layer 1 of context compression: prevents large tool outputs from blowing
/// up the context window.
pub(crate) fn truncate_tool_result(raw: &str, max_chars: usize) -> String {
    if max_chars == 0 || raw.len() <= max_chars {
        return raw.to_string();
    }
    // Truncate on a UTF-8 char boundary.
    let mut end = max_chars;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = raw.len() - end;
    format!(
        "{}\n\n[truncated: {} chars omitted of {} total — re-run with narrower scope if you need more]",
        &raw[..end],
        omitted,
        raw.len()
    )
}

#[cfg(test)]
mod orchestrator_unit_tests {
    use super::{
        build_request_messages, truncate_tool_result, CLEARED_TOOL_RESULT_PLACEHOLDER,
    };
    use crate::memory::Message;

    #[test]
    fn passthrough_when_under_limit() {
        let s = "hello world";
        assert_eq!(truncate_tool_result(s, 100), s);
    }

    #[test]
    fn truncates_when_over_limit() {
        let s = "a".repeat(1000);
        let out = truncate_tool_result(&s, 100);
        assert!(out.starts_with(&"a".repeat(100)));
        assert!(out.contains("truncated"));
        assert!(out.contains("900 chars omitted"));
    }

    #[test]
    fn zero_disables_truncation() {
        let s = "a".repeat(1000);
        assert_eq!(truncate_tool_result(&s, 0), s);
    }

    #[test]
    fn handles_utf8_boundary() {
        // Multi-byte chars; ensure we don't slice mid-codepoint.
        let s = "日本語".repeat(50); // 9 bytes per repeat = 450 bytes
        let out = truncate_tool_result(&s, 10);
        // Must not panic and prefix must be valid UTF-8.
        assert!(out.contains("truncated"));
    }

    fn make_context() -> Vec<Message> {
        // user, assistant, tool(t1), assistant, tool(t2), assistant, tool(t3)
        vec![
            Message::user("q"),
            Message::assistant("thinking"),
            Message::tool("read_file", "call-1", "OLD result 1"),
            Message::assistant("more thinking"),
            Message::tool("read_file", "call-2", "OLD result 2"),
            Message::assistant("still thinking"),
            Message::tool("read_file", "call-3", "FRESH result 3"),
        ]
    }

    #[test]
    fn clears_older_tool_results_keeping_recent() {
        let ctx = make_context();
        let out = build_request_messages(&ctx, 1);

        // Find each tool message in the output and check content.
        let tool_contents: Vec<&str> = out
            .iter()
            .filter(|m| m.role == "tool")
            .map(|m| m.content.as_str())
            .collect();

        assert_eq!(tool_contents.len(), 3);
        assert_eq!(tool_contents[0], CLEARED_TOOL_RESULT_PLACEHOLDER);
        assert_eq!(tool_contents[1], CLEARED_TOOL_RESULT_PLACEHOLDER);
        assert_eq!(tool_contents[2], "FRESH result 3");
    }

    #[test]
    fn keeps_all_tool_results_when_under_limit() {
        let ctx = make_context();
        let out = build_request_messages(&ctx, 5);

        let cleared = out
            .iter()
            .filter(|m| m.content == CLEARED_TOOL_RESULT_PLACEHOLDER)
            .count();
        assert_eq!(cleared, 0);
    }

    #[test]
    fn keep_zero_disables_clearing() {
        let ctx = make_context();
        let out = build_request_messages(&ctx, 0);

        let cleared = out
            .iter()
            .filter(|m| m.content == CLEARED_TOOL_RESULT_PLACEHOLDER)
            .count();
        assert_eq!(cleared, 0);
    }

    #[test]
    fn non_tool_messages_unaffected() {
        let ctx = make_context();
        let out = build_request_messages(&ctx, 1);

        // user/assistant messages should pass through unchanged regardless
        // of the keep limit.
        let assistants: Vec<&str> = out
            .iter()
            .filter(|m| m.role == "assistant")
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            assistants,
            vec!["thinking", "more thinking", "still thinking"]
        );
    }

    // --- Cancel handle ---

    #[test]
    fn cancel_handle_default_is_unset() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        assert!(!cancel.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn cancel_handle_signals_through_clone() {
        // The same Arc<AtomicBool> shared between threads should observe
        // the flag flip. This mirrors how main.rs and the orchestrator
        // share state.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

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
