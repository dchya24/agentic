//! OpenAI-compatible provider implementation

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{LLMProvider, ProviderError, ProviderResult, ChatRequest, ChatResponse, ChatUsage};

pub struct OpenAIProvider {
    config: OpenAIProviderConfig,
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<ModelConfig>,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context_window: Option<u32>,
}

impl OpenAIProviderConfig {
    pub fn new(
        id: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        let id_str = id.into();
        Self {
            id: id_str.clone(),
            name: id_str,
            provider_type: "openai-compatible".into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            models: vec![],
            default_model: default_model.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsageResp>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    index: u32,
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsageResp {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");
        
        Self { config, client }
    }

    pub fn chat_sync(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(self.chat_inner(request))
    }

    pub async fn chat_inner(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        
        let messages: Vec<super::ChatMessageRequest> = request.messages
            .iter()
            .map(|m| super::ChatMessageRequest {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
        });
        
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let runtime = tokio::runtime::Handle::current();
        
        let response = runtime.block_on(async {
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
        }).await.map_err(|e| ProviderError::new(format!("Request failed: {}", e)))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let text = runtime.block_on(response.text()).unwrap_or_default();
            return Err(ProviderError::new(format!("API error ({}): {}", status, text)));
        }

        let oai_response: OpenAIChatResponse = runtime.block_on(response.json())
            .map_err(|e| ProviderError::new(format!("Failed to parse response: {}", e)))?;
        
        let choice = oai_response.choices.first()
            .ok_or_else(|| ProviderError::new("No choices in response".to_string()))?;
        
        let usage = oai_response.usage.map(|u| ChatUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        Ok(ChatResponse {
            id: oai_response.id,
            model: oai_response.model,
            message: super::ChatMessageResponse {
                role: choice.message.role.clone(),
                content: choice.message.content.clone(),
            },
            finish_reason: choice.finish_reason.clone(),
            usage,
        })
    }

    pub fn chat_stream_sync(&self, request: ChatRequest) -> ProviderResult<super::StreamResult<super::ChatChunk, ProviderError>> {
        Err(ProviderError::new("Streaming not implemented yet".to_string()))
    }
}

impl LLMProvider for OpenAIProvider {
    fn provider_type(&self) -> &str {
        &self.config.provider_type
    }

    fn provider_id(&self) -> &str {
        &self.config.id
    }

    fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        self.chat_sync(request)
    }

    fn chat_stream(&self, request: ChatRequest) -> super::StreamResult<super::ChatChunk, ProviderError> {
        self.chat_stream_sync(request)?
    }
}