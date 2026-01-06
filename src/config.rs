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
        Provider::Zai => std::env::var("ZAI_API_KEY").unwrap_or_default(),
    }
}

pub fn provider_default_base_url(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string()),
        Provider::Gemini => std::env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        Provider::Zai => std::env::var("ZAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.z.ai/api/anthropic".to_string()),
    }
}

pub fn provider_default_model(provider: Provider) -> String {
    match provider {
        Provider::Anthropic => "claude-3-5-sonnet-20240620".to_string(),
        Provider::Gemini => "gemini-flash-latest".to_string(),
        Provider::Zai => "glm-4.7".to_string(),
    }
}

pub fn provider_models(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Anthropic => &["claude-3-5-sonnet-20240620", "claude-3-5-haiku-20241022"],
        Provider::Gemini => &[
            "gemini-flash-latest",
            "gemini-1.5-pro",
            "gemini-1.5-flash",
            "gemini-2.5-flash",
            "gemini-3-flash-preview",
            "gemini-2.5-flash-lite",
            "gemini-2.5-pro",
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
        }
    }
}

impl Config {
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("aixplosion")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = env::var(key).ok();
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    fn temp_config_path(temp_dir: &TempDir) -> String {
        temp_dir
            .path()
            .join("config.toml")
            .to_string_lossy()
            .to_string()
    }

    #[tokio::test]
    async fn save_clears_api_key_in_toml() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_config_path(&temp_dir);
        let mut config = Config::default();
        config.api_key = "super-secret".to_string();

        config.save(Some(&path)).await.expect("save config");

        let content = fs::read_to_string(&path).await.expect("read config");
        assert!(!content.contains("api_key"));
        assert!(!content.contains("super-secret"));
    }

    #[tokio::test]
    async fn load_ignores_api_key_from_file_and_uses_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _api_guard = EnvVarGuard::set("ANTHROPIC_AUTH_TOKEN", Some("env-key"));
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_config_path(&temp_dir);
        let mut config = Config::default();
        config.base_url = "https://config.example".to_string();
        config.default_model = "config-model".to_string();
        let content = toml::to_string_pretty(&config).expect("serialize config");
        let content = format!("api_key = \"file-key\"\n{content}");
        fs::write(&path, content).await.expect("write config");

        let loaded = Config::load(Some(&path)).await.expect("load config");

        assert_eq!(loaded.api_key, "env-key");
        assert_ne!(loaded.api_key, "file-key");
    }

    #[tokio::test]
    async fn load_applies_provider_defaults_when_fields_empty() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _base_url_guard =
            EnvVarGuard::set("ANTHROPIC_BASE_URL", Some("https://defaults.example"));
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_config_path(&temp_dir);
        let mut config = Config::default();
        config.base_url = String::new();
        config.default_model = String::new();
        let content = toml::to_string_pretty(&config).expect("serialize config");
        fs::write(&path, content).await.expect("write config");

        let loaded = Config::load(Some(&path)).await.expect("load config");

        assert_eq!(loaded.base_url, "https://defaults.example");
        assert_eq!(
            loaded.default_model,
            provider_default_model(Provider::Anthropic)
        );
    }

    #[test]
    fn set_provider_refreshes_provider_defaults() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _base_url_guard = EnvVarGuard::set("GEMINI_BASE_URL", Some("https://gemini.example"));
        let _api_key_guard = EnvVarGuard::set("GEMINI_API_KEY", Some("gemini-key"));
        let mut config = Config::default();
        config.base_url = "https://old.example".to_string();
        config.default_model = "old-model".to_string();
        config.api_key = "old-key".to_string();

        config.set_provider(Provider::Gemini);

        assert_eq!(config.provider, Provider::Gemini);
        assert_eq!(config.base_url, "https://gemini.example");
        assert_eq!(config.default_model, "gemini-flash-latest");
        assert_eq!(config.api_key, "gemini-key");
    }
}
