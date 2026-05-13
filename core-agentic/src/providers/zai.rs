//! Z.ai provider implementation
//!
//! Z.ai uses an OpenAI-compatible API, so this wraps OpenAIProvider
//! with Z.ai-specific defaults (base URL, model names).

use serde::{Deserialize, Serialize};

use super::{
    ChatChunk, ChatRequest, ChatResponse, LLMProvider, ModelCapability, ModelInfo,
    ProviderError, ProviderResult, StreamResult,
};

/// Configuration for the Z.ai provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZaiProviderConfig {
    pub id: String,
    pub name: String,
    pub api_key: String,
    pub default_model: String,
    #[serde(default = "zai_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<ZaiModelConfig>,
}

fn zai_base_url() -> String {
    "https://api.z.ai/v1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZaiModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context_window: Option<u32>,
}

impl ZaiProviderConfig {
    pub fn new(api_key: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            id: "zai".to_string(),
            name: "Z.ai".to_string(),
            api_key: api_key.into(),
            default_model: default_model.into(),
            base_url: zai_base_url(),
            models: vec![],
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
}

/// Z.ai provider — uses the OpenAI-compatible chat completions API.
pub struct ZaiProvider {
    config: ZaiProviderConfig,
    client: reqwest::blocking::Client,
    async_client: reqwest::Client,
}

impl ZaiProvider {
    pub fn new(config: ZaiProviderConfig) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        let async_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build async HTTP client");

        Self {
            config,
            client,
            async_client,
        }
    }

    fn build_request_body(&self, request: &ChatRequest, stream: bool) -> serde_json::Value {
        let system_content = request.effective_system_prompt().to_string();

        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": system_content
        })];

        for msg in &request.messages {
            messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": stream,
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

        body
    }
}

impl LLMProvider for ZaiProvider {
    fn provider_type(&self) -> &str {
        "zai"
    }

    fn provider_id(&self) -> &str {
        &self.config.id
    }

    fn chat(&self, request: ChatRequest) -> ProviderResult<ChatResponse> {
        // Delegate to OpenAI-compatible endpoint
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let body = self.build_request_body(&request, false);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ProviderError::new(format!("Z.ai request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(ProviderError::new(format!(
                "Z.ai API error ({}): {}",
                status, text
            )));
        }

        // Parse OpenAI-compatible response
        #[derive(Deserialize)]
        struct ZaiChoice {
            message: ZaiMessage,
            finish_reason: Option<String>,
        }
        #[derive(Deserialize)]
        struct ZaiMessage {
            role: String,
            content: Option<String>,
        }
        #[derive(Deserialize)]
        struct ZaiUsage {
            #[allow(dead_code)]
            prompt_tokens: Option<u32>,
            #[allow(dead_code)]
            completion_tokens: Option<u32>,
            #[allow(dead_code)]
            total_tokens: Option<u32>,
        }
        #[derive(Deserialize)]
        struct ZaiResponse {
            id: String,
            model: String,
            choices: Vec<ZaiChoice>,
            usage: Option<ZaiUsage>,
        }

        let zai_resp: ZaiResponse = response
            .json()
            .map_err(|e| ProviderError::new(format!("Failed to parse Z.ai response: {}", e)))?;

        let choice = zai_resp
            .choices
            .first()
            .ok_or_else(|| ProviderError::new("No choices in Z.ai response"))?;

        Ok(ChatResponse {
            id: zai_resp.id,
            model: zai_resp.model,
            message: super::ChatMessageResponse {
                role: choice.message.role.clone(),
                content: choice.message.content.clone(),
                tool_calls: vec![],
            },
            finish_reason: choice.finish_reason.clone(),
            usage: zai_resp.usage.map(|u| super::ChatUsage {
                prompt_tokens: u.prompt_tokens.unwrap_or(0),
                completion_tokens: u.completion_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
            }),
        })
    }

    fn chat_stream(&self, request: ChatRequest) -> StreamResult<ChatChunk, ProviderError> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let body = self.build_request_body(&request, true);
        let api_key = self.config.api_key.clone();

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.async_client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ProviderError::new(format!("Z.ai stream request failed: {}", e)))?
                    .error_for_status()
                    .map_err(|e| ProviderError::new(format!("Z.ai stream API error: {}", e)))
            })
        })?;

        let stream = async_stream::stream! {
            use futures::stream::StreamExt;

            let mut buffer = String::new();
            let mut stream = response.bytes_stream();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].trim().to_string();
                            buffer.drain(..=pos);
                            if let Some(data) = line.strip_prefix("data: ") {
                                let trimmed = data.trim();
                                if trimmed.is_empty() || trimmed == "[DONE]" {
                                    if trimmed == "[DONE]" { return; }
                                    continue;
                                }
                                // Parse OpenAI-compatible SSE chunk
                                #[derive(Deserialize)]
                                struct StreamDelta { content: Option<String> }
                                #[derive(Deserialize)]
                                struct StreamChoice { delta: StreamDelta, finish_reason: Option<String> }
                                #[derive(Deserialize)]
                                struct StreamResp { id: String, choices: Vec<StreamChoice> }

                                if let Ok(resp) = serde_json::from_str::<StreamResp>(trimmed) {
                                    if let Some(choice) = resp.choices.first() {
                                        yield Ok(ChatChunk {
                                            id: resp.id,
                                            delta: choice.delta.content.clone().unwrap_or_default(),
                                            finish_reason: choice.finish_reason.clone(),
                                            tool_calls: vec![],
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(ProviderError::new(format!("Z.ai stream read error: {}", e)));
                        return;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn health_check(&self) -> ProviderResult<bool> {
        let url = format!(
            "{}/models",
            self.config.base_url.trim_end_matches('/')
        );
        let result = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send();
        match result {
            Ok(resp) if resp.status().is_success() => Ok(true),
            Ok(resp) => Err(ProviderError::new(format!(
                "Z.ai health check failed: HTTP {}",
                resp.status()
            ))),
            Err(e) => Err(ProviderError::new(format!(
                "Z.ai health check connection failed: {}",
                e
            ))),
        }
    }

    fn list_models(&self) -> ProviderResult<Vec<ModelInfo>> {
        Ok(self
            .config
            .models
            .iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                context_window: m.context_window,
                capabilities: vec![
                    ModelCapability::Chat,
                    ModelCapability::Streaming,
                ],
            })
            .collect())
    }

    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zai_config_new() {
        let config = ZaiProviderConfig::new("test-key", "zai-model");
        assert_eq!(config.id, "zai");
        assert_eq!(config.name, "Z.ai");
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.default_model, "zai-model");
        assert_eq!(config.base_url, "https://api.z.ai/v1");
    }

    #[test]
    fn test_zai_config_custom_base_url() {
        let config = ZaiProviderConfig::new("key", "model")
            .with_base_url("https://custom.z.ai/v1");
        assert_eq!(config.base_url, "https://custom.z.ai/v1");
    }

    #[test]
    fn test_zai_config_custom_id() {
        let config = ZaiProviderConfig::new("key", "model")
            .with_id("my-zai");
        assert_eq!(config.id, "my-zai");
    }

    #[test]
    fn test_zai_config_serialization_roundtrip() {
        let config = ZaiProviderConfig::new("key", "model");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ZaiProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, config.id);
        assert_eq!(parsed.api_key, config.api_key);
    }

    #[test]
    fn test_zai_provider_new() {
        let config = ZaiProviderConfig::new("key", "model");
        let _provider = ZaiProvider::new(config);
    }

    #[test]
    fn test_zai_provider_type() {
        let config = ZaiProviderConfig::new("key", "model");
        let provider = ZaiProvider::new(config);
        assert_eq!(provider.provider_type(), "zai");
        assert_eq!(provider.provider_id(), "zai");
    }

    #[test]
    fn test_zai_count_tokens() {
        let config = ZaiProviderConfig::new("key", "model");
        let provider = ZaiProvider::new(config);
        assert_eq!(provider.count_tokens("hello world"), 2); // 11 / 4 = 2
    }

    #[test]
    fn test_zai_list_models_empty() {
        let config = ZaiProviderConfig::new("key", "model");
        let provider = ZaiProvider::new(config);
        let models = provider.list_models().unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn test_zai_list_models_configured() {
        let config = ZaiProviderConfig {
            id: "zai".to_string(),
            name: "Z.ai".to_string(),
            api_key: "key".to_string(),
            default_model: "z-1".to_string(),
            base_url: zai_base_url(),
            models: vec![
                ZaiModelConfig {
                    id: "z-1".to_string(),
                    name: "Z-1".to_string(),
                    context_window: Some(128000),
                },
            ],
        };
        let provider = ZaiProvider::new(config);
        let models = provider.list_models().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "z-1");
        assert_eq!(models[0].context_window, Some(128000));
        assert!(models[0].capabilities.contains(&ModelCapability::Chat));
    }
}
