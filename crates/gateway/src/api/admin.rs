use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::api::routes::AppState;
use crate::routing::Profile;

#[derive(Serialize)]
pub struct GatewayStatus {
    pub status: &'static str,
    pub version: &'static str,
    pub listen: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub edge_configured: bool,
    pub cloud_configured: bool,
    pub default_profile: String,
    pub pid_file: String,
    pub data_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    /// `session` (default) = current gateway process; `global` = cumulative persisted totals.
    #[serde(default)]
    pub scope: Option<String>,
}

pub async fn stats(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<crate::stats::StatsSnapshot>, (StatusCode, Json<serde_json::Value>)> {
    let scope = match query.scope.as_deref() {
        None | Some("session") => crate::stats::StatsScope::Session,
        Some("global") => crate::stats::StatsScope::Global,
        Some(other) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid stats scope `{other}` (use session or global)")
                })),
            ));
        }
    };
    let _ = state.stats.flush_if_dirty();
    let _ = state.experience.flush_if_dirty();
    let _ = state.sessions.flush_if_dirty();
    let uptime = state.stats.session_uptime_secs();
    let experience = if scope == crate::stats::StatsScope::Global {
        Some(state.experience.snapshot())
    } else {
        None
    };
    Ok(Json(state.stats.snapshot(scope, uptime, experience)))
}

pub async fn status(State(state): State<AppState>) -> Json<GatewayStatus> {
    Json(GatewayStatus {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        listen: state.config.listen_addr.clone(),
        pid: std::process::id(),
        uptime_secs: state.runtime.started_at.elapsed().as_secs(),
        edge_configured: state.config.edge_base_url.is_some(),
        cloud_configured: state.config.cloud_base_url.is_some(),
        default_profile: profile_name(state.config.default_profile).to_string(),
        pid_file: state.config.pid_file.display().to_string(),
        data_dir: state.config.data_dir.display().to_string(),
    })
}

pub async fn shutdown(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(expected) = state.config.admin_token.as_ref() {
        let provided = headers
            .get("x-flowy-admin-token")
            .and_then(|v| v.to_str().ok());
        if provided != Some(expected.as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid admin token"})),
            )
                .into_response();
        }
    }

    if let Err(e) = state.stats.flush() {
        tracing::warn!(error = %e, "stats flush before shutdown failed");
    }
    if let Err(e) = state.experience.flush() {
        tracing::warn!(error = %e, "experience flush before shutdown failed");
    }
    if let Err(e) = state.sessions.flush() {
        tracing::warn!(error = %e, "session flush before shutdown failed");
    }
    state.runtime.trigger_shutdown();
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "shutting_down"})),
    )
        .into_response()
}

fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::Economy => "economy",
        Profile::Balanced => "balanced",
        Profile::Premium => "premium",
        Profile::Privacy => "privacy",
    }
}
