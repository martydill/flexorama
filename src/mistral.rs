use crate::anthropic::{AnthropicResponse, ContentBlock, Message, Usage};
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
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct MistralRequest {
    model: String,
    messages: Vec<MistralMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<MistralTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MistralMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<MistralToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MistralToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: MistralFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MistralFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct MistralTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: MistralFunctionDefinition,
}

#[derive(Debug, Serialize)]
struct MistralFunctionDefinition {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MistralResponse {
    choices: Vec<MistralChoice>,
    usage: Option<MistralUsage>,
}

#[derive(Debug, Deserialize)]
struct MistralChoice {
    message: MistralMessage,
}

#[derive(Debug, Deserialize)]
struct MistralUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamResponse {
    choices: Vec<MistralStreamChoice>,
    usage: Option<MistralUsage>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamChoice {
    delta: MistralStreamDelta,
}

#[derive(Debug, Deserialize)]
struct MistralStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<MistralStreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<MistralStreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub struct MistralClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl MistralClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
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
        let endpoint = format!("{}/chat/completions", self.base_url);

        debug!("Sending Mistral request to {}", endpoint);
        let response = self
            .client
            .post(&endpoint)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("Mistral request failed: {}", status);
            error!("Response: {}", response_text);
            return Err(anyhow::anyhow!(
                "Mistral API error: {} - {}",
                status,
                response_text
            ));
        }

        debug!("Mistral raw response: {}", response_text);
        let parsed: MistralResponse = serde_json::from_str(&response_text).map_err(|e| {
            error!("Failed to parse Mistral response: {}", e);
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
        let endpoint = format!("{}/chat/completions", self.base_url);

        debug!("Sending Mistral streaming request to {}", endpoint);
        let response = self
            .client
            .post(&endpoint)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!("Mistral streaming request failed: {}", status);
            error!("Response: {}", error_text);
            return Err(anyhow::anyhow!(
                "Mistral API error: {} - {}",
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
                        buffer.push_str(chunk_str);

                        while let Some(event_start) = buffer.find("data: ") {
                            if let Some(event_end) = buffer[event_start..].find("\n\n") {
                                let event_end = event_start + event_end + 2;
                                let event_data = buffer[event_start + 6..event_end - 2].trim();

                                if event_data == "[DONE]" {
                                    buffer = buffer[event_end..].to_string();
                                    break;
                                }

                                if let Ok(event) = 
                                    serde_json::from_str::<MistralStreamResponse>(event_data)
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
                .unwrap_or_else(|| format!("mistral_call_{}", Uuid::new_v4().simple()));
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
        stream: bool,
    ) -> MistralRequest {
        let mistral_messages = map_messages(messages, system_prompt);

        let tool_defs = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|tool| MistralTool {
                        tool_type: "function".to_string(),
                        function: MistralFunctionDefinition {
                            name: tool.name.clone(),
                            description: Some(tool.description.clone()),
                            parameters: Some(tool.input_schema.clone()),
                        },
                    })
                    .collect(),
            )
        };

        MistralRequest {
            model: model.to_string(),
            messages: mistral_messages,
            max_tokens: Some(max_tokens),
            temperature: Some(temperature),
            tools: tool_defs,
            stream: Some(stream),
        }
    }

    fn map_response(&self, response: MistralResponse) -> AnthropicResponse {
        let mut content_blocks = Vec::new();

        if let Some(choice) = response.choices.into_iter().next() {
            if let Some(content) = choice.message.content {
                if !content.is_empty() {
                    content_blocks.push(ContentBlock::text(content));
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

fn map_messages(messages: Vec<Message>, system_prompt: Option<&String>) -> Vec<MistralMessage> {
    let mut mistral_messages = Vec::new();

    if let Some(prompt) = system_prompt {
        mistral_messages.push(MistralMessage {
            role: "system".to_string(),
            content: Some(prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in messages {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();

        for block in &message.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        text_parts.push(text.clone());
                    }
                }
                "tool_use" => {
                    let id = block
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("mistral_call_{}", Uuid::new_v4().simple()));
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
                    tool_calls.push(MistralToolCall {
                        id,
                        call_type: "function".to_string(),
                        function: MistralFunctionCall {
                            name,
                            arguments: input,
                        },
                    });
                }
                "tool_result" => {
                    if let Some(tool_use_id) = &block.tool_use_id {
                        let content = block.content.clone().unwrap_or_default();
                        tool_results.push(MistralMessage {
                            role: "tool".to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                }
                _ => {}
            }
        }

        let text = text_parts.join("\n");
        if !tool_calls.is_empty() || !text.is_empty() {
            mistral_messages.push(MistralMessage {
                role: message.role.clone(),
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
            });
        }

        if !tool_results.is_empty() {
            mistral_messages.extend(tool_results);
        }
    }

    mistral_messages
}

fn parse_arguments(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Value::Object(Map::new());
    }

    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(arguments.to_string()))
}
