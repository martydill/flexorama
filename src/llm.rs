use crate::anthropic::{AnthropicClient, AnthropicResponse, ContentBlock, Message};
use crate::config::Provider;
use crate::gemini::GeminiClient;
use crate::ollama::OllamaClient;
use crate::openai::OpenAIClient;
use crate::tools::{Tool, ToolCall};
use anyhow::Result;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub type LlmResponse = AnthropicResponse;

pub struct LlmClient {
    provider: Provider,
    anthropic: Option<AnthropicClient>,
    gemini: Option<GeminiClient>,
    openai: Option<OpenAIClient>,
    ollama: Option<OllamaClient>,
}

impl LlmClient {
    pub fn new(provider: Provider, api_key: String, base_url: String) -> Self {
        match provider {
            Provider::Anthropic => Self {
                provider,
                anthropic: Some(AnthropicClient::new(api_key, base_url)),
                gemini: None,
                openai: None,
                ollama: None,
            },
            Provider::Gemini => Self {
                provider,
                anthropic: None,
                gemini: Some(GeminiClient::new(api_key, base_url)),
                openai: None,
                ollama: None,
            },
            Provider::OpenAI => Self {
                provider,
                anthropic: None,
                gemini: None,
                openai: Some(OpenAIClient::new(api_key, base_url)),
                ollama: None,
            },
            Provider::Zai => Self {
                provider,
                anthropic: Some(AnthropicClient::new(api_key, base_url)),
                gemini: None,
                openai: None,
                ollama: None,
            },
            Provider::Ollama => Self {
                provider,
                anthropic: None,
                gemini: None,
                openai: None,
                ollama: Some(OllamaClient::new(api_key, base_url)),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn provider(&self) -> Provider {
        self.provider
    }

    #[cfg(test)]
    pub(crate) fn has_anthropic_client(&self) -> bool {
        self.anthropic.is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_gemini_client(&self) -> bool {
        self.gemini.is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_openai_client(&self) -> bool {
        self.openai.is_some()
    }

    pub async fn create_message(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<LlmResponse> {
        match self.provider {
            Provider::Anthropic => {
                self.anthropic
                    .as_ref()
                    .expect("Anthropic client should be initialized")
                    .create_message(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Gemini => {
                self.gemini
                    .as_ref()
                    .expect("Gemini client should be initialized")
                    .create_message(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::OpenAI => {
                self.openai
                    .as_ref()
                    .expect("OpenAI client should be initialized")
                    .create_message(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Zai => {
                self.anthropic
                    .as_ref()
                    .expect("Z.ai client should be initialized")
                    .create_message(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Ollama => {
                self.ollama
                    .as_ref()
                    .expect("Ollama client should be initialized")
                    .create_message(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        cancellation_flag,
                    )
                    .await
            }
        }
    }

    pub async fn create_message_stream(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        on_content: Arc<dyn Fn(String) + Send + Sync + 'static>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<LlmResponse> {
        match self.provider {
            Provider::Anthropic => {
                self.anthropic
                    .as_ref()
                    .expect("Anthropic client should be initialized")
                    .create_message_stream(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        on_content,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Gemini => {
                self.gemini
                    .as_ref()
                    .expect("Gemini client should be initialized")
                    .create_message_stream(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        on_content,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::OpenAI => {
                self.openai
                    .as_ref()
                    .expect("OpenAI client should be initialized")
                    .create_message_stream(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        on_content,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Zai => {
                self.anthropic
                    .as_ref()
                    .expect("Z.ai client should be initialized")
                    .create_message_stream(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        on_content,
                        cancellation_flag,
                    )
                    .await
            }
            Provider::Ollama => {
                self.ollama
                    .as_ref()
                    .expect("Ollama client should be initialized")
                    .create_message_stream(
                        model,
                        messages,
                        tools,
                        max_tokens,
                        temperature,
                        system_prompt,
                        on_content,
                        cancellation_flag,
                    )
                    .await
            }
        }
    }

    pub fn convert_tool_calls(&self, content_blocks: &[ContentBlock]) -> Vec<ToolCall> {
        convert_tool_calls(content_blocks)
    }

    pub fn create_response_content(&self, content_blocks: &[ContentBlock]) -> String {
        create_response_content(content_blocks)
    }
}

pub fn convert_tool_calls(content_blocks: &[ContentBlock]) -> Vec<ToolCall> {
    content_blocks
        .iter()
        .filter_map(|block| {
            if block.block_type == "tool_use" {
                let tool_call = ToolCall {
                    id: block.id.as_ref().unwrap_or(&String::new()).clone(),
                    name: block.name.as_ref().unwrap_or(&String::new()).clone(),
                    arguments: block.input.as_ref().unwrap_or(&Value::Null).clone(),
                };
                Some(tool_call)
            } else {
                None
            }
        })
        .collect()
}

pub fn create_response_content(content_blocks: &[ContentBlock]) -> String {
    content_blocks
        .iter()
        .filter_map(|block| {
            if block.block_type == "text" {
                block.text.clone()
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{OriginalUri, State};
    use axum::http::header;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[derive(Clone, Default)]
    struct RequestLog {
        hits: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    }

    impl RequestLog {
        fn record(&self, path: String) {
            self.hits.fetch_add(1, Ordering::SeqCst);
            self.paths.lock().expect("paths lock").push(path);
        }

        fn hit_count(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }

        fn recorded_paths(&self) -> Vec<String> {
            self.paths.lock().expect("paths lock").clone()
        }
    }

    async fn spawn_server(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{}", addr);
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test server");
        });
        base_url
    }

    fn configure_no_proxy() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
    }

    async fn anthropic_handler(
        State(log): State<RequestLog>,
        OriginalUri(uri): OriginalUri,
        Json(payload): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        log.record(uri.path().to_string());
        let is_stream = payload
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if is_stream {
            let sse_body = concat!(
                "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                "data: {\"type\":\"content_block_stop\"}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n",
                "data: [DONE]\n\n"
            );
            ([(header::CONTENT_TYPE, "text/event-stream")], sse_body).into_response()
        } else {
            Json(json!({"content":[{"type":"text","text":"ok"}]})).into_response()
        }
    }

    async fn gemini_handler(
        State(log): State<RequestLog>,
        OriginalUri(uri): OriginalUri,
        Json(_payload): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        log.record(uri.path().to_string());
        Json(json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "ok"}]
                }
            }]
        }))
    }

    async fn openai_handler(
        State(log): State<RequestLog>,
        OriginalUri(uri): OriginalUri,
        Json(_payload): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        log.record(uri.path().to_string());
        Json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "ok"
                }
            }]
        }))
    }

    #[tokio::test]
    async fn provider_returns_expected_variant() {
        configure_no_proxy();

        // Test Anthropic provider
        let client = LlmClient::new(
            Provider::Anthropic,
            "test-key".to_string(),
            "http://localhost".to_string(),
        );
        assert_eq!(client.provider(), Provider::Anthropic);
        assert!(client.has_anthropic_client());
        assert!(!client.has_gemini_client());
        assert!(!client.has_openai_client());

        // Test Gemini provider
        let client = LlmClient::new(
            Provider::Gemini,
            "test-key".to_string(),
            "http://localhost".to_string(),
        );
        assert_eq!(client.provider(), Provider::Gemini);
        assert!(!client.has_anthropic_client());
        assert!(client.has_gemini_client());
        assert!(!client.has_openai_client());

        // Test OpenAI provider
        let client = LlmClient::new(
            Provider::OpenAI,
            "test-key".to_string(),
            "http://localhost".to_string(),
        );
        assert_eq!(client.provider(), Provider::OpenAI);
        assert!(!client.has_anthropic_client());
        assert!(!client.has_gemini_client());
        assert!(client.has_openai_client());

        // Test Zai provider (uses Anthropic client)
        let client = LlmClient::new(
            Provider::Zai,
            "test-key".to_string(),
            "http://localhost".to_string(),
        );
        assert_eq!(client.provider(), Provider::Zai);
        assert!(client.has_anthropic_client());
        assert!(!client.has_gemini_client());
        assert!(!client.has_openai_client());
    }

    #[tokio::test]
    async fn routes_anthropic_based_providers() {
        let log = RequestLog::default();
        let app = Router::new()
            .route("/*path", post(anthropic_handler))
            .with_state(log.clone());
        configure_no_proxy();
        let base_url = spawn_server(app).await;

        let providers = [Provider::Anthropic, Provider::Zai];
        for provider in providers {
            let client = LlmClient::new(provider, "test-key".to_string(), base_url.clone());
            let messages = vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::text("ping".to_string())],
            }];
            let cancellation_flag = Arc::new(AtomicBool::new(false));

            client
                .create_message(
                    "test-model",
                    messages.clone(),
                    &[],
                    16,
                    0.0,
                    None,
                    cancellation_flag.clone(),
                )
                .await
                .expect("create_message");

            client
                .create_message_stream(
                    "test-model",
                    messages.clone(),
                    &[],
                    16,
                    0.0,
                    None,
                    Arc::new(|_chunk| {}),
                    cancellation_flag.clone(),
                )
                .await
                .expect("create_message_stream");
        }

        assert_eq!(log.hit_count(), 4);
        for path in log.recorded_paths() {
            assert_eq!(path, "/v1/messages");
        }
    }

    #[tokio::test]
    async fn routes_openai_provider() {
        let log = RequestLog::default();
        let app = Router::new()
            .route("/*path", post(openai_handler))
            .with_state(log.clone());
        configure_no_proxy();
        let base_url = spawn_server(app).await;

        let client = LlmClient::new(Provider::OpenAI, "test-key".to_string(), base_url);
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("ping".to_string())],
        }];
        let cancellation_flag = Arc::new(AtomicBool::new(false));

        client
            .create_message(
                "test-model",
                messages.clone(),
                &[],
                16,
                0.0,
                None,
                cancellation_flag.clone(),
            )
            .await
            .expect("create_message");

        client
            .create_message_stream(
                "test-model",
                messages,
                &[],
                16,
                0.0,
                None,
                Arc::new(|_chunk| {}),
                cancellation_flag,
            )
            .await
            .expect("create_message_stream");

        assert_eq!(log.hit_count(), 2);
        for path in log.recorded_paths() {
            assert_eq!(path, "/chat/completions");
        }
    }

    #[tokio::test]
    async fn routes_gemini_provider() {
        let log = RequestLog::default();
        let app = Router::new()
            .route("/*path", post(gemini_handler))
            .with_state(log.clone());
        configure_no_proxy();
        let base_url = spawn_server(app).await;

        let client = LlmClient::new(Provider::Gemini, "test-key".to_string(), base_url);
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("ping".to_string())],
        }];
        let cancellation_flag = Arc::new(AtomicBool::new(false));

        client
            .create_message(
                "test-model",
                messages.clone(),
                &[],
                16,
                0.0,
                None,
                cancellation_flag.clone(),
            )
            .await
            .expect("create_message");

        client
            .create_message_stream(
                "test-model",
                messages.clone(),
                &[],
                16,
                0.0,
                None,
                Arc::new(|_chunk| {}),
                cancellation_flag,
            )
            .await
            .expect("create_message_stream");

        assert_eq!(log.hit_count(), 2);
        for path in log.recorded_paths() {
            assert_eq!(path, "/models/test-model:generateContent");
        }
    }
}
