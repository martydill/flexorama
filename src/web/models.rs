use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;
use crate::config;

#[derive(Serialize)]
struct ModelListResponse {
    provider: String,
    active_model: String,
    models: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ModelUpdateRequest {
    model: String,
}

pub async fn get_models(State(state): State<WebState>) -> impl IntoResponse {
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

pub async fn set_model(
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
