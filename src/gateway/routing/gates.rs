use super::signals::RequestSignals;
use super::step_kind::StepKind;

#[derive(Debug, Clone)]
pub struct HardGate {
    pub code: &'static str,
}

pub fn check_hard_gates(
    signals: &RequestSignals,
    step_kind: StepKind,
    ctx_edge_max: u32,
    edge_available: bool,
    cloud_sticky_until: Option<u64>,
) -> Option<HardGate> {
    if !edge_available {
        return Some(HardGate {
            code: "GATE_EDGE_DOWN",
        });
    }

    if cloud_sticky_active(cloud_sticky_until) {
        return Some(HardGate {
            code: "GATE_STICKY_CLOUD",
        });
    }

    if signals.tok_total_in > (ctx_edge_max as f64 * 0.8) as u32 {
        return Some(HardGate {
            code: "GATE_CTX_OVERFLOW",
        });
    }

    if signals.assistant_failed_recent && step_kind != StepKind::HeartbeatAck {
        return Some(HardGate {
            code: "GATE_ASSISTANT_FAILURE",
        });
    }

    if signals.risky_tool_tier1 {
        return Some(HardGate {
            code: "GATE_RISKY_TOOL",
        });
    }

    if step_kind == StepKind::MemoryCompact && signals.tok_total_in > 12_000 {
        return Some(HardGate {
            code: "GATE_OPENCLAW_COMPACT",
        });
    }

    None
}

fn cloud_sticky_active(cloud_sticky_until: Option<u64>) -> bool {
    let Some(until) = cloud_sticky_until else {
        return false;
    };
    now_unix() < until
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::routing::signals::RequestSignals;

    fn empty_signals() -> RequestSignals {
        RequestSignals {
            tok_system: 0,
            tok_tools_schema: 0,
            tok_total_in: 100,
            tok_loop_delta: 0,
            tok_out_estimate: 0,
            n_tool_defs: 0,
            n_turns: 1,
            loop_steps: 0,
            pending_tool_calls: false,
            tool_arg_ready: false,
            last_role_tool: false,
            synthetic_tool_result: false,
            assistant_failed_recent: false,
            is_heartbeat_poll: false,
            voice_repair_loop: false,
            subagent_spawn_hint: false,
            memory_compact_hint: false,
            cron_background: false,
            tools_enabled: false,
            had_tool_roundtrip: false,
            risky_tool_tier1: false,
            intent_hard: false,
            intent_easy: false,
            multimodal: false,
        }
    }

    #[test]
    fn sticky_cloud_gate_fires() {
        let until = now_unix() + 3600;
        let gate = check_hard_gates(
            &empty_signals(),
            StepKind::DirectChat,
            65536,
            true,
            Some(until),
        );
        assert_eq!(gate.unwrap().code, "GATE_STICKY_CLOUD");
    }
}
