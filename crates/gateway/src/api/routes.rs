use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::Serialize;

use crate::api::admin;
use crate::api::chat::chat_completions;
use crate::server::GatewayRuntime;
use crate::config::AppConfig;
use crate::experience::ExperienceStore;
use crate::session::SessionStore;
use crate::stats::GatewayStats;
use crate::upstream::UpstreamClient;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub sessions: Arc<SessionStore>,
    pub experience: Arc<ExperienceStore>,
    pub upstream: UpstreamClient,
    pub runtime: Arc<GatewayRuntime>,
    pub stats: Arc<GatewayStats>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/admin/status", get(admin::status))
        .route("/v1/admin/stats", get(admin::stats))
        .route("/v1/admin/shutdown", post(admin::shutdown))
        .route("/v1/chat/completions", post(chat_completions_handler))
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    edge_configured: bool,
    cloud_configured: bool,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        edge_configured: state.config.edge_base_url.is_some(),
        cloud_configured: state.config.cloud_base_url.is_some(),
    })
}

async fn chat_completions_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<crate::api::openai::ChatCompletionRequest>,
) -> crate::error::AppResult<impl axum::response::IntoResponse> {
    chat_completions(state, headers, req).await
}
