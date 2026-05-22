use std::path::PathBuf;

use anyhow::{Context, Result};
use flowy_gateway::experience::{ExperienceSettings, ExperienceSnapshot, ExperienceStore};
use flowy_gateway::stats::{self, StatsScope};
use serde::{Deserialize, Serialize};

use crate::client::GatewayClient;
use crate::config::CliSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStats {
    pub scope: String,
    pub stats_file: String,
    pub persisted: bool,
    #[serde(default)]
    pub first_record_at_unix: Option<u64>,
    #[serde(default)]
    pub last_updated_at_unix: Option<u64>,
    pub session_uptime_secs: u64,
    pub requests_total: u64,
    pub requests_stream: u64,
    pub requests_non_stream: u64,
    pub requests_per_minute: f64,
    pub routing: RouteCounts,
    pub upstream: UpstreamCounts,
    pub cascade: CascadeCounts,
    pub tokens: TokenCounts,
    pub difficulty: DifficultyStats,
    pub errors: ErrorCounts,
    pub step_kinds: std::collections::HashMap<String, u64>,
    pub experience: Option<ExperienceSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceSection {
    pub enabled: bool,
    pub experience_file: String,
    pub last_updated_at_unix: Option<u64>,
    pub steps: Vec<ExperienceStepRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceStepRow {
    pub step_kind: String,
    pub edge_ok: u64,
    pub cascade_fallback: u64,
    pub upstream_error: u64,
    pub fallback_rate: f64,
    pub bias: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCounts {
    pub edge: u64,
    pub cloud: u64,
    pub cascade: u64,
    pub edge_pct: f64,
    pub cloud_pct: f64,
    pub cascade_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamCounts {
    pub edge_calls: u64,
    pub cloud_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeCounts {
    pub edge_ok: u64,
    pub fallback_to_cloud: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCounts {
    pub in_estimate: u64,
    pub out_estimate: u64,
    pub cloud_input_saved_estimate: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyStats {
    pub avg: f64,
    pub samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCounts {
    pub total: u64,
    pub unauthorized: u64,
    pub unavailable: u64,
    pub upstream: u64,
    pub bad_request: u64,
}

pub async fn print_stats(
    config_override: &Option<PathBuf>,
    global: bool,
    json: bool,
) -> Result<()> {
    let settings = load_settings(config_override)?;

    let stats = if global {
        load_global_stats(&settings).await?
    } else {
        let client = GatewayClient::new(
            settings.gateway_url(),
            settings.api_key(),
            settings.admin_token(),
        );
        client
            .stats_session()
            .await
            .context("fetch session stats (is `flowy gateway start` running?)")?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        print_human(&stats, &settings.gateway_url());
    }
    Ok(())
}

async fn load_global_stats(settings: &CliSettings) -> Result<GatewayStats> {
    let client = GatewayClient::new(
        settings.gateway_url(),
        settings.api_key(),
        settings.admin_token(),
    );
    if let Ok(stats) = client.stats_global().await {
        return Ok(stats);
    }

    load_global_stats_from_disk(settings)
}

fn load_global_stats_from_disk(settings: &CliSettings) -> Result<GatewayStats> {
    let stats_path = flowy_config::stats_file()?;
    let data = stats::data::load(&stats_path)?;
    let data_dir = flowy_config::app_dir()?;
    let experience = experience_snapshot_from_disk(&data_dir, settings).ok();
    let snap = stats::build_snapshot(
        &data,
        StatsScope::Global,
        stats_path.display().to_string(),
        0,
        experience,
    );
    Ok(from_snapshot(snap))
}

fn experience_snapshot_from_disk(
    data_dir: &std::path::Path,
    settings: &CliSettings,
) -> Result<ExperienceSnapshot> {
    let gw = settings.file.gateway.clone();
    let store = ExperienceStore::open(
        data_dir,
        ExperienceSettings {
            enabled: gw.experience_enabled,
            learning_rate: gw.experience_learning_rate,
            max_bias: gw.experience_max_bias,
            target_fallback: gw.experience_target_fallback,
        },
    )?;
    Ok(store.snapshot())
}

fn from_snapshot(s: stats::StatsSnapshot) -> GatewayStats {
    GatewayStats {
        scope: s.scope,
        stats_file: s.stats_file,
        persisted: s.persisted,
        first_record_at_unix: s.first_record_at_unix,
        last_updated_at_unix: s.last_updated_at_unix,
        session_uptime_secs: s.session_uptime_secs,
        requests_total: s.requests_total,
        requests_stream: s.requests_stream,
        requests_non_stream: s.requests_non_stream,
        requests_per_minute: s.requests_per_minute,
        routing: RouteCounts {
            edge: s.routing.edge,
            cloud: s.routing.cloud,
            cascade: s.routing.cascade,
            edge_pct: s.routing.edge_pct,
            cloud_pct: s.routing.cloud_pct,
            cascade_pct: s.routing.cascade_pct,
        },
        upstream: UpstreamCounts {
            edge_calls: s.upstream.edge_calls,
            cloud_calls: s.upstream.cloud_calls,
        },
        cascade: CascadeCounts {
            edge_ok: s.cascade.edge_ok,
            fallback_to_cloud: s.cascade.fallback_to_cloud,
        },
        tokens: TokenCounts {
            in_estimate: s.tokens.in_estimate,
            out_estimate: s.tokens.out_estimate,
            cloud_input_saved_estimate: s.tokens.cloud_input_saved_estimate,
        },
        difficulty: DifficultyStats {
            avg: s.difficulty.avg,
            samples: s.difficulty.samples,
        },
        errors: ErrorCounts {
            total: s.errors.total,
            unauthorized: s.errors.unauthorized,
            unavailable: s.errors.unavailable,
            upstream: s.errors.upstream,
            bad_request: s.errors.bad_request,
        },
        step_kinds: s.step_kinds,
        experience: s.experience.map(experience_from_snapshot),
    }
}

fn experience_from_snapshot(exp: ExperienceSnapshot) -> ExperienceSection {
    ExperienceSection {
        enabled: exp.enabled,
        experience_file: exp.experience_file,
        last_updated_at_unix: exp.last_updated_at_unix,
        steps: exp
            .steps
            .into_iter()
            .map(|row| ExperienceStepRow {
                step_kind: row.step_kind,
                edge_ok: row.edge_ok,
                cascade_fallback: row.cascade_fallback,
                upstream_error: row.upstream_error,
                fallback_rate: row.fallback_rate,
                bias: row.bias,
            })
            .collect(),
    }
}

fn load_settings(config_override: &Option<PathBuf>) -> Result<CliSettings> {
    let path = match config_override {
        Some(p) => p.clone(),
        None => flowy_config::config_file()?,
    };
    let (file, config_path) = flowy_config::load_from_path(&path)?;
    Ok(CliSettings { file, config_path })
}

fn print_human(s: &GatewayStats, gateway_url: &str) {
    let title = if s.scope == "global" {
        "Flowy Gateway Stats (global / all-time)"
    } else {
        "Flowy Gateway Stats (current session)"
    };
    println!("{title}");
    println!("  Gateway:              {gateway_url}");
    println!("  Scope:                {}", s.scope);
    println!("  Stats file:           {}", s.stats_file);
    println!("  Persisted:            {}", s.persisted);
    if let Some(ts) = s.first_record_at_unix {
        println!("  First record (unix):  {ts}");
    }
    if let Some(ts) = s.last_updated_at_unix {
        println!("  Last saved (unix):    {ts}");
    }
    if s.scope == "session" {
        println!("  Session uptime:       {}s", s.session_uptime_secs);
    }
    if s.scope == "global" {
        println!("  (counters include all gateway runs; written to stats.json)");
    }
    println!();
    println!("Requests");
    println!("  total:                {}", s.requests_total);
    println!("  stream:               {}", s.requests_stream);
    println!("  non-stream:           {}", s.requests_non_stream);
    let rpm_label = if s.scope == "global" {
        "per minute (since first record)"
    } else {
        "per minute (this session)"
    };
    println!("  {rpm_label}:  {:.2}", s.requests_per_minute);
    println!();
    println!("Routing decisions");
    println!(
        "  edge:                 {} ({:.1}%)",
        s.routing.edge, s.routing.edge_pct
    );
    println!(
        "  cloud:                {} ({:.1}%)",
        s.routing.cloud, s.routing.cloud_pct
    );
    println!(
        "  cascade:              {} ({:.1}%)",
        s.routing.cascade, s.routing.cascade_pct
    );
    println!();
    println!("Upstream HTTP calls");
    println!("  edge:                 {}", s.upstream.edge_calls);
    println!("  cloud:                {}", s.upstream.cloud_calls);
    println!();
    println!("Cascade (non-stream)");
    println!("  edge quality pass:    {}", s.cascade.edge_ok);
    println!("  fallback to cloud:    {}", s.cascade.fallback_to_cloud);
    println!();
    println!("Token estimates (cumulative)");
    println!("  input (tok_in):       {}", s.tokens.in_estimate);
    println!("  output (tok_out):     {}", s.tokens.out_estimate);
    println!(
        "  cloud_input_saved:    {}",
        s.tokens.cloud_input_saved_estimate
    );
    println!();
    println!("Difficulty");
    println!("  avg:                  {:.3}", s.difficulty.avg);
    println!("  samples:              {}", s.difficulty.samples);
    println!();
    println!("Errors");
    println!("  total:                {}", s.errors.total);
    if s.errors.total > 0 {
        println!("  unauthorized:         {}", s.errors.unauthorized);
        println!("  unavailable:          {}", s.errors.unavailable);
        println!("  upstream:             {}", s.errors.upstream);
        println!("  bad_request:          {}", s.errors.bad_request);
    }
    if !s.step_kinds.is_empty() {
        println!();
        println!("Step kinds");
        let mut kinds: Vec<_> = s.step_kinds.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in kinds {
            println!("  {name:<22} {count}");
        }
    }
    if let Some(exp) = &s.experience {
        println!();
        println!("Experience (routing learning)");
        println!("  enabled:              {}", exp.enabled);
        println!("  file:                 {}", exp.experience_file);
        for row in &exp.steps {
            println!(
                "  {:<22} ok={} fallback={} rate={:.0}% bias={:+.3}",
                row.step_kind,
                row.edge_ok,
                row.cascade_fallback,
                row.fallback_rate * 100.0,
                row.bias
            );
        }
    }
}
