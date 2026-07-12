#[test]
fn thread_snapshot_projects_visible_entries_for_history_session_with_messages() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
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

#[tokio::test]
async fn thread_history_read_pages_the_authoritative_projection_by_entry_id() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    for (timestamp_ms, text) in [(1, "first"), (2, "second")] {
        state
            .inner
            .state
            .store()
            .append_message(
                &session_id,
                &RuntimeMessage::User {
                    content: vec![UserContentBlock::text(text)],
                    timestamp_ms,
                },
            )
            .expect("append history message");
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-first")),
            method: "thread/history/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "limit": 1
            })),
        },
    )
    .await
    .expect("first history page");
    assert_eq!(first["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(first["entries"][0]["blocks"][0]["body"], "first");
    let cursor = first["nextCursor"]
        .as_str()
        .expect("opaque stable entry cursor")
        .to_string();
    assert_eq!(first["history"]["cursor"], cursor);

    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-second")),
            method: "thread/history/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "cursor": cursor,
                "limit": 1
            })),
        },
    )
    .await
    .expect("second history page");
    assert_eq!(second["entries"][0]["blocks"][0]["body"], "second");
    assert_eq!(second["nextCursor"], Value::Null);

    let unknown = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-unknown")),
            method: "thread/history/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "cursor": "missing-entry"
            })),
        },
    )
    .await
    .expect_err("unknown cursor fails closed");
    assert!(unknown.to_string().contains("cursor"), "{unknown}");
}

#[test]
fn thread_snapshot_replays_running_exec_live_overlay() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    store
        .append_message(
            &session_id,
            &RuntimeMessage::Assistant {
                content: vec![psychevo_runtime::AssistantBlock::ToolCall(
                    psychevo_runtime::ToolCallBlock {
                        id: "call_exec".to_string(),
                        name: "exec_command".to_string(),
                        arguments: json!({"cmd": "python fetch.py"}),
                        arguments_json: "{\"cmd\":\"python fetch.py\"}".to_string(),
                        arguments_error: None,
                        content_index: 0,
                        call_index: 0,
                    },
                )],
                timestamp_ms: 10,
                finish_reason: Some("tool_calls".to_string()),
                outcome: psychevo_runtime::Outcome::Normal,
                model: Some("fake-model".to_string()),
                provider: Some("fake-provider".to_string()),
            },
        )
        .expect("append assistant tool call");
    store
        .append_message(
            &session_id,
            &RuntimeMessage::ToolResult {
                tool_call_id: "call_exec".to_string(),
                tool_name: "exec_command".to_string(),
                content: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\n\"}"
                    .to_string(),
                is_error: false,
                timestamp_ms: 20,
            },
        )
        .expect("append yielded exec result");

    let turn_id = "turn-running";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("claim running activity");

    append_exec_live_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "first\nsecond\n",
    );
    append_exec_live_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "first\nsecond\npoll\n",
    );

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");
    assert_eq!(
        snapshot["activity"]["startedAtMs"],
        json!(activity.started_at_ms),
        "{snapshot:#}"
    );
    let entries = snapshot["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1, "{snapshot:#}");
    let exec_blocks = entries
        .iter()
        .flat_map(|entry| entry["blocks"].as_array().into_iter().flatten())
        .filter(|block| block["metadata"]["tool_name"] == "exec_command")
        .collect::<Vec<_>>();
    assert_eq!(exec_blocks.len(), 1, "{snapshot:#}");
    let exec = exec_blocks[0];
    assert_eq!(exec["status"], "running");
    assert_eq!(
        exec["metadata"]["result"]["output"],
        "first\nsecond\npoll\n"
    );
    assert_eq!(exec["metadata"]["result"]["session_id"], 7);
}

#[test]
fn thread_snapshot_does_not_downgrade_completed_tool_with_stale_live_overlay() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    let command = "sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id, title FROM stories;\"";
    store
        .append_message(
            &session_id,
            &RuntimeMessage::Assistant {
                content: vec![psychevo_runtime::AssistantBlock::ToolCall(
                    psychevo_runtime::ToolCallBlock {
                        id: "call_exec".to_string(),
                        name: "exec_command".to_string(),
                        arguments: json!({"cmd": command}),
                        arguments_json: json!({"cmd": command}).to_string(),
                        arguments_error: None,
                        content_index: 0,
                        call_index: 0,
                    },
                )],
                timestamp_ms: 10,
                finish_reason: Some("tool_calls".to_string()),
                outcome: psychevo_runtime::Outcome::Normal,
                model: Some("fake-model".to_string()),
                provider: Some("fake-provider".to_string()),
            },
        )
        .expect("append assistant tool call");
    store
        .append_message(
            &session_id,
            &RuntimeMessage::ToolResult {
                tool_call_id: "call_exec".to_string(),
                tool_name: "exec_command".to_string(),
                content: "{\"exit_code\":0,\"output\":\"story one\\n\"}".to_string(),
                is_error: false,
                timestamp_ms: 20,
            },
        )
        .expect("append completed exec result");

    let turn_id = "turn-running";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .expect("claim running activity");

    append_stale_exec_live_snapshot(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        TranscriptBlockStatus::Running,
    );

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries");
    let exec = entries
        .iter()
        .flat_map(|entry| entry["blocks"].as_array().into_iter().flatten())
        .find(|block| block["metadata"]["tool_call_id"] == "call_exec")
        .expect("exec block");

    assert_eq!(exec["status"], "completed", "{snapshot:#}");
    assert_eq!(exec["title"], format!("exec_command {command}"));
    assert_eq!(exec["metadata"]["args"]["cmd"], command);
    assert_eq!(exec["metadata"]["result"]["output"], "story one\n");
    assert_eq!(
        exec["body"],
        "{\"exit_code\":0,\"output\":\"story one\\n\"}"
    );
}

#[test]
fn thread_snapshot_does_not_replay_live_text_for_committed_active_owner() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    store
        .append_message(
            &session_id,
            &RuntimeMessage::Assistant {
                content: vec![psychevo_runtime::AssistantBlock::Text {
                    text: "Committed **answer**.".to_string(),
                }],
                timestamp_ms: 10,
                finish_reason: Some("stop".to_string()),
                outcome: psychevo_runtime::Outcome::Normal,
                model: Some("fake-model".to_string()),
                provider: Some("fake-provider".to_string()),
            },
        )
        .expect("append committed assistant");

    let turn_id = "turn-running";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .expect("claim running activity");

    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "Committed answer.",
    );

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id)).expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1, "{snapshot:#}");
    assert_eq!(entries[0]["source"], "runtime.message");
    assert_eq!(entries[0]["turnId"], turn_id);
    assert_eq!(entries[0]["metadata"]["liveOrder"], 0);
    assert_eq!(entries[0]["blocks"][0]["body"], "Committed **answer**.");
}

#[test]
fn thread_snapshot_stamps_committed_prefix_after_scoped_child_turn_started() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let parent_session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("parent session");
    let child_session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "agent",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("child session");
    store
        .append_message(
            &parent_session_id,
            &RuntimeMessage::Assistant {
                content: vec![psychevo_runtime::AssistantBlock::Text {
                    text: "Committed **prefix**.".to_string(),
                }],
                timestamp_ms: 10,
                finish_reason: Some("tool_calls".to_string()),
                outcome: psychevo_runtime::Outcome::Normal,
                model: Some("fake-model".to_string()),
                provider: Some("fake-provider".to_string()),
            },
        )
        .expect("append committed assistant");

    let turn_id = "turn-running";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&parent_session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .expect("claim running activity");
    store
        .update_gateway_activity_thread(
            &activity.activity_id,
            &activity.owner_id,
            activity.generation,
            &child_session_id,
            gateway_now_ms() + 30_000,
        )
        .expect("scoped child turn started");

    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &parent_session_id,
        turn_id,
        "Committed prefix.",
    );

    let snapshot = thread_snapshot(&state, &scope, Some(&parent_session_id)).expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries");
    assert_eq!(snapshot["activity"]["running"], true, "{snapshot:#}");
    assert_eq!(entries.len(), 1, "{snapshot:#}");
    assert_eq!(entries[0]["source"], "runtime.message");
    assert_eq!(entries[0]["turnId"], turn_id);
    assert_eq!(entries[0]["metadata"]["liveOrder"], 0);
    assert_eq!(entries[0]["blocks"][0]["body"], "Committed **prefix**.");
}

#[test]
fn thread_snapshot_replays_open_child_overlay_from_running_parent_activity() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let parent_session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("parent session");
    let child_session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "agent",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent_session_id,
            &child_session_id,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("open child edge");

    let turn_id = "turn-parent-running";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&parent_session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("claim parent activity");
    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &child_session_id,
        turn_id,
        "Streaming child answer.",
    );

    let child_snapshot =
        thread_snapshot(&state, &scope, Some(&child_session_id)).expect("child snapshot");
    assert_eq!(
        child_snapshot["activity"]["running"], true,
        "{child_snapshot:#}"
    );
    assert_eq!(
        child_snapshot["activity"]["activeTurnId"], turn_id,
        "{child_snapshot:#}"
    );
    let child_entries = child_snapshot["entries"].as_array().expect("child entries");
    assert_eq!(child_entries.len(), 1, "{child_snapshot:#}");
    assert_eq!(
        child_entries[0]["blocks"][0]["body"],
        "Streaming child answer."
    );

    let parent_snapshot =
        thread_snapshot(&state, &scope, Some(&parent_session_id)).expect("parent snapshot");
    assert_eq!(
        parent_snapshot["activity"]["running"], true,
        "{parent_snapshot:#}"
    );
    assert_eq!(
        parent_snapshot["activity"]["activeTurnId"], turn_id,
        "{parent_snapshot:#}"
    );

    store
        .set_agent_edge_status(&child_session_id, psychevo_runtime::AgentEdgeStatus::Closed)
        .expect("close child edge");
    let closed_child_snapshot =
        thread_snapshot(&state, &scope, Some(&child_session_id)).expect("closed child snapshot");
    assert_eq!(
        closed_child_snapshot["activity"]["running"], false,
        "{closed_child_snapshot:#}"
    );
    assert_eq!(
        closed_child_snapshot["entries"],
        json!([]),
        "{closed_child_snapshot:#}"
    );
}

#[test]
fn thread_snapshot_does_not_revive_child_overlay_from_stale_or_terminal_parent_activity() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let parent_session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("parent session");
    let child_session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "agent",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent_session_id,
            &child_session_id,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("open child edge");

    let turn_id = "turn-parent-stale";
    let activity = store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&parent_session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() - 1,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("claim stale parent activity");
    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &child_session_id,
        turn_id,
        "Stale child answer.",
    );

    let stale_child_snapshot =
        thread_snapshot(&state, &scope, Some(&child_session_id)).expect("stale child snapshot");
    assert_eq!(
        stale_child_snapshot["activity"]["running"], false,
        "{stale_child_snapshot:#}"
    );
    assert_eq!(
        stale_child_snapshot["entries"],
        json!([]),
        "{stale_child_snapshot:#}"
    );

    store
        .finish_gateway_activity(
            &activity.activity_id,
            &activity.owner_id,
            activity.generation,
            "completed",
        )
        .expect("finish parent activity");
    let terminal_child_snapshot =
        thread_snapshot(&state, &scope, Some(&child_session_id)).expect("terminal child snapshot");
    assert_eq!(
        terminal_child_snapshot["activity"]["running"], false,
        "{terminal_child_snapshot:#}"
    );
    assert_eq!(
        terminal_child_snapshot["entries"],
        json!([]),
        "{terminal_child_snapshot:#}"
    );
}

#[test]
fn acp_bound_child_snapshot_does_not_inherit_parent_activity_without_child_activity() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let parent_session_id = store
        .create_session_with_metadata(&state.inner.cwd, "web", "model", "provider", None)
        .expect("parent session");
    let child_session_id = store
        .create_child_session_with_metadata(
            &parent_session_id,
            &state.inner.cwd,
            "peer_agent",
            "opencode",
            "acp:opencode",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent_session_id,
            &child_session_id,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("open child edge");
    let profile = RuntimeProfileConfig {
        id: "acp:opencode".to_string(),
        runtime: RuntimeProfileKind::Acp,
        enabled: true,
        label: "OpenCode (ACP)".to_string(),
        backend_ref: Some("opencode".to_string()),
        default_model: None,
        default_mode: None,
        default_agent: None,
        approval_mode: None,
        sandbox: None,
        workspace_roots: Vec::new(),
        options: Value::Null,
    };
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_json = r#"{"name":"opencode"}"#;
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint(agent_json);
    let cwd = state.inner.cwd.display().to_string();
    store
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &child_session_id,
            agent_ref: Some("opencode"),
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: agent_json,
            runtime_ref: "acp:opencode",
            backend_kind: "acp",
            native_kind: "acp",
            native_session_id: Some("native-child"),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "acp",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: Some(&parent_session_id),
        })
        .expect("ACP child binding");
    store
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "turn-parent-running",
            thread_id: Some(&parent_session_id),
            source_key: None,
            turn_id: Some("turn-parent-running"),
            kind: "turn",
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("parent activity");

    let child_snapshot =
        thread_snapshot(&state, &scope, Some(&child_session_id)).expect("child snapshot");
    assert_eq!(child_snapshot["activity"]["running"], false, "{child_snapshot:#}");
    assert_eq!(child_snapshot["activity"]["activeTurnId"], Value::Null);
}

fn append_exec_live_update(
    state: &WebState,
    activity_id: &str,
    session_id: &str,
    turn_id: &str,
    output: &str,
) {
    let entry = TranscriptEntry {
        id: format!("live:{turn_id}:assistant:0"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Running,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("live:{turn_id}:tool:call_exec"),
            kind: TranscriptBlockKind::Shell,
            status: TranscriptBlockStatus::Running,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: Some("exec_command python fetch.py".to_string()),
            body: Some(
                json!({
                    "session_id": 7,
                    "exit_code": null,
                    "output": output,
                })
                .to_string(),
            ),
            preview: Some(output.to_string()),
            detail: Some(
                json!({
                    "session_id": 7,
                    "exit_code": null,
                    "output": output,
                })
                .to_string(),
            ),
            artifact_ids: Vec::new(),
            metadata: Some(json!({
                "projection": "tool",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "python fetch.py"},
                "result": {
                    "session_id": 7,
                    "exit_code": null,
                    "output": output,
                },
            })),
            result: None,
            created_at_ms: 30,
            updated_at_ms: 40,
        }],
        metadata: Some(json!({"streamSeq": 1, "liveOrder": 0})),
        usage: None,
        accounting: None,
        created_at_ms: 30,
        updated_at_ms: 40,
    };
    let event = GatewayEvent::EntryUpdated {
        turn_id: turn_id.to_string(),
        entry,
    };
    state
        .inner
        .state
        .store()
        .upsert_gateway_live_snapshot(psychevo_runtime::GatewayLiveSnapshotInput {
            snapshot_key: &format!("{activity_id}:{turn_id}:live-tool"),
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        })
        .expect("upsert live snapshot");
}

fn append_stale_exec_live_snapshot(
    state: &WebState,
    activity_id: &str,
    session_id: &str,
    turn_id: &str,
    status: TranscriptBlockStatus,
) {
    let entry = TranscriptEntry {
        id: format!("live:{turn_id}:assistant:0"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("live:{turn_id}:tool:call_exec"),
            kind: TranscriptBlockKind::Shell,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: Some("exec_command".to_string()),
            body: None,
            preview: None,
            detail: None,
            artifact_ids: Vec::new(),
            metadata: Some(json!({
                "projection": "tool",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec"
            })),
            result: None,
            created_at_ms: 30,
            updated_at_ms: 40,
        }],
        metadata: Some(json!({"streamSeq": 1, "liveOrder": 0})),
        usage: None,
        accounting: None,
        created_at_ms: 30,
        updated_at_ms: 40,
    };
    let event = GatewayEvent::EntryUpdated {
        turn_id: turn_id.to_string(),
        entry,
    };
    state
        .inner
        .state
        .store()
        .upsert_gateway_live_snapshot(psychevo_runtime::GatewayLiveSnapshotInput {
            snapshot_key: &format!("{activity_id}:{turn_id}:stale-live-tool"),
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        })
        .expect("upsert stale live snapshot");
}

fn append_assistant_live_text_update(
    state: &WebState,
    activity_id: &str,
    session_id: &str,
    turn_id: &str,
    text: &str,
) {
    let entry = TranscriptEntry {
        id: format!("live:{turn_id}:assistant:0"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("live:{turn_id}:assistant:0:text"),
            kind: TranscriptBlockKind::Text,
            status: TranscriptBlockStatus::Completed,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: None,
            body: Some(text.to_string()),
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: Some(json!({"projection": "assistant_segment"})),
            result: None,
            created_at_ms: 30,
            updated_at_ms: 40,
        }],
        metadata: Some(json!({"projection": "assistant_segment", "streamSeq": 1, "liveOrder": 0})),
        usage: None,
        accounting: None,
        created_at_ms: 30,
        updated_at_ms: 40,
    };
    let event = GatewayEvent::EntryUpdated {
        turn_id: turn_id.to_string(),
        entry,
    };
    state
        .inner
        .state
        .store()
        .upsert_gateway_live_snapshot(psychevo_runtime::GatewayLiveSnapshotInput {
            snapshot_key: &format!("{activity_id}:{turn_id}:live-text"),
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        })
        .expect("upsert live snapshot");
}
