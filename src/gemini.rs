use crate::anthropic::{AnthropicResponse, ContentBlock, Message, Usage};
use crate::tools::Tool;
use anyhow::Result;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiPart {
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiFunctionCall {
    name: String,
    #[serde(rename = "args", skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: Option<u32>,
    candidates_token_count: Option<u32>,
    total_token_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

pub struct GeminiClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl GeminiClient {
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

        let request = self.build_request(messages, tools, max_tokens, temperature, system_prompt);

        let endpoint = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        );

        debug!("Sending Gemini request to {}", endpoint);
        let response = self.client.post(&endpoint).json(&request).send().await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("Gemini request failed: {}", status);
            error!("Response: {}", response_text);
            return Err(anyhow::anyhow!(
                "Gemini API error: {} - {}",
                status,
                response_text
            ));
        }

        debug!("Gemini raw response: {}", response_text);
        let parsed: GeminiResponse = serde_json::from_str(&response_text).map_err(|e| {
            error!("Failed to parse Gemini response: {}", e);
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
        // Simple streaming fallback: use non-streaming endpoint and emit aggregated text
        let response = self
            .create_message(
                model,
                messages,
                tools,
                max_tokens,
                temperature,
                system_prompt,
                cancellation_flag,
            )
            .await?;

        let text = response
            .content
            .iter()
            .filter_map(|block| {
                if block.block_type == "text" {
                    block.text.clone()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !text.is_empty() {
            on_content(text);
        }

        Ok(response)
    }

    fn build_request(
        &self,
        messages: Vec<Message>,
        tools: &[Tool],
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&String>,
    ) -> GeminiRequest {
        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();
        let contents: Vec<GeminiContent> = messages
            .iter()
            .map(|message| {
                let mut parts = Vec::new();
                for block in &message.content {
                    match block.block_type.as_str() {
                        "text" => {
                            if let Some(text) = &block.text {
                                parts.push(GeminiPart {
                                    text: Some(text.clone()),
                                    function_call: None,
                                    function_response: None,
                                });
                            }
                        }
                        "tool_use" => {
                            let args = block.input.clone().unwrap_or(Value::Null);
                            if let (Some(id), Some(name)) = (&block.id, &block.name) {
                                tool_name_by_id.insert(id.clone(), name.clone());
                            }
                            parts.push(GeminiPart {
                                text: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: block.name.clone().unwrap_or_else(|| "tool".to_string()),
                                    arguments: Some(args),
                                }),
                                function_response: None,
                            });
                        }
                        "tool_result" => {
                            let mut response_value = Value::Null;
                            if let Some(content) = &block.content {
                                response_value = serde_json::from_str(content)
                                    .unwrap_or_else(|_| json!({ "result": content }));
                            }
                            let name = block
                                .tool_use_id
                                .as_ref()
                                .and_then(|id| tool_name_by_id.get(id))
                                .cloned()
                                .or_else(|| block.tool_use_id.clone())
                                .unwrap_or_else(|| "tool".to_string());
                            parts.push(GeminiPart {
                                text: None,
                                function_call: None,
                                function_response: Some(GeminiFunctionResponse {
                                    name,
                                    response: response_value,
                                }),
                            });
                        }
                        _ => {}
                    }
                }

                let role = if parts.iter().any(|p| p.function_response.is_some()) {
                    "function".to_string()
                } else if message.role == "assistant" {
                    "model".to_string()
                } else {
                    message.role.clone()
                };

                GeminiContent { role, parts }
            })
            .collect();

        let system_instruction = system_prompt.map(|prompt| GeminiContent {
            role: "system".to_string(),
            parts: vec![GeminiPart {
                text: Some(prompt.clone()),
                function_call: None,
                function_response: None,
            }],
        });

        let tool_declarations = if tools.is_empty() {
            None
        } else {
            let declarations: Vec<GeminiFunctionDeclaration> = tools
                .iter()
                .map(|tool| GeminiFunctionDeclaration {
                    name: tool.name.clone(),
                    description: Some(tool.description.clone()),
                    parameters: Some(tool.input_schema.clone()),
                })
                .collect();
            Some(vec![GeminiTool {
                function_declarations: declarations,
            }])
        };

        GeminiRequest {
            contents,
            tools: tool_declarations,
            system_instruction,
            generation_config: Some(GenerationConfig {
                max_output_tokens: Some(max_tokens),
                temperature: Some(temperature),
            }),
        }
    }

    fn map_response(&self, response: GeminiResponse) -> AnthropicResponse {
        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        if let Some(candidates) = response.candidates {
            if let Some(candidate) = candidates.into_iter().next() {
                if let Some(content) = candidate.content {
                    for part in content.parts {
                        if let Some(text) = part.text {
                            content_blocks.push(ContentBlock::text(text));
                        }
                        if let Some(call) = part.function_call {
                            let call_id = format!("gemini_call_{}", Uuid::new_v4().simple());
                            let args = normalize_args(call.arguments);
                            content_blocks.push(ContentBlock {
                                block_type: "tool_use".to_string(),
                                text: None,
                                id: Some(call_id),
                                name: Some(call.name),
                                input: Some(args),
                                tool_use_id: None,
                                content: None,
                                is_error: None,
                            });
                        }
                        if let Some(response) = part.function_response {
                            let content_string =
                                serde_json::to_string(&response.response).unwrap_or_default();
                            content_blocks.push(ContentBlock::tool_result(
                                response.name,
                                content_string,
                                None,
                            ));
                        }
                    }
                }
            }
        }

        let usage = response.usage_metadata.map(|meta| Usage {
            input_tokens: meta.prompt_token_count.unwrap_or(0),
            output_tokens: meta
                .candidates_token_count
                .or(meta.total_token_count)
                .unwrap_or(0),
        });

        AnthropicResponse {
            content: content_blocks,
            usage,
        }
    }
}

fn normalize_args(args: Option<Value>) -> Value {
    match args {
        Some(Value::Object(map)) => Value::Object(map),
        Some(other) => {
            let mut wrapper = Map::new();
            wrapper.insert("value".to_string(), other);
            Value::Object(wrapper)
        }
        None => Value::Object(Map::new()),
    }
}
