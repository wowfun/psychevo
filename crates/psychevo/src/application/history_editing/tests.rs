use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::future::BoxFuture;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::{Notify, Semaphore};

use super::*;
use crate::application::{
    AgentSessionAdapter, AgentTurnInvocation, AgentTurnPreparation, Application, ForkThreadRequest,
    PreparedAgentTurn, StartThreadRequest, Thread, TurnOutcome, TurnRequest, TurnResult,
    UserContentBlock,
};
use crate::state::{GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership};
use crate::types::EDITABLE_INPUT_METADATA_KEY;
use crate::{Error, Result};
use psychevo_agent_core::Message;

#[derive(Debug)]
struct BlockingAdapter {
    started_count: AtomicUsize,
    started: Notify,
    permits: Semaphore,
}

impl BlockingAdapter {
    fn new() -> Self {
        Self {
            started_count: AtomicUsize::new(0),
            started: Notify::new(),
            permits: Semaphore::new(0),
        }
    }

    async fn wait_for_started(&self, expected: usize) {
        loop {
            let notified = self.started.notified();
            if self.started_count.load(Ordering::SeqCst) >= expected {
                return;
            }
            notified.await;
        }
    }

    fn release(&self, count: usize) {
        self.permits.add_permits(count);
    }
}

#[derive(Debug)]
struct BlockingPreparedTurn(Arc<BlockingAdapter>);

impl AgentSessionAdapter for BlockingAdapter {
    fn prepare_turn(
        self: Arc<Self>,
        _request: AgentTurnPreparation,
    ) -> BoxFuture<'static, Result<Box<dyn PreparedAgentTurn>>> {
        Box::pin(
            async move { Ok(Box::new(BlockingPreparedTurn(self)) as Box<dyn PreparedAgentTurn>) },
        )
    }
}

impl PreparedAgentTurn for BlockingPreparedTurn {
    fn invoke(
        self: Box<Self>,
        invocation: AgentTurnInvocation,
    ) -> BoxFuture<'static, Result<TurnResult>> {
        let adapter = self.0;
        Box::pin(async move {
            adapter.started_count.fetch_add(1, Ordering::SeqCst);
            adapter.started.notify_waiters();
            let permit = adapter
                .permits
                .acquire()
                .await
                .map_err(|_| Error::Message("test adapter closed".to_string()))?;
            permit.forget();
            invocation.persistence.confirm_delivery().await?;
            Ok(TurnResult {
                thread_id: invocation.receipt.thread_id,
                outcome: TurnOutcome::Completed,
                final_answer: "test result".to_string(),
                provider: "fake".to_string(),
                model: "fake".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    }
}

async fn fixture() -> (TempDir, Application, Thread, Arc<BlockingAdapter>) {
    let temp = tempfile::tempdir().expect("tempdir");
    let adapter = Arc::new(BlockingAdapter::new());
    let application = Application::builder()
        .home(temp.path())
        .database_path(":memory:")
        .agent_session_adapter(adapter.clone())
        .build()
        .await
        .expect("application");
    let mut request = StartThreadRequest::new(temp.path());
    request.source = "tui".to_string();
    let thread = application
        .client()
        .start_thread(request)
        .await
        .expect("Thread");
    make_native_history_eligible(&application, &thread, temp.path()).await;
    (temp, application, thread, adapter)
}

async fn make_native_history_eligible(application: &Application, thread: &Thread, cwd: &Path) {
    let cwd = cwd.display().to_string();
    application
        .inner
        .state
        .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
            thread_id: thread.id(),
            agent_ref: None,
            agent_fingerprint: "agent-fingerprint",
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(thread.id()),
            cwd: &cwd,
            profile_fingerprint: "profile-fingerprint",
            profile_revision: "profile-revision",
            profile_config_json: "{}",
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .await
        .expect("native binding");
}

async fn append_exact_editable_user(application: &Application, thread: &Thread) -> i64 {
    application
        .inner
        .state
        .append_message_with_undo_snapshot_metadata_and_context_evidence(
            thread.id(),
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

fn text_draft(text: &str) -> ThreadEditableDraft {
    ThreadEditableDraft {
        parts: vec![ThreadEditableDraftPart::Text {
            text: text.to_string(),
        }],
    }
}

fn assert_thread_busy(error: Error, blocking_operation: &str) {
    let data = error.structured_data().expect("structured Thread busy");
    assert_eq!(data["kind"], "thread_busy");
    assert_eq!(data["blockingOperation"], blocking_operation);
    assert_eq!(data["retryable"], true);
}

async fn shutdown(application: Application) {
    application
        .shutdown()
        .await
        .expect("shutdown")
        .require_clean()
        .expect("clean shutdown");
}

async fn application_session_count(application: &Application) -> i64 {
    let mut conn = application
        .inner
        .state
        .acquire_sqlx()
        .await
        .expect("connection");
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&mut *conn)
        .await
        .expect("session count")
}

#[tokio::test]
async fn public_history_editing_maps_exact_draft_stage_conflict_idempotence_and_restore() {
    let (_temp, application, thread, _adapter) = fixture().await;
    application
        .inner
        .state
        .set_session_metadata_field(thread.id(), "unrelated", Some(json!({"kept": true})))
        .await
        .expect("unrelated metadata");
    let boundary = append_exact_editable_user(&application, &thread).await;
    application
        .inner
        .state
        .append_message(
            thread.id(),
            &Message::User {
                content: vec![UserContentBlock::text("suffix")],
                timestamp_ms: 2,
            },
        )
        .await
        .expect("suffix");

    assert_eq!(
        thread.editable_draft(boundary).await.expect("draft"),
        ThreadEditableDraftReadOutcome::Available(ThreadEditableDraftRead {
            message_seq: boundary,
            draft: ThreadEditableDraft {
                parts: vec![
                    ThreadEditableDraftPart::Image {
                        input: ImageInput::ImageUrl("https://example.test/remote.png".to_string(),),
                    },
                    ThreadEditableDraftPart::Text {
                        text: "visible text".to_string(),
                    },
                    ThreadEditableDraftPart::Image {
                        input: ImageInput::LocalPath("local.png".into()),
                    },
                ],
            },
            fidelity: ThreadEditableDraftFidelity::Exact,
        })
    );
    assert_eq!(
        thread.history_editing_state().await.expect("initial state"),
        ThreadHistoryEditingState {
            eligibility: ThreadHistoryEditingEligibility::Eligible,
            staged: None,
        }
    );

    let replacement = text_draft("edited");
    assert_eq!(
        thread
            .stage_conversation_edit(boundary, replacement.clone())
            .await
            .expect("stage"),
        ThreadConversationEditStageOutcome::Staged
    );
    assert_eq!(
        thread.history_editing_state().await.expect("staged state"),
        ThreadHistoryEditingState {
            eligibility: ThreadHistoryEditingEligibility::Eligible,
            staged: Some(ThreadHistoryEditingStaged::ConversationEdit {
                boundary_message_seq: boundary,
                hidden_entry_count: 2,
                draft: replacement.clone(),
            }),
        }
    );
    assert_eq!(
        thread
            .stage_conversation_edit(boundary, replacement.clone())
            .await
            .expect("idempotent stage"),
        ThreadConversationEditStageOutcome::AlreadyStaged
    );
    assert_eq!(
        thread
            .stage_conversation_edit(boundary, text_draft("different"))
            .await
            .expect("conflicting stage"),
        ThreadConversationEditStageOutcome::Conflict(
            ThreadConversationEditConflict::ConversationEditStaged,
        )
    );
    assert_eq!(
        thread.restore_conversation_edit().await.expect("restore"),
        ThreadConversationEditRestoreOutcome::Restored(replacement)
    );
    assert_eq!(
        thread
            .restore_conversation_edit()
            .await
            .expect("idempotent restore"),
        ThreadConversationEditRestoreOutcome::NotStaged
    );
    assert_eq!(
        application
            .inner
            .state
            .session_metadata(thread.id())
            .await
            .expect("metadata")
            .and_then(|metadata| metadata.get("unrelated").cloned()),
        Some(json!({"kept": true}))
    );
    shutdown(application).await;
}

#[tokio::test]
async fn active_and_queued_turns_reject_public_edit_before_queue_or_sql() {
    let (_temp, application, thread, adapter) = fixture().await;
    let boundary = append_exact_editable_user(&application, &thread).await;

    let active = thread
        .start_turn(TurnRequest::new("active"))
        .await
        .expect("active Turn");
    adapter.wait_for_started(1).await;
    assert_eq!(
        thread
            .history_editing_state()
            .await
            .expect("busy state")
            .eligibility,
        ThreadHistoryEditingEligibility::Unavailable(ThreadHistoryEditingUnavailable::ThreadBusy)
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1
    );
    assert_thread_busy(
        thread
            .stage_conversation_edit(boundary, text_draft("active edit"))
            .await
            .expect_err("active Turn must reject edit"),
        "turn",
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1,
        "rejected edit must not enter the Thread queue"
    );
    assert!(
        application
            .inner
            .state
            .session_revert_state(thread.id())
            .await
            .expect("revert state")
            .is_none(),
        "rejected edit must not touch persistence"
    );
    adapter.release(1);
    active.wait().await.expect("active completion");

    let ordinary = application
        .inner
        .runtime
        .reserve_mutation(thread.id())
        .expect("ordinary reservation")
        .acquire()
        .await
        .expect("ordinary reservation ready");
    let queued = thread
        .start_turn(TurnRequest::new("queued"))
        .await
        .expect("queued Turn acceptance");
    assert_eq!(
        adapter.started_count.load(Ordering::SeqCst),
        1,
        "Turn must still be queued behind the ordinary mutation"
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        2
    );
    assert_thread_busy(
        thread
            .stage_conversation_edit(boundary, text_draft("queued edit"))
            .await
            .expect_err("queued Turn must reject edit"),
        "turn",
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        2,
        "rejected edit must not enter the Thread queue"
    );
    assert!(
        application
            .inner
            .state
            .session_revert_state(thread.id())
            .await
            .expect("revert state")
            .is_none()
    );
    drop(ordinary);
    adapter.wait_for_started(2).await;
    adapter.release(1);
    queued.wait().await.expect("queued completion");
    shutdown(application).await;
}

#[tokio::test]
async fn pending_acceptance_turn_rejects_edit_even_while_public_activity_is_idle() {
    let (_temp, application, thread, adapter) = fixture().await;
    let boundary = append_exact_editable_user(&application, &thread).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    application
        .inner
        .state
        .set_gateway_turn_acceptance_barrier_for_test(entered.clone(), release.clone());
    let turn_id = "pending-acceptance-turn".to_string();
    let start = tokio::spawn({
        let thread = thread.clone();
        let turn_id = turn_id.clone();
        async move {
            thread
                .start_turn(TurnRequest::new("pending").with_requested_turn_id(turn_id))
                .await
        }
    });
    entered.notified().await;

    assert!(!thread.activity().running);
    assert_eq!(
        thread
            .history_editing_state()
            .await
            .expect("pending acceptance state")
            .eligibility,
        ThreadHistoryEditingEligibility::Unavailable(ThreadHistoryEditingUnavailable::ThreadBusy)
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1
    );
    assert!(
        application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery read")
            .is_none()
    );
    assert_thread_busy(
        thread
            .stage_conversation_edit(boundary, text_draft("pending edit"))
            .await
            .expect_err("pending acceptance must reject edit"),
        "turn",
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1
    );
    assert!(
        application
            .inner
            .state
            .session_revert_state(thread.id())
            .await
            .expect("revert state")
            .is_none()
    );

    release.notify_one();
    let handle = start
        .await
        .expect("start task")
        .expect("Turn accepted after barrier");
    adapter.wait_for_started(1).await;
    adapter.release(1);
    handle.wait().await.expect("Turn completion");
    shutdown(application).await;
}

#[tokio::test]
async fn idle_reservation_projects_busy_and_blocks_turn_before_durable_acceptance() {
    let (_temp, application, thread, adapter) = fixture().await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let mutation = tokio::spawn({
        let thread = thread.clone();
        let entered = entered.clone();
        let release = release.clone();
        async move {
            thread
                .enqueue_idle_history_mutation(move |_| async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(())
                })
                .await
        }
    });
    entered.notified().await;
    assert_eq!(
        thread
            .history_editing_state()
            .await
            .expect("reserved state")
            .eligibility,
        ThreadHistoryEditingEligibility::Unavailable(ThreadHistoryEditingUnavailable::ThreadBusy)
    );

    let turn_id = "turn-after-history-reservation".to_string();
    assert_thread_busy(
        thread
            .start_turn(
                TurnRequest::new("blocked by history edit").with_requested_turn_id(turn_id.clone()),
            )
            .await
            .expect_err("idle reservation must reject Turn"),
        "history_editing",
    );
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1,
        "rejected Turn must not enter the Thread queue"
    );
    assert!(
        application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery read")
            .is_none(),
        "rejected Turn must fail before durable acceptance"
    );

    release.notify_one();
    mutation
        .await
        .expect("history mutation task")
        .expect("history mutation");
    let accepted = thread
        .start_turn(TurnRequest::new("after reservation"))
        .await
        .expect("Turn after reservation");
    adapter.wait_for_started(1).await;
    adapter.release(1);
    accepted.wait().await.expect("Turn completion");
    shutdown(application).await;
}

#[tokio::test]
async fn active_and_queued_turns_reject_public_fork_before_queue_or_sql() {
    let (_temp, application, thread, adapter) = fixture().await;

    let active = thread
        .start_turn(TurnRequest::new("active before fork"))
        .await
        .expect("active Turn");
    adapter.wait_for_started(1).await;
    let before = application_session_count(&application).await;
    assert_thread_busy(
        thread
            .fork(ForkThreadRequest::default())
            .await
            .expect_err("active Turn must reject fork"),
        "turn",
    );
    assert_eq!(application_session_count(&application).await, before);
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1,
        "rejected fork must not enter the Thread queue"
    );
    adapter.release(1);
    active.wait().await.expect("active completion");

    let ordinary = application
        .inner
        .runtime
        .reserve_mutation(thread.id())
        .expect("ordinary reservation")
        .acquire()
        .await
        .expect("ordinary reservation ready");
    let queued = thread
        .start_turn(TurnRequest::new("queued before fork"))
        .await
        .expect("queued Turn acceptance");
    let before = application_session_count(&application).await;
    assert_thread_busy(
        thread
            .fork(ForkThreadRequest::default())
            .await
            .expect_err("queued Turn must reject fork"),
        "turn",
    );
    assert_eq!(application_session_count(&application).await, before);
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        2,
        "rejected fork must not enter the Thread queue"
    );
    drop(ordinary);
    adapter.wait_for_started(2).await;
    adapter.release(1);
    queued.wait().await.expect("queued completion");
    shutdown(application).await;
}

#[tokio::test]
async fn public_fork_holds_idle_reservation_for_the_complete_store_future() {
    let (_temp, application, thread, adapter) = fixture().await;
    let before = application_session_count(&application).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    application
        .inner
        .state
        .set_native_history_fork_barrier_for_test(entered.clone(), release.clone());
    let fork = tokio::spawn({
        let thread = thread.clone();
        async move { thread.fork(ForkThreadRequest::default()).await }
    });

    entered.notified().await;
    assert_eq!(
        application
            .inner
            .runtime
            .thread_operation_count_for_test(thread.id()),
        1,
        "fork must reserve the idle lane before waiting on Store"
    );

    let turn_id = "turn-during-fork-store-future".to_string();
    assert_thread_busy(
        thread
            .start_turn(TurnRequest::new("blocked by fork").with_requested_turn_id(turn_id.clone()))
            .await
            .expect_err("fork reservation must reject Turn"),
        "history_editing",
    );
    assert!(
        application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery read")
            .is_none(),
        "rejected Turn must fail before durable acceptance"
    );
    assert_thread_busy(
        thread
            .fork(ForkThreadRequest::default())
            .await
            .expect_err("second fork must not queue"),
        "history_editing",
    );
    assert_eq!(application_session_count(&application).await, before);

    release.notify_one();
    let child = fork.await.expect("fork task").expect("fork after barrier");
    assert_ne!(child.id(), thread.id());
    assert_eq!(application_session_count(&application).await, before + 1);

    let accepted = thread
        .start_turn(TurnRequest::new("after fork"))
        .await
        .expect("Turn after fork");
    adapter.wait_for_started(1).await;
    adapter.release(1);
    accepted.wait().await.expect("Turn completion");
    shutdown(application).await;
}
