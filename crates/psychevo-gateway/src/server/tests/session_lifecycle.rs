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
            "targetId": target_id,
            "archived": true
        }),
    )
    .await;
    let imported_thread_id = imported["snapshot"]["thread"]["id"]
        .as_str()
        .expect("published imported Thread")
        .to_string();
    let imported_entries = imported["snapshot"]["entries"]
        .as_array()
        .expect("imported transcript entries");
    assert_eq!(imported["snapshot"]["history"]["owner"], "agent");
    assert_eq!(imported["snapshot"]["history"]["fidelity"], "full");
    assert_eq!(imported_entries.len(), 3, "{imported:#}");
    assert_eq!(imported_entries[0]["role"], "user");
    assert_eq!(imported_entries[0]["blocks"][0]["body"], "Imported user question");
    assert_eq!(imported_entries[1]["role"], "assistant");
    assert_eq!(imported_entries[1]["blocks"][0]["kind"], "reasoning");
    assert_eq!(imported_entries[1]["blocks"][0]["body"], "Imported reasoning");
    assert_eq!(
        imported_entries[1]["blocks"][1]["body"],
        "Imported assistant answer"
    );
    assert_eq!(imported_entries[1]["blocks"][2]["kind"], "shell");
    assert_eq!(imported_entries[1]["blocks"][2]["status"], "completed");
    assert_eq!(
        imported_entries[1]["blocks"][2]["metadata"]["result"]["output"],
        "imported tool output\n"
    );
    assert_eq!(
        imported_entries[2]["blocks"][0]["kind"],
        "status",
        "{imported:#}"
    );
    assert_eq!(imported_entries[2]["blocks"][0]["title"], "Plan");
    assert_eq!(
        imported_entries[2]["blocks"][0]["body"],
        "- [x] Verify imported replay"
    );
    let reread = rpc_test_request(
        &state,
        &tx,
        "thread/read",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert_eq!(reread["entries"], imported["snapshot"]["entries"]);
    let sessions_after_import = rpc_test_request(
        &state,
        &tx,
        "thread/list",
        json!({"cwd": state.inner.cwd, "archived": true}),
    )
    .await;
    let imported_summary = sessions_after_import["sessions"]
        .as_array()
        .expect("session summaries")
        .iter()
        .find(|session| session["id"] == imported_thread_id)
        .expect("published imported Thread summary");
    assert!(imported_summary["archivedAtMs"].is_number());
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

    let restored = rpc_test_request(
        &state,
        &tx,
        "thread/restore",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert!(restored["session"]["archivedAtMs"].is_null());
    let archived = rpc_test_request(
        &state,
        &tx,
        "thread/archive",
        json!({"threadId": imported_thread_id}),
    )
    .await;
    assert!(archived["session"]["archivedAtMs"].is_number());
    rpc_test_request(
        &state,
        &tx,
        "thread/restore",
        json!({"threadId": imported_thread_id}),
    )
    .await;

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
        "session/load",
        "session/close",
        "session/fork",
        "session/delete",
    ] {
        assert!(methods.iter().any(|method| method == expected), "missing {expected}: {methods:?}");
    }
    let load_index = methods
        .iter()
        .position(|method| method == "session/load")
        .expect("import load request");
    let close_index = methods
        .iter()
        .position(|method| method == "session/close")
        .expect("archive close request");
    assert!(
        methods[..close_index]
            .iter()
            .all(|method| method != "session/resume"),
        "import must use session/load; session/resume is reserved for restore: {methods:?}"
    );
    assert!(load_index < close_index, "import load must precede lifecycle restore: {methods:?}");
}

#[tokio::test]
async fn agent_session_import_surfaces_partial_ordered_replacement_history_and_reloads_once() {
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
    let log = state.inner.cwd.join("agent-session-history-review.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();

    rpc_test_request(
        &state,
        &tx,
        "backend/write",
        json!({
            "id": "history-review-fixture",
            "target": "project",
            "enabled": true,
            "label": "History review fixture",
            "command": python,
            "args": [fixture],
            "env": {
                "ACP_LIFECYCLE_LOG": log,
                "ACP_LIFECYCLE_MODE": "history-replay-review"
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
            "id": "history-review-fixture",
            "target": "project",
            "runtime": "acp",
            "enabled": true,
            "label": "History review fixture",
            "backendRef": "history-review-fixture"
        }),
    )
    .await;
    rpc_test_request(
        &state,
        &tx,
        "agent/write",
        json!({
            "scope": wire_scope.clone(),
            "name": "history-review-fixture",
            "target": "project",
            "description": "History review fixture Agent",
            "enabled": true,
            "instructions": "Exercise history replay review cases.",
            "backend": {"ref": "history-review-fixture"},
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
        .find(|profile| profile["runtimeProfileRef"] == "history-review-fixture")
        .expect("history review profile");
    let imported = rpc_test_request(
        &state,
        &tx,
        "thread/import",
        json!({
            "scope": wire_scope.clone(),
            "candidateId": profile["sessions"][0]["candidateId"],
            "targetId": profile["targets"][0]["targetId"]
        }),
    )
    .await;
    let thread_id = imported["snapshot"]["thread"]["id"]
        .as_str()
        .expect("imported Thread")
        .to_string();
    assert_eq!(imported["snapshot"]["history"]["fidelity"], "partial");
    assert!(
        imported["snapshot"]["history"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("incomplete")),
        "{imported:#}"
    );
    let entries = imported["snapshot"]["entries"]
        .as_array()
        .expect("imported entries");
    assert_eq!(entries.len(), 5, "{imported:#}");
    assert_eq!(entries[0]["blocks"][0]["body"], "Reliable imported question");
    assert_eq!(
        entries[1]["blocks"][0]["body"],
        "Unidentified imported question"
    );
    assert_eq!(entries[1]["metadata"]["acp"]["messageIds"], json!([]));
    assert_eq!(
        entries[1]["metadata"]["acp"]["replayId"],
        "anonymous:user:1"
    );
    let assistant_blocks = entries[2]["blocks"].as_array().expect("assistant blocks");
    assert_eq!(
        assistant_blocks
            .iter()
            .map(|block| block["kind"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["text", "shell", "text"]
    );
    assert_eq!(assistant_blocks[0]["body"], "Before tool");
    assert_eq!(
        assistant_blocks[1]["metadata"]["result"]["output"],
        "ordered tool output\n"
    );
    assert_eq!(assistant_blocks[2]["body"], "After tool");
    assert_eq!(
        entries[3]["blocks"][0]["body"],
        "Unidentified imported answer"
    );
    assert_eq!(entries[3]["metadata"]["acp"]["messageIds"], json!([]));
    assert_eq!(
        entries[3]["metadata"]["acp"]["replayId"],
        "anonymous:assistant:2"
    );
    assert_eq!(entries[4]["blocks"][0]["title"], "Plan");
    assert_eq!(entries[4]["blocks"][0]["body"], "- [x] Verify replay");
    assert!(!imported.to_string().contains("Inspect replay"));
    assert!(!imported.to_string().contains("Implement replay"));

    let reread = rpc_test_request(
        &state,
        &tx,
        "thread/read",
        json!({"threadId": thread_id}),
    )
    .await;
    assert_eq!(reread["entries"], imported["snapshot"]["entries"]);

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("restart resident ACP generation");
    let context = rpc_test_request(
        &state,
        &tx,
        "thread/context/read",
        json!({"scope": wire_scope.clone(), "threadId": thread_id}),
    )
    .await;
    assert_eq!(context["history"]["fidelity"], "partial");
    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-review-follow-up")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id,
                "input": [{"type": "text", "text": "continue imported history"}],
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("partial imported Thread remains writable");
    assert_eq!(accepted["accepted"], true);
    let terminal = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = rx.recv().await {
            if message.contains("\"method\":\"turn/result\"")
                || message.contains("\"method\":\"turn/error\"")
            {
                return message;
            }
        }
        String::new()
    })
    .await
    .expect("follow-up terminal");
    assert!(terminal.contains("\"method\":\"turn/result\""), "{terminal}");
    let after = rpc_test_request(
        &state,
        &mpsc::unbounded_channel().0,
        "thread/read",
        json!({"threadId": thread_id}),
    )
    .await;
    let after_entries = after["entries"].as_array().expect("entries after repeated load");
    assert_eq!(
        after_entries
            .iter()
            .filter(|entry| entry.to_string().contains("Reliable imported question"))
            .count(),
        1
    );
    assert_eq!(
        after_entries
            .iter()
            .filter(|entry| entry.to_string().contains("Unidentified imported question"))
            .count(),
        1
    );
    assert_eq!(
        after_entries
            .iter()
            .filter(|entry| entry.to_string().contains("Unidentified imported answer"))
            .count(),
        1
    );
    assert_eq!(
        after_entries
            .iter()
            .filter(|entry| entry.to_string().contains("Verify replay"))
            .count(),
        1
    );
}

#[tokio::test]
async fn agent_session_import_rejects_resume_only_history_without_publishing_a_thread() {
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
    let log = state.inner.cwd.join("agent-session-resume-only.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    rpc_test_request(
        &state,
        &tx,
        "backend/write",
        json!({
            "id": "resume-only-fixture",
            "target": "project",
            "enabled": true,
            "label": "Resume-only fixture",
            "command": python,
            "args": [fixture],
            "env": {
                "ACP_LIFECYCLE_LOG": log,
                "ACP_LIFECYCLE_MODE": "resume-only"
            },
            "entrypoints": ["peer"]
        }),
    )
    .await;
    rpc_test_request(
        &state,
        &tx,
        "runtime/profile/write",
        json!({
            "id": "resume-only-fixture",
            "target": "project",
            "runtime": "acp",
            "enabled": true,
            "label": "Resume-only fixture",
            "backendRef": "resume-only-fixture"
        }),
    )
    .await;
    rpc_test_request(
        &state,
        &tx,
        "agent/write",
        json!({
            "scope": wire_scope.clone(),
            "name": "resume-only-fixture",
            "target": "project",
            "description": "Resume-only fixture Agent",
            "enabled": true,
            "instructions": "Exercise import rejection.",
            "backend": {"ref": "resume-only-fixture"},
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
        .find(|profile| profile["runtimeProfileRef"] == "resume-only-fixture")
        .expect("resume-only profile result");
    let target_id = profile["targets"]
        .as_array()
        .expect("profile targets")
        .iter()
        .find(|target| target["ready"] == true)
        .expect("ready import target")["targetId"]
        .clone();
    let error = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("resume-only-import")),
            method: "thread/import".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "candidateId": profile["sessions"][0]["candidateId"],
                "targetId": target_id
            })),
        },
    )
    .await
    .expect_err("resume-only Agent must not publish an empty import");
    assert!(
        error.to_string().contains("does not advertise session/load"),
        "unexpected import error: {error}"
    );
    let sessions = rpc_test_request(
        &state,
        &tx,
        "thread/list",
        json!({"cwd": state.inner.cwd}),
    )
    .await;
    assert_eq!(sessions["sessions"], json!([]));
    assert!(
        state
            .inner
            .state
            .store()
            .gateway_runtime_binding_by_native_session("resume-only-fixture", "listed-native")
            .expect("binding lookup")
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
    assert!(methods.iter().any(|method| method == "session/list"));
    assert!(
        methods
            .iter()
            .all(|method| !matches!(method.as_str(), "session/load" | "session/resume")),
        "resume-only import must stop before a history lifecycle request: {methods:?}"
    );
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
