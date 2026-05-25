use crate::gateway::api::openai::{ChatCompletionRequest, Message, Role};

#[derive(Debug, Clone)]
pub struct RequestSignals {
    pub tok_system: u32,
    pub tok_tools_schema: u32,
    pub tok_total_in: u32,
    pub tok_loop_delta: u32,
    pub tok_out_estimate: u32,
    pub n_tool_defs: u32,
    pub n_turns: u32,
    pub loop_steps: u32,
    pub pending_tool_calls: bool,
    pub tool_arg_ready: bool,
    pub last_role_tool: bool,
    pub synthetic_tool_result: bool,
    pub assistant_failed_recent: bool,
    pub is_heartbeat_poll: bool,
    pub voice_repair_loop: bool,
    pub subagent_spawn_hint: bool,
    pub memory_compact_hint: bool,
    pub cron_background: bool,
    pub tools_enabled: bool,
    pub had_tool_roundtrip: bool,
    pub risky_tool_tier1: bool,
    pub intent_hard: bool,
    pub intent_easy: bool,
    pub multimodal: bool,
}

/// Tool-free, short multimodal chat — eligible for edge probe/cache.
pub fn is_simple_multimodal(signals: &RequestSignals) -> bool {
    signals.multimodal
        && !signals.tools_enabled
        && !signals.had_tool_roundtrip
        && !signals.pending_tool_calls
        && signals.n_turns <= 2
        && signals.tok_total_in < 2048
        && !signals.intent_hard
        && !signals.assistant_failed_recent
}

pub struct SignalExtractor {
    pub ctx_edge_max: u32,
}

impl SignalExtractor {
    pub fn extract(
        &self,
        req: &ChatCompletionRequest,
        prev_tok_total_in: Option<u32>,
    ) -> RequestSignals {
        let tok_tools_schema = estimate_tokens(&serde_json::to_string(&req.tools).unwrap_or_default());
        let mut tok_system = 0u32;
        let mut tok_rest = 0u32;
        let mut n_turns = 0u32;
        let mut had_tool = false;
        let mut multimodal = false;

        for (i, msg) in req.messages.iter().enumerate() {
            let t = estimate_message_tokens(msg);
            if i == 0 && msg.role == Role::System {
                tok_system = t;
            } else {
                tok_rest += t;
            }
            if msg.role == Role::User {
                n_turns += 1;
            }
            if msg.role == Role::Tool {
                had_tool = true;
            }
            if message_has_image(msg) {
                multimodal = true;
            }
        }

        let tok_total_in = tok_system.saturating_add(tok_rest).saturating_add(tok_tools_schema);
        let tok_loop_delta = prev_tok_total_in.map_or(tok_total_in, |prev| {
            tok_total_in.saturating_sub(prev)
        });

        let tail = req.messages.iter().rev().take(4).collect::<Vec<_>>();
        let last = req.messages.last();
        let prev_assistant = req
            .messages
            .iter()
            .rev()
            .filter(|m| m.role == Role::Assistant)
            .nth(0);

        let pending_tool_calls = prev_assistant
            .and_then(|m| m.tool_calls.as_ref())
            .is_some_and(|tc| !tc.is_empty());

        let tool_arg_ready = prev_assistant
            .and_then(|m| m.tool_calls.as_ref())
            .is_some_and(|calls| {
                calls.iter().all(|c| {
                    c.function
                        .arguments
                        .starts_with('{')
                        && c.function.arguments.contains('}')
                })
            });

        let last_role_tool = last.is_some_and(|m| m.role == Role::Tool);
        let synthetic_tool_result = last.is_some_and(|m| {
            message_text(m).contains("[openclaw] missing tool result")
                || message_text(m).contains("prompt lock was released")
        });

        let assistant_failed_recent = tail.iter().any(|m| {
            m.role == Role::Assistant
                && message_text(m).contains("[assistant turn failed")
        });

        let is_heartbeat_poll = last.is_some_and(|m| {
            m.role == Role::User && message_text(m).trim() == "[OpenClaw heartbeat poll]"
        });

        let voice_repair_loop = last.is_some_and(|m| {
            m.role == Role::User
                && message_text(m).contains("[Audio transcript")
                && had_tool
        });

        // Do not scan system/tool-schema boilerplate (OpenClaw documents sessions_spawn in system).
        let subagent_spawn_hint = subagent_spawn_in_transcript(req)
            || prev_assistant
                .and_then(|m| m.tool_calls.as_ref())
                .is_some_and(|calls| {
                    calls
                        .iter()
                        .any(|c| c.function.name == "sessions_spawn")
                });

        let memory_compact_hint = tok_system > 0
            && req.messages.iter().any(|m| {
                let t = message_text(m);
                t.contains("Dynamic Project Context") || t.contains("compaction")
            });

        let cron_background = req.messages.iter().any(|m| {
            let t = message_text(m).to_ascii_lowercase();
            t.contains("[cron]") || t.contains("cron background") || t.contains("cron job")
        });

        let loop_steps = req
            .messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .count() as u32;

        let intent_hard = last
            .map(|m| message_text(m))
            .is_some_and(|t| contains_hard_intent(&t));
        let intent_easy = last
            .map(|m| message_text(m))
            .is_some_and(|t| contains_easy_intent(&t));

        let risky_tool_tier1 = prev_assistant
            .and_then(|m| m.tool_calls.as_ref())
            .is_some_and(|calls| {
                calls.iter().any(|c| {
                    matches!(
                        c.function.name.as_str(),
                        "exec" | "write" | "edit" | "browser" | "sessions_spawn" | "message"
                    )
                })
            });

        RequestSignals {
            tok_system,
            tok_tools_schema,
            tok_total_in,
            tok_loop_delta,
            tok_out_estimate: 0,
            n_tool_defs: req.tools.len() as u32,
            n_turns,
            loop_steps,
            pending_tool_calls,
            tool_arg_ready,
            last_role_tool,
            synthetic_tool_result,
            assistant_failed_recent,
            is_heartbeat_poll,
            voice_repair_loop,
            subagent_spawn_hint,
            memory_compact_hint,
            cron_background,
            tools_enabled: !req.tools.is_empty(),
            had_tool_roundtrip: had_tool,
            risky_tool_tier1,
            intent_hard,
            intent_easy,
            multimodal,
        }
    }
}

fn contains_hard_intent(text: &str) -> bool {
    const KWS: &[&str] = &[
        "架构",
        "证明",
        "refactor",
        "distributed",
        "legal",
        "medical",
        "跨仓库",
    ];
    KWS.iter().any(|k| text.contains(k))
}

fn contains_easy_intent(text: &str) -> bool {
    const KWS: &[&str] = &[
        "分类",
        "提取",
        "格式化",
        "translate",
        "yes/no",
        "是否",
        "你好",
        "hello",
        "hi",
        "嗨",
        "几点",
        "what time",
        "current time",
        "现在几点",
        "什么时间",
    ];
    KWS.iter().any(|k| text.contains(k))
}

pub fn estimate_tokens(text: &str) -> u32 {
    // Fast heuristic (~4 chars/token); replace with tiktoken later.
    ((text.len() as f64) / 4.0).ceil() as u32
}

fn estimate_message_tokens(msg: &Message) -> u32 {
    let mut n = 0u32;
    if let Some(c) = &msg.content {
        n += estimate_tokens(c);
    }
    if let Some(parts) = &msg.content_parts {
        for p in parts {
            if let Some(t) = &p.text {
                n += estimate_tokens(t);
            }
            if p.image_url.is_some() {
                n += 512;
            }
        }
    }
    if let Some(calls) = &msg.tool_calls {
        for c in calls {
            n += estimate_tokens(&c.function.name);
            n += estimate_tokens(&c.function.arguments);
        }
    }
    n
}

fn message_text(msg: &Message) -> String {
    if let Some(c) = &msg.content {
        return c.clone();
    }
    msg.content_parts
        .as_ref()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.text.as_ref())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// True when the live transcript (not system prompt docs) indicates sub-agent work.
fn subagent_spawn_in_transcript(req: &ChatCompletionRequest) -> bool {
    req.messages.iter().any(|m| {
        if m.role == Role::System {
            return false;
        }
        let t = message_text(m);
        t.contains("[Subagent Task]")
            || (m.role == Role::User
                && (t.contains("sessions_spawn(")
                    || t.contains("spawn a sub-agent")
                    || t.contains("spawn subagent")
                    || t.contains("子代理")
                    || t.contains("子 agent")))
    })
}

fn message_has_image(msg: &Message) -> bool {
    msg.content_parts
        .as_ref()
        .is_some_and(|p| p.iter().any(|part| part.image_url.is_some()))
}
