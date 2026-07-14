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
    store
        .append_message(
            &top_level,
            &RuntimeMessage::User {
                content: vec![UserContentBlock::text(format!(
                    "{}   {}",
                    "fallback ".repeat(14),
                    "title"
                ))],
                timestamp_ms: gateway_now_ms(),
            },
        )
        .expect("fallback title message");
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
        listed["project"]["cwd"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
    assert_eq!(listed["project"]["label"], "other-work");
    let display_title = listed["displayTitle"].as_str().expect("display title");
    assert_eq!(display_title.chars().count(), 120);
    assert!(display_title.ends_with('…'));
    assert!(listed.get("visibleEntryCount").is_none());
    assert!(listed.get("preview").is_none());
    assert!(listed.get("source").is_none());
}

#[tokio::test]
async fn thread_browser_bounds_projection_to_returned_pages_at_large_candidate_counts() {
    let (temp, state) = web_state();
    let other_cwd = temp.path().join("large-other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let store = state.inner.state.store();
    for index in 0..2_000 {
        let cwd = if index % 2 == 0 {
            &state.inner.cwd
        } else {
            &other_cwd
        };
        store
            .create_session_with_metadata(
                cwd,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("large candidate session");
    }
    let internal = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "tui-side-conversation",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("internal session");
    let reserved = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            Some(json!({ "agentSessionImportState": { "phase": "reserved" } })),
        )
        .expect("import reserved session");
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut samples = Vec::new();
    let mut value = None;
    for request_id in 1..=20 {
        let started = std::time::Instant::now();
        value = Some(
            handle_rpc(
                state.clone(),
                AuthContext::Bearer,
                tx.clone(),
                RpcRequest {
                    jsonrpc: wire::JSONRPC_VERSION.to_string(),
                    id: Some(json!(request_id)),
                    method: "thread/browser".to_string(),
                    params: Some(json!({ "limit": 20, "recentDays": 7 })),
                },
            )
            .await
            .expect("large thread/browser page"),
        );
        samples.push(started.elapsed());
    }
    samples.sort();
    eprintln!(
        "large thread/browser projection: p50={:?}, p95={:?}",
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1]
    );
    let value = value.expect("large browser result");

    let workspaces = value["workspaces"].as_array().expect("workspaces");
    assert_eq!(workspaces.len(), 2);
    assert!(workspaces.iter().all(|workspace| {
        workspace["sessions"].as_array().is_some_and(|sessions| sessions.len() == 20)
            && workspace["hiddenCount"] == 980
            && workspace["nextCursor"]["offset"] == 20
    }));
    let ids = workspaces
        .iter()
        .flat_map(|workspace| workspace["sessions"].as_array().into_iter().flatten())
        .filter_map(|session| session["id"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(!ids.contains(internal.as_str()));
    assert!(!ids.contains(reserved.as_str()));
}

#[tokio::test]
async fn thread_browser_pages_workspace_sessions_and_keeps_include_exceptions() {
    let (_temp, state) = web_state();
    let cwd_string = state.inner.cwd.display().to_string();
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
            params: Some(json!({ "cwd": cwd_string.clone(), "limit": 20 })),
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
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "thread/browser".to_string(),
            params: Some(json!({
                "cwd": cwd_string.clone(),
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

    let running_id = ids[0].clone();
    store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "browser-running-activity",
            thread_id: Some(&running_id),
            source_key: None,
            turn_id: Some("browser-running-turn"),
            kind: "turn",
            owner_id: "other-gateway",
            owner_surface: Some("test"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("running browser activity");
    let running = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "thread/browser".to_string(),
            params: Some(json!({ "cwd": cwd_string, "limit": 1 })),
        },
    )
    .await
    .expect("thread/browser running exception");
    let running_sessions = running["workspaces"][0]["sessions"]
        .as_array()
        .expect("running sessions");
    assert_eq!(running_sessions.len(), 2);
    assert!(running_sessions.iter().any(|session| {
        session["id"] == running_id && session["activity"]["running"] == true
    }));
}
