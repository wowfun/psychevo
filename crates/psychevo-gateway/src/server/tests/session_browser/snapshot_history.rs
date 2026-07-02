#[test]
fn thread_snapshot_projects_visible_entries_for_history_session_with_messages() {
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
fn thread_snapshot_replays_running_exec_live_overlay() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
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
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
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
    assert_eq!(exec["body"], "{\"exit_code\":0,\"output\":\"story one\\n\"}");
}

#[test]
fn thread_snapshot_does_not_replay_live_text_for_committed_active_owner() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let session_id = store
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
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
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
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
