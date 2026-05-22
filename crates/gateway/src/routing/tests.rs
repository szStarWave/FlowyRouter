#[cfg(test)]
mod tests {
    use crate::api::openai::{
        ChatCompletionRequest, FunctionDefinition, Message, Role, ToolDefinition,
    };
    use flowy_config::{ConfigFile, UpstreamEndpoint};

    use crate::config::AppConfig;
    use crate::routing::{
        RouteTier, StepKind, decide, require_any_upstream,
    };
    use crate::session::SessionStore;

    fn test_config(edge: bool, cloud: bool) -> AppConfig {
        let mut file = ConfigFile::default();
        if edge {
            file.upstream.edge = Some(UpstreamEndpoint {
                base_url: "http://127.0.0.1:11434/v1".into(),
                api_key: None,
            });
        }
        if cloud {
            file.upstream.cloud = Some(UpstreamEndpoint {
                base_url: "https://api.deepseek.com/v1".into(),
                api_key: Some("test-key".into()),
            });
        }
        AppConfig::from_file(file, std::path::PathBuf::from("/tmp/flowy-test-config.toml"))
            .unwrap()
    }

    fn heartbeat_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "flowy-auto".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("You are OpenClaw".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: Some("[OpenClaw heartbeat poll]".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: vec![],
            stream: false,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        }
    }

    fn simple_greeting_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "abc".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("你好".into()),
                content_parts: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![],
            stream: true,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        }
    }

    fn complex_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "flowy-auto".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("You are a coding agent.".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: Some(
                        "Refactor the entire authentication module with tests and migration."
                            .into(),
                    ),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: vec![],
            stream: false,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        }
    }

    #[test]
    fn require_any_upstream_rejects_empty() {
        let cfg = test_config(false, false);
        assert!(require_any_upstream(&cfg).is_err());
    }

    #[test]
    fn simple_greeting_prefers_edge() {
        let cfg = test_config(true, true);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &simple_greeting_request(),
            &sessions,
            None,
        );
        assert_eq!(decision.step_kind, StepKind::DirectChat, "{:?}", decision);
        assert!(
            matches!(decision.route, RouteTier::Edge),
            "expected edge, got {:?} reasons {:?}",
            decision.route,
            decision.reason_codes
        );
    }

    #[test]
    fn heartbeat_prefers_edge() {
        let cfg = test_config(true, true);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &heartbeat_request(),
            &sessions,
            None,
        );
        assert_eq!(decision.step_kind, StepKind::HeartbeatAck, "{:?}", decision);
        assert!(
            matches!(decision.route, RouteTier::Edge),
            "expected edge, got {:?} reasons {:?}",
            decision.route,
            decision.reason_codes
        );
    }

    #[test]
    fn edge_only_forces_edge_even_for_hard_tasks() {
        let cfg = test_config(true, false);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &complex_request(),
            &sessions,
            None,
        );
        assert!(
            matches!(decision.route, RouteTier::Edge),
            "expected edge-only override, got {:?} {:?}",
            decision.route,
            decision.reason_codes
        );
        assert!(
            decision.reason_codes.iter().any(|c| c == "UPSTREAM_EDGE_ONLY"),
            "{:?}",
            decision.reason_codes
        );
    }

    fn hermes_mid_loop_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "flowy-auto".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("You are a coding agent.".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: Some("fix the bug".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::Assistant,
                    content: Some("I'll run a command.".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::Tool,
                    content: Some("ok".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: Some("call_1".into()),
                },
                Message {
                    role: Role::User,
                    content: Some("continue".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: vec![ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "exec".into(),
                    description: None,
                    parameters: serde_json::json!({}),
                },
            }],
            stream: true,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        }
    }

    #[test]
    fn hermes_mid_loop_not_initial_plan() {
        let cfg = test_config(true, true);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &hermes_mid_loop_request(),
            &sessions,
            None,
        );
        assert_ne!(
            decision.step_kind,
            StepKind::InitialPlan,
            "mid-loop with tool history should not be InitialPlan: {:?}",
            decision
        );
    }

    fn openclaw_system_with_spawn_docs() -> String {
        "Use sessions_spawn for larger work. Do not poll subagents in a loop.".into()
    }

    fn openclaw_time_question_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "Minimax-M2.5".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some(openclaw_system_with_spawn_docs()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: Some("你好".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::Assistant,
                    content: Some("你好！".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: Some("现在几点了？".into()),
                    content_parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: vec![ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "session_status".into(),
                    description: None,
                    parameters: serde_json::json!({}),
                },
            }],
            stream: true,
            tool_choice: None,
            max_tokens: None,
            ..Default::default()
        }
    }

    #[test]
    fn openclaw_system_docs_do_not_force_subagent_spawn() {
        let cfg = test_config(true, true);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &openclaw_time_question_request(),
            &sessions,
            None,
        );
        assert_ne!(
            decision.step_kind,
            StepKind::SubagentSpawn,
            "system prompt tool docs must not classify as spawn: {:?}",
            decision
        );
        assert!(
            !matches!(decision.route, RouteTier::Cloud),
            "simple time question should not force cloud: {:?}",
            decision
        );
    }

    #[test]
    fn cloud_only_forces_cloud() {
        let cfg = test_config(false, true);
        let sessions = SessionStore::new_in_memory();
        let decision = decide(
            &cfg,
            &simple_greeting_request(),
            &sessions,
            None,
        );
        assert!(
            matches!(decision.route, RouteTier::Cloud),
            "expected cloud-only override, got {:?} {:?}",
            decision.route,
            decision.reason_codes
        );
        assert!(
            decision.reason_codes.iter().any(|c| c == "UPSTREAM_CLOUD_ONLY"),
            "{:?}",
            decision.reason_codes
        );
    }
}
