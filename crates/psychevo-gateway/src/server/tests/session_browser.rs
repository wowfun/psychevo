    #[tokio::test]
    async fn thread_list_returns_global_top_level_sessions_without_source_partition() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        let store = state.inner.state.store();
        let top_level = store
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("top level");
        let internal = store
            .create_session_with_metadata(
                &state.inner.workdir,
                "tui-side",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("internal");
        let child = store
            .create_child_session_with_metadata(
                &top_level,
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("child");
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("thread list");
        let sessions = value["sessions"].as_array().expect("sessions");
        let ids = sessions
            .iter()
            .filter_map(|session| session["id"].as_str())
            .collect::<Vec<_>>();

        assert!(ids.contains(&top_level.as_str()));
        assert!(!ids.contains(&internal.as_str()));
        assert!(!ids.contains(&child.as_str()));
        let listed = sessions
            .iter()
            .find(|session| session["id"].as_str() == Some(top_level.as_str()))
            .expect("top level listed");
        assert_eq!(
            listed["project"]["workdir"],
            other_workdir.display().to_string()
        );
        assert_eq!(listed["project"]["label"], "other-work");
        assert_eq!(listed["visibleEntryCount"], 0);
        assert!(listed.get("source").is_none());
    }

    #[tokio::test]
    async fn thread_browser_pages_workspace_sessions_and_keeps_include_exceptions() {
        let (_temp, state) = web_state();
        let workdir = state.inner.workdir.display().to_string();
        let store = state.inner.state.store();
        let mut ids = Vec::new();
        for index in 0..25 {
            let id = store
                .create_session_with_metadata(
                    &state.inner.workdir,
                    "web",
                    &format!("fake-model-{index}"),
                    "fake-provider",
                    None,
                )
                .expect("session");
            ids.push(id);
        }
        let (tx, _rx) = mpsc::unbounded_channel();

        let first = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/browser".to_string(),
                params: Some(json!({ "workdir": workdir.clone(), "limit": 20 })),
            },
        )
        .await
        .expect("thread/browser first page");
        let workspace = &first["workspaces"][0];
        assert_eq!(workspace["sessions"].as_array().expect("sessions").len(), 20);
        assert_eq!(workspace["hiddenCount"], 5);
        assert_eq!(workspace["nextCursor"]["offset"], 20);

        let second = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "thread/browser".to_string(),
                params: Some(json!({ "cursor": workspace["nextCursor"].clone(), "limit": 20 })),
            },
        )
        .await
        .expect("thread/browser second page");
        let second_workspace = &second["workspaces"][0];
        assert_eq!(
            second_workspace["sessions"]
                .as_array()
                .expect("second sessions")
                .len(),
            5
        );
        assert_eq!(second_workspace["hiddenCount"], 0);
        assert!(second_workspace["nextCursor"].is_null());
        let included_id = second_workspace["sessions"][0]["id"]
            .as_str()
            .expect("included candidate")
            .to_string();

        let included = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(3)),
                method: "thread/browser".to_string(),
                params: Some(json!({
                    "workdir": workdir.clone(),
                    "limit": 20,
                    "includeSessionIds": [included_id.clone()],
                })),
            },
        )
        .await
        .expect("thread/browser included session");
        let sessions = included["workspaces"][0]["sessions"]
            .as_array()
            .expect("included sessions");
        assert_eq!(sessions.len(), 21);
        assert!(sessions.iter().any(|session| session["id"] == included_id));
        assert_eq!(included["workspaces"][0]["hiddenCount"], 4);
    }

    #[tokio::test]
    async fn thread_snapshot_prunes_pending_permission_without_live_activity() {
        let (_temp, state) = web_state();
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        state
            .inner
            .pending_permissions
            .lock()
            .expect("pending permissions")
            .insert(
                "permission-1".to_string(),
                PendingPermissionView {
                    request_id: "permission-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    reason: "requires approval".to_string(),
                    thread_id: Some(session_id.clone()),
                    turn_id: Some("turn-1".to_string()),
                    activity_id: Some("missing-activity".to_string()),
                    owner_id: Some("gateway:foreign".to_string()),
                    lease_expires_at_ms: Some(gateway_now_ms() + 60_000),
                },
            );
        let scope = ResolvedScope {
            workdir: state.inner.workdir.clone(),
            source: state.inner.source.clone(),
        };

        let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");

        assert_eq!(
            snapshot["pendingPermissions"]
                .as_array()
                .expect("pending permissions")
                .len(),
            0
        );
        assert!(
            !state
                .inner
                .pending_permissions
                .lock()
                .expect("pending permissions")
                .contains_key("permission-1")
        );
    }

    #[tokio::test]
    async fn thread_snapshot_removes_pending_permission_after_activity_finishes() {
        let (_temp, state) = web_state();
        let store = state.inner.state.store();
        let session_id = store
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        let owner_id = "gateway:foreign";
        let activity = store
            .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
                activity_id: "activity-1",
                thread_id: Some(&session_id),
                source_key: None,
                turn_id: Some("turn-1"),
                kind: "turn",
                owner_id,
                owner_surface: Some("tui"),
                lease_expires_at_ms: gateway_now_ms() + 60_000,
                queued_turns: 0,
                superseded_activity_id: None,
                intent: None,
            })
            .expect("claim activity");
        state
            .inner
            .pending_permissions
            .lock()
            .expect("pending permissions")
            .insert(
                "permission-1".to_string(),
                PendingPermissionView {
                    request_id: "permission-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    reason: "requires approval".to_string(),
                    thread_id: Some(session_id.clone()),
                    turn_id: Some("turn-1".to_string()),
                    activity_id: Some(activity.activity_id.clone()),
                    owner_id: Some(owner_id.to_string()),
                    lease_expires_at_ms: Some(activity.lease_expires_at_ms),
                },
            );
        let scope = ResolvedScope {
            workdir: state.inner.workdir.clone(),
            source: state.inner.source.clone(),
        };

        let running = thread_snapshot(&state, &scope, Some(&session_id)).expect("running snapshot");
        assert_eq!(
            running["pendingPermissions"]
                .as_array()
                .expect("running permissions")
                .len(),
            1
        );

        store
            .finish_gateway_activity(
                &activity.activity_id,
                owner_id,
                activity.generation,
                "failed",
            )
            .expect("finish activity");
        let finished =
            thread_snapshot(&state, &scope, Some(&session_id)).expect("finished snapshot");

        assert_eq!(
            finished["pendingPermissions"]
                .as_array()
                .expect("finished permissions")
                .len(),
            0
        );
        assert!(
            !state
                .inner
                .pending_permissions
                .lock()
                .expect("pending permissions")
                .contains_key("permission-1")
        );
    }

    #[tokio::test]
    async fn turn_completed_event_removes_pending_permission_panel() {
        let (_temp, state) = web_state();
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        state.record_event_with_context(
            &GatewayEvent::PermissionRequested {
                request_id: "permission-1".to_string(),
                tool_name: "exec_command".to_string(),
                summary: "exec_command".to_string(),
                reason: "requires approval".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: true,
                timeout_secs: 300,
            },
            PendingInteractionContext {
                thread_id: Some(session_id.clone()),
                turn_id: Some("turn-1".to_string()),
                activity_id: Some("turn-1".to_string()),
                owner_id: Some(state.inner.gateway.owner_id().to_string()),
                lease_expires_at_ms: Some(gateway_now_ms() + 60_000),
            },
        );

        state.record_event(&GatewayEvent::TurnCompleted {
            thread_id: Some(session_id.clone()),
            turn_id: "turn-1".to_string(),
            outcome: Some("failed".to_string()),
            committed_entries: Vec::new(),
        });
        let scope = ResolvedScope {
            workdir: state.inner.workdir.clone(),
            source: state.inner.source.clone(),
        };
        let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");

        assert_eq!(
            snapshot["pendingPermissions"]
                .as_array()
                .expect("pending permissions")
                .len(),
            0
        );
        assert!(
            !state
                .inner
                .pending_permissions
                .lock()
                .expect("pending permissions")
                .contains_key("permission-1")
        );
    }

    #[tokio::test]
    async fn browser_cross_project_resume_authorizes_followup_rpcs_on_same_connection() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let (tx, _rx) = mpsc::unbounded_channel();

        handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/resume".to_string(),
                params: Some(json!({ "threadId": session_id })),
            },
        )
        .await
        .expect("thread/resume");
        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": other_workdir })),
            },
        )
        .await
        .expect("settings/read after cross-project resume");

        assert_eq!(
            settings["project"]["path"],
            other_workdir.display().to_string()
        );
    }

    #[tokio::test]
    async fn browser_project_group_start_adopts_known_session_project_scope() {
        let (temp, state) = web_state();
        let other_workdir = temp.path().join("other-work");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_workdir = canonicalize_workdir(&other_workdir).expect("other canonical");
        state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("existing project session");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let scope = ResolvedScope {
            workdir: other_workdir.clone(),
            source: workdir_source(&other_workdir),
        }
        .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let snapshot = handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "thread/start".to_string(),
                params: Some(json!({ "scope": scope })),
            },
        )
        .await
        .expect("thread/start in known project");
        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert_eq!(
            snapshot["scope"]["workdir"],
            other_workdir.display().to_string()
        );

        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": other_workdir })),
            },
        )
        .await
        .expect("settings/read after project start");

        assert_eq!(
            settings["project"]["path"],
            other_workdir.display().to_string()
        );
    }

    #[tokio::test]
    async fn browser_workspace_create_uses_configured_root_and_authorizes_workdir() {
        let (temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"
[workspaces]
root = "~/workspaces"
"#,
        )
        .expect("config");
        let browser_session_id = "browser-session".to_string();
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .insert(
                browser_session_id.clone(),
                BrowserSession {
                    workdir: state.inner.workdir.clone(),
                    source: state.inner.source.clone(),
                },
            );
        let auth = AuthContext::Browser {
            session_id: browser_session_id,
        };
        let (tx, _rx) = mpsc::unbounded_channel();

        let created = handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(1)),
                method: "workspace/create".to_string(),
                params: Some(json!({ "name": "Notes" })),
            },
        )
        .await
        .expect("workspace/create");
        let workdir = temp
            .path()
            .join("workspaces")
            .join("Notes")
            .canonicalize()
            .expect("created workdir");
        let workdir_string = workdir.display().to_string();

        assert_eq!(created["workdir"], workdir_string);
        assert_eq!(created["scope"]["workdir"], workdir_string);

        let settings = handle_rpc(
            state,
            auth,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(2)),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": workdir_string.clone() })),
            },
        )
        .await
        .expect("settings/read after workspace/create");

        assert_eq!(settings["workdir"], workdir_string);
        assert_eq!(settings["project"]["path"], workdir_string);
    }

    #[test]
    fn workspace_dir_name_rejects_path_components() {
        assert_eq!(workspace_dir_name(" notes ").expect("trimmed"), "notes");
        let err = workspace_dir_name("../notes").expect_err("parent path rejected");
        assert!(
            err.to_string()
                .contains("workspace name must be a single directory name")
        );
    }

    #[test]
    fn reset_source_to_empty_archives_previous_binding_without_replacement() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let first_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        bind_source_to_thread(&state, &scope, &first_id).expect("bind");

        let snapshot = reset_source_to_empty(&state, &scope).expect("reset");

        assert!(snapshot.get("thread").is_some_and(Value::is_null));
        assert!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .is_none()
        );
        assert!(
            state
                .inner
                .state
                .store()
                .session_summary(&first_id)
                .expect("first summary")
                .expect("first exists")
                .archived_at_ms
                .is_some()
        );
        assert_eq!(
            state
                .inner
                .state
                .store()
                .list_sessions_for_workdir_with_sources(&state.inner.workdir, &[])
                .expect("active sessions")
                .len(),
            0
        );
    }

    #[test]
    fn bind_source_to_thread_rebinds_existing_session() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");

        bind_source_to_thread(&state, &scope, &session_id).expect("bind");

        assert_eq!(
            state
                .inner
                .gateway
                .resolve_source_thread(&state.inner.source)
                .expect("source lookup")
                .as_deref(),
            Some(session_id.as_str())
        );
    }

    #[test]
    fn thread_snapshot_projects_visible_entries_for_history_session_with_messages() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        state
            .inner
            .state
            .store()
            .append_message(
                &session_id,
                &RuntimeMessage::User {
                    content: vec![UserContentBlock::text("hello history")],
                    timestamp_ms: 1,
                },
            )
            .expect("append user");
        state
            .inner
            .state
            .store()
            .append_message(
                &session_id,
                &RuntimeMessage::Assistant {
                    content: vec![psychevo_runtime::AssistantBlock::Text {
                        text: "hello from assistant".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: psychevo_runtime::Outcome::Normal,
                    model: Some("fake-model".to_string()),
                    provider: Some("fake-provider".to_string()),
                },
            )
            .expect("append assistant");
        let summary = state
            .inner
            .state
            .store()
            .session_summary(&session_id)
            .expect("summary")
            .expect("session exists");
        assert!(summary.message_count > 0);

        let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");
        let entries = snapshot["entries"].as_array().expect("entries array");

        assert_eq!(entries.len(), 2, "{snapshot:#}");
        assert_eq!(entries[0]["blocks"][0]["body"], "hello history");
        assert_eq!(entries[1]["blocks"][0]["body"], "hello from assistant");
    }

    #[test]
    fn bind_source_to_thread_keeps_previous_history_active() {
        let (_temp, state) = web_state();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let first = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("first");
        let second = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("second");

        bind_source_to_thread(&state, &scope, &first).expect("bind first");
        bind_source_to_thread(&state, &scope, &second).expect("bind second");

        assert!(
            state
                .inner
                .state
                .store()
                .session_summary(&first)
                .expect("first summary")
                .expect("first exists")
                .archived_at_ms
                .is_none()
        );
    }

    #[test]
    fn active_completion_token_keeps_at_paths_with_slashes() {
        let token = active_completion_token("@src/ma", 7).expect("token");

        assert_eq!(token.sigil, '@');
        assert_eq!(token.query, "src/ma");
        assert_eq!(token.start, 0);
        assert_eq!(token.end, 7);
    }
