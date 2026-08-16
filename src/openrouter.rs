// OpenRouter client - uses OpenAI-compatible API
// OpenRouter provides access to multiple LLM providers through a single API

use crate::anthropic::{AnthropicResponse, Message};
use crate::config::EffortLevel;
use crate::openai::OpenAIClient;
use crate::tools::Tool;
use anyhow::Result;
use log::{debug, info};
use reqwest::Client;
use serde::Deserialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// OpenRouter client that wraps the OpenAI client
/// OpenRouter uses an OpenAI-compatible API format, so we can reuse the OpenAI implementation
pub struct OpenRouterClient {
    openai_client: OpenAIClient,
    api_key: String,
    base_url: String,
    http_client: Client,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    context_length: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    pricing: Option<OpenRouterPricing>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPricing {
    #[serde(default)]
    #[allow(dead_code)]
    prompt: String,
    #[serde(default)]
    #[allow(dead_code)]
    completion: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

impl OpenRouterClient {
    /// Create a new OpenRouter client
    ///
    /// # Arguments
    /// * `api_key` - OpenRouter API key (from OPENROUTER_API_KEY env var)
    /// * `base_url` - OpenRouter base URL (defaults to https://openrouter.ai/api/v1)
    pub fn new(api_key: String, base_url: String) -> Self {
        let openai_client = OpenAIClient::new(api_key.clone(), base_url.clone());
        Self {
            openai_client,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            http_client: Client::new(),
        }
    }

    /// Fetch available models from OpenRouter API
    /// Returns a list of model IDs that can be used
    pub async fn fetch_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.base_url);
        debug!("Fetching models from OpenRouter: {}", url);

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://flexorama.ai")
            .header("X-Title", "Flexorama")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "OpenRouter models API error: {} - {}",
                status,
                error_text
            ));
        }

        let models_response: OpenRouterModelsResponse = response.json().await?;
        let models: Vec<String> = models_response.data.iter().map(|m| m.id.clone()).collect();

        info!("Fetched {} models from OpenRouter", models.len());
        debug!("Available models: {:?}", models);

        Ok(models)
    }

    /// Get a static list of fallback models in case API fetch fails
    pub fn fallback_models() -> &'static [&'static str] {
        &[
            "anthropic/claude-opus-5",
            "anthropic/claude-sonnet-5",
            "openai/gpt-5.6-sol",
            "google/gemini-2.5-pro",
            "meta-llama/llama-4-matrix",
            "mistralai/mistral-large",
            "openai/gpt-4o",
            "openai/gpt-4o-mini",
            "google/gemini-flash-latest",
        ]
    }

    /// Send a message to OpenRouter (non-streaming)
    pub async fn create_message(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        effort: EffortLevel,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<AnthropicResponse> {
        self.openai_client
            .create_message(model, messages, tools, max_tokens, temperature, system_prompt, effort, cancellation_flag)
            .await
    }

    /// Send a message to OpenRouter with streaming
    pub async fn create_message_stream(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        effort: EffortLevel,
        on_content: Arc<dyn Fn(String) + Send + Sync + 'static>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<AnthropicResponse> {
        self.openai_client
            .create_message_stream(model, messages, tools, max_tokens, temperature, system_prompt, effort, on_content, cancellation_flag)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_client_creation() {
        let _client = OpenRouterClient::new(
            "test-key".to_string(),
            "https://openrouter.ai/api/v1".to_string(),
        );
        // Verify client was created successfully
        // The actual API testing is done in llm.rs integration tests
    }

    #[test]
    fn test_fallback_models() {
        let models = OpenRouterClient::fallback_models();
        assert!(!models.is_empty());
        assert!(models.contains(&"anthropic/claude-opus-5"));
    }
}
