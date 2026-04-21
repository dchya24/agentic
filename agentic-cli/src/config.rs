use anyhow::{Context, Result};
use core_agentic::providers::OpenAIProviderConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: ProviderConfig,
    pub model: ModelConfig,
    pub safety: SafetyConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub auto_approve_low_risk: bool,
    pub blocked_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub color: bool,
    pub stream: bool,
    pub show_thoughts: bool,
    pub show_tool_calls: bool,
}

impl Config {
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("agentic")
            .join("config.json")
    }

    pub fn load(path: &str) -> Result<Self> {
        let path = PathBuf::from(path);

        if !path.exists() {
            return Err(anyhow::anyhow!("Config file not found: {}", path.display()));
        }

        let content = std::fs::read_to_string(&path).context("Failed to read config file")?;

        let mut config: Config =
            serde_json::from_str(&content).context("Failed to parse config file")?;

        config.apply_env_substitution()?;

        Ok(config)
    }

    pub fn default() -> Result<Self> {
        let default_path = Self::default_path();

        if default_path.exists() {
            return Self::load(default_path.to_str().unwrap_or_default());
        }

        let provider = ProviderConfig {
            provider_type: "openai-compatible".into(),
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".into()),
        };

        let model = ModelConfig {
            id: "glm-4.7".into(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };

        let safety = SafetyConfig {
            auto_approve_low_risk: true,
            blocked_commands: vec!["rm -rf /".into(), "mkfs".into(), "dd if=".into()],
        };

        let output = OutputConfig {
            color: true,
            stream: true,
            show_thoughts: true,
            show_tool_calls: true,
        };

        Ok(Config {
            provider,
            model,
            safety,
            output,
        })
    }

    fn apply_env_substitution(&mut self) -> Result<()> {
        if self.provider.api_key.starts_with('$') {
            let var = &self.provider.api_key[1..];
            let value = std::env::var(var)
                .map_err(|_| anyhow::anyhow!("Environment variable not found: {}", var))?;
            self.provider.api_key = value;
        }

        if self.provider.base_url.starts_with('$') {
            let var = &self.provider.base_url[1..];
            let value = std::env::var(var)
                .map_err(|_| anyhow::anyhow!("Environment variable not found: {}", var))?;
            self.provider.base_url = value;
        }

        Ok(())
    }

    pub fn to_provider_config(&self) -> OpenAIProviderConfig {
        OpenAIProviderConfig::new(
            &self.provider.provider_type,
            &self.provider.base_url,
            &self.provider.api_key,
            &self.model.id,
        )
    }
}
