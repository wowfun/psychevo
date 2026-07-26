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
        assert!(!thread.__steer("stale-turn", "steer"));
        assert!(thread.__steer(&active_turn_id, "steer"));

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
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
