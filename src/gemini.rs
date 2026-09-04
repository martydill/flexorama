use crate::anthropic::{AnthropicResponse, ContentBlock, Message, Usage};
use crate::config::EffortLevel;
use crate::tools::Tool;
use anyhow::Result;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiPart {
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<GeminiInlineData>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiFunctionCall {
    name: String,
    #[serde(rename = "args", skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    thought_signature: Option<String>,
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
    client: Arc<OnceLock<Client>>,
    api_key: String,
    base_url: String,
}

impl GeminiClient {
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
        _effort: EffortLevel,
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
        let response = self.get_client().post(&endpoint).json(&request).send().await?;

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
        _effort: EffortLevel,
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
                _effort,
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
                                    inline_data: None,
                                });
                            }
                        }
                        "image" => {
                            if let Some(source) = &block.source {
                                parts.push(GeminiPart {
                                    text: None,
                                    function_call: None,
                                    function_response: None,
                                    inline_data: Some(GeminiInlineData {
                                        mime_type: source.media_type.clone(),
                                        data: source.data.clone(),
                                    }),
                                });
                            }
                        }
                        "tool_use" => {
                            let args = block.input.clone().unwrap_or(Value::Null);
                            if let (Some(id), Some(name)) = (&block.id, &block.name) {
                                tool_name_by_id.insert(id.clone(), name.clone());
                            }
                            let name = block.name.clone().unwrap_or_else(|| "tool".to_string());
                            if let Some(signature) = block.thought_signature.clone() {
                                parts.push(GeminiPart {
                                    text: None,
                                    function_call: Some(GeminiFunctionCall {
                                        name,
                                        arguments: Some(args),
                                        thought_signature: Some(signature),
                                    }),
                                    function_response: None,
                                    inline_data: None,
                                });
                            } else {
                                debug!(
                                    "Skipping Gemini tool_call without thought_signature: {}",
                                    name
                                );
                            }
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
                                inline_data: None,
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

        let system_instruction = system_prompt.map(|prompt| GeminiSystemInstruction {
            parts: vec![GeminiPart {
                text: Some(prompt.clone()),
                function_call: None,
                function_response: None,
                inline_data: None,
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
                                thought_signature: call.thought_signature,
                                source: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolCall, ToolResult};
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    fn client() -> GeminiClient {
        GeminiClient::new("test-key".to_string(), "https://gemini.test/v1beta".to_string())
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
    fn normalize_args_wraps_non_object_values() {
        assert_eq!(
            normalize_args(Some(json!({ "path": "x" }))),
            json!({ "path": "x" })
        );
        assert_eq!(
            normalize_args(Some(json!("plain"))),
            json!({ "value": "plain" })
        );
        assert_eq!(normalize_args(None), json!({}));
    }

    #[test]
    fn build_request_maps_text_and_roles() {
        let request = client().build_request(
            vec![
                message("user", vec![ContentBlock::text("question".to_string())]),
                message("assistant", vec![ContentBlock::text("answer".to_string())]),
            ],
            &[tool()],
            900,
            0.4,
            Some(&"You are terse.".to_string()),
        );

        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(
            wire["systemInstruction"]["parts"][0]["text"],
            json!("You are terse.")
        );
        assert_eq!(wire["contents"][0]["role"], json!("user"));
        assert_eq!(wire["contents"][0]["parts"][0]["text"], json!("question"));
        assert_eq!(wire["contents"][1]["role"], json!("model"));
        assert_eq!(wire["generationConfig"]["maxOutputTokens"], json!(900));
        assert_eq!(wire["tools"][0]["functionDeclarations"][0]["name"], json!("read_file"));
        assert!(wire.get("tools").is_some());
    }

    #[test]
    fn build_request_sends_images_as_inline_data() {
        let image = ContentBlock::image("image/png".to_string(), "aGk=".to_string());
        let request = client().build_request(
            vec![message("user", vec![image])],
            &[],
            100,
            0.5,
            None,
        );

        let wire = serde_json::to_value(&request).unwrap();
        let part = &wire["contents"][0]["parts"][0];
        assert_eq!(part["inlineData"]["mimeType"], json!("image/png"));
        assert_eq!(part["inlineData"]["data"], json!("aGk="));
    }

    #[test]
    fn build_request_only_sends_tool_calls_with_thought_signatures() {
        let mut signed = ContentBlock::tool_use("call-1".to_string(), "read_file".to_string(), json!({ "path": "x" }));
        signed.thought_signature = Some("sig-1".to_string());
        let unsigned = ContentBlock::tool_use("call-2".to_string(), "glob".to_string(), json!({}));

        let request = client().build_request(
            vec![message("assistant", vec![signed, unsigned])],
            &[],
            100,
            0.5,
            None,
        );

        let wire = serde_json::to_value(&request).unwrap();
        let parts = wire["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1, "unsigned tool calls must be dropped");
        assert_eq!(parts[0]["functionCall"]["name"], json!("read_file"));
        assert_eq!(parts[0]["functionCall"]["args"], json!({ "path": "x" }));
        assert_eq!(parts[0]["functionCall"]["thoughtSignature"], json!("sig-1"));
    }

    #[test]
    fn build_request_resolves_tool_result_names_and_roles() {
        let mut tool_use =
            ContentBlock::tool_use("call-1".to_string(), "read_file".to_string(), json!({}));
        tool_use.thought_signature = Some("sig".to_string());
        let json_result =
            ContentBlock::tool_result("call-1".to_string(), r#"{"content":"body"}"#.to_string(), None);

        let request = client().build_request(
            vec![
                message("assistant", vec![tool_use]),
                message("user", vec![json_result]),
            ],
            &[],
            100,
            0.5,
            None,
        );

        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire["contents"][1]["role"], json!("function"));
        let response_part = &wire["contents"][1]["parts"][0]["functionResponse"];
        assert_eq!(response_part["name"], json!("read_file"));
        assert_eq!(response_part["response"], json!({ "content": "body" }));
    }

    #[test]
    fn build_request_wraps_non_json_tool_results() {
        let result = ContentBlock::tool_result(
            "unknown-tool".to_string(),
            "plain failure text".to_string(),
            None,
        );

        let request = client().build_request(
            vec![message("user", vec![result])],
            &[],
            100,
            0.5,
            None,
        );

        let wire = serde_json::to_value(&request).unwrap();
        let response_part = &wire["contents"][0]["parts"][0]["functionResponse"];
        assert_eq!(response_part["name"], json!("unknown-tool"));
        assert_eq!(response_part["response"], json!({ "result": "plain failure text" }));
    }

    #[test]
    fn map_response_handles_all_part_kinds_and_usage() {
        let parsed: GeminiResponse = serde_json::from_value(json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        { "text": "here you go" },
                        { "functionCall": { "name": "read_file", "args": { "path": "z" }, "thoughtSignature": "s" } },
                        { "functionResponse": { "name": "read_file", "response": { "ok": true } } }
                    ]
                }
            }],
            "usageMetadata": { "promptTokenCount": 12, "candidatesTokenCount": 5 }
        }))
        .unwrap();

        let mapped = client().map_response(parsed);

        assert_eq!(mapped.content.len(), 3);
        assert_eq!(mapped.content[0].text.as_deref(), Some("here you go"));

        let tool_use = &mapped.content[1];
        assert_eq!(tool_use.block_type, "tool_use");
        assert_eq!(tool_use.name.as_deref(), Some("read_file"));
        assert_eq!(tool_use.input, Some(json!({ "path": "z" })));
        assert_eq!(tool_use.thought_signature.as_deref(), Some("s"));
        assert!(tool_use.id.as_deref().unwrap_or_default().starts_with("gemini_call_"));

        let tool_result = &mapped.content[2];
        assert_eq!(tool_result.block_type, "tool_result");
        assert_eq!(tool_result.tool_use_id.as_deref(), Some("read_file"));
        assert_eq!(tool_result.content.as_deref(), Some(r#"{"ok":true}"#));

        let usage = mapped.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (12, 5));
    }

    #[test]
    fn map_response_falls_back_to_total_token_count() {
        let parsed: GeminiResponse = serde_json::from_value(json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "hi" }] } }],
            "usageMetadata": { "promptTokenCount": 3, "totalTokenCount": 9 }
        }))
        .unwrap();

        let mapped = client().map_response(parsed);
        let usage = mapped.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 9));
    }

    #[test]
    fn map_response_without_candidates_is_empty() {
        let parsed: GeminiResponse = serde_json::from_value(json!({})).unwrap();
        let mapped = client().map_response(parsed);
        assert!(mapped.content.is_empty());
        assert!(mapped.usage.is_none());
    }

    #[tokio::test]
    async fn stream_fallback_emits_aggregated_text_via_callback() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{}", addr);

        let app = Router::new().route(
            "/models/gemini-test:generateContent",
            post(|| async {
                Json(json!({
                    "candidates": [{
                        "content": {
                            "role": "model",
                            "parts": [{ "text": "line one" }, { "text": "line two" }]
                        }
                    }]
                }))
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&chunks);
        let callback: Arc<dyn Fn(String) + Send + Sync + 'static> =
            Arc::new(move |text: String| sink.lock().unwrap().push(text));

        let response = client_at(&base_url)
            .create_message_stream(
                "gemini-test",
                vec![message("user", vec![ContentBlock::text("hi".to_string())])],
                &[],
                100,
                0.5,
                None,
                EffortLevel::Medium,
                callback,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
            .await
            .unwrap();

        assert_eq!(chunks.lock().unwrap().join("|"), "line one\nline two");
        assert_eq!(response.content.len(), 2);
    }

    fn client_at(base_url: &str) -> GeminiClient {
        GeminiClient::new("test-key".to_string(), base_url.to_string())
    }
}
