use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::routing::{RouteDecision, RouteTier, StepKind};

pub const STATS_VERSION: u32 = 1;

/// Cumulative counters persisted to `stats.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsData {
    pub version: u32,
    #[serde(default)]
    pub first_record_at_unix: Option<u64>,
    #[serde(default)]
    pub last_updated_at_unix: Option<u64>,
    pub requests_total: u64,
    pub requests_stream: u64,
    pub requests_non_stream: u64,
    pub route_edge: u64,
    pub route_cloud: u64,
    pub route_cascade: u64,
    pub upstream_edge_calls: u64,
    pub upstream_cloud_calls: u64,
    pub cascade_edge_ok: u64,
    pub cascade_fallback: u64,
    pub errors_total: u64,
    pub errors_unauthorized: u64,
    pub errors_unavailable: u64,
    pub errors_upstream: u64,
    pub errors_bad_request: u64,
    pub tokens_in_estimate: u64,
    pub tokens_out_estimate: u64,
    pub cloud_input_saved_estimate: u64,
    pub difficulty_sum: u64,
    pub difficulty_count: u64,
    #[serde(default)]
    pub step_kinds: HashMap<String, u64>,
}

impl StatsData {
    pub fn touch_updated(&mut self) {
        let now = now_unix();
        if self.first_record_at_unix.is_none() {
            self.first_record_at_unix = Some(now);
        }
        self.last_updated_at_unix = Some(now);
        self.version = STATS_VERSION;
    }

    pub fn record_request(&mut self, stream: bool) {
        self.touch_updated();
        self.requests_total += 1;
        if stream {
            self.requests_stream += 1;
        } else {
            self.requests_non_stream += 1;
        }
    }

    pub fn record_decision(&mut self, decision: &RouteDecision) {
        self.touch_updated();
        match decision.route {
            RouteTier::Edge => self.route_edge += 1,
            RouteTier::Cloud => self.route_cloud += 1,
            RouteTier::Cascade => self.route_cascade += 1,
        }
        self.tokens_in_estimate += decision.tokens_in_estimate as u64;
        self.cloud_input_saved_estimate += decision.cloud_input_saved_estimate as u64;
        let scaled = (decision.difficulty * 1000.0).round() as u64;
        self.difficulty_sum += scaled;
        self.difficulty_count += 1;
        let key = step_kind_key(decision.step_kind);
        *self.step_kinds.entry(key).or_insert(0) += 1;
    }

    pub fn record_completion_tokens(&mut self, tokens_out: u32) {
        self.touch_updated();
        self.tokens_out_estimate += tokens_out as u64;
    }

    pub fn record_upstream_edge(&mut self) {
        self.touch_updated();
        self.upstream_edge_calls += 1;
    }

    pub fn record_upstream_cloud(&mut self) {
        self.touch_updated();
        self.upstream_cloud_calls += 1;
    }

    pub fn record_cascade_edge_ok(&mut self) {
        self.touch_updated();
        self.cascade_edge_ok += 1;
    }

    pub fn record_cascade_fallback(&mut self) {
        self.touch_updated();
        self.cascade_fallback += 1;
    }

    pub fn record_error(&mut self, err: &AppError) {
        self.touch_updated();
        self.errors_total += 1;
        match err {
            AppError::Unauthorized(_) => self.errors_unauthorized += 1,
            AppError::Unavailable(_) => self.errors_unavailable += 1,
            AppError::Upstream(_) => self.errors_upstream += 1,
            AppError::BadRequest(_) => self.errors_bad_request += 1,
            AppError::Internal(_) => {}
        }
    }
}

pub fn load(path: &Path) -> anyhow::Result<StatsData> {
    if !path.exists() {
        return Ok(StatsData::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read stats {}", path.display()))?;
    match serde_json::from_str(&text) {
        Ok(data) => Ok(data),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "invalid stats file, starting fresh"
            );
            Ok(StatsData::default())
        }
    }
}

pub fn save(path: &Path, data: &StatsData) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create stats dir {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&tmp, json).with_context(|| format!("write stats {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename stats {}", path.display()))?;
    Ok(())
}

fn step_kind_key(k: StepKind) -> String {
    format!("{:?}", k).to_ascii_lowercase()
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
