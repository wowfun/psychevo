#[allow(unused_imports)]
use super::*;

#[cfg(test)]
mod metrics_tests {
    use super::*;

    fn event(sequence: u64, kind: &str, data: Value) -> TrajectoryEvent {
        TrajectoryEvent {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            sequence,
            case_id: "case".to_string(),
            kind: kind.to_string(),
            message: kind.to_string(),
            timestamp_ms: Some(0),
            data,
        }
    }

    fn acp_update(sequence: u64, update: Value) -> TrajectoryEvent {
        event(
            sequence,
            "acp_session_update",
            json!({
                "raw_event": {
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": "s",
                        "update": update,
                    },
                },
            }),
        )
    }

    #[test]
    fn acp_metrics_collect_prompt_response_usage_accounting_and_warnings() {
        let events = vec![
            event(0, "acp_agent_started", json!({})),
            event(1, "acp_agent_prompt_started", json!({})),
            acp_update(
                2,
                json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-1",
                    "title": "Tool",
                }),
            ),
            acp_update(
                3,
                json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": "failed",
                }),
            ),
            acp_update(
                4,
                json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": "failed",
                }),
            ),
            acp_update(
                5,
                json!({
                    "sessionUpdate": "usage_update",
                    "used": 512,
                    "size": 4096,
                    "cost": {
                        "amount": 0.12,
                        "currency": "USD",
                    },
                }),
            ),
            event(
                6,
                "acp_agent_prompt_finished",
                json!({
                    "prompt_result": {
                        "stopReason": "end_turn",
                        "usage": {
                            "inputTokens": 10,
                            "outputTokens": 5,
                            "cachedReadTokens": 2,
                            "totalTokens": 15,
                        },
                        "_meta": {
                            "psychevo": {
                                "turns": 2,
                                "warnings": ["MCP server degraded"],
                                "accounting": {
                                    "context_input_tokens": 10,
                                    "billable_input_tokens": 8,
                                    "billable_output_tokens": 5,
                                    "cache_read_tokens": 2,
                                    "reported_total_tokens": 15,
                                    "estimated_cost_nanodollars": 120000000,
                                    "pricing_source": "fixture",
                                    "pricing_tier": "standard",
                                },
                            },
                        },
                    },
                }),
            ),
            event(7, "acp_agent_finished", json!({ "ignored": true })),
        ];

        let observed = collect_case_observability(&events, 123);
        assert_eq!(observed.metrics.duration_ms, 123);
        assert_eq!(observed.metrics.tool_calls, 1);
        assert_eq!(observed.metrics.tool_errors, 1);
        assert_eq!(observed.metrics.turns, Some(2));
        assert_eq!(observed.metrics.usage.input_tokens, Some(10));
        assert_eq!(observed.metrics.usage.output_tokens, Some(5));
        assert_eq!(observed.metrics.usage.cache_read_tokens, Some(2));
        assert_eq!(observed.metrics.usage.total_tokens, Some(15));
        assert_eq!(
            observed.metrics.accounting.estimated_cost_nanodollars,
            Some(120000000)
        );
        assert_eq!(
            observed.metrics.accounting.pricing_source.as_deref(),
            Some("fixture")
        );
        assert_eq!(observed.metrics.cost.amount_usd, Some(0.12));
        assert_eq!(observed.warnings, vec!["MCP server degraded"]);
    }

    #[test]
    fn acp_metrics_synthesizes_usage_from_accounting_without_prompt_usage() {
        let events = vec![
            event(0, "acp_agent_prompt_started", json!({})),
            event(
                1,
                "acp_agent_prompt_finished",
                json!({
                    "prompt_result": {
                        "stopReason": "end_turn",
                        "_meta": {
                            "psychevo": {
                                "accounting": {
                                    "billable_input_tokens": 8,
                                    "billable_output_tokens": 5,
                                    "cache_read_tokens": 2,
                                    "reasoning_tokens": 1,
                                    "reported_total_tokens": 16,
                                },
                            },
                        },
                    },
                }),
            ),
        ];

        let observed = collect_case_observability(&events, 50);
        assert_eq!(observed.metrics.usage.input_tokens, Some(10));
        assert_eq!(observed.metrics.usage.output_tokens, Some(6));
        assert_eq!(observed.metrics.usage.cache_read_tokens, Some(2));
        assert_eq!(observed.metrics.usage.reasoning_tokens, Some(1));
        assert_eq!(observed.metrics.usage.total_tokens, Some(16));
    }

    #[test]
    fn harbor_environment_reads_terminal_bench_float_timeouts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task_dir = temp.path().join("task");
        fs::create_dir_all(task_dir.join("environment")).expect("environment");
        fs::write(
            task_dir.join("task.toml"),
            r#"
[verifier]
timeout_sec = 1800.0

[agent]
timeout_sec = 900.0

[environment]
docker_image = "example/task:latest"
build_timeout_sec = 600.0
cpus = 1
memory_mb = 4096
allow_internet = false
"#,
        )
        .expect("task.toml");
        let task = harbor_test_task(&task_dir);
        let environment = harbor_container_environment(&task).expect("environment");
        assert_eq!(
            environment.docker_image.as_deref(),
            Some("example/task:latest")
        );
        assert!(!environment.allow_internet);
        assert_eq!(environment.build_timeout_seconds, 600);
        assert_eq!(environment.memory_mb, Some(4096));
        assert_eq!(harbor_task_agent_timeout_seconds(&task), Some(900));

        let raw = fs::read_to_string(task_dir.join("task.toml")).expect("task.toml");
        let value: toml::Value = toml::from_str(&raw).expect("toml");
        assert_eq!(read_task_verifier_timeout(&value), Some(1800));
    }

    #[test]
    fn harbor_compose_hides_tests_and_solution_from_agent_phase() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task_dir = temp.path().join("task");
        fs::create_dir_all(task_dir.join("environment")).expect("environment");
        fs::create_dir_all(task_dir.join("tests")).expect("tests");
        fs::create_dir_all(task_dir.join("solution")).expect("solution");
        fs::write(
            task_dir.join("environment").join("Dockerfile"),
            "FROM scratch\n",
        )
        .expect("Dockerfile");
        fs::write(
            task_dir.join("task.toml"),
            r#"
[environment]
build_timeout_sec = 60
allow_internet = false
"#,
        )
        .expect("task.toml");
        let artifact_root = temp.path().join("artifacts");
        let logs_dir = artifact_root.join("logs");
        fs::create_dir_all(&logs_dir).expect("logs");
        let case = CasePlan {
            case_id: "case".to_string(),
            task_set: TaskSetManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                id: "set".to_string(),
                name: None,
                description: None,
                tasks: vec!["harbor/task".to_string()],
                manifest_path: temp.path().join("benchmark.toml"),
            },
            task: harbor_test_task(&task_dir),
            agent: harbor_test_agent(temp.path()),
        };
        let environment = harbor_container_environment(&case.task).expect("environment");
        let runtime = prepare_harbor_compose(&case, &artifact_root, &logs_dir, &environment)
            .expect("compose");
        let compose = fs::read_to_string(runtime.compose_path).expect("compose");
        assert!(compose.contains("target: /logs"));
        assert!(compose.contains("target: /peval"));
        assert!(compose.contains("network_mode: none"));
        assert!(!compose.contains("/tests"));
        assert!(!compose.contains("/solution"));
    }

    #[test]
    fn host_psychevo_acp_inherits_user_home_by_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let agent = harbor_test_agent(temp.path());

        let env = resolve_acp_env(&agent, &workspace).expect("acp env");

        assert!(!env.contains_key("PSYCHEVO_HOME"));
        assert!(!env.contains_key("PSYCHEVO_DB"));
        assert!(!env.contains_key("PSYCHEVO_CONFIG"));
        assert!(
            !workspace
                .join(".peval")
                .join("agent-state")
                .join("agent")
                .exists()
        );
    }

    fn harbor_test_task(task_dir: &Path) -> TaskManifest {
        TaskManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: "harbor/task".to_string(),
            name: None,
            kind: "harbor".to_string(),
            problem_statement: "Do the task.".to_string(),
            workspace: WorkspaceManifest {
                source: PathBuf::from("environment"),
            },
            test_spec: TestSpecManifest { checks: Vec::new() },
            source_kind: TaskSourceKind::Harbor,
            source_id: "harbor".to_string(),
            native_id: "task".to_string(),
            execution: ExecutionBackend::Container,
            verifier_timeout_seconds: None,
            manifest_path: task_dir.join("benchmark.toml"),
            dir: task_dir.to_path_buf(),
        }
    }

    fn harbor_test_agent(root: &Path) -> AgentManifest {
        AgentManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: "agent".to_string(),
            name: None,
            kind: AgentKind::PsychevoAcp,
            fake: FakeAgentOptions::default(),
            command: CommandAgentOptions::default(),
            acp: AcpAgentOptions::default(),
            psychevo: PsychevoAgentOptions::default(),
            opencode: WrapperAgentOptions::default(),
            hermes: WrapperAgentOptions::default(),
            manifest_path: root.join("eval.toml"),
        }
    }
}
