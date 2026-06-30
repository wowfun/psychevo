    fn web_state() -> (tempfile::TempDir, WebState) {
        web_state_with_env(BTreeMap::new())
    }

    fn web_state_with_env(
        inherited_env: BTreeMap<String, String>,
    ) -> (tempfile::TempDir, WebState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let mut env = BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ]);
        env.extend(inherited_env);
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::new(state);
        let config = GatewayWebServerConfig::new(
            gateway,
            home,
            cwd,
            None,
            env,
            temp.path().join("static"),
        );
        (temp, WebState::new(config))
    }

    fn write_agent_definition(dir: &Path, name: &str, description: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("agent dir");
        let path = dir.join(format!("{name}.md"));
        std::fs::write(
            &path,
            format!("---\ndescription: {description:?}\n---\n\nUse this agent.\n"),
        )
        .expect("agent definition");
        path
    }

    #[test]
    fn peer_runtime_rejects_structured_self_agent_mention() {
        let (_temp, state) = web_state();
        let mut options = state.run_options(state.inner.cwd.clone(), None);
        options.runtime_ref = Some("opencode".to_string());
        let err = apply_mentions_to_run_options(
            &mut options,
            &[wire::GatewayMention {
                visible_text: "@opencode".to_string(),
                range: wire::GatewayMentionRange { start: 0, end: 9 },
                target: wire::GatewayMentionTarget::Agent {
                    name: "opencode".to_string(),
                    source: Some("generated".to_string()),
                    entrypoints: vec!["subagent".to_string()],
                    backend_ref: Some("opencode".to_string()),
                },
            }],
        )
        .expect_err("self delegation should be rejected");
        assert!(err.to_string().contains("already the current runtime"));
    }

    #[test]
    fn peer_runtime_allows_literal_agent_text_without_structured_mention() {
        let (_temp, state) = web_state();
        let mut options = state.run_options(state.inner.cwd.clone(), None);
        options.runtime_ref = Some("opencode".to_string());
        apply_mentions_to_run_options(&mut options, &[]).expect("literal text is not inspected");
        assert!(options.skill_inputs.is_empty());
    }

    fn web_state_with_static() -> (tempfile::TempDir, WebState) {
        let (temp, state) = web_state();
        let static_dir = temp.path().join("static");
        std::fs::create_dir_all(&static_dir).expect("static dir");
        std::fs::write(
            static_dir.join("index.html"),
            "<!doctype html><title>workbench</title>",
        )
        .expect("index");
        (temp, state)
    }

    fn append_accounted_assistant(
        state: &WebState,
        session_id: &str,
        context_tokens: u64,
        cache_read_tokens: u64,
    ) {
        state
            .inner
            .state
            .store()
            .append_message_with_metrics(
                session_id,
                &RuntimeMessage::Assistant {
                    content: vec![psychevo_runtime::AssistantBlock::Text {
                        text: "done".to_string(),
                    }],
                    timestamp_ms: 1,
                    finish_reason: Some("stop".to_string()),
                    outcome: psychevo_ai::Outcome::Normal,
                    model: Some("fake-model".to_string()),
                    provider: Some("fake-provider".to_string()),
                },
                Some(json!({
                    "input_tokens": context_tokens,
                    "total_tokens": context_tokens,
                    "cached_tokens": cache_read_tokens,
                })),
                None,
            )
            .expect("assistant");
    }

    async fn response_text(response: Response<Body>) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    async fn occupied_port_with_free_successor() -> TcpListener {
        for _ in 0..100 {
            let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupy port");
            let port = occupied.local_addr().expect("occupied addr").port();
            let Some(next_port) = port.checked_add(1) else {
                continue;
            };
            if let Ok(probe) = TcpListener::bind(("127.0.0.1", next_port)).await {
                drop(probe);
                return occupied;
            }
        }
        panic!("could not find adjacent free loopback ports");
    }

    #[tokio::test]
    async fn bind_gateway_web_server_falls_back_from_used_port() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        let static_dir = temp.path().join("static");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&static_dir).expect("static dir");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::new(state);
        let occupied = occupied_port_with_free_successor().await;
        let occupied_addr = occupied.local_addr().expect("occupied addr");
        let mut config = GatewayWebServerConfig::new(
            gateway,
            temp.path().join("home"),
            cwd,
            None,
            BTreeMap::new(),
            static_dir,
        );
        config.bind_addr = occupied_addr;
        config.bind_port_fallbacks = 1;

        let bound = bind_gateway_web_server(config).await.expect("bind");

        assert_eq!(bound.local_addr().ip(), occupied_addr.ip());
        assert_eq!(bound.local_addr().port(), occupied_addr.port() + 1);
    }

    #[tokio::test]
    async fn initialize_reports_current_profile() {
        let mut env = BTreeMap::new();
        env.insert("PSYCHEVO_PROFILE".to_string(), "coder".to_string());
        let (temp, state) = web_state_with_env(env);
        let home = temp.path().join("home").display().to_string();
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "initialize".to_string(),
                params: None,
            },
        )
        .await
        .expect("initialize");

        assert_eq!(value["profile"]["name"], "coder");
        assert_eq!(value["profile"]["home"].as_str(), Some(home.as_str()));
        assert_eq!(value["profile"]["default"], false);
    }

    #[tokio::test]
    async fn observability_read_returns_active_session_usage() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.cwd,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        append_accounted_assistant(&state, &session_id, 200, 50);
        bind_source_to_thread(&state, &scope, &session_id).expect("bind");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["sessionId"], session_id);
        assert_eq!(value["usage"]["contextInputTokens"], 200);
        assert_eq!(value["usage"]["cacheReadTokens"], 50);
        assert_eq!(value["usage"]["estimatedCostNanodollars"], 0);
        assert_eq!(value["usage"]["cacheReadPercent"], 25.0);
        let categories = value["context"]["categories"]
            .as_array()
            .expect("context categories");
        assert!(!categories.is_empty());
        assert!(
            categories
                .iter()
                .all(|category| category.get("details").is_some())
        );
        assert!(
            categories
                .iter()
                .all(|category| category.get("id").and_then(Value::as_str) != Some("free_space"))
        );
        let serialized_categories = serde_json::to_string(categories).expect("categories json");
        assert!(!serialized_categories.contains("done"));
        assert!(!serialized_categories.contains("content"));
    }

    #[tokio::test]
    async fn observability_read_projects_acp_peer_usage_update() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.cwd,
                "peer_agent",
                "opencode",
                "acp:opencode",
                Some(json!({
                    "peer_agent": {
                        "agentName": "opencode",
                        "backendId": "opencode",
                        "backendKind": "acp",
                        "nativeSessionId": "native-1",
                        "usageUpdate": {
                            "sessionUpdate": "usage_update",
                            "used": 1234,
                            "size": 8000,
                            "cost": {"amount": 0.0025, "currency": "USD"}
                        }
                    }
                })),
            )
            .expect("session");
        bind_source_to_thread(&state, &scope, &session_id).expect("bind");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["context"]["usedTokens"], 1234);
        assert_eq!(value["context"]["contextLimit"], 8000);
        assert_eq!(value["context"]["status"], "reported by ACP peer");
        assert_eq!(value["context"]["categories"], json!([]));
        assert_eq!(value["usage"]["reportedTotalTokens"], 1234);
        assert_eq!(value["usage"]["contextInputTokens"], 1234);
        assert_eq!(value["usage"]["estimatedCostNanodollars"], 2_500_000);
    }

    #[tokio::test]
    async fn observability_read_returns_explicit_thread_usage_without_active_binding() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.cwd,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        append_accounted_assistant(&state, &session_id, 90, 9);
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope(), "threadId": session_id })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["contextInputTokens"], 90);
        assert_eq!(value["usage"]["cacheReadPercent"], 10.0);
    }

    #[tokio::test]
    async fn observability_read_clears_usage_when_no_active_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({ "scope": scope.to_wire_scope() })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], false);
        assert_eq!(value["usage"]["reportedTotalTokens"], 0);
        assert_eq!(value["context"]["available"], false);
    }

    #[tokio::test]
    async fn browser_observability_read_authorizes_cross_cwd_thread() {
        let (temp, state) = web_state();
        let other_cwd = temp.path().join("other-work");
        std::fs::create_dir_all(&other_cwd).expect("other cwd");
        let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_cwd,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        append_accounted_assistant(&state, &session_id, 300, 150);
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    cwd: state.inner.cwd.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let current_scope = default_resolved_scope(&state, &auth).expect("scope");
        let (tx, _rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "observability/read".to_string(),
                params: Some(json!({
                    "scope": current_scope.to_wire_scope(),
                    "threadId": session_id
                })),
            },
        )
        .await
        .expect("observability/read");

        assert_eq!(value["usage"]["available"], true);
        assert_eq!(value["usage"]["sessionId"], session_id);
        assert_eq!(value["usage"]["contextInputTokens"], 300);
        assert_eq!(value["usage"]["cacheReadPercent"], 50.0);
    }

    #[test]
    fn start_empty_source_returns_null_thread_and_creates_no_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let snapshot = start_empty_source(&state, &scope).expect("snapshot");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert_eq!(
            state
                .inner
                .state
                .store()
                .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
                .expect("sessions")
                .len(),
            0
        );
        assert_eq!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .as_deref(),
            None
        );
    }

    #[test]
    fn start_empty_source_clears_binding_without_archiving_previous_history() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.cwd,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        bind_source_to_thread(&state, &scope, &session_id).expect("bind");

        let snapshot = start_empty_source(&state, &scope).expect("snapshot");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .is_none()
        );
        let active_ids = state
            .inner
            .state
            .store()
            .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
            .expect("active sessions")
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();

        assert_eq!(active_ids, vec![session_id]);
    }
