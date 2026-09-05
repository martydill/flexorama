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
        Provider::Mistral => "mistral-medium-latest".to_string(),
        Provider::OpenAI => "gpt-6-astra".to_string(),
        Provider::Zai => "glm-5.3".to_string(),
        Provider::Ollama => "qwen3:8b".to_string(),
        Provider::OpenRouter => "anthropic/claude-opus-5".to_string(),
    }
}

pub fn provider_models(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Anthropic => &[
            "claude-fable-5-1",
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
            "gemini-3.8-flash",
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
            "mistral-medium-latest",
            "mistral-medium-3-5",
            "mistral-medium-2604",
            "mistral-large-latest",
            "mistral-large-2512",
            "mistral-small-2603",
            "codestral-2508",
            "ministral-14b-2512",
            "ministral-8b-2512",
            "ministral-3b-2512",
        ],
        Provider::OpenAI => &[
            "gpt-6-astra",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.6-cyber",
            "gpt-5.3-codex",
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
        Provider::Ollama => &[
            "qwen3:8b",
            "gpt-oss:20b",
            "deepseek-r1:8b",
            "qwen3-coder:30b",
            "llama4:scout",
            "gemma3:4b",
            "gemma3:1b",
        ],
        Provider::OpenRouter => &[
            "openai/gpt-6-astra",
            "anthropic/claude-opus-5",
            "anthropic/claude-fable-5",
            "anthropic/claude-sonnet-5",
            "openai/gpt-5.6-sol",
            "google/gemini-3.7-flash",
            "z-ai/glm-5.3",
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        let current_dir = std::env::current_dir().expect("current dir");
        tempfile::tempdir_in(current_dir).expect("temp dir")
    }

    fn set_env(key: &str, value: &str) {
        std::env::set_var(key, value);
    }

    fn remove_env(key: &str) {
        std::env::remove_var(key);
    }

    #[test]
    fn provider_parses_all_known_names() {
        assert_eq!("anthropic".parse::<Provider>(), Ok(Provider::Anthropic));
        assert_eq!("gemini".parse::<Provider>(), Ok(Provider::Gemini));
        assert_eq!("mistral".parse::<Provider>(), Ok(Provider::Mistral));
        assert_eq!("openai".parse::<Provider>(), Ok(Provider::OpenAI));
        assert_eq!("z.ai".parse::<Provider>(), Ok(Provider::Zai));
        assert_eq!("zai".parse::<Provider>(), Ok(Provider::Zai));
        assert_eq!("ollama".parse::<Provider>(), Ok(Provider::Ollama));
        assert_eq!("openrouter".parse::<Provider>(), Ok(Provider::OpenRouter));
    }

    #[test]
    fn provider_parsing_is_case_insensitive() {
        assert_eq!("ANTHROPIC".parse::<Provider>(), Ok(Provider::Anthropic));
        assert_eq!("OpenAI".parse::<Provider>(), Ok(Provider::OpenAI));
        assert_eq!("Z.AI".parse::<Provider>(), Ok(Provider::Zai));
    }

    #[test]
    fn provider_rejects_unknown_names() {
        assert!("".parse::<Provider>().is_err());
        assert!("claude".parse::<Provider>().is_err());
        let err = "nope".parse::<Provider>().unwrap_err();
        assert_eq!(err, "Unsupported provider 'nope'");
    }

    #[test]
    fn provider_display_roundtrips_through_from_str() {
        let providers = [
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Mistral,
            Provider::OpenAI,
            Provider::Zai,
            Provider::Ollama,
            Provider::OpenRouter,
        ];
        for provider in providers {
            assert_eq!(provider.to_string().parse::<Provider>(), Ok(provider));
        }
    }

    #[test]
    fn zai_serializes_with_dotted_name() {
        let json = serde_json::to_value(Provider::Zai).unwrap();
        assert_eq!(json, serde_json::json!("z.ai"));
        assert_eq!(
            serde_json::from_value::<Provider>(serde_json::json!("z.ai")).unwrap(),
            Provider::Zai
        );
    }

    #[test]
    fn provider_serde_roundtrips_every_variant() {
        let providers = [
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Mistral,
            Provider::OpenAI,
            Provider::Zai,
            Provider::Ollama,
            Provider::OpenRouter,
        ];
        for provider in providers {
            let value = serde_json::to_value(provider).unwrap();
            assert_eq!(serde_json::from_value::<Provider>(value).unwrap(), provider);
        }
    }

    #[test]
    fn provider_defaults_to_anthropic() {
        assert_eq!(Provider::default(), Provider::Anthropic);
    }

    #[test]
    fn effort_level_parses_case_insensitively() {
        assert_eq!("low".parse::<EffortLevel>(), Ok(EffortLevel::Low));
        assert_eq!("MEDIUM".parse::<EffortLevel>(), Ok(EffortLevel::Medium));
        assert_eq!("High".parse::<EffortLevel>(), Ok(EffortLevel::High));
    }

    #[test]
    fn effort_level_rejects_unknown_values() {
        let err = "extreme".parse::<EffortLevel>().unwrap_err();
        assert_eq!(
            err,
            "Invalid effort level 'extreme'. Valid values: low, medium, high"
        );
    }

    #[test]
    fn effort_level_display_roundtrips() {
        for level in [EffortLevel::Low, EffortLevel::Medium, EffortLevel::High] {
            assert_eq!(level.to_string().parse::<EffortLevel>(), Ok(level));
        }
    }

    #[test]
    fn effort_level_defaults_to_medium() {
        assert_eq!(EffortLevel::default(), EffortLevel::Medium);
    }

    #[test]
    fn effort_level_anthropic_reasoning_budget() {
        assert_eq!(EffortLevel::Low.anthropic_reasoning_budget(), None);
        assert_eq!(EffortLevel::Medium.anthropic_reasoning_budget(), Some(10_000));
        assert_eq!(EffortLevel::High.anthropic_reasoning_budget(), Some(50_000));
    }

    #[test]
    fn effort_level_openai_reasoning_effort() {
        assert_eq!(EffortLevel::Low.openai_reasoning_effort(), "low");
        assert_eq!(EffortLevel::Medium.openai_reasoning_effort(), "medium");
        assert_eq!(EffortLevel::High.openai_reasoning_effort(), "high");
    }

    #[test]
    fn effort_level_ollama_think() {
        assert!(!EffortLevel::Low.ollama_think());
        assert!(EffortLevel::Medium.ollama_think());
        assert!(EffortLevel::High.ollama_think());
    }

    #[test]
    fn provider_default_models_are_populated() {
        let providers = [
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Mistral,
            Provider::OpenAI,
            Provider::Zai,
            Provider::Ollama,
            Provider::OpenRouter,
        ];
        for provider in providers {
            assert!(!provider_default_model(provider).is_empty());
            assert!(!provider_models(provider).is_empty());
        }
    }

    #[test]
    fn provider_default_model_is_in_model_list() {
        let providers = [
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Mistral,
            Provider::OpenAI,
            Provider::Zai,
            Provider::Ollama,
            Provider::OpenRouter,
        ];
        for provider in providers {
            let default_model = provider_default_model(provider);
            assert!(
                provider_models(provider).contains(&default_model.as_str()),
                "default model '{}' for {} missing from model list",
                default_model,
                provider
            );
        }
    }

    #[test]
    fn provider_model_lists_include_latest_flagships() {
        let latest = [
            (Provider::Anthropic, "claude-fable-5-1"),
            (Provider::Gemini, "gemini-3.8-flash"),
            (Provider::Mistral, "mistral-medium-3-5"),
            (Provider::OpenAI, "gpt-6-astra"),
            (Provider::Zai, "glm-5.3"),
            (Provider::Ollama, "qwen3:8b"),
            (Provider::OpenRouter, "openai/gpt-6-astra"),
        ];
        for (provider, model) in latest {
            assert!(
                provider_models(provider).contains(&model),
                "{} model list is missing latest model '{}'",
                provider,
                model
            );
        }
    }

    #[test]
    fn provider_default_base_urls_are_https() {
        let providers = [
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Mistral,
            Provider::OpenAI,
            Provider::Zai,
            Provider::OpenRouter,
        ];
        for provider in providers {
            let url = provider_default_base_url(provider);
            assert!(
                url.starts_with("https://"),
                "unexpected default base url for {}: {}",
                provider,
                url
            );
        }
    }

    #[test]
    fn ollama_default_base_url_is_local() {
        assert_eq!(
            provider_default_base_url(Provider::Ollama),
            "http://localhost:11434"
        );
    }

    #[test]
    fn default_config_path_points_at_flexorama_config() {
        let path = Config::default_config_path();
        assert_eq!(path.file_name().unwrap(), "config.toml");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "flexorama");
    }

    #[test]
    fn default_config_has_sane_values() {
        let config = Config::default();
        assert_eq!(config.provider, Provider::Anthropic);
        assert_eq!(config.max_tokens, 4096);
        assert!((config.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.effort, EffortLevel::Medium);
        assert!(config.default_system_prompt.is_some());
        assert!(config.mcp.servers.is_empty());
    }

    #[test]
    fn set_provider_refreshes_provider_specific_defaults() {
        let mut config = Config::default();
        config.set_provider(Provider::Mistral);

        assert_eq!(config.provider, Provider::Mistral);
        assert_eq!(config.base_url, provider_default_base_url(Provider::Mistral));
        assert_eq!(
            config.default_model,
            provider_default_model(Provider::Mistral)
        );
        assert_eq!(config.api_key, provider_default_api_key(Provider::Mistral));
    }

    #[tokio::test]
    async fn save_writes_toml_without_api_key() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        let mut config = Config::default();
        config.api_key = "sk-secret".to_string();
        config.default_system_prompt = Some("Be brief.".to_string());

        config.save(Some(path.to_str().unwrap())).await.unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("sk-secret"), "api key leaked to disk");
        assert!(!contents.contains("api_key"), "api key field present on disk");
        assert!(contents.contains("default_system_prompt"));
        assert!(contents.contains("max_tokens"));
    }

    #[tokio::test]
    async fn save_creates_missing_parent_directories() {
        let temp = temp_dir();
        let path = temp.path().join("nested/dir/config.toml");

        Config::default()
            .save(Some(path.to_str().unwrap()))
            .await
            .unwrap();

        assert!(path.exists());
    }

    #[tokio::test]
    async fn load_returns_defaults_when_file_missing() {
        let temp = temp_dir();
        let path = temp.path().join("does-not-exist.toml");

        let config = Config::load(Some(path.to_str().unwrap())).await.unwrap();

        assert_eq!(config.provider, Provider::default());
        assert_eq!(config.max_tokens, 4096);
        assert!(!config.base_url.is_empty());
        assert!(!config.default_model.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn load_reads_back_saved_config() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        let mut saved = Config::default();
        saved.provider = Provider::Mistral;
        saved.base_url = "https://example.test/v1".to_string();
        saved.default_model = "mistral-large-latest".to_string();
        saved.max_tokens = 1234;
        saved.temperature = 0.3;
        saved.effort = EffortLevel::High;
        saved
            .skills
            .active_skills
            .push("code-review".to_string());
        saved.save(Some(path.to_str().unwrap())).await.unwrap();

        set_env("MISTRAL_API_KEY", "test-mistral-key");
        let loaded = Config::load(Some(path.to_str().unwrap())).await.unwrap();
        remove_env("MISTRAL_API_KEY");

        assert_eq!(loaded.provider, Provider::Mistral);
        assert_eq!(loaded.base_url, "https://example.test/v1");
        assert_eq!(loaded.default_model, "mistral-large-latest");
        assert_eq!(loaded.max_tokens, 1234);
        assert!((loaded.temperature - 0.3).abs() < f32::EPSILON);
        assert_eq!(loaded.effort, EffortLevel::High);
        assert_eq!(loaded.skills.active_skills, vec!["code-review".to_string()]);
        assert_eq!(loaded.api_key, "test-mistral-key");
    }

    #[tokio::test]
    #[serial]
    async fn load_fills_missing_base_url_and_model_from_provider() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "provider = \"openai\"\nbase_url = \"\"\ndefault_model = \"\"\nmax_tokens = 100\ntemperature = 0.5\ndefault_system_prompt = \"p\"\n\n[bash_security]\nallowed_commands = []\ndenied_commands = []\nask_for_permission = false\nenabled = false\n\n[mcp]\n[mcp.servers]"
        )
        .unwrap();
        drop(file);

        remove_env("OPENAI_BASE_URL");
        let config = Config::load(Some(path.to_str().unwrap())).await.unwrap();

        assert_eq!(config.provider, Provider::OpenAI);
        assert_eq!(config.base_url, provider_default_base_url(Provider::OpenAI));
        assert_eq!(
            config.default_model,
            provider_default_model(Provider::OpenAI)
        );
        assert_eq!(config.max_tokens, 100);
    }

    #[tokio::test]
    #[serial]
    async fn load_never_takes_api_key_from_config_file() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "api_key = \"leaked-key\"\nprovider = \"anthropic\"\nbase_url = \"https://example.test/v1\"\ndefault_model = \"claude\"\nmax_tokens = 10\ntemperature = 0.5\ndefault_system_prompt = \"p\"\n\n[bash_security]\nallowed_commands = []\ndenied_commands = []\nask_for_permission = false\nenabled = false\n\n[mcp]\n[mcp.servers]"
        )
        .unwrap();
        drop(file);

        remove_env("ANTHROPIC_AUTH_TOKEN");
        let config = Config::load(Some(path.to_str().unwrap())).await.unwrap();
        remove_env("ANTHROPIC_BASE_URL");

        assert_eq!(config.api_key, "");
    }

    #[tokio::test]
    #[serial]
    async fn load_prefers_environment_base_url_override() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "provider = \"anthropic\"\nbase_url = \"\"\ndefault_model = \"\"\nmax_tokens = 10\ntemperature = 0.5\ndefault_system_prompt = \"p\"\n\n[bash_security]\nallowed_commands = []\ndenied_commands = []\nask_for_permission = false\nenabled = false\n\n[mcp]\n[mcp.servers]"
        )
        .unwrap();
        drop(file);

        set_env("ANTHROPIC_BASE_URL", "https://proxy.test/v1");
        let config = Config::load(Some(path.to_str().unwrap())).await.unwrap();
        remove_env("ANTHROPIC_BASE_URL");

        assert_eq!(config.base_url, "https://proxy.test/v1");
    }

    #[tokio::test]
    async fn load_rejects_invalid_toml() {
        let temp = temp_dir();
        let path = temp.path().join("config.toml");

        std::fs::write(&path, "not [ valid toml").unwrap();

        assert!(Config::load(Some(path.to_str().unwrap())).await.is_err());
    }

    #[test]
    fn config_path_reflects_default_location() {
        let config = Config::default();
        assert_eq!(config.path(), Config::default_config_path());
    }

    #[test]
    fn mcp_oauth_grant_and_client_auth_have_defaults() {
        assert_eq!(
            McpOAuthGrantType::default(),
            McpOAuthGrantType::AuthorizationCode
        );
        assert_eq!(McpOAuthClientAuth::default(), McpOAuthClientAuth::Body);
    }

    #[test]
    fn mcp_server_config_deserializes_from_partial_toml() {
        let config: McpServerConfig = toml::from_str(
            "name = \"docs\"\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server\"]\nenabled = true",
        )
        .unwrap();

        assert_eq!(config.name, "docs");
        assert!(config.url.is_none());
        assert!(config.auth.is_none());
        assert!(config.enabled);
    }

    #[test]
    fn mcp_oauth_config_deserializes_with_defaults() {
        let config: McpOAuthConfig = toml::from_str(
            "client_id = \"client\"\ntoken_url = \"https://example.test/token\"",
        )
        .unwrap();

        assert_eq!(config.client_id, "client");
        assert_eq!(config.token_url.as_deref(), Some("https://example.test/token"));
        assert_eq!(config.grant_type, McpOAuthGrantType::AuthorizationCode);
        assert_eq!(config.client_auth, McpOAuthClientAuth::Body);
        assert!(config.client_secret.is_none());
    }

    #[test]
    fn mcp_auth_config_tags_oauth_variant() {
        let config: McpAuthConfig = toml::from_str(
            "type = \"oauth\"\nclient_id = \"client\"",
        )
        .unwrap();

        match config {
            McpAuthConfig::OAuth(oauth) => assert_eq!(oauth.client_id, "client"),
        }
    }
}
