use axum::{
    Json,
    body::Body,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use tracing::info;

use crate::api::auth::require_gateway_api_key;
use crate::api::meta::{build_flowy_meta, flowy_meta_headers};
use crate::api::openai::ChatCompletionRequest;
use crate::api::routes::AppState;
use crate::experience::RequestOutcome;
use crate::error::{AppError, AppResult};

pub async fn chat_completions(
    state: AppState,
    headers: HeaderMap,
    req: ChatCompletionRequest,
) -> AppResult<impl IntoResponse> {
    let stream = req.stream;
    state.stats.record_request(stream);

    if let Err(e) = require_gateway_api_key(&headers, &state.config.api_key) {
        state.stats.record_error(&e);
        return Err(e);
    }

    if let Err(e) = crate::routing::require_any_upstream(&state.config) {
        state.stats.record_error(&e);
        return Err(e);
    }

    let decision = crate::routing::decide(
        &state.config,
        &req,
        state.sessions.as_ref(),
        Some(state.experience.as_ref()),
    );
    state.stats.record_decision(&decision);

    let conv_key = decision.conversation_key.clone();
    let assistant_failed = decision.assistant_failed_recent;

    info!(
        route = ?decision.route,
        step = ?decision.step_kind,
        difficulty = decision.difficulty,
        stream = stream,
        tok_in = decision.tokens_in_estimate,
        reasons = ?decision.reason_codes,
        "routing decision"
    );

    if stream {
        match state.upstream.stream(&req, &decision).await {
            Ok(byte_stream) => {
                let outcome = RequestOutcome::success(&decision, false);
                record_learning(&state, &decision, &conv_key, outcome, assistant_failed);
                let mut resp = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from_stream(byte_stream))
                    .map_err(|e| AppError::Internal(e.into()))?;
                let headers = resp.headers_mut();
                headers.extend(flowy_meta_headers(&decision, false));
                apply_sse_headers(headers);
                Ok(resp.into_response())
            }
            Err(e) => {
                state.stats.record_error(&e);
                record_learning(
                    &state,
                    &decision,
                    &conv_key,
                    RequestOutcome::upstream_error(),
                    assistant_failed,
                );
                Err(e)
            }
        }
    } else {
        match state.upstream.complete(&req, &decision).await {
            Ok(mut resp) => {
                let fallback = resp.flowy_meta.as_ref().is_some_and(|m| m.fallback);
                let outcome = RequestOutcome::success(&decision, fallback);
                record_learning(&state, &decision, &conv_key, outcome, assistant_failed);
                resp.flowy_meta = Some(build_flowy_meta(&decision, fallback, &resp));
                if let Some(m) = &resp.flowy_meta {
                    state.stats.record_completion_tokens(m.tokens_out);
                }
                Ok(Json(resp).into_response())
            }
            Err(e) => {
                state.stats.record_error(&e);
                record_learning(
                    &state,
                    &decision,
                    &conv_key,
                    RequestOutcome::upstream_error(),
                    assistant_failed,
                );
                Err(e)
            }
        }
    }
}

fn record_learning(
    state: &AppState,
    decision: &crate::routing::RouteDecision,
    conv_key: &str,
    outcome: RequestOutcome,
    assistant_failed_signal: bool,
) {
    state
        .experience
        .record_outcome(decision.step_kind, outcome);
    state.sessions.apply_outcome(
        conv_key,
        decision,
        outcome,
        state.config.cloud_sticky_ttl_secs,
        assistant_failed_signal,
    );
}

fn apply_sse_headers(headers: &mut HeaderMap) {
    use axum::http::header::{CACHE_CONTROL, CONNECTION, CONTENT_TYPE};
    headers.insert(
        CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    headers.insert(CACHE_CONTROL, axum::http::HeaderValue::from_static("no-cache"));
    headers.insert(CONNECTION, axum::http::HeaderValue::from_static("keep-alive"));
}
