use std::str::FromStr;

use crate::api::openai::ChatCompletionRequest;
use crate::config::AppConfig;
use crate::experience::ExperienceStore;

use super::conversation::conversation_key;
use super::difficulty::DifficultyScore;
use super::gates::check_hard_gates;
use super::policy::{self, Profile};
use super::signals::SignalExtractor;
use super::step_kind::{StepKind, resolve_step_kind};
use super::upstream_availability::{edge_configured, finalize_route};
use crate::session::SessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTier {
    Edge,
    Cloud,
    Cascade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum RoutingMode {
    Single,
    Cascade,
    Split,
}

impl FromStr for RoutingMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "single" => Ok(RoutingMode::Single),
            "cascade" => Ok(RoutingMode::Cascade),
            "split" => Ok(RoutingMode::Split),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteDecision {
    pub route: RouteTier,
    pub profile: Profile,
    pub mode: RoutingMode,
    pub step_kind: StepKind,
    pub difficulty: f32,
    pub reason_codes: Vec<String>,
    pub tokens_in_estimate: u32,
    pub tokens_out_estimate: u32,
    pub cloud_input_saved_estimate: u32,
    /// Set by `decide` for outcome recording.
    #[serde(skip)]
    pub conversation_key: String,
    #[serde(skip)]
    pub assistant_failed_recent: bool,
}

pub fn decide(
    config: &AppConfig,
    req: &ChatCompletionRequest,
    sessions: &SessionStore,
    experience: Option<&ExperienceStore>,
) -> RouteDecision {
    let profile = config.default_profile;
    let mode = config.routing_mode;
    let edge_ok = edge_configured(config);

    let conv_key = conversation_key(req);
    let prev_tok = sessions.get_last_tok_in(&conv_key);
    let sticky_until = sessions.cloud_sticky_until(&conv_key);
    let extractor = SignalExtractor {
        ctx_edge_max: config.ctx_edge_max_tokens,
    };
    let signals = extractor.extract(req, prev_tok);
    let step_kind = resolve_step_kind(req, &signals);

    let mut reason_codes = Vec::new();
    reason_codes.push(format!("STEP_{}", step_kind_code(step_kind)));

    if let Some(fixed) = config.fixed_route {
        let mut route = fixed;
        reason_codes.push(format!("CONFIG_ROUTE_{}", tier_name(route)));
        route = finalize_route(route, config, &mut reason_codes);
        sessions.record_tokens(&conv_key, signals.tok_total_in);
        return finish(
            route,
            profile,
            mode,
            step_kind,
            &signals,
            reason_codes,
            cloud_input_saved(route, &signals),
            0.0,
            conv_key,
            signals.assistant_failed_recent,
        );
    }

    if let Some(gate) = check_hard_gates(
        &signals,
        step_kind,
        config.ctx_edge_max_tokens,
        edge_ok,
        sticky_until,
    ) {
        reason_codes.push(gate.code.to_string());
        let mut route = RouteTier::Cloud;
        route = finalize_route(route, config, &mut reason_codes);
        sessions.record_tokens(&conv_key, signals.tok_total_in);
        let exp_bias = experience.map(|e| e.bias_for(step_kind)).unwrap_or(0.0);
        return finish(
            route,
            profile,
            mode,
            step_kind,
            &signals,
            reason_codes,
            0,
            d_score_for_gate(&signals, step_kind, config.ctx_edge_max_tokens, exp_bias),
            conv_key,
            signals.assistant_failed_recent,
        );
    }

    let exp_bias = experience.map(|e| e.bias_for(step_kind)).unwrap_or(0.0);
    if exp_bias.abs() > f32::EPSILON {
        reason_codes.push(format!("EXP_BIAS_{exp_bias:+.2}"));
    }
    let d = DifficultyScore::compute(
        &signals,
        step_kind,
        config.ctx_edge_max_tokens,
        exp_bias,
    );
    let mut route = policy::map_policy(d, step_kind, profile, mode);

    reason_codes.push(format!("DIFFICULTY_{:.2}", d.0));
    reason_codes.push(format!("TOK_IN_{}", signals.tok_total_in));
    reason_codes.push(format!("TOK_DELTA_{}", signals.tok_loop_delta));

    route = finalize_route(route, config, &mut reason_codes);
    sessions.record_tokens(&conv_key, signals.tok_total_in);

    finish(
        route,
        profile,
        mode,
        step_kind,
        &signals,
        reason_codes,
        cloud_input_saved(route, &signals),
        d.0,
        conv_key,
        signals.assistant_failed_recent,
    )
}

fn cloud_input_saved(route: RouteTier, signals: &super::signals::RequestSignals) -> u32 {
    match route {
        RouteTier::Edge => signals.tok_total_in,
        RouteTier::Cascade => signals.tok_total_in / 2,
        RouteTier::Cloud => 0,
    }
}

fn tier_name(t: RouteTier) -> &'static str {
    match t {
        RouteTier::Edge => "EDGE",
        RouteTier::Cloud => "CLOUD",
        RouteTier::Cascade => "CASCADE",
    }
}

fn d_score_for_gate(
    signals: &super::signals::RequestSignals,
    step_kind: StepKind,
    ctx_edge_max: u32,
    experience_bias: f32,
) -> f32 {
    DifficultyScore::compute(signals, step_kind, ctx_edge_max, experience_bias).0
}

fn finish(
    route: RouteTier,
    profile: Profile,
    mode: RoutingMode,
    step_kind: StepKind,
    signals: &super::signals::RequestSignals,
    reason_codes: Vec<String>,
    cloud_input_saved: u32,
    difficulty: f32,
    conversation_key: String,
    assistant_failed_recent: bool,
) -> RouteDecision {
    RouteDecision {
        route,
        profile,
        mode,
        step_kind,
        difficulty,
        reason_codes,
        tokens_in_estimate: signals.tok_total_in,
        tokens_out_estimate: signals.tok_out_estimate,
        cloud_input_saved_estimate: cloud_input_saved,
        conversation_key,
        assistant_failed_recent,
    }
}

fn step_kind_code(k: StepKind) -> &'static str {
    match k {
        StepKind::HeartbeatAck => "HEARTBEAT_ACK",
        StepKind::DirectChat => "DIRECT_CHAT",
        StepKind::RecoveryAfterFailure => "RECOVERY_AFTER_FAILURE",
        StepKind::ToolSelect => "TOOL_SELECT",
        StepKind::ToolArgFill => "TOOL_ARG_FILL",
        StepKind::ToolResultDigest => "TOOL_RESULT_DIGEST",
        StepKind::InitialPlan => "INITIAL_PLAN",
        StepKind::FinalReply => "FINAL_REPLY",
        StepKind::SubagentSpawn => "SUBAGENT_SPAWN",
        StepKind::MemoryCompact => "MEMORY_COMPACT",
        StepKind::CronBackground => "CRON_BACKGROUND",
    }
}
