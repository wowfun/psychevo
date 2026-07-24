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

                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(second.result.session_id.as_str()),
            "the immutable public thread is materialized before backend dispatch"
        );
    }

    #[tokio::test]
    async fn invocation_source_continue_latest_reuses_matching_public_thread() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());

        let mut initial = request(
            &harness,
            GatewaySource::new("cli", "run-1").invocation(),
            "first",
        );
        initial.options.cwd = harness.cwd.join("..").join("work");
        let first = harness
            .gateway
            .send_turn(initial)
            .await
            .expect("first turn");
        let mut continued = request(
            &harness,
            GatewaySource::new("cli", "run-2").invocation(),
            "second",
        );
        continued.options.continue_latest = true;
        let second = harness
            .gateway
            .send_turn(continued)
            .await
            .expect("continued turn");

        assert_eq!(second.result.session_id, first.result.session_id);
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(first.result.session_id.as_str())
        );
        assert_eq!(
            harness
                .state

                .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
                .expect("sessions")
                .len(),
            1
        );
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
        assert_eq!(
            backend.runs()[0].session.as_deref(),
            Some(first.result.session_id.as_str()),
            "the immutable public thread is materialized before backend dispatch"
        );
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(first.result.session_id.as_str())
        );
        assert_eq!(
            harness
                .state

                .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
                .expect("sessions")
                .len(),
            1
        );
        let lane = harness
            .state

            .gateway_source_lane(&source.source_key().0)
            .expect("lane lookup")
            .expect("lane");
        assert_eq!(lane.thread_id.as_deref(), Some(first.result.session_id.as_str()));
        let legacy_projection = harness
            .state

            .gateway_source_binding(&source.source_key().0)
            .expect("legacy binding lookup")
            .expect("legacy binding projection");
        assert_eq!(legacy_projection.backend_kind, "unresolved");
        assert_eq!(legacy_projection.backend_native_id, None);
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

                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
        assert_eq!(
            harness
                .state

                .gateway_source_binding(&other_source.source_key().0)
                .expect("other binding lookup")
                .expect("other binding")
                .thread_id,
            other.result.session_id
        );
        let old_summary = harness
            .state

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

                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .expect("binding")
                .thread_id,
            session_id
        );
        assert_eq!(
            harness
                .state

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
            .expect("draft open clears canonical binding");

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

            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("first session");
        let second = harness
            .state

            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("second session");
        harness
            .gateway
            .bind_source_thread(
                &source,
                &first,
                &GatewayBackendInfo {
                    kind: BackendKind::Native,
                    runtime_ref: Some("native".to_string()),
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
                    kind: BackendKind::Native,
                    runtime_ref: Some("native".to_string()),
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

    #[tokio::test]
    async fn durable_activity_does_not_rebind_parent_turn_to_scoped_child_turn_started() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
        let turn_id = "turn-parent";
        let parent_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("parent session");
        let child_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "agent", "model", "provider", None)
            .expect("child session");

        let activity = harness
            .gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: turn_id,
                thread_id: None,
                source_key: Some("web:test"),
                turn_id: Some(turn_id),
                kind: "turn",
                owner_surface: Some("web"),
                queued_turns: 0,
                intent: Some(json!({
                    "kind": "turn",
                    "threadId": parent_thread.clone(),
                })),
            })
            .expect("claim activity");
        assert!(
            harness
                .state

                .update_gateway_activity_thread(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    &parent_thread,
                    gateway_now_ms() + 30_000,
                )
                .expect("parent turn started")
        );
        assert!(
            !harness
                .state

                .update_gateway_activity_thread(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    &child_thread,
                    gateway_now_ms() + 30_000,
                )
                .expect("scoped child turn started")
        );

        let record = harness
            .state

            .gateway_activity(turn_id)
            .expect("activity lookup")
            .expect("activity");
        assert_eq!(record.thread_id.as_deref(), Some(parent_thread.as_str()));
    }

    #[tokio::test]
    async fn gateway_event_sink_does_not_alias_scoped_child_thread_to_parent_activity() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
        let turn_id = "turn-parent";
        let queue_key = "source:web:test";
        let parent_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("parent session");
        let child_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "agent", "model", "provider", None)
            .expect("child session");

        harness.gateway.register_active(
            queue_key,
            turn_id.to_string(),
            None,
            ActiveActivityKind::Turn,
        );
        let activity = harness
            .gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: turn_id,
                thread_id: None,
                source_key: Some("web:test"),
                turn_id: Some(turn_id),
                kind: "turn",
                owner_surface: Some("web"),
                queued_turns: 0,
                intent: Some(json!({
                    "kind": "turn",
                    "threadId": parent_thread.clone(),
                })),
            })
            .expect("claim activity");
        let sink = harness
            .gateway
            .wrap_gateway_event_sink(
                None,
                Some(activity),
                Some(queue_key.to_string()),
                Some(turn_id.to_string()),
            )
            .expect("event sink");

        sink(GatewayEvent::TurnStarted {
            thread_id: Some(parent_thread.clone()),
            turn_id: turn_id.to_string(),
            selected_skills: Vec::new(),
        });
        sink(GatewayEvent::TurnStarted {
            thread_id: Some(child_thread.clone()),
            turn_id: turn_id.to_string(),
            selected_skills: Vec::new(),
        });

        assert!(
            harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread))
                .running
        );
        assert!(
            !harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(&child_thread))
                .running
        );
    }

    #[test]
    fn gateway_event_sink_attributes_child_interaction_to_child_activity() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
        let parent_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
            .expect("parent session");
        let child_thread = harness
            .state

            .create_session_with_metadata(&harness.cwd, "agent", "model", "provider", None)
            .expect("child session");
        let parent_activity = harness
            .gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-parent",
                thread_id: Some(&parent_thread),
                source_key: None,
                turn_id: Some("turn-parent"),
                kind: "turn",
                owner_surface: Some("web"),
                queued_turns: 0,
                intent: None,
            })
            .expect("parent activity");
        harness
            .gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-child",
                thread_id: Some(&child_thread),
                source_key: None,
                turn_id: Some("turn-child"),
                kind: "turn",
                owner_surface: Some("agent"),
                queued_turns: 0,
                intent: None,
            })
            .expect("child activity");
        let observed = Arc::new(Mutex::new(None));
        let observed_for_sink = Arc::clone(&observed);
        let downstream: GatewayEventSink = Arc::new(move |event| {
            *observed_for_sink.lock().expect("observed event") = Some(event);
        });
        let sink = harness
            .gateway
            .wrap_gateway_event_sink(
                Some(downstream),
                Some(parent_activity),
                Some("thread:parent".to_string()),
                Some("turn-parent".to_string()),
            )
            .expect("wrapped sink");

        sink(GatewayEvent::ActionRequested {
            action: PendingActionView {
                action_id: "permission-child".to_string(),
                kind: GatewayActionKind::Permission,
                title: Some("child permission".to_string()),
                summary: None,
                payload: json!({}),
                thread_id: Some(child_thread),
                turn_id: Some("turn-child".to_string()),
                activity_id: None,
                source_key: None,
                owner_id: None,
                lease_expires_at_ms: None,
            },
        });

        let event = observed
            .lock()
            .expect("observed event")
            .clone()
            .expect("event");
        let GatewayEvent::ActionRequested { action } = event else {
            panic!("expected child action");
        };
        assert_eq!(action.activity_id.as_deref(), Some("turn-child"));
        assert_eq!(action.turn_id.as_deref(), Some("turn-child"));
    }
    #[tokio::test]
    async fn draft_mutation_guard_serializes_only_the_same_source() {
        let harness = harness(Arc::new(FakeBackend::default()));
        let source = GatewaySource::new("web", "same-source").persistent();
        let other = GatewaySource::new("web", "other-source").persistent();
        let first_guard = harness
            .gateway
            .lock_source_mutation(&source.source_key())
            .await;

        let other_guard = tokio::time::timeout(
            Duration::from_secs(1),
            harness.gateway.lock_source_mutation(&other.source_key()),
        )
        .await
        .expect("an unrelated source must remain concurrent");
        drop(other_guard);

        let gateway = harness.gateway.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (acquired_tx, mut acquired_rx) = oneshot::channel();
        let queued_source = source.clone();
        let queued = tokio::spawn(async move {
            let _ = started_tx.send(());
            let _guard = gateway
                .lock_source_mutation(&queued_source.source_key())
                .await;
            let _ = acquired_tx.send(());
        });
        started_rx.await.expect("queued mutation started");
        assert!(matches!(
            acquired_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        drop(first_guard);
        tokio::time::timeout(Duration::from_secs(1), acquired_rx)
            .await
            .expect("same-source mutation must proceed after release")
            .expect("queued mutation acquired guard");
        queued.await.expect("queued mutation task");
    }
