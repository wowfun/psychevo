use crate::application::{FrameworkTurnTerminalOutcome, FrameworkTurnTerminalStatus};
use crate::paths::canonical_cwd;
use crate::state::{SessionRevertState, StateRuntime};
use crate::store::{
    ContextEvidenceInput, GatewayActivityClaimInput, GatewayActivityKind,
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewaySourceBindingInput,
    GatewayTurnDeliveryInput, GatewayTurnTerminalInput, NativeSessionForkInput, PromptPrefixRecord,
    SessionCompactionInput,
};
use crate::tests::sessions_titles::{assistant_message, user_message};
use psychevo_agent_core::{Message, now_ms};
use serde_json::json;
use tempfile::tempdir;

fn native_binding_input<'a>(
    thread_id: &'a str,
    cwd: &'a str,
    native_session_id: Option<&'a str>,
) -> GatewayRuntimeBindingInput<'a> {
    GatewayRuntimeBindingInput {
        thread_id,
        agent_ref: Some("main"),
        agent_fingerprint: "agent-fingerprint",
        agent_definition_json: r#"{"name":"main"}"#,
        runtime_ref: "native-default",
        backend_kind: "native",
        native_kind: "native",
        native_session_id,
        cwd,
        profile_fingerprint: "profile-fingerprint",
        profile_revision: "profile-revision",
        profile_config_json: "{}",
        adapter_kind: "native",
        adapter_revision: "adapter-revision",
        ownership: GatewayRuntimeBindingOwnership::ReadWrite,
        parent_thread_id: None,
    }
}

fn assert_native_history_unavailable(error: crate::Error, expected_reason: &str) {
    let evidence = error
        .structured_data()
        .expect("Native history rejection must be structured");
    assert_eq!(evidence["kind"], "native_history_unavailable");
    assert_eq!(evidence["reason"], expected_reason);
}

fn assert_thread_busy(error: crate::Error, expected_operation: &str) {
    let evidence = error
        .structured_data()
        .expect("busy rejection must be structured");
    assert_eq!(evidence["kind"], "thread_busy");
    assert_eq!(evidence["blockingOperation"], expected_operation);
}

async fn session_count(store: &StateRuntime) -> i64 {
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&mut *conn)
        .await
        .expect("session count")
}

async fn assert_fork_unavailable_without_child(
    store: &StateRuntime,
    thread_id: &str,
    expected_reason: &str,
) {
    let before = session_count(store).await;
    assert_native_history_unavailable(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: thread_id,
                before_session_seq: None,
            })
            .await
            .expect_err("ineligible fork must fail"),
        expected_reason,
    );
    assert_eq!(
        session_count(store).await,
        before,
        "failed fork created a child"
    );
}

async fn assert_fork_busy_without_child(store: &StateRuntime, thread_id: &str) {
    let before = session_count(store).await;
    assert_thread_busy(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: thread_id,
                before_session_seq: None,
            })
            .await
            .expect_err("busy fork must fail"),
        "gateway_activity",
    );
    assert_eq!(
        session_count(store).await,
        before,
        "busy fork created a child"
    );
}

#[tokio::test]
async fn native_history_fork_copies_prefix_and_omits_transient_ownership() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let source = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("source");
    store
        .set_session_title(&source, "Kept title")
        .await
        .expect("title");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &source,
            &cwd_text,
            Some("resident-native-handle"),
        ))
        .await
        .expect("binding");
    store
        .upsert_gateway_source_binding(GatewaySourceBindingInput {
            source_key: "web:fork-source",
            source_kind: "web",
            raw_identity: json!({"id": "fork-source"}),
            visible_name: Some("Workbench"),
            thread_id: &source,
            backend_kind: "psychevo",
            backend_native_id: Some(&source),
            lineage: None,
        })
        .await
        .expect("source binding");

    store
        .upsert_session_prompt_prefix(PromptPrefixRecord {
            session_id: source.clone(),
            version: 1,
            created_at_ms: 1,
            provider: "provider".to_string(),
            model: "model".to_string(),
            prefix_hash: "prefix-hash".to_string(),
            tool_declarations_hash: "tools-hash".to_string(),
            invalidation_reason: None,
            slots: Vec::new(),
            metadata: None,
        })
        .await
        .expect("prompt prefix");
    let first = store
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            &source,
            &user_message("first", 1),
            Some(json!({"prompt_prefix": {"version": 1}})),
            None,
            &[ContextEvidenceInput {
                role: "system".to_string(),
                source_kind: "instruction".to_string(),
                source_name: None,
                source_path: None,
                provider_group: None,
                provider_block_index: None,
                context_kind: None,
                content_text: "evidence".to_string(),
                metadata: None,
            }],
        )
        .await
        .expect("first");
    store
        .append_message(&source, &assistant_message("answer", 2))
        .await
        .expect("assistant");
    let selected = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &source,
            &user_message("selected", 3),
            None,
            &[],
        )
        .await
        .expect("selected");
    store
        .append_message(&source, &assistant_message("suffix", 4))
        .await
        .expect("suffix");
    store
        .append_session_compaction(SessionCompactionInput {
            session_id: source.clone(),
            reason: "threshold".to_string(),
            summary_text: "kept compaction".to_string(),
            first_kept_session_seq: 2,
            created_after_session_seq: 2,
            tokens_before: Some(100),
            tokens_after: Some(40),
            summary_provider: "provider".to_string(),
            summary_model: "model".to_string(),
            instructions: None,
            metadata: None,
        })
        .await
        .expect("kept compaction");
    store
        .append_session_compaction(SessionCompactionInput {
            session_id: source.clone(),
            reason: "threshold".to_string(),
            summary_text: "suffix compaction".to_string(),
            first_kept_session_seq: 3,
            created_after_session_seq: 3,
            tokens_before: None,
            tokens_after: None,
            summary_provider: "provider".to_string(),
            summary_model: "model".to_string(),
            instructions: None,
            metadata: None,
        })
        .await
        .expect("suffix compaction");
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-kept",
            thread_id: &source,
            status: FrameworkTurnTerminalStatus::Failed,
            outcome: Some(FrameworkTurnTerminalOutcome::Failed),
            error_message: Some("kept failure"),
            started_at_ms: Some(1),
            completed_at_ms: 2,
            boundary_session_seq: Some(2),
            metadata: Some(json!({"firstCommittedSeq": 1, "lastCommittedSeq": 2})),
        })
        .await
        .expect("kept terminal");
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-suffix",
            thread_id: &source,
            status: FrameworkTurnTerminalStatus::Failed,
            outcome: Some(FrameworkTurnTerminalOutcome::Failed),
            error_message: Some("suffix failure"),
            started_at_ms: Some(3),
            completed_at_ms: 4,
            boundary_session_seq: Some(4),
            metadata: Some(json!({"firstCommittedSeq": 3, "lastCommittedSeq": 4})),
        })
        .await
        .expect("suffix terminal");
    store
        .set_session_revert_state(
            &source,
            SessionRevertState::workspace_undo(selected, "snapshot".to_string()),
        )
        .await
        .expect("source revert");
    assert_native_history_unavailable(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &source,
                before_session_seq: Some(selected),
            })
            .await
            .expect_err("staged source must reject fork"),
        "workspace_undo_staged",
    );
    store
        .clear_session_revert_state(&source)
        .await
        .expect("clear source revert");

    let child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: Some(selected),
        })
        .await
        .expect("point fork");

    let child_summary = store
        .session_summary(&child)
        .await
        .expect("child summary")
        .expect("child exists");
    assert_eq!(child_summary.parent_session_id, None);
    assert_eq!(child_summary.source, "web");
    assert_eq!(child_summary.cwd, cwd_text);
    assert_eq!(child_summary.title.as_deref(), Some("Kept title"));
    assert_eq!(child_summary.message_count, 2);
    assert_eq!(
        store
            .session_metadata(&child)
            .await
            .expect("metadata")
            .and_then(|metadata| metadata.get("forkedFromThreadId").cloned()),
        Some(json!(source))
    );
    assert!(
        store
            .session_revert_state(&child)
            .await
            .expect("revert")
            .is_none()
    );
    assert_eq!(
        store
            .load_context_evidence(&child, first)
            .await
            .expect("evidence")
            .len(),
        1
    );
    assert_eq!(
        store
            .load_session_prompt_prefix_version(&child, 1)
            .await
            .expect("child prompt prefix")
            .map(|prefix| prefix.prefix_hash),
        Some("prefix-hash".to_string())
    );
    assert_eq!(
        store
            .list_valid_session_compactions(&child)
            .await
            .expect("child compactions")
            .into_iter()
            .map(|compaction| compaction.summary_text)
            .collect::<Vec<_>>(),
        ["kept compaction"]
    );
    let child_terminals = store
        .list_gateway_turn_terminals_for_thread(&child)
        .await
        .expect("child terminals");
    assert_eq!(child_terminals.len(), 1);
    assert_eq!(child_terminals[0].thread_id, child);
    assert!(child_terminals[0].turn_id.starts_with("fork:"));
    assert_eq!(
        child_terminals[0].error_message.as_deref(),
        Some("kept failure")
    );
    let child_binding = store
        .gateway_runtime_binding(&child)
        .await
        .expect("binding")
        .expect("child binding");
    assert_eq!(child_binding.backend_kind.as_deref(), Some("native"));
    assert_eq!(child_binding.runtime_ref.as_deref(), Some("native-default"));
    assert_eq!(child_binding.native_session_id, None);
    assert!(
        store
            .gateway_source_binding("web:fork-source")
            .await
            .expect("source binding")
            .is_some_and(|binding| binding.thread_id == source)
    );
    assert_eq!(
        store
            .load_messages(&source)
            .await
            .expect("source messages")
            .len(),
        4
    );

    let empty_child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: Some(first),
        })
        .await
        .expect("empty prefix fork");
    assert!(
        store
            .load_messages(&empty_child)
            .await
            .expect("empty child")
            .is_empty()
    );

    let full_child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: None,
        })
        .await
        .expect("full fork");
    assert_eq!(
        store
            .load_messages(&full_child)
            .await
            .expect("full child")
            .len(),
        4
    );
}

#[tokio::test]
async fn native_history_fork_requires_interactive_native_writable_root_and_idle_durable_state() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let root = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("root");
    store
        .create_gateway_runtime_binding(native_binding_input(&root, &cwd_text, Some(&root)))
        .await
        .expect("root binding");

    let child = store
        .create_child_session_with_metadata(&root, &cwd, "tui", "model", "provider", None)
        .await
        .expect("child");
    store
        .create_gateway_runtime_binding(native_binding_input(&child, &cwd_text, Some(&child)))
        .await
        .expect("child binding");
    let side = store
        .create_session_with_metadata(
            &cwd,
            "web",
            "model",
            "provider",
            Some(json!({"side_conversation": true})),
        )
        .await
        .expect("side");
    store
        .create_gateway_runtime_binding(native_binding_input(&side, &cwd_text, Some(&side)))
        .await
        .expect("side binding");

    assert_fork_unavailable_without_child(&store, &child, "child_thread").await;
    assert_fork_unavailable_without_child(&store, &side, "side_conversation").await;

    for source in ["channel", "automation"] {
        let dedicated = store
            .create_session_with_metadata(&cwd, source, "model", "provider", None)
            .await
            .expect("dedicated source");
        store
            .create_gateway_runtime_binding(native_binding_input(
                &dedicated,
                &cwd_text,
                Some(&dedicated),
            ))
            .await
            .expect("dedicated binding");
        assert_fork_unavailable_without_child(&store, &dedicated, "unsupported_source").await;
    }

    let missing_binding = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("missing binding");
    assert_fork_unavailable_without_child(&store, &missing_binding, "runtime_binding_missing")
        .await;

    let unresolved = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("unresolved");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &unresolved,
            &cwd_text,
            Some(&unresolved),
        ))
        .await
        .expect("unresolved binding");
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query(
        "UPDATE gateway_runtime_bindings SET resolution_status = 'unresolved', \
         unresolved_reason = 'test' WHERE thread_id = ?1",
    )
    .bind(&unresolved)
    .execute(&mut *conn)
    .await
    .expect("mark unresolved");
    drop(conn);
    assert_fork_unavailable_without_child(&store, &unresolved, "runtime_binding_unresolved").await;

    let non_native = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("non-native");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &non_native,
            &cwd_text,
            Some(&non_native),
        ))
        .await
        .expect("non-native binding");
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query("UPDATE gateway_runtime_bindings SET backend_kind = 'acp' WHERE thread_id = ?1")
        .bind(&non_native)
        .execute(&mut *conn)
        .await
        .expect("mark ACP binding");
    drop(conn);
    assert_fork_unavailable_without_child(&store, &non_native, "runtime_binding_not_native").await;

    let read_only = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("read-only");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &read_only,
            &cwd_text,
            Some(&read_only),
        ))
        .await
        .expect("read-only binding");
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query("UPDATE gateway_runtime_bindings SET ownership = 'read_only' WHERE thread_id = ?1")
        .bind(&read_only)
        .execute(&mut *conn)
        .await
        .expect("mark read-only binding");
    drop(conn);
    assert_fork_unavailable_without_child(&store, &read_only, "runtime_binding_read_only").await;

    let no_editable_input = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("no editable input");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &no_editable_input,
            &cwd_text,
            Some(&no_editable_input),
        ))
        .await
        .expect("no editable input binding");
    let empty_user_seq = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &no_editable_input,
            &Message::User {
                content: Vec::new(),
                timestamp_ms: 1,
            },
            None,
            &[],
        )
        .await
        .expect("empty user message");
    let before = session_count(&store).await;
    assert_native_history_unavailable(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &no_editable_input,
                before_session_seq: Some(empty_user_seq),
            })
            .await
            .expect_err("point fork requires editable input"),
        "no_editable_input",
    );
    assert_eq!(session_count(&store).await, before);

    let non_user_boundary = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &no_editable_input,
            &assistant_message("answer", 2),
            None,
            &[],
        )
        .await
        .expect("assistant boundary");
    let before = session_count(&store).await;
    assert_native_history_unavailable(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &no_editable_input,
                before_session_seq: Some(non_user_boundary),
            })
            .await
            .expect_err("point fork boundary must be a user message"),
        "not_user_message",
    );
    assert_eq!(session_count(&store).await, before);

    store
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "running-history-fork",
            thread_id: Some(&root),
            source_key: Some("web:history-fork"),
            turn_id: Some("turn:history-fork"),
            kind: GatewayActivityKind::Turn,
            owner_id: "test-owner",
            owner_surface: Some("test"),
            lease_expires_at_ms: now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("running activity");
    assert_fork_busy_without_child(&store, &root).await;

    let pending_delivery = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("pending delivery");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &pending_delivery,
            &cwd_text,
            Some(&pending_delivery),
        ))
        .await
        .expect("pending delivery binding");
    store
        .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
            turn_id: "pending-history-fork",
            thread_id: &pending_delivery,
            runtime_ref: "native-default",
            input_json: "[]",
            input_hash: "pending-history-fork-hash",
        })
        .await
        .expect("pending delivery");
    assert_fork_busy_without_child(&store, &pending_delivery).await;
}
