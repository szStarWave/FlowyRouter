pub mod data;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

pub use data::StatsData;

use crate::error::AppError;
use crate::experience::ExperienceSnapshot;
use crate::routing::RouteDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsScope {
    Session,
    Global,
}

impl StatsScope {
    pub fn as_str(self) -> &'static str {
        match self {
            StatsScope::Session => "session",
            StatsScope::Global => "global",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "session" => Some(StatsScope::Session),
            "global" => Some(StatsScope::Global),
            _ => None,
        }
    }
}

pub struct GatewayStats {
    global: Mutex<StatsData>,
    session: Mutex<StatsData>,
    path: PathBuf,
    dirty: AtomicBool,
    session_started: Instant,
}

impl GatewayStats {
    pub fn open(data_dir: &Path) -> anyhow::Result<std::sync::Arc<Self>> {
        let path = data_dir.join("stats.json");
        let data = data::load(&path)?;
        Ok(std::sync::Arc::new(Self {
            global: Mutex::new(data),
            session: Mutex::new(StatsData::default()),
            path,
            dirty: AtomicBool::new(false),
            session_started: Instant::now(),
        }))
    }

    #[cfg(test)]
    pub fn new_in_memory() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            global: Mutex::new(StatsData::default()),
            session: Mutex::new(StatsData::default()),
            path: PathBuf::from("/tmp/flowy-test-stats.json"),
            dirty: AtomicBool::new(false),
            session_started: Instant::now(),
        })
    }

    pub fn stats_file(&self) -> &Path {
        &self.path
    }

    pub fn record_request(&self, stream: bool) {
        self.with_mut(|d| d.record_request(stream));
    }

    pub fn record_decision(&self, decision: &RouteDecision) {
        self.with_mut(|d| d.record_decision(decision));
    }

    pub fn record_completion_tokens(&self, tokens_out: u32) {
        self.with_mut(|d| d.record_completion_tokens(tokens_out));
    }

    pub fn record_upstream_edge(&self) {
        self.with_mut(|d| d.record_upstream_edge());
    }

    pub fn record_upstream_cloud(&self) {
        self.with_mut(|d| d.record_upstream_cloud());
    }

    pub fn record_cascade_edge_ok(&self) {
        self.with_mut(|d| d.record_cascade_edge_ok());
    }

    pub fn record_cascade_fallback(&self) {
        self.with_mut(|d| d.record_cascade_fallback());
    }

    pub fn record_error(&self, err: &AppError) {
        self.with_mut(|d| d.record_error(err));
    }

    fn with_mut(&self, update: impl Fn(&mut StatsData)) {
        update(&mut self.global.lock().expect("stats global mutex"));
        update(&mut self.session.lock().expect("stats session mutex"));
        self.dirty.store(true, Ordering::Release);
    }

    pub fn flush_if_dirty(&self) -> anyhow::Result<()> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let data = self.global.lock().expect("stats global mutex").clone();
        data::save(&self.path, &data)
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        self.dirty.store(true, Ordering::Release);
        self.flush_if_dirty()
    }

    pub fn snapshot(
        &self,
        scope: StatsScope,
        session_uptime_secs: u64,
        experience: Option<ExperienceSnapshot>,
    ) -> StatsSnapshot {
        let data = match scope {
            StatsScope::Session => self.session.lock().expect("stats session mutex").clone(),
            StatsScope::Global => self.global.lock().expect("stats global mutex").clone(),
        };
        build_snapshot(
            &data,
            scope,
            self.path.display().to_string(),
            session_uptime_secs,
            experience,
        )
    }

    pub fn session_uptime_secs(&self) -> u64 {
        self.session_started.elapsed().as_secs()
    }

    pub fn spawn_flush_task(self: &std::sync::Arc<Self>) {
        let stats = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(e) = stats.flush_if_dirty() {
                    tracing::warn!(error = %e, "stats flush failed");
                }
            }
        });
    }
}

pub fn build_snapshot(
    data: &StatsData,
    scope: StatsScope,
    stats_file: String,
    session_uptime_secs: u64,
    experience: Option<ExperienceSnapshot>,
) -> StatsSnapshot {
    let requests = data.requests_total;
    let difficulty_count = data.difficulty_count;
    let avg_difficulty = if difficulty_count > 0 {
        (data.difficulty_sum as f64) / 1000.0 / difficulty_count as f64
    } else {
        0.0
    };

    let route_edge = data.route_edge;
    let route_cloud = data.route_cloud;
    let route_cascade = data.route_cascade;
    let routed = route_edge + route_cloud + route_cascade;

    let requests_per_minute = match scope {
        StatsScope::Session => {
            if session_uptime_secs > 0 {
                requests as f64 * 60.0 / session_uptime_secs as f64
            } else {
                0.0
            }
        }
        StatsScope::Global => global_requests_per_minute(data, requests),
    };

    StatsSnapshot {
        scope: scope.as_str().to_string(),
        stats_file,
        persisted: scope == StatsScope::Global,
        first_record_at_unix: data.first_record_at_unix,
        last_updated_at_unix: data.last_updated_at_unix,
        session_uptime_secs: match scope {
            StatsScope::Session => session_uptime_secs,
            StatsScope::Global => 0,
        },
        requests_total: requests,
        requests_stream: data.requests_stream,
        requests_non_stream: data.requests_non_stream,
        requests_per_minute,
        routing: RouteCounts {
            edge: route_edge,
            cloud: route_cloud,
            cascade: route_cascade,
            edge_pct: pct(route_edge, routed),
            cloud_pct: pct(route_cloud, routed),
            cascade_pct: pct(route_cascade, routed),
        },
        upstream: UpstreamCounts {
            edge_calls: data.upstream_edge_calls,
            cloud_calls: data.upstream_cloud_calls,
        },
        cascade: CascadeCounts {
            edge_ok: data.cascade_edge_ok,
            fallback_to_cloud: data.cascade_fallback,
        },
        tokens: TokenCounts {
            in_estimate: data.tokens_in_estimate,
            out_estimate: data.tokens_out_estimate,
            cloud_input_saved_estimate: data.cloud_input_saved_estimate,
        },
        difficulty: DifficultyStats {
            avg: avg_difficulty,
            samples: difficulty_count,
        },
        errors: ErrorCounts {
            total: data.errors_total,
            unauthorized: data.errors_unauthorized,
            unavailable: data.errors_unavailable,
            upstream: data.errors_upstream,
            bad_request: data.errors_bad_request,
        },
        step_kinds: data.step_kinds.clone(),
        experience,
    }
}

fn global_requests_per_minute(data: &StatsData, requests: u64) -> f64 {
    let Some(first) = data.first_record_at_unix else {
        return 0.0;
    };
    let end = data
        .last_updated_at_unix
        .unwrap_or_else(data::now_unix);
    let span_secs = end.saturating_sub(first).max(1);
    requests as f64 * 60.0 / span_secs as f64
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    pub scope: String,
    pub stats_file: String,
    pub persisted: bool,
    pub first_record_at_unix: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experience: Option<ExperienceSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteCounts {
    pub edge: u64,
    pub cloud: u64,
    pub cascade: u64,
    pub edge_pct: f64,
    pub cloud_pct: f64,
    pub cascade_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamCounts {
    pub edge_calls: u64,
    pub cloud_calls: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CascadeCounts {
    pub edge_ok: u64,
    pub fallback_to_cloud: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenCounts {
    pub in_estimate: u64,
    pub out_estimate: u64,
    pub cloud_input_saved_estimate: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DifficultyStats {
    pub avg: f64,
    pub samples: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorCounts {
    pub total: u64,
    pub unauthorized: u64,
    pub unavailable: u64,
    pub upstream: u64,
    pub bad_request: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Profile;
    use crate::routing::{RouteDecision, RouteTier, RoutingMode, StepKind};

    fn sample_decision(route: RouteTier) -> RouteDecision {
        RouteDecision {
            route,
            profile: Profile::Balanced,
            mode: RoutingMode::Cascade,
            step_kind: StepKind::DirectChat,
            difficulty: 0.2,
            reason_codes: vec![],
            tokens_in_estimate: 100,
            tokens_out_estimate: 50,
            cloud_input_saved_estimate: 100,
            conversation_key: String::new(),
            assistant_failed_recent: false,
        }
    }

    #[test]
    fn snapshot_aggregates_decisions() {
        let stats = GatewayStats::new_in_memory();
        stats.record_request(false);
        stats.record_decision(&sample_decision(RouteTier::Edge));
        stats.record_decision(&sample_decision(RouteTier::Cloud));
        let snap = stats.snapshot(StatsScope::Session, 60, None);
        assert_eq!(snap.scope, "session");
        assert_eq!(snap.requests_total, 1);
        assert_eq!(snap.routing.edge, 1);
        assert_eq!(snap.routing.cloud, 1);
        assert_eq!(snap.tokens.in_estimate, 200);
        assert!(snap.step_kinds.contains_key("directchat"));

        let global = stats.snapshot(StatsScope::Global, 60, None);
        assert_eq!(global.scope, "global");
        assert_eq!(global.requests_total, 1);
    }

    #[test]
    fn session_resets_on_reopen_global_persists() {
        let dir = std::env::temp_dir().join(format!(
            "flowy-stats-session-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        {
            let stats = GatewayStats::open(&dir).unwrap();
            stats.record_request(true);
            stats.flush().unwrap();
            let session = stats.snapshot(StatsScope::Session, 10, None);
            assert_eq!(session.requests_total, 1);
        }
        {
            let stats = GatewayStats::open(&dir).unwrap();
            let session = stats.snapshot(StatsScope::Session, 10, None);
            let global = stats.snapshot(StatsScope::Global, 10, None);
            assert_eq!(session.requests_total, 0, "new process session starts at 0");
            assert_eq!(global.requests_total, 1, "global survives restart");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "flowy-stats-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stats.json");
        {
            let stats = GatewayStats::open(&dir).unwrap();
            stats.record_request(true);
            stats.flush().unwrap();
        }
        let loaded = data::load(&path).unwrap();
        assert_eq!(loaded.requests_total, 1);
        assert_eq!(loaded.requests_stream, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
