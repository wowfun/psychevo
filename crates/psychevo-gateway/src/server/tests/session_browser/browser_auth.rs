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
            BrowserSession {
                cwd: state.inner.cwd.clone(),
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
            params: Some(json!({ "cwd": other_cwd })),
        },
    )
    .await
    .expect("settings/read after cross-project resume");

    assert_eq!(
        settings["project"]["path"],
        other_cwd.display().to_string()
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
            BrowserSession {
                cwd: state.inner.cwd.clone(),
                source: state.inner.source.clone(),
            },
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
        settings["project"]["path"],
        other_cwd.display().to_string()
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
            BrowserSession {
                cwd: state.inner.cwd.clone(),
                source: state.inner.source.clone(),
            },
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id,
    };
    let scope = ResolvedScope {
        cwd: other_cwd.clone(),
        source: cwd_source(&other_cwd),
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
        snapshot["scope"]["cwd"],
        other_cwd.display().to_string()
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
        settings["project"]["path"],
        other_cwd.display().to_string()
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
            BrowserSession {
                cwd: state.inner.cwd.clone(),
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
    let cwd = temp
        .path()
        .join("workspaces")
        .join("Notes")
        .canonicalize()
        .expect("created cwd");
    let cwd_string = cwd.display().to_string();

    assert_eq!(created["cwd"], cwd_string);
    assert_eq!(created["scope"]["cwd"], cwd_string);

    let settings = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": cwd_string.clone() })),
        },
    )
    .await
    .expect("settings/read after workspace/create");

    assert_eq!(settings["cwd"], cwd_string);
    assert_eq!(settings["project"]["path"], cwd_string);
}
