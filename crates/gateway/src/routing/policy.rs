use std::str::FromStr;

use super::difficulty::DifficultyScore;
use super::step_kind::StepKind;
use crate::routing::decision::{RouteTier, RoutingMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Profile {
    Economy,
    Balanced,
    Premium,
    Privacy,
}

impl Profile {
    pub fn thresholds(self) -> (f32, f32) {
        match self {
            Profile::Economy => (0.40, 0.60),
            Profile::Balanced => (0.35, 0.55),
            Profile::Premium => (0.25, 0.45),
            Profile::Privacy => (0.45, 0.65),
        }
    }

    pub fn default_mode(self) -> RoutingMode {
        match self {
            Profile::Premium => RoutingMode::Single,
            _ => RoutingMode::Cascade,
        }
    }
}

impl FromStr for Profile {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "economy" => Ok(Profile::Economy),
            "balanced" => Ok(Profile::Balanced),
            "premium" => Ok(Profile::Premium),
            "privacy" => Ok(Profile::Privacy),
            _ => Err(()),
        }
    }
}

pub fn map_policy(
    d: DifficultyScore,
    step_kind: StepKind,
    profile: Profile,
    mode: RoutingMode,
) -> RouteTier {
    if matches!(step_kind, StepKind::HeartbeatAck | StepKind::DirectChat) {
        return RouteTier::Edge;
    }

    if profile == Profile::Privacy {
        if step_kind == StepKind::RecoveryAfterFailure {
            return RouteTier::Cloud;
        }
        return RouteTier::Edge;
    }

    let (theta_edge, theta_cloud) = profile.thresholds();
    let score = d.0;

    match mode {
        RoutingMode::Single => {
            if score < theta_edge {
                RouteTier::Edge
            } else {
                RouteTier::Cloud
            }
        }
        RoutingMode::Cascade => {
            if score < theta_edge {
                RouteTier::Edge
            } else if score < theta_cloud {
                RouteTier::Cascade
            } else {
                RouteTier::Cloud
            }
        }
        RoutingMode::Split => RouteTier::Cloud,
    }
}
