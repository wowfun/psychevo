#[tokio::test]
async fn thread_list_returns_global_top_level_sessions_without_source_partition() {
    let (temp, state) = web_state();
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let store = state.inner.state.store();
    let top_level = store
        .create_session_with_metadata(&other_cwd, "web", "fake-model", "fake-provider", None)
        .expect("top level");
    let internal = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "tui-side-conversation",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("internal");
    let child = store
        .create_child_session_with_metadata(
            &top_level,
            &other_cwd,
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
        listed["project"]["cwd"],
        other_cwd.display().to_string()
    );
    assert_eq!(listed["project"]["label"], "other-work");
    assert_eq!(listed["visibleEntryCount"], 0);
    assert!(listed.get("source").is_none());
}

#[tokio::test]
async fn thread_browser_pages_workspace_sessions_and_keeps_include_exceptions() {
    let (_temp, state) = web_state();
    let cwd = state.inner.cwd.display().to_string();
    let store = state.inner.state.store();
    let mut ids = Vec::new();
    for index in 0..25 {
        let id = store
            .create_session_with_metadata(
                &state.inner.cwd,
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
            params: Some(json!({ "cwd": cwd.clone(), "limit": 20 })),
        },
    )
    .await
    .expect("thread/browser first page");
    let workspace = &first["workspaces"][0];
    assert_eq!(
        workspace["sessions"].as_array().expect("sessions").len(),
        20
    );
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
                "cwd": cwd.clone(),
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
