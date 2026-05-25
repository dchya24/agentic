//! `spawn_subagent` tool.
//!
//! Spawns a subagent that runs the **same** agentic loop but with an
//! **isolated** conversation history. The parent only sees the final text
//! the subagent returns. This is the architecture doc's "subagent" pattern:
//! complex tasks are decomposed into subtasks whose context doesn't pollute
//! the parent.
//!
//! What's shared with the parent:
//! - The `ToolRegistry` (so subagents have the same tools).
//! - The `LLMProvider` (same network connection, same auth).
//! - Safety policy (blocklist, sandbox) is enforced via a fresh `Safety`
//!   inside the new orchestrator; the parent's permission mode is copied.
//!
//! What's isolated:
//! - The conversation history (fresh `Memory`).
//! - Tool result truncation / autocompact / cancel state.
//!
//! Subagents have a tighter `max_iterations` than the main agent (default
//! 12) to bound runaway token usage.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::providers::LLMProvider;
use crate::safety::PermissionMode;
use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};
use crate::tool_registry::ToolRegistry;

/// Default cap on subagent iterations. Tighter than the main agent's loop
/// because subagents handle scoped subtasks.
pub const DEFAULT_SUBAGENT_MAX_ITERATIONS: u32 = 12;

pub struct SpawnSubagentTool {
    provider: Arc<dyn LLMProvider>,
    tools: ToolRegistry,
    /// Model used by spawned subagents (defaults to the parent's model).
    model: String,
    /// Permission mode applied to subagents at construction. Mirrors the
    /// parent's mode at the moment the tool was registered.
    mode: PermissionMode,
    /// Cancel flag shared with the parent so a parent cancel kills the
    /// subagent too. Matches the architecture doc's "linked abort signal".
    parent_cancel: Option<Arc<AtomicBool>>,
    max_iterations: u32,
}

impl SpawnSubagentTool {
    pub fn new(
        provider: Arc<dyn LLMProvider>,
        tools: ToolRegistry,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tools,
            model: model.into(),
            mode: PermissionMode::Default,
            parent_cancel: None,
            max_iterations: DEFAULT_SUBAGENT_MAX_ITERATIONS,
        }
    }

    pub fn with_mode(mut self, mode: PermissionMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.parent_cancel = Some(cancel);
        self
    }

    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max.max(1);
        self
    }
}

const SUBAGENT_SYSTEM_PROMPT: &str = r#"You are a focused subagent spawned by a parent AI coding agent to handle a scoped subtask.

You have the same tools as the parent (read_file, edit_file, list_files, etc.) but a FRESH conversation history. The parent does not see your tool calls; it only sees the final text you return.

Rules:
1. Complete the task you were given. Don't try to coordinate with the parent.
2. Be concise in your final answer. The parent uses your text as a summary.
3. Don't ask clarifying questions. If the task is ambiguous, make a reasonable assumption and note it.
4. Stop as soon as you have an answer. Don't keep using tools speculatively.
"#;

impl Tool for SpawnSubagentTool {
    fn name(&self) -> &str {
        "spawn_subagent"
    }

    fn description(&self) -> &str {
        "Spawn a subagent with an isolated conversation history to handle a focused subtask. \
         Use this when a step would pollute the main context (large file exploration, multi-step \
         refactor, etc.). The parent only sees the subagent's final text answer."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "task".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some(
                    "The subtask description. Be specific and self-contained: the subagent has no memory of the parent conversation.".to_string(),
                ),
                default: None,
            },
        );
        params.insert(
            "max_iterations".to_string(),
            ToolParam {
                param_type: "number".to_string(),
                description: Some(
                    "Optional cap on subagent loop iterations. Defaults to a small value.".to_string(),
                ),
                default: Some(serde_json::json!(DEFAULT_SUBAGENT_MAX_ITERATIONS)),
            },
        );

        ToolSchema {
            name: "spawn_subagent".to_string(),
            description: "Spawn a subagent with isolated context for a focused subtask.".to_string(),
            parameters: params,
            required: vec!["task".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let task = args_obj
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: task"))?;

        if task.trim().is_empty() {
            return Err(ToolError::new("task must not be empty"));
        }

        let max_iter = args_obj
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(self.max_iterations);

        // Build a fresh orchestrator. We use the synchronous `run()` so we
        // don't need a Tokio runtime context; spawn_subagent itself runs
        // inside Tool::execute, which the orchestrator may call from a
        // spawn_blocking thread.
        let mut sub = crate::orchestrator::Orchestrator::new(
            self.provider.clone(),
            self.tools.clone(),
        );
        sub.set_model(self.model.clone());
        sub.set_max_iterations(max_iter);
        sub.set_system_prompt(SUBAGENT_SYSTEM_PROMPT);
        sub.set_permission_mode(self.mode);
        if let Some(c) = &self.parent_cancel {
            sub.set_cancel_handle(c.clone());
        }

        let answer = sub.run(task).map_err(|e| {
            let preview: String = task.chars().take(80).collect();
            ToolError::new(format!("Subagent failed: {} (task='{}')", e, preview))
        })?;

        Ok(serde_json::json!({
            "success": true,
            "answer": answer,
            "max_iterations": max_iter,
        }))
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        ChatChunk, ChatMessageResponse, ChatRequest, ChatResponse, LLMProvider, ProviderError,
        ProviderResult, StreamResult,
    };
    use std::sync::Mutex;

    /// Minimal fake provider for subagent testing.
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

    fn text_response(s: &str) -> ChatResponse {
        ChatResponse {
            id: "test".into(),
            model: "fake".into(),
            message: ChatMessageResponse {
                role: "assistant".into(),
                content: Some(s.into()),
                tool_calls: vec![],
            },
            finish_reason: Some("stop".into()),
            usage: None,
        }
    }

    #[test]
    fn rejects_empty_task() {
        let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![]));
        let tool = SpawnSubagentTool::new(provider, ToolRegistry::new(), "fake");
        let err = tool
            .execute(serde_json::json!({"task": "   "}))
            .expect_err("empty task should fail");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn returns_subagent_answer_to_parent() {
        let provider: Arc<dyn LLMProvider> =
            Arc::new(ScriptedProvider::new(vec![text_response("42 is the answer")]));
        let tool = SpawnSubagentTool::new(provider, ToolRegistry::new(), "fake");
        let result = tool
            .execute(serde_json::json!({"task": "what is the meaning of life?"}))
            .expect("subagent should succeed");
        assert_eq!(result["success"], true);
        assert_eq!(result["answer"], "42 is the answer");
    }

    #[test]
    fn respects_max_iterations_argument() {
        let provider: Arc<dyn LLMProvider> =
            Arc::new(ScriptedProvider::new(vec![text_response("done")]));
        let tool = SpawnSubagentTool::new(provider, ToolRegistry::new(), "fake");
        let result = tool
            .execute(serde_json::json!({"task": "x", "max_iterations": 1}))
            .expect("should succeed");
        assert_eq!(result["max_iterations"], 1);
    }
}
