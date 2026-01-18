use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;

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
pub(crate) struct NewSkillRequest {
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
pub(crate) struct SkillUpdateRequest {
    description: String,
    content: String,
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tags: Vec<String>,
}

pub async fn list_skills(State(state): State<WebState>) -> impl IntoResponse {
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

pub async fn get_skill(
    State(state): State<WebState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
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

pub async fn create_skill(
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

pub async fn update_skill(
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

pub async fn delete_skill(
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

pub async fn activate_skill(
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

pub async fn deactivate_skill(
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

pub async fn get_active_skills(State(state): State<WebState>) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    let active_skills = agent.get_active_skills();
    Json(active_skills).into_response()
}
