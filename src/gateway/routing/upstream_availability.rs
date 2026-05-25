use crate::gateway::config::AppConfig;
use crate::gateway::error::{AppError, AppResult};

use super::decision::RouteTier;

pub fn edge_configured(config: &AppConfig) -> bool {
    config.edge_base_url.is_some()
}

pub fn cloud_configured(config: &AppConfig) -> bool {
    config.cloud_base_url.is_some()
}

/// Require at least one upstream in config before handling chat requests.
pub fn require_any_upstream(config: &AppConfig) -> AppResult<()> {
    if edge_configured(config) || cloud_configured(config) {
        Ok(())
    } else {
        Err(AppError::Unavailable(
            "no upstream configured: set [upstream.edge] and/or [upstream.cloud] in config.toml"
                .into(),
        ))
    }
}

/// When only one side is configured, force all traffic to that side.
pub fn apply_upstream_availability(
    route: RouteTier,
    config: &AppConfig,
) -> (RouteTier, Option<&'static str>) {
    match (edge_configured(config), cloud_configured(config)) {
        (true, false) => (RouteTier::Edge, Some("UPSTREAM_EDGE_ONLY")),
        (false, true) => (RouteTier::Cloud, Some("UPSTREAM_CLOUD_ONLY")),
        (true, true) | (false, false) => (route, None),
    }
}

pub fn finalize_route(
    route: RouteTier,
    config: &AppConfig,
    reason_codes: &mut Vec<String>,
) -> RouteTier {
    let (route, tag) = apply_upstream_availability(route, config);
    if let Some(tag) = tag {
        reason_codes.push(tag.to_string());
    }
    route
}
