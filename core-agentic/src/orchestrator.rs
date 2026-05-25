//! Orchestrator - Core agent loop

use std::sync::{Arc, Mutex};

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
        context
            .iter()
            .map(|m| {
                let (role, tool_call_id) = match &m.role {
                    MessageRole::User => ("user", None),
                    MessageRole::Assistant => ("assistant", None),
                    MessageRole::System => ("system", None),
                    MessageRole::Tool { tool_call_id, .. } => {
                        ("tool", Some(tool_call_id.clone()))
                    }
                };
                ChatMessageRequest {
                    role: role.to_string(),
                    content: m.content.clone(),
                    tool_call_id,
                    tool_calls: vec![],
                }
            })
            .collect()
    }

    fn handle_tool_calls(&self, content: &str, tool_calls: &[(String, String, String)]) {
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant(content));

        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));

            if self.should_confirm(tc_name, &args) {
                let request = self
                    .safety
                    .create_request(tc_name, &format!("{:?}", args));
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.memory.lock().unwrap().add_message(Message::tool(
                        tc_id.clone(),
                        tc_name.clone(),
                        "Skipped: Confirmation denied".to_string(),
                    ));
                    continue;
                }
            }

            let result = self.execute_tool(tc_name, &args);

            self.memory.lock().unwrap().add_message(Message::tool(
                tc_id.clone(),
                tc_name.clone(),
                result,
            ));
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
                self.handle_tool_calls(&content_buf, &accumulated_tool_calls);
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
    use super::truncate_tool_result;

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
}
