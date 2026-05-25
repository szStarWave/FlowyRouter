use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::gateway::config::AppConfig;
use crate::gateway::experience::ExperienceStore;

use super::decision::RouteTier;
use super::step_kind::StepKind;
use super::upstream_availability::{cloud_configured, edge_configured};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkStrategy {
    #[default]
    None,
    /// Experience shows edge handles this step_kind reliably.
    CachedEdge,
    /// Edge first, cloud validates; outcomes feed experience.
    Verify,
}

pub fn is_plan_step(step_kind: StepKind) -> bool {
    step_kind == StepKind::InitialPlan
}

pub fn is_work_step(step_kind: StepKind) -> bool {
    matches!(
        step_kind,
        StepKind::ToolSelect
            | StepKind::ToolArgFill
            | StepKind::ToolResultDigest
            | StepKind::FinalReply
            | StepKind::SubagentSpawn
            | StepKind::MemoryCompact
            | StepKind::CronBackground
    )
}

/// Deterministic per-request sampling from conversation key + step + token estimate.
pub fn should_work_verify_sample(
    conv_key: &str,
    step_kind: StepKind,
    tokens_in: u32,
    rate: f32,
) -> bool {
    let rate = rate.clamp(0.0, 1.0);
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let mut h = DefaultHasher::new();
    conv_key.hash(&mut h);
    format!("{step_kind:?}").hash(&mut h);
    tokens_in.hash(&mut h);
    let bucket = (h.finish() % 10_000) as f32 / 10_000.0;
    bucket < rate
}

pub fn apply_work_route(
    route: RouteTier,
    step_kind: StepKind,
    config: &AppConfig,
    experience: Option<&ExperienceStore>,
    conv_key: &str,
    tokens_in: u32,
    work_verify_sample_rate: f32,
    reason_codes: &mut Vec<String>,
) -> (RouteTier, WorkStrategy) {
    if !cloud_configured(config) {
        return (route, WorkStrategy::None);
    }

    if is_plan_step(step_kind) {
        reason_codes.push("INITIAL_PLAN_CLOUD".to_string());
        return (RouteTier::Cloud, WorkStrategy::None);
    }

    if !is_work_step(step_kind) || !edge_configured(config) {
        return (route, WorkStrategy::None);
    }

    if experience.is_some_and(|exp| exp.edge_trusted(step_kind)) {
        reason_codes.push("WORK_CACHE_EDGE".to_string());
        return (RouteTier::Edge, WorkStrategy::CachedEdge);
    }

    if should_work_verify_sample(
        conv_key,
        step_kind,
        tokens_in,
        work_verify_sample_rate,
    ) {
        reason_codes.push(format!(
            "WORK_VERIFY_SAMPLE(p={work_verify_sample_rate:.2})"
        ));
        return (RouteTier::Cascade, WorkStrategy::Verify);
    }

    reason_codes.push(format!(
        "WORK_SAMPLE_SKIP(p={work_verify_sample_rate:.2})"
    ));
    (RouteTier::Edge, WorkStrategy::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigFile, UpstreamEndpoint};
    use crate::gateway::config::AppConfig;

    fn app_config(rate: f32) -> AppConfig {
        let mut file = ConfigFile::default();
        file.gateway.work_verify_sample_rate = rate;
        file.upstream.edge = Some(UpstreamEndpoint {
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: None,
            model: None,
        });
        file.upstream.cloud = Some(UpstreamEndpoint {
            base_url: "https://api.example.com/v1".into(),
            api_key: None,
            model: None,
        });
        AppConfig::from_file(file, std::path::PathBuf::from("/tmp/flowy-test-config.toml"))
            .unwrap()
    }

    #[test]
    fn sample_rate_zero_never_verifies() {
        assert!(!should_work_verify_sample("conv:a", StepKind::ToolSelect, 100, 0.0));
    }

    #[test]
    fn sample_rate_one_always_verifies() {
        assert!(should_work_verify_sample("conv:a", StepKind::ToolSelect, 100, 1.0));
    }

    #[test]
    fn apply_work_route_skips_verify_at_zero_rate() {
        let cfg = app_config(0.0);
        let mut codes = Vec::new();
        let (route, strategy) = apply_work_route(
            RouteTier::Cloud,
            StepKind::ToolSelect,
            &cfg,
            None,
            "conv:sample",
            512,
            0.0,
            &mut codes,
        );
        assert_eq!(route, RouteTier::Edge);
        assert_eq!(strategy, WorkStrategy::None);
        assert!(codes.iter().any(|c| c.starts_with("WORK_SAMPLE_SKIP")));
    }

    #[test]
    fn apply_work_route_verifies_at_full_rate() {
        let cfg = app_config(1.0);
        let mut codes = Vec::new();
        let (route, strategy) = apply_work_route(
            RouteTier::Cloud,
            StepKind::ToolSelect,
            &cfg,
            None,
            "conv:sample",
            512,
            1.0,
            &mut codes,
        );
        assert_eq!(strategy, WorkStrategy::Verify);
        assert!(codes.iter().any(|c| c.starts_with("WORK_VERIFY_SAMPLE")));
    }
}
