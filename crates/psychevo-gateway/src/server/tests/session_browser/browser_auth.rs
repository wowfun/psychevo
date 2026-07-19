#[tokio::test]
async fn browser_cross_project_resume_authorizes_followup_rpcs_on_same_connection() {
    let (temp, state) = web_state();
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&other_cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    let browser_session_id = "browser-session".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id.clone(),
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
    assert!(
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .get(&browser_session_id)
            .expect("browser session")
            .external_action_grants
            .contains(&normalized_native_path(&other_cwd))
    );
    let settings = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": other_cwd })),
        },
    )
    .await
    .expect("settings/read after cross-project resume");

    assert_eq!(
        settings["project"]["path"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
}

#[tokio::test]
async fn browser_session_profile_auth_allows_global_settings_for_other_cwd() {
    let (temp, state) = web_state();
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let browser_session_id = "browser-session".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let (tx, _rx) = mpsc::unbounded_channel();

    let settings = handle_rpc(
        state,
        AuthContext::Browser {
            session_id: browser_session_id,
        },
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": other_cwd })),
        },
    )
    .await
    .expect("settings/read for other cwd");

    assert_eq!(
        settings["project"]["path"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
}

#[tokio::test]
async fn browser_project_group_start_adopts_known_session_project_scope() {
    let (temp, state) = web_state();
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    state
        .inner
        .state
        .store()
        .create_session_with_metadata(&other_cwd, "web", "fake-model", "fake-provider", None)
        .expect("existing project session");
    let browser_session_id = "browser-session".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id.clone(),
    };
    let scope = ResolvedScope {
        cwd: other_cwd.clone(),
        source: cwd_source(&other_cwd),
    }
    .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let opened = handle_rpc(
        state.clone(),
        auth.clone(),
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/draft/open".to_string(),
            params: Some(json!({ "origin": scope, "targetIntent": { "kind": "default" } })),
        },
    )
    .await
    .expect("thread/draft/open in known project");
    let snapshot = &opened["snapshot"];
    assert!(snapshot.get("thread").is_some_and(Value::is_null));
    assert_eq!(
        snapshot["scope"]["cwd"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
    assert!(
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .get(&browser_session_id)
            .expect("browser session")
            .external_action_grants
            .contains(&normalized_native_path(&other_cwd))
    );

    let settings = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": other_cwd })),
        },
    )
    .await
    .expect("settings/read after project start");

    assert_eq!(
        settings["project"]["path"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
}

#[tokio::test]
async fn browser_workspace_create_uses_configured_root_and_authorizes_cwd() {
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
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id.clone(),
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
    let cwd = temp
        .path()
        .join("workspaces")
        .join("Notes")
        .canonicalize()
        .expect("created cwd");
    let cwd_string = cwd.display().to_string();

    assert_eq!(created["cwd"], cwd_string);
    assert_eq!(created["scope"]["cwd"].as_str(), Some(cwd_string.as_str()));
    assert!(
        state
            .inner
            .browser_sessions
            .lock()
            .expect("sessions")
            .get(&browser_session_id)
            .expect("browser session")
            .external_action_grants
            .contains(&normalized_native_path(&cwd))
    );

    let unrestricted_parent = temp.path().join("outside-configured-root");
    std::fs::create_dir_all(&unrestricted_parent).expect("unrestricted parent");
    let unrestricted_parent = unrestricted_parent
        .canonicalize()
        .expect("canonical unrestricted parent");
    let outside_created = handle_rpc(
        state.clone(),
        auth.clone(),
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "workspace/create".to_string(),
            params: Some(json!({
                "name": "Existing-parent-child",
                "parent": unrestricted_parent
            })),
        },
    )
    .await
    .expect("workspace/create beneath an explicit unrestricted parent");
    let outside_cwd = unrestricted_parent
        .join("Existing-parent-child")
        .canonicalize()
        .expect("created outside cwd");
    assert_eq!(outside_created["cwd"].as_str(), Some(outside_cwd.to_string_lossy().as_ref()));

    let missing_parent = temp.path().join("missing-parent");
    let missing_error = handle_rpc(
        state.clone(),
        auth.clone(),
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "workspace/create".to_string(),
            params: Some(json!({ "name": "child", "parent": missing_parent })),
        },
    )
    .await
    .expect_err("an explicit parent must already exist");
    assert!(!missing_error.to_string().is_empty());
    assert!(!missing_parent.exists());

    let settings = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": cwd_string.clone() })),
        },
    )
    .await
    .expect("settings/read after workspace/create");

    assert_eq!(settings["cwd"].as_str(), Some(cwd_string.as_str()));
    assert_eq!(
        settings["project"]["path"].as_str(),
        Some(cwd_string.as_str())
    );
}
