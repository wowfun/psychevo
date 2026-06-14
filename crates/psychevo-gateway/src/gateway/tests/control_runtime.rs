    #[tokio::test]
    async fn typed_steer_requires_expected_turn_id() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend);
        let source = GatewaySource::new("tui", "workdir").process();
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
    async fn interrupt_aborts_active_and_clear_queue_drops_pending_turns() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "workdir").process();

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
        let agents_dir = harness.workdir.join(".psychevo").join("agents");
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
