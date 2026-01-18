use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;
use crate::custom_commands;

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
pub(crate) struct NewCustomCommandRequest {
    name: String,
    description: String,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    content: String,
}

#[derive(Deserialize)]
pub(crate) struct CustomCommandUpdateRequest {
    description: String,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    content: String,
}

pub async fn list_custom_commands(State(_state): State<WebState>) -> impl IntoResponse {
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

pub async fn get_custom_command(
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

pub async fn create_custom_command(
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

pub async fn update_custom_command(
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

pub async fn delete_custom_command(
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

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
