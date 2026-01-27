use crate::agent::{Agent, ConversationSnapshot, StreamToolEvent};
use crate::anthropic::ContentBlock;
use crate::config;
use crate::conversation::ConversationManager;
use crate::csrf::CsrfManager;
use crate::custom_commands;
use crate::database::{Conversation, DatabaseManager, ToolCallRecord};
use crate::mcp::{McpAuthConfig, McpManager, McpServerConfig};
use crate::security::{PermissionHandler, PermissionKind, PermissionPrompt};
use crate::skill::SkillManager;
use crate::subagent::{SubagentConfig, SubagentManager};
use anyhow::Result;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(Clone)]
pub struct WebState {
    pub agent: Arc<Mutex<Agent>>,
    pub database: Arc<DatabaseManager>,
    pub mcp_manager: Arc<McpManager>,
    pub subagent_manager: Arc<Mutex<SubagentManager>>,
    pub permission_hub: Arc<PermissionHub>,
    pub skill_manager: Arc<Mutex<SkillManager>>,
    pub conversation_agents: Arc<Mutex<HashMap<String, Arc<Mutex<Agent>>>>>,
    pub csrf_manager: Arc<CsrfManager>,
    pub config: Arc<config::Config>,
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
struct ImageSourceDto {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
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
    source: Option<ImageSourceDto>,
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

#[derive(Deserialize, Clone)]
struct ImageData {
    media_type: String,
    data: String,
}

#[derive(Deserialize)]
struct MessageRequest {
    message: String,
    images: Option<Vec<ImageData>>,
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
    auth: Option<McpAuthConfig>,
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
struct FileAutocompleteQuery {
    prefix: String,
}

#[derive(Serialize)]
struct FileAutocompleteResponse {
    files: Vec<FileAutocompleteItem>,
}

#[derive(Serialize)]
struct FileAutocompleteItem {
    path: String,
    is_directory: bool,
}

// Skill-related DTOs
#[derive(Serialize)]
struct SkillDto {
    name: String,
    description: String,
    content: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tags: Vec<String>,
    references: Vec<String>,
    active: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct NewSkillRequest {
    name: String,
    description: String,
    content: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct SkillUpdateRequest {
    description: String,
    content: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tags: Vec<String>,
}

// Custom command DTOs
#[derive(Serialize)]
struct CustomCommandDto {
    name: String,
    description: String,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    content: String,
}

#[derive(Deserialize)]
struct NewCustomCommandRequest {
    name: String,
    description: String,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    content: String,
}

#[derive(Deserialize)]
struct CustomCommandUpdateRequest {
    description: String,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    content: String,
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

    async fn list_pending(&self, conversation_id: Option<&str>) -> Vec<PermissionRequestDto> {
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

/// Get or create an agent for a specific conversation
/// This allows multiple conversations to be processed concurrently without blocking
async fn get_or_create_conversation_agent(
    state: &WebState,
    conversation_id: &str,
) -> Result<Arc<Mutex<Agent>>> {
    // Check if agent already exists for this conversation
    {
        let agents = state.conversation_agents.lock().await;
        if let Some(agent) = agents.get(conversation_id) {
            return Ok(agent.clone());
        }
    }

    // Create a new agent for this conversation
    // Use the config from WebState (which includes CLI-provided API key)
    let config = (*state.config).clone();
    let (model, plan_mode) = {
        let template_agent = state.agent.lock().await;
        (
            template_agent.model().to_string(),
            template_agent.plan_mode(),
        )
    };

    // Create new agent with same configuration (yolo_mode always false for web)
    let mut new_agent = Agent::new_with_plan_mode(config, model, false, plan_mode)
        .await
        .with_database_manager(state.database.clone())
        .with_skill_manager(state.skill_manager.clone());

    // Resume the conversation in the new agent
    new_agent.resume_conversation(conversation_id).await?;

    let agent_arc = Arc::new(Mutex::new(new_agent));

    // Store in pool
    {
        let mut agents = state.conversation_agents.lock().await;
        agents.insert(conversation_id.to_string(), agent_arc.clone());
    }

    Ok(agent_arc)
}

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

            match receiver.await {
                Ok(selection) => selection,
                Err(_) => {
                    let _ = hub.resolve(&request_id, None).await;
                    None
                }
            }
        })
    })
}

async fn ensure_default_conversation(state: &WebState) -> Result<Option<String>> {
    // Check for conversations with messages (not just empty conversations)
    let existing = state.database.get_recent_conversations(1, None).await?;
    eprintln!(
        "DEBUG: Found {} conversations with messages",
        existing.len()
    );

    if !existing.is_empty() {
        eprintln!("DEBUG: Conversations exist, not creating default");
        return Ok(None);
    }

    eprintln!("DEBUG: Creating default conversation");
    let (model, system_prompt, subagent) = {
        let agent = state.agent.lock().await;
        (
            agent.model().to_string(),
            agent.get_system_prompt().cloned(),
            agent.active_subagent_name(),
        )
    };

    let conversation_id = state
        .database
        .create_conversation(system_prompt, &model, subagent.as_deref())
        .await?;
    eprintln!("DEBUG: Created conversation: {}", conversation_id);

    state
        .database
        .add_message(
            &conversation_id,
            "assistant",
            "Welcome to Flexorama. Send a message to start chatting.",
            &model,
            0,
        )
        .await?;
    eprintln!("DEBUG: Added welcome message");

    Ok(Some(conversation_id))
}

/// CSRF token validation middleware
async fn csrf_middleware(
    State(state): State<WebState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract CSRF token from X-CSRF-Token header
    let token = request
        .headers()
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok());

    match token {
        Some(token) => {
            // Validate the token (but don't consume it - allow reuse within the session)
            if state.csrf_manager.validate_token(token).await {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        None => Err(StatusCode::FORBIDDEN),
    }
}

pub async fn launch_web_ui(state: WebState, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    app_println!("🌐 Web UI starting on http://{} (Ctrl+C to stop)", addr);

    ensure_default_conversation(&state).await?;

    // Routes that require CSRF protection (state-changing operations)
    // Configure CORS to only allow requests from the same origin
    let allowed_origin = format!("http://127.0.0.1:{}", port)
        .parse::<axum::http::HeaderValue>()
        .expect("Invalid origin");

    let cors = CorsLayer::new()
        .allow_origin(allowed_origin)
        .allow_methods(Any)
        .allow_headers(Any);

    let protected_routes = Router::new()
        .route("/api/conversations", post(create_conversation))
        .route(
            "/api/conversations/:id/message",
            post(send_message_to_conversation),
        )
        .route(
            "/api/conversations/:id/message/stream",
            post(stream_message_to_conversation),
        )
        .route("/api/models", post(set_model))
        .route("/api/plans", post(create_plan))
        .route("/api/plans/:id", put(update_plan).delete(delete_plan))
        .route("/api/mcp/servers", post(upsert_mcp_server))
        .route(
            "/api/mcp/servers/:name",
            put(upsert_mcp_server_named).delete(delete_mcp_server),
        )
        .route("/api/mcp/servers/:name/connect", post(connect_mcp_server))
        .route(
            "/api/mcp/servers/:name/disconnect",
            post(disconnect_mcp_server),
        )
        .route("/api/agents", post(create_agent))
        .route("/api/agents/:name", put(update_agent).delete(delete_agent))
        .route("/api/agents/active", post(set_active_agent))
        .route("/api/skills", post(create_skill))
        .route("/api/skills/:name", put(update_skill).delete(delete_skill))
        .route("/api/skills/:name/activate", post(activate_skill))
        .route("/api/skills/:name/deactivate", post(deactivate_skill))
        .route("/api/commands", post(create_custom_command))
        .route(
            "/api/commands/:name",
            put(update_custom_command).delete(delete_custom_command),
        )
        .route("/api/permissions/respond", post(resolve_permission_request))
        .route("/api/plan-mode", post(set_plan_mode))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            csrf_middleware,
        ));

    // Public routes (no CSRF protection needed for GET requests)
    let router = Router::new()
        .route("/", get(serve_index))
        .route("/app.js", get(serve_app_js))
        .route("/api/health", get(health))
        .route("/api/conversations", get(list_conversations))
        .route("/api/conversations/search", get(search_conversations))
        .route("/api/conversations/:id", get(get_conversation))
        .route("/api/models", get(get_models))
        .route("/api/plans", get(list_plans))
        .route("/api/plans/:id", get(get_plan))
        .route("/api/mcp/servers", get(list_mcp_servers))
        .route("/api/mcp/servers/:name", get(get_mcp_server))
        .route("/api/agents", get(list_agents))
        .route("/api/agents/:name", get(get_agent))
        .route("/api/agents/active", get(get_active_agent))
        .route("/api/skills", get(list_skills))
        .route("/api/skills/:name", get(get_skill))
        .route("/api/skills/active", get(get_active_skills))
        .route("/api/commands", get(list_custom_commands))
        .route("/api/commands/:name", get(get_custom_command))
        .route("/api/permissions/pending", get(list_pending_permissions))
        .route("/api/plan-mode", get(get_plan_mode))
        .route("/api/todos", get(list_todos))
        .route("/api/file-autocomplete", get(get_file_autocomplete))
        .route("/api/stats/overview", get(get_stats_overview))
        .route("/api/stats/usage", get(get_usage_stats))
        .route("/api/stats/models", get(get_model_stats))
        .route("/api/stats/conversations", get(get_conversation_stats))
        .route(
            "/api/stats/conversations-by-provider",
            get(get_conversation_stats_by_provider),
        )
        .route(
            "/api/stats/conversations-by-subagent",
            get(get_conversation_stats_by_subagent),
        )
        .merge(protected_routes)
        .with_state(state)
        .layer(cors);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, router).await?;
    Ok(())
}

async fn serve_index(State(state): State<WebState>) -> impl IntoResponse {
    // Generate CSRF token and inject it into the HTML
    let csrf_token = state.csrf_manager.generate_token().await;
    let html_with_token = INDEX_HTML.replace(
        "</head>",
        &format!(
            r#"<script>window.FLEXORAMA_CSRF_TOKEN = "{}";</script></head>"#,
            csrf_token
        ),
    );
    Html(html_with_token)
}

async fn serve_app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], APP_JS)
}

async fn health() -> impl IntoResponse {
    Json(HashMap::from([("status", "ok")]))
}

#[derive(Deserialize)]
struct ConversationListQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_conversations(
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

#[derive(Deserialize)]
struct ConversationSearchQuery {
    query: Option<String>,
}

async fn search_conversations(
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

async fn get_conversation(
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

    // Add images to conversation if provided
    if let Some(images) = &payload.images {
        for image in images {
            agent.add_image(image.media_type.clone(), image.data.clone(), None);
        }
    }

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
        return (StatusCode::BAD_REQUEST, "Model is required".to_string()).into_response();
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
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    let cancellation_flag = Arc::new(AtomicBool::new(false));
    let message = payload.message.clone();
    let images = payload.images.clone();
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

        // Add images to conversation if provided
        if let Some(images) = images {
            for image in images {
                agent.add_image(image.media_type, image.data, None);
            }
        }

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
        .update_plan(
            &id,
            payload.title,
            payload.user_request,
            payload.plan_markdown,
        )
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
        auth: payload.auth,
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

// Skill API handlers
async fn list_skills(State(state): State<WebState>) -> impl IntoResponse {
    let manager = state.skill_manager.lock().await;
    let skills = manager.list_skills();
    let agent = state.agent.lock().await;
    let active_skills = agent.get_active_skills();

    let dtos: Vec<SkillDto> = skills
        .iter()
        .map(|skill| SkillDto {
            name: skill.name.clone(),
            description: skill.description.clone(),
            content: skill.content.clone(),
            allowed_tools: skill.allowed_tools.iter().cloned().collect(),
            denied_tools: skill.denied_tools.iter().cloned().collect(),
            model: skill.model.clone(),
            temperature: skill.temperature,
            max_tokens: skill.max_tokens,
            tags: skill.tags.clone(),
            references: skill.references.iter().map(|r| r.path.clone()).collect(),
            active: active_skills.contains(&skill.name),
            created_at: skill.created_at.to_rfc3339(),
            updated_at: skill.updated_at.to_rfc3339(),
        })
        .collect();

    Json(dtos).into_response()
}

async fn get_skill(State(state): State<WebState>, Path(name): Path<String>) -> impl IntoResponse {
    let manager = state.skill_manager.lock().await;
    let agent = state.agent.lock().await;
    let active_skills = agent.get_active_skills();

    match manager.get_skill(&name) {
        Some(skill) => Json(SkillDto {
            name: skill.name.clone(),
            description: skill.description.clone(),
            content: skill.content.clone(),
            allowed_tools: skill.allowed_tools.iter().cloned().collect(),
            denied_tools: skill.denied_tools.iter().cloned().collect(),
            model: skill.model.clone(),
            temperature: skill.temperature,
            max_tokens: skill.max_tokens,
            tags: skill.tags.clone(),
            references: skill.references.iter().map(|r| r.path.clone()).collect(),
            active: active_skills.contains(&skill.name),
            created_at: skill.created_at.to_rfc3339(),
            updated_at: skill.updated_at.to_rfc3339(),
        })
        .into_response(),
        None => (StatusCode::NOT_FOUND, "Skill not found".to_string()).into_response(),
    }
}

async fn create_skill(
    State(state): State<WebState>,
    Json(payload): Json<NewSkillRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let skill = crate::skill::Skill {
        name: payload.name.clone(),
        description: payload.description,
        content: payload.content,
        allowed_tools: payload.allowed_tools.into_iter().collect(),
        denied_tools: payload.denied_tools.into_iter().collect(),
        model: payload.model,
        temperature: payload.temperature,
        max_tokens: payload.max_tokens,
        tags: payload.tags,
        references: vec![],
        created_at: now,
        updated_at: now,
    };

    let mut manager = state.skill_manager.lock().await;
    match manager.create_skill(skill).await {
        Ok(_) => Json(HashMap::from([("name", payload.name)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create skill: {}", e),
        )
            .into_response(),
    }
}

async fn update_skill(
    State(state): State<WebState>,
    Path(name): Path<String>,
    Json(payload): Json<SkillUpdateRequest>,
) -> impl IntoResponse {
    let mut manager = state.skill_manager.lock().await;

    let existing = match manager.get_skill(&name) {
        Some(s) => s.clone(),
        None => return (StatusCode::NOT_FOUND, "Skill not found".to_string()).into_response(),
    };

    let updated_skill = crate::skill::Skill {
        name: existing.name,
        description: payload.description,
        content: payload.content,
        allowed_tools: payload.allowed_tools.into_iter().collect(),
        denied_tools: payload.denied_tools.into_iter().collect(),
        model: payload.model,
        temperature: payload.temperature,
        max_tokens: payload.max_tokens,
        tags: payload.tags,
        references: existing.references,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    match manager.update_skill(&updated_skill).await {
        Ok(_) => Json(HashMap::from([("name", name)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update skill: {}", e),
        )
            .into_response(),
    }
}

async fn delete_skill(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut manager = state.skill_manager.lock().await;

    match manager.delete_skill(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete skill: {}", e),
        )
            .into_response(),
    }
}

async fn activate_skill(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock().await;

    match agent.activate_skill(&name).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to activate skill: {}", e),
        )
            .into_response(),
    }
}

async fn deactivate_skill(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut agent = state.agent.lock().await;

    match agent.deactivate_skill(&name).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to deactivate skill: {}", e),
        )
            .into_response(),
    }
}

async fn get_active_skills(State(state): State<WebState>) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    let active_skills = agent.get_active_skills();
    Json(active_skills).into_response()
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// Custom command API handlers
async fn list_custom_commands(State(_state): State<WebState>) -> impl IntoResponse {
    match custom_commands::list_custom_commands().await {
        Ok(commands) => {
            let dtos: Vec<CustomCommandDto> = commands
                .into_iter()
                .map(|command| CustomCommandDto {
                    name: command.name,
                    description: command.description.unwrap_or_default(),
                    argument_hint: command.argument_hint,
                    allowed_tools: command.allowed_tools,
                    model: command.model,
                    content: command.content,
                })
                .collect();
            Json(dtos).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list commands: {}", e),
        )
            .into_response(),
    }
}

async fn get_custom_command(
    State(_state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match custom_commands::load_custom_command(&name).await {
        Ok(Some(command)) => Json(CustomCommandDto {
            name: command.name,
            description: command.description.unwrap_or_default(),
            argument_hint: command.argument_hint,
            allowed_tools: command.allowed_tools,
            model: command.model,
            content: command.content,
        })
        .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Command not found".to_string()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch command: {}", e),
        )
            .into_response(),
    }
}

async fn create_custom_command(
    State(_state): State<WebState>,
    Json(payload): Json<NewCustomCommandRequest>,
) -> impl IntoResponse {
    let command = custom_commands::CustomCommand {
        name: payload.name.clone(),
        description: normalize_optional(payload.description),
        argument_hint: payload.argument_hint.and_then(normalize_optional),
        allowed_tools: payload.allowed_tools,
        model: payload.model.and_then(normalize_optional),
        content: payload.content,
    };

    match custom_commands::save_custom_command(&command).await {
        Ok(_) => {
            let name = custom_commands::load_custom_command(&payload.name)
                .await
                .ok()
                .flatten()
                .map(|cmd| cmd.name)
                .unwrap_or(payload.name);
            Json(HashMap::from([("name", name)])).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create command: {}", e),
        )
            .into_response(),
    }
}

async fn update_custom_command(
    State(_state): State<WebState>,
    Path(name): Path<String>,
    Json(payload): Json<CustomCommandUpdateRequest>,
) -> impl IntoResponse {
    let existing = match custom_commands::load_custom_command(&name).await {
        Ok(Some(command)) => command,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Command not found".to_string()).into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load command: {}", e),
            )
                .into_response()
        }
    };

    let updated = custom_commands::CustomCommand {
        name: existing.name.clone(),
        description: normalize_optional(payload.description),
        argument_hint: payload.argument_hint.and_then(normalize_optional),
        allowed_tools: payload.allowed_tools,
        model: payload.model.and_then(normalize_optional),
        content: payload.content,
    };

    match custom_commands::save_custom_command(&updated).await {
        Ok(_) => Json(HashMap::from([("name", existing.name)])).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update command: {}", e),
        )
            .into_response(),
    }
}

async fn delete_custom_command(
    State(_state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match custom_commands::delete_custom_command(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete command: {}", e),
        )
            .into_response(),
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
        (
            StatusCode::NOT_FOUND,
            "Permission request not found".to_string(),
        )
            .into_response()
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

#[derive(Deserialize)]
struct TodoQuery {
    conversation_id: Option<String>,
}

async fn list_todos(
    State(state): State<WebState>,
    Query(query): Query<TodoQuery>,
) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    let todos = agent.get_todos_for(query.conversation_id.as_deref()).await;
    Json(todos).into_response()
}

// Stats API handlers
async fn get_stats_overview(State(state): State<WebState>) -> impl IntoResponse {
    db_result_to_response(
        state.database.get_stats_overview().await,
        "Failed to load stats overview",
        |overview| Json(overview).into_response(),
    )
}

async fn get_usage_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_usage_stats_range(start_date, end_date)
            .await,
        "Failed to load usage stats",
        |stats| {
            let response = UsageStatsResponse {
                period,
                data: stats
                    .into_iter()
                    .map(|s| UsageStatsPoint {
                        date: s.date,
                        total_requests: s.total_requests,
                        total_input_tokens: s.total_input_tokens,
                        total_output_tokens: s.total_output_tokens,
                        total_tokens: s.total_tokens,
                    })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

async fn get_model_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_stats_by_model(start_date, end_date)
            .await,
        "Failed to load model stats",
        |stats| {
            let response = ModelStatsResponse {
                period,
                data: stats
                    .into_iter()
                    .map(|s| ModelStatsPoint {
                        model: s.model.clone(),
                        provider: extract_provider_from_model(&s.model),
                        total_conversations: s.total_conversations,
                        total_tokens: s.total_tokens,
                        request_count: s.request_count,
                    })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

async fn get_conversation_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date(start_date, end_date)
            .await,
        "Failed to load conversation stats",
        |counts| {
            let response = ConversationStatsResponse {
                period,
                data: counts
                    .into_iter()
                    .map(|(date, count)| ConversationStatsPoint { date, count })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

async fn get_conversation_stats_by_provider(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date_and_model(start_date, end_date)
            .await,
        "Failed to load conversation stats by provider",
        |counts| {
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

            let response = ConversationsByProviderResponse { period, data };
            Json(response).into_response()
        },
    )
}

async fn get_conversation_stats_by_subagent(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date_and_subagent(start_date, end_date)
            .await,
        "Failed to load conversation stats by subagent",
        |counts| {
            let data: Vec<ConversationsBySubagentPoint> = counts
                .into_iter()
                .map(|(date, subagent, count)| ConversationsBySubagentPoint {
                    date,
                    subagent,
                    count,
                })
                .collect();

            let response = ConversationsBySubagentResponse { period, data };
            Json(response).into_response()
        },
    )
}

fn extract_provider_from_model(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower.contains("claude") {
        "Anthropic".to_string()
    } else if lower.contains("gpt") {
        "OpenAI".to_string()
    } else if lower.contains("gemini") {
        "Gemini".to_string()
    } else if lower.contains("mistral") {
        "Mistral".to_string()
    } else if lower.contains("glm") {
        "Z.AI".to_string()
    } else if lower.contains("llama") || lower.contains("gemma") {
        "Ollama".to_string()
    } else {
        "Other".to_string()
    }
}

async fn get_file_autocomplete(
    axum::extract::Query(params): axum::extract::Query<FileAutocompleteQuery>,
) -> impl IntoResponse {
    use std::path::{Component, Path, PathBuf};

    let prefix = params.prefix.trim();
    let root = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    fn resolve_search_dir(root: &Path, dir_part: &str) -> Option<PathBuf> {
        if dir_part.is_empty() || dir_part == "." {
            return Some(root.to_path_buf());
        }

        let mut resolved = root.to_path_buf();
        let base_depth = resolved.components().count();
        let rel_path = Path::new(dir_part);

        for component in rel_path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => resolved.push(part),
                Component::ParentDir => {
                    if resolved.components().count() <= base_depth {
                        return None;
                    }
                    resolved.pop();
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }

        Some(resolved)
    }

    // Determine the search directory and filter pattern
    let (search_dir, filter_prefix) = if prefix.is_empty() {
        (".", "")
    } else if prefix.ends_with('/') || prefix.ends_with('\\') {
        // If prefix ends with separator, list contents of that directory
        (prefix, "")
    } else {
        // Split the prefix into directory and filename parts
        let path = Path::new(prefix);
        if let Some(parent) = path.parent() {
            let parent_str = if parent.as_os_str().is_empty() {
                "."
            } else {
                parent.to_str().unwrap_or(".")
            };
            let file_part = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            (parent_str, file_part)
        } else {
            (".", prefix)
        }
    };

    let search_dir_path = match resolve_search_dir(&root, search_dir) {
        Some(path) => path,
        None => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    let entries = match std::fs::read_dir(&search_dir_path) {
        Ok(entries) => entries,
        Err(_) => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    let mut files: Vec<FileAutocompleteItem> = Vec::new();
    let filter_lower = filter_prefix.to_lowercase();

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

            if filter_prefix.is_empty() || filename.to_lowercase().starts_with(&filter_lower) {
                let relative_path = match path.strip_prefix(&root) {
                    Ok(rel_path) => rel_path,
                    Err(_) => continue,
                };

                files.push(FileAutocompleteItem {
                    path: relative_path.to_string_lossy().to_string(),
                    is_directory: path.is_dir(),
                });

                if files.len() >= 50 {
                    break;
                }
            }
        }
    }

    files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
    });

    Json(FileAutocompleteResponse { files }).into_response()
}

/// Helper to handle database results and convert to HTTP responses with consistent error handling
fn db_result_to_response<T, F>(result: Result<T>, error_msg: &str, transform: F) -> Response
where
    F: FnOnce(T) -> Response,
{
    match result {
        Ok(data) => transform(data),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{}: {}", error_msg, e),
        )
            .into_response(),
    }
}

fn calculate_date_range(
    params: &StatsQueryParams,
) -> (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) {
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
        source: block.source.as_ref().map(|s| ImageSourceDto {
            source_type: s.source_type.clone(),
            media_type: s.media_type.clone(),
            data: s.data.clone(),
        }),
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
        "image" => "[Image]".to_string(),
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
        Some(build_message_dto(id, role, created_at, filtered_blocks))
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

fn parse_tool_arguments(arg_text: &str) -> serde_json::Value {
    serde_json::from_str(arg_text)
        .unwrap_or_else(|_| serde_json::Value::String(arg_text.to_string()))
}

fn is_todo_tool(name: &str) -> bool {
    matches!(name, "create_todo" | "complete_todo" | "list_todos")
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{delete, put};
    use chrono::TimeZone;
    use http_body_util::BodyExt;
    use serial_test::serial;
    use std::sync::OnceLock;
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    struct EnvGuard {
        commands_dir: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_commands_dir(path: &std::path::Path) -> Self {
            let commands_dir = std::env::var_os("FLEXORAMA_COMMANDS_DIR");
            std::env::set_var("FLEXORAMA_COMMANDS_DIR", path);
            Self { commands_dir }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.commands_dir.take() {
                std::env::set_var("FLEXORAMA_COMMANDS_DIR", value);
            } else {
                std::env::remove_var("FLEXORAMA_COMMANDS_DIR");
            }
        }
    }

    #[test]
    fn test_extract_provider_from_model() {
        assert_eq!(extract_provider_from_model("claude-3-opus"), "Anthropic");
        assert_eq!(extract_provider_from_model("gpt-4o"), "OpenAI");
        assert_eq!(extract_provider_from_model("gemini-1.5-pro"), "Gemini");
        assert_eq!(
            extract_provider_from_model("mistral-large-latest"),
            "Mistral"
        );
        assert_eq!(extract_provider_from_model("glm-4.6"), "Z.AI");
        assert_eq!(extract_provider_from_model("local-model"), "Other");
    }

    #[test]
    fn test_calculate_date_range_with_custom_dates() {
        let params = StatsQueryParams {
            period: Some("week".to_string()),
            start_date: Some("2024-01-01".to_string()),
            end_date: Some("2024-01-31".to_string()),
        };
        let (start, end) = calculate_date_range(&params);
        assert_eq!(
            start,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
        );
        assert_eq!(
            end,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 31).unwrap())
        );
    }

    #[test]
    fn test_calculate_date_range_with_period() {
        let params = StatsQueryParams {
            period: Some("week".to_string()),
            start_date: None,
            end_date: None,
        };
        let (start, end) = calculate_date_range(&params);
        let now = Utc::now().naive_utc().date();
        assert_eq!(start, Some(now - Duration::days(7)));
        assert_eq!(end, Some(now));
    }

    #[test]
    fn test_parse_tool_arguments() {
        let parsed = parse_tool_arguments("{\"key\": 1}");
        assert_eq!(parsed, serde_json::json!({"key": 1}));

        let fallback = parse_tool_arguments("not-json");
        assert_eq!(fallback, serde_json::Value::String("not-json".to_string()));
    }

    #[test]
    fn test_build_visible_message_dto_filters_context_blocks() {
        let context_block = ContentBlock::text("Context from file 'foo.txt': example".to_string());
        let regular_block = ContentBlock::text("Hello there".to_string());
        let visible = build_visible_message_dto(
            "id-1".to_string(),
            "assistant".to_string(),
            "2024-01-01T00:00:00Z".to_string(),
            vec![context_block.clone(), regular_block.clone()],
        )
        .expect("expected visible message");
        assert_eq!(visible.blocks.len(), 1);
        assert_eq!(visible.content, "Hello there");

        let hidden = build_visible_message_dto(
            "id-2".to_string(),
            "assistant".to_string(),
            "2024-01-01T00:00:00Z".to_string(),
            vec![context_block],
        );
        assert!(hidden.is_none());
    }

    #[test]
    fn test_extract_context_files_from_messages() {
        let message = build_message_dto(
            "msg-1".to_string(),
            "assistant".to_string(),
            "2024-01-01T00:00:00Z".to_string(),
            vec![ContentBlock::text(
                "Context from file 'foo.txt': example content".to_string(),
            )],
        );
        let message_two = build_message_dto(
            "msg-2".to_string(),
            "assistant".to_string(),
            "2024-01-02T00:00:00Z".to_string(),
            vec![ContentBlock::text(
                "Context from file 'bar.md': details".to_string(),
            )],
        );
        let files = extract_context_files_from_messages(&[message, message_two]);
        assert_eq!(files, vec!["foo.txt".to_string(), "bar.md".to_string()]);
    }

    #[test]
    fn test_timeline_messages_to_dto_includes_tool_calls() {
        let created_at = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let message = crate::database::Message {
            id: "msg-1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at,
        };
        let tool_call = ToolCallRecord {
            id: "tool-1".to_string(),
            tool_name: "Read".to_string(),
            tool_arguments: "{\"path\":\"/tmp/file.txt\"}".to_string(),
            result_content: Some("Not found".to_string()),
            is_error: true,
            created_at: created_at + Duration::seconds(1),
        };

        let timeline = timeline_messages_to_dto(vec![message], vec![tool_call]);
        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline[0].content, "Hello");
        assert!(timeline[1].content.contains("Tool call: Read"));
        assert!(timeline[2].content.contains("(error) Not found"));
        assert_eq!(timeline[2].id, "tool-1-result");
    }

    fn init_test_home() -> std::path::PathBuf {
        static TEST_HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
        TEST_HOME
            .get_or_init(|| {
                let temp_dir = tempdir().expect("create tempdir");
                let root = temp_dir.path().to_path_buf();
                std::mem::forget(temp_dir);
                root
            })
            .clone()
    }

    async fn build_test_state() -> WebState {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path().to_path_buf();
        let db_path = root.join("test.sqlite");
        let agents_dir = root.join("agents");
        let config_path = root.join("config.toml");
        let database = Arc::new(
            DatabaseManager::new(db_path)
                .await
                .expect("create database"),
        );
        std::mem::forget(temp_dir);
        let test_home = init_test_home();
        std::env::set_var("USERPROFILE", &test_home);
        std::env::set_var("HOME", &test_home);

        let config = config::Config::default();
        let config_arc = Arc::new(tokio::sync::RwLock::new(config.clone()));
        let skill_manager = Arc::new(Mutex::new(
            SkillManager::new(config_arc).expect("create skill manager"),
        ));
        {
            let mut manager = skill_manager.lock().await;
            manager.set_test_paths(root.join("skills"), config_path.clone());
        }
        let agent =
            Agent::new_with_plan_mode(config.clone(), config.default_model.clone(), false, false)
                .await
                .with_database_manager(database.clone())
                .with_skill_manager(skill_manager.clone());

        let subagent_manager = Arc::new(Mutex::new(
            SubagentManager::new_with_dir(agents_dir).expect("create subagent manager"),
        ));

        WebState {
            agent: Arc::new(Mutex::new(agent)),
            database,
            mcp_manager: Arc::new(McpManager::new_with_config_path(config_path)),
            subagent_manager,
            permission_hub: Arc::new(PermissionHub::new()),
            skill_manager,
            conversation_agents: Arc::new(Mutex::new(HashMap::new())),
            csrf_manager: Arc::new(CsrfManager::new()),
            config: Arc::new(config),
        }
    }

    fn build_test_router(state: WebState) -> Router {
        Router::new()
            .route("/api/health", get(health))
            .route("/api/models", get(get_models).post(set_model))
            .route("/api/plan-mode", get(get_plan_mode).post(set_plan_mode))
            .route("/api/conversations", get(list_conversations))
            .route("/api/conversations/search", get(search_conversations))
            .route("/api/conversations/:id", get(get_conversation))
            .route("/api/plans", get(list_plans).post(create_plan))
            .route("/api/permissions/pending", get(list_pending_permissions))
            .route("/api/permissions/respond", post(resolve_permission_request))
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
            .route("/api/agents", get(list_agents).post(create_agent))
            .route(
                "/api/agents/:name",
                get(get_agent).put(update_agent).delete(delete_agent),
            )
            .route("/api/agents/active", get(get_active_agent))
            .route("/api/skills", get(list_skills).post(create_skill))
            .route(
                "/api/skills/:name",
                get(get_skill).put(update_skill).delete(delete_skill),
            )
            .route("/api/skills/:name/activate", post(activate_skill))
            .route("/api/skills/:name/deactivate", post(deactivate_skill))
            .route("/api/skills/active", get(get_active_skills))
            .route(
                "/api/commands",
                get(list_custom_commands).post(create_custom_command),
            )
            .route(
                "/api/commands/:name",
                get(get_custom_command)
                    .put(update_custom_command)
                    .delete(delete_custom_command),
            )
            .with_state(state)
    }

    async fn json_response(
        router: &Router,
        request: axum::http::Request<Body>,
    ) -> (StatusCode, serde_json::Value) {
        let response = router.clone().oneshot(request).await.expect("send request");
        let status = response.status();
        let body = response.into_body().collect().await.expect("read body");
        let value = serde_json::from_slice(&body.to_bytes()).expect("parse json");
        (status, value)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/health")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn test_models_endpoint() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/models")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let active_model = body["active_model"].as_str().expect("active model");
        let models = body["models"].as_array().expect("models array");
        assert!(models.iter().any(|m| m.as_str() == Some(active_model)));
        assert!(body["provider"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_plan_mode_endpoint() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/plan-mode")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"enabled":true}"#))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], true);

        let request = axum::http::Request::builder()
            .uri("/api/plan-mode")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], true);
    }

    #[tokio::test]
    async fn test_skills_crud_and_activation() {
        let state = build_test_state().await;
        let router = build_test_router(state);

        let payload = serde_json::json!({
            "name": "skill-one",
            "description": "Example skill",
            "content": "## Instructions\n\nDo the thing.\n",
            "allowed_tools": [],
            "denied_tools": [],
            "model": null,
            "temperature": null,
            "max_tokens": null,
            "tags": []
        });
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/skills")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("skill-one"));

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/skills")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let skills = body.as_array().expect("skills array");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["name"], "skill-one");
        assert_eq!(skills[0]["active"], false);

        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/skills/skill-one/activate")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("activate");
        assert_eq!(response.status(), StatusCode::OK);

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/skills/skill-one")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["active"], true);

        let update = serde_json::json!({
            "description": "Updated description",
            "content": "## Instructions\n\nUpdated.\n",
            "allowed_tools": ["Read"],
            "denied_tools": [],
            "model": "glm-4.6",
            "temperature": 0.5,
            "max_tokens": 2048,
            "tags": ["test"]
        });
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/api/skills/skill-one")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("skill-one"));

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/skills/skill-one")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["description"], "Updated description");
        assert_eq!(body["allowed_tools"][0], "Read");

        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/skills/skill-one/deactivate")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("deactivate");
        assert_eq!(response.status(), StatusCode::OK);

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/skills/active")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let active = body.as_array().expect("active array");
        assert!(active.is_empty());

        let request = axum::http::Request::builder()
            .method("DELETE")
            .uri("/api/skills/skill-one")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("delete");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    #[serial]
    async fn test_custom_commands_crud() {
        let state = build_test_state().await;
        let commands_root = tempdir().expect("commands dir");
        let _guard = EnvGuard::set_commands_dir(commands_root.path());
        let router = build_test_router(state);

        let payload = serde_json::json!({
            "name": "cmd-alpha",
            "description": "Command alpha",
            "argument_hint": "[issue-number]",
            "allowed_tools": ["Bash(git status:*)"],
            "model": "test-model",
            "content": "Fix issue $ARGUMENTS"
        });
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/commands")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("cmd-alpha"));

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/commands")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let commands = body.as_array().expect("commands array");
        assert!(commands.iter().any(|cmd| cmd["name"] == "cmd-alpha"));

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/commands/cmd-alpha")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["description"], "Command alpha");
        assert_eq!(body["argument_hint"], "[issue-number]");
        assert_eq!(body["allowed_tools"][0], "Bash(git status:*)");
        assert_eq!(body["model"], "test-model");

        let update = serde_json::json!({
            "description": "Updated",
            "argument_hint": "[ticket]",
            "allowed_tools": [],
            "model": null,
            "content": "Updated content $1"
        });
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/api/commands/cmd-alpha")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("cmd-alpha"));

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/commands/cmd-alpha")
            .body(Body::empty())
            .unwrap();
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["description"], "Updated");
        assert_eq!(body["argument_hint"], "[ticket]");
        assert!(body["allowed_tools"]
            .as_array()
            .expect("allowed tools")
            .is_empty());
        assert!(body["model"].is_null());
        assert_eq!(body["content"], "Updated content $1");

        let request = axum::http::Request::builder()
            .method("DELETE")
            .uri("/api/commands/cmd-alpha")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("delete");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/api/commands/cmd-alpha")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("get missing");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_conversations_endpoint() {
        let state = build_test_state().await;
        let conversation_id = state
            .database
            .create_conversation(None, "test-model", None)
            .await
            .expect("create conversation");
        state
            .database
            .add_message(&conversation_id, "user", "Hello", "test-model", 1)
            .await
            .expect("add message");

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/conversations")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().expect("expected list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["message_count"], 1);
        assert_eq!(items[0]["last_message"], "Hello");
    }

    #[tokio::test]
    async fn test_search_conversations_endpoint() {
        let state = build_test_state().await;
        let matching_id = state
            .database
            .create_conversation(None, "test-model", None)
            .await
            .expect("create conversation");
        state
            .database
            .add_message(&matching_id, "user", "Hello alpha", "test-model", 1)
            .await
            .expect("add message");

        let other_id = state
            .database
            .create_conversation(None, "test-model", None)
            .await
            .expect("create conversation");
        state
            .database
            .add_message(&other_id, "user", "Beta message", "test-model", 1)
            .await
            .expect("add message");

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/conversations/search?query=alpha")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().expect("expected list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["last_message"], "Hello alpha");
    }

    #[tokio::test]
    async fn test_ensure_default_conversation_creates_when_empty() {
        let state = build_test_state().await;
        let created = ensure_default_conversation(&state)
            .await
            .expect("ensure default conversation");
        assert!(created.is_some());

        let conversations = state
            .database
            .get_recent_conversations(10, None)
            .await
            .expect("list conversations");
        assert_eq!(conversations.len(), 1);
        let messages = state
            .database
            .get_conversation_messages(&conversations[0].id)
            .await
            .expect("get messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
    }

    #[tokio::test]
    async fn test_ensure_default_conversation_noop_when_existing() {
        let state = build_test_state().await;
        let conversation_id = state
            .database
            .create_conversation(None, "test-model", None)
            .await
            .expect("create conversation");
        state
            .database
            .add_message(&conversation_id, "user", "Hello", "test-model", 1)
            .await
            .expect("add message");

        let created = ensure_default_conversation(&state)
            .await
            .expect("ensure default conversation");
        assert!(created.is_none());

        let conversations = state
            .database
            .get_recent_conversations(10, None)
            .await
            .expect("list conversations");
        assert_eq!(conversations.len(), 1);
    }

    #[tokio::test]
    async fn test_get_conversation_not_found() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/conversations/missing")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_plan_and_list_plans() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/plans")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"title":"Plan A","user_request":"Do work","plan_markdown":"Step 1"}"#,
            ))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["id"].as_str().is_some());

        let request = axum::http::Request::builder()
            .uri("/api/plans")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let plans = body.as_array().expect("plans array");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0]["title"], "Plan A");
    }

    #[tokio::test]
    async fn test_permissions_pending_empty() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/permissions/pending")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        let pending = body.as_array().expect("pending array");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_permissions_respond_requires_id() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/permissions/respond")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"id":""}"#))
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_model_endpoint() {
        let state = build_test_state().await;
        let router = build_test_router(state.clone());
        let request = axum::http::Request::builder()
            .uri("/api/models")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"glm-4.6"}"#))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["model"], "glm-4.6");
    }

    #[tokio::test]
    async fn test_set_model_rejects_empty() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/models")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"  "}"#))
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_conversation_success() {
        let state = build_test_state().await;
        let conversation_id = state
            .database
            .create_conversation(Some("Test prompt".to_string()), "test-model", None)
            .await
            .expect("create conversation");
        state
            .database
            .add_message(&conversation_id, "user", "Hello", "test-model", 1)
            .await
            .expect("add message");

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri(&format!("/api/conversations/{}", conversation_id))
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["conversation"]["id"], conversation_id);
        assert_eq!(body["conversation"]["system_prompt"], "Test prompt");
        let messages = body["messages"].as_array().expect("messages array");
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_get_plan_success() {
        let state = build_test_state().await;
        let plan_id = state
            .database
            .create_plan(None, Some("Test Plan"), "Do work", "## Step 1")
            .await
            .expect("create plan");

        let router = Router::new()
            .route("/api/plans/:id", get(get_plan))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri(&format!("/api/plans/{}", plan_id))
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], plan_id);
        assert_eq!(body["title"], "Test Plan");
        assert_eq!(body["user_request"], "Do work");
    }

    #[tokio::test]
    async fn test_get_plan_not_found() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/plans/:id", get(get_plan))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/plans/missing-plan-id")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_plan() {
        let state = build_test_state().await;
        let plan_id = state
            .database
            .create_plan(None, Some("Original"), "Original request", "Original plan")
            .await
            .expect("create plan");

        let router = Router::new()
            .route("/api/plans/:id", put(update_plan))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri(&format!("/api/plans/{}", plan_id))
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"title":"Updated","user_request":"Updated request","plan_markdown":"Updated plan"}"#,
            ))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["title"], "Updated");
        assert_eq!(body["user_request"], "Updated request");
        assert_eq!(body["plan_markdown"], "Updated plan");
    }

    #[tokio::test]
    async fn test_delete_plan() {
        let state = build_test_state().await;
        let plan_id = state
            .database
            .create_plan(None, Some("To Delete"), "Work", "Plan")
            .await
            .expect("create plan");

        let router = Router::new()
            .route("/api/plans/:id", delete(delete_plan))
            .with_state(state.clone());
        let request = axum::http::Request::builder()
            .uri(&format!("/api/plans/{}", plan_id))
            .method("DELETE")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify plan is deleted
        let result = state.database.get_plan(&plan_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_mcp_servers() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/mcp/servers", get(list_mcp_servers))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn test_upsert_mcp_server() {
        let state = build_test_state().await;
        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"name":"test-server","command":"node","args":["server.js"],"enabled":false}"#,
            ))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "test-server");
    }

    #[tokio::test]
    async fn test_upsert_mcp_server_requires_name() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/mcp/servers", post(upsert_mcp_server))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"command":"node"}"#))
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_upsert_mcp_server_requires_command_or_url() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/mcp/servers", post(upsert_mcp_server))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"name":"test-server","enabled":true}"#))
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_mcp_server() {
        let state = build_test_state().await;
        let config = McpServerConfig {
            name: "test-server".to_string(),
            command: Some("node".to_string()),
            args: Some(vec!["server.js".to_string()]),
            url: None,
            env: None,
            auth: None,
            enabled: false,
        };
        state
            .mcp_manager
            .upsert_server("test-server", config)
            .await
            .expect("upsert server");

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers/test-server")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "test-server");
        assert_eq!(body["config"]["command"], "node");
    }

    #[tokio::test]
    async fn test_get_mcp_server_not_found() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/mcp/servers/:name", get(get_mcp_server))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers/missing-server")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_mcp_server() {
        let state = build_test_state().await;
        let config = McpServerConfig {
            name: "test-server".to_string(),
            command: Some("node".to_string()),
            args: None,
            url: None,
            env: None,
            auth: None,
            enabled: false,
        };
        state
            .mcp_manager
            .upsert_server("test-server", config)
            .await
            .expect("upsert server");

        let router = build_test_router(state.clone());
        let request = axum::http::Request::builder()
            .uri("/api/mcp/servers/test-server")
            .method("DELETE")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify server is deleted
        let result = state.mcp_manager.get_server("test-server").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_agents() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/agents", get(list_agents))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn test_create_agent() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/agents", post(create_agent))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"name":"test-agent","system_prompt":"You are a test","allowed_tools":[],"denied_tools":[]}"#,
            ))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "test-agent");
    }

    #[tokio::test]
    async fn test_get_agent() {
        let state = build_test_state().await;
        let config = SubagentConfig {
            name: "test-agent".to_string(),
            system_prompt: "You are a test".to_string(),
            allowed_tools: vec![].into_iter().collect(),
            denied_tools: vec![].into_iter().collect(),
            max_tokens: None,
            temperature: None,
            model: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let mut manager = state.subagent_manager.lock().await;
        manager.save_subagent(&config).await.expect("save agent");
        manager.load_all_subagents().await.expect("load agents");
        drop(manager);

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents/test-agent")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "test-agent");
        assert_eq!(body["system_prompt"], "You are a test");
    }

    #[tokio::test]
    async fn test_get_agent_not_found() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/agents/:name", get(get_agent))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents/missing-agent")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_agent() {
        let state = build_test_state().await;
        let config = SubagentConfig {
            name: "test-agent".to_string(),
            system_prompt: "Original".to_string(),
            allowed_tools: vec![].into_iter().collect(),
            denied_tools: vec![].into_iter().collect(),
            max_tokens: None,
            temperature: None,
            model: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let mut manager = state.subagent_manager.lock().await;
        manager.save_subagent(&config).await.expect("save agent");
        manager.load_all_subagents().await.expect("load agents");
        drop(manager);

        let router = build_test_router(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents/test-agent")
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"system_prompt":"Updated","allowed_tools":["Read"],"denied_tools":[]}"#,
            ))
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "test-agent");
    }

    #[tokio::test]
    async fn test_delete_agent() {
        let state = build_test_state().await;
        let config = SubagentConfig {
            name: "test-agent".to_string(),
            system_prompt: "Test".to_string(),
            allowed_tools: vec![].into_iter().collect(),
            denied_tools: vec![].into_iter().collect(),
            max_tokens: None,
            temperature: None,
            model: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        state
            .subagent_manager
            .lock()
            .await
            .save_subagent(&config)
            .await
            .expect("save agent");

        let router = Router::new()
            .route("/api/agents/:name", delete(delete_agent))
            .with_state(state.clone());
        let request = axum::http::Request::builder()
            .uri("/api/agents/test-agent")
            .method("DELETE")
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("send request");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify agent is deleted
        let manager = state.subagent_manager.lock().await;
        assert!(manager.get_subagent("test-agent").is_none());
    }

    #[tokio::test]
    async fn test_get_active_agent() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/agents/active", get(get_active_agent))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/agents/active")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["active"].is_null());
    }

    #[tokio::test]
    async fn test_stats_overview() {
        let state = build_test_state().await;
        state
            .database
            .create_conversation(None, "test-model", None)
            .await
            .expect("create conversation");

        let router = Router::new()
            .route("/api/stats/overview", get(get_stats_overview))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/overview")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["total_conversations"].as_i64().is_some());
        assert!(body["total_messages"].as_i64().is_some());
        assert!(body["total_tokens"].as_i64().is_some());
    }

    #[tokio::test]
    async fn test_usage_stats() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/stats/usage", get(get_usage_stats))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/usage?period=month")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["period"], "month");
        assert!(body["data"].is_array());
    }

    #[tokio::test]
    async fn test_model_stats() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/stats/models", get(get_model_stats))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/models?period=week")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["period"], "week");
        assert!(body["data"].is_array());
    }

    #[tokio::test]
    async fn test_conversation_stats() {
        let state = build_test_state().await;
        let router = Router::new()
            .route("/api/stats/conversations", get(get_conversation_stats))
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/conversations?period=day")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["period"], "day");
        assert!(body["data"].is_array());
    }

    #[tokio::test]
    async fn test_conversation_stats_by_provider() {
        let state = build_test_state().await;
        let router = Router::new()
            .route(
                "/api/stats/conversations-by-provider",
                get(get_conversation_stats_by_provider),
            )
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/conversations-by-provider?period=month")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["period"], "month");
        assert!(body["data"].is_array());
    }

    #[tokio::test]
    async fn test_conversation_stats_by_subagent() {
        let state = build_test_state().await;
        let router = Router::new()
            .route(
                "/api/stats/conversations-by-subagent",
                get(get_conversation_stats_by_subagent),
            )
            .with_state(state);
        let request = axum::http::Request::builder()
            .uri("/api/stats/conversations-by-subagent?period=lifetime")
            .method("GET")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = json_response(&router, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["period"], "lifetime");
        assert!(body["data"].is_array());
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_empty_prefix() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create test files and directories
        std::fs::write(root.join("test.txt"), "content").expect("create file");
        std::fs::write(root.join("example.rs"), "code").expect("create file");
        std::fs::create_dir(root.join("src")).expect("create dir");

        // Change to temp directory for test
        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        // Restore original directory
        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        assert!(body["files"].is_array());
        let files = body["files"].as_array().expect("files array");
        assert!(files.len() > 0);

        // Check that we got our test files
        let paths: Vec<String> = files
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("test.txt")));
        assert!(paths.iter().any(|p| p.ends_with("example.rs")));
        assert!(paths.iter().any(|p| p.ends_with("src")));
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_case_insensitive() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create test files with various cases
        std::fs::write(root.join("Cargo.toml"), "content").expect("create file");
        std::fs::write(root.join("CARGO.lock"), "content").expect("create file");
        std::fs::write(root.join("cargo.rs"), "content").expect("create file");
        std::fs::write(root.join("test.txt"), "content").expect("create file");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        // Test lowercase prefix matching uppercase files
        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=car")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        // Should match all cargo files regardless of case
        let paths: Vec<String> = files
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p.ends_with("Cargo.toml")),
            "Should find Cargo.toml"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("CARGO.lock")),
            "Should find CARGO.lock"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("cargo.rs")),
            "Should find cargo.rs"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("test.txt")),
            "Should not find test.txt"
        );
        assert_eq!(files.len(), 3);
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_with_prefix() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create test files
        std::fs::write(root.join("test.txt"), "content").expect("create file");
        std::fs::write(root.join("testing.rs"), "code").expect("create file");
        std::fs::write(root.join("example.rs"), "code").expect("create file");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=test")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        let paths: Vec<String> = files
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p.ends_with("test.txt")),
            "Should find test.txt"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("testing.rs")),
            "Should find testing.rs"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("example.rs")),
            "Should not find example.rs"
        );
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_directory_flag() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create files and directories
        std::fs::write(root.join("file.txt"), "content").expect("create file");
        std::fs::create_dir(root.join("src")).expect("create dir");
        std::fs::create_dir(root.join("target")).expect("create dir");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        // Check directory flags
        for file in files.iter() {
            let path = file["path"].as_str().expect("path");
            let is_dir = file["is_directory"].as_bool().expect("is_directory");

            if path.contains("src") || path.contains("target") {
                assert!(is_dir, "Directory should be flagged as is_directory");
            } else if path.contains("file.txt") {
                assert!(!is_dir, "File should not be flagged as is_directory");
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_sorts_directories_first() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create mix of files and directories
        std::fs::write(root.join("afile.txt"), "content").expect("create file");
        std::fs::create_dir(root.join("zdir")).expect("create dir");
        std::fs::write(root.join("bfile.rs"), "code").expect("create file");
        std::fs::create_dir(root.join("mdir")).expect("create dir");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        // Find indices of directories and files
        let mut first_dir_idx = None;
        let mut last_dir_idx = None;
        let mut first_file_idx = None;

        for (i, file) in files.iter().enumerate() {
            let is_dir = file["is_directory"].as_bool().expect("is_directory");
            if is_dir {
                if first_dir_idx.is_none() {
                    first_dir_idx = Some(i);
                }
                last_dir_idx = Some(i);
            } else if first_file_idx.is_none() {
                first_file_idx = Some(i);
            }
        }

        // Directories should come before files
        if let (Some(last_dir), Some(first_file)) = (last_dir_idx, first_file_idx) {
            assert!(
                last_dir < first_file,
                "All directories should appear before files"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_subdirectory_prefix() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create subdirectory with files
        let src_dir = root.join("src");
        std::fs::create_dir(&src_dir).expect("create dir");
        std::fs::write(src_dir.join("main.rs"), "code").expect("create file");
        std::fs::write(src_dir.join("lib.rs"), "code").expect("create file");
        std::fs::write(root.join("other.txt"), "content").expect("create file");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=src/m")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        // Should only match main.rs in src directory
        assert_eq!(files.len(), 1);
        let path = files[0]["path"].as_str().expect("path");
        assert!(path.contains("main.rs"));
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_blocks_parent_traversal() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();
        let parent = root.parent().expect("parent dir");

        std::fs::write(root.join("safe.txt"), "content").expect("create file");
        std::fs::write(parent.join("outside.txt"), "content").expect("create file");

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=../")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");
        let _ = std::fs::remove_file(parent.join("outside.txt"));

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");
        assert!(files.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_file_autocomplete_limit_50_results() {
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path();

        // Create more than 50 files
        for i in 0..100 {
            std::fs::write(root.join(format!("file{:03}.txt", i)), "content").expect("create file");
        }

        let original_dir = std::env::current_dir().expect("get current dir");
        std::env::set_current_dir(root).expect("change dir");

        let router = Router::new().route("/api/file-autocomplete", get(get_file_autocomplete));

        let request = axum::http::Request::builder()
            .uri("/api/file-autocomplete?prefix=file")
            .method("GET")
            .body(Body::empty())
            .expect("build request");

        let (status, body) = json_response(&router, request).await;

        std::env::set_current_dir(original_dir).expect("restore dir");

        assert_eq!(status, StatusCode::OK);
        let files = body["files"].as_array().expect("files array");

        // Should be limited to 50 results
        assert_eq!(files.len(), 50);
    }

    #[tokio::test]
    async fn test_webstate_config_is_passed_to_conversation_agents() {
        // Create a test state with a custom config
        let temp_dir = tempdir().expect("create tempdir");
        let root = temp_dir.path().to_path_buf();
        let db_path = root.join("test.sqlite");
        let agents_dir = root.join("agents");
        let config_path = root.join("config.toml");
        let database = Arc::new(
            DatabaseManager::new(db_path)
                .await
                .expect("create database"),
        );
        std::mem::forget(temp_dir);
        let test_home = init_test_home();
        std::env::set_var("USERPROFILE", &test_home);
        std::env::set_var("HOME", &test_home);

        // Create a config with Mistral provider and custom API key
        let mut config = config::Config::default();
        config.provider = config::Provider::Mistral;
        config.api_key = "test-mistral-api-key-12345".to_string();
        config.default_model = "mistral-large-latest".to_string();

        let config_arc = Arc::new(tokio::sync::RwLock::new(config.clone()));
        let skill_manager = Arc::new(Mutex::new(
            SkillManager::new(config_arc).expect("create skill manager"),
        ));
        {
            let mut manager = skill_manager.lock().await;
            manager.set_test_paths(root.join("skills"), config_path.clone());
        }
        let agent =
            Agent::new_with_plan_mode(config.clone(), config.default_model.clone(), false, false)
                .await
                .with_database_manager(database.clone())
                .with_skill_manager(skill_manager.clone());

        let subagent_manager = Arc::new(Mutex::new(
            SubagentManager::new_with_dir(agents_dir).expect("create subagent manager"),
        ));

        let state = WebState {
            agent: Arc::new(Mutex::new(agent)),
            database: database.clone(),
            mcp_manager: Arc::new(McpManager::new_with_config_path(config_path)),
            subagent_manager,
            permission_hub: Arc::new(PermissionHub::new()),
            skill_manager,
            conversation_agents: Arc::new(Mutex::new(HashMap::new())),
            csrf_manager: Arc::new(CsrfManager::new()),
            config: Arc::new(config.clone()),
        };

        // Create a conversation
        let conversation_id = database
            .create_conversation(None, &config.default_model, None)
            .await
            .expect("create conversation");

        // Get or create conversation agent
        let agent_arc = get_or_create_conversation_agent(&state, &conversation_id)
            .await
            .expect("get or create conversation agent");

        // Verify the agent has the correct provider (which indicates it got the right config)
        let agent = agent_arc.lock().await;
        assert_eq!(agent.provider(), config::Provider::Mistral);
        assert_eq!(agent.model(), "mistral-large-latest");
    }

    #[tokio::test]
    async fn test_conversation_agents_are_cached() {
        let state = build_test_state().await;

        // Create a conversation
        let conversation_id = state
            .database
            .create_conversation(None, "claude-sonnet-4-5", None)
            .await
            .expect("create conversation");

        // Get agent for first time
        let agent1 = get_or_create_conversation_agent(&state, &conversation_id)
            .await
            .expect("get agent first time");

        // Get agent for second time
        let agent2 = get_or_create_conversation_agent(&state, &conversation_id)
            .await
            .expect("get agent second time");

        // Verify they are the same Arc (same memory address)
        assert!(Arc::ptr_eq(&agent1, &agent2));
    }

    #[test]
    fn test_block_to_dto_with_image_source() {
        let image_block = ContentBlock::image(
            "image/png".to_string(),
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string(),
        );

        let dto = block_to_dto(&image_block);

        assert_eq!(dto.block_type, "image");
        assert!(dto.source.is_some());

        let source = dto.source.as_ref().unwrap();
        assert_eq!(source.source_type, "base64");
        assert_eq!(source.media_type, "image/png");
        assert!(!source.data.is_empty());
    }

    #[test]
    fn test_block_to_dto_text_block_no_source() {
        let text_block = ContentBlock::text("Hello world".to_string());
        let dto = block_to_dto(&text_block);

        assert_eq!(dto.block_type, "text");
        assert_eq!(dto.text.as_deref(), Some("Hello world"));
        assert!(dto.source.is_none());
    }

    #[test]
    fn test_block_text_summary_image_type() {
        let image_dto = ContentBlockDto {
            block_type: "image".to_string(),
            text: None,
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
            is_error: None,
            source: Some(ImageSourceDto {
                source_type: "base64".to_string(),
                media_type: "image/png".to_string(),
                data: "fake_data".to_string(),
            }),
        };

        let summary = block_text_summary(&image_dto);
        assert_eq!(summary, "[Image]");
    }

    #[test]
    fn test_block_text_summary_text_type() {
        let text_dto = ContentBlockDto {
            block_type: "text".to_string(),
            text: Some("Hello world".to_string()),
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
            is_error: None,
            source: None,
        };

        let summary = block_text_summary(&text_dto);
        assert_eq!(summary, "Hello world");
    }

    #[test]
    fn test_message_request_deserialize_with_images() {
        let json = r#"{
            "message": "Check this image",
            "images": [
                {
                    "media_type": "image/png",
                    "data": "base64data123"
                },
                {
                    "media_type": "image/jpeg",
                    "data": "base64data456"
                }
            ]
        }"#;

        let request: MessageRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(request.message, "Check this image");
        assert!(request.images.is_some());

        let images = request.images.unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].data, "base64data123");
        assert_eq!(images[1].media_type, "image/jpeg");
        assert_eq!(images[1].data, "base64data456");
    }

    #[test]
    fn test_message_request_deserialize_without_images() {
        let json = r#"{
            "message": "Just text"
        }"#;

        let request: MessageRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(request.message, "Just text");
        assert!(request.images.is_none());
    }

    #[test]
    fn test_message_request_deserialize_with_empty_images() {
        let json = r#"{
            "message": "Text with empty images",
            "images": []
        }"#;

        let request: MessageRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(request.message, "Text with empty images");
        assert!(request.images.is_some());
        assert_eq!(request.images.unwrap().len(), 0);
    }
}
