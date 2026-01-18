use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::state::WebState;

#[derive(Serialize, Clone)]
pub struct PermissionRequestDto {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<String>,
    pub conversation_id: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub(crate) struct PermissionResolveRequest {
    id: String,
    selection: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct PermissionPendingQuery {
    conversation_id: Option<String>,
}

pub async fn list_pending_permissions(
    State(state): State<WebState>,
    axum::extract::Query(query): axum::extract::Query<PermissionPendingQuery>,
) -> impl IntoResponse {
    let pending = state
        .permission_hub
        .list_pending(query.conversation_id.as_deref())
        .await;
    Json(pending).into_response()
}

pub async fn resolve_permission_request(
    State(state): State<WebState>,
    Json(payload): Json<PermissionResolveRequest>,
) -> Response {
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
