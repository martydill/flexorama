use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use super::state::WebState;

#[derive(Deserialize)]
pub(crate) struct TodoQuery {
    conversation_id: Option<String>,
}

pub async fn list_todos(
    State(state): State<WebState>,
    Query(query): Query<TodoQuery>,
) -> impl IntoResponse {
    let agent = state.agent.lock().await;
    let todos = agent.get_todos_for(query.conversation_id.as_deref()).await;
    Json(todos).into_response()
}
