use crate::anthropic::{AnthropicResponse, ContentBlock, Message, Usage};
use crate::tools::Tool;
use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OllamaToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, rename = "type")]
    call_type: Option<String>,
    function: OllamaFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OllamaFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OllamaFunctionDefinition,
}

#[derive(Debug, Serialize)]
struct OllamaFunctionDefinition {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[serde(default)]
    done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_eval_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    eval_count: Option<u32>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelInfo>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelInfo {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    modified_at: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    size: Option<i64>,
}

pub struct OllamaClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OllamaClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch available models from Ollama server
    pub async fn fetch_available_models(&self) -> Result<Vec<String>> {
        let endpoint = format!("{}/api/tags", self.base_url);

        debug!("Fetching available Ollama models from {}", endpoint);

        let mut request_builder = self
            .client
            .get(&endpoint)
            .header("content-type", "application/json");

        // Only add authorization header if API key is not empty
        if !self.api_key.is_empty() {
            request_builder = request_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        let response = match request_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to fetch Ollama models: {}. Using default models.", e);
                return Ok(vec!["llama2".to_string(), "gemma3:1b".to_string()]);
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(
                "Ollama /api/tags returned error {}: {}. Using default models.",
                status, error_text
            );
            return Ok(vec!["llama2".to_string(), "gemma3:1b".to_string()]);
        }

        let response_text = response.text().await?;
        debug!("Ollama tags response: {}", response_text);

        match serde_json::from_str::<OllamaTagsResponse>(&response_text) {
            Ok(tags) => {
                let model_names: Vec<String> = tags
                    .models
                    .into_iter()
                    .map(|m| m.name)
                    .collect();

                if model_names.is_empty() {
                    warn!("No models found in Ollama. Using default models.");
                    Ok(vec!["llama2".to_string(), "gemma3:1b".to_string()])
                } else {
                    debug!("Found {} Ollama models", model_names.len());
                    Ok(model_names)
                }
            }
            Err(e) => {
                warn!("Failed to parse Ollama tags response: {}. Using default models.", e);
                Ok(vec!["llama2".to_string(), "gemma3:1b".to_string()])
            }
        }
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
    ) -> Result<AnthropicResponse> {
        if cancellation_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("CANCELLED"));
        }

        let request = self.build_request(
            model,
            messages,
            tools,
            max_tokens,
            temperature,
            system_prompt,
            false,
        );
        let endpoint = format!("{}/api/chat", self.base_url);

        // Log the full request
        let request_json = serde_json::to_string_pretty(&request).unwrap_or_else(|_| "Failed to serialize".to_string());
        debug!("=== Ollama Request ===");
        debug!("Endpoint: {}", endpoint);
        debug!("Model: {}", model);
        debug!("Tools count: {}", tools.len());
        debug!("Request body:\n{}", request_json);

        let mut request_builder = self
            .client
            .post(&endpoint)
            .header("content-type", "application/json")
            .json(&request);

        // Only add authorization header if API key is not empty
        if !self.api_key.is_empty() {
            request_builder = request_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        let response = request_builder.send().await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("=== Ollama Request Failed ===");
            error!("Status: {}", status);
            error!("Response: {}", response_text);
            return Err(anyhow::anyhow!(
                "Ollama API error: {} - {}",
                status,
                response_text
            ));
        }

        debug!("=== Ollama Response ===");
        debug!("Status: {}", status);
        debug!("Response body:\n{}", response_text);

        let parsed: OllamaResponse = serde_json::from_str(&response_text).map_err(|e| {
            error!("Failed to parse Ollama response: {}", e);
            error!("Response text: {}", response_text);
            e
        })?;

        debug!("Parsed response: {:?}", parsed);
        let anthropic_response = self.map_response(parsed);
        debug!("Mapped to AnthropicResponse: {:?}", anthropic_response);

        Ok(anthropic_response)
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
    ) -> Result<AnthropicResponse> {
        if cancellation_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("CANCELLED"));
        }

        let request = self.build_request(
            model,
            messages,
            tools,
            max_tokens,
            temperature,
            system_prompt,
            true,
        );
        let endpoint = format!("{}/api/chat", self.base_url);

        // Log the full request
        let request_json = serde_json::to_string_pretty(&request).unwrap_or_else(|_| "Failed to serialize".to_string());
        debug!("=== Ollama Streaming Request ===");
        debug!("Endpoint: {}", endpoint);
        debug!("Model: {}", model);
        debug!("Tools count: {}", tools.len());
        debug!("Request body:\n{}", request_json);

        let mut request_builder = self
            .client
            .post(&endpoint)
            .header("content-type", "application/json")
            .json(&request);

        // Only add authorization header if API key is not empty
        if !self.api_key.is_empty() {
            request_builder = request_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        let response = request_builder.send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!("=== Ollama Streaming Request Failed ===");
            error!("Status: {}", status);
            error!("Response: {}", error_text);
            return Err(anyhow::anyhow!(
                "Ollama API error: {} - {}",
                status,
                error_text
            ));
        }

        debug!("=== Ollama Streaming Response ===");
        debug!("Status: {}", status);

        let mut buffer = String::new();
        let mut content = String::new();
        let mut tool_calls: BTreeMap<usize, ToolCallBuilder> = BTreeMap::new();
        let mut prompt_tokens = 0;
        let mut completion_tokens = 0;
        let mut stream = response.bytes_stream();
        let mut chunk_count = 0;
        let mut empty_chunk_count = 0;
        const MAX_EMPTY_CHUNKS: usize = 50; // Prevent infinite loops

        while let Some(chunk_result) = stream.next().await {
            if cancellation_flag.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("CANCELLED"));
            }

            // Safety check: if we get too many empty chunks, something is wrong
            if empty_chunk_count > MAX_EMPTY_CHUNKS {
                warn!("Received {} empty chunks in a row. Breaking stream to prevent infinite loop.", empty_chunk_count);
                warn!("This likely means the model doesn't support tool calling or encountered an error.");
                break;
            }

            match chunk_result {
                Ok(chunk) => {
                    if let Ok(chunk_str) = std::str::from_utf8(&chunk) {
                        buffer.push_str(chunk_str);

                        // Process complete JSON lines
                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].trim().to_string();
                            buffer = buffer[newline_pos + 1..].to_string();

                            if line.is_empty() {
                                continue;
                            }

                            if let Ok(response) = serde_json::from_str::<OllamaResponse>(&line) {
                                chunk_count += 1;

                                // Track empty chunks to detect infinite loops
                                let is_empty_chunk = response.message.content.is_empty()
                                    && response.message.tool_calls.is_none()
                                    && !response.done;

                                if is_empty_chunk {
                                    empty_chunk_count += 1;
                                } else {
                                    empty_chunk_count = 0; // Reset counter on non-empty chunk
                                }

                                // Only log every 10th chunk or important ones to reduce spam
                                if chunk_count % 10 == 0 || response.done || response.message.tool_calls.is_some() {
                                    debug!("Streaming chunk #{}: done={}, content_len={}, has_tools={}, empty_streak={}",
                                           chunk_count, response.done, response.message.content.len(),
                                           response.message.tool_calls.is_some(), empty_chunk_count);
                                }

                                let text = &response.message.content;
                                if !text.is_empty() {
                                    content.push_str(text);
                                    on_content(text.clone());
                                }

                                if let Some(calls) = response.message.tool_calls {
                                    debug!("Received tool calls in stream: {} calls", calls.len());
                                    for (idx, call) in calls.into_iter().enumerate() {
                                        debug!("Tool call {}: {:?}", idx, call);
                                        let entry = tool_calls.entry(idx).or_default();
                                        if let Some(id) = call.id {
                                            entry.id = Some(id);
                                        }
                                        entry.name = Some(call.function.name);
                                        entry.arguments = serde_json::to_string(&call.function.arguments)
                                            .unwrap_or_else(|_| "{}".to_string());
                                    }
                                }

                                if let Some(prompt_count) = response.prompt_eval_count {
                                    prompt_tokens = prompt_count;
                                }
                                if let Some(eval_count) = response.eval_count {
                                    completion_tokens = eval_count;
                                }

                                if response.done {
                                    debug!("Stream complete after {} chunks. Done=true received.", chunk_count);
                                    break;
                                }
                            } else {
                                debug!("Failed to parse streaming line: {}", line);
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
            }
        }

        debug!("=== Stream Processing Complete ===");
        debug!("Total content length: {}", content.len());
        debug!("Tool calls collected: {}", tool_calls.len());

        let mut content_blocks = Vec::new();
        if !content.is_empty() {
            debug!("Adding text content block");
            content_blocks.push(ContentBlock::text(content.clone()));
        }

        for (index, builder) in tool_calls {
            let id = builder
                .id
                .clone()
                .unwrap_or_else(|| format!("ollama_call_{}", Uuid::new_v4().simple()));
            let name = builder.name.clone().unwrap_or_else(|| "tool".to_string());
            let input = parse_arguments(&builder.arguments);
            debug!("Adding tool_use block - index: {}, id: {}, name: {}, args: {}",
                   index, id, name, builder.arguments);
            content_blocks.push(ContentBlock::tool_use(id, name, input));
        }

        let usage = if prompt_tokens > 0 || completion_tokens > 0 {
            Some(Usage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
            })
        } else {
            None
        };

        let response = AnthropicResponse {
            content: content_blocks.clone(),
            usage,
        };

        debug!("Final AnthropicResponse: {} content blocks", content_blocks.len());
        for (i, block) in content_blocks.iter().enumerate() {
            debug!("  Block {}: {:?}", i, block);
        }

        Ok(response)
    }

    fn build_request(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        stream: bool,
    ) -> OllamaRequest {
        debug!("=== Building Ollama Request ===");
        debug!("Model: {}", model);
        debug!("Input messages count: {}", messages.len());
        debug!("Tools count: {}", tools.len());
        if let Some(prompt) = system_prompt {
            debug!("System prompt: {} chars", prompt.len());
        }

        let ollama_messages = map_messages(messages.clone(), system_prompt);
        debug!("Mapped to {} Ollama messages", ollama_messages.len());
        if tools.is_empty() {
            debug!("No tools to include");
        } else {
            debug!(
                "Ollama provider: ignoring {} tools to avoid tool payloads in requests",
                tools.len()
            );
        }

        OllamaRequest {
            model: model.to_string(),
            messages: ollama_messages,
            tools: None,
            stream: Some(stream),
            options: Some(OllamaOptions {
                temperature: Some(temperature),
                num_predict: Some(max_tokens),
            }),
        }
    }

    fn map_response(&self, response: OllamaResponse) -> AnthropicResponse {
        debug!("=== Mapping Ollama Response ===");
        debug!("Content: {}", response.message.content);
        debug!("Has tool_calls: {}", response.message.tool_calls.is_some());

        let mut content_blocks = Vec::new();

        if !response.message.content.is_empty() {
            debug!("Adding text content block");
            content_blocks.push(ContentBlock::text(response.message.content.clone()));
        }

        if let Some(tool_calls) = response.message.tool_calls {
            debug!("Processing {} tool calls from response", tool_calls.len());
            for (idx, call) in tool_calls.iter().enumerate() {
                let id = call
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("ollama_call_{}", Uuid::new_v4().simple()));
                debug!("  Tool call {}: id={}, name={}, args={:?}",
                       idx, id, call.function.name, call.function.arguments);
                content_blocks.push(ContentBlock::tool_use(
                    id,
                    call.function.name.clone(),
                    call.function.arguments.clone()
                ));
            }
        }

        let usage = if response.prompt_eval_count.is_some() || response.eval_count.is_some() {
            Some(Usage {
                input_tokens: response.prompt_eval_count.unwrap_or(0),
                output_tokens: response.eval_count.unwrap_or(0),
            })
        } else {
            None
        };

        debug!("Final content_blocks count: {}", content_blocks.len());

        AnthropicResponse {
            content: content_blocks,
            usage,
        }
    }
}

fn map_messages(messages: Vec<Message>, system_prompt: Option<&String>) -> Vec<OllamaMessage> {
    let mut ollama_messages = Vec::new();

    // Add system prompt as the first message if provided
    if let Some(prompt) = system_prompt {
        ollama_messages.push(OllamaMessage {
            role: "system".to_string(),
            content: prompt.clone(),
            tool_calls: None,
        });
    }

    for message in messages {
        let mut text_parts = Vec::new();

        for block in &message.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        text_parts.push(text.clone());
                    }
                }
                "tool_use" => {}
                "tool_result" => {
                    // Represent tool results as plain text so we avoid tool payloads.
                    if let Some(tool_content) = &block.content {
                        text_parts.push(tool_content.clone());
                    }
                }
                _ => {}
            }
        }

        let text = text_parts.join("\n");
        if !text.is_empty() {
            ollama_messages.push(OllamaMessage {
                role: message.role.clone(),
                content: text,
                tool_calls: None,
            });
        }
    }

    ollama_messages
}

fn parse_arguments(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Value::Object(Map::new());
    }

    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(arguments.to_string()))
}


