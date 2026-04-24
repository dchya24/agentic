use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub mcp_servers: std::collections::HashMap<String, crate::mcp::types::McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub api_base: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    8192
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_auto_approve")]
    pub auto_approve_low_risk: bool,
    #[serde(default)]
    pub blocked_commands: Vec<String>,
}

fn default_auto_approve() -> bool {
    true
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            auto_approve_low_risk: true,
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "mkfs".to_string(),
                "dd if=".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_true")]
    pub color: bool,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(default = "default_true")]
    pub show_thoughts: bool,
    #[serde(default = "default_true")]
    pub show_tool_calls: bool,
}

fn default_true() -> bool {
    true
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            color: true,
            stream: true,
            show_thoughts: true,
            show_tool_calls: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyCliConfig {
    provider: LegacyProviderConfig,
    model: LegacyModelConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyModelConfig {
    pub id: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacySingleProvider {
    provider_type: String,
    api_base: String,
    api_key: String,
    model: String,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    tools_enabled: Option<Vec<String>>,
}

impl Config {
    pub fn config_path() -> PathBuf {
        let home = if cfg!(windows) {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        } else {
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        };

        home.join(".config").join("agentic").join("config.json")
    }

    pub fn load() -> Option<Self> {
        Self::load_from(Self::config_path())
    }

    pub fn load_from_path(path: &str) -> Option<Self> {
        Self::load_from(PathBuf::from(path))
    }

    pub fn load_from(path: PathBuf) -> Option<Self> {
        log::info!("Loading config from: {}", path.display());

        if !path.exists() {
            log::info!("Config file not found at: {}", path.display());
            return None;
        }

        let content = std::fs::read_to_string(&path).ok()?;

        let mut config = Self::try_parse_unified(&content)
            .or_else(|| Self::try_parse_legacy_cli(&content))
            .or_else(|| Self::try_parse_legacy_single(&content))?;

        config.apply_env_substitution();

        log::info!("Loaded config with {} provider(s)", config.providers.len());
        Some(config)
    }

    fn try_parse_unified(content: &str) -> Option<Self> {
        serde_json::from_str::<Config>(content).ok()
    }

    fn try_parse_legacy_cli(content: &str) -> Option<Self> {
        let legacy: LegacyCliConfig = serde_json::from_str(content).ok()?;

        Some(Config {
            providers: vec![ProviderConfig {
                name: "default".to_string(),
                provider_type: legacy.provider.provider_type,
                api_base: legacy.provider.base_url,
                api_key: legacy.provider.api_key,
                models: vec![ModelConfig {
                    model: legacy.model.id,
                    display_name: None,
                    temperature: legacy.model.temperature.unwrap_or(0.7),
                    max_tokens: legacy.model.max_tokens.unwrap_or(8192),
                }],
            }],
            safety: SafetyConfig::default(),
            output: OutputConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
        })
    }

    fn try_parse_legacy_single(content: &str) -> Option<Self> {
        let legacy: LegacySingleProvider = serde_json::from_str(content).ok()?;

        Some(Config {
            providers: vec![ProviderConfig {
                name: "default".to_string(),
                provider_type: legacy.provider_type,
                api_base: legacy.api_base,
                api_key: legacy.api_key,
                models: vec![ModelConfig {
                    model: legacy.model,
                    display_name: None,
                    temperature: legacy.temperature.unwrap_or(0.7),
                    max_tokens: legacy.max_tokens.unwrap_or(8192),
                }],
            }],
            safety: SafetyConfig::default(),
            output: OutputConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
        })
    }

    fn apply_env_substitution(&mut self) {
        for provider in &mut self.providers {
            if provider.api_key.starts_with('$') {
                let var = &provider.api_key[1..];
                if let Ok(value) = std::env::var(var) {
                    provider.api_key = value;
                }
            }
            if provider.api_base.starts_with('$') {
                let var = &provider.api_base[1..];
                if let Ok(value) = std::env::var(var) {
                    provider.api_base = value;
                }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config file: {}", e))
    }

    pub fn fallback() -> Self {
        Self {
            providers: vec![ProviderConfig {
                name: "z.ai".to_string(),
                provider_type: "openai-compatible".to_string(),
                api_base: std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.z.ai/api/coding/paas/v4".to_string()),
                api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                models: vec![ModelConfig {
                    model: "glm-4.7".to_string(),
                    display_name: Some("GLM-4.7".to_string()),
                    temperature: 0.7,
                    max_tokens: 8192,
                }],
            }],
            safety: SafetyConfig::default(),
            output: OutputConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
        }
    }

    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.providers.first()
    }

    pub fn active_model(&self) -> Option<&ModelConfig> {
        self.providers.first().and_then(|p| p.models.first())
    }

    pub fn to_output_mapping(&self) -> Vec<ModelOutput> {
        self.providers
            .iter()
            .flat_map(|provider| {
                provider.models.iter().map(|model| {
                    let display = model
                        .display_name
                        .clone()
                        .unwrap_or_else(|| normalize_model_name(&model.model));
                    ModelOutput {
                        name: format!("{} ({})", display, provider.name),
                        model: model.model.clone(),
                    }
                })
            })
            .collect()
    }

    pub fn to_provider_config(&self) -> Option<super::providers::OpenAIProviderConfig> {
        let provider = self.providers.first()?;
        let model = provider.models.first()?;

        Some(super::providers::OpenAIProviderConfig::new(
            &provider.name,
            &provider.api_base,
            &provider.api_key,
            &model.model,
        ))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::load().unwrap_or_else(Self::fallback)
    }
}

fn normalize_model_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOutput {
    pub name: String,
    pub model: String,
}
