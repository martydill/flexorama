use crate::anthropic::{AnthropicResponse, ContentBlock, Message, Usage};
use crate::config::EffortLevel;
use crate::tools::Tool;
use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "max_completion_tokens")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum OpenAIContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAIImageUrl },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIImageUrl {
    url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<OpenAIContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunctionDefinition,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionDefinition {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamResponse {
    choices: Vec<OpenAIStreamChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIStreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<OpenAIStreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub struct OpenAIClient {
    client: Arc<OnceLock<Client>>,
    api_key: String,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Arc::new(OnceLock::new()),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Get or initialize the HTTP client
    fn get_client(&self) -> &Client {
        self.client.get_or_init(|| Client::new())
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
            effort,
            false,
        );
        let endpoint = format!("{}/chat/completions", self.base_url);

        debug!("Sending OpenAI request to {}", endpoint);
        let response = self
            .get_client()
            .post(&endpoint)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("OpenAI request failed: {}", status);
            error!("Response: {}", response_text);
            return Err(anyhow::anyhow!(
                "OpenAI API error: {} - {}",
                status,
                response_text
            ));
        }

        debug!("OpenAI raw response: {}", response_text);
        let parsed: OpenAIResponse = serde_json::from_str(&response_text).map_err(|e| {
            error!("Failed to parse OpenAI response: {}", e);
            e
        })?;

        Ok(self.map_response(parsed))
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
            effort,
            true,
        );
        let endpoint = format!("{}/chat/completions", self.base_url);

        debug!("Sending OpenAI streaming request to {}", endpoint);
        let response = self
            .get_client()
            .post(&endpoint)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!("OpenAI streaming request failed: {}", status);
            error!("Response: {}", error_text);
            return Err(anyhow::anyhow!(
                "OpenAI API error: {} - {}",
                status,
                error_text
            ));
        }

        let mut buffer = String::new();
        let mut content = String::new();
        let mut tool_calls: BTreeMap<usize, ToolCallBuilder> = BTreeMap::new();
        let mut usage_info = None;
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            if cancellation_flag.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("CANCELLED"));
            }

            match chunk_result {
                Ok(chunk) => {
                    if let Ok(chunk_str) = std::str::from_utf8(&chunk) {
                        // Normalize CRLF to LF to handle different API line endings
                        let normalized = chunk_str.replace("\r\n", "\n");
                        buffer.push_str(&normalized);

                        while let Some(event_start) = buffer.find("data: ") {
                            if let Some(event_end) = buffer[event_start..].find("\n\n") {
                                let event_end = event_start + event_end + 2;
                                let event_data = buffer[event_start + 6..event_end - 2].trim();

                                if event_data == "[DONE]" {
                                    buffer = buffer[event_end..].to_string();
                                    break;
                                }

                                if let Ok(event) =
                                    serde_json::from_str::<OpenAIStreamResponse>(event_data)
                                {
                                    if let Some(choice) = event.choices.first() {
                                        if let Some(text) = &choice.delta.content {
                                            content.push_str(text);
                                            on_content(text.clone());
                                        }

                                        if let Some(tool_deltas) = &choice.delta.tool_calls {
                                            for delta in tool_deltas {
                                                let index = match delta.index {
                                                    Some(index) => index,
                                                    None => continue,
                                                };
                                                let entry = tool_calls.entry(index).or_default();
                                                if let Some(id) = &delta.id {
                                                    entry.id = Some(id.clone());
                                                }
                                                if let Some(call_type) = &delta.call_type {
                                                    if call_type != "function" {
                                                        continue;
                                                    }
                                                }
                                                if let Some(function) = &delta.function {
                                                    if let Some(name) = &function.name {
                                                        entry.name = Some(name.clone());
                                                    }
                                                    if let Some(arguments) = &function.arguments {
                                                        entry.arguments.push_str(arguments);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if event.usage.is_some() {
                                        usage_info = event.usage;
                                    }
                                }

                                buffer = buffer[event_end..].to_string();
                            } else {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
            }
        }

        let mut content_blocks = Vec::new();
        if !content.is_empty() {
            content_blocks.push(ContentBlock::text(content));
        }

        for (_index, builder) in tool_calls {
            let id = builder
                .id
                .unwrap_or_else(|| format!("openai_call_{}", Uuid::new_v4().simple()));
            let name = builder.name.unwrap_or_else(|| "tool".to_string());
            let input = parse_arguments(&builder.arguments);
            content_blocks.push(ContentBlock::tool_use(id, name, input));
        }

        let usage = usage_info.map(|usage| Usage {
            input_tokens: usage.prompt_tokens.unwrap_or(0),
            output_tokens: usage.completion_tokens.unwrap_or(0),
        });

        Ok(AnthropicResponse {
            content: content_blocks,
            usage,
        })
    }

    fn build_request(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
        effort: EffortLevel,
        stream: bool,
    ) -> OpenAIRequest {
        let openai_messages = map_messages(messages, system_prompt);

        let tool_defs = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|tool| OpenAITool {
                        tool_type: "function".to_string(),
                        function: OpenAIFunctionDefinition {
                            name: tool.name.clone(),
                            description: Some(tool.description.clone()),
                            parameters: Some(tool.input_schema.clone()),
                        },
                    })
                    .collect(),
            )
        };

        // Only include reasoning_effort for o1/o3 models, not for standard models
        let reasoning_effort = if model.starts_with("o1") || model.starts_with("o3") {
            Some(effort.openai_reasoning_effort().to_string())
        } else {
            None
        };

        OpenAIRequest {
            model: model.to_string(),
            messages: openai_messages,
            max_tokens: Some(max_tokens),
            temperature: Some(temperature),
            tools: tool_defs,
            stream: Some(stream),
            reasoning_effort,
        }
    }

    fn map_response(&self, response: OpenAIResponse) -> AnthropicResponse {
        let mut content_blocks = Vec::new();

        if let Some(choice) = response.choices.into_iter().next() {
            if let Some(content) = choice.message.content {
                match content {
                    OpenAIContent::Text(text) => {
                        if !text.is_empty() {
                            content_blocks.push(ContentBlock::text(text));
                        }
                    }
                    OpenAIContent::Parts(parts) => {
                        for part in parts {
                            if let OpenAIContentPart::Text { text } = part {
                                if !text.is_empty() {
                                    content_blocks.push(ContentBlock::text(text));
                                }
                            }
                        }
                    }
                }
            }

            if let Some(tool_calls) = choice.message.tool_calls {
                for call in tool_calls {
                    let input = parse_arguments(&call.function.arguments);
                    content_blocks.push(ContentBlock::tool_use(call.id, call.function.name, input));
                }
            }
        }

        let usage = response.usage.map(|usage| Usage {
            input_tokens: usage.prompt_tokens.unwrap_or(0),
            output_tokens: usage.completion_tokens.unwrap_or(0),
        });

        AnthropicResponse {
            content: content_blocks,
            usage,
        }
    }
}

fn map_messages(messages: Vec<Message>, system_prompt: Option<&String>) -> Vec<OpenAIMessage> {
    let mut openai_messages = Vec::new();

    if let Some(prompt) = system_prompt {
        openai_messages.push(OpenAIMessage {
            role: "system".to_string(),
            content: Some(OpenAIContent::Text(prompt.clone())),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in messages {
        let mut content_parts: Vec<OpenAIContentPart> = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();

        for block in &message.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        content_parts.push(OpenAIContentPart::Text { text: text.clone() });
                    }
                }
                "image" => {
                    if let Some(source) = &block.source {
                        let data_url = format!("data:{};base64,{}", source.media_type, source.data);
                        content_parts.push(OpenAIContentPart::ImageUrl {
                            image_url: OpenAIImageUrl { url: data_url },
                        });
                    }
                }
                "tool_use" => {
                    let id = block
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("openai_call_{}", Uuid::new_v4().simple()));
                    let name = block.name.clone().unwrap_or_else(|| "tool".to_string());
                    let input = block
                        .input
                        .as_ref()
                        .map(|value| match value {
                            Value::String(text) => text.clone(),
                            other => {
                                serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string())
                            }
                        })
                        .unwrap_or_else(|| "{}".to_string());
                    tool_calls.push(OpenAIToolCall {
                        id,
                        call_type: "function".to_string(),
                        function: OpenAIFunctionCall {
                            name,
                            arguments: input,
                        },
                    });
                }
                "tool_result" => {
                    if let Some(tool_use_id) = &block.tool_use_id {
                        let content = block.content.clone().unwrap_or_default();
                        tool_results.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(OpenAIContent::Text(content)),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                }
                _ => {}
            }
        }

        if !tool_calls.is_empty() || !content_parts.is_empty() {
            let content = if content_parts.is_empty() {
                None
            } else if content_parts.len() == 1 {
                if let OpenAIContentPart::Text { text } = &content_parts[0] {
                    Some(OpenAIContent::Text(text.clone()))
                } else {
                    Some(OpenAIContent::Parts(content_parts))
                }
            } else {
                Some(OpenAIContent::Parts(content_parts))
            };

            openai_messages.push(OpenAIMessage {
                role: message.role.clone(),
                content,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
            });
        }

        if !tool_results.is_empty() {
            openai_messages.extend(tool_results);
        }
    }

    openai_messages
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
    use crate::tools::{ToolCall, ToolResult};
    use serde_json::json;

    fn client() -> OpenAIClient {
        OpenAIClient::new("test-key".to_string(), "https://api.openai.test/v1".to_string())
    }

    fn message(role: &str, blocks: Vec<ContentBlock>) -> Message {
        Message {
            role: role.to_string(),
            content: blocks,
        }
    }

    fn tool() -> Tool {
        Tool {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
            handler: Box::new(|_call: ToolCall| {
                Box::pin(async {
                    Ok(ToolResult {
                        tool_use_id: String::new(),
                        content: String::new(),
                        is_error: false,
                    })
                })
            }),
            metadata: None,
        }
    }

    #[test]
    fn parse_arguments_handles_empty_and_valid_json() {
        assert_eq!(parse_arguments(""), json!({}));
        assert_eq!(parse_arguments("   "), json!({}));
        assert_eq!(
            parse_arguments(r#"{"path": "main.rs"}"#),
            json!({ "path": "main.rs" })
        );
    }

    #[test]
    fn parse_arguments_falls_back_to_raw_string_for_invalid_json() {
        assert_eq!(
            parse_arguments("not json at all"),
            Value::String("not json at all".to_string())
        );
    }

    #[test]
    fn map_messages_prepends_system_prompt() {
        let mapped = map_messages(
            vec![message("user", vec![ContentBlock::text("hello".to_string())])],
            Some(&"You are terse.".to_string()),
        );

        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].role, "system");
        match &mapped[0].content {
            Some(OpenAIContent::Text(text)) => assert_eq!(text, "You are terse."),
            other => panic!("expected text content, got {:?}", other),
        }
        assert_eq!(mapped[1].role, "user");
    }

    #[test]
    fn map_messages_collapses_single_text_part_to_plain_content() {
        let mapped = map_messages(
            vec![message("user", vec![ContentBlock::text("just text".to_string())])],
            None,
        );

        assert_eq!(mapped.len(), 1);
        match &mapped[0].content {
            Some(OpenAIContent::Text(text)) => assert_eq!(text, "just text"),
            other => panic!("expected text content, got {:?}", other),
        }
        assert!(mapped[0].tool_calls.is_none());
    }

    #[test]
    fn map_messages_sends_images_as_data_urls() {
        let image = ContentBlock::image("image/png".to_string(), "aGk=".to_string());
        let mapped = map_messages(
            vec![message(
                "user",
                vec![ContentBlock::text("look".to_string()), image],
            )],
            None,
        );

        assert_eq!(mapped.len(), 1);
        match &mapped[0].content {
            Some(OpenAIContent::Parts(parts)) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], OpenAIContentPart::Text { .. }));
                match &parts[1] {
                    OpenAIContentPart::ImageUrl { image_url } => {
                        assert_eq!(image_url.url, "data:image/png;base64,aGk=");
                    }
                    other => panic!("expected image url part, got {:?}", other),
                }
            }
            other => panic!("expected parts content, got {:?}", other),
        }
    }

    #[test]
    fn map_messages_converts_tool_use_blocks_to_tool_calls() {
        let tool_use = ContentBlock::tool_use(
            "call-1".to_string(),
            "read_file".to_string(),
            json!({ "path": "main.rs" }),
        );
        let mapped = map_messages(vec![message("assistant", vec![tool_use])], None);

        assert_eq!(mapped.len(), 1);
        let calls = mapped[0].tool_calls.as_ref().expect("tool calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].call_type, "function");
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments, r#"{"path":"main.rs"}"#);
        assert!(mapped[0].content.is_none(), "tool-only message has no content");
    }

    #[test]
    fn map_messages_keeps_raw_string_tool_input() {
        let mut tool_use = ContentBlock::tool_use("call-1".to_string(), "t".to_string(), json!({}));
        tool_use.input = Some(Value::String("{\"path\":\"x\"}".to_string()));

        let mapped = map_messages(vec![message("assistant", vec![tool_use])], None);
        let calls = mapped[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].function.arguments, "{\"path\":\"x\"}");
    }

    #[test]
    fn map_messages_emits_tool_results_as_tool_role_messages() {
        let result = ContentBlock::tool_result("call-1".to_string(), "file contents".to_string(), None);
        let mapped = map_messages(vec![message("user", vec![result])], None);

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].role, "tool");
        assert_eq!(mapped[0].tool_call_id.as_deref(), Some("call-1"));
        match &mapped[0].content {
            Some(OpenAIContent::Text(text)) => assert_eq!(text, "file contents"),
            other => panic!("expected text content, got {:?}", other),
        }
    }

    #[test]
    fn map_messages_drops_tool_results_without_ids_and_unknown_blocks() {
        let orphan = ContentBlock {
            block_type: "tool_result".to_string(),
            text: None,
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: Some("no id".to_string()),
            is_error: None,
            thought_signature: None,
            source: None,
        };
        let unknown = ContentBlock {
            block_type: "mystery".to_string(),
            text: Some("ignored".to_string()),
            ..orphan.clone()
        };

        let mapped = map_messages(
            vec![message("user", vec![orphan, unknown])],
            None,
        );

        assert!(mapped.is_empty());
    }

    #[test]
    fn build_request_omits_reasoning_effort_for_standard_models() {
        let request = client().build_request(
            "gpt-4o",
            vec![message("user", vec![ContentBlock::text("hi".to_string())])],
            &[],
            512,
            0.2,
            None,
            EffortLevel::High,
            false,
        );

        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire.get("model"), Some(&json!("gpt-4o")));
        assert!(
            wire.get("reasoning_effort").is_none(),
            "standard models must not send reasoning_effort"
        );
        assert_eq!(wire.get("max_completion_tokens"), Some(&json!(512)));
        let temperature = wire.get("temperature").unwrap().as_f64().unwrap();
        assert!((temperature - 0.2).abs() < 1e-6);
        assert_eq!(wire.get("stream"), Some(&json!(false)));
        assert!(wire.get("tools").is_none());
    }

    #[test]
    fn build_request_includes_reasoning_effort_for_reasoning_models() {
        for model in ["o1", "o1-mini", "o3", "o3-mini"] {
            let request = client().build_request(
                model,
                vec![],
                &[],
                256,
                0.5,
                None,
                EffortLevel::Low,
                true,
            );

            let wire = serde_json::to_value(&request).unwrap();
            assert_eq!(
                wire.get("reasoning_effort"),
                Some(&json!("low")),
                "expected reasoning_effort for {}",
                model
            );
            assert_eq!(wire.get("stream"), Some(&json!(true)));
        }
    }

    #[test]
    fn build_request_maps_tool_definitions() {
        let request = client().build_request(
            "gpt-4o",
            vec![],
            &[tool()],
            128,
            0.7,
            None,
            EffortLevel::Medium,
            false,
        );

        let wire = serde_json::to_value(&request).unwrap();
        let tools = wire.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "read_file");
        assert_eq!(tools[0]["function"]["description"], "Read a file");
    }

    #[test]
    fn map_response_extracts_text_tool_calls_and_usage() {
        let parsed: OpenAIResponse = serde_json::from_value(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "answer",
                    "tool_calls": [{
                        "id": "call-7",
                        "type": "function",
                        "function": { "name": "read_file", "arguments": "{\"path\":\"x\"}" }
                    }]
                }
            }],
            "usage": { "prompt_tokens": 9, "completion_tokens": 4 }
        }))
        .unwrap();

        let mapped = client().map_response(parsed);

        assert_eq!(mapped.content.len(), 2);
        assert_eq!(mapped.content[0].text.as_deref(), Some("answer"));
        assert_eq!(mapped.content[1].block_type, "tool_use");
        assert_eq!(mapped.content[1].id.as_deref(), Some("call-7"));
        assert_eq!(mapped.content[1].input, Some(json!({ "path": "x" })));
        let usage = mapped.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (9, 4));
    }

    #[test]
    fn map_response_handles_parts_and_defaulted_usage() {
        let parsed: OpenAIResponse = serde_json::from_value(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "first" },
                        { "type": "image_url", "image_url": { "url": "data:image/png;base64,aGk=" } },
                        { "type": "text", "text": "second" }
                    ]
                }
            }],
            "usage": {}
        }))
        .unwrap();

        let mapped = client().map_response(parsed);

        let texts: Vec<&str> = mapped
            .content
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect();
        assert_eq!(texts, vec!["first", "second"]);
        let usage = mapped.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (0, 0));
    }

    #[test]
    fn map_response_skips_empty_text() {
        let parsed: OpenAIResponse = serde_json::from_value(json!({
            "choices": [{ "message": { "role": "assistant", "content": "" } }]
        }))
        .unwrap();

        let mapped = client().map_response(parsed);
        assert!(mapped.content.is_empty());
        assert!(mapped.usage.is_none());
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use crate::tools::ToolCall;
    use axum::response::IntoResponse;
    use serde_json::json;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    fn client_at(base_url: &str) -> OpenAIClient {
        OpenAIClient::new("test-key".to_string(), base_url.to_string())
    }

    fn user_message(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text(text.to_string())],
        }
    }

    async fn spawn_server(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base_url
    }

    fn collected_text() -> (Arc<Mutex<Vec<String>>>, Arc<dyn Fn(String) + Send + Sync + 'static>) {
        let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&chunks);
        let callback = Arc::new(move |text: String| {
            sink.lock().unwrap().push(text);
        }) as Arc<dyn Fn(String) + Send + Sync + 'static>;
        (chunks, callback)
    }

    #[tokio::test]
    async fn stream_accumulates_text_and_usage() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/chat/completions",
            post(|| async {
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2}}\n\n",
                    "data: [DONE]\n\n"
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body.to_string(),
                )
                    .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "gpt-4o",
                vec![user_message("hi")],
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();

        assert_eq!(chunks.lock().unwrap().join(""), "Hello");
        assert_eq!(response.content[0].text.as_deref(), Some("Hello"));
        let usage = response.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (6, 2));
    }

    #[tokio::test]
    async fn stream_reassembles_tool_calls_from_fragments() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/chat/completions",
            post(|| async {
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body.to_string(),
                )
                    .into_response()
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let response = client_at(&base_url)
            .create_message_stream(
                "gpt-4o",
                vec![user_message("read a.txt")],
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();

        assert!(chunks.lock().unwrap().is_empty());
        assert_eq!(response.content.len(), 1);
        let block = &response.content[0];
        assert_eq!(block.block_type, "tool_use");
        assert_eq!(block.id.as_deref(), Some("call-1"));
        assert_eq!(block.name.as_deref(), Some("read_file"));
        assert_eq!(block.input, Some(json!({ "path": "a.txt" })));
    }

    #[tokio::test]
    async fn stream_reports_http_errors() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/chat/completions",
            post(|| async {
                (
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    "{\"error\":\"rate limited\"}".to_string(),
                )
            }),
        ))
        .await;

        let (chunks, callback) = collected_text();
        let err = client_at(&base_url)
            .create_message_stream(
                "gpt-4o",
                vec![user_message("hi")],
                &[],
                100,
                0.7,
                None,
                EffortLevel::Medium,
                callback,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("429"), "error: {}", message);
        assert!(message.contains("rate limited"));
        assert!(chunks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_message_maps_tool_call_response() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let base_url = spawn_server(Router::new().route(
            "/chat/completions",
            post(|Json(payload): Json<serde_json::Value>| async move {
                assert_eq!(payload["model"], json!("gpt-4o"));
                assert_eq!(payload["max_completion_tokens"], json!(321));
                Json(json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "calling tool",
                            "tool_calls": [{
                                "id": "call-9",
                                "type": "function",
                                "function": { "name": "read_file", "arguments": "{\"path\":\"z\"}" }
                            }]
                        }
                    }],
                    "usage": { "prompt_tokens": 4, "completion_tokens": 1 }
                }))
            }),
        ))
        .await;

        let response = client_at(&base_url)
            .create_message(
                "gpt-4o",
                vec![user_message("hi")],
                &[],
                321,
                0.7,
                None,
                EffortLevel::Medium,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();

        assert_eq!(response.content[0].text.as_deref(), Some("calling tool"));
        assert_eq!(response.content[1].id.as_deref(), Some("call-9"));
        assert_eq!(response.content[1].input, Some(json!({ "path": "z" })));
    }
}
