#[tokio::test]
async fn agent_session_import_lifecycle_is_explicit_opaque_and_capability_gated() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("agent-session-lifecycle.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    rpc_test_request(
        &state,
        &tx,
        "backend/write",
        json!({
            "id": "lifecycle-fixture",
            "target": "project",
            "enabled": true,
            "label": "Lifecycle fixture",
            "command": python,
            "args": [fixture],
            "env": {
                "ACP_LIFECYCLE_LOG": log,
                "ACP_LIFECYCLE_MODE": "all"
            },
            "entrypoints": ["peer", "subagent"]
        }),
    )
    .await;
    rpc_test_request(
        &state,
        &tx,
        "runtime/profile/write",
        json!({
            "id": "lifecycle-fixture",
            "target": "project",
            "runtime": "acp",
            "enabled": true,
            "label": "Lifecycle fixture",
            "backendRef": "lifecycle-fixture",
            "defaultAgent": "build"
        }),
    )
    .await;
    rpc_test_request(
        &state,
        &tx,
        "agent/write",
        json!({
            "scope": wire_scope.clone(),
            "name": "lifecycle-fixture",
            "target": "project",
            "description": "Lifecycle fixture Agent",
            "enabled": true,
            "instructions": "Exercise the lifecycle fixture.",
            "backend": {"ref": "lifecycle-fixture"},
            "entrypoints": ["peer"]
        }),
    )
    .await;

    let ordinary = rpc_test_request(
        &state,
        &tx,
        "thread/list",
        json!({"cwd": state.inner.cwd}),
    )
    .await;
    assert_eq!(ordinary["sessions"], json!([]));
    assert!(!log.exists(), "ordinary Session reads must not initialize ACP");

    let listed = rpc_test_request(
        &state,
        &tx,
        "thread/import/list",
        json!({"scope": wire_scope.clone(), "cursors": {}}),
    )
    .await;
    let profile = listed["profiles"]
        .as_array()
        .expect("profile results")
        .iter()
        .find(|profile| profile["runtimeProfileRef"] == "lifecycle-fixture")
        .expect("lifecycle profile result");
    assert_eq!(profile["status"], "ready");
    let profile_targets = profile["targets"].as_array().expect("profile targets");
    assert!(
        profile_targets
            .iter()
            .all(|target| target["agentRef"] != "build"),
        "a Runtime Profile default must not synthesize a missing Agent Definition"
    );
    let candidate_id = profile["sessions"][0]["candidateId"]
        .as_str()
        .expect("opaque candidate")
        .to_string();
    let target_id = profile_targets
        .iter()
        .find(|target| target["ready"] == true)
        .expect("ready import target")["targetId"]
        .as_str()
        .expect("opaque target")
        .to_string();
    assert!(candidate_id.starts_with("candidate:"));
    assert!(!candidate_id.contains("listed-native"));
    assert!(!listed.to_string().contains("listed-native"));

    let imported = rpc_test_request(
        &state,
        &tx,
        "thread/import",
        json!({
            "scope": wire_scope.clone(),
            "candidateId": candidate_id,
            "targetId": target_id
        }),
    )
    .await;
    let imported_thread_id = imported["snapshot"]["thread"]["id"]
        .as_str()
        .expect("published imported Thread")
        .to_string();
    let sessions_after_import = rpc_test_request(
        &state,
        &tx,
        "thread/list",
        json!({"cwd": state.inner.cwd}),
    )
    .await;
    let imported_summary = sessions_after_import["sessions"]
        .as_array()
        .expect("session summaries")
        .iter()
        .find(|session| session["id"] == imported_thread_id)
        .expect("published imported Thread summary");
    assert_eq!(
        imported_summary["title"],
        "Listed fixture",
        "the published Thread must preserve the Agent-owned session title"
    );
    assert_eq!(
        state
            .inner
            .state
            .store()
            .gateway_runtime_binding(&imported_thread_id)
            .expect("binding")
            .expect("imported binding")
            .native_session_id
            .as_deref(),
        Some("listed-native")
    );

    let archived = rpc_test_request(
        &state,
        &tx,
        "thread/archive",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert!(archived["session"]["archivedAtMs"].is_number());
    let restored = rpc_test_request(
        &state,
        &tx,
        "thread/restore",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert!(restored["session"]["archivedAtMs"].is_null());

    let forked = rpc_test_request(
        &state,
        &tx,
        "thread/action/run",
        json!({
            "scope": wire_scope.clone(),
            "threadId": imported_thread_id,
            "action": {"kind": "fork"}
        }),
    )
    .await;
    assert_eq!(forked["kind"], "fork");
    let forked_thread_id = forked["snapshot"]["thread"]["id"]
        .as_str()
        .expect("forked Thread")
        .to_string();
    assert_ne!(forked_thread_id, imported_thread_id);

    let deleted = rpc_test_request(
        &state,
        &tx,
        "thread/delete",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert_eq!(deleted["deleted"], true);
    assert!(
        state
            .inner
            .state
            .store()
            .session_summary(&imported_thread_id)
            .expect("deleted lookup")
            .is_none()
    );

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown lifecycle fixture");
    let methods = std::fs::read_to_string(log)
        .expect("lifecycle log")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| entry["method"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    for expected in [
        "session/list",
        "session/resume",
        "session/close",
        "session/fork",
        "session/delete",
    ] {
        assert!(methods.iter().any(|method| method == expected), "missing {expected}: {methods:?}");
    }
}

#[tokio::test]
async fn agent_session_delete_requires_remote_capability_and_acknowledgement() {
    for (mode, expected_enabled, expected_error) in [
        ("no-delete", false, "does not support deleting"),
        ("delete-fails", true, "fixture remote delete failed"),
    ] {
        let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let host_cwd = std::env::current_dir().expect("host cwd");
        let python = ["python3", "python"]
            .into_iter()
            .find_map(|command| {
                resolve_executable_path(
                    command,
                    &host_cwd,
                    &ExecutableResolveOptions {
                        platform: HostPlatform::current(),
                        env: &host_env,
                    },
                )
            })
            .expect("Python is required by the ACP fixture");
        let (_temp, state) = web_state();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake_acp_lifecycle.py");
        let log = state.inner.cwd.join(format!("agent-session-delete-{mode}.jsonl"));
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let wire_scope = scope.to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        rpc_test_request(
            &state,
            &tx,
            "backend/write",
            json!({
                "id": "lifecycle-fixture",
                "target": "project",
                "enabled": true,
                "label": "Lifecycle fixture",
                "command": python,
                "args": [fixture],
                "env": {"ACP_LIFECYCLE_LOG": log, "ACP_LIFECYCLE_MODE": mode},
                "entrypoints": ["peer", "subagent"]
            }),
        )
        .await;
        rpc_test_request(
            &state,
            &tx,
            "runtime/profile/write",
            json!({
                "id": "lifecycle-fixture",
                "target": "project",
                "runtime": "acp",
                "enabled": true,
                "label": "Lifecycle fixture",
                "backendRef": "lifecycle-fixture",
                "defaultAgent": "lifecycle-fixture"
            }),
        )
        .await;
        rpc_test_request(
            &state,
            &tx,
            "agent/write",
            json!({
                "scope": wire_scope.clone(),
                "name": "lifecycle-fixture",
                "target": "project",
                "description": "Lifecycle fixture Agent",
                "enabled": true,
                "instructions": "Exercise the lifecycle fixture.",
                "backend": {"ref": "lifecycle-fixture"},
                "entrypoints": ["peer"]
            }),
        )
        .await;

        let listed = rpc_test_request(
            &state,
            &tx,
            "thread/import/list",
            json!({"scope": wire_scope.clone(), "cursors": {}}),
        )
        .await;
        let profile = listed["profiles"]
            .as_array()
            .expect("profile results")
            .iter()
            .find(|profile| profile["runtimeProfileRef"] == "lifecycle-fixture")
            .expect("lifecycle profile result");
        let imported = rpc_test_request(
            &state,
            &tx,
            "thread/import",
            json!({
                "scope": wire_scope,
                "candidateId": profile["sessions"][0]["candidateId"],
                "targetId": profile["targets"][0]["targetId"]
            }),
        )
        .await;
        let thread_id = imported["snapshot"]["thread"]["id"]
            .as_str()
            .expect("imported Thread")
            .to_string();
        let sessions = rpc_test_request(
            &state,
            &tx,
            "thread/list",
            json!({"cwd": state.inner.cwd}),
        )
        .await;
        let summary = sessions["sessions"]
            .as_array()
            .expect("session summaries")
            .iter()
            .find(|session| session["id"] == thread_id)
            .expect("imported summary");
        let delete = summary["lifecycle"]["actions"]
            .as_array()
            .expect("lifecycle actions")
            .iter()
            .find(|action| action["id"] == "delete")
            .expect("delete action");
        assert_eq!(delete["enabled"], expected_enabled);
        rpc_test_request(
            &state,
            &tx,
            "source/reset",
            json!({"scope": scope.to_wire_scope()}),
        )
        .await;

        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(format!("delete-{mode}"))),
                method: "thread/delete".to_string(),
                params: Some(json!({"threadId": thread_id})),
            },
        )
        .await
        .expect_err("delete must fail without remote capability or acknowledgement");
        assert!(
            error.to_string().contains(expected_error),
            "unexpected {mode} delete error: {error}"
        );
        assert!(
            state
                .inner
                .state
                .store()
                .session_summary(&thread_id)
                .expect("local Thread lookup")
                .is_some(),
            "{mode} must preserve local history when remote deletion is not acknowledged"
        );
        state
            .inner
            .gateway
            .shutdown_runtimes(false)
            .await
            .expect("shutdown lifecycle fixture");
    }
}

async fn rpc_test_request(
    state: &WebState,
    tx: &mpsc::UnboundedSender<String>,
    method: &str,
    params: Value,
) -> Value {
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(format!("test-{method}"))),
            method: method.to_string(),
            params: Some(params),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("{method} failed: {error}"))
}
