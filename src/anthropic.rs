use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

use crate::config::EffortLevel;
use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing, skip_deserializing)]
    pub thought_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource>,
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
            thought_signature: None,
            source: None,
        }
    }

    pub fn image(media_type: String, base64_data: String) -> Self {
        Self {
            block_type: "image".to_string(),
            text: None,
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
            is_error: None,
            thought_signature: None,
            source: Some(ImageSource {
                source_type: "base64".to_string(),
                media_type,
                data: base64_data,
            }),
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
            thought_signature: None,
            source: None,
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
            thought_signature: None,
            source: None,
        }
    }

    pub fn is_image(&self) -> bool {
        self.block_type == "image"
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_budget: Option<u32>,
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
    client: Arc<OnceLock<Client>>,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Arc::new(OnceLock::new()),
            api_key,
            base_url,
        }
    }

    /// Get or initialize the HTTP client
    fn get_client(&self) -> &Client {
        self.client.get_or_init(|| Client::new())
    }

    fn endpoint_candidates(&self) -> Vec<String> {
        let base = self.base_url.trim_end_matches('/');
        let mut endpoints = Vec::new();

        if base.ends_with("/v1") {
            endpoints.push(format!("{}/messages", base));
        } else {
            endpoints.push(format!("{}/v1/messages", base));
        }

        endpoints.push(format!("{}/messages", base));
        endpoints
    }

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
        // Try the standard endpoint first, then fall back to alternatives if needed
        let endpoints = self.endpoint_candidates();

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
                    effort,
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
                effort,
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
        effort: EffortLevel,
        on_content: Arc<dyn Fn(String) + Send + Sync + 'static>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<AnthropicResponse> {
        // Try the standard endpoint first, then fall back to alternatives if needed
        let endpoints = self.endpoint_candidates();

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
                    effort,
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
                effort,
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
        effort: EffortLevel,
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
            reasoning_budget: effort.anthropic_reasoning_budget(),
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
            .get_client()
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
        effort: EffortLevel,
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
            reasoning_budget: effort.anthropic_reasoning_budget(),
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
            .get_client()
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
        let mut streamed_any_text = false; // Track if we've sent any text via callback

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
                        // Normalize CRLF to LF to handle different API line endings
                        let normalized = chunk_str.replace("\r\n", "\n");
                        buffer.push_str(&normalized);

                        // Process complete SSE events
                        while let Some(event_start) = buffer.find("data: ") {
                            if let Some(event_end) = buffer[event_start..].find("\n\n") {
                                let event_end = event_start + event_end + 2;
                                let event_data = &buffer[event_start + 6..event_end - 2]; // Skip "data: " and trailing "\n\n"

                                debug!("Parsed SSE event: {}", event_data);

                                if event_data.trim() == "[DONE]" {
                                    break;
                                }

                                match serde_json::from_str::<StreamEvent>(event_data) {
                                    Ok(event) => {
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
                                                        // Some APIs include initial text in content_block_start
                                                        if let Some(text) = &content_block.text {
                                                            if !text.is_empty() {
                                                                current_content.push_str(text);
                                                                on_content(text.clone());
                                                                streamed_any_text = true;
                                                            }
                                                        }
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
                                                                    thought_signature: None,
                                                                    source: None,
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
                                                        debug!("Streaming text delta: {:?}", text);
                                                        current_content.push_str(&text);
                                                        on_content.as_ref()(text.clone());
                                                        streamed_any_text = true;
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
                                                } else {
                                                    debug!("content_block_delta has no text or partial_json, delta: {:?}", delta);
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
                                                debug!("content_block_stop: pushing text block ({} chars), streamed_any_text={}",
                                                    current_content.len(), streamed_any_text);
                                                content_blocks.push(ContentBlock::text(
                                                    current_content.clone(),
                                                ));
                                                current_content.clear();
                                            }
                                        }
                                        "message_stop" => {
                                            debug!("Stream ended");
                                        }
                                        "message_delta" => {
                                            // Some APIs send text via message_delta events
                                            if let Some(delta) = event.delta {
                                                if let Some(text) = delta.text {
                                                    if !text.is_empty() {
                                                        current_content.push_str(&text);
                                                        on_content(text.clone());
                                                        streamed_any_text = true;
                                                    }
                                                }
                                            }
                                        }
                                        _ => {
                                            debug!("Unknown event type: {}", event.event_type);
                                            // Try to extract text from delta for unknown event types
                                            if let Some(delta) = event.delta {
                                                if let Some(text) = delta.text {
                                                    if !text.is_empty() && current_tool_block.is_none() {
                                                        debug!("Extracting text from unknown event type: {}", text);
                                                        current_content.push_str(&text);
                                                        on_content(text.clone());
                                                        streamed_any_text = true;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if let Some(usage) = event.usage {
                                        usage_info = Some(usage);
                                    }
                                    }
                                    Err(e) => {
                                        debug!("Failed to parse SSE event JSON: {} - data: {}", e, event_data);
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

        // If no content was streamed but there's data in the buffer,
        // the API might have returned a non-streaming JSON response.
        // Try to parse it and emit the content through the callback.
        if content_blocks.is_empty() && !buffer.trim().is_empty() {
            debug!("No SSE events parsed from streaming response, attempting JSON fallback (buffer size: {} bytes)", buffer.len());
            debug!("Buffer content: {}", buffer);
            if let Ok(response) = serde_json::from_str::<AnthropicResponse>(&buffer) {
                debug!("Successfully parsed non-SSE JSON response");
                // Emit any text content through the streaming callback
                for block in &response.content {
                    if block.block_type == "text" {
                        if let Some(text) = &block.text {
                            on_content(text.clone());
                        }
                    }
                }
                return Ok(response);
            }
        }

        // Check if there's text content in the blocks that wasn't streamed via callback.
        // This can happen if the API uses a slightly different event format.
        if !streamed_any_text {
            for block in &content_blocks {
                if block.block_type == "text" {
                    if let Some(text) = &block.text {
                        if !text.is_empty() {
                            debug!("Streaming fallback: emitting unstreamed text content ({} chars)", text.len());
                            on_content(text.clone());
                        }
                    }
                }
            }
        }

        // Return a response with the accumulated content
        Ok(AnthropicResponse {
            content: content_blocks,
            usage: usage_info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolCall;
    use axum::extract::OriginalUri;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    fn client_at(base_url: &str) -> AnthropicClient {
        AnthropicClient::new("test-key".to_string(), base_url.to_string())
    }

    fn text_message(text: &str) -> Vec<Message> {
        vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text(text.to_string())],
        }]
    }

    fn sample_tool() -> Tool {
        Tool {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
            handler: Box::new(|_call: ToolCall| {
                Box::pin(async {
                    Ok(crate::tools::ToolResult {
                        tool_use_id: String::new(),
                        content: String::new(),
                        is_error: false,
                    })
                })
            }),
            metadata: None,
        }
    }

    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn collected_text() -> (Arc<Mutex<Vec<String>>>, Arc<dyn Fn(String) + Send + Sync + 'static>) {
        let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&chunks);
        let callback = Arc::new(move |text: String| {
            sink.lock().expect("sink lock").push(text);
        }) as Arc<dyn Fn(String) + Send + Sync + 'static>;
        (chunks, callback)
    }

    #[test]
    fn text_block_has_only_text_set() {
        let block = ContentBlock::text("hello".to_string());

        assert_eq!(block.block_type, "text");
        assert_eq!(block.text.as_deref(), Some("hello"));
        assert!(block.id.is_none());
        assert!(block.name.is_none());
        assert!(block.input.is_none());
        assert!(block.tool_use_id.is_none());
        assert!(!block.is_image());
    }

    #[test]
    fn tool_use_block_carries_id_name_and_input() {
        let block = ContentBlock::tool_use(
            "call-1".to_string(),
            "read_file".to_string(),
            json!({ "path": "a.txt" }),
        );

        assert_eq!(block.block_type, "tool_use");
        assert_eq!(block.id.as_deref(), Some("call-1"));
        assert_eq!(block.name.as_deref(), Some("read_file"));
        assert_eq!(block.input, Some(json!({ "path": "a.txt" })));
        assert!(!block.is_image());
    }

    #[test]
    fn tool_result_block_carries_result_fields() {
        let block = ContentBlock::tool_result("call-1".to_string(), "file body".to_string(), Some(true));

        assert_eq!(block.block_type, "tool_result");
        assert_eq!(block.tool_use_id.as_deref(), Some("call-1"));
        assert_eq!(block.content.as_deref(), Some("file body"));
        assert_eq!(block.is_error, Some(true));
    }

    #[test]
    fn image_block_wraps_base64_source() {
        let block = ContentBlock::image("image/png".to_string(), "aGk=".to_string());

        assert!(block.is_image());
        let source = block.source.expect("image source");
        assert_eq!(source.source_type, "base64");
        assert_eq!(source.media_type, "image/png");
        assert_eq!(source.data, "aGk=");
    }

    #[test]
    fn text_block_serializes_without_null_fields() {
        let value = serde_json::to_value(ContentBlock::text("hi".to_string())).unwrap();

        assert_eq!(value, json!({ "type": "text", "text": "hi" }));
    }

    #[test]
    fn tool_use_block_roundtrips_through_json() {
        let block = ContentBlock::tool_use(
            "call-9".to_string(),
            "Edit".to_string(),
            json!({ "path": "f.rs", "old_text": "a", "new_text": "b" }),
        );

        let parsed: ContentBlock =
            serde_json::from_value(serde_json::to_value(&block).unwrap()).unwrap();

        assert_eq!(parsed.block_type, "tool_use");
        assert_eq!(parsed.id, block.id);
        assert_eq!(parsed.name, block.name);
        assert_eq!(parsed.input, block.input);
    }

    #[test]
    fn thought_signature_is_never_serialized_or_deserialized() {
        let mut block = ContentBlock::tool_use("call-1".to_string(), "t".to_string(), json!({}));
        block.thought_signature = Some("sig".to_string());

        let value = serde_json::to_value(&block).unwrap();
        assert!(value.get("thought_signature").is_none());

        let from_json: ContentBlock = serde_json::from_value(json!({
            "type": "tool_use",
            "id": "call-2",
            "name": "t",
            "input": {},
            "thoughtSignature": "sig"
        }))
        .unwrap();
        assert!(from_json.thought_signature.is_none());
    }

    #[test]
    fn response_parses_content_and_usage() {
        let response: AnthropicResponse = serde_json::from_str(
            r#"{
                "content": [
                    { "type": "text", "text": "part one" },
                    { "type": "tool_use", "id": "call-1", "name": "Read", "input": { "path": "x" } }
                ],
                "usage": { "input_tokens": 11, "output_tokens": 7 }
            }"#,
        )
        .unwrap();

        assert_eq!(response.content.len(), 2);
        assert_eq!(response.content[0].text.as_deref(), Some("part one"));
        assert_eq!(response.content[1].name.as_deref(), Some("Read"));
        let usage = response.usage.expect("usage");
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
    }

    #[test]
    fn response_parses_without_usage() {
        let response: AnthropicResponse = serde_json::from_str(
            r#"{ "content": [ { "type": "text", "text": "hi" } ] }"#,
        )
        .unwrap();

        assert!(response.usage.is_none());
    }

    #[test]
    fn stream_events_parse_all_relevant_shapes() {
        let start: StreamEvent = serde_json::from_str(
            r#"{ "type": "content_block_start", "content_block": { "type": "tool_use", "id": "call-1", "name": "Read" } }"#,
        )
        .unwrap();
        assert_eq!(start.event_type, "content_block_start");
        assert_eq!(start.content_block.unwrap().block_type, "tool_use");

        let delta: StreamEvent = serde_json::from_str(
            r#"{ "type": "content_block_delta", "delta": { "type": "input_json_delta", "partial_json": "{\"path\":" } }"#,
        )
        .unwrap();
        assert_eq!(
            delta.delta.unwrap().partial_json.as_deref(),
            Some("{\"path\":")
        );

        let usage: StreamEvent = serde_json::from_str(
            r#"{ "type": "message_delta", "usage": { "input_tokens": 3, "output_tokens": 4 } }"#,
        )
        .unwrap();
        assert_eq!(usage.usage.unwrap().output_tokens, 4);
    }

    #[test]
    fn endpoint_candidates_append_messages_path_correctly() {
        let with_version = client_at("https://proxy.example.com/v1");
        assert_eq!(
            with_version.endpoint_candidates(),
            vec![
                "https://proxy.example.com/v1/messages".to_string(),
                "https://proxy.example.com/v1/messages".to_string(),
            ]
        );

        let without_version = client_at("https://proxy.example.com");
        assert_eq!(
            without_version.endpoint_candidates(),
            vec![
                "https://proxy.example.com/v1/messages".to_string(),
                "https://proxy.example.com/messages".to_string(),
            ]
        );

        let trailing_slash = client_at("https://proxy.example.com/v1/");
        assert_eq!(
            trailing_slash.endpoint_candidates()[0],
            "https://proxy.example.com/v1/messages"
        );
    }

    #[tokio::test]
    async fn create_message_returns_cancelled_error_when_flag_set() {
        let client = client_at("http://127.0.0.1:9");
        let cancelled = Arc::new(AtomicBool::new(true));

        let err = client
            .create_message(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                cancelled,
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("CANCELLED"));
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

    async fn non_streaming_handler(
        OriginalUri(uri): OriginalUri,
        Json(payload): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        assert_eq!(uri.path(), "/v1/messages");
        assert_eq!(payload.get("stream").and_then(|v| v.as_bool()), Some(false));

        Json(json!({
            "content": [{ "type": "text", "text": "non-streamed reply" }],
            "usage": { "input_tokens": 5, "output_tokens": 2 }
        }))
    }

    #[tokio::test]
    async fn create_message_parses_non_streaming_response() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(
            Router::new().route("/v1/messages", post(non_streaming_handler)),
        )
        .await;

        let response = client_at(&base_url)
            .create_message(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                Some(&"Be helpful.".to_string()),
                EffortLevel::High,
                no_cancel(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.content[0].text.as_deref(),
            Some("non-streamed reply")
        );
        assert_eq!(response.usage.unwrap().output_tokens, 2);
    }

    #[tokio::test]
    async fn create_message_surfaces_http_errors() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let handler = || async {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                "{\"error\":\"invalid api key\"}".to_string(),
            )
        };
        let base_url = spawn_server(
            Router::new()
                .route("/v1/messages", post(handler))
                .route("/messages", post(handler)),
        )
        .await;

        let err = client_at(&base_url)
            .create_message(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                no_cancel(),
            )
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("401"), "unexpected error: {}", message);
        assert!(message.contains("invalid api key"));
    }

    #[tokio::test]
    async fn create_message_reports_error_envelopes_with_success_false() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let handler = || async { Json(json!({ "code": 401, "msg": "bad key", "success": false })) };
        let base_url = spawn_server(
            Router::new()
                .route("/v1/messages", post(handler))
                .route("/messages", post(handler)),
        )
        .await;

        let err = client_at(&base_url)
            .create_message(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                no_cancel(),
            )
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("API Error (HTTP 401)") && message.contains("bad key"),
            "unexpected error: {}",
            message
        );
    }

    #[tokio::test]
    async fn stream_accumulates_text_deltas() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                let body = concat!(
                    "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
                    "data: {\"type\":\"content_block_stop\"}\n\n",
                    "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":4,\"output_tokens\":2}}\n\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
                    .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                no_cancel(),
            )
            .await
            .unwrap();

        assert_eq!(chunks.lock().unwrap().join(""), "Hello");
        assert_eq!(response.content.len(), 1);
        assert_eq!(response.content[0].text.as_deref(), Some("Hello"));
        let usage = response.usage.expect("streamed usage");
        assert_eq!((usage.input_tokens, usage.output_tokens), (4, 2));
    }

    #[tokio::test]
    async fn stream_assembles_tool_use_from_partial_json() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                let body = concat!(
                    "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"tool_use\",\"id\":\"call-42\",\"name\":\"read_file\"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"main.rs\\\"}\"}}\n\n",
                    "data: {\"type\":\"content_block_stop\"}\n\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
                    .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "claude-opus-5",
                text_message("read main.rs"),
                &[sample_tool()],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                no_cancel(),
            )
            .await
            .unwrap();

        assert!(chunks.lock().unwrap().is_empty(), "no text should stream");
        assert_eq!(response.content.len(), 1);
        let tool_block = &response.content[0];
        assert_eq!(tool_block.block_type, "tool_use");
        assert_eq!(tool_block.id.as_deref(), Some("call-42"));
        assert_eq!(tool_block.name.as_deref(), Some("read_file"));
        assert_eq!(tool_block.input, Some(json!({ "path": "main.rs" })));
    }

    #[tokio::test]
    async fn stream_accepts_crlf_line_endings() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                let body = "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\"}}\r\n\r\n\
                            data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"CRLF ok\"}}\r\n\r\n\
                            data: {\"type\":\"content_block_stop\"}\r\n\r\n";
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
                    .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                no_cancel(),
            )
            .await
            .unwrap();

        assert_eq!(chunks.lock().unwrap().join(""), "CRLF ok");
        assert_eq!(response.content[0].text.as_deref(), Some("CRLF ok"));
    }

    #[tokio::test]
    async fn stream_falls_back_to_plain_json_body() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                // Not SSE: some proxies return a plain JSON envelope even when streaming
                Json(json!({
                    "content": [{ "type": "text", "text": "json fallback" }]
                }))
                .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "claude-opus-5",
                text_message("hi"),
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                no_cancel(),
            )
            .await
            .unwrap();

        assert_eq!(chunks.lock().unwrap().join(""), "json fallback");
        assert_eq!(response.content[0].text.as_deref(), Some("json fallback"));
    }
}
