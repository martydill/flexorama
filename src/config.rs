use crate::openrouter::OpenRouterClient;
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
    Mistral,
    OpenAI,
    #[serde(rename = "z.ai")]
    Zai,
    Ollama,
    OpenRouter,
}

/// Effort level for AI reasoning/thinking
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    /// Minimal reasoning - fastest responses
    Low,
    /// Balanced reasoning (default)
    #[default]
    Medium,
    /// Maximum reasoning - most thorough responses
    High,
}

impl std::str::FromStr for EffortLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(EffortLevel::Low),
            "medium" => Ok(EffortLevel::Medium),
            "high" => Ok(EffortLevel::High),
            _ => Err(format!("Invalid effort level '{}'. Valid values: low, medium, high", s)),
        }
    }
}

impl std::fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffortLevel::Low => write!(f, "low"),
            EffortLevel::Medium => write!(f, "medium"),
            EffortLevel::High => write!(f, "high"),
        }
    }
}

impl EffortLevel {
    /// Returns the reasoning budget token count for Anthropic models
    pub fn anthropic_reasoning_budget(&self) -> Option<u32> {
        match self {
            EffortLevel::Low => None, // No extended thinking
            EffortLevel::Medium => Some(10_000),
            EffortLevel::High => Some(50_000),
        }
    }

    /// Returns the reasoning effort string for OpenAI o1/o3 models
    pub fn openai_reasoning_effort(&self) -> &str {
        match self {
            EffortLevel::Low => "low",
            EffortLevel::Medium => "medium",
            EffortLevel::High => "high",
        }
    }

    /// Returns whether to enable thinking mode for Ollama
    pub fn ollama_think(&self) -> bool {
        matches!(self, EffortLevel::Medium | EffortLevel::High)
    }
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
            "mistral" => Ok(Provider::Mistral),
            "openai" => Ok(Provider::OpenAI),
            "z.ai" | "zai" => Ok(Provider::Zai),
            "ollama" => Ok(Provider::Ollama),
            "openrouter" => Ok(Provider::OpenRouter),
            other => Err(format!("Unsupported provider '{}'", other)),
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::Gemini => write!(f, "gemini"),
            Provider::Mistral => write!(f, "mistral"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::Zai => write!(f, "z.ai"),
            Provider::Ollama => write!(f, "ollama"),
            Provider::OpenRouter => write!(f, "openrouter"),
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
    #[serde(default)]
    pub auth: Option<McpAuthConfig>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpAuthConfig {
    OAuth(McpOAuthConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    #[serde(default)]
    pub token_url: Option<String>,
    #[serde(default)]
    pub authorization_url: Option<String>,
    pub client_id: String,
    /// Client secret - required for client_credentials flow, optional for authorization_code with PKCE
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub extra_params: Option<HashMap<String, String>>,
    #[serde(default)]
    pub client_auth: McpOAuthClientAuth,
    /// OAuth grant type: "authorization_code" (default, with PKCE) or "client_credentials"
    #[serde(default)]
    pub grant_type: McpOAuthGrantType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpOAuthGrantType {
    /// Authorization Code flow with PKCE - requires user interaction
    AuthorizationCode,
    /// Client Credentials flow - machine-to-machine, no user interaction
    ClientCredentials,
}

impl Default for McpOAuthGrantType {
    fn default() -> Self {
        McpOAuthGrantType::AuthorizationCode
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpOAuthClientAuth {
    Basic,
    Body,
}

impl Default for McpOAuthClientAuth {
    fn default() -> Self {
        McpOAuthClientAuth::Body
    }
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
    #[serde(default)]
    pub effort: EffortLevel,
}
const DEFAULT_SYSTEM_PROMPT: &str = r#"
You are an expert in software development. Your job is to help the user build awesome software.

Everything you do must follow all best practices for architecture, design, security, and performance.

Whenever you generate code, you must make sure it compiles properly by running any available linter or compiler.

Generate a chain of thought, explaining your reasoning step-by-step before giving the final answer. Think deeply about what steps are required to proceed and tell me what they are.

When making tool calls, you must explain why you are making them, and what you hope to accomplish.

You MUST create a list of todos for each task you want to accomplish. Do not start writing code until you have created a todo list.
You must call the create_todo tool call to create each item in the todo list before starting your work.
You must call the complete_todo tool call after completing an item in your tool list.
"#;

pub fn provider_default_api_key(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_AUTH_TOKEN").unwrap_or_default(),
        Provider::Gemini => std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default(),
        Provider::Mistral => std::env::var("MISTRAL_API_KEY").unwrap_or_default(),
        Provider::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        Provider::Zai => std::env::var("ZAI_API_KEY").unwrap_or_default(),
        Provider::Ollama => std::env::var("OLLAMA_API_KEY").unwrap_or_default(),
        Provider::OpenRouter => std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
    }
}

pub fn provider_default_base_url(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string()),
        Provider::Gemini => std::env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        Provider::Mistral => std::env::var("MISTRAL_BASE_URL")
            .unwrap_or_else(|_| "https://api.mistral.ai/v1".to_string()),
        Provider::OpenAI => std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        Provider::Zai => std::env::var("ZAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.z.ai/api/anthropic".to_string()),
        Provider::Ollama => std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string()),
        Provider::OpenRouter => std::env::var("OPENROUTER_BASE_URL")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string()),
    }
}

pub fn provider_default_model(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => "claude-opus-5".to_string(),
        Provider::Gemini => "gemini-flash-latest".to_string(),
        Provider::Mistral => "mistral-large-latest".to_string(),
        Provider::OpenAI => "gpt-5.6-sol".to_string(),
        Provider::Zai => "glm-5.3".to_string(),
        Provider::Ollama => "llama2".to_string(),
        Provider::OpenRouter => "anthropic/claude-opus-5".to_string(),
    }
}

pub fn provider_models(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Anthropic => &[
            "claude-fable-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-opus-4-5",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
        ],
        Provider::Gemini => &[
            "gemini-flash-latest",
            "gemini-3.7-flash",
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-3.5-flash-lite",
            "gemini-3.1-pro-preview",
            "gemini-3.1-flash-lite",
            "gemini-3-pro-image",
            "gemini-3-flash-preview",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
        ],
        Provider::Mistral => &[
            "mistral-large-latest",
            "mistral-large-2512",
            "mistral-medium-2604",
            "mistral-small-2603",
            "codestral-2508",
            "ministral-14b-2512",
            "ministral-8b-2512",
            "ministral-3b-2512",
        ],
        Provider::OpenAI => &[
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.6-cyber",
            "gpt-5.1-codex-max",
            "gpt-5.1",
            "gpt-5.1-codex",
            "gpt-5-pro",
            "gpt-5",
            "gpt-5-mini",
            "gpt-5-nano",
            "o4-mini",
            "o3",
            "gpt-4o",
            "gpt-4o-mini",
        ],
        Provider::Zai => &["glm-5.3", "glm-5.2", "glm-5.1", "glm-5", "glm-4.7", "glm-4.6", "glm-4.5"],
        Provider::Ollama => &["llama2", "gemma3:1b"],
        Provider::OpenRouter => &[
            "anthropic/claude-opus-5",
            "anthropic/claude-sonnet-5",
            "openai/gpt-5.6-sol",
            "google/gemini-2.5-pro",
            "meta-llama/llama-4-matrix",
            "mistralai/mistral-large",
            "openai/gpt-4o",
            "openai/gpt-4o-mini",
            "google/gemini-flash-latest",
        ],
    }
}

/// Fetch available models from OpenRouter API
/// Returns dynamic list if successful, otherwise returns fallback models
/// Fetch available models from OpenRouter API
/// Returns dynamic list if successful, otherwise returns fallback models
pub async fn fetch_openrouter_models(api_key: &str, base_url: &str) -> Vec<String> {
    let client = OpenRouterClient::new(api_key.to_string(), base_url.to_string());

    match client.fetch_models().await {
        Ok(models) => {
            info!("Successfully fetched {} models from OpenRouter", models.len());
            models
        }
        Err(e) => {
            info!("Failed to fetch OpenRouter models: {}. Using fallback models.", e);
            OpenRouterClient::fallback_models()
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
    }
}

/// Get available models for a provider
/// For OpenRouter, attempts to fetch dynamic models if API key is available
/// For other providers, returns static model list
pub async fn get_provider_models(provider: Provider) -> Vec<String> {
    if provider == Provider::OpenRouter {
        let api_key = provider_default_api_key(provider);
        let base_url = provider_default_base_url(provider);

        if !api_key.is_empty() {
            fetch_openrouter_models(&api_key, &base_url).await
        } else {
            OpenRouterClient::fallback_models()
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
    } else {
        provider_models(provider)
            .iter()
            .map(|s| s.to_string())
            .collect()
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
            effort: EffortLevel::default(),
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

    /// Get the config file path for display purposes
    pub fn path(&self) -> PathBuf {
        Self::default_config_path()
    }
}
