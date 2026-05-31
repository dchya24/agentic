//! Test support: scripted provider + small helpers for orchestrator
//! integration tests.

use core_agentic::providers::{
    ChatChunk, ChatMessageResponse, ChatRequest, ChatResponse, ChatUsage, LLMProvider,
    ProviderError, ProviderResult, StreamResult, ToolCallFunction, ToolCallResponse,
};
use std::sync::Mutex;

/// Provider that returns scripted `ChatResponse`s in order, one per
/// `chat()` call. When the queue is empty, returns an error so a
/// runaway loop fails loudly instead of hanging.
pub struct ScriptedProvider {
    responses: Mutex<Vec<ChatResponse>>,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

impl LLMProvider for ScriptedProvider {
    fn provider_type(&self) -> &str {
        "fake"
    }
    fn provider_id(&self) -> &str {
        "fake"
    }
    fn chat(&self, _req: ChatRequest) -> ProviderResult<ChatResponse> {
        let mut q = self.responses.lock().unwrap();
        if q.is_empty() {
            return Err(ProviderError::new(
                "ScriptedProvider: no scripted response left",
            ));
        }
        Ok(q.remove(0))
    }
    fn chat_stream(&self, _req: ChatRequest) -> StreamResult<ChatChunk, ProviderError> {
        Err(ProviderError::new("streaming not supported in test"))
    }
}

/// Build a single `ToolCallResponse` for a tool call.
pub fn tool_call(id: &str, name: &str, args: &serde_json::Value) -> ToolCallResponse {
    ToolCallResponse {
        id: id.to_string(),
        call_type: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: args.to_string(),
        },
    }
}

/// Build a `ChatResponse` containing exactly one tool call. The
/// assistant's `content` is empty (matches what most providers send
/// alongside a tool-call turn).
pub fn tool_call_response(id: &str, name: &str, args: &serde_json::Value) -> ChatResponse {
    ChatResponse {
        id: format!("resp-{}", id),
        model: "gpt-4o-mini".to_string(),
        message: ChatMessageResponse {
            role: "assistant".to_string(),
            content: None,
            tool_calls: vec![tool_call(id, name, args)],
        },
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    }
}

/// Build a `ChatResponse` containing multiple tool calls in one turn.
pub fn multi_tool_call_response(calls: Vec<ToolCallResponse>) -> ChatResponse {
    ChatResponse {
        id: "resp-multi".to_string(),
        model: "gpt-4o-mini".to_string(),
        message: ChatMessageResponse {
            role: "assistant".to_string(),
            content: None,
            tool_calls: calls,
        },
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    }
}

/// Build a final-text `ChatResponse` (no tool calls). The orchestrator
/// will treat this as the loop terminator.
pub fn text_response(s: &str) -> ChatResponse {
    ChatResponse {
        id: "resp-text".to_string(),
        model: "gpt-4o-mini".to_string(),
        message: ChatMessageResponse {
            role: "assistant".to_string(),
            content: Some(s.to_string()),
            tool_calls: vec![],
        },
        finish_reason: Some("stop".to_string()),
        usage: None,
    }
}

/// Same as [`text_response`] but attaches usage so cost-tracking tests
/// can drive the cumulative-cost path through `record_usage`.
pub fn text_response_with_usage(s: &str, in_tok: u32, out_tok: u32) -> ChatResponse {
    let mut r = text_response(s);
    r.usage = Some(ChatUsage {
        prompt_tokens: in_tok,
        completion_tokens: out_tok,
        total_tokens: in_tok + out_tok,
    });
    r
}

/// Per-test temp dir under the system tmp. Avoids collisions with
/// parallel tests via timestamp + pid.
pub fn tempdir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("agentic-orch-it-{}-{}", pid, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
