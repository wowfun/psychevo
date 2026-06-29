#[tokio::test]
async fn automation_rpc_create_list_and_delete_round_trips() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "Daily review",
                "prompt": "Summarize the current repository state.",
                "schedule": { "kind": "interval", "everyMinutes": 30 }
            })),
        },
    )
    .await
    .expect("automation/write");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();
    assert_eq!(created["automation"]["kind"], "project");
    assert_eq!(created["automation"]["execution"]["policy"], "autoSandbox");
    assert!(created["automation"]["nextRunAtMs"].is_number());
    assert_eq!(
        created["automation"]["sourceKey"].as_str(),
        Some(format!("automation:{automation_id}").as_str())
    );

    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list");
    let automations = listed["automations"].as_array().expect("automations");
    assert_eq!(automations.len(), 1);
    assert_eq!(automations[0]["id"], automation_id);

    let deleted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "automation/delete".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/delete");
    assert_eq!(deleted["deleted"], true);

    let listed = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list empty");
    assert!(
        listed["automations"]
            .as_array()
            .expect("automations")
            .is_empty()
    );
}

#[tokio::test]
async fn browser_session_can_manage_automation_for_other_cwd() {
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
    let auth = AuthContext::Browser {
        session_id: browser_session_id,
    };
    let scope = ResolvedScope {
        cwd: other_cwd.clone(),
        source: cwd_source(&other_cwd),
    }
    .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state.clone(),
        auth.clone(),
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": { "kind": "project" },
                "title": "Other cwd review",
                "prompt": "Summarize the other workspace.",
                "schedule": { "kind": "interval", "everyMinutes": 30 }
            })),
        },
    )
    .await
    .expect("automation/write other cwd");
    assert_eq!(created["automation"]["cwd"], other_cwd.display().to_string());

    let listed = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/list".to_string(),
            params: Some(json!({ "cwd": other_cwd })),
        },
    )
    .await
    .expect("automation/list other cwd");
    let automations = listed["automations"].as_array().expect("automations");
    assert_eq!(automations.len(), 1);
    assert_eq!(automations[0]["title"], "Other cwd review");
}

#[tokio::test]
async fn automation_rpc_accepts_one_shot_delay_schedule() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "One-shot review",
                "prompt": "Review the repo once soon.",
                "schedule": { "kind": "delay", "afterMinutes": 15 }
            })),
        },
    )
    .await
    .expect("automation/write");

    assert_eq!(
        created["automation"]["schedule"],
        json!({"kind": "delay", "afterMinutes": 15})
    );
    assert!(created["automation"]["nextRunAtMs"].is_number());
}

#[tokio::test]
async fn automation_rpc_pause_resume_are_explicit_lifecycle_mutations() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "Lifecycle review",
                "prompt": "Review lifecycle behavior.",
                "schedule": { "kind": "interval", "everyMinutes": 30 }
            })),
        },
    )
    .await
    .expect("automation/write");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();

    let paused = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/pause".to_string(),
            params: Some(json!({ "automationId": automation_id.clone() })),
        },
    )
    .await
    .expect("automation/pause");
    assert_eq!(paused["automation"]["enabled"], false);
    assert!(paused["automation"]["nextRunAtMs"].is_null());

    let updated = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "automationId": automation_id.clone(),
                "target": { "kind": "project" },
                "title": "Updated lifecycle review",
                "prompt": "Review updated lifecycle behavior.",
                "schedule": { "kind": "interval", "everyMinutes": 45 }
            })),
        },
    )
    .await
    .expect("automation/write update");
    assert_eq!(updated["automation"]["title"], "Updated lifecycle review");
    assert_eq!(updated["automation"]["enabled"], false);
    assert!(updated["automation"]["nextRunAtMs"].is_null());

    let resumed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "automation/resume".to_string(),
            params: Some(json!({ "automationId": automation_id.clone() })),
        },
    )
    .await
    .expect("automation/resume");
    assert_eq!(resumed["automation"]["enabled"], true);
    assert!(resumed["automation"]["nextRunAtMs"].is_number());

    let missing = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(5)),
            method: "automation/pause".to_string(),
            params: Some(json!({ "automationId": "missing" })),
        },
    )
    .await;
    assert!(missing.is_err());
}
