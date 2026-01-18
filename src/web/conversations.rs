use crate::agent::{ConversationSnapshot, StreamToolEvent};
use crate::anthropic::ContentBlock;
use crate::conversation::ConversationManager;
use crate::custom_commands;
use crate::database::{Conversation, DatabaseManager, ToolCallRecord};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::state::{build_permission_handler, get_or_create_conversation_agent, WebState};

#[derive(Serialize)]
struct ConversationListItem {
    id: String,
    created_at: String,
    updated_at: String,
    model: String,
    subagent: Option<String>,
    total_tokens: i32,
    request_count: i32,
    last_message: Option<String>,
    message_count: usize,
}

#[derive(Serialize, Clone)]
pub(crate) struct ContentBlockDto {
    #[serde(rename = "type")]
    pub(crate) block_type: String,
    pub(crate) text: Option<String>,
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) input: Option<serde_json::Value>,
    pub(crate) tool_use_id: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) is_error: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct MessageDto {
    pub(crate) id: String,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) blocks: Vec<ContentBlockDto>,
    pub(crate) created_at: String,
}

#[derive(Serialize)]
struct ConversationDetail {
    conversation: ConversationMeta,
    messages: Vec<MessageDto>,
    context_files: Vec<String>,
}

#[derive(Serialize)]
struct ConversationMeta {
    id: String,
    created_at: String,
    updated_at: String,
    system_prompt: Option<String>,
    model: String,
    subagent: Option<String>,
    total_tokens: i32,
    request_count: i32,
}

#[derive(Deserialize)]
pub(crate) struct NewConversationRequest {
    system_prompt: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct MessageRequest {
    message: String,
}

#[derive(Deserialize)]
pub(crate) struct ConversationListQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct ConversationSearchQuery {
    query: Option<String>,
}

pub async fn list_conversations(
    State(state): State<WebState>,
    Query(query): Query<ConversationListQuery>,
) -> impl IntoResponse {
    let db = state.database.clone();
    let limit = query.limit.unwrap_or(10);
    let offset = query.offset.unwrap_or(0);

    let result = db
        .get_recent_conversations_with_offset(limit, offset, None)
        .await;

    match result {
        Ok(conversations) => {
            eprintln!(
                "DEBUG: list_conversations found {} conversations (limit={}, offset={})",
                conversations.len(),
                limit,
                offset
            );
            let items = build_conversation_list_items(db.as_ref(), conversations).await;
            eprintln!("DEBUG: Returning {} conversation items", items.len());
            Json(items).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list conversations: {}", e),
        )
            .into_response(),
    }
}

pub async fn search_conversations(
    State(state): State<WebState>,
    Query(query): Query<ConversationSearchQuery>,
) -> impl IntoResponse {
    let search_term = query.query.unwrap_or_default();
    let trimmed = search_term.trim();
    if trimmed.is_empty() {
        return Json(Vec::<ConversationListItem>::new()).into_response();
    }

    let db = state.database.clone();
    let result = db.get_recent_conversations(100, Some(trimmed)).await;

    match result {
        Ok(conversations) => {
            eprintln!(
                "DEBUG: search_conversations found {} conversations",
                conversations.len()
            );
            let items = build_conversation_list_items(db.as_ref(), conversations).await;
            Json(items).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to search conversations: {}", e),
        )
            .into_response(),
    }
}

async fn build_conversation_list_items(
    db: &DatabaseManager,
    conversations: Vec<Conversation>,
) -> Vec<ConversationListItem> {
    let mut items = Vec::new();
    for conversation in conversations {
        eprintln!("DEBUG: Processing conversation {}", conversation.id);
        let messages = db
            .get_conversation_messages(&conversation.id)
            .await
            .unwrap_or_default();
        eprintln!(
            "DEBUG: Conversation {} has {} messages",
            conversation.id,
            messages.len()
        );
        let first_user = messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone());
        let last_message = first_user.or_else(|| messages.last().map(|m| m.content.clone()));
        let item = ConversationListItem {
            id: conversation.id.clone(),
            created_at: conversation.created_at.to_rfc3339(),
            updated_at: conversation.updated_at.to_rfc3339(),
            model: conversation.model.clone(),
            subagent: conversation.subagent.clone(),
            total_tokens: conversation.total_tokens,
            request_count: conversation.request_count,
            last_message,
            message_count: messages.len(),
        };
        items.push(item);
    }
    items
}

fn conversation_to_meta(conversation: &Conversation) -> ConversationMeta {
    ConversationMeta {
        id: conversation.id.clone(),
        created_at: conversation.created_at.to_rfc3339(),
        updated_at: conversation.updated_at.to_rfc3339(),
        system_prompt: conversation.system_prompt.clone(),
        model: conversation.model.clone(),
        subagent: conversation.subagent.clone(),
        total_tokens: conversation.total_tokens,
        request_count: conversation.request_count,
    }
}

pub async fn get_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = state.database.clone();

    // First check if there's a dedicated agent for this conversation in the pool
    let snapshot = {
        let agents = state.conversation_agents.lock().await;
        if let Some(agent_arc) = agents.get(&id) {
            // Use the dedicated conversation agent's snapshot (most up-to-date)
            let agent = agent_arc.lock().await;
            agent.snapshot_conversation()
        } else {
            // Fall back to main agent's snapshot
            let agent_guard = state.agent.lock().await;
            agent_guard.snapshot_conversation()
        }
    };
    let conversation = db.get_conversation(&id).await;

    match conversation {
        Ok(Some(conversation)) => {
            let use_snapshot = snapshot
                .id
                .as_ref()
                .map(|cid| cid == &conversation.id)
                .unwrap_or(false);

            let mut meta = conversation_to_meta(&conversation);

            let messages: Vec<MessageDto> = if use_snapshot {
                meta.system_prompt = snapshot.system_prompt.clone();
                meta.model = snapshot.model.clone();
                snapshot_messages_to_dto(&snapshot)
            } else {
                let raw_messages = match db.get_conversation_messages(&id).await {
                    Ok(messages) => messages,
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to load messages: {}", e),
                        )
                            .into_response()
                    }
                };
                let tool_calls = match db.get_conversation_tool_calls(&id).await {
                    Ok(calls) => calls,
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to load tool calls: {}", e),
                        )
                            .into_response()
                    }
                };
                timeline_messages_to_dto(raw_messages.clone(), tool_calls)
            };

            let raw_context_messages: Vec<MessageDto> =
                match db.get_conversation_messages(&id).await {
                    Ok(msgs) => msgs
                        .into_iter()
                        .map(|m| {
                            let blocks = vec![ContentBlock::text(m.content.clone())];
                            build_message_dto(m.id, m.role, m.created_at.to_rfc3339(), blocks)
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };

            let mut context_files = extract_context_files_from_messages(&raw_context_messages);
            for f in ConversationManager::default_agents_files() {
                if !context_files.contains(&f) {
                    context_files.push(f);
                }
            }

            Json(ConversationDetail {
                conversation: meta,
                messages,
                context_files,
            })
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Conversation not found".to_string()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load conversation: {}", e),
        )
            .into_response(),
    }
}

pub async fn create_conversation(
    State(state): State<WebState>,
    Json(payload): Json<NewConversationRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock_owned().await;

    if let Some(prompt) = payload.system_prompt {
        agent.set_system_prompt(prompt);
    }

    let result = agent.clear_conversation_keep_agents_md().await;

    match result {
        Ok(_) => match agent.current_conversation_id() {
            Some(id) => Json(HashMap::from([("id", id)])).into_response(),
            None => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Conversation ID missing".to_string(),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create conversation: {}", e),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
pub async fn send_message_to_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<MessageRequest>,
) -> impl IntoResponse {
    // Get or create a dedicated agent for this conversation
    let agent_arc = match get_or_create_conversation_agent(&state, &id).await {
        Ok(agent) => agent,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to load conversation: {}", e),
            )
                .into_response();
        }
    };

    let mut agent = agent_arc.lock().await;

    let permission_handler =
        build_permission_handler(state.permission_hub.clone(), Some(id.clone()), None);
    agent.set_permission_handler(Some(permission_handler)).await;

    let cancellation_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut message = payload.message.clone();
    match custom_commands::render_custom_command_input(&message).await {
        Ok(Some(rendered)) => {
            if let Some(model) = rendered.command.model {
                if let Err(e) = agent.set_model(model).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to set command model: {}", e),
                    )
                        .into_response();
                }
            }
            message = rendered.message;
        }
        Ok(None) => {}
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }

    match agent.process_message(&message, cancellation_flag).await {
        Ok(response) => Json(HashMap::from([("response", response)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to process message: {}", e),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
pub async fn stream_message_to_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<MessageRequest>,
) -> impl IntoResponse {
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    let cancellation_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let message = payload.message.clone();
    let permission_hub = state.permission_hub.clone();
    let conversation_id = id.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let stream_sender = tx.clone();
        let tool_sender = stream_sender.clone();

        let send_json = |sender: &mpsc::Sender<Result<Bytes, Infallible>>,
                         value: serde_json::Value| {
            if let Ok(text) = serde_json::to_string(&value) {
                let _ = sender.try_send(Ok(Bytes::from(text + "\n")));
            }
        };

        // Get or create agent for this conversation inside the spawned task
        let agent_arc = match get_or_create_conversation_agent(&state_clone, &conversation_id).await
        {
            Ok(agent) => agent,
            Err(e) => {
                send_json(
                    &stream_sender,
                    serde_json::json!({
                        "type": "error",
                        "error": format!("Failed to load conversation: {}", e)
                    }),
                );
                return;
            }
        };

        let mut agent = agent_arc.lock().await;

        let on_stream = {
            let sender = tx.clone();
            Arc::new(move |delta: String| {
                send_json(
                    &sender,
                    serde_json::json!({
                        "type": "text",
                        "delta": delta
                    }),
                );
            })
        };

        let permission_handler = build_permission_handler(
            permission_hub,
            Some(conversation_id),
            Some(stream_sender.clone()),
        );
        agent.set_permission_handler(Some(permission_handler)).await;

        let mut resolved_message = message.clone();
        match custom_commands::render_custom_command_input(&message).await {
            Ok(Some(rendered)) => {
                if let Some(model) = rendered.command.model {
                    if let Err(e) = agent.set_model(model).await {
                        send_json(
                            &stream_sender,
                            serde_json::json!({
                                "type": "error",
                                "error": format!("Failed to set command model: {}", e)
                            }),
                        );
                        return;
                    }
                }
                resolved_message = rendered.message;
            }
            Ok(None) => {}
            Err(e) => {
                send_json(
                    &stream_sender,
                    serde_json::json!({
                        "type": "error",
                        "error": e.to_string()
                    }),
                );
                return;
            }
        }

        let result = agent
            .process_message_with_stream(
                &resolved_message,
                Some(on_stream),
                Some(Arc::new(move |evt: StreamToolEvent| {
                    send_json(
                        &tool_sender,
                        serde_json::json!({
                            "type": evt.event,
                            "tool_use_id": evt.tool_use_id,
                            "name": evt.name,
                            "input": evt.input,
                            "content": evt.content,
                            "is_error": evt.is_error,
                        }),
                    );
                })),
                cancellation_flag.clone(),
            )
            .await;

        match result {
            Ok(final_response) => send_json(
                &stream_sender,
                serde_json::json!({
                    "type": "final",
                    "content": final_response
                }),
            ),
            Err(e) => send_json(
                &stream_sender,
                serde_json::json!({
                    "type": "error",
                    "error": e.to_string()
                }),
            ),
        }
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap()
}

fn block_to_dto(block: &ContentBlock) -> ContentBlockDto {
    ContentBlockDto {
        block_type: block.block_type.clone(),
        text: block.text.clone(),
        id: block.id.clone(),
        name: block.name.clone(),
        input: block.input.clone(),
        tool_use_id: block.tool_use_id.clone(),
        content: block.content.clone(),
        is_error: block.is_error,
    }
}

fn is_context_block(block: &ContentBlock) -> bool {
    block
        .text
        .as_ref()
        .map(|t| t.starts_with("Context from file '"))
        .unwrap_or(false)
}

fn block_text_summary(block: &ContentBlockDto) -> String {
    match block.block_type.as_str() {
        "text" => block.text.clone().unwrap_or_default(),
        "tool_use" => format!(
            "Tool call: {}",
            block.name.as_deref().unwrap_or("unknown tool")
        ),
        "tool_result" => {
            let base = block
                .content
                .clone()
                .unwrap_or_else(|| "Tool result".to_string());
            if block.is_error.unwrap_or(false) {
                format!("(error) {}", base)
            } else {
                base
            }
        }
        _ => "".to_string(),
    }
}

pub(crate) fn build_message_dto(
    id: String,
    role: String,
    created_at: String,
    blocks: Vec<ContentBlock>,
) -> MessageDto {
    let block_dtos: Vec<ContentBlockDto> = blocks.iter().map(block_to_dto).collect();
    let content = {
        let parts: Vec<String> = block_dtos
            .iter()
            .map(block_text_summary)
            .filter(|s| !s.is_empty())
            .collect();
        parts.join("\n")
    };

    MessageDto {
        id,
        role,
        content,
        blocks: block_dtos,
        created_at,
    }
}

pub(crate) fn build_visible_message_dto(
    id: String,
    role: String,
    created_at: String,
    blocks: Vec<ContentBlock>,
) -> Option<MessageDto> {
    let filtered_blocks: Vec<ContentBlock> = blocks
        .into_iter()
        .filter(|b| !is_context_block(b))
        .collect();
    if filtered_blocks.is_empty() {
        None
    } else {
        Some(build_message_dto(id, role, created_at, filtered_blocks))
    }
}

pub(crate) fn extract_context_files_from_messages(messages: &[MessageDto]) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for msg in messages {
        for text in msg
            .blocks
            .iter()
            .filter_map(|b| b.text.as_deref().or(b.content.as_deref()))
            .chain(std::iter::once(msg.content.as_str()))
        {
            if let Some(start) = text.find("Context from file '") {
                let remainder = &text[start + "Context from file '".len()..];
                if let Some(end_idx) = remainder.find("':") {
                    let path = &remainder[..end_idx];
                    if !files.contains(&path.to_string()) {
                        files.push(path.to_string());
                    }
                }
            }
        }
    }
    files
}

fn snapshot_messages_to_dto(snapshot: &ConversationSnapshot) -> Vec<MessageDto> {
    let todo_tool_ids: HashSet<String> = snapshot
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|block| {
            if block.block_type == "tool_use" {
                if let Some(name) = block.name.as_deref() {
                    if is_todo_tool(name) {
                        return block.id.clone();
                    }
                }
            }
            None
        })
        .collect();

    snapshot
        .messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            let filtered_blocks: Vec<ContentBlock> = m
                .content
                .iter()
                .filter(|block| match block.block_type.as_str() {
                    "tool_use" => block
                        .name
                        .as_deref()
                        .map(|name| !is_todo_tool(name))
                        .unwrap_or(true),
                    "tool_result" => block
                        .tool_use_id
                        .as_deref()
                        .map(|id| !todo_tool_ids.contains(id))
                        .unwrap_or(true),
                    _ => true,
                })
                .cloned()
                .collect();
            build_visible_message_dto(
                format!("snapshot-{}", idx),
                m.role.clone(),
                chrono::Utc::now().to_rfc3339(),
                filtered_blocks,
            )
        })
        .collect()
}

pub(crate) fn parse_tool_arguments(arg_text: &str) -> serde_json::Value {
    serde_json::from_str(arg_text)
        .unwrap_or_else(|_| serde_json::Value::String(arg_text.to_string()))
}

fn is_todo_tool(name: &str) -> bool {
    matches!(name, "create_todo" | "complete_todo" | "list_todos")
}

pub(crate) fn timeline_messages_to_dto(
    messages: Vec<crate::database::Message>,
    tool_calls: Vec<ToolCallRecord>,
) -> Vec<MessageDto> {
    enum Entry {
        Message(crate::database::Message),
        ToolCall(ToolCallRecord),
        ToolResult(ToolCallRecord),
    }

    let mut timeline: Vec<(chrono::DateTime<chrono::Utc>, i32, Entry)> = Vec::new();

    for m in messages {
        timeline.push((m.created_at, 0, Entry::Message(m)));
    }

    for tc in tool_calls {
        if is_todo_tool(&tc.tool_name) {
            continue;
        }
        timeline.push((tc.created_at, 1, Entry::ToolCall(tc.clone())));
        if tc.result_content.is_some() {
            timeline.push((
                tc.created_at + Duration::milliseconds(1),
                2,
                Entry::ToolResult(tc.clone()),
            ));
        }
    }

    timeline.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    timeline
        .into_iter()
        .filter_map(|(_ts, _order, entry)| match entry {
            Entry::Message(m) => build_visible_message_dto(
                m.id,
                m.role,
                m.created_at.to_rfc3339(),
                vec![ContentBlock::text(m.content)],
            ),
            Entry::ToolCall(tc) => build_visible_message_dto(
                tc.id.clone(),
                "assistant".to_string(),
                tc.created_at.to_rfc3339(),
                vec![ContentBlock::tool_use(
                    tc.id,
                    tc.tool_name,
                    parse_tool_arguments(&tc.tool_arguments),
                )],
            ),
            Entry::ToolResult(tc) => build_visible_message_dto(
                format!("{}-result", tc.id),
                "assistant".to_string(),
                tc.created_at.to_rfc3339(),
                vec![ContentBlock::tool_result(
                    tc.id,
                    tc.result_content.unwrap_or_default(),
                    Some(tc.is_error),
                )],
            ),
        })
        .collect()
}
