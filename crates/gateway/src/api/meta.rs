use axum::http::{HeaderMap, HeaderValue};

use crate::api::openai::{ChatCompletionResponse, FlowyMeta};
use crate::routing::{Profile, RouteDecision, RouteTier, StepKind};

pub fn build_flowy_meta(decision: &RouteDecision, fallback: bool, resp: &ChatCompletionResponse) -> FlowyMeta {
    let tokens_out = resp
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .map(|t| ((t.len() as f64) / 4.0).ceil() as u32)
        .unwrap_or(0);
    let tokens_in = decision.tokens_in_estimate;
    let input_ratio = if tokens_in + tokens_out > 0 {
        tokens_in as f32 / (tokens_in + tokens_out) as f32
    } else {
        0.9933
    };

    FlowyMeta {
        route: tier_name(decision.route).to_string(),
        fallback,
        difficulty_score: decision.difficulty,
        step_kind: step_kind_name(decision.step_kind).to_string(),
        reason_codes: decision.reason_codes.clone(),
        tokens_in,
        tokens_out,
        input_ratio,
        cloud_input_saved: decision.cloud_input_saved_estimate,
        profile: profile_name(decision.profile).to_string(),
    }
}

pub fn flowy_meta_headers(decision: &RouteDecision, fallback: bool) -> HeaderMap {
    let mut headers = HeaderMap::new();
    insert(&mut headers, "x-flowy-route", tier_name(decision.route));
    insert(
        &mut headers,
        "x-flowy-fallback",
        if fallback { "true" } else { "false" },
    );
    insert(
        &mut headers,
        "x-flowy-step-kind",
        step_kind_name(decision.step_kind),
    );
    insert(
        &mut headers,
        "x-flowy-profile",
        profile_name(decision.profile),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("{:.4}", decision.difficulty)) {
        headers.insert("x-flowy-difficulty", v);
    }
    if !decision.reason_codes.is_empty() {
        let joined = decision.reason_codes.join(",");
        if let Ok(v) = HeaderValue::from_str(&joined) {
            headers.insert("x-flowy-reason-codes", v);
        }
    }
    headers
}

fn insert(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        headers.insert(name, v);
    }
}

pub fn tier_name(t: RouteTier) -> &'static str {
    match t {
        RouteTier::Edge => "edge",
        RouteTier::Cloud => "cloud",
        RouteTier::Cascade => "cascade",
    }
}

pub fn step_kind_name(k: StepKind) -> &'static str {
    match k {
        StepKind::HeartbeatAck => "heartbeat_ack",
        StepKind::DirectChat => "direct_chat",
        StepKind::RecoveryAfterFailure => "recovery_after_failure",
        StepKind::ToolSelect => "tool_select",
        StepKind::ToolArgFill => "tool_arg_fill",
        StepKind::ToolResultDigest => "tool_result_digest",
        StepKind::InitialPlan => "initial_plan",
        StepKind::FinalReply => "final_reply",
        StepKind::SubagentSpawn => "subagent_spawn",
        StepKind::MemoryCompact => "memory_compact",
        StepKind::CronBackground => "cron_background",
    }
}

pub fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::Economy => "economy",
        Profile::Balanced => "balanced",
        Profile::Premium => "premium",
        Profile::Privacy => "privacy",
    }
}
