use super::agents::{
    create_agent, delete_agent, get_active_agent, get_agent, list_agents, update_agent,
};
use super::commands::{
    create_custom_command, delete_custom_command, get_custom_command, list_custom_commands,
    update_custom_command,
};
use super::conversations::{
    build_message_dto, build_visible_message_dto, extract_context_files_from_messages,
    get_conversation, list_conversations, search_conversations, timeline_messages_to_dto,
};
use super::files::get_file_autocomplete;
use super::mcp::{
    delete_mcp_server, get_mcp_server, list_mcp_servers, upsert_mcp_server, upsert_mcp_server_named,
};
use super::models::{get_models, set_model};
use super::permissions::{list_pending_permissions, resolve_permission_request};
use super::plans::{
    create_plan, delete_plan, get_plan, get_plan_mode, list_plans, set_plan_mode, update_plan,
};
use super::routes::ensure_default_conversation;
use super::skills::{
    activate_skill, create_skill, deactivate_skill, delete_skill, get_active_skills, get_skill,
    list_skills, update_skill,
};
use super::state::{PermissionHub, WebState};
use super::stats::{
    calculate_date_range, extract_provider_from_model, get_conversation_stats,
    get_conversation_stats_by_provider, get_conversation_stats_by_subagent, get_model_stats,
    get_stats_overview, get_usage_stats, StatsQueryParams,
};
use super::todos::list_todos;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::routing::{delete, get, post, put};
use axum::Router;
use chrono::TimeZone;
use http_body_util::BodyExt;
use serial_test::serial;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tower::util::ServiceExt;

use crate::agent::Agent;
use crate::config;
use crate::csrf::CsrfManager;
use crate::database::DatabaseManager;
use crate::mcp::{McpManager, McpServerConfig};
use crate::skill::SkillManager;
use crate::subagent::{SubagentConfig, SubagentManager};

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
    let now = chrono::Utc::now().naive_utc().date();
    assert_eq!(start, Some(now - chrono::Duration::days(7)));
    assert_eq!(end, Some(now));
}

#[test]
fn test_parse_tool_arguments() {
    let parsed = super::conversations::parse_tool_arguments("{\"key\": 1}");
    assert_eq!(parsed, serde_json::json!({"key": 1}));

    let fallback = super::conversations::parse_tool_arguments("not-json");
    assert_eq!(fallback, serde_json::Value::String("not-json".to_string()));
}

#[test]
fn test_build_visible_message_dto_filters_context_blocks() {
    let context_block =
        crate::anthropic::ContentBlock::text("Context from file 'foo.txt': example".to_string());
    let regular_block = crate::anthropic::ContentBlock::text("Hello there".to_string());
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
        vec![crate::anthropic::ContentBlock::text(
            "Context from file 'foo.txt': example content".to_string(),
        )],
    );
    let message_two = build_message_dto(
        "msg-2".to_string(),
        "assistant".to_string(),
        "2024-01-02T00:00:00Z".to_string(),
        vec![crate::anthropic::ContentBlock::text(
            "Context from file 'bar.md': details".to_string(),
        )],
    );
    let files = extract_context_files_from_messages(&[message, message_two]);
    assert_eq!(files, vec!["foo.txt".to_string(), "bar.md".to_string()]);
}

#[test]
fn test_timeline_messages_to_dto_includes_tool_calls() {
    let created_at = chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let message = crate::database::Message {
        id: "msg-1".to_string(),
        role: "user".to_string(),
        content: "Hello".to_string(),
        created_at,
    };
    let tool_call = crate::database::ToolCallRecord {
        id: "tool-1".to_string(),
        tool_name: "read_file".to_string(),
        tool_arguments: "{\"path\":\"/tmp/file.txt\"}".to_string(),
        result_content: Some("Not found".to_string()),
        is_error: true,
        created_at: created_at + chrono::Duration::seconds(1),
    };

    let timeline = timeline_messages_to_dto(vec![message], vec![tool_call]);
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0].content, "Hello");
    assert!(timeline[1].content.contains("Tool call: read_file"));
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
    }
}

fn build_test_router(state: WebState) -> Router {
    Router::new()
        .route("/api/health", get(super::assets::health))
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
        .route("/api/todos", get(list_todos))
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
        "allowed_tools": ["read_file"],
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
    assert_eq!(body["allowed_tools"][0], "read_file");

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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
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
            r#"{"system_prompt":"Updated","allowed_tools":["read_file"],"denied_tools":[]}"#,
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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
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
