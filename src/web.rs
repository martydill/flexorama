use crate::agent::{Agent, ConversationSnapshot, StreamToolEvent};
use crate::anthropic::ContentBlock;
use crate::config;
use crate::conversation::ConversationManager;
use crate::database::{Conversation, DatabaseManager, ToolCallRecord};
use crate::mcp::{McpManager, McpServerConfig};
use crate::security::{PermissionHandler, PermissionKind, PermissionPrompt};
use crate::subagent::{SubagentConfig, SubagentManager};
use anyhow::Result;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

#[derive(Clone)]
pub struct WebState {
    pub agent: Arc<Mutex<Agent>>,
    pub database: Arc<DatabaseManager>,
    pub mcp_manager: Arc<McpManager>,
    pub subagent_manager: Arc<Mutex<SubagentManager>>,
    pub permission_hub: Arc<PermissionHub>,
}

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
struct ContentBlockDto {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    tool_use_id: Option<String>,
    content: Option<String>,
    is_error: Option<bool>,
}

#[derive(Serialize)]
struct MessageDto {
    id: String,
    role: String,
    content: String,
    blocks: Vec<ContentBlockDto>,
    created_at: String,
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
struct NewConversationRequest {
    system_prompt: Option<String>,
}

#[derive(Deserialize)]
struct MessageRequest {
    message: String,
}

#[derive(Serialize)]
struct ModelListResponse {
    provider: String,
    active_model: String,
    models: Vec<String>,
}

#[derive(Deserialize)]
struct ModelUpdateRequest {
    model: String,
}

#[derive(Serialize)]
struct PlanDto {
    id: String,
    conversation_id: Option<String>,
    title: Option<String>,
    user_request: String,
    plan_markdown: String,
    created_at: String,
}

#[derive(Deserialize)]
struct PlanUpdateRequest {
    title: Option<String>,
    user_request: Option<String>,
    plan_markdown: Option<String>,
}

#[derive(Deserialize)]
struct PlanCreateRequest {
    title: Option<String>,
    user_request: String,
    plan_markdown: String,
    conversation_id: Option<String>,
}

#[derive(Deserialize)]
struct UpsertServerRequest {
    name: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    env: Option<HashMap<String, String>>,
    enabled: Option<bool>,
}

#[derive(Serialize)]
struct ServerDto {
    name: String,
    config: McpServerConfig,
    connected: bool,
}

#[derive(Serialize)]
struct AgentDto {
    name: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model: Option<String>,
    created_at: String,
    updated_at: String,
    system_prompt: String,
    active: bool,
}

#[derive(Deserialize)]
struct AgentUpdateRequest {
    system_prompt: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct NewAgentRequest {
    name: String,
    system_prompt: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct ActivateAgentRequest {
    name: Option<String>,
}

#[derive(Serialize, Clone)]
struct PermissionRequestDto {
    id: String,
    kind: String,
    title: String,
    detail: String,
    options: Vec<String>,
    conversation_id: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
struct PermissionResolveRequest {
    id: String,
    selection: Option<usize>,
}

#[derive(Deserialize)]
struct PermissionPendingQuery {
    conversation_id: Option<String>,
}

#[derive(Serialize)]
struct PlanModeResponse {
    enabled: bool,
}

#[derive(Deserialize)]
struct PlanModeRequest {
    enabled: bool,
}

// Stats API DTOs
#[derive(Deserialize)]
struct StatsQueryParams {
    period: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Serialize)]
struct UsageStatsResponse {
    period: String,
    data: Vec<UsageStatsPoint>,
}

#[derive(Serialize)]
struct UsageStatsPoint {
    date: String,
    total_requests: i32,
    total_input_tokens: i32,
    total_output_tokens: i32,
    total_tokens: i32,
}

#[derive(Serialize)]
struct ConversationStatsResponse {
    period: String,
    data: Vec<ConversationStatsPoint>,
}

#[derive(Serialize)]
struct ConversationStatsPoint {
    date: String,
    count: i32,
}

#[derive(Serialize)]
struct ModelStatsResponse {
    period: String,
    data: Vec<ModelStatsPoint>,
}

#[derive(Serialize)]
struct ModelStatsPoint {
    model: String,
    provider: String,
    total_conversations: i32,
    total_tokens: i32,
    request_count: i32,
}

#[derive(Serialize)]
struct ConversationsByProviderResponse {
    period: String,
    data: Vec<ConversationsByProviderPoint>,
}

#[derive(Serialize)]
struct ConversationsByProviderPoint {
    date: String,
    provider: String,
    count: i32,
}

#[derive(Serialize)]
struct ConversationsBySubagentResponse {
    period: String,
    data: Vec<ConversationsBySubagentPoint>,
}

#[derive(Serialize)]
struct ConversationsBySubagentPoint {
    date: String,
    subagent: String,
    count: i32,
}

pub struct PermissionHub {
    pending: Mutex<HashMap<String, PermissionRequestDto>>,
    responders: Mutex<HashMap<String, oneshot::Sender<Option<usize>>>>,
}

impl PermissionHub {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            responders: Mutex::new(HashMap::new()),
        }
    }

    async fn create_request(
        &self,
        request: PermissionRequestDto,
    ) -> oneshot::Receiver<Option<usize>> {
        let (tx, rx) = oneshot::channel();
        let request_id = request.id.clone();
        self.pending
            .lock()
            .await
            .insert(request_id.clone(), request);
        self.responders.lock().await.insert(request_id, tx);
        rx
    }

    async fn list_pending(
        &self,
        conversation_id: Option<&str>,
    ) -> Vec<PermissionRequestDto> {
        let pending = self.pending.lock().await;
        pending
            .values()
            .filter(|req| {
                if let Some(cid) = conversation_id {
                    req.conversation_id.as_deref() == Some(cid)
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }

    async fn resolve(&self, id: &str, selection: Option<usize>) -> bool {
        let sender = self.responders.lock().await.remove(id);
        self.pending.lock().await.remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(selection);
            true
        } else {
            false
        }
    }
}

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");

fn permission_kind_label(kind: &PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Bash => "bash",
        PermissionKind::File => "file",
    }
}

fn build_permission_handler(
    hub: Arc<PermissionHub>,
    conversation_id: Option<String>,
    stream_sender: Option<mpsc::Sender<Result<Bytes, Infallible>>>,
) -> PermissionHandler {
    Arc::new(move |prompt: PermissionPrompt| {
        let hub = hub.clone();
        let conversation_id = conversation_id.clone();
        let stream_sender = stream_sender.clone();
        Box::pin(async move {
            let request_id = Uuid::new_v4().to_string();
            let request_conversation_id = conversation_id.clone();
            let request = PermissionRequestDto {
                id: request_id.clone(),
                kind: permission_kind_label(&prompt.kind).to_string(),
                title: prompt.summary,
                detail: prompt.detail,
                options: prompt.options,
                conversation_id: request_conversation_id,
                created_at: Utc::now().to_rfc3339(),
            };

            let pending = request.clone();
            let receiver = hub.create_request(request).await;

            if let Some(sender) = &stream_sender {
                if let Ok(text) = serde_json::to_string(&serde_json::json!({
                    "type": "permission_request",
                    "id": pending.id,
                    "kind": pending.kind,
                    "title": pending.title,
                    "detail": pending.detail,
                    "options": pending.options,
                    "conversation_id": pending.conversation_id,
                    "created_at": pending.created_at,
                })) {
                    let _ = sender.try_send(Ok(Bytes::from(text + "\n")));
                }
            }

            match tokio::time::timeout(std::time::Duration::from_secs(30), receiver).await {
                Ok(Ok(selection)) => selection,
                _ => {
                    let _ = hub.resolve(&request_id, None).await;
                    None
                }
            }
        })
    })
}

pub async fn launch_web_ui(state: WebState, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    app_println!(
        "?? Web UI starting on http://{} (Ctrl+C to stop)",
        addr
    );

    let router = Router::new()
        .route("/", get(serve_index))
        .route("/app.js", get(serve_app_js))
        .route("/api/health", get(health))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/api/conversations/:id", get(get_conversation))
        .route(
            "/api/conversations/:id/message",
            post(send_message_to_conversation),
        )
        .route(
            "/api/conversations/:id/message/stream",
            post(stream_message_to_conversation),
        )
        .route("/api/models", get(get_models).post(set_model))
        .route("/api/plans", get(list_plans).post(create_plan))
        .route(
            "/api/plans/:id",
            get(get_plan).put(update_plan).delete(delete_plan),
        )
        .route(
            "/api/mcp/servers",
            get(list_mcp_servers).post(upsert_mcp_server),
        )
        .route(
            "/api/mcp/servers/:name",
            get(get_mcp_server)
                .put(upsert_mcp_server_named)
                .delete(delete_mcp_server),
        )
        .route(
            "/api/mcp/servers/:name/connect",
            post(connect_mcp_server),
        )
        .route(
            "/api/mcp/servers/:name/disconnect",
            post(disconnect_mcp_server),
        )
        .route("/api/agents", get(list_agents).post(create_agent))
        .route(
            "/api/agents/:name",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
        .route("/api/agents/active", get(get_active_agent).post(set_active_agent))
        .route(
            "/api/permissions/pending",
            get(list_pending_permissions),
        )
        .route(
            "/api/permissions/respond",
            post(resolve_permission_request),
        )
        .route("/api/plan-mode", get(get_plan_mode).post(set_plan_mode))
        .route("/api/stats/overview", get(get_stats_overview))
        .route("/api/stats/usage", get(get_usage_stats))
        .route("/api/stats/models", get(get_model_stats))
        .route("/api/stats/conversations", get(get_conversation_stats))
        .route("/api/stats/conversations-by-provider", get(get_conversation_stats_by_provider))
        .route(
            "/api/stats/conversations-by-subagent",
            get(get_conversation_stats_by_subagent),
        )
        .with_state(state);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, router).await?;
    Ok(())
}

async fn serve_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn serve_app_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        APP_JS,
    )
}

async fn health() -> impl IntoResponse {
    Json(HashMap::from([("status", "ok")]))
}

async fn list_conversations(State(state): State<WebState>) -> impl IntoResponse {
    let db = state.database.clone();
    let result = db.get_recent_conversations(100, None).await;

    match result {
        Ok(conversations) => {
            let mut items = Vec::new();
            for conversation in conversations {
                let messages = db
                    .get_conversation_messages(&conversation.id)
                    .await
                    .unwrap_or_default();
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
            Json(items).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list conversations: {}", e),
            )
                .into_response()
        }
    }
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

async fn get_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = state.database.clone();
    let snapshot = {
        let agent_guard = state.agent.lock().await;
        agent_guard.snapshot_conversation()
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

            let raw_context_messages: Vec<MessageDto> = match db.get_conversation_messages(&id).await {
                Ok(msgs) => msgs
                    .into_iter()
                    .map(|m| {
                        let blocks = vec![ContentBlock::text(m.content.clone())];
                        build_message_dto(
                            m.id,
                            m.role,
                            m.created_at.to_rfc3339(),
                            blocks,
                        )
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

async fn create_conversation(
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
async fn send_message_to_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<MessageRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock_owned().await;

    if agent.current_conversation_id() != Some(id.clone()) {
        if let Err(e) = agent.resume_conversation(&id).await {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to load conversation: {}", e),
            )
                .into_response();
        }
    }

    let permission_handler =
        build_permission_handler(state.permission_hub.clone(), Some(id.clone()), None);
    agent.set_permission_handler(Some(permission_handler)).await;

    let cancellation_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    match agent
        .process_message(&payload.message, cancellation_flag)
        .await
    {
        Ok(response) => Json(HashMap::from([("response", response)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to process message: {}", e),
        )
            .into_response(),
    }
}

async fn get_models(State(state): State<WebState>) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    let provider = agent.provider();
    let models = config::provider_models(provider)
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>();
    Json(ModelListResponse {
        provider: provider.to_string(),
        active_model: agent.model().to_string(),
        models,
    })
    .into_response()
}

async fn set_model(
    State(state): State<WebState>,
    Json(payload): Json<ModelUpdateRequest>,
) -> impl IntoResponse {
    let model = payload.model.trim().to_string();
    if model.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Model is required".to_string(),
        )
            .into_response();
    }
    let mut agent = state.agent.lock_owned().await;
    match agent.set_model(model.clone()).await {
        Ok(_) => Json(HashMap::from([("model", model)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to set model: {}", e),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
async fn stream_message_to_conversation(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<MessageRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock_owned().await;

    if agent.current_conversation_id() != Some(id.clone()) {
        if let Err(e) = agent.resume_conversation(&id).await {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to load conversation: {}", e),
            )
                .into_response();
        }
    }

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    let cancellation_flag = Arc::new(AtomicBool::new(false));
    let message = payload.message.clone();
    let permission_hub = state.permission_hub.clone();
    let conversation_id = id.clone();

    tokio::spawn(async move {
        let stream_sender = tx.clone();
        let tool_sender = stream_sender.clone();

        let send_json =
            |sender: &mpsc::Sender<Result<Bytes, Infallible>>, value: serde_json::Value| {
                if let Ok(text) = serde_json::to_string(&value) {
                    let _ = sender.try_send(Ok(Bytes::from(text + "\n")));
                }
            };

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

        let result = agent
            .process_message_with_stream(
                &message,
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

async fn list_plans(State(state): State<WebState>) -> impl IntoResponse {
    match state.database.list_plans(None).await {
        Ok(plans) => {
            let list: Vec<PlanDto> = plans
                .into_iter()
                .map(|p| PlanDto {
                    id: p.id,
                    conversation_id: p.conversation_id,
                    title: p.title,
                    user_request: p.user_request,
                    plan_markdown: p.plan_markdown,
                    created_at: p.created_at.to_rfc3339(),
                })
                .collect();
            Json(list).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list plans: {}", e),
        )
            .into_response(),
    }
}

async fn get_plan(State(state): State<WebState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.database.get_plan(&id).await {
        Ok(Some(plan)) => Json(PlanDto {
            id: plan.id,
            conversation_id: plan.conversation_id,
            title: plan.title,
            user_request: plan.user_request,
            plan_markdown: plan.plan_markdown,
            created_at: plan.created_at.to_rfc3339(),
        })
        .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Plan not found".to_string()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch plan: {}", e),
        )
            .into_response(),
    }
}

async fn update_plan(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<PlanUpdateRequest>,
) -> impl IntoResponse {
    match state
        .database
        .update_plan(&id, payload.title, payload.user_request, payload.plan_markdown)
        .await
    {
        Ok(plan) => Json(PlanDto {
            id: plan.id,
            conversation_id: plan.conversation_id,
            title: plan.title,
            user_request: plan.user_request,
            plan_markdown: plan.plan_markdown,
            created_at: plan.created_at.to_rfc3339(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update plan: {}", e),
            )
                .into_response(),
    }
}

async fn delete_plan(State(state): State<WebState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.database.delete_plan(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete plan: {}", e),
        )
            .into_response(),
    }
}

async fn create_plan(
    State(state): State<WebState>,
    Json(payload): Json<PlanCreateRequest>,
) -> impl IntoResponse {
    match state
        .database
        .create_plan(
            payload.conversation_id.as_deref(),
            payload.title.as_deref(),
            &payload.user_request,
            &payload.plan_markdown,
        )
        .await
    {
        Ok(id) => Json(HashMap::from([("id", id)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create plan: {}", e),
        )
            .into_response(),
    }
}

async fn list_mcp_servers(State(state): State<WebState>) -> impl IntoResponse {
    match state.mcp_manager.list_servers().await {
        Ok(servers) => {
            let list: Vec<ServerDto> = servers
                .into_iter()
                .map(|(name, config, connected)| ServerDto {
                    name,
                    config,
                    connected,
                })
                .collect();
            Json(list).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list MCP servers: {}", e),
        )
            .into_response(),
    }
}

async fn get_mcp_server(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.mcp_manager.get_server(&name).await {
        Some(config) => Json(ServerDto {
            name: name.clone(),
            config,
            connected: state.mcp_manager.is_connected(&name).await,
        })
        .into_response(),
        None => (StatusCode::NOT_FOUND, "Server not found".to_string()).into_response(),
    }
}

async fn upsert_mcp_server(
    State(state): State<WebState>,
    Json(payload): Json<UpsertServerRequest>,
) -> impl IntoResponse {
    let name = match payload.name.clone() {
        Some(name) => name,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Name is required to create a server".to_string(),
            )
                .into_response()
        }
    };
    upsert_mcp_server_inner(state, name, payload).await
}

async fn upsert_mcp_server_named(
    State(state): State<WebState>,
    Path(name): Path<String>,
    Json(payload): Json<UpsertServerRequest>,
) -> impl IntoResponse {
    upsert_mcp_server_inner(state, name, payload).await
}

async fn upsert_mcp_server_inner(
    state: WebState,
    name: String,
    payload: UpsertServerRequest,
) -> Response {
    let enabled = payload.enabled.unwrap_or(true);

    if payload.command.is_none() && payload.url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            "Either command or url is required".to_string(),
        )
            .into_response();
    }

    let config = McpServerConfig {
        name: name.clone(),
        command: payload.command,
        args: payload.args,
        url: payload.url,
        env: payload.env,
        enabled,
    };

    match state.mcp_manager.upsert_server(&name, config).await {
        Ok(_) => Json(HashMap::from([("name", name)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save MCP server: {}", e),
        )
            .into_response(),
    }
}

async fn delete_mcp_server(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.mcp_manager.remove_server(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete MCP server: {}", e),
        )
            .into_response(),
    }
}

async fn connect_mcp_server(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.mcp_manager.connect_server(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to connect MCP server: {}", e),
        )
            .into_response(),
    }
}

async fn disconnect_mcp_server(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.mcp_manager.disconnect_server(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to disconnect MCP server: {}", e),
        )
            .into_response(),
    }
}

async fn list_agents(State(state): State<WebState>) -> impl IntoResponse {
    let active = state.agent.lock().await.active_subagent_name();
    let mut manager = state.subagent_manager.lock().await;
    if let Err(e) = manager.load_all_subagents().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load agents: {}", e),
        )
            .into_response();
    }

    let list: Vec<AgentDto> = manager
        .list_subagents()
        .into_iter()
        .map(|a| AgentDto {
            name: a.name.clone(),
            allowed_tools: a.allowed_tools.iter().cloned().collect(),
            denied_tools: a.denied_tools.iter().cloned().collect(),
            max_tokens: a.max_tokens,
            temperature: a.temperature,
            model: a.model.clone(),
            created_at: a.created_at.to_rfc3339(),
            updated_at: a.updated_at.to_rfc3339(),
            system_prompt: a.system_prompt.clone(),
            active: active.as_ref() == Some(&a.name),
        })
        .collect();

    Json(list).into_response()
}

async fn get_agent(State(state): State<WebState>, Path(name): Path<String>) -> impl IntoResponse {
    let active = state.agent.lock().await.active_subagent_name();
    let manager = state.subagent_manager.lock().await;
    match manager.get_subagent(&name) {
        Some(agent) => Json(AgentDto {
            name: agent.name.clone(),
            allowed_tools: agent.allowed_tools.iter().cloned().collect(),
            denied_tools: agent.denied_tools.iter().cloned().collect(),
            max_tokens: agent.max_tokens,
            temperature: agent.temperature,
            model: agent.model.clone(),
            created_at: agent.created_at.to_rfc3339(),
            updated_at: agent.updated_at.to_rfc3339(),
            system_prompt: agent.system_prompt.clone(),
            active: active.as_ref() == Some(&agent.name),
        })
        .into_response(),
        None => (StatusCode::NOT_FOUND, "Agent not found".to_string()).into_response(),
    }
}

async fn create_agent(
    State(state): State<WebState>,
    Json(payload): Json<NewAgentRequest>,
) -> impl IntoResponse {
    let mut manager = state.subagent_manager.lock().await;
    let now = Utc::now();

    let config = SubagentConfig {
        name: payload.name.clone(),
        system_prompt: payload.system_prompt,
        allowed_tools: payload.allowed_tools.into_iter().collect(),
        denied_tools: payload.denied_tools.into_iter().collect(),
        max_tokens: payload.max_tokens,
        temperature: payload.temperature,
        model: payload.model,
        created_at: now,
        updated_at: now,
    };

    let save_result = manager.save_subagent(&config).await;
    if let Err(e) = save_result {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save agent: {}", e),
        )
            .into_response();
    }

    // Refresh in-memory list
    if let Err(e) = manager.load_all_subagents().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to reload agents: {}", e),
        )
            .into_response();
    }

    Json(HashMap::from([("name", payload.name)])).into_response()
}

async fn update_agent(
    State(state): State<WebState>,
    Path(name): Path<String>,
    Json(payload): Json<AgentUpdateRequest>,
) -> impl IntoResponse {
    let mut manager = state.subagent_manager.lock().await;
    let existing = match manager.get_subagent(&name).cloned() {
        Some(agent) => agent,
        None => return (StatusCode::NOT_FOUND, "Agent not found".to_string()).into_response(),
    };

    let config = SubagentConfig {
        name: existing.name.clone(),
        system_prompt: payload.system_prompt,
        allowed_tools: payload.allowed_tools.into_iter().collect(),
        denied_tools: payload.denied_tools.into_iter().collect(),
        max_tokens: payload.max_tokens,
        temperature: payload.temperature,
        model: payload.model,
        created_at: existing.created_at,
        updated_at: existing.updated_at,
    };

    match manager.update_subagent(&config).await {
        Ok(_) => Json(HashMap::from([("name", name)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update agent: {}", e),
        )
            .into_response(),
    }
}

async fn delete_agent(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut manager = state.subagent_manager.lock().await;
    match manager.delete_subagent(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete agent: {}", e),
        )
            .into_response(),
    }
}

async fn get_active_agent(State(state): State<WebState>) -> impl IntoResponse {
    let name = state.agent.lock().await.active_subagent_name();
    Json(HashMap::from([("active", name)])).into_response()
}

async fn set_active_agent(
    State(state): State<WebState>,
    Json(payload): Json<ActivateAgentRequest>,
) -> impl IntoResponse {
    if let Some(name) = payload.name {
        let config = {
            let manager = state.subagent_manager.lock().await;
            manager.get_subagent(&name).cloned()
        };
        let config = match config {
            Some(cfg) => cfg,
            None => return (StatusCode::NOT_FOUND, "Agent not found".to_string()).into_response(),
        };

        let mut agent = state.agent.lock_owned().await;
        match agent.switch_to_subagent(&config).await {
            Ok(_) => {
                let mut manager = state.subagent_manager.lock().await;
                manager.set_active_subagent(Some(name.clone()));
                let conversation_id = agent.current_conversation_id();
                Json(HashMap::from([
                    ("active", Some(name)),
                    ("conversation_id", conversation_id),
                ]))
                .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to activate agent: {}", e),
            )
                .into_response(),
        }
    } else {
        let mut agent = state.agent.lock_owned().await;
        match agent.exit_subagent().await {
            Ok(_) => {
                let mut manager = state.subagent_manager.lock().await;
                manager.set_active_subagent(None);
                let conversation_id = agent.current_conversation_id();
                Json(HashMap::from([
                    ("active", Option::<String>::None),
                    ("conversation_id", conversation_id),
                ]))
                .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to exit agent: {}", e),
            )
                .into_response(),
        }
    }
}

async fn list_pending_permissions(
    State(state): State<WebState>,
    axum::extract::Query(query): axum::extract::Query<PermissionPendingQuery>,
) -> impl IntoResponse {
    let pending = state
        .permission_hub
        .list_pending(query.conversation_id.as_deref())
        .await;
    Json(pending).into_response()
}

async fn resolve_permission_request(
    State(state): State<WebState>,
    Json(payload): Json<PermissionResolveRequest>,
) -> impl IntoResponse {
    if payload.id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "id is required".to_string()).into_response();
    }
    let resolved = state
        .permission_hub
        .resolve(&payload.id, payload.selection)
        .await;
    if resolved {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "Permission request not found".to_string()).into_response()
    }
}

async fn get_plan_mode(State(state): State<WebState>) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    Json(PlanModeResponse {
        enabled: agent.plan_mode(),
    })
    .into_response()
}

async fn set_plan_mode(
    State(state): State<WebState>,
    Json(payload): Json<PlanModeRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock_owned().await;
    match agent.set_plan_mode(payload.enabled).await {
        Ok(_) => Json(PlanModeResponse {
            enabled: payload.enabled,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to set plan mode: {}", e),
        )
            .into_response(),
    }
}

// Stats API handlers
async fn get_stats_overview(State(state): State<WebState>) -> impl IntoResponse {
    match state.database.get_stats_overview().await {
        Ok(overview) => Json(overview).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load stats overview: {}", e),
        )
            .into_response(),
    }
}

async fn get_usage_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);

    match state.database.get_usage_stats_range(start_date, end_date).await {
        Ok(stats) => {
            let response = UsageStatsResponse {
                period: params.period.unwrap_or_else(|| "month".to_string()),
                data: stats.into_iter().map(|s| UsageStatsPoint {
                    date: s.date,
                    total_requests: s.total_requests,
                    total_input_tokens: s.total_input_tokens,
                    total_output_tokens: s.total_output_tokens,
                    total_tokens: s.total_tokens,
                }).collect(),
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load usage stats: {}", e),
        )
            .into_response(),
    }
}

async fn get_model_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);

    match state.database.get_stats_by_model(start_date, end_date).await {
        Ok(stats) => {
            let response = ModelStatsResponse {
                period: params.period.unwrap_or_else(|| "month".to_string()),
                data: stats.into_iter().map(|s| ModelStatsPoint {
                    model: s.model.clone(),
                    provider: extract_provider_from_model(&s.model),
                    total_conversations: s.total_conversations,
                    total_tokens: s.total_tokens,
                    request_count: s.request_count,
                }).collect(),
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load model stats: {}", e),
        )
            .into_response(),
    }
}

async fn get_conversation_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);

    match state.database.get_conversation_counts_by_date(start_date, end_date).await {
        Ok(counts) => {
            let response = ConversationStatsResponse {
                period: params.period.unwrap_or_else(|| "month".to_string()),
                data: counts.into_iter().map(|(date, count)| ConversationStatsPoint {
                    date,
                    count,
                }).collect(),
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load conversation stats: {}", e),
        )
            .into_response(),
    }
}

async fn get_conversation_stats_by_provider(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);

    match state.database.get_conversation_counts_by_date_and_model(start_date, end_date).await {
        Ok(counts) => {
            // Aggregate by (date, provider) since multiple models can map to the same provider
            let mut aggregated: HashMap<(String, String), i32> = HashMap::new();

            for (date, model, count) in counts {
                let provider = extract_provider_from_model(&model);
                let key = (date.clone(), provider.clone());
                *aggregated.entry(key).or_insert(0) += count;
            }

            let data: Vec<ConversationsByProviderPoint> = aggregated
                .into_iter()
                .map(|((date, provider), count)| ConversationsByProviderPoint {
                    date,
                    provider,
                    count,
                })
                .collect();

            let response = ConversationsByProviderResponse {
                period: params.period.unwrap_or_else(|| "month".to_string()),
                data,
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load conversation stats by provider: {}", e),
        )
            .into_response(),
    }
}

async fn get_conversation_stats_by_subagent(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);

    match state
        .database
        .get_conversation_counts_by_date_and_subagent(start_date, end_date)
        .await
    {
        Ok(counts) => {
            let data: Vec<ConversationsBySubagentPoint> = counts
                .into_iter()
                .map(|(date, subagent, count)| ConversationsBySubagentPoint {
                    date,
                    subagent,
                    count,
                })
                .collect();

            let response = ConversationsBySubagentResponse {
                period: params.period.unwrap_or_else(|| "month".to_string()),
                data,
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load conversation stats by subagent: {}", e),
        )
            .into_response(),
    }
}

fn extract_provider_from_model(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower.contains("claude") {
        "Anthropic".to_string()
    } else if lower.contains("gpt") {
        "OpenAI".to_string()
    } else if lower.contains("gemini") {
        "Gemini".to_string()
    } else if lower.contains("glm") {
        "Z.AI".to_string()
    } else {
        "Other".to_string()
    }
}

fn calculate_date_range(params: &StatsQueryParams) -> (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) {
    use chrono::NaiveDate;

    // If custom dates are provided, use them
    if let (Some(start), Some(end)) = (&params.start_date, &params.end_date) {
        let start_parsed = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok();
        let end_parsed = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok();
        return (start_parsed, end_parsed);
    }

    // Otherwise, calculate based on period
    let now = Utc::now().naive_utc().date();
    let period = params.period.as_deref().unwrap_or("month");

    match period {
        "day" => (Some(now - Duration::days(1)), Some(now)),
        "week" => (Some(now - Duration::days(7)), Some(now)),
        "month" => (Some(now - Duration::days(30)), Some(now)),
        "lifetime" => (None, None),
        _ => (Some(now - Duration::days(30)), Some(now)),
    }
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
            let base = block.content.clone().unwrap_or_else(|| "Tool result".to_string());
            if block.is_error.unwrap_or(false) {
                format!("(error) {}", base)
            } else {
                base
            }
        }
        _ => "".to_string(),
    }
}

fn build_message_dto(
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

fn build_visible_message_dto(
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
        Some(build_message_dto(
            id,
            role,
            created_at,
            filtered_blocks,
        ))
    }
}

fn extract_context_files_from_messages(messages: &[MessageDto]) -> Vec<String> {
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
    snapshot
        .messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            build_visible_message_dto(
                format!("snapshot-{}", idx),
                m.role.clone(),
                chrono::Utc::now().to_rfc3339(),
                m.content.clone(),
            )
        })
        .collect()
}

fn parse_tool_arguments(arg_text: &str) -> serde_json::Value {
    serde_json::from_str(arg_text).unwrap_or_else(|_| serde_json::Value::String(arg_text.to_string()))
}

fn timeline_messages_to_dto(
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


