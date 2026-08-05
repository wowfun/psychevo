use psychevo::application::{
    GatewayActivityClaimInput, GatewayActivityKind, GatewayLiveSnapshotInput,
    Message as RuntimeMessage, UserContentBlock,
};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::gateway_now_ms;
use crate::server::binding::{AuthContext, WebState};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::scope_session::default_resolved_scope;
use crate::server::session_view::thread_snapshot;
use crate::server::tests::helpers::{
    framework_message_fixture_executor, web_state_with_native_test_executor,
};
use psychevo_gateway_protocol::events_transcript::{
    GatewayEvent, TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

async fn start_thread(state: &WebState, source: &str) -> psychevo::Thread {
    let mut request = psychevo::StartThreadRequest::new(&state.inner.cwd);
    request.source = source.to_string();
    state
        .inner
        .framework
        .start_thread(request)
        .await
        .expect("thread")
}

async fn web_state_with_messages(
    messages: Vec<RuntimeMessage>,
) -> (tempfile::TempDir, WebState, psychevo::Thread) {
    let (temp, state) =
        web_state_with_native_test_executor(framework_message_fixture_executor(messages)).await;
    let thread = start_thread(&state, "web").await;
    thread
        .start_turn(psychevo::TurnRequest::new("seed transcript fixture"))
        .await
        .expect("fixture turn")
        .wait()
        .await
        .expect("fixture turn completion");
    (temp, state, thread)
}

#[tokio::test]
async fn thread_snapshot_projects_visible_entries_for_history_session_with_messages() {
    let (_temp, state, thread) = web_state_with_messages(vec![
        RuntimeMessage::User {
            content: vec![UserContentBlock::text("hello history")],
            timestamp_ms: 1,
        },
        RuntimeMessage::Assistant {
            content: vec![psychevo::application::AssistantBlock::Text {
                text: "hello from assistant".to_string(),
            }],
            timestamp_ms: 2,
            finish_reason: Some("stop".to_string()),
            outcome: psychevo::application::Outcome::Normal,
            model: Some("fake-model".to_string()),
            provider: Some("fake-provider".to_string()),
        },
    ])
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();
    let summary = state
        .inner
        .framework
        .thread_summary(&session_id)
        .await
        .expect("summary")
        .expect("session exists");
    assert!(summary.message_count > 0);

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries array");

    assert_eq!(entries.len(), 2, "{snapshot:#}");
    assert_eq!(entries[0]["blocks"][0]["body"], "hello history");
    assert_eq!(entries[1]["blocks"][0]["body"], "hello from assistant");
}

#[tokio::test]
async fn thread_history_read_pages_the_authoritative_projection_by_entry_id() {
    let (_temp, state, thread) = web_state_with_messages(
        [(1, "first"), (2, "second")]
            .into_iter()
            .map(|(timestamp_ms, text)| RuntimeMessage::User {
                content: vec![UserContentBlock::text(text)],
                timestamp_ms,
            })
            .collect(),
    )
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();
    let (tx, _rx) = mpsc::unbounded_channel();
    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
    assert_eq!(first["entries"][0]["blocks"][0]["body"], "second");
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
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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
    assert_eq!(second["entries"][0]["blocks"][0]["body"], "first");
    assert_eq!(second["nextCursor"], Value::Null);

    let unknown = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
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

#[tokio::test]
async fn thread_snapshot_and_history_read_cover_three_bounded_pages_without_overlap() {
    let (_temp, state, thread) = web_state_with_messages(
        (1..=205)
            .map(|session_seq| RuntimeMessage::User {
                content: vec![UserContentBlock::text(format!("message {session_seq}"))],
                timestamp_ms: session_seq,
            })
            .collect(),
    )
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");
    let snapshot_entries = snapshot["entries"].as_array().expect("snapshot entries");
    assert_eq!(snapshot_entries.len(), 100);
    assert_eq!(snapshot_entries[0]["messageSeq"], 106);
    assert_eq!(snapshot_entries[99]["messageSeq"], 205);
    let snapshot_cursor = snapshot["history"]["cursor"]
        .as_str()
        .expect("opaque composite cursor")
        .to_string();
    assert!(!snapshot_cursor.starts_with("message:"));

    let (tx, _rx) = mpsc::unbounded_channel();
    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-second")),
            method: "thread/history/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "cursor": snapshot_cursor,
                "limit": 100
            })),
        },
    )
    .await
    .expect("second history page");
    let second_entries = second["entries"].as_array().expect("second entries");
    assert_eq!(second_entries.len(), 100);
    assert_eq!(second_entries[0]["messageSeq"], 6);
    assert_eq!(second_entries[99]["messageSeq"], 105);
    let second_cursor = second["nextCursor"]
        .as_str()
        .expect("second opaque composite cursor")
        .to_string();

    let third = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("history-third")),
            method: "thread/history/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "cursor": second_cursor,
                "limit": 100
            })),
        },
    )
    .await
    .expect("third history page");
    let third_entries = third["entries"].as_array().expect("third entries");
    assert_eq!(third_entries.len(), 5);
    assert_eq!(third_entries[0]["messageSeq"], 1);
    assert_eq!(third_entries[4]["messageSeq"], 5);
    assert_eq!(third["nextCursor"], Value::Null);
}

#[tokio::test]
async fn thread_snapshot_replays_running_exec_live_overlay() {
    let (_temp, state, thread) = web_state_with_messages(vec![
        RuntimeMessage::Assistant {
            content: vec![psychevo::application::AssistantBlock::ToolCall(
                psychevo::application::ToolCallBlock {
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
            outcome: psychevo::application::Outcome::Normal,
            model: Some("fake-model".to_string()),
            provider: Some("fake-provider".to_string()),
        },
        RuntimeMessage::ToolResult {
            tool_call_id: "call_exec".to_string(),
            tool_name: "exec_command".to_string(),
            content: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\n\"}".to_string(),
            is_error: false,
            timestamp_ms: 20,
        },
    ])
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();

    let turn_id = "turn-running";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: GatewayActivityKind::Turn,
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("claim running activity");

    append_exec_live_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "first\nsecond\n",
    )
    .await;
    append_exec_live_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "first\nsecond\npoll\n",
    )
    .await;

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");
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

#[tokio::test]
async fn thread_snapshot_does_not_downgrade_completed_tool_with_stale_live_overlay() {
    let command = "sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id, title FROM stories;\"";
    let (_temp, state, thread) = web_state_with_messages(vec![
        RuntimeMessage::Assistant {
            content: vec![psychevo::application::AssistantBlock::ToolCall(
                psychevo::application::ToolCallBlock {
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
            outcome: psychevo::application::Outcome::Normal,
            model: Some("fake-model".to_string()),
            provider: Some("fake-provider".to_string()),
        },
        RuntimeMessage::ToolResult {
            tool_call_id: "call_exec".to_string(),
            tool_name: "exec_command".to_string(),
            content: "{\"exit_code\":0,\"output\":\"story one\\n\"}".to_string(),
            is_error: false,
            timestamp_ms: 20,
        },
    ])
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();

    let turn_id = "turn-running";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: GatewayActivityKind::Turn,
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .await
        .expect("claim running activity");

    append_stale_exec_live_snapshot(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        TranscriptBlockStatus::Running,
    )
    .await;

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");
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

#[tokio::test]
async fn thread_snapshot_does_not_replay_live_text_for_committed_active_owner() {
    let (_temp, state, thread) = web_state_with_messages(vec![RuntimeMessage::Assistant {
        content: vec![psychevo::application::AssistantBlock::Text {
            text: "Committed **answer**.".to_string(),
        }],
        timestamp_ms: 10,
        finish_reason: Some("stop".to_string()),
        outcome: psychevo::application::Outcome::Normal,
        model: Some("fake-model".to_string()),
        provider: Some("fake-provider".to_string()),
    }])
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = thread.id().to_string();

    let turn_id = "turn-running";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: GatewayActivityKind::Turn,
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .await
        .expect("claim running activity");

    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &session_id,
        turn_id,
        "Committed answer.",
    )
    .await;

    let snapshot = thread_snapshot(&state, &scope, Some(&session_id))
        .await
        .expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1, "{snapshot:#}");
    assert_eq!(entries[0]["source"], "runtime.message");
    assert_eq!(entries[0]["turnId"], turn_id);
    assert_eq!(entries[0]["metadata"]["liveOrder"], 0);
    assert_eq!(entries[0]["blocks"][0]["body"], "Committed **answer**.");
}

#[tokio::test]
async fn thread_snapshot_stamps_committed_prefix_after_scoped_child_turn_started() {
    let (_temp, state, parent_thread) = web_state_with_messages(vec![RuntimeMessage::Assistant {
        content: vec![psychevo::application::AssistantBlock::Text {
            text: "Committed **prefix**.".to_string(),
        }],
        timestamp_ms: 10,
        finish_reason: Some("tool_calls".to_string()),
        outcome: psychevo::application::Outcome::Normal,
        model: Some("fake-model".to_string()),
        provider: Some("fake-provider".to_string()),
    }])
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let parent_session_id = parent_thread.id().to_string();
    let child_session_id = start_thread(&state, "agent").await.id().to_string();

    let turn_id = "turn-running";
    let activity = state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: turn_id,
            thread_id: Some(&parent_session_id),
            source_key: None,
            turn_id: Some(turn_id),
            kind: GatewayActivityKind::Turn,
            owner_id: state.inner.gateway.owner_id(),
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: Some(json!({"kind": "turn", "firstCommittedSeq": 1})),
        })
        .await
        .expect("claim running activity");
    state
        .inner
        .durability
        .update_gateway_activity_thread(
            &activity.activity_id,
            &activity.owner_id,
            activity.generation,
            &child_session_id,
            gateway_now_ms() + 30_000,
        )
        .await
        .expect("scoped child turn started");

    append_assistant_live_text_update(
        &state,
        &activity.activity_id,
        &parent_session_id,
        turn_id,
        "Committed prefix.",
    )
    .await;

    let snapshot = thread_snapshot(&state, &scope, Some(&parent_session_id))
        .await
        .expect("snapshot");
    let entries = snapshot["entries"].as_array().expect("entries");
    assert_eq!(snapshot["activity"]["running"], true, "{snapshot:#}");
    assert_eq!(entries.len(), 1, "{snapshot:#}");
    assert_eq!(entries[0]["source"], "runtime.message");
    assert_eq!(entries[0]["turnId"], turn_id);
    assert_eq!(entries[0]["metadata"]["liveOrder"], 0);
    assert_eq!(entries[0]["blocks"][0]["body"], "Committed **prefix**.");
}

async fn append_exec_live_update(
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
    let snapshot_key = format!("{activity_id}:{turn_id}:live-tool");
    state
        .inner
        .durability
        .upsert_gateway_live_snapshots(&[GatewayLiveSnapshotInput {
            snapshot_key: &snapshot_key,
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        }])
        .await
        .expect("upsert live snapshot");
}

async fn append_stale_exec_live_snapshot(
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
    let snapshot_key = format!("{activity_id}:{turn_id}:stale-live-tool");
    state
        .inner
        .durability
        .upsert_gateway_live_snapshots(&[GatewayLiveSnapshotInput {
            snapshot_key: &snapshot_key,
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        }])
        .await
        .expect("upsert stale live snapshot");
}

async fn append_assistant_live_text_update(
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
    let snapshot_key = format!("{activity_id}:{turn_id}:live-text");
    state
        .inner
        .durability
        .upsert_gateway_live_snapshots(&[GatewayLiveSnapshotInput {
            snapshot_key: &snapshot_key,
            activity_id: Some(activity_id),
            owner_id: Some(state.inner.gateway.owner_id()),
            thread_id: Some(session_id),
            turn_id: Some(turn_id),
            event_kind: "entryUpdated",
            event: serde_json::to_value(event).expect("event value"),
        }])
        .await
        .expect("upsert live snapshot");
}
