use anyhow::Result;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::routing::{get, post, put};
use axum::Router;
use std::net::SocketAddr;

use super::agents::{
    create_agent, delete_agent, get_active_agent, get_agent, list_agents, set_active_agent,
    update_agent,
};
use super::assets::{health, serve_app_js, serve_index};
use super::commands::{
    create_custom_command, delete_custom_command, get_custom_command, list_custom_commands,
    update_custom_command,
};
use super::conversations::{
    create_conversation, get_conversation, list_conversations, search_conversations,
    send_message_to_conversation, stream_message_to_conversation,
};
use super::files::get_file_autocomplete;
use super::mcp::{
    connect_mcp_server, delete_mcp_server, disconnect_mcp_server, get_mcp_server, list_mcp_servers,
    upsert_mcp_server, upsert_mcp_server_named,
};
use super::models::{get_models, set_model};
use super::permissions::{list_pending_permissions, resolve_permission_request};
use super::plans::{
    create_plan, delete_plan, get_plan, get_plan_mode, list_plans, set_plan_mode, update_plan,
};
use super::skills::{
    activate_skill, create_skill, deactivate_skill, delete_skill, get_active_skills, get_skill,
    list_skills, update_skill,
};
use super::state::WebState;
use super::stats::{
    get_conversation_stats, get_conversation_stats_by_provider, get_conversation_stats_by_subagent,
    get_model_stats, get_stats_overview, get_usage_stats,
};
use super::todos::list_todos;

use tower_http::cors::{Any, CorsLayer};

pub(crate) async fn ensure_default_conversation(state: &WebState) -> Result<Option<String>> {
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
) -> Result<axum::response::Response, StatusCode> {
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
