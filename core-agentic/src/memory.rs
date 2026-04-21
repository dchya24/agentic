//! Memory and context management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::tool::{ToolCall, ToolResultValue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

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

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn tool(
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool {
                tool_name: tool_name.into(),
                tool_call_id: tool_call_id.into(),
            },
            content: content.into(),
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResultValue>,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(default)]
    pub max_tokens: u32,
}

impl Memory {
    pub fn new(max_tokens: u32) -> Self {
        Self {
            messages: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            total_tokens: 0,
            max_tokens,
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.total_tokens += estimate_tokens(&message.content);
        self.messages.push(message);
    }

    pub fn add_tool_call(&mut self, call: ToolCall) {
        self.tool_calls.push(call);
    }

    pub fn add_tool_result(&mut self, result: ToolResultValue) {
        self.tool_results.push(result);
    }

    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn get_context(&self, max_messages: usize) -> Vec<Message> {
        let start = self.messages.len().saturating_sub(max_messages);
        self.messages[start..].to_vec()
    }

    pub fn token_count(&self) -> u32 {
        self.total_tokens
    }

    pub fn role_type(&self) -> &str {
        self.messages
            .last()
            .map(|m| m.role.as_str())
            .unwrap_or("user")
    }

    pub fn needs_summarization(&self) -> bool {
        self.total_tokens >= self.max_tokens.saturating_sub(1000)
    }

    pub fn summarize(&mut self) {
        if self.messages.is_empty() {
            return;
        }

        let summary = format!(
            "Previous context ({} messages, {} tokens): {}",
            self.messages.len(),
            self.total_tokens,
            self.messages
                .iter()
                .take(3)
                .map(|m| format!(
                    "[{}]: {}",
                    m.role.as_str(),
                    &m.content[..m.content.len().min(100)]
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );

        let tokens = estimate_tokens(&summary);

        self.messages.clear();
        self.messages.push(Message {
            role: MessageRole::System,
            content: summary,
            timestamp: Utc::now(),
        });
        self.total_tokens = tokens;
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_calls.clear();
        self.tool_results.clear();
        self.total_tokens = 0;
    }
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

fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 / 4) + 1
}
