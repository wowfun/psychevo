    #[tokio::test]
    async fn invocation_source_does_not_bind_or_reuse() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("cli", "run-1").invocation();

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let second = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");

        assert_ne!(first.result.session_id, second.result.session_id);
        assert!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
        assert_eq!(backend.runs()[1].session, None);
    }
    #[tokio::test]
    async fn process_source_reuses_only_within_gateway_instance() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "cwd").process();

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let second = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");
        let rebuilt_gateway = Gateway::with_backend(harness.state.clone(), backend.clone());
        let third = rebuilt_gateway
            .send_turn(request(&harness, source.clone(), "third"))
            .await
            .expect("third turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        assert_ne!(first.result.session_id, third.result.session_id);
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(first.result.session_id.as_str())
        );
        assert!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
    }
    #[tokio::test]
    async fn persistent_source_round_trips_through_store() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("acp", "client-session").persistent();

        assert_eq!(
            harness
                .state
                .store()
                .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
                .expect("initial sessions")
                .len(),
            0
        );

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let rebuilt_gateway = Gateway::with_backend(harness.state.clone(), backend.clone());
        let second = rebuilt_gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        assert_eq!(backend.runs()[0].session, None);
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(first.result.session_id.as_str())
        );
        assert_eq!(
            harness
                .state
                .store()
                .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
                .expect("sessions")
                .len(),
            1
        );
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .expect("binding")
                .thread_id,
            first.result.session_id
        );
    }

    #[tokio::test]
    async fn bound_thread_uses_stored_cwd_over_request_default() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("im.wechat", "remote-lane").persistent();
        let changed_default = harness
            .cwd
            .parent()
            .expect("temp root")
            .join("changed-default");
        std::fs::create_dir_all(&changed_default).expect("changed default cwd");

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let mut second_request = request(&harness, source, "second");
        second_request.options.cwd = changed_default.clone();
        let second = harness
            .gateway
            .send_turn(second_request)
            .await
            .expect("second turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        let runs = backend.runs();
        assert_eq!(runs[1].session.as_deref(), Some(first.result.session_id.as_str()));
        assert_eq!(runs[0].cwd, harness.cwd);
        assert_eq!(runs[1].cwd, harness.cwd);
        assert_ne!(runs[1].cwd, changed_default);
    }

    #[tokio::test]
    async fn channel_connection_rotation_starts_next_turn_in_changed_default_cwd() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("im.wechat", "remote-lane")
            .persistent()
            .with_raw_identity(json!({
                "connectionId": "wechat",
                "chatId": "remote-lane",
            }));
        let other_source = GatewaySource::new("im.telegram", "remote-lane")
            .persistent()
            .with_raw_identity(json!({
                "connectionId": "telegram",
                "chatId": "remote-lane",
            }));
        let changed_default = harness
            .cwd
            .parent()
            .expect("temp root")
            .join("changed-default");
        std::fs::create_dir_all(&changed_default).expect("changed default cwd");

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let other = harness
            .gateway
            .send_turn(request(&harness, other_source.clone(), "other"))
            .await
            .expect("other turn");

        assert_eq!(
            harness
                .gateway
                .rotate_channel_connection_sources("wechat")
                .expect("rotate wechat"),
            1
        );
        assert!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&other_source.source_key().0)
                .expect("other binding lookup")
                .expect("other binding")
                .thread_id,
            other.result.session_id
        );
        let old_summary = harness
            .state
            .store()
            .session_summary(&first.result.session_id)
            .expect("old summary")
            .expect("old session");
        assert_eq!(
            old_summary.end_reason.as_deref(),
            Some("channel_workspace_changed")
        );
        assert!(old_summary.archived_at_ms.is_some());

        let mut second_request = request(&harness, source.clone(), "second");
        second_request.options.cwd = changed_default.clone();
        let second = harness
            .gateway
            .send_turn(second_request)
            .await
            .expect("second turn");

        assert_ne!(first.result.session_id, second.result.session_id);
        assert_eq!(backend.runs().last().expect("last run").cwd, changed_default);
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("new binding lookup")
                .expect("new binding")
                .thread_id,
            second.result.session_id
        );
    }

    #[tokio::test]
    async fn channel_connection_rotation_waits_for_running_bound_turn() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("im.wechat", "remote-lane")
            .persistent()
            .with_raw_identity(json!({
                "connectionId": "wechat",
                "chatId": "remote-lane",
            }));
        let changed_default = harness
            .cwd
            .parent()
            .expect("temp root")
            .join("changed-default");
        std::fs::create_dir_all(&changed_default).expect("changed default cwd");

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");

        let wait = backend.wait_on_next_run();
        let second_gateway = harness.gateway.clone();
        let second_request = request(&harness, source.clone(), "second-running");
        let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });
        wait.started.notified().await;

        assert_eq!(
            harness
                .gateway
                .rotate_channel_connection_sources("wechat")
                .expect("rotate wechat"),
            1
        );

        let third_gateway = harness.gateway.clone();
        let mut third_request = request(&harness, source.clone(), "third-new-cwd");
        third_request.options.cwd = changed_default.clone();
        let third = tokio::spawn(async move { third_gateway.send_turn(third_request).await });

        tokio::task::yield_now().await;
        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second-running".to_string()]
        );

        wait.release.notify_one();
        let second = second.await.expect("second task").expect("second turn");
        let third = third.await.expect("third task").expect("third turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        assert_ne!(first.result.session_id, third.result.session_id);
        let runs = backend.runs();
        assert_eq!(runs[1].cwd, harness.cwd);
        assert_eq!(runs[2].cwd, changed_default);
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("new binding lookup")
                .expect("new binding")
                .thread_id,
            third.result.session_id
        );
    }

    #[tokio::test]
    async fn first_shell_without_bound_source_creates_and_binds_runtime_session() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
        let source = GatewaySource::new("web", "cwd").persistent();
        let root = harness.cwd.parent().expect("temp root");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "lmstudio/test-model"

[provider.lmstudio.models.test-model]
"#,
        )
        .expect("config");

        let result = harness
            .gateway
            .send_shell(SendShellRequest {
                thread_id: None,
                source: Some(source.clone()),
                bind_source: None,
                cwd: harness.cwd.clone(),
                command: "printf shell-ok".to_string(),
                context: UserShellContextOptions {
                    state: harness.state.clone(),
                    session: None,
                    continue_latest: false,
                    source: "web".to_string(),
                    continue_sources: vec!["web".to_string()],
                    config_path: None,
                    model: None,
                    reasoning_effort: None,
                    mode: RunMode::Default,
                    inherited_env: Some(BTreeMap::from([
                        ("HOME".to_string(), root.to_string_lossy().to_string()),
                        (
                            "PSYCHEVO_HOME".to_string(),
                            home.to_string_lossy().to_string(),
                        ),
                    ])),
                },
                stream: None,
                event_sink: None,
                lineage: None,
            })
            .await
            .expect("shell");

        let session_id = result.result.session_id.expect("shell session");
        assert_eq!(result.thread.id, session_id);
        assert_eq!(result.result.outcome, Outcome::Normal);
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .expect("binding")
                .thread_id,
            session_id
        );
        assert_eq!(
            harness
                .state
                .store()
                .list_sessions_for_cwd_with_sources(&harness.cwd, &["web"])
                .expect("sessions")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn send_turn_serializes_same_source_fifo() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "cwd").process();

        let first_gateway = harness.gateway.clone();
        let first_request = request(&harness, source.clone(), "first");
        let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let second_gateway = harness.gateway.clone();
        let second_request = request(&harness, source.clone(), "second");
        let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });

        tokio::task::yield_now().await;
        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string()]
        );

        wait.release.notify_one();
        let first = first.await.expect("first task").expect("first turn");
        let second = second.await.expect("second task").expect("second turn");
        assert_eq!(first.result.session_id, second.result.session_id);
        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[tokio::test]
    async fn draft_source_lane_runs_while_previous_unbound_source_turn_finishes_later() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let canonical = GatewaySource::new("web", "cwd").persistent();
        let draft = GatewaySource::new("web", "cwd:draft:test").persistent();

        let first_gateway = harness.gateway.clone();
        let first_request = request(&harness, canonical.clone(), "first");
        let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
        wait.started.notified().await;

        harness
            .gateway
            .clear_source_binding(&canonical)
            .expect("thread/start clears canonical binding");

        let mut second_request = request(&harness, draft, "second");
        second_request.bind_source = Some(canonical.clone());
        let second = harness
            .gateway
            .send_turn(second_request)
            .await
            .expect("second draft turn");

        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&canonical.source_key().0)
                .expect("binding lookup")
                .expect("canonical binding after draft")
                .thread_id,
            second.result.session_id
        );

        wait.release.notify_one();
        let first = first.await.expect("first task").expect("first turn");

        assert_ne!(first.result.session_id, second.result.session_id);
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&canonical.source_key().0)
                .expect("binding lookup")
                .expect("canonical binding after stale completion")
                .thread_id,
            second.result.session_id
        );
    }

    #[tokio::test]
    async fn explicit_thread_turn_allows_source_rebind_while_running() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend);
        let source = GatewaySource::new("web", "cwd").persistent();
        let first = harness
            .state
            .store()
            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("first session");
        let second = harness
            .state
            .store()
            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("second session");
        harness
            .gateway
            .bind_source_thread(
                &source,
                &first,
                &GatewayBackendInfo {
                    kind: BackendKind::Psychevo,
                    native_id: Some(first.clone()),
                },
                None,
            )
            .expect("bind first");

        let mut first_request = request(&harness, source.clone(), "first");
        first_request.thread_id = Some(first.clone());
        let gateway = harness.gateway.clone();
        let running = tokio::spawn(async move { gateway.send_turn(first_request).await });
        wait.started.notified().await;

        harness
            .gateway
            .bind_source_thread(
                &source,
                &second,
                &GatewayBackendInfo {
                    kind: BackendKind::Psychevo,
                    native_id: Some(second.clone()),
                },
                None,
            )
            .expect("bind second");

        assert!(
            harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(&first))
                .running
        );
        assert!(
            !harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::source(source.source_key()))
                .running
        );

        wait.release.notify_one();
        running
            .await
            .expect("running task")
            .expect("running result");
    }
