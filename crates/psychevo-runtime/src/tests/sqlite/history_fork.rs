#[allow(unused_imports)]
use super::*;
use crate::store::PromptPrefixRecord;
use psychevo_agent_core::now_ms;

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

#[test]
fn native_history_fork_copies_prefix_and_omits_transient_ownership() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = StateRuntime::open(temp.path().join("state.db")).expect("store");
    let source = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .expect("source");
    store
        .set_session_title(&source, "Kept title")
        .expect("title");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &source,
            &cwd_text,
            Some("resident-native-handle"),
        ))
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
        .expect("first");
    store
        .append_message(&source, &assistant_message("answer", 2))
        .expect("assistant");
    let selected = store
        .append_message_with_undo_snapshot_and_context_evidence(
            &source,
            &user_message("selected", 3),
            None,
            &[],
        )
        .expect("selected");
    store
        .append_message(&source, &assistant_message("suffix", 4))
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
        .expect("suffix compaction");
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-kept",
            thread_id: &source,
            status: "failed",
            outcome: Some("failure"),
            error_message: Some("kept failure"),
            started_at_ms: Some(1),
            completed_at_ms: 2,
            metadata: Some(json!({"firstCommittedSeq": 1, "lastCommittedSeq": 2})),
        })
        .expect("kept terminal");
    store
        .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
            turn_id: "turn-suffix",
            thread_id: &source,
            status: "failed",
            outcome: Some("failure"),
            error_message: Some("suffix failure"),
            started_at_ms: Some(3),
            completed_at_ms: 4,
            metadata: Some(json!({"firstCommittedSeq": 3, "lastCommittedSeq": 4})),
        })
        .expect("suffix terminal");
    store
        .set_session_revert_state(
            &source,
            SessionRevertState::workspace_undo(selected, "snapshot".to_string()),
        )
        .expect("source revert");
    assert!(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &source,
                before_session_seq: Some(selected),
            })
            .expect_err("staged source must reject fork")
            .to_string()
            .contains("not an eligible root interactive Thread")
    );
    store
        .clear_session_revert_state(&source)
        .expect("clear source revert");

    let child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: Some(selected),
        })
        .expect("point fork");

    let child_summary = store
        .session_summary(&child)
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
            .expect("metadata")
            .and_then(|metadata| metadata.get("forkedFromThreadId").cloned()),
        Some(json!(source))
    );
    assert!(
        store
            .session_revert_state(&child)
            .expect("revert")
            .is_none()
    );
    assert_eq!(
        store
            .load_context_evidence(&child, first)
            .expect("evidence")
            .len(),
        1
    );
    assert_eq!(
        store
            .load_session_prompt_prefix_version(&child, 1)
            .expect("child prompt prefix")
            .map(|prefix| prefix.prefix_hash),
        Some("prefix-hash".to_string())
    );
    assert_eq!(
        store
            .list_valid_session_compactions(&child)
            .expect("child compactions")
            .into_iter()
            .map(|compaction| compaction.summary_text)
            .collect::<Vec<_>>(),
        ["kept compaction"]
    );
    let child_terminals = store
        .list_gateway_turn_terminals_for_thread(&child)
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
        .expect("binding")
        .expect("child binding");
    assert_eq!(child_binding.backend_kind.as_deref(), Some("native"));
    assert_eq!(child_binding.runtime_ref.as_deref(), Some("native-default"));
    assert_eq!(child_binding.native_session_id, None);
    assert!(
        store
            .gateway_source_binding("web:fork-source")
            .expect("source binding")
            .is_some_and(|binding| binding.thread_id == source)
    );
    assert_eq!(
        store.load_messages(&source).expect("source messages").len(),
        4
    );

    let empty_child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: Some(first),
        })
        .expect("empty prefix fork");
    assert!(
        store
            .load_messages(&empty_child)
            .expect("empty child")
            .is_empty()
    );

    let full_child = store
        .fork_native_session_history(NativeSessionForkInput {
            source_session_id: &source,
            before_session_seq: None,
        })
        .expect("full fork");
    assert_eq!(
        store.load_messages(&full_child).expect("full child").len(),
        4
    );
}

#[test]
fn native_history_fork_rejects_non_root_dedicated_side_and_running_sessions() {
    let temp = tempdir().expect("temp");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_text = cwd.display().to_string();
    let store = StateRuntime::open(temp.path().join("state.db")).expect("store");
    let root = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .expect("root");
    store
        .create_gateway_runtime_binding(native_binding_input(&root, &cwd_text, Some(&root)))
        .expect("root binding");

    let child = store
        .create_child_session_with_metadata(&root, &cwd, "tui", "model", "provider", None)
        .expect("child");
    store
        .create_gateway_runtime_binding(native_binding_input(&child, &cwd_text, Some(&child)))
        .expect("child binding");
    let dedicated = store
        .create_session_with_metadata(&cwd, "channel", "model", "provider", None)
        .expect("dedicated");
    store
        .create_gateway_runtime_binding(native_binding_input(
            &dedicated,
            &cwd_text,
            Some(&dedicated),
        ))
        .expect("dedicated binding");
    let side = store
        .create_session_with_metadata(
            &cwd,
            "web",
            "model",
            "provider",
            Some(json!({"side_conversation": true})),
        )
        .expect("side");
    store
        .create_gateway_runtime_binding(native_binding_input(&side, &cwd_text, Some(&side)))
        .expect("side binding");

    for session_id in [&child, &dedicated, &side] {
        assert!(
            store
                .fork_native_session_history(NativeSessionForkInput {
                    source_session_id: session_id,
                    before_session_seq: None,
                })
                .expect_err("ineligible session")
                .to_string()
                .contains("not an eligible root interactive Thread")
        );
    }

    store
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "running-history-fork",
            thread_id: Some(&root),
            source_key: Some("web:history-fork"),
            turn_id: Some("turn:history-fork"),
            kind: "turn",
            owner_id: "test-owner",
            owner_surface: Some("test"),
            lease_expires_at_ms: now_ms() + 60_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("running activity");
    assert!(
        store
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &root,
                before_session_seq: None,
            })
            .expect_err("running session")
            .to_string()
            .contains("running Thread cannot be forked")
    );
}
