use anyhow::Result;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::WebState;

#[derive(Deserialize)]
pub(crate) struct StatsQueryParams {
    pub(crate) period: Option<String>,
    pub(crate) start_date: Option<String>,
    pub(crate) end_date: Option<String>,
}

#[derive(Serialize)]
struct UsageStatsResponse {
    period: String,
    data: Vec<UsageStatsPoint>,
}

#[derive(Serialize)]
struct UsageStatsPoint {
    date: String,
    total_requests: i32,
    total_input_tokens: i32,
    total_output_tokens: i32,
    total_tokens: i32,
}

#[derive(Serialize)]
struct ConversationStatsResponse {
    period: String,
    data: Vec<ConversationStatsPoint>,
}

#[derive(Serialize)]
struct ConversationStatsPoint {
    date: String,
    count: i32,
}

#[derive(Serialize)]
struct ModelStatsResponse {
    period: String,
    data: Vec<ModelStatsPoint>,
}

#[derive(Serialize)]
struct ModelStatsPoint {
    model: String,
    provider: String,
    total_conversations: i32,
    total_tokens: i32,
    request_count: i32,
}

#[derive(Serialize)]
struct ConversationsByProviderResponse {
    period: String,
    data: Vec<ConversationsByProviderPoint>,
}

#[derive(Serialize)]
struct ConversationsByProviderPoint {
    date: String,
    provider: String,
    count: i32,
}

#[derive(Serialize)]
struct ConversationsBySubagentResponse {
    period: String,
    data: Vec<ConversationsBySubagentPoint>,
}

#[derive(Serialize)]
struct ConversationsBySubagentPoint {
    date: String,
    subagent: String,
    count: i32,
}

pub async fn get_stats_overview(State(state): State<WebState>) -> impl IntoResponse {
    db_result_to_response(
        state.database.get_stats_overview().await,
        "Failed to load stats overview",
        |overview| Json(overview).into_response(),
    )
}

pub async fn get_usage_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_usage_stats_range(start_date, end_date)
            .await,
        "Failed to load usage stats",
        |stats| {
            let response = UsageStatsResponse {
                period,
                data: stats
                    .into_iter()
                    .map(|s| UsageStatsPoint {
                        date: s.date,
                        total_requests: s.total_requests,
                        total_input_tokens: s.total_input_tokens,
                        total_output_tokens: s.total_output_tokens,
                        total_tokens: s.total_tokens,
                    })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

pub async fn get_model_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_stats_by_model(start_date, end_date)
            .await,
        "Failed to load model stats",
        |stats| {
            let response = ModelStatsResponse {
                period,
                data: stats
                    .into_iter()
                    .map(|s| ModelStatsPoint {
                        model: s.model.clone(),
                        provider: extract_provider_from_model(&s.model),
                        total_conversations: s.total_conversations,
                        total_tokens: s.total_tokens,
                        request_count: s.request_count,
                    })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

pub async fn get_conversation_stats(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date(start_date, end_date)
            .await,
        "Failed to load conversation stats",
        |counts| {
            let response = ConversationStatsResponse {
                period,
                data: counts
                    .into_iter()
                    .map(|(date, count)| ConversationStatsPoint { date, count })
                    .collect(),
            };
            Json(response).into_response()
        },
    )
}

pub async fn get_conversation_stats_by_provider(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date_and_model(start_date, end_date)
            .await,
        "Failed to load conversation stats by provider",
        |counts| {
            // Aggregate by (date, provider) since multiple models can map to the same provider
            let mut aggregated: HashMap<(String, String), i32> = HashMap::new();

            for (date, model, count) in counts {
                let provider = extract_provider_from_model(&model);
                let key = (date.clone(), provider.clone());
                *aggregated.entry(key).or_insert(0) += count;
            }

            let data: Vec<ConversationsByProviderPoint> = aggregated
                .into_iter()
                .map(|((date, provider), count)| ConversationsByProviderPoint {
                    date,
                    provider,
                    count,
                })
                .collect();

            let response = ConversationsByProviderResponse { period, data };
            Json(response).into_response()
        },
    )
}

pub async fn get_conversation_stats_by_subagent(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<StatsQueryParams>,
) -> impl IntoResponse {
    let (start_date, end_date) = calculate_date_range(&params);
    let period = params.period.unwrap_or_else(|| "month".to_string());

    db_result_to_response(
        state
            .database
            .get_conversation_counts_by_date_and_subagent(start_date, end_date)
            .await,
        "Failed to load conversation stats by subagent",
        |counts| {
            let data: Vec<ConversationsBySubagentPoint> = counts
                .into_iter()
                .map(|(date, subagent, count)| ConversationsBySubagentPoint {
                    date,
                    subagent,
                    count,
                })
                .collect();

            let response = ConversationsBySubagentResponse { period, data };
            Json(response).into_response()
        },
    )
}

pub(crate) fn extract_provider_from_model(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower.contains("claude") {
        "Anthropic".to_string()
    } else if lower.contains("gpt") {
        "OpenAI".to_string()
    } else if lower.contains("gemini") {
        "Gemini".to_string()
    } else if lower.contains("glm") {
        "Z.AI".to_string()
    } else if lower.contains("llama") || lower.contains("gemma") {
        "Ollama".to_string()
    } else {
        "Other".to_string()
    }
}

/// Helper to handle database results and convert to HTTP responses with consistent error handling
fn db_result_to_response<T, F>(result: Result<T>, error_msg: &str, transform: F) -> Response
where
    F: FnOnce(T) -> Response,
{
    match result {
        Ok(data) => transform(data),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{}: {}", error_msg, e),
        )
            .into_response(),
    }
}

pub(crate) fn calculate_date_range(
    params: &StatsQueryParams,
) -> (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) {
    use chrono::NaiveDate;

    // If custom dates are provided, use them
    if let (Some(start), Some(end)) = (&params.start_date, &params.end_date) {
        let start_parsed = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok();
        let end_parsed = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok();
        return (start_parsed, end_parsed);
    }

    // Otherwise, calculate based on period
    let now = Utc::now().naive_utc().date();
    let period = params.period.as_deref().unwrap_or("month");

    match period {
        "day" => (Some(now - Duration::days(1)), Some(now)),
        "week" => (Some(now - Duration::days(7)), Some(now)),
        "month" => (Some(now - Duration::days(30)), Some(now)),
        "lifetime" => (None, None),
        _ => (Some(now - Duration::days(30)), Some(now)),
    }
}
