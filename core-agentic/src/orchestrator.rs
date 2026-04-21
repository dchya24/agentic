//! Orchestrator - Core agent loop

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::memory::{Memory, Message, MessageRole};
use crate::providers::{ChatMessageRequest, ChatRequest, LLMProvider};
use crate::safety::Safety;
use crate::tool::{ToolCall, ToolResultValue};
use crate::tool_registry::ToolRegistry;
use crate::AgenticError;

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
    memory: RwLock<Memory>,
    safety: Safety,
    state: RwLock<OrchestratorState>,
}

impl Orchestrator {
    pub fn new(provider: Arc<dyn LLMProvider>, tools: ToolRegistry) -> Self {
        Self {
            provider,
            tools,
            memory: RwLock::new(Memory::new(128000)),
            safety: Safety::new(),
            state: RwLock::new(OrchestratorState::Idle),
        }
    }

    pub async fn run(&self, input: &str) -> Result<String, AgenticError> {
        {
            let mut state = self.state.write().await;
            *state = OrchestratorState::Planning;
        }

        self.memory.write().await.add_message(Message::user(input));

        let mut iterations = 0;
        let max_iterations = 10;
        let mut last_output = String::new();

        while iterations < max_iterations {
            iterations += 1;
            
            {
                let mut state = self.state.write().await;
                *state = OrchestratorState::Executing;
            }

            let context = self.memory.read().await.get_context(10);
            let messages: Vec<ChatMessageRequest> = context
                .iter()
                .map(|m| ChatMessageRequest {
                    role: match &m.role {
                        MessageRole::User => "user".to_string(),
                        MessageRole::Assistant => "assistant".to_string(),
                        MessageRole::System => "system".to_string(),
                        MessageRole::Tool { .. } => "tool".to_string(),
                    },
                    content: m.content.clone(),
                })
                .collect();

            let request = ChatRequest::new("gpt-4o", messages);
            
            let response = self.provider.chat(request)
                .map_err(|e| AgenticError::Provider(e.to_string()))?;

            last_output = response.message.content.clone();
            
            self.memory.write().await.add_message(Message::assistant(&last_output));

            if self.should_stop(&last_output, iterations, max_iterations) {
                break;
            }

            if let Some(tool_call) = self.parse_tool_call(&last_output) {
                let result: ToolResultValue = self.tools.execute(tool_call).await
                    .map_err(|e: crate::tool::ToolError| AgenticError::Tool(e.to_string()))?;

                self.memory.write().await.add_message(
                    Message::tool(
                        result.tool_call_id.clone(),
                        result.tool_call_id.clone(),
                        result.output.to_string()
                    )
                );
            }
        }

        let mut state = self.state.write().await;
        *state = OrchestratorState::Completed;

        Ok(last_output)
    }

    fn should_stop(&self, output: &str, iterations: u32, max: u32) -> bool {
        let output = output.to_lowercase();
        
        if iterations >= max {
            return true;
        }

        if output.contains("task completed") || 
           output.contains("done!") ||
           output.contains("finished") ||
           output.contains("successfully") {
            return true;
        }

        false
    }

    fn parse_tool_call(&self, output: &str) -> Option<ToolCall> {
        let mut in_tool_block = false;
        let mut tool_name = String::new();
        let mut args = String::new();

        for line in output.lines() {
            let line = line.trim();
            
            if line.starts_with("```tool:") {
                in_tool_block = true;
                tool_name = line.trim_start_matches("```tool:").trim().to_string();
                continue;
            }
            
            if in_tool_block && line == "```" {
                break;
            }
            
            if in_tool_block {
                args.push_str(line);
                args.push('\n');
            }
        }

        if tool_name.is_empty() {
            return None;
        }

        let arguments: serde_json::Value = serde_json::from_str(&args).unwrap_or(
            serde_json::json!({ "raw": args })
        );

        Some(ToolCall::new(tool_name, arguments))
    }

    pub async fn get_state(&self) -> OrchestratorState {
        *self.state.read().await
    }

    pub async fn clear_memory(&self) {
        self.memory.write().await.clear();
    }
}