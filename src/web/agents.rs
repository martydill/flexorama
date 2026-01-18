use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;
use crate::subagent::SubagentConfig;

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
pub(crate) struct AgentUpdateRequest {
    system_prompt: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct NewAgentRequest {
    name: String,
    system_prompt: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ActivateAgentRequest {
    name: Option<String>,
}

pub async fn list_agents(State(state): State<WebState>) -> impl IntoResponse {
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

pub async fn get_agent(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
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

pub async fn create_agent(
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

pub async fn update_agent(
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

pub async fn delete_agent(
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

pub async fn get_active_agent(State(state): State<WebState>) -> impl IntoResponse {
    let name = state.agent.lock().await.active_subagent_name();
    Json(HashMap::from([("active", name)])).into_response()
}

pub async fn set_active_agent(
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
