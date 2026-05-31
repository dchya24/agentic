//! Anthropic Claude provider implementation

use reqwest::blocking::Client;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};

use super::{ChatRequest, ChatResponse, ChatUsage, LLMProvider, ProviderError, ProviderResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
}

fn default_max_retries() -> u32 { 3 }
fn default_base_delay_ms() -> u64 { 1000 }
fn default_max_delay_ms() -> u64 { 30000 }

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            base_delay_ms: default_base_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
        }
    }
}

impl RetryConfig {
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let exp = 2u64.saturating_pow(attempt);
        let delay = self.base_delay_ms.saturating_mul(exp);
        std::time::Duration::from_millis(delay.min(self.max_delay_ms))
    }
}

pub struct AnthropicProvider {
    config: AnthropicProviderConfig,
    client: Client,
    async_client: reqwest::Client,
    retry: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<ModelConfig>,
    pub default_model: String,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context_window: Option<u32>,
}

impl AnthropicProviderConfig {
    pub fn new(
        id: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        let id_str = id.into();
        Self {
            id: id_str.clone(),
            name: id_str,
            provider_type: "anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api_key: api_key.into(),
            models: vec![],
            default_model: default_model.into(),
            retry: RetryConfig::default(),
            version: "2023-06-01".into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }
}

// Anthropic API request types
//
// Note: structs for the legacy non-untagged content shape (AnthropicMessage,
// AnthropicToolUse, AnthropicToolResult) were removed when we switched to
// the untagged `AnthropicContentBlock` enum. Restore from git history if a
// future change needs the named-struct form.

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum AnthropicContentBlock {
    Text { text: String },
    ToolUse {
        #[serde(rename = "type")]
        tool_use_type: String,
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        #[serde(rename = "type")]
        tool_result_type: String,
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessageRequest {
    role: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessageRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicToolDefinition {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

// Anthropic API response types

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[allow(dead_code)]
    pub r#type: String,
    pub role: String,
    pub content: Vec<AnthropicContentBlockResponse>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContentBlockResponse {
    Text { text: String },
    ToolUse {
        #[serde(rename = "type")]
        tool_use_type: String,
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicError {
    #[allow(dead_code)]
    pub r#type: String,
    pub error: AnthropicErrorDetail,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicErrorDetail {
    #[allow(dead_code)]
    pub r#type: String,
    pub message: String,
}

// Streaming response types

#[derive(Debug, Deserialize)]
pub struct AnthropicStreamEvent {
    #[allow(dead_code)]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<AnthropicStreamDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<AnthropicStreamMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AnthropicErrorDetail>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicStreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub model: String,
    pub role: String,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        let async_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build async HTTP client");

        let retry = config.retry.clone();
        Self { config, client, async_client, retry }
    }

    fn convert_request(&self, request: ChatRequest) -> Result<AnthropicRequest, ProviderError> {
        let mut system_message = None;
        let mut anthropic_messages = Vec::new();

        for msg in &request.messages {
            match msg.role.as_str() {
                "system" => {
                    system_message = Some(msg.content.clone());
                }
                "user" | "assistant" => {
                    let content_blocks = if !msg.tool_calls.is_empty() {
                        // Convert tool calls to Anthropic format
                        let mut blocks = Vec::new();
                        for tc in &msg.tool_calls {
                            blocks.push(AnthropicContentBlock::ToolUse {
                                tool_use_type: "tool_use".into(),
                                id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                input: serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                            });
                        }
                        if !msg.content.is_empty() {
                            blocks.push(AnthropicContentBlock::Text {
                                text: msg.content.clone(),
                            });
                        }
                        blocks
                    } else if msg.tool_call_id.is_some() {
                        // Tool result
                        vec![AnthropicContentBlock::ToolResult {
                            tool_result_type: "tool_result".into(),
                            tool_use_id: msg.tool_call_id.clone().unwrap_or_default(),
                            content: msg.content.clone(),
                            is_error: None,
                        }]
                    } else {
                        vec![AnthropicContentBlock::Text {
                            text: msg.content.clone(),
                        }]
                    };

                    anthropic_messages.push(AnthropicMessageRequest {
                        role: msg.role.clone(),
                        content: content_blocks,
                    });
                }
                _ => {
                    // Skip unknown roles
                }
            }
        }

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .into_iter()
                    .map(|t| AnthropicToolDefinition {
                        tool_type: t.tool_type,
                        name: t.function.name,
                        description: t.function.description,
                        input_schema: t.function.parameters,
                    })
                    .collect(),
            )
        };

        Ok(AnthropicRequest {
            model: request.model,
            system: system_message,
            messages: anthropic_messages,
            tools,
            temperature: request.temperature,
            top_p: None,
            top_k: None,
            max_tokens: request.max_tokens.unwrap_or(4096),
            stream: false,
        })
    }

    fn is_retryable_error(error: &ProviderError) -> bool {
        let msg = error.0.to_lowercase();
        msg.contains("timeout")
            || msg.contains("connection")
            || msg.contains("429")
            || msg.contains("500")
            || msg.contains("502")
            || msg.contains("503")
            || msg.contains("504")
            || msg.contains("rate limit")
    }

    pub fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        let anthropic_request = self.convert_request(request)?;

        let mut last_error = None;

        for attempt in 0..=self.retry.max_retries {
            if attempt > 0 {
                let delay = self.retry.delay_for_attempt(attempt - 1);
                log::warn!("Retry attempt {}/{} after {:?}", attempt, self.retry.max_retries, delay);
                std::thread::sleep(delay);
            }

            match self.chat_once(&anthropic_request) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let retryable = Self::is_retryable_error(&e);
                    last_error = Some(e);
                    if !retryable || attempt == self.retry.max_retries {
                        break;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    fn chat_once(&self, request: &AnthropicRequest) -> ProviderResult<ChatResponse> {
        let url = format!("{}/messages", self.config.base_url.trim_end_matches('/'));

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.version)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .map_err(|e| ProviderError::new(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            
            // Try to parse as Anthropic error
            if let Ok(anthropic_err) = serde_json::from_str::<AnthropicError>(&text) {
                return Err(ProviderError::new(format!(
                    "API error ({}): {}",
                    status, anthropic_err.error.message
                )));
            }
            
            return Err(ProviderError::new(format!(
                "API error ({}): {}",
                status, text
            )));
        }

        let anthropic_response: AnthropicResponse = response
            .json()
            .map_err(|e| ProviderError::new(format!("Failed to parse response: {}", e)))?;

        let (content, tool_calls) = self.extract_content_and_tool_calls(&anthropic_response.content);

        let usage = anthropic_response.usage.map(|u| ChatUsage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
        });

        Ok(ChatResponse {
            id: anthropic_response.id,
            model: anthropic_response.model,
            message: super::ChatMessageResponse {
                role: anthropic_response.role,
                content: Some(content),
                tool_calls,
            },
            finish_reason: anthropic_response.stop_reason,
            usage,
        })
    }

    fn extract_content_and_tool_calls(
        &self,
        content_blocks: &[AnthropicContentBlockResponse],
    ) -> (String, Vec<super::ToolCallResponse>) {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content_blocks {
            match block {
                AnthropicContentBlockResponse::Text { text } => {
                    text_parts.push(text.clone());
                }
                AnthropicContentBlockResponse::ToolUse {
                    tool_use_type,
                    id,
                    name,
                    input,
                } => {
                    tool_calls.push(super::ToolCallResponse {
                        id: id.clone(),
                        call_type: tool_use_type.clone(),
                        function: super::ToolCallFunction {
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        },
                    });
                }
            }
        }

        (text_parts.join(""), tool_calls)
    }

    fn parse_sse_line(buffer: &mut String) -> Option<String> {
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if let Some(data) = line.strip_prefix("data: ") {
                let trimmed = data.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        None
    }
}

impl LLMProvider for AnthropicProvider {
    fn provider_type(&self) -> &str {
        &self.config.provider_type
    }

    fn provider_id(&self) -> &str {
        &self.config.id
    }

    fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        self.chat(request)
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> super::StreamResult<super::ChatChunk, ProviderError> {
        let url = format!("{}/messages", self.config.base_url.trim_end_matches('/'));

        let mut anthropic_request = self.convert_request(request)?;
        anthropic_request.stream = true;

        let api_key = self.config.api_key.clone();
        let version = self.config.version.clone();
        let async_client = self.async_client.clone();
        let retry = self.retry.clone();

        let mut last_error = None;

        for attempt in 0..=retry.max_retries {
            if attempt > 0 {
                let delay = retry.delay_for_attempt(attempt - 1);
                log::warn!("Stream retry attempt {}/{} after {:?}", attempt, retry.max_retries, delay);
                std::thread::sleep(delay);
            }

            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    async_client
                        .post(&url)
                        .header("x-api-key", &api_key)
                        .header("anthropic-version", &version)
                        .header("Content-Type", "application/json")
                        .json(&anthropic_request)
                        .send()
                        .await
                        .map_err(|e| ProviderError::new(format!("Stream request failed: {}", e)))?
                        .error_for_status()
                        .map_err(|e| ProviderError::new(format!("Stream API error: {}", e)))
                })
            });

            match result {
                Ok(response) => {
                    let stream = async_stream::stream! {
                        let mut buffer = String::new();
                        let mut stream = response.bytes_stream();
                        let mut response_id = String::new();

                        while let Some(chunk_result) = stream.next().await {
                            match chunk_result {
                                Ok(bytes) => {
                                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                                    
                                    while let Some(line) = Self::parse_sse_line(&mut buffer) {
                                        match serde_json::from_str::<AnthropicStreamEvent>(&line) {
                                            Ok(event) => {
                                                if let Some(error) = event.error {
                                                    yield Err(ProviderError::new(format!(
                                                        "Stream error: {}", error.message
                                                    )));
                                                    return;
                                                }

                                                if let Some(msg) = event.message {
                                                    response_id = msg.id;
                                                }

                                                if let Some(delta) = event.delta {
                                                    yield Ok(super::ChatChunk {
                                                        id: response_id.clone(),
                                                        delta: delta.text.unwrap_or_default(),
                                                        finish_reason: None,
                                                        tool_calls: vec![],
                                                    });
                                                }

                                                if event.r#type == "content_block_stop" {
                                                    yield Ok(super::ChatChunk {
                                                        id: response_id.clone(),
                                                        delta: String::new(),
                                                        finish_reason: None,
                                                        tool_calls: vec![],
                                                    });
                                                }

                                                if event.r#type == "message_stop" {
                                                    return;
                                                }
                                            }
                                            Err(e) => {
                                                // Ignore parse errors for non-event lines
                                                log::trace!("Failed to parse SSE line: {}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    yield Err(ProviderError::new(format!("Stream read error: {}", e)));
                                    return;
                                }
                            }
                        }
                    };

                    return Ok(Box::pin(stream));
                }
                Err(e) => {
                    let retryable = Self::is_retryable_error(&e);
                    last_error = Some(e);
                    if !retryable || attempt == retry.max_retries {
                        break;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    fn health_check(&self) -> super::ProviderResult<bool> {
        // Anthropic doesn't have a dedicated health endpoint.
        // We just check that we can reach the API base URL.
        let url = self.config.base_url.trim_end_matches('/').to_string();
        let result = self
            .client
            .get(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.version)
            .send();
        match result {
            Ok(_) => Ok(true), // Any response means the API is reachable
            Err(e) => Err(super::ProviderError::new(format!(
                "Health check connection failed: {}",
                e
            ))),
        }
    }

    fn list_models(&self) -> super::ProviderResult<Vec<super::ModelInfo>> {
        // Return the statically configured models from config
        Ok(self
            .config
            .models
            .iter()
            .map(|m| super::ModelInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                context_window: m.context_window,
                capabilities: vec![
                    super::ModelCapability::Chat,
                    super::ModelCapability::Streaming,
                    super::ModelCapability::ToolCalling,
                ],
            })
            .collect())
    }

    fn count_tokens(&self, text: &str) -> usize {
        // Claude uses a different tokenizer, but ~3.5 chars per token is reasonable
        (text.len() as f32 / 3.5) as usize
    }
}
