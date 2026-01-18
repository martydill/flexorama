use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;

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
pub(crate) struct PlanUpdateRequest {
    title: Option<String>,
    user_request: Option<String>,
    plan_markdown: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PlanCreateRequest {
    title: Option<String>,
    user_request: String,
    plan_markdown: String,
    conversation_id: Option<String>,
}

#[derive(Serialize)]
struct PlanModeResponse {
    enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct PlanModeRequest {
    enabled: bool,
}

pub async fn list_plans(State(state): State<WebState>) -> impl IntoResponse {
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

pub async fn get_plan(State(state): State<WebState>, Path(id): Path<String>) -> impl IntoResponse {
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

pub async fn update_plan(
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

pub async fn delete_plan(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.database.delete_plan(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete plan: {}", e),
        )
            .into_response(),
    }
}

pub async fn create_plan(
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

pub async fn get_plan_mode(State(state): State<WebState>) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    Json(PlanModeResponse {
        enabled: agent.plan_mode(),
    })
    .into_response()
}

pub async fn set_plan_mode(
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
