use psychevo::{Error, application::GatewayActivityTerminalStatus};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::Gateway;
use super::activity::{
    ActiveActivityControl, ActiveActivityKind, HistoryMutationReservation, PendingQueuedActivity,
    PendingQueuedShell, SendShellRequest,
};
use super::results::GatewayShellResult;
use super::stream_input::{source_key_key, thread_key};
use super::supervisor::{GatewayActivityPermit, GatewayTaskOutcome};
use psychevo_gateway_protocol::source::{
    GatewaySource, GatewaySourceLifetime, GatewayThreadSelector, SourceKey,
};

impl Gateway {
    pub(crate) fn reserve_history_mutation(
        &self,
        thread_id: &str,
        shell_busy_message: &str,
    ) -> psychevo::Result<HistoryMutationReservation> {
        let mut queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        let alias = thread_key(thread_id);
        let queue_key = queue.aliases.get(&alias).cloned().unwrap_or(alias);
        let state = queue.activities.entry(queue_key.clone()).or_default();
        if state.running || !state.queued.is_empty() {
            return Err(gateway_thread_busy(thread_id, "shell", shell_busy_message));
        }
        if state.history_mutation_reserved {
            return Err(gateway_thread_busy(
                thread_id,
                "history_editing",
                "Finish the current history operation before starting another edit or fork.",
            ));
        }
        state.history_mutation_reserved = true;
        drop(queue);
        Ok(HistoryMutationReservation::new(
            self.active_queue.clone(),
            queue_key,
        ))
    }

    pub(crate) fn local_history_editing_unavailable_reason(
        &self,
        thread_id: &str,
    ) -> Option<String> {
        let queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        let alias = thread_key(thread_id);
        let queue_key = queue.aliases.get(&alias).unwrap_or(&alias);
        queue.activities.get(queue_key).and_then(|state| {
            if state.running || !state.queued.is_empty() {
                Some(
                    "Finish the running turn before editing or forking conversation history."
                        .to_string(),
                )
            } else if state.history_mutation_reserved {
                Some(
                    "Finish the current history operation before starting another edit or fork."
                        .to_string(),
                )
            } else {
                None
            }
        })
    }

    pub(super) fn spawn_shell_activity(
        &self,
        queue_key: String,
        shell_id: String,
        request: SendShellRequest,
        permit: GatewayActivityPermit,
        responder: oneshot::Sender<psychevo::Result<GatewayShellResult>>,
    ) {
        let gateway = self.clone();
        let task_name = format!("shell:{shell_id}");
        let execution_gateway = gateway.clone();
        let execution_queue_key = queue_key.clone();
        let execution_shell_id = shell_id.clone();
        self.supervisor.spawn_finalizer_owned_activity(
            task_name,
            permit,
            async move {
                execution_gateway
                    .run_shell_now(&execution_queue_key, request, execution_shell_id)
                    .await
            },
            move |outcome, permit| async move {
                let (mut result, abandoned_status) = match outcome {
                    GatewayTaskOutcome::Completed(Ok(execution)) => {
                        (gateway.finalize_shell_execution(execution).await, None)
                    }
                    GatewayTaskOutcome::Completed(Err(error)) => {
                        (Err(error), Some(GatewayActivityTerminalStatus::Failed))
                    }
                    GatewayTaskOutcome::Cancelled => (
                        Err(Error::Message(format!(
                            "Gateway Shell activity `{shell_id}` was interrupted by forced shutdown"
                        ))),
                        Some(GatewayActivityTerminalStatus::Interrupted),
                    ),
                    GatewayTaskOutcome::Panicked(panic) => {
                        let message = format!(
                            "Gateway Shell activity `{shell_id}` panicked: {}",
                            panic.message
                        );
                        (
                            Err(Error::Message(message)),
                            Some(GatewayActivityTerminalStatus::Failed),
                        )
                    }
                };
                if let Some(status) = abandoned_status
                    && let Err(finalize_error) = gateway
                        .finalize_abandoned_shell_activity(&shell_id, status)
                        .await
                {
                    gateway.shell_activity_runtime.record_failure(format!(
                        "Gateway Shell activity `{shell_id}` abandoned finalization failed: {finalize_error}"
                    ));
                    result = Err(Error::Message(format!(
                        "{}; Gateway Shell finalization also failed: {finalize_error}",
                        result
                            .as_ref()
                            .err()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "Gateway Shell activity was abandoned".to_string())
                    )));
                }
                gateway.finish_activity_and_spawn_next(queue_key);
                drop(permit);
                let _ = responder.send(result);
            },
        );
    }

    pub(super) fn cancel_active_queue(&self) {
        let mut queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        queue.activities.retain(|_, state| {
            if !state.history_mutation_reserved {
                return false;
            }
            state.running = false;
            state.active_turn_id = None;
            state.active_kind = None;
            state.control = None;
            state.queued.clear();
            true
        });
        let retained = queue
            .activities
            .keys()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        queue
            .aliases
            .retain(|_, primary| retained.contains(primary));
    }

    fn finish_activity_and_spawn_next(&self, queue_key: String) {
        let next = {
            let mut queue = self
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let Some(state) = queue.activities.get_mut(&queue_key) else {
                return;
            };
            state.control = None;
            state.active_turn_id = None;
            state.active_kind = None;
            if let Some(next) = state.queued.pop_front() {
                state.running = true;
                Some(next)
            } else {
                queue.activities.remove(&queue_key);
                queue.aliases.retain(|_, primary| primary != &queue_key);
                None
            }
        };
        if let Some(PendingQueuedActivity::Shell(next)) = next {
            let PendingQueuedShell {
                shell_id,
                queued_at_ms: _,
                request,
                permit,
                responder,
            } = *next;
            self.spawn_shell_activity(queue_key, shell_id, request, permit, responder);
        }
    }

    pub(super) async fn queue_key_for_shell_request(
        &self,
        request: &SendShellRequest,
    ) -> psychevo::Result<(String, Option<String>)> {
        if let Some(thread_id) = &request.thread_id {
            return Ok((
                self.primary_queue_key_for_alias(thread_key(thread_id)),
                Some(thread_id.clone()),
            ));
        }
        if let Some(source) = &request.source {
            if let Some(thread_id) = self.lookup_source_thread(source).await? {
                return Ok((
                    self.primary_queue_key_for_alias(thread_key(&thread_id)),
                    Some(thread_id),
                ));
            }
            return Ok((
                self.primary_queue_key_for_alias(source_key_key(&source.source_key())),
                None,
            ));
        }
        Ok((format!("shell:{}", Uuid::now_v7()), None))
    }

    pub(super) async fn lookup_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo::Result<Option<String>> {
        match source.lifetime {
            GatewaySourceLifetime::Invocation => Ok(None),
            GatewaySourceLifetime::Process => Ok(self
                .process_bindings
                .lock()
                .expect("gateway process binding map poisoned")
                .get(&source.source_key().0)
                .cloned()),
            GatewaySourceLifetime::Persistent => Ok(self
                .durability
                .gateway_source_lane(&source.source_key().0)
                .await?
                .and_then(|lane| lane.thread_id)),
        }
    }

    pub(super) fn source_generation(&self, source: &GatewaySource) -> u64 {
        let key = source.source_key();
        self.source_generations
            .lock()
            .expect("gateway source generation map poisoned")
            .get(&key.0)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn bump_source_generation_key(&self, source_key: &SourceKey) {
        let mut generations = self
            .source_generations
            .lock()
            .expect("gateway source generation map poisoned");
        let generation = generations.entry(source_key.0.clone()).or_default();
        *generation = generation.saturating_add(1);
    }

    pub(super) fn register_active(
        &self,
        key: &str,
        activity_id: String,
        control: Option<ActiveActivityControl>,
        kind: ActiveActivityKind,
    ) {
        let mut queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        let state = queue.activities.entry(key.to_string()).or_default();
        state.active_turn_id = Some(activity_id);
        state.control = control;
        state.active_kind = Some(kind);
    }

    pub(super) fn register_active_thread_alias(
        &self,
        key: &str,
        thread_id: &str,
    ) -> psychevo::Result<()> {
        let alias = thread_key(thread_id);
        if alias == key {
            return Ok(());
        }
        let mut queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        if queue.activities.contains_key(&alias) {
            return Err(gateway_thread_busy(
                thread_id,
                "history_editing",
                "Finish the history operation before continuing the Shell command.",
            ));
        }
        queue.aliases.insert(alias, key.to_string());
        Ok(())
    }

    pub(super) fn register_active_queue_alias(&self, alias: &str, primary: &str) {
        if alias != primary {
            self.active_queue
                .lock()
                .expect("gateway active queue poisoned")
                .aliases
                .insert(alias.to_string(), primary.to_string());
        }
    }

    fn primary_queue_key_for_alias(&self, key: String) -> String {
        self.active_queue
            .lock()
            .expect("gateway active queue poisoned")
            .aliases
            .get(&key)
            .cloned()
            .unwrap_or(key)
    }

    pub(super) fn selector_keys(&self, selector: &GatewayThreadSelector) -> Vec<String> {
        match selector {
            GatewayThreadSelector::ThreadId { thread_id } => vec![thread_key(thread_id)],
            GatewayThreadSelector::Source { source_key } => {
                let mut keys = vec![source_key_key(source_key)];
                if let Some(thread_id) = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .get(&source_key.0)
                    .cloned()
                {
                    keys.push(thread_key(&thread_id));
                }
                keys
            }
        }
    }
}

pub(super) fn gateway_thread_busy(
    thread_id: &str,
    blocking_operation: &str,
    message: &str,
) -> Error {
    Error::structured(
        message,
        serde_json::json!({
            "kind": "thread_busy",
            "threadId": thread_id,
            "blockingOperation": blocking_operation,
            "retryable": true,
        }),
    )
}

#[cfg(test)]
mod history_mutation_tests {
    use std::sync::Arc;

    use psychevo::application::{
        Application, GatewayDurability, Message, Thread, user_text_message,
    };
    use psychevo_gateway_protocol as wire;

    use super::super::activity::{SendShellRequest, ShellExecutionIntent};
    use super::super::stream_input::thread_key;
    use super::Gateway;
    use crate::composition::GatewayApplication;
    use crate::history_editing::HistoryEditingSurface;

    struct Fixture {
        _temp: tempfile::TempDir,
        durability: GatewayDurability,
        gateway: Gateway,
        application: Application,
        thread: Thread,
        thread_id: String,
        message_id: String,
    }

    async fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let executor: crate::FrameworkNativeTestExecutor = Arc::new(|invocation| {
            Box::pin(async move {
                invocation.persistence.confirm_delivery().await?;
                invocation
                    .persistence
                    .append_message(user_text_message(invocation.input.prompt.clone()))
                    .await?;
                Ok(psychevo::TurnResult {
                    thread_id: invocation.receipt.thread_id,
                    outcome: psychevo::TurnOutcome::Completed,
                    final_answer: String::new(),
                    provider: "fixture-provider".to_string(),
                    model: "fixture-model".to_string(),
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
        });
        let runtime = GatewayApplication::open_with_native_test_executor(
            home.clone(),
            temp.path().join("state.db"),
            None,
            Default::default(),
            executor,
        )
        .await
        .expect("test composition");
        let application = runtime.application().clone();
        let gateway = runtime.gateway().clone();
        let durability = application.gateway_durability();
        let mut start = psychevo::StartThreadRequest::new(&cwd);
        start.source = "web".to_string();
        let thread = runtime.client().start_thread(start).await.expect("Thread");
        thread
            .start_turn(psychevo::TurnRequest::new("original"))
            .await
            .expect("accepted fixture turn")
            .wait()
            .await
            .expect("completed fixture turn");
        let history = thread
            .history()
            .latest(Some(200))
            .await
            .expect("fixture history")
            .items;
        let message_seq = history
            .iter()
            .find(|item| matches!(&item.message, Message::User { .. }))
            .expect("user fixture message")
            .session_seq;
        let thread_id = thread.id().to_string();
        Fixture {
            _temp: temp,
            durability,
            gateway,
            application,
            thread,
            thread_id,
            message_id: format!("message:{message_seq}"),
        }
    }

    fn replacement() -> wire::thread_command_turn::ThreadEditableDraft {
        wire::thread_command_turn::ThreadEditableDraft {
            parts: vec![wire::thread_command_turn::ThreadEditableInputPart::Text {
                text: "edited".to_string(),
            }],
        }
    }

    fn shell_request(fixture: &Fixture) -> SendShellRequest {
        SendShellRequest {
            thread_id: Some(fixture.thread_id.clone()),
            source: None,
            bind_source: None,
            cwd: fixture._temp.path().join("workspace"),
            command: "must-not-run".to_string(),
            execution: ShellExecutionIntent::new("test"),
            event_sink: None,
            lineage: None,
        }
    }

    fn assert_busy(error: psychevo::Error, blocking_operation: &str) {
        let data = error.structured_data().expect("structured busy");
        assert_eq!(data["kind"], "thread_busy");
        assert_eq!(data["blockingOperation"], blocking_operation);
        assert_eq!(data["retryable"], true);
    }

    async fn shutdown(fixture: Fixture) {
        fixture
            .gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("Gateway shutdown");
        fixture
            .application
            .shutdown()
            .await
            .expect("Application shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn shell_first_rejects_history_mutation_without_staging_or_queueing() {
        let fixture = fixture().await;
        let queue_key = thread_key(&fixture.thread_id);
        {
            let mut active = fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            active
                .activities
                .entry(queue_key.clone())
                .or_default()
                .running = true;
        }

        let error = fixture
            .gateway
            .stage_native_conversation_edit(
                &fixture.thread_id,
                &fixture.message_id,
                &replacement(),
                HistoryEditingSurface::Workbench,
            )
            .await
            .expect_err("Shell must block history mutation");
        assert_busy(error, "shell");
        {
            let active = fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let state = active.activities.get(&queue_key).expect("Shell state");
            assert!(state.running);
            assert!(state.queued.is_empty());
            assert!(!state.history_mutation_reserved);
        }
        assert!(
            fixture
                .thread
                .history_editing_state()
                .await
                .expect("history editing state")
                .staged
                .is_none()
        );
        assert!(
            fixture
                .durability
                .active_gateway_activity_for_thread(&fixture.thread_id)
                .await
                .expect("durable activity")
                .is_none()
        );
        fixture
            .gateway
            .active_queue
            .lock()
            .expect("gateway active queue poisoned")
            .activities
            .remove(&queue_key);
        shutdown(fixture).await;
    }

    #[tokio::test]
    async fn shell_first_rejects_native_fork_without_creating_a_child() {
        let fixture = fixture().await;
        let queue_key = thread_key(&fixture.thread_id);
        fixture
            .gateway
            .active_queue
            .lock()
            .expect("gateway active queue poisoned")
            .activities
            .entry(queue_key.clone())
            .or_default()
            .running = true;
        let before = fixture
            .application
            .client()
            .list_threads(psychevo::ThreadListQuery::default())
            .await
            .expect("Thread list")
            .threads
            .len();

        let error = fixture
            .gateway
            .fork_native_history(&fixture.thread_id, None, HistoryEditingSurface::Workbench)
            .await
            .expect_err("Shell must block Native fork");
        assert_busy(error, "shell");
        assert_eq!(
            fixture
                .application
                .client()
                .list_threads(psychevo::ThreadListQuery::default())
                .await
                .expect("Thread list")
                .threads
                .len(),
            before,
            "rejected Native fork created a child"
        );

        fixture
            .gateway
            .active_queue
            .lock()
            .expect("gateway active queue poisoned")
            .activities
            .remove(&queue_key);
        shutdown(fixture).await;
    }

    #[tokio::test]
    async fn history_mutation_first_rejects_shell_without_queue_or_durable_side_effect() {
        let fixture = fixture().await;
        let queue_key = thread_key(&fixture.thread_id);
        let reservation = fixture
            .gateway
            .reserve_history_mutation(
                &fixture.thread_id,
                "Finish the running turn before editing or forking conversation history.",
            )
            .expect("history reservation");

        let error = fixture
            .gateway
            .send_shell(shell_request(&fixture))
            .await
            .expect_err("history mutation must block Shell");
        assert_busy(error, "history_editing");
        {
            let active = fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let state = active.activities.get(&queue_key).expect("history state");
            assert!(!state.running);
            assert!(state.queued.is_empty());
            assert!(state.history_mutation_reserved);
        }
        assert_eq!(
            fixture
                .gateway
                .shell_activity_diagnostics()
                .admitted_activities,
            0,
            "rejected Shell admission must release its supervisor permit"
        );
        assert!(
            fixture
                .durability
                .active_gateway_activity_for_thread(&fixture.thread_id)
                .await
                .expect("durable activity")
                .is_none()
        );
        assert!(
            fixture
                .thread
                .history_editing_state()
                .await
                .expect("history editing state")
                .staged
                .is_none()
        );

        drop(reservation);
        assert!(
            !fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned")
                .activities
                .contains_key(&queue_key),
            "reservation drop must remove the otherwise-empty active state"
        );
        shutdown(fixture).await;
    }

    #[tokio::test]
    async fn forced_queue_cancellation_preserves_a_live_history_reservation() {
        let fixture = fixture().await;
        let queue_key = thread_key(&fixture.thread_id);
        let reservation = fixture
            .gateway
            .reserve_history_mutation(
                &fixture.thread_id,
                "Finish the Shell before editing conversation history.",
            )
            .expect("history reservation");

        fixture.gateway.cancel_active_queue();
        {
            let active = fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            assert!(
                active
                    .activities
                    .get(&queue_key)
                    .is_some_and(|state| state.history_mutation_reserved),
                "forced Shell cancellation must not shorten a history Store future"
            );
        }
        assert_busy(
            fixture
                .gateway
                .reserve_history_mutation(
                    &fixture.thread_id,
                    "Finish the Shell before editing conversation history.",
                )
                .expect_err("second reservation must remain blocked"),
            "history_editing",
        );

        drop(reservation);
        assert!(
            !fixture
                .gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned")
                .activities
                .contains_key(&queue_key)
        );
        shutdown(fixture).await;
    }
}
