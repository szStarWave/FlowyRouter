use serde::Serialize;

use crate::gateway::config::{AdaptiveRoutingSettings, AppConfig};
use crate::gateway::experience::ExperienceSnapshot;
use crate::gateway::routing::{Profile, RoutingMode};
use crate::gateway::stats::data::StatsData;

/// Runtime routing knobs derived from config + experience + stats (not written to config.toml).
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveRouting {
    pub enabled: bool,
    pub work_verify_sample_rate: f32,
    pub theta_edge: f32,
    pub theta_cloud: f32,
    pub base_verify_sample_rate: f32,
    pub base_theta_edge: f32,
    pub base_theta_cloud: f32,
    pub reasons: Vec<String>,
}

impl EffectiveRouting {
    pub fn passthrough(config: &AppConfig) -> Self {
        let (theta_edge, theta_cloud) = config.default_profile.thresholds();
        Self {
            enabled: false,
            work_verify_sample_rate: config.work_verify_sample_rate,
            theta_edge,
            theta_cloud,
            base_verify_sample_rate: config.work_verify_sample_rate,
            base_theta_edge: theta_edge,
            base_theta_cloud: theta_cloud,
            reasons: vec!["ADAPTIVE_OFF".to_string()],
        }
    }
}

pub fn compute_effective_routing(
    config: &AppConfig,
    experience: &ExperienceSnapshot,
    stats: Option<&StatsData>,
    settings: &AdaptiveRoutingSettings,
) -> EffectiveRouting {
    let (base_te, base_tc) = config.default_profile.thresholds();
    let mut eff = EffectiveRouting {
        enabled: true,
        work_verify_sample_rate: config.work_verify_sample_rate,
        theta_edge: base_te,
        theta_cloud: base_tc,
        base_verify_sample_rate: config.work_verify_sample_rate,
        base_theta_edge: base_te,
        base_theta_cloud: base_tc,
        reasons: vec!["ADAPTIVE_ON".to_string()],
    };

    if !settings.enabled || !config.experience.enabled || config.fixed_route.is_some() {
        return EffectiveRouting::passthrough(config);
    }

    if config.routing_mode != RoutingMode::Cascade {
        eff.reasons.push("ADAPTIVE_SKIP_MODE".to_string());
        return eff;
    }

    let totals = &experience.totals;
    if totals.verified_total < settings.min_verified_samples {
        eff.reasons.push(format!(
            "ADAPTIVE_WARMUP(n={}/{})",
            totals.verified_total, settings.min_verified_samples
        ));
        return eff;
    }

    let target = config.experience.target_fallback as f64;
    let fb = totals.fallback_rate;
    let trust_ratio = if totals.step_kinds > 0 {
        totals.trusted_steps as f64 / totals.step_kinds as f64
    } else {
        0.0
    };

    adjust_verify_rate(&mut eff, config.work_verify_sample_rate, fb, target, trust_ratio, settings);
    adjust_thresholds(
        &mut eff,
        base_te,
        base_tc,
        fb,
        target,
        trust_ratio,
        totals.verified_total,
        settings,
    );

    if let Some(stats) = stats {
        adjust_from_cascade_stats(&mut eff, stats, settings);
    }

    eff.work_verify_sample_rate = eff
        .work_verify_sample_rate
        .clamp(settings.verify_rate_floor, settings.verify_rate_ceiling);
    eff.theta_edge = eff.theta_edge.clamp(0.15, 0.55);
    eff.theta_cloud = eff.theta_cloud.clamp(eff.theta_edge + 0.05, 0.80);

    eff
}

fn adjust_verify_rate(
    eff: &mut EffectiveRouting,
    base: f32,
    fallback_rate: f64,
    target: f64,
    trust_ratio: f64,
    settings: &AdaptiveRoutingSettings,
) {
    let base = base as f64;
    let mut rate = base;

    if fallback_rate <= target * 0.75 && trust_ratio >= 0.2 {
        let scale = 0.65 + 0.35 * (1.0 - trust_ratio.min(1.0));
        rate = base * scale;
        eff.reasons.push(format!("ADAPTIVE_VERIFY_DOWN(scale={scale:.2})"));
    } else if fallback_rate > target * 1.35 {
        rate = base * 1.45;
        eff.reasons.push("ADAPTIVE_VERIFY_UP".to_string());
    } else if fallback_rate > target {
        rate = base * 1.15;
        eff.reasons.push("ADAPTIVE_VERIFY_SLIGHT_UP".to_string());
    } else {
        eff.reasons.push("ADAPTIVE_VERIFY_HOLD".to_string());
    }

    let _ = settings;
    eff.work_verify_sample_rate = rate as f32;
}

fn adjust_thresholds(
    eff: &mut EffectiveRouting,
    base_te: f32,
    base_tc: f32,
    fallback_rate: f64,
    target: f64,
    trust_ratio: f64,
    verified_total: u64,
    settings: &AdaptiveRoutingSettings,
) {
    let mut edge_shift = 0.0f32;

    if fallback_rate <= target
        && trust_ratio >= 0.25
        && verified_total >= settings.min_verified_samples
    {
        edge_shift = -(settings.max_theta_shift * trust_ratio as f32).max(-settings.max_theta_shift);
        eff.reasons
            .push(format!("ADAPTIVE_EDGE_EASIER(shift={edge_shift:+.3})"));
    } else if fallback_rate > target * 1.4 {
        edge_shift = settings.max_theta_shift * 0.6;
        eff.reasons
            .push(format!("ADAPTIVE_EDGE_STRICTER(shift={edge_shift:+.3})"));
    }

    eff.theta_edge = base_te + edge_shift;
    eff.theta_cloud = base_tc + edge_shift * 0.5;
}

fn adjust_from_cascade_stats(eff: &mut EffectiveRouting, stats: &StatsData, settings: &AdaptiveRoutingSettings) {
    let cascade_total = stats.cascade_edge_ok + stats.cascade_fallback;
    if cascade_total < 10 {
        return;
    }
    let cascade_fb = stats.cascade_fallback as f64 / cascade_total as f64;
    if cascade_fb > 0.28 {
        eff.work_verify_sample_rate =
            (eff.work_verify_sample_rate as f64 * 1.12).min(settings.verify_rate_ceiling as f64) as f32;
        eff.reasons.push(format!(
            "ADAPTIVE_CASCADE_FB({cascade_fb:.0}%)"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;
    use crate::gateway::experience::{
        ExperienceSettingsSnapshot, ExperienceSnapshot, ExperienceTotals, StepSnapshot,
    };
    use crate::gateway::routing::RouteTier;

    fn test_config(adaptive: bool, verify_rate: f32) -> AppConfig {
        let mut file = ConfigFile::default();
        file.gateway.adaptive_routing_enabled = adaptive;
        file.gateway.work_verify_sample_rate = verify_rate;
        file.gateway.route = "auto".to_string();
        file.gateway.routing_mode = "cascade".to_string();
        file.upstream.edge = Some(crate::config::UpstreamEndpoint {
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: None,
            model: None,
        });
        file.upstream.cloud = Some(crate::config::UpstreamEndpoint {
            base_url: "https://api.example/v1".into(),
            api_key: None,
            model: None,
        });
        AppConfig::from_file(file, "/tmp/flowy-test.toml".into()).unwrap()
    }

    fn good_experience() -> ExperienceSnapshot {
        ExperienceSnapshot {
            enabled: true,
            experience_file: "/tmp/exp.json".into(),
            last_updated_at_unix: Some(1),
            settings: ExperienceSettingsSnapshot {
                learning_rate: 0.08,
                max_bias: 0.12,
                target_fallback: 0.15,
                min_trust_samples: 3,
            },
            totals: ExperienceTotals {
                step_kinds: 4,
                edge_ok: 90,
                cascade_fallback: 10,
                upstream_error: 2,
                verified_total: 100,
                total_outcomes: 102,
                fallback_rate: 0.10,
                edge_success_rate: 0.90,
                trusted_steps: 3,
            },
            steps: vec![StepSnapshot {
                step_kind: "toolselect".into(),
                edge_ok: 30,
                cascade_fallback: 2,
                upstream_error: 0,
                verified_total: 32,
                total_outcomes: 32,
                fallback_rate: 0.0625,
                edge_success_rate: 0.9375,
                bias: -0.01,
                edge_trusted: true,
            }],
        }
    }

    #[test]
    fn lowers_verify_when_healthy() {
        let config = test_config(true, 0.20);
        let settings = config.adaptive_routing.clone();
        let eff = compute_effective_routing(&config, &good_experience(), None, &settings);
        assert!(eff.work_verify_sample_rate < 0.20);
        assert!(eff.theta_edge < eff.base_theta_edge);
    }

    #[test]
    fn passthrough_when_disabled() {
        let config = test_config(false, 0.20);
        let settings = config.adaptive_routing.clone();
        let eff = compute_effective_routing(&config, &good_experience(), None, &settings);
        assert!(!eff.enabled);
        assert_eq!(eff.work_verify_sample_rate, 0.20);
    }

    #[test]
    fn warmup_keeps_base() {
        let config = test_config(true, 0.20);
        let settings = config.adaptive_routing.clone();
        let mut exp = good_experience();
        exp.totals.verified_total = 5;
        let eff = compute_effective_routing(&config, &exp, None, &settings);
        assert_eq!(eff.work_verify_sample_rate, 0.20);
        assert!(eff.reasons.iter().any(|r| r.starts_with("ADAPTIVE_WARMUP")));
    }

    #[test]
    fn fixed_route_disables_adaptive() {
        let mut config = test_config(true, 0.20);
        config.fixed_route = Some(RouteTier::Edge);
        let settings = config.adaptive_routing.clone();
        let eff = compute_effective_routing(&config, &good_experience(), None, &settings);
        assert!(!eff.enabled);
    }
}
