use crate::security::{BashSecurity, FileSecurity};
use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Gemini,
    OpenAI,
    #[serde(rename = "z.ai")]
    Zai,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Anthropic
    }
}

impl std::str::FromStr for Provider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(Provider::Anthropic),
            "gemini" => Ok(Provider::Gemini),
            "openai" => Ok(Provider::OpenAI),
            "z.ai" | "zai" => Ok(Provider::Zai),
            other => Err(format!("Unsupported provider '{}'", other)),
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::Gemini => write!(f, "gemini"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::Zai => write!(f, "z.ai"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub servers: HashMap<String, McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub deactivated_skills: Vec<String>,
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            active_skills: Vec::new(),
            deactivated_skills: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip)]
    pub api_key: String,
    #[serde(default)]
    pub provider: Provider,
    pub base_url: String,
    pub default_model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub default_system_prompt: Option<String>,
    pub bash_security: BashSecurity,
    #[serde(skip)]
    pub file_security: FileSecurity,
    pub mcp: McpConfig,
    #[serde(default)]
    pub skills: SkillConfig,
}
const DEFAULT_SYSTEM_PROMPT: &str = r#"
You are an expert in software development. Your job is to help the user build awesome software.

Everything you do must follow all best practices for architecture, design, security, and performance.

Whenever you generate code, you must make sure it compiles properly by running any available linter or compiler.

Generate a chain of thought, explaining your reasoning step-by-step before giving the final answer. Think deeply about what steps are required to proceed and tell me what they are.

When making tool calls, you must explain why you are making them, and what you hope to accomplish.
"#;

pub fn provider_default_api_key(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_AUTH_TOKEN").unwrap_or_default(),
        Provider::Gemini => std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default(),
        Provider::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        Provider::Zai => std::env::var("ZAI_API_KEY").unwrap_or_default(),
    }
}

pub fn provider_default_base_url(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string()),
        Provider::Gemini => std::env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        Provider::OpenAI => std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        Provider::Zai => std::env::var("ZAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.z.ai/api/anthropic".to_string()),
    }
}

pub fn provider_default_model(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => "claude-sonnet-4-5".to_string(),
        Provider::Gemini => "gemini-flash-latest".to_string(),
        Provider::OpenAI => "gpt-5.2".to_string(),
        Provider::Zai => "glm-4.7".to_string(),
    }
}

pub fn provider_models(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Anthropic => &[
            "claude-opus-4-1",
            "claude-sonnet-4-0",
            "claude-3-7-sonnet-latest",
            "claude-opus-4-0",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "claude-opus-4-5",
        ],
        Provider::Gemini => &[
            "gemini-flash-latest",
            "gemini-1.5-pro",
            "gemini-1.5-flash",
            "gemini-2.5-flash",
            "gemini-3-flash-preview",
            "gemini-2.5-flash-lite",
            "gemini-2.5-pro",
        ],
        Provider::OpenAI => &[
            "gpt-5.2",
            "gpt-5.1-codex-max",
            "gpt-5.1",
            "gpt-5.1-chat",
            "gpt-5.1-codex",
            "gpt-5.1-codex-mini",
            "gpt-5-pro",
            "gpt-5-codex",
            "gpt-5",
            "gpt-5-mini",
            "gpt-5-nano",
            "o3-pro",
            "codex-mini",
            "o4-mini",
            "o3",
            "o3-mini",
            "o1",
            "o1-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4",
            "gpt-3.5-turbo",
        ],
        Provider::Zai => &["glm-4.7", "glm-4.6", "glm-4.5"],
    }
}

impl Default for Config {
    fn default() -> Self {
        let provider = Provider::default();
        Self {
            api_key: provider_default_api_key(provider),
            provider,
            base_url: provider_default_base_url(provider),
            default_model: provider_default_model(provider),
            max_tokens: 4096,
            temperature: 0.7,
            default_system_prompt: DEFAULT_SYSTEM_PROMPT.to_string().into(),
            bash_security: BashSecurity::default(),
            file_security: FileSecurity::default(),
            mcp: McpConfig::default(),
            skills: SkillConfig::default(),
        }
    }
}

impl Config {
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("flexorama")
            .join("config.toml")
    }

    /// Load configuration from file and merge with environment variables
    pub async fn load(path: Option<&str>) -> Result<Self> {
        let config_path = path
            .map(PathBuf::from)
            .unwrap_or_else(Self::default_config_path);

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            let mut config: Config = toml::from_str(&content)?;

            // Ensure API key is never loaded from config file
            if !config.api_key.is_empty() {
                info!("API key found in config file - ignoring for security. Use environment variables or command line.");
                config.api_key = String::new();
            }

            config
        } else {
            info!(
                "No config file found at {}, using defaults",
                config_path.display()
            );
            Config::default()
        };

        // Ensure provider defaults when missing from older config files
        if config.base_url.is_empty() {
            config.base_url = provider_default_base_url(config.provider);
        }
        if config.default_model.is_empty() {
            config.default_model = provider_default_model(config.provider);
        }

        // Always prioritize environment variables for API key based on provider
        config.api_key = provider_default_api_key(config.provider);

        Ok(config)
    }

    /// Save configuration to file (without API key)
    pub async fn save(&self, path: Option<&str>) -> Result<()> {
        let config_path = path
            .map(PathBuf::from)
            .unwrap_or_else(Self::default_config_path);

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Create a copy of the config without the API key for saving
        let mut config_for_save = self.clone();
        config_for_save.api_key = String::new(); // Clear API key before saving

        let content = toml::to_string_pretty(&config_for_save)?;
        fs::write(&config_path, content).await?;
        info!(
            "Configuration saved to: {} (API key excluded for security)",
            config_path.display()
        );
        Ok(())
    }

    /// Update provider and refresh provider-specific defaults
    pub fn set_provider(&mut self, provider: Provider) {
        self.provider = provider;
        self.base_url = provider_default_base_url(provider);
        self.default_model = provider_default_model(provider);
        self.api_key = provider_default_api_key(provider);
    }
}
