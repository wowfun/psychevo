    #[tokio::test]
    async fn typed_steer_requires_expected_turn_id() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend).await;
        let source = GatewaySource::new("tui", "cwd").process();

        let (handle, control) = run_control();
        let mut first_request = request(&harness, source.clone(), "first");
        first_request.control_handle = Some(handle);
        first_request.control = Some(control);
        let application = harness._application.clone();
        let gateway = harness.gateway.clone();
        let first = tokio::spawn(async move {
            send_framework_turn(application, gateway, first_request).await
        });
        wait.started.notified().await;

        let thread_id = harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("source lookup")
            .expect("source binding");
        let thread = harness
            ._application
            .client()
            .resume_thread(&thread_id)
            .await
            .expect("Framework Thread");
        let active_turn_id = thread.__activity().1.expect("active turn id");
        assert!(!thread.__steer("stale-turn", "steer").expect("stale result"));
        assert!(
            thread
                .__steer(&active_turn_id, "steer")
                .expect("active steer")
        );

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
    }

    #[tokio::test]
    async fn durable_steer_remains_pending_until_control_capacity_recovers() {
        let harness = harness(Arc::new(FakeBackend::default())).await;
        let activity_id = "foreign-turn".to_string();
        let (handle, mut control) = run_control();
        let mut pending_ids = Vec::new();
        for index in 0..psychevo::__agent_core::MAX_CONTROL_INPUT_ITEMS {
            pending_ids.push(
                handle
                    .steer_user_message(psychevo::__agent_core::user_text_message(format!(
                        "queued-{index}"
                    )))
                    .expect("fill control input"),
            );
        }
        harness.gateway.register_active(
            "foreign-control",
            activity_id.clone(),
            Some(handle.clone()),
            ActiveActivityKind::Shell,
        );
        harness
            .state
            .enqueue_gateway_control_command(GatewayControlCommandInput {
                activity_id: &activity_id,
                owner_id: harness.gateway.owner_id(),
                command_kind: "steer",
                payload: json!({
                    "expectedTurnId": activity_id,
                    "message": psychevo::__agent_core::user_text_message("foreign steer"),
                }),
            })
            .await
            .expect("enqueue steer");

        harness
            .gateway
            .apply_pending_gateway_control_commands()
            .await;
        assert_eq!(
            harness
                .state
                .pending_gateway_control_commands(harness.gateway.owner_id(), 10)
                .await
                .expect("pending commands")
                .len(),
            1
        );

        assert!(handle.cancel_pending_user_message(pending_ids[0]));
        harness
            .gateway
            .apply_pending_gateway_control_commands()
            .await;
        assert!(
            harness
                .state
                .pending_gateway_control_commands(harness.gateway.owner_id(), 10)
                .await
                .expect("pending commands")
                .is_empty()
        );
        assert!(control.drain_pending_user_messages().iter().any(|(_, message)| {
            matches!(
                message,
                psychevo::__agent_core::Message::User { content, .. }
                    if content.iter().any(
                        |block| block.text_value() == Some("foreign steer")
                    )
            )
        }));
    }

    #[tokio::test]
    async fn permanently_oversized_durable_steer_is_rejected_before_acceptance() {
        let harness = harness(Arc::new(FakeBackend::default())).await;
        let activity_id = "foreign-turn";
        let accepted = harness
            .gateway
            .enqueue_exact_foreign_control_command(
                activity_id,
                harness.gateway.owner_id(),
                "steer",
                json!({
                    "expectedTurnId": activity_id,
                    "message": psychevo::__agent_core::user_text_message(
                        "x".repeat(psychevo::__agent_core::MAX_CONTROL_INPUT_BYTES + 1)
                    ),
                }),
            )
            .await;

        assert!(!accepted);
        assert!(
            harness
                .state
                .pending_gateway_control_commands(harness.gateway.owner_id(), 10)
                .await
                .expect("pending commands")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn gateway_shutdown_drains_every_scope_before_reporting_a_task_panic() {
        let harness = harness(Arc::new(FakeBackend::default())).await;
        let producer_started = Arc::new(tokio::sync::Notify::new());
        let producer_started_for_task = Arc::clone(&producer_started);
        let turn_completed = Arc::new(AtomicBool::new(false));
        let turn_completed_for_task = turn_completed.clone();
        let infrastructure_completed = Arc::new(AtomicBool::new(false));
        let infrastructure_completed_for_task = infrastructure_completed.clone();
        harness
            .gateway
            .supervisor
            .spawn_producer("panic-producer", async move {
                producer_started_for_task.notify_one();
                panic!("injected Gateway producer panic");
            });
        producer_started.notified().await;
        harness.gateway.supervisor.spawn_turn("turn", async move {
            turn_completed_for_task.store(true, Ordering::Release);
        });
        harness
            .gateway
            .supervisor
            .spawn_infrastructure("infrastructure", async move {
                infrastructure_completed_for_task.store(true, Ordering::Release);
            });

        let error = harness
            .gateway
            .shutdown_application(false)
            .await
            .expect_err("panic must make shutdown non-clean");

        assert!(turn_completed.load(Ordering::Acquire));
        assert!(infrastructure_completed.load(Ordering::Acquire));
        assert!(error.to_string().contains("panic-producer"));
        assert!(error.to_string().contains("Producer"));
    }

    #[tokio::test]
    async fn native_agent_adapter_lowers_runtime_control_map_without_dispatch_name_branch() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone()).await;
        let mut request = request(
            &harness,
            GatewaySource::new("web", "native-controls").process(),
            "control lowering",
        );
        request.options.runtime_options = BTreeMap::from([
            ("model".to_string(), "model-a".to_string()),
            ("reasoning".to_string(), "high".to_string()),
            ("mode".to_string(), "plan".to_string()),
            ("permissionMode".to_string(), "dontAsk".to_string()),
        ]);

            let result = harness
                .send(request)
                .await
                .expect("Native turn");

            let runs = backend.runs();
        let run = runs.first().expect("captured Native request");
        assert_eq!(run.model.as_deref(), Some("model-a"));
        assert_eq!(run.reasoning_effort.as_deref(), Some("high"));
            assert_eq!(run.mode, RunMode::Plan);
            assert_eq!(run.permission_mode, Some(PermissionMode::DontAsk));
            assert!(run.runtime_options.is_empty());
            let binding = harness
                .state

                .gateway_runtime_binding(&result.thread.id)
                .await.expect("binding read")
                .expect("binding");
            assert_eq!(binding.agent_ref, None);
            assert!(binding.agent_fingerprint.is_some());
            assert!(
                binding
                    .agent_definition_json
                    .as_deref()
                    .is_some_and(|snapshot| snapshot.contains("psychevo.default-agent"))
            );
        }

        #[tokio::test]
        async fn bound_named_agent_ignores_current_definition_drift() {
            let backend = Arc::new(FakeBackend::default());
            let harness = harness(backend).await;
            let home = harness._temp.path().join("home");
            let agents = harness.cwd.join(".psychevo/agents");
            std::fs::create_dir_all(&home).expect("home");
            std::fs::create_dir_all(&agents).expect("agents");
            let definition = agents.join("reviewer.md");
            std::fs::write(
                &definition,
                "---\ndescription: Reviewer\n---\nReview version one.\n",
            )
            .expect("Agent Definition");
            let env = BTreeMap::from([
                (
                    "HOME".to_string(),
                    harness._temp.path().display().to_string(),
                ),
                ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            ]);
            let source = GatewaySource::new("web", "agent-fingerprint").process();
            let mut first = request(&harness, source.clone(), "first");
            first.options.agent = Some("reviewer".to_string());
            first.options.inherited_env = Some(env.clone());
            let first = harness.send(first).await.expect("first turn");
            let binding = harness
                .state

                .gateway_runtime_binding(&first.thread.id)
                .await.expect("binding read")
                .expect("binding");
            assert_eq!(binding.agent_ref.as_deref(), Some("reviewer"));
            assert!(binding.agent_definition_json.as_deref().is_some_and(|snapshot| {
                snapshot.contains("Review version one.")
            }));

            std::fs::write(
                &definition,
                "---\ndescription: Reviewer\n---\nReview version two.\n",
            )
            .expect("changed Agent Definition");
            let mut second = request(&harness, source, "second");
            second.thread_id = Some(first.thread.id);
            second.explicit_thread = true;
            second.options.inherited_env = Some(env);
            let second = harness
                .send(second)
                .await
                .expect("captured Agent Definition remains authoritative");
            let binding = harness
                .state

                .gateway_runtime_binding(&second.thread.id)
                .await.expect("binding read")
                .expect("binding");
            assert!(binding.agent_definition_json.as_deref().is_some_and(|snapshot| {
                snapshot.contains("Review version one.")
                    && !snapshot.contains("Review version two.")
            }));
        }

    #[tokio::test]
    async fn runtime_ref_resolves_generated_peer_backend_without_agent_selection() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend).await;
        let home = harness._temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            home.join("config.toml"),
            r#"[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP runtime."
command = "opencode"
args = ["acp"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
        )
        .expect("config");

        let env = BTreeMap::from([
            (
                "HOME".to_string(),
                harness._temp.path().display().to_string(),
            ),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
        ]);
        let mut options = run_options(&harness, "hello");
        options.runtime_ref = Some("opencode".to_string());
        options.inherited_env = Some(env);

        let peer = resolve_peer_turn(&options)
            .expect("resolve peer")
            .expect("peer runtime");

        assert_eq!(peer.agent.name, "opencode");
        assert_eq!(peer.backend.id, "opencode");
    }

    #[tokio::test]
    async fn runtime_ref_rejects_local_agent_definitions() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend).await;
        let home = harness._temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            home.join("config.toml"),
            r#"[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP runtime."
command = "opencode"
args = ["acp"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
        )
        .expect("config");
        let agents_dir = harness.cwd.join(".psychevo").join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir");
        std::fs::write(
            agents_dir.join("translate.md"),
            r#"---
name: translate
description: Translate messages.
entrypoints: [subagent]
---
Translate the prompt.
"#,
        )
        .expect("agent file");

        let env = BTreeMap::from([
            (
                "HOME".to_string(),
                harness._temp.path().display().to_string(),
            ),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
        ]);
        let mut options = run_options(&harness, "hello");
        options.agent = Some("translate".to_string());
        options.runtime_ref = Some("opencode".to_string());
        options.inherited_env = Some(env);

        let error = resolve_peer_turn(&options).expect_err("incompatible runtime");

        assert!(
            error
                .to_string()
                .contains("ACP peer runtimes run their own modes"),
            "{error}"
        );
    }
