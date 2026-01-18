use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;
use crate::mcp::McpServerConfig;

#[derive(Deserialize)]
pub(crate) struct UpsertServerRequest {
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

pub async fn list_mcp_servers(State(state): State<WebState>) -> impl IntoResponse {
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

pub async fn get_mcp_server(
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

pub async fn upsert_mcp_server(
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

pub async fn upsert_mcp_server_named(
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

pub async fn delete_mcp_server(
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

pub async fn connect_mcp_server(
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

pub async fn disconnect_mcp_server(
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
