use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::tools::{Tool, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub input: Option<Value>,
    pub tool_use_id: Option<String>,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ContentBlock {
    pub fn text(text: String) -> Self {
        Self {
            block_type: "text".to_string(),
            text: Some(text),
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
            is_error: None,
        }
    }

    pub fn tool_use(id: String, name: String, input: Value) -> Self {
        Self {
            block_type: "tool_use".to_string(),
            text: None,
            id: Some(id),
            name: Some(name),
            input: Some(input),
            tool_use_id: None,
            content: None,
            is_error: None,
        }
    }

    pub fn tool_result(tool_use_id: String, content: String, is_error: Option<bool>) -> Self {
        Self {
            block_type: "tool_result".to_string(),
            text: None,
            id: None,
            name: None,
            input: None,
            tool_use_id: Some(tool_use_id),
            content: Some(content),
            is_error,
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDefinition>>,
    stream: Option<bool>,
    system: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub struct StreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: Option<u32>,
    pub delta: Option<StreamDelta>,
    pub content_block: Option<ContentBlock>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    pub text: Option<String>,
    pub partial_json: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub struct AnthropicClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url,
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
        // Try the standard endpoint first, then fall back to alternatives if needed
        let endpoints = vec![
            format!("{}/v1/messages", self.base_url),
            format!("{}/messages", self.base_url),
            format!("{}/anthropic/v1/messages", self.base_url),
        ];

        for endpoint in endpoints.iter() {
            match self
                .try_endpoint(
                    endpoint,
                    model,
                    &messages,
                    tools,
                    max_tokens,
                    temperature,
                    system_prompt,
                    cancellation_flag.clone(),
                )
                .await
            {
                Ok(response) => {
                    return Ok(response);
                }
                Err(_) => {
                    // Continue to the next endpoint
                    continue;
                }
            }
        }

        // If all endpoints failed, return the error from the last attempt
        let last_endpoint = &endpoints[endpoints.len() - 1];
        return self
            .try_endpoint(
                last_endpoint,
                model,
                &messages,
                tools,
                max_tokens,
                temperature,
                system_prompt,
                cancellation_flag.clone(),
            )
            .await;
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
        // Try the standard endpoint first, then fall back to alternatives if needed
        let endpoints = vec![
            format!("{}/v1/messages", self.base_url),
            format!("{}/messages", self.base_url),
            format!("{}/anthropic/v1/messages", self.base_url),
        ];

        for endpoint in endpoints.iter() {
            match self
                .try_endpoint_stream(
                    endpoint,
                    model,
                    &messages,
                    tools,
                    max_tokens,
                    temperature,
                    system_prompt,
                    on_content.clone(),
                    cancellation_flag.clone(),
                )
                .await
            {
                Ok(response) => {
                    return Ok(response);
                }
                Err(_) => {
                    // Continue to the next endpoint
                    continue;
                }
            }
        }

        // If all endpoints failed, return the error from the last attempt
        let last_endpoint = &endpoints[endpoints.len() - 1];
        return self
            .try_endpoint_stream(
                last_endpoint,
                model,
                &messages,
                tools,
                max_tokens,
                temperature,
                system_prompt,
                on_content.clone(),
                cancellation_flag.clone(),
            )
            .await;
    }

    async fn try_endpoint(
        &self,
        endpoint: &str,
        model: &str,
        messages: &[Message],
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<AnthropicResponse> {
        let tool_definitions = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| ToolDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect(),
            )
        };

        let request = AnthropicRequest {
            model: model.to_string(),
            max_tokens,
            temperature,
            messages: messages.to_vec(),
            tools: tool_definitions,
            stream: Some(false),
            system: system_prompt.cloned(),
        };

        // Log outgoing request (debug level only)
        debug!("Sending API request to endpoint: {}", endpoint);
        debug!("Request body: {}", serde_json::to_string_pretty(&request)?);
        debug!("Sending message to model: {}", model);
        if let Some(system_prompt) = system_prompt {
            debug!("Using system prompt: {}", system_prompt);
        }

        // Check for cancellation before making the request
        if cancellation_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("CANCELLED"));
        }

        let response = self
            .client
            .post(endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!("API Request Failed:");
            error!("  Endpoint: {}", endpoint);
            error!("  Status: {}", status);
            error!("  Model: {}", model);
            error!("  Error Response: {}", error_text);
            error!("  Request headers: x-api-key=[REDACTED], anthropic-version=2023-06-01");
            return Err(anyhow::anyhow!("API error: {} - {}", status, error_text));
        }

        // Get the response text
        let response_text = response.text().await?;

        // Log incoming response (debug level only)
        debug!("Received API response with status: {}", status);
        debug!("Response body: {}", response_text);

        // Try to parse the response
        match serde_json::from_str::<AnthropicResponse>(&response_text) {
            Ok(anthropic_response) => {
                debug!("Successfully received response from API");
                if let Some(usage) = &anthropic_response.usage {
                    debug!(
                        "Token usage - Input: {}, Output: {}",
                        usage.input_tokens, usage.output_tokens
                    );
                }
                Ok(anthropic_response)
            }
            Err(e) => {
                // Try to parse as a generic JSON to handle error responses
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(value) => {
                        // Check if this is an error response with specific fields
                        if let (Some(code), Some(msg), Some(success)) = (
                            value.get("code").and_then(|v| v.as_u64()),
                            value.get("msg").and_then(|v| v.as_str()),
                            value.get("success").and_then(|v| v.as_bool()),
                        ) {
                            if !success {
                                return Err(anyhow::anyhow!("API Error (HTTP {}): {} - This suggests the endpoint or authentication is incorrect", code, msg));
                            }
                        }

                        Err(anyhow::anyhow!(
                            "Failed to parse API response: {} - Invalid response format",
                            e
                        ))
                    }
                    Err(_) => Err(anyhow::anyhow!("Invalid JSON response from API: {}", e)),
                }
            }
        }
    }

    async fn try_endpoint_stream(
        &self,
        endpoint: &str,
        model: &str,
        messages: &[Message],
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        on_content: Arc<dyn Fn(String) + Send + Sync + 'static>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<AnthropicResponse> {
        let tool_definitions = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| ToolDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect(),
            )
        };

        let request = AnthropicRequest {
            model: model.to_string(),
            max_tokens,
            temperature,
            messages: messages.to_vec(),
            tools: tool_definitions,
            stream: Some(true),
            system: system_prompt.cloned(),
        };

        // Log outgoing streaming request (debug level only)
        debug!("Sending streaming API request to endpoint: {}", endpoint);
        debug!("Request body: {}", serde_json::to_string_pretty(&request)?);
        if let Some(system_prompt) = system_prompt {
            debug!("Using system prompt: {}", system_prompt);
        }

        // Check for cancellation before making the request
        if cancellation_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("CANCELLED"));
        }

        let response = self
            .client
            .post(endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!("API Request Failed:");
            error!("  Endpoint: {}", endpoint);
            error!("  Status: {}", status);
            error!("  Model: {}", model);
            error!("  Error Response: {}", error_text);
            error!("  Request headers: x-api-key=[REDACTED], anthropic-version=2023-06-01");
            return Err(anyhow::anyhow!("API error: {} - {}", status, error_text));
        }

        // Process the streaming response
        let mut buffer = String::new();
        let mut content_blocks = Vec::new();
        let mut current_content = String::new();
        let mut usage_info = None;
        let mut current_tool_block: Option<ContentBlock> = None;

        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            // Check for cancellation before processing each chunk
            if cancellation_flag.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("CANCELLED"));
            }

            match chunk_result {
                Ok(chunk) => {
                    if let Ok(chunk_str) = std::str::from_utf8(&chunk) {
                        debug!("Received chunk: {}", chunk_str);
                        buffer.push_str(chunk_str);

                        // Process complete SSE events
                        while let Some(event_start) = buffer.find("data: ") {
                            if let Some(event_end) = buffer[event_start..].find("\n\n") {
                                let event_end = event_start + event_end + 2;
                                let event_data = &buffer[event_start + 6..event_end - 2]; // Skip "data: " and trailing "\n\n"

                                debug!("Parsed SSE event: {}", event_data);

                                if event_data.trim() == "[DONE]" {
                                    break;
                                }

                                if let Ok(event) = serde_json::from_str::<StreamEvent>(event_data) {
                                    debug!(
                                        "Received stream event: type={}, delta={:?}",
                                        event.event_type, event.delta
                                    );
                                    match event.event_type.as_str() {
                                        "content_block_start" => {
                                            debug!("Starting new content block");
                                            if let Some(content_block) = event.content_block {
                                                debug!(
                                                    "Content block from event: {:?}",
                                                    content_block
                                                );
                                                match content_block.block_type.as_str() {
                                                    "text" => {
                                                        current_content.clear();
                                                    }
                                                    "tool_use" => {
                                                        debug!(
                                                            "Setting tool_use block: {:?}",
                                                            content_block
                                                        );
                                                        current_tool_block = Some(content_block);
                                                    }
                                                    _ => {
                                                        debug!(
                                                            "Unknown block type: {}",
                                                            content_block.block_type
                                                        );
                                                    }
                                                }
                                            } else if let Some(delta) = event.delta {
                                                // Handle tool_use blocks from delta
                                                debug!("Delta in content_block_start: {:?}", delta);
                                                if let Some(block_type) = delta.delta_type {
                                                    match block_type.as_str() {
                                                        "tool_use" => {
                                                            debug!("Creating tool_use block from delta: id={:?}, name={:?}", delta.id, delta.name);
                                                            current_tool_block =
                                                                Some(ContentBlock {
                                                                    block_type: "tool_use"
                                                                        .to_string(),
                                                                    text: None,
                                                                    id: delta.id,
                                                                    name: delta.name,
                                                                    input: None,
                                                                    tool_use_id: None,
                                                                    content: None,
                                                                    is_error: None,
                                                                });
                                                        }
                                                        _ => {
                                                            debug!(
                                                                "Unknown delta block type: {}",
                                                                block_type
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        "content_block_delta" => {
                                            if let Some(delta) = event.delta {
                                                if let Some(text) = delta.text {
                                                    if current_tool_block.is_some() {
                                                        // We're in a tool_use block, but got text - this shouldn't happen
                                                        debug!("Unexpected text in tool_use block");
                                                    } else {
                                                        // Regular text content
                                                        current_content.push_str(&text);
                                                        on_content.as_ref()(text.clone());
                                                    }
                                                } else if let Some(partial_json) =
                                                    delta.partial_json
                                                {
                                                    // Handle tool input JSON
                                                    debug!(
                                                        "Received partial_json: {}",
                                                        partial_json
                                                    );
                                                    if let Some(ref mut tool_block) =
                                                        current_tool_block
                                                    {
                                                        if let Some(Value::String(mut existing)) =
                                                            tool_block.input.take()
                                                        {
                                                            // Append to existing JSON string
                                                            existing.push_str(&partial_json);
                                                            debug!(
                                                                "Appending to existing JSON: {}",
                                                                existing
                                                            );
                                                            tool_block.input =
                                                                Some(Value::String(existing));
                                                        } else {
                                                            // Start new JSON string (replace any existing non-string or create new)
                                                            debug!(
                                                                "Starting new JSON string: {}",
                                                                partial_json
                                                            );
                                                            tool_block.input =
                                                                Some(Value::String(partial_json));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        "content_block_stop" => {
                                            if let Some(mut tool_block) = current_tool_block.take()
                                            {
                                                debug!("Finalizing tool block: {:?}", tool_block);
                                                // Finalize tool_use block
                                                // Parse the accumulated JSON string into a proper JSON value
                                                if let Some(Value::String(ref json_str)) =
                                                    tool_block.input
                                                {
                                                    debug!("Parsing JSON string: {}", json_str);
                                                    match serde_json::from_str::<Value>(&json_str) {
                                                        Ok(parsed_json) => {
                                                            debug!(
                                                                "Successfully parsed JSON: {:?}",
                                                                parsed_json
                                                            );
                                                            tool_block.input = Some(parsed_json);
                                                        }
                                                        Err(e) => {
                                                            debug!("Failed to parse tool JSON: {}, keeping as string", e);
                                                            // Keep as string if parsing fails
                                                        }
                                                    }
                                                }
                                                debug!("Finalized tool block: {:?}", tool_block);
                                                content_blocks.push(tool_block);
                                            } else if !current_content.is_empty() {
                                                content_blocks.push(ContentBlock::text(
                                                    current_content.clone(),
                                                ));
                                                current_content.clear();
                                            }
                                        }
                                        "message_stop" => {
                                            debug!("Stream ended");
                                        }
                                        _ => {
                                            debug!("Unknown event type: {}", event.event_type);
                                        }
                                    }

                                    if let Some(usage) = event.usage {
                                        usage_info = Some(usage);
                                    }
                                }

                                buffer = buffer[event_end..].to_string();
                            } else {
                                break; // Incomplete event, wait for more data
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
            }
        }

        // Return a response with the accumulated content
        Ok(AnthropicResponse {
            content: content_blocks,
            usage: usage_info,
        })
    }

    pub fn convert_tool_calls(&self, content_blocks: &[ContentBlock]) -> Vec<ToolCall> {
        debug!(
            "Converting tool calls from {} content blocks",
            content_blocks.len()
        );
        let tool_calls: Vec<ToolCall> = content_blocks
            .iter()
            .filter_map(|block| {
                if block.block_type == "tool_use" {
                    debug!("Found tool_use block: {:?}", block);
                    let tool_call = ToolCall {
                        id: block.id.as_ref().unwrap_or(&String::new()).clone(),
                        name: block.name.as_ref().unwrap_or(&String::new()).clone(),
                        arguments: block.input.as_ref().unwrap_or(&Value::Null).clone(),
                    };
                    debug!("Converted to tool call: {:?}", tool_call);
                    Some(tool_call)
                } else {
                    None
                }
            })
            .collect();
        tool_calls
    }

    pub fn create_response_content(&self, content_blocks: &[ContentBlock]) -> String {
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
}
