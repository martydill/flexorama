use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse};

use super::state::WebState;

const INDEX_HTML: &str = include_str!("../../web/index.html");
const APP_JS: &str = include_str!("../../web/app.js");

pub async fn serve_index(State(state): State<WebState>) -> impl IntoResponse {
    // Generate CSRF token and inject it into the HTML
    let csrf_token = state.csrf_manager.generate_token().await;
    let html_with_token = INDEX_HTML.replace(
        "</head>",
        &format!(
            r#"<script>window.FLEXORAMA_CSRF_TOKEN = "{}";</script></head>"#,
            csrf_token
        ),
    );
    Html(html_with_token)
}

pub async fn serve_app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], APP_JS)
}

pub async fn health() -> impl IntoResponse {
    axum::Json(std::collections::HashMap::from([("status", "ok")]))
}
