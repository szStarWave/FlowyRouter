use crate::routing::{RouteDecision, RouteTier, StepKind};

/// Result of a completed chat request (implicit signals for learning).
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestOutcome {
    pub edge_ok: bool,
    pub cascade_fallback: bool,
    pub upstream_error: bool,
}

impl RequestOutcome {
    pub fn success(decision: &RouteDecision, fallback: bool) -> Self {
        match decision.route {
            RouteTier::Edge => Self {
                edge_ok: true,
                cascade_fallback: false,
                upstream_error: false,
            },
            RouteTier::Cloud => Self::default(),
            RouteTier::Cascade => Self {
                edge_ok: !fallback,
                cascade_fallback: fallback,
                upstream_error: false,
            },
        }
    }

    pub fn upstream_error() -> Self {
        Self {
            edge_ok: false,
            cascade_fallback: false,
            upstream_error: true,
        }
    }

    pub fn should_set_cloud_sticky(self, step_kind: StepKind) -> bool {
        self.cascade_fallback
            || (self.upstream_error && step_kind != StepKind::HeartbeatAck)
    }
}
