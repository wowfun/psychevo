    #[tokio::test]
    async fn typed_steer_requires_expected_turn_id() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend);
        let source = GatewaySource::new("tui", "cwd").process();
        let selector = GatewayThreadSelector::source(source.source_key());

        let (handle, control) = run_control();
        let mut first_request = request(&harness, source.clone(), "first");
        first_request.control_handle = Some(handle);
        first_request.control = Some(control);
        let gateway = harness.gateway.clone();
        let first = tokio::spawn(async move { gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let active_turn_id = harness
            .gateway
            .activity_for_selector(selector.clone())
            .active_turn_id
            .expect("active turn id");
        let message = Message::User {
            content: vec![UserContentBlock::text("steer")],
            timestamp_ms: 0,
        };

        assert!(
            harness
                .gateway
                .steer_turn(selector.clone(), Some("stale-turn"), message.clone())
                .is_none()
        );
        let input_id = harness
            .gateway
            .steer_turn(selector.clone(), Some(&active_turn_id), message.clone())
            .expect("current turn steer");
        assert!(!harness.gateway.update_steer(
            selector.clone(),
            Some("stale-turn"),
            input_id,
            message.clone()
        ));
        assert!(harness.gateway.update_steer(
            selector.clone(),
            Some(&active_turn_id),
            input_id,
            message.clone()
        ));
        assert!(
            !harness
                .gateway
                .cancel_steer(selector.clone(), Some("stale-turn"), input_id)
        );
        assert!(
            harness
                .gateway
                .cancel_steer(selector, Some(&active_turn_id), input_id)
        );

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
    }

    #[tokio::test]
        async fn native_agent_adapter_lowers_runtime_control_map_without_dispatch_name_branch() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
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
                .gateway
                .send_turn(request)
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
                .store()
                .gateway_runtime_binding(&result.thread.id)
                .expect("binding read")
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
            let harness = harness(backend);
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
            let first = harness.gateway.send_turn(first).await.expect("first turn");
            let binding = harness
                .state
                .store()
                .gateway_runtime_binding(&first.thread.id)
                .expect("binding read")
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
            second.options.inherited_env = Some(env);
            let second = harness
                .gateway
                .send_turn(second)
                .await
                .expect("captured Agent Definition remains authoritative");
            let binding = harness
                .state
                .store()
                .gateway_runtime_binding(&second.thread.id)
                .expect("binding read")
                .expect("binding");
            assert!(binding.agent_definition_json.as_deref().is_some_and(|snapshot| {
                snapshot.contains("Review version one.")
                    && !snapshot.contains("Review version two.")
            }));
        }

        #[test]
    fn acp_binding_rejects_public_steer_before_queueing() {
        let harness = harness(Arc::new(FakeBackend::default()));
        let thread_id = harness
            .state
            .store()
            .create_session_with_metadata(&harness.cwd, "test", "model", "provider", None)
            .expect("session");
        let cwd = harness.cwd.to_string_lossy().to_string();
        harness
            .state
            .store()
            .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
                thread_id: &thread_id,
                agent_ref: Some("opencode"),
                agent_fingerprint: "agent-fingerprint",
                agent_definition_json: r#"{"name":"opencode"}"#,
                runtime_ref: "opencode",
                backend_kind: "acp",
                native_kind: "acp",
                native_session_id: Some("native-session"),
                cwd: &cwd,
                profile_fingerprint: "fingerprint",
                profile_revision: "1",
                profile_config_json: "{}",
                adapter_kind: "acp",
                adapter_revision: "1",
                ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                parent_thread_id: None,
            })
            .expect("runtime binding");
        let (handle, _control) = run_control();
        harness.gateway.register_active(
            &thread_key(&thread_id),
            "turn-1".to_string(),
            Some(handle),
            ActiveActivityKind::Turn,
        );

        let selector = GatewayThreadSelector::thread_id(thread_id);
        assert!(
            harness
                .gateway
                .steer_turn(
                    selector.clone(),
                    Some("turn-1"),
                    psychevo_agent_core::user_text_message("unsupported steer"),
                )
                .is_none()
        );
        assert!(!harness.gateway.steer_foreign_turn(
            selector,
            Some("turn-1"),
            psychevo_agent_core::user_text_message("unsupported foreign steer"),
        ));
    }
    #[tokio::test]
    async fn interrupt_aborts_active_and_clear_queue_drops_pending_turns() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "cwd").process();

        let (handle, control) = run_control();
        let mut first_request = request(&harness, source.clone(), "first");
        first_request.control_handle = Some(handle);
        first_request.control = Some(control);
        let first_gateway = harness.gateway.clone();
        let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let second_gateway = harness.gateway.clone();
        let second_request = request(&harness, source.clone(), "second");
        let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });
        tokio::task::yield_now().await;

        let selector = GatewayThreadSelector::source(source.source_key());
        assert!(harness.gateway.interrupt_turn(selector.clone()));
        let mut cleared = harness.gateway.clear_queue(selector);
        for _ in 0..10 {
            if cleared > 0 {
                break;
            }
            tokio::task::yield_now().await;
            cleared = harness
                .gateway
                .clear_queue(GatewayThreadSelector::source(source.source_key()));
        }
        assert_eq!(cleared, 1);

        let second_err = second
            .await
            .expect("second task")
            .expect_err("queued turn should be cleared");
        assert!(second_err.to_string().contains("queue cleared"));

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
    }
    #[tokio::test]
    async fn runtime_ref_resolves_generated_peer_backend_without_agent_selection() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
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
        let harness = harness(backend);
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
