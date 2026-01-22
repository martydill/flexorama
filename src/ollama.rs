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
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
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

    /// Helper to build authenticated request with optional JSON body
    fn build_authenticated_request(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        json_body: Option<&impl Serialize>,
    ) -> reqwest::RequestBuilder {
        let mut request_builder = self
            .client
            .request(method, endpoint)
            .header("content-type", "application/json");

        if let Some(body) = json_body {
            request_builder = request_builder.json(body);
        }

        // Only add authorization header if API key is not empty
        if !self.api_key.is_empty() {
            request_builder =
                request_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        request_builder
    }

    /// Fetch available models from Ollama server
    pub async fn fetch_available_models(&self) -> Result<Vec<String>> {
        let endpoint = format!("{}/api/tags", self.base_url);

        debug!("Fetching available Ollama models from {}", endpoint);

        let response = match self
            .build_authenticated_request(reqwest::Method::GET, &endpoint, None::<&()>)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                warn!(
                    "Failed to fetch Ollama models: {}. Using default models.",
                    e
                );
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
                let model_names: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();

                if model_names.is_empty() {
                    warn!("No models found in Ollama. Using default models.");
                    Ok(vec!["llama2".to_string(), "gemma3:1b".to_string()])
                } else {
                    debug!("Found {} Ollama models", model_names.len());
                    Ok(model_names)
                }
            }
            Err(e) => {
                warn!(
                    "Failed to parse Ollama tags response: {}. Using default models.",
                    e
                );
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
        let request_json = serde_json::to_string_pretty(&request)
            .unwrap_or_else(|_| "Failed to serialize".to_string());
        debug!("=== Ollama Request ===");
        debug!("Endpoint: {}", endpoint);
        debug!("Model: {}", model);
        debug!("Tools count: {}", tools.len());
        debug!("Request body:\n{}", request_json);

        let response = self
            .build_authenticated_request(reqwest::Method::POST, &endpoint, Some(&request))
            .send()
            .await?;

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
        let request_json = serde_json::to_string_pretty(&request)
            .unwrap_or_else(|_| "Failed to serialize".to_string());
        debug!("=== Ollama Streaming Request ===");
        debug!("Endpoint: {}", endpoint);
        debug!("Model: {}", model);
        debug!("Tools count: {}", tools.len());
        debug!("Request body:\n{}", request_json);

        let response = self
            .build_authenticated_request(reqwest::Method::POST, &endpoint, Some(&request))
            .send()
            .await?;

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
                warn!(
                    "Received {} empty chunks in a row. Breaking stream to prevent infinite loop.",
                    empty_chunk_count
                );
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
                                if chunk_count % 10 == 0
                                    || response.done
                                    || response.message.tool_calls.is_some()
                                {
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
                                        entry.arguments =
                                            serde_json::to_string(&call.function.arguments)
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
                                    debug!(
                                        "Stream complete after {} chunks. Done=true received.",
                                        chunk_count
                                    );
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
            debug!(
                "Adding tool_use block - index: {}, id: {}, name: {}, args: {}",
                index, id, name, builder.arguments
            );
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

        debug!(
            "Final AnthropicResponse: {} content blocks",
            content_blocks.len()
        );
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
                debug!(
                    "  Tool call {}: id={}, name={}, args={:?}",
                    idx, id, call.function.name, call.function.arguments
                );
                content_blocks.push(ContentBlock::tool_use(
                    id,
                    call.function.name.clone(),
                    call.function.arguments.clone(),
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
            images: None,
        });
    }

    for message in messages {
        let mut text_parts = Vec::new();
        let mut image_data: Vec<String> = Vec::new();

        for block in &message.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        text_parts.push(text.clone());
                    }
                }
                "image" => {
                    if let Some(source) = &block.source {
                        image_data.push(source.data.clone());
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
        if !text.is_empty() || !image_data.is_empty() {
            ollama_messages.push(OllamaMessage {
                role: message.role.clone(),
                content: text,
                tool_calls: None,
                images: if image_data.is_empty() {
                    None
                } else {
                    Some(image_data)
                },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::ContentBlock;
    use serde_json::json;

    #[test]
    fn test_ollama_client_new() {
        let client =
            OllamaClient::new("test-key".to_string(), "http://localhost:11434".to_string());
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_ollama_client_new_trims_trailing_slash() {
        let client = OllamaClient::new(
            "test-key".to_string(),
            "http://localhost:11434/".to_string(),
        );
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_ollama_client_new_empty_api_key() {
        let client = OllamaClient::new("".to_string(), "http://localhost:11434".to_string());
        assert_eq!(client.api_key, "");
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_parse_arguments_empty() {
        let result = parse_arguments("");
        assert!(result.is_object());
        assert_eq!(result.as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_parse_arguments_whitespace() {
        let result = parse_arguments("   ");
        assert!(result.is_object());
        assert_eq!(result.as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_parse_arguments_valid_json() {
        let result = parse_arguments(r#"{"key": "value", "num": 42}"#);
        assert!(result.is_object());
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("key").unwrap().as_str().unwrap(), "value");
        assert_eq!(obj.get("num").unwrap().as_i64().unwrap(), 42);
    }

    #[test]
    fn test_parse_arguments_invalid_json() {
        let result = parse_arguments("not valid json");
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), "not valid json");
    }

    #[test]
    fn test_parse_arguments_array() {
        let result = parse_arguments(r#"[1, 2, 3]"#);
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_map_messages_empty() {
        let result = map_messages(vec![], None);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_map_messages_with_system_prompt() {
        let system_prompt = "You are a helpful assistant".to_string();
        let result = map_messages(vec![], Some(&system_prompt));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content, system_prompt);
        assert!(result[0].tool_calls.is_none());
    }

    #[test]
    fn test_map_messages_text_content() {
        let message = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hello world".to_string())],
        };
        let result = map_messages(vec![message], None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].content, "Hello world");
    }

    #[test]
    fn test_map_messages_multiple_text_blocks() {
        let message = Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::text("First".to_string()),
                ContentBlock::text("Second".to_string()),
                ContentBlock::text("Third".to_string()),
            ],
        };
        let result = map_messages(vec![message], None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "First\nSecond\nThird");
    }

    #[test]
    fn test_map_messages_tool_result() {
        let mut tool_result = ContentBlock::text("".to_string());
        tool_result.block_type = "tool_result".to_string();
        tool_result.content = Some("Tool output".to_string());

        let message = Message {
            role: "user".to_string(),
            content: vec![tool_result],
        };
        let result = map_messages(vec![message], None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "Tool output");
    }

    #[test]
    fn test_map_messages_skips_empty_messages() {
        let empty_message = Message {
            role: "user".to_string(),
            content: vec![],
        };
        let result = map_messages(vec![empty_message], None);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_map_messages_with_system_and_user_messages() {
        let system_prompt = "System".to_string();
        let user_message = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("User".to_string())],
        };
        let result = map_messages(vec![user_message], Some(&system_prompt));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content, "System");
        assert_eq!(result[1].role, "user");
        assert_eq!(result[1].content, "User");
    }

    #[test]
    fn test_ollama_message_serialization() {
        let message = OllamaMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            images: None,
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));
        assert!(!json.contains("tool_calls"));
    }

    #[test]
    fn test_ollama_message_deserialization() {
        let json = r#"{"role":"assistant","content":"Hi there"}"#;
        let message: OllamaMessage = serde_json::from_str(json).unwrap();
        assert_eq!(message.role, "assistant");
        assert_eq!(message.content, "Hi there");
        assert!(message.tool_calls.is_none());
    }

    #[test]
    fn test_ollama_response_deserialization() {
        let json = r#"{
            "message": {"role": "assistant", "content": "Response"},
            "done": true,
            "prompt_eval_count": 10,
            "eval_count": 20
        }"#;
        let response: OllamaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.message.content, "Response");
        assert_eq!(response.done, true);
        assert_eq!(response.prompt_eval_count, Some(10));
        assert_eq!(response.eval_count, Some(20));
    }

    #[test]
    fn test_ollama_response_deserialization_minimal() {
        let json = r#"{"message": {"role": "assistant", "content": "Hi"}}"#;
        let response: OllamaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.message.content, "Hi");
        assert_eq!(response.done, false);
        assert!(response.prompt_eval_count.is_none());
        assert!(response.eval_count.is_none());
    }

    #[test]
    fn test_ollama_tool_call_serialization() {
        let tool_call = OllamaToolCall {
            id: Some("call_123".to_string()),
            call_type: Some("function".to_string()),
            function: OllamaFunction {
                name: "get_weather".to_string(),
                arguments: json!({"location": "NYC"}),
            },
        };
        let json = serde_json::to_string(&tool_call).unwrap();
        assert!(json.contains("call_123"));
        assert!(json.contains("function"));
        assert!(json.contains("get_weather"));
        assert!(json.contains("NYC"));
    }

    #[test]
    fn test_ollama_tool_call_deserialization() {
        let json = r#"{
            "id": "call_456",
            "type": "function",
            "function": {
                "name": "search",
                "arguments": {"query": "test"}
            }
        }"#;
        let tool_call: OllamaToolCall = serde_json::from_str(json).unwrap();
        assert_eq!(tool_call.id, Some("call_456".to_string()));
        assert_eq!(tool_call.call_type, Some("function".to_string()));
        assert_eq!(tool_call.function.name, "search");
    }

    #[test]
    fn test_ollama_request_serialization() {
        let request = OllamaRequest {
            model: "llama2".to_string(),
            messages: vec![OllamaMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                tool_calls: None,
                images: None,
            }],
            tools: None,
            stream: Some(false),
            options: Some(OllamaOptions {
                temperature: Some(0.7),
                num_predict: Some(1000),
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("llama2"));
        assert!(json.contains("Hello"));
        assert!(json.contains("0.7"));
        assert!(json.contains("1000"));
    }

    #[test]
    fn test_ollama_options_skip_none() {
        let options = OllamaOptions {
            temperature: Some(0.5),
            num_predict: None,
        };
        let json = serde_json::to_string(&options).unwrap();
        assert!(json.contains("temperature"));
        assert!(!json.contains("num_predict"));
    }

    #[test]
    fn test_ollama_tags_response_deserialization() {
        let json = r#"{
            "models": [
                {"name": "llama2", "size": 1000000},
                {"name": "gemma", "size": 2000000}
            ]
        }"#;
        let response: OllamaTagsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.models.len(), 2);
        assert_eq!(response.models[0].name, "llama2");
        assert_eq!(response.models[1].name, "gemma");
    }

    #[test]
    fn test_ollama_tags_response_empty() {
        let json = r#"{"models": []}"#;
        let response: OllamaTagsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.models.len(), 0);
    }

    #[test]
    fn test_tool_call_builder_default() {
        let builder = ToolCallBuilder::default();
        assert!(builder.id.is_none());
        assert!(builder.name.is_none());
        assert_eq!(builder.arguments, "");
    }

    #[test]
    fn test_map_response_text_only() {
        let client = OllamaClient::new("".to_string(), "http://localhost".to_string());
        let ollama_response = OllamaResponse {
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "Hello user".to_string(),
                tool_calls: None,
                images: None,
            },
            done: true,
            prompt_eval_count: Some(5),
            eval_count: Some(10),
        };

        let response = client.map_response(ollama_response);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock {
                block_type, text, ..
            } => {
                assert_eq!(block_type, "text");
                assert_eq!(text.as_ref().unwrap(), "Hello user");
            }
        }
        assert_eq!(response.usage.as_ref().unwrap().input_tokens, 5);
        assert_eq!(response.usage.as_ref().unwrap().output_tokens, 10);
    }

    #[test]
    fn test_map_response_with_tool_calls() {
        let client = OllamaClient::new("".to_string(), "http://localhost".to_string());
        let ollama_response = OllamaResponse {
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_calls: Some(vec![OllamaToolCall {
                    id: Some("call_1".to_string()),
                    call_type: Some("function".to_string()),
                    function: OllamaFunction {
                        name: "test_tool".to_string(),
                        arguments: json!({"param": "value"}),
                    },
                }]),
                images: None,
            },
            done: true,
            prompt_eval_count: None,
            eval_count: None,
        };

        let response = client.map_response(ollama_response);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock {
                block_type,
                name,
                input,
                id,
                ..
            } => {
                assert_eq!(block_type, "tool_use");
                assert_eq!(id.as_ref().unwrap(), "call_1");
                assert_eq!(name.as_ref().unwrap(), "test_tool");
                assert_eq!(input.as_ref().unwrap()["param"], "value");
            }
        }
        assert!(response.usage.is_none());
    }

    #[test]
    fn test_map_response_text_and_tool_calls() {
        let client = OllamaClient::new("".to_string(), "http://localhost".to_string());
        let ollama_response = OllamaResponse {
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "Let me help".to_string(),
                tool_calls: Some(vec![OllamaToolCall {
                    id: None,
                    call_type: None,
                    function: OllamaFunction {
                        name: "helper".to_string(),
                        arguments: json!({}),
                    },
                }]),
                images: None,
            },
            done: true,
            prompt_eval_count: Some(15),
            eval_count: Some(25),
        };

        let response = client.map_response(ollama_response);
        assert_eq!(response.content.len(), 2);
        assert_eq!(response.content[0].block_type, "text");
        assert_eq!(response.content[1].block_type, "tool_use");
        assert!(response.usage.is_some());
    }

    #[test]
    fn test_map_response_empty_content() {
        let client = OllamaClient::new("".to_string(), "http://localhost".to_string());
        let ollama_response = OllamaResponse {
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_calls: None,
                images: None,
            },
            done: true,
            prompt_eval_count: None,
            eval_count: None,
        };

        let response = client.map_response(ollama_response);
        assert_eq!(response.content.len(), 0);
        assert!(response.usage.is_none());
    }

    #[test]
    fn test_map_response_generates_id_when_missing() {
        let client = OllamaClient::new("".to_string(), "http://localhost".to_string());
        let ollama_response = OllamaResponse {
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_calls: Some(vec![OllamaToolCall {
                    id: None,
                    call_type: None,
                    function: OllamaFunction {
                        name: "test".to_string(),
                        arguments: json!({}),
                    },
                }]),
                images: None,
            },
            done: true,
            prompt_eval_count: None,
            eval_count: None,
        };

        let response = client.map_response(ollama_response);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock { id, .. } => {
                let id_str = id.as_ref().unwrap();
                assert!(id_str.starts_with("ollama_call_"));
            }
        }
    }

    #[test]
    fn test_build_request_basic() {
        let client = OllamaClient::new("key".to_string(), "http://localhost".to_string());
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hi".to_string())],
        }];
        let request = client.build_request("llama2", messages, &[], 1000, 0.7, None, false);

        assert_eq!(request.model, "llama2");
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content, "Hi");
        assert!(request.tools.is_none());
        assert_eq!(request.stream, Some(false));
        assert_eq!(request.options.as_ref().unwrap().temperature, Some(0.7));
        assert_eq!(request.options.as_ref().unwrap().num_predict, Some(1000));
    }

    #[test]
    fn test_build_request_with_system_prompt() {
        let client = OllamaClient::new("key".to_string(), "http://localhost".to_string());
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hi".to_string())],
        }];
        let system = "You are helpful".to_string();
        let request = client.build_request("llama2", messages, &[], 1000, 0.5, Some(&system), true);

        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(request.messages[0].content, "You are helpful");
        assert_eq!(request.messages[1].role, "user");
        assert_eq!(request.stream, Some(true));
    }

    #[test]
    fn test_build_request_ignores_tools() {
        // Note: We can't easily create Tool instances in tests because they require
        // a handler function. However, we can verify that the build_request method
        // always sets tools to None regardless of the tools slice length.
        // This test verifies the behavior with an empty tools slice.
        let client = OllamaClient::new("key".to_string(), "http://localhost".to_string());
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hi".to_string())],
        }];
        let request = client.build_request(
            "llama2",
            messages,
            &[], // Empty tools slice
            1000,
            0.7,
            None,
            false,
        );

        // Ollama provider always sets tools to None
        assert!(request.tools.is_none());
    }

    #[test]
    fn test_ollama_function_definition_serialization() {
        let def = OllamaFunctionDefinition {
            name: "get_weather".to_string(),
            description: Some("Get weather info".to_string()),
            parameters: Some(json!({"type": "object"})),
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("get_weather"));
        assert!(json.contains("Get weather info"));
        assert!(json.contains("object"));
    }

    #[test]
    fn test_ollama_tool_serialization() {
        let tool = OllamaTool {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "search".to_string(),
                description: None,
                parameters: None,
            },
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("search"));
        assert!(!json.contains("description"));
        assert!(!json.contains("parameters"));
    }
}
