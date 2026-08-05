use crate::snapshot::SnapshotStore;
use crate::state::{ConversationDraftPart, SessionRevertState, StateRuntime};
use crate::store::{
    AgentEdgeStatus, GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, SessionRevertKind,
};
use crate::tests::sessions_titles::{assistant_message, user_message};
use crate::types::{EDITABLE_INPUT_METADATA_KEY, SessionUndoOptions};
use crate::undo::{redo_session, undo_session};
use psychevo_agent_core::{Message, UserContentBlock};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

use crate::state::store_undo_state::{
    ConversationEditConflict, ConversationEditDraftFidelity, ConversationEditDraftRead,
    ConversationEditDraftReadOutcome, ConversationEditDraftUnavailable,
    ConversationEditRestoreOutcome, ConversationEditStageOutcome, ConversationEditUnavailable,
    HistoryEditingEligibility, HistoryEditingFacts, HistoryEditingRevertFacts,
    HistoryEditingUnavailable,
};

async fn make_native_history_eligible(
    store: &StateRuntime,
    session_id: &str,
    cwd: &std::path::Path,
) {
    make_history_binding(
        store,
        session_id,
        cwd,
        "native",
        GatewayRuntimeBindingOwnership::ReadWrite,
        None,
    )
    .await;
}

async fn make_history_binding(
    store: &StateRuntime,
    session_id: &str,
    cwd: &std::path::Path,
    backend_kind: &str,
    ownership: GatewayRuntimeBindingOwnership,
    parent_thread_id: Option<&str>,
) {
    let cwd = cwd.display().to_string();
    store
        .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
            thread_id: session_id,
            agent_ref: None,
            agent_fingerprint: "agent-fingerprint",
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind,
            native_kind: "native",
            native_session_id: Some(session_id),
            cwd: &cwd,
            profile_fingerprint: "profile-fingerprint",
            profile_revision: "profile-revision",
            profile_config_json: "{}",
            adapter_kind: "native",
            adapter_revision: "test",
            ownership,
            parent_thread_id,
        })
        .await
        .expect("native binding");
}

async fn assert_history_editing_ineligible(
    store: &StateRuntime,
    session_id: &str,
    boundary_seq: i64,
    expected: HistoryEditingUnavailable,
) {
    let baseline = store.session_metadata(session_id).await.expect("baseline");
    assert_eq!(
        store
            .history_editing_facts(session_id)
            .await
            .expect("eligibility facts"),
        HistoryEditingFacts {
            eligibility: HistoryEditingEligibility::Unavailable(expected),
            staged: None,
        }
    );
    assert_eq!(
        store
            .stage_conversation_edit_atomic(session_id, boundary_seq, replacement_draft("edited"),)
            .await
            .expect("ineligible stage"),
        ConversationEditStageOutcome::Unavailable(ConversationEditUnavailable::HistoryEditing(
            expected
        ))
    );
    assert_eq!(
        store
            .session_metadata(session_id)
            .await
            .expect("unchanged metadata"),
        baseline
    );
    assert!(
        store
            .session_revert_state(session_id)
            .await
            .expect("revert state")
            .is_none()
    );
}

async fn append_exact_editable_user(store: &StateRuntime, session_id: &str) -> i64 {
    store
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            session_id,
            &Message::User {
                content: vec![
                    UserContentBlock::text("durable text plus synthetic context"),
                    UserContentBlock::local_image("local.png"),
                    UserContentBlock::image_url("https://example.test/remote.png"),
                ],
                timestamp_ms: 1,
            },
            Some(json!({
                EDITABLE_INPUT_METADATA_KEY: {
                    "version": 1,
                    "parts": [
                        {"type": "image", "imageBlockIndex": 1},
                        {"type": "text", "text": "visible text"},
                        {"type": "image", "imageBlockIndex": 0}
                    ]
                }
            })),
            Some("visible text".to_string()),
            &[],
        )
        .await
        .expect("exact editable user")
}

fn replacement_draft(text: &str) -> Vec<ConversationDraftPart> {
    vec![ConversationDraftPart::Text {
        text: text.to_string(),
    }]
}

#[tokio::test]
pub(crate) async fn conversation_edit_draft_read_preserves_exact_order_and_marks_legacy_fidelity() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let session_id = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("session");
    make_native_history_eligible(&store, &session_id, temp.path()).await;
    let exact_seq = append_exact_editable_user(&store, &session_id).await;
    assert_eq!(
        store
            .conversation_editable_draft(&session_id, exact_seq)
            .await
            .expect("exact draft"),
        ConversationEditDraftReadOutcome::Available(ConversationEditDraftRead {
            session_seq: exact_seq,
            draft: vec![
                ConversationDraftPart::ImageUrl {
                    url: "https://example.test/remote.png".to_string(),
                },
                ConversationDraftPart::Text {
                    text: "visible text".to_string(),
                },
                ConversationDraftPart::LocalImage {
                    path: "local.png".to_string(),
                },
            ],
            fidelity: ConversationEditDraftFidelity::Exact,
        })
    );

    let legacy_seq = store
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            &session_id,
            &Message::User {
                content: vec![
                    UserContentBlock::text("legacy text"),
                    UserContentBlock::image_url("https://example.test/legacy.png"),
                ],
                timestamp_ms: 2,
            },
            None,
            None,
            &[],
        )
        .await
        .expect("legacy message");
    assert_eq!(
        store
            .conversation_editable_draft(&session_id, legacy_seq)
            .await
            .expect("legacy draft"),
        ConversationEditDraftReadOutcome::Available(ConversationEditDraftRead {
            session_seq: legacy_seq,
            draft: vec![
                ConversationDraftPart::Text {
                    text: "legacy text".to_string(),
                },
                ConversationDraftPart::ImageUrl {
                    url: "https://example.test/legacy.png".to_string(),
                },
            ],
            fidelity: ConversationEditDraftFidelity::BestEffort,
        })
    );
}

#[tokio::test]
pub(crate) async fn conversation_edit_stage_restore_is_atomic_idempotent_and_preserves_metadata() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let session_id = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("session");
    make_native_history_eligible(&store, &session_id, temp.path()).await;
    store
        .set_session_metadata_field(&session_id, "unrelated", Some(json!({"kept": true})))
        .await
        .expect("unrelated metadata");
    let boundary_seq = append_exact_editable_user(&store, &session_id).await;
    store
        .append_message(&session_id, &assistant_message("suffix", 2))
        .await
        .expect("suffix");
    let replacement = replacement_draft("edited");

    assert_eq!(
        store
            .stage_conversation_edit_atomic(&session_id, boundary_seq, replacement.clone(),)
            .await
            .expect("stage"),
        ConversationEditStageOutcome::Staged
    );
    assert_eq!(
        store
            .history_editing_facts(&session_id)
            .await
            .expect("facts"),
        HistoryEditingFacts {
            eligibility: HistoryEditingEligibility::Eligible,
            staged: Some(HistoryEditingRevertFacts::ConversationEdit {
                boundary_seq,
                hidden_entry_count: 2,
                draft: replacement.clone(),
            }),
        }
    );
    assert_eq!(
        store
            .stage_conversation_edit_atomic(&session_id, boundary_seq, replacement.clone(),)
            .await
            .expect("idempotent retry"),
        ConversationEditStageOutcome::AlreadyStaged
    );
    assert_eq!(
        store
            .stage_conversation_edit_atomic(
                &session_id,
                boundary_seq,
                replacement_draft("different"),
            )
            .await
            .expect("conflicting retry"),
        ConversationEditStageOutcome::Conflict(ConversationEditConflict::ConversationEditStaged)
    );
    assert_eq!(
        store
            .restore_conversation_edit_atomic(&session_id)
            .await
            .expect("restore"),
        ConversationEditRestoreOutcome::Restored(replacement)
    );
    assert_eq!(
        store
            .history_editing_facts(&session_id)
            .await
            .expect("restored facts"),
        HistoryEditingFacts {
            eligibility: HistoryEditingEligibility::Eligible,
            staged: None,
        }
    );
    assert_eq!(
        store
            .session_metadata(&session_id)
            .await
            .expect("metadata")
            .and_then(|metadata| metadata.get("unrelated").cloned()),
        Some(json!({"kept": true}))
    );
}

#[tokio::test]
pub(crate) async fn conversation_edit_faults_leave_metadata_and_workspace_undo_unchanged() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let session_id = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("session");
    make_native_history_eligible(&store, &session_id, temp.path()).await;
    let boundary_seq = append_exact_editable_user(&store, &session_id).await;
    let baseline = store.session_metadata(&session_id).await.expect("baseline");

    assert_eq!(
        store
            .stage_conversation_edit_atomic(&session_id, boundary_seq, replacement_draft("   "),)
            .await
            .expect("empty replacement"),
        ConversationEditStageOutcome::Unavailable(ConversationEditUnavailable::Draft(
            ConversationEditDraftUnavailable::EmptyReplacement
        ))
    );
    assert_eq!(
        store
            .stage_conversation_edit_atomic(
                &session_id,
                boundary_seq + 100,
                replacement_draft("edited"),
            )
            .await
            .expect("missing boundary"),
        ConversationEditStageOutcome::Unavailable(ConversationEditUnavailable::Draft(
            ConversationEditDraftUnavailable::MessageNotFound
        ))
    );
    assert_eq!(
        store
            .session_metadata(&session_id)
            .await
            .expect("unchanged"),
        baseline
    );

    let workspace_undo =
        SessionRevertState::workspace_undo(boundary_seq, "workspace-snapshot".to_string());
    store
        .set_session_revert_state(&session_id, workspace_undo.clone())
        .await
        .expect("workspace undo");
    assert_eq!(
        store
            .stage_conversation_edit_atomic(&session_id, boundary_seq, replacement_draft("edited"),)
            .await
            .expect("workspace conflict"),
        ConversationEditStageOutcome::Conflict(ConversationEditConflict::WorkspaceUndoStaged)
    );
    assert_eq!(
        store
            .restore_conversation_edit_atomic(&session_id)
            .await
            .expect("workspace restore conflict"),
        ConversationEditRestoreOutcome::Conflict(ConversationEditConflict::WorkspaceUndoStaged)
    );
    assert_eq!(
        store
            .session_revert_state(&session_id)
            .await
            .expect("workspace undo remains"),
        Some(workspace_undo)
    );
}

#[tokio::test]
pub(crate) async fn concurrent_store_handles_cannot_overwrite_conversation_edits() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let first = StateRuntime::open(&db).await.expect("first store");
    let second = StateRuntime::open(&db).await.expect("second store");
    let session_id = first
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("session");
    make_native_history_eligible(&first, &session_id, temp.path()).await;
    let boundary_seq = append_exact_editable_user(&first, &session_id).await;
    let (left, right) = tokio::join!(
        first.stage_conversation_edit_atomic(&session_id, boundary_seq, replacement_draft("left"),),
        second.stage_conversation_edit_atomic(
            &session_id,
            boundary_seq,
            replacement_draft("right"),
        ),
    );
    let outcomes = [left.expect("left"), right.expect("right")];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ConversationEditStageOutcome::Staged)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                **outcome
                    == ConversationEditStageOutcome::Conflict(
                        ConversationEditConflict::ConversationEditStaged,
                    )
            })
            .count(),
        1
    );
    let staged = first
        .session_revert_state(&session_id)
        .await
        .expect("revert")
        .expect("staged");
    assert!(matches!(
        staged.kind,
        SessionRevertKind::ConversationEdit { ref draft, .. }
            if draft == &replacement_draft("left") || draft == &replacement_draft("right")
    ));
}

#[tokio::test]
pub(crate) async fn conversation_edit_transaction_rechecks_every_native_history_eligibility_fact() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");

    assert_eq!(
        store
            .history_editing_facts("missing-thread")
            .await
            .expect("missing facts")
            .eligibility,
        HistoryEditingEligibility::Unavailable(HistoryEditingUnavailable::SessionNotFound)
    );
    assert_eq!(
        store
            .stage_conversation_edit_atomic("missing-thread", 1, replacement_draft("edited"),)
            .await
            .expect("missing stage"),
        ConversationEditStageOutcome::Unavailable(ConversationEditUnavailable::HistoryEditing(
            HistoryEditingUnavailable::SessionNotFound
        ))
    );

    let unsupported = store
        .create_session_with_metadata(temp.path(), "automation", "model", "provider", None)
        .await
        .expect("unsupported session");
    make_native_history_eligible(&store, &unsupported, temp.path()).await;
    let unsupported_seq = append_exact_editable_user(&store, &unsupported).await;
    assert_history_editing_ineligible(
        &store,
        &unsupported,
        unsupported_seq,
        HistoryEditingUnavailable::UnsupportedSource,
    )
    .await;

    let parent = store
        .create_session_with_metadata(temp.path(), "web", "model", "provider", None)
        .await
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, temp.path(), "web", "model", "provider", None)
        .await
        .expect("child");
    make_history_binding(
        &store,
        &child,
        temp.path(),
        "native",
        GatewayRuntimeBindingOwnership::ReadWrite,
        Some(&parent),
    )
    .await;
    let child_seq = append_exact_editable_user(&store, &child).await;
    assert_history_editing_ineligible(
        &store,
        &child,
        child_seq,
        HistoryEditingUnavailable::ChildThread,
    )
    .await;

    let agent_child = store
        .create_session_with_metadata(temp.path(), "web", "model", "provider", None)
        .await
        .expect("agent child");
    make_native_history_eligible(&store, &agent_child, temp.path()).await;
    store
        .upsert_agent_edge(&parent, &agent_child, AgentEdgeStatus::Closed, None)
        .await
        .expect("agent edge");
    let agent_child_seq = append_exact_editable_user(&store, &agent_child).await;
    assert_history_editing_ineligible(
        &store,
        &agent_child,
        agent_child_seq,
        HistoryEditingUnavailable::AgentChildThread,
    )
    .await;

    let side = store
        .create_session_with_metadata(
            temp.path(),
            "web",
            "model",
            "provider",
            Some(json!({"side_conversation": true})),
        )
        .await
        .expect("side root");
    make_native_history_eligible(&store, &side, temp.path()).await;
    let side_seq = append_exact_editable_user(&store, &side).await;
    assert_history_editing_ineligible(
        &store,
        &side,
        side_seq,
        HistoryEditingUnavailable::SideConversation,
    )
    .await;

    let missing_binding = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("missing binding session");
    let missing_binding_seq = append_exact_editable_user(&store, &missing_binding).await;
    assert_history_editing_ineligible(
        &store,
        &missing_binding,
        missing_binding_seq,
        HistoryEditingUnavailable::RuntimeBindingMissing,
    )
    .await;

    let unresolved = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("unresolved session");
    make_native_history_eligible(&store, &unresolved, temp.path()).await;
    let unresolved_seq = append_exact_editable_user(&store, &unresolved).await;
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query(
        "UPDATE gateway_runtime_bindings SET resolution_status = 'unresolved', \
         unresolved_reason = 'test' WHERE thread_id = ?1",
    )
    .bind(&unresolved)
    .execute(&mut *conn)
    .await
    .expect("unresolve binding");
    drop(conn);
    assert_history_editing_ineligible(
        &store,
        &unresolved,
        unresolved_seq,
        HistoryEditingUnavailable::RuntimeBindingUnresolved,
    )
    .await;

    let non_native = store
        .create_session_with_metadata(temp.path(), "web", "model", "provider", None)
        .await
        .expect("non-native session");
    make_native_history_eligible(&store, &non_native, temp.path()).await;
    let non_native_seq = append_exact_editable_user(&store, &non_native).await;
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query("UPDATE gateway_runtime_bindings SET backend_kind = 'acp' WHERE thread_id = ?1")
        .bind(&non_native)
        .execute(&mut *conn)
        .await
        .expect("non-native binding");
    drop(conn);
    assert_history_editing_ineligible(
        &store,
        &non_native,
        non_native_seq,
        HistoryEditingUnavailable::RuntimeBindingNotNative,
    )
    .await;

    let changed_after_precheck = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("prechecked session");
    make_native_history_eligible(&store, &changed_after_precheck, temp.path()).await;
    let changed_seq = append_exact_editable_user(&store, &changed_after_precheck).await;
    assert_eq!(
        store
            .history_editing_facts(&changed_after_precheck)
            .await
            .expect("eligible precheck")
            .eligibility,
        HistoryEditingEligibility::Eligible
    );
    let mut conn = store.acquire_sqlx().await.expect("connection");
    sqlx::query("UPDATE gateway_runtime_bindings SET ownership = 'read_only' WHERE thread_id = ?1")
        .bind(&changed_after_precheck)
        .execute(&mut *conn)
        .await
        .expect("change ownership after precheck");
    drop(conn);
    assert_history_editing_ineligible(
        &store,
        &changed_after_precheck,
        changed_seq,
        HistoryEditingUnavailable::RuntimeBindingReadOnly,
    )
    .await;
}

#[tokio::test]
pub(crate) async fn undo_redo_restore_git_snapshots_and_visible_message_ranges() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = cwd.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");

    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let snapshots = SnapshotStore::new(temp.path().join("snapshots"), cwd.clone());
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    assert!(snapshots.git_dir().expect("git dir").join("HEAD").exists());
    assert!(!temp.path().join("snapshots").join("sessions").exists());
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .await
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .await
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .await
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .await
        .expect("assistant second");

    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).await.expect("state runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };
    let undo = undo_session(options.clone()).await.expect("undo latest");
    assert_eq!(undo.prompt, "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .await
            .expect("visible")
            .len(),
        2
    );

    let undo = undo_session(options.clone()).await.expect("undo previous");
    assert_eq!(undo.prompt, "first prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "base\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .await
            .expect("visible")
            .len(),
        0
    );

    let redo = redo_session(options.clone()).await.expect("redo first");
    assert!(!redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .await
            .expect("visible")
            .len(),
        2
    );

    let redo = redo_session(options).await.expect("redo complete");
    assert!(redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .await
            .expect("visible")
            .len(),
        4
    );
    assert!(
        store
            .session_revert_state(&session_id)
            .await
            .expect("revert state")
            .is_none()
    );
}

#[tokio::test]
pub(crate) async fn cleanup_reverted_messages_deletes_hidden_range() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = cwd.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let snapshots = SnapshotStore::new(temp.path().join("snapshots"), cwd.clone());
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .await
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .await
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .await
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .await
        .expect("assistant second");

    undo_session(SessionUndoOptions {
        state: StateRuntime::open(&db).await.expect("state runtime"),
        cwd,
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    })
    .await
    .expect("undo");

    let removed = store
        .cleanup_reverted_messages(&session_id)
        .await
        .expect("cleanup");
    assert_eq!(removed, 2);
    assert_eq!(
        store
            .load_messages(&session_id)
            .await
            .expect("messages")
            .len(),
        2
    );
    let summary = store
        .session_summary(&session_id)
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(summary.message_count, 2);
    assert!(
        store
            .session_revert_state(&session_id)
            .await
            .expect("revert state")
            .is_none()
    );
}

#[tokio::test]
pub(crate) async fn undo_redo_error_paths_do_not_mutate_revert_state() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).await.expect("state runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };

    let err = undo_session(options.clone())
        .await
        .expect_err("nothing to undo");
    assert!(err.to_string().contains("nothing to undo"));
    let err = redo_session(options.clone())
        .await
        .expect_err("nothing to redo");
    assert!(err.to_string().contains("nothing to redo"));

    store
        .append_message(&session_id, &user_message("no snapshot", 1))
        .await
        .expect("user");
    let err = undo_session(options).await.expect_err("missing snapshot");
    assert!(err.to_string().contains("undo snapshot is unavailable"));
    assert!(
        store
            .session_revert_state(&session_id)
            .await
            .expect("revert state")
            .is_none()
    );
    assert_eq!(
        store
            .load_messages(&session_id)
            .await
            .expect("messages")
            .len(),
        1
    );
}

#[tokio::test]
pub(crate) async fn conversation_edit_is_restart_safe_and_never_restores_workspace_snapshots() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let file = cwd.join("tracked.txt");
    fs::write(&file, "workspace stays current\n").expect("workspace");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some("unused-snapshot-one".to_string()),
        )
        .await
        .expect("first");
    let boundary = 2;
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 2),
            Some("unused-snapshot-two".to_string()),
        )
        .await
        .expect("second");
    let staged = SessionRevertState::conversation_edit(
        boundary,
        format!("message:{boundary}"),
        vec![ConversationDraftPart::Text {
            text: "edited prompt".to_string(),
        }],
    );
    store
        .set_session_revert_state(&session_id, staged.clone())
        .await
        .expect("stage conversation edit");

    assert_eq!(
        fs::read_to_string(&file).expect("workspace"),
        "workspace stays current\n"
    );
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .await
            .expect("visible")
            .len(),
        1
    );
    drop(store);
    let restarted = StateRuntime::open(&db).await.expect("restart");
    assert_eq!(
        restarted
            .session_revert_state(&session_id)
            .await
            .expect("revert"),
        Some(staged)
    );

    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).await.expect("runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };
    assert!(
        undo_session(options.clone())
            .await
            .expect_err("conversation edit blocks undo")
            .to_string()
            .contains("staged conversation edit")
    );
    assert!(
        redo_session(options)
            .await
            .expect_err("conversation edit blocks redo")
            .to_string()
            .contains("staged conversation edit")
    );
    assert_eq!(
        fs::read_to_string(&file).expect("workspace"),
        "workspace stays current\n"
    );
    assert_eq!(
        restarted
            .cleanup_reverted_messages(&session_id)
            .await
            .expect("accepted replacement cleanup"),
        1
    );
    assert_eq!(
        restarted
            .load_messages(&session_id)
            .await
            .expect("messages")
            .len(),
        1
    );
}

#[tokio::test]
pub(crate) async fn legacy_revert_metadata_parses_as_workspace_undo() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db"))
        .await
        .expect("store");
    let session_id = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .await
        .expect("session");
    store
        .set_session_metadata_field(
            &session_id,
            crate::store::SESSION_REVERT_METADATA_KEY,
            Some(json!({"start_seq": 7, "original_snapshot": "legacy-snapshot"})),
        )
        .await
        .expect("legacy metadata");
    assert_eq!(
        store
            .session_revert_state(&session_id)
            .await
            .expect("revert")
            .and_then(|revert| revert.original_snapshot().map(str::to_string)),
        Some("legacy-snapshot".to_string())
    );
}
