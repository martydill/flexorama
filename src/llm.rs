use crate::anthropic::{AnthropicClient, AnthropicResponse, ContentBlock, Message};
use crate::config::Provider;
use crate::gemini::GeminiClient;
use crate::tools::{Tool, ToolCall};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub type LlmResponse = AnthropicResponse;

pub struct LlmClient {
    provider: Provider,
    anthropic: Option<AnthropicClient>,
    gemini: Option<GeminiClient>,
}

impl LlmClient {
    pub fn new(provider: Provider, api_key: String, base_url: String) -> Self {
        match provider {
            Provider::Anthropic => Self {
                provider,
                anthropic: Some(AnthropicClient::new(api_key, base_url)),
                gemini: None,
            },
            Provider::Gemini => Self {
                provider,
                anthropic: None,
                gemini: Some(GeminiClient::new(api_key, base_url)),
            },
            Provider::OpenAI => Self {
                provider,
                anthropic: Some(AnthropicClient::new(api_key, base_url)),
                gemini: None,
            },
            Provider::Zai => Self {
                provider,
                anthropic: Some(AnthropicClient::new(api_key, base_url)),
                gemini: None,
            },
        }
    }

    pub fn provider(&self) -> Provider {
        self.provider
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
                self.anthropic
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
                self.anthropic
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
