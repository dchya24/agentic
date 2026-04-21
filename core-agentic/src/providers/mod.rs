//! LLM Provider trait and implementations

pub mod openai;

pub use openai::{OpenAIProvider, OpenAIProviderConfig};

use serde::{Deserialize, Serialize};

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

#[derive(Debug, thiserror::Error)]
#[error("Provider error: {0}")]
pub struct ProviderError(pub String);

impl ProviderError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRequest {
    pub role: String,
    pub content: String,
}

impl ChatMessageRequest {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessageRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessageRequest>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            stream: false,
        }
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn stream(mut self) -> Self {
        self.stream = true;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageResponse {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub message: ChatMessageResponse,
    pub finish_reason: Option<String>,
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChunk {
    pub id: String,
    pub delta: String,
    pub finish_reason: Option<String>,
}

pub type StreamResult<T, E> = std::result::Result<std::pin::Pin<Box<dyn futures::Stream<Item = std::result::Result<T, E>> + Send + Sync>>, E>;

pub trait LLMProvider: Send + Sync {
    fn provider_type(&self) -> &str;
    fn provider_id(&self) -> &str;
    
    fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse>;
    
    fn chat_stream(
        &self, 
        request: ChatRequest,
    ) -> StreamResult<ChatChunk, ProviderError>;
}