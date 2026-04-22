//! OpenAI-compatible provider implementation

use reqwest::blocking::Client;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};

use super::{ChatRequest, ChatResponse, ChatUsage, LLMProvider, ProviderError, ProviderResult};

pub struct OpenAIProvider {
    config: OpenAIProviderConfig,
    client: Client,
    async_client: reqwest::Client,
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
    #[allow(dead_code)]
    object: String,
    #[allow(dead_code)]
    created: u64,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsageResp>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    #[allow(dead_code)]
    index: u32,
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunction {
    name: String,
    arguments: String,
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

        let async_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build async HTTP client");

        Self { config, client, async_client }
    }

    fn extract_sse_line(buffer: &mut String) -> Option<String> {
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

    fn parse_sse_chunk(data: &str) -> Option<super::ChatChunk> {
        #[derive(Deserialize)]
        struct StreamFunctionDelta {
            name: Option<String>,
            arguments: Option<String>,
        }
        #[derive(Deserialize)]
        struct StreamToolCallDelta {
            index: u32,
            id: Option<String>,
            function: Option<StreamFunctionDelta>,
        }
        #[derive(Deserialize)]
        struct StreamDelta {
            content: Option<String>,
            #[serde(default)]
            tool_calls: Option<Vec<StreamToolCallDelta>>,
        }
        #[derive(Deserialize)]
        struct StreamChoice {
            delta: StreamDelta,
            finish_reason: Option<String>,
        }
        #[derive(Deserialize)]
        struct StreamResponse {
            id: String,
            choices: Vec<StreamChoice>,
        }

        let resp: StreamResponse = serde_json::from_str(data).ok()?;
        let choice = resp.choices.first()?;
        let tool_calls: Vec<super::ToolCallDelta> = choice
            .delta
            .tool_calls
            .as_ref()
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| super::ToolCallDelta {
                        index: tc.index,
                        id: tc.id.clone(),
                        function_name: tc.function.as_ref().and_then(|f| f.name.clone()),
                        function_arguments: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.arguments.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(super::ChatChunk {
            id: resp.id,
            delta: choice.delta.content.clone().unwrap_or_default(),
            finish_reason: choice.finish_reason.clone(),
            tool_calls,
        })
    }

    pub fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        // first message is always system prompt
        let mut messages: Vec<super::ChatMessageRequest> = vec![super::ChatMessageRequest {
            role: "system".into(),
            content: "You are a helpful coding assistant".into(),
            tool_call_id: None,
            tool_calls: vec![],
        }];

        // push the rest of the messages from the request
        messages.extend(request.messages.iter().cloned());

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
        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
        }

        log::info!(
            "Api Key set: {}",
            if self.config.api_key.is_empty() {
                "NO"
            } else {
                "YES"
            }
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ProviderError::new(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(ProviderError::new(format!(
                "API error ({}): {}",
                status, text
            )));
        }

        let oai_response: OpenAIChatResponse = response
            .json()
            .map_err(|e| ProviderError::new(format!("Failed to parse response: {}", e)))?;

        let choice = oai_response
            .choices
            .first()
            .ok_or_else(|| ProviderError::new("No choices in response".to_string()))?;

        let usage = oai_response.usage.map(|u| ChatUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        let tool_calls: Vec<super::ToolCallResponse> = choice
            .message
            .tool_calls
            .as_ref()
            .map(|tc| {
                tc.iter()
                    .map(|t| super::ToolCallResponse {
                        id: t.id.clone(),
                        call_type: t.call_type.clone(),
                        function: super::ToolCallFunction {
                            name: t.function.name.clone(),
                            arguments: t.function.arguments.clone(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ChatResponse {
            id: oai_response.id,
            model: oai_response.model,
            message: super::ChatMessageResponse {
                role: choice.message.role.clone(),
                content: choice.message.content.clone(),
                tool_calls,
            },
            finish_reason: choice.finish_reason.clone(),
            usage,
        })
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
        self.chat(request)
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> super::StreamResult<super::ChatChunk, ProviderError> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let mut messages: Vec<super::ChatMessageRequest> = vec![super::ChatMessageRequest {
            role: "system".into(),
            content: "You are a helpful coding assistant".into(),
            tool_call_id: None,
            tool_calls: vec![],
        }];

        messages.extend(request.messages.iter().cloned());

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }
        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
        }

        let api_key = self.config.api_key.clone();
        let async_client = self.async_client.clone();

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                async_client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ProviderError::new(format!("Stream request failed: {}", e)))?
                    .error_for_status()
                    .map_err(|e| ProviderError::new(format!("Stream API error: {}", e)))
            })
        })?;

        let stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut stream = response.bytes_stream();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(line) = Self::extract_sse_line(&mut buffer) {
                            if line == "[DONE]" {
                                return;
                            }
                            if let Some(chat_chunk) = Self::parse_sse_chunk(&line) {
                                yield Ok(chat_chunk);
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

        Ok(Box::pin(stream))
    }
}
