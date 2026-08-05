use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use psychevo::application::{
    AssistantBlock, Message, Outcome, PermissionApprovalRequest, RunStreamEvent, UserContentBlock,
};
use psychevo::{Application, PermissionMode, RunMode};
use serde_json::json;
use tokio::sync::{Notify, oneshot};
use uuid::Uuid;

use super::super::Gateway;
use super::super::activity::{
    SendTurnRequest, ThreadCallerContext, ThreadSurface, ThreadTurnIntent, ThreadTurnPolicy,
};
use super::super::results::GatewayTurnResult;
use crate::composition::GatewayApplication;
use crate::{FrameworkNativeTestExecutor, gateway_now_ms};
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewayInputPart, GatewaySource, GatewaySourceLifetime,
    GatewayThread, GatewayTurn, GatewayTurnStatus,
};

#[derive(Debug, Clone)]
pub(super) struct FakeRun {
    pub(super) prompt: String,
    pub(super) session: Option<String>,
    pub(super) cwd: PathBuf,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) mode: RunMode,
    pub(super) permission_mode: Option<PermissionMode>,
    pub(super) runtime_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(super) struct WaitFirst {
    run_number: usize,
    pub(super) started: Arc<Notify>,
    pub(super) release: Arc<Notify>,
}

#[derive(Default)]
struct FrameworkNativeProbeInner {
    runs: Mutex<Vec<FakeRun>>,
    binding_before_run: Mutex<Vec<bool>>,
    next_run: AtomicUsize,
    wait_first: Mutex<Option<WaitFirst>>,
    request_permission: AtomicBool,
    emit_stream_terminal: AtomicBool,
    persist_history: AtomicBool,
    fail_next: AtomicBool,
    hidden_messages_next: AtomicUsize,
    context_snapshot: Mutex<Option<psychevo::context_usage::ContextSnapshot>>,
}

#[derive(Clone, Default)]
pub(super) struct FrameworkNativeProbe {
    inner: Arc<FrameworkNativeProbeInner>,
}

impl FrameworkNativeProbe {
    pub(super) fn runs(&self) -> Vec<FakeRun> {
        self.inner
            .runs
            .lock()
            .expect("fake run lock poisoned")
            .clone()
    }

    pub(super) fn binding_before_run(&self) -> Vec<bool> {
        self.inner
            .binding_before_run
            .lock()
            .expect("fake binding observation lock poisoned")
            .clone()
    }

    pub(super) fn wait_on_first_run(&self) -> WaitFirst {
        self.wait_on_next_run()
    }

    pub(super) fn wait_on_next_run(&self) -> WaitFirst {
        let run_number = self.inner.next_run.load(Ordering::SeqCst) + 1;
        let wait = WaitFirst {
            run_number,
            started: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        *self
            .inner
            .wait_first
            .lock()
            .expect("fake wait lock poisoned") = Some(wait.clone());
        wait
    }

    pub(super) fn request_permission(&self) {
        self.inner.request_permission.store(true, Ordering::SeqCst);
    }

    pub(super) fn emit_stream_terminal(&self) {
        self.inner
            .emit_stream_terminal
            .store(true, Ordering::SeqCst);
    }

    pub(super) fn persist_history(&self) {
        self.inner.persist_history.store(true, Ordering::SeqCst);
    }

    pub(super) fn fail_next(&self) {
        self.inner.fail_next.store(true, Ordering::SeqCst);
    }

    pub(super) fn append_hidden_messages_on_next(&self, count: usize) {
        self.inner
            .hidden_messages_next
            .store(count, Ordering::SeqCst);
    }
}

impl fmt::Debug for FrameworkNativeProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FrameworkNativeProbe")
    }
}

impl FrameworkNativeProbe {
    pub(super) fn executor(&self) -> FrameworkNativeTestExecutor {
        let inner = Arc::clone(&self.inner);
        Arc::new(move |invocation| {
            let inner = Arc::clone(&inner);
            Box::pin(async move {
                invocation.persistence.confirm_delivery().await?;
                let run_number = inner.next_run.fetch_add(1, Ordering::SeqCst) + 1;
                let session_id = invocation.receipt.thread_id.clone();
                let binding_before_run = invocation.binding.as_ref().is_some_and(|binding| {
                    binding.runtime_ref == "native"
                        && binding.native_session_id.as_deref() == Some(session_id.as_str())
                });
                inner
                    .binding_before_run
                    .lock()
                    .expect("fake binding observation lock poisoned")
                    .push(binding_before_run);
                {
                    let mut runs = inner.runs.lock().expect("fake run lock poisoned");
                    runs.push(FakeRun {
                        prompt: invocation.input.prompt.clone(),
                        session: Some(session_id.clone()),
                        cwd: PathBuf::from(&invocation.thread.cwd),
                        model: invocation.model.model.clone(),
                        reasoning_effort: invocation.model.reasoning_effort.clone(),
                        mode: invocation.execution.mode,
                        permission_mode: invocation.execution.permission_mode,
                        runtime_options: invocation.target.runtime_options.clone(),
                    });
                }

                let wait_first = inner
                    .wait_first
                    .lock()
                    .expect("fake wait lock poisoned")
                    .clone();
                let mut aborted = false;
                if let Some(wait) = wait_first
                    && run_number == wait.run_number
                {
                    wait.started.notify_one();
                    tokio::select! {
                        _ = wait.release.notified() => {}
                        _ = invocation.control.wait_for_interrupt() => aborted = true,
                    }
                }

                if !aborted
                    && inner.request_permission.swap(false, Ordering::SeqCst)
                    && let Some(handler) = invocation.execution.approval_handler.clone()
                {
                    let _decision = handler
                        .request_permission(PermissionApprovalRequest {
                            tool_call_id: "permission-1".to_string(),
                            tool_name: "fake_tool".to_string(),
                            summary: "fake permission".to_string(),
                            reason: "test permission".to_string(),
                            matched_rule: None,
                            suggested_rule: None,
                            allow_always: true,
                            filesystem: None,
                            mcp_startup: None,
                            timeout_secs: 300,
                        })
                        .await;
                }

                let outcome = if aborted {
                    Outcome::Aborted
                } else {
                    Outcome::Normal
                };
                if inner.fail_next.swap(false, Ordering::SeqCst) {
                    return Err(psychevo::Error::Message(
                        "injected transcript pagination failure".to_string(),
                    ));
                }
                let final_answer = format!("answer {run_number}");
                let hidden_message_count = inner.hidden_messages_next.swap(0, Ordering::SeqCst);
                if outcome == Outcome::Normal && hidden_message_count > 0 {
                    let timestamp_ms = crate::gateway_now_ms();
                    for index in 0..hidden_message_count {
                        invocation
                            .persistence
                            .append_message_with_metrics(
                                Message::User {
                                    content: vec![UserContentBlock::text(format!(
                                        "hidden context {index}"
                                    ))],
                                    timestamp_ms: timestamp_ms.saturating_add(index as i64),
                                },
                                None,
                                Some(json!({
                                    "side_inherited": {"hidden": true}
                                })),
                            )
                            .await?;
                    }
                } else if outcome == Outcome::Normal && inner.persist_history.load(Ordering::SeqCst)
                {
                    let timestamp_ms = crate::gateway_now_ms();
                    invocation
                        .persistence
                        .append_message(Message::User {
                            content: vec![UserContentBlock::text(invocation.input.prompt.clone())],
                            timestamp_ms,
                        })
                        .await?;
                    invocation
                        .persistence
                        .append_message(Message::Assistant {
                            content: vec![AssistantBlock::Text {
                                text: final_answer.clone(),
                            }],
                            timestamp_ms: timestamp_ms.saturating_add(1),
                            finish_reason: Some("stop".to_string()),
                            outcome,
                            model: Some("fake-model".to_string()),
                            provider: Some("fake-provider".to_string()),
                        })
                        .await?;
                }
                if inner.emit_stream_terminal.load(Ordering::SeqCst) {
                    invocation
                        .events
                        .emit_agent_event(RunStreamEvent::value(json!({
                            "type": "turn_complete",
                            "session_id": session_id.clone(),
                            "source": "native_conformance_fake",
                            "outcome": outcome.as_str(),
                        })));
                }

                Ok(psychevo::TurnResult {
                    thread_id: session_id,
                    outcome: if aborted {
                        psychevo::TurnOutcome::Interrupted
                    } else {
                        psychevo::TurnOutcome::Completed
                    },
                    terminal_reason: None,
                    final_answer,
                    provider: "fake-provider".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    context_limit: None,
                    tool_failures: 0,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                    context_snapshot: inner
                        .context_snapshot
                        .lock()
                        .expect("fake context snapshot lock poisoned")
                        .clone(),
                    terminal_error: None,
                    warnings: Vec::new(),
                })
            })
        })
    }
}

pub(super) struct Harness {
    pub(super) _temp: tempfile::TempDir,
    pub(super) cwd: PathBuf,
    pub(super) db_path: PathBuf,
    pub(super) durability: psychevo::application::GatewayDurability,
    pub(super) gateway: Gateway,
    pub(super) _application: Application,
}

impl Harness {
    pub(super) async fn send(
        &self,
        request: SendTurnRequest,
    ) -> psychevo::Result<GatewayTurnResult> {
        send_framework_turn(self._application.clone(), self.gateway.clone(), request).await
    }

    pub(super) fn runner(&self) -> (Application, Gateway) {
        (self._application.clone(), self.gateway.clone())
    }
}

pub(super) async fn harness(backend: Arc<FrameworkNativeProbe>) -> Harness {
    harness_with_native_test_executor(Some(backend.executor())).await
}

pub(super) async fn native_provider_harness() -> Harness {
    harness_with_native_test_executor(None).await
}

async fn harness_with_native_test_executor(
    native_test_executor: Option<FrameworkNativeTestExecutor>,
) -> Harness {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let db_path = temp.path().join("state.db");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let inherited_env = if native_test_executor.is_some() {
        BTreeMap::new()
    } else {
        BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().into_owned(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().into_owned(),
            ),
        ])
    };
    let runtime = match native_test_executor {
        Some(executor) => {
            GatewayApplication::open_with_native_test_executor(
                home.clone(),
                db_path.clone(),
                None,
                inherited_env,
                executor,
            )
            .await
        }
        None => GatewayApplication::open(home.clone(), db_path.clone(), None, inherited_env).await,
    }
    .expect("test composition");
    let application = runtime.application().clone();
    let durability = application.gateway_durability();
    let gateway = runtime.gateway().clone();
    Harness {
        _temp: temp,
        cwd,
        db_path,
        durability,
        gateway,
        _application: application,
    }
}

pub(super) async fn send_framework_turn(
    application: Application,
    gateway: Gateway,
    request: SendTurnRequest,
) -> psychevo::Result<GatewayTurnResult> {
    send_framework_turn_inner(application, gateway, request, None, None).await
}

pub(super) async fn send_framework_turn_with_id(
    application: Application,
    gateway: Gateway,
    request: SendTurnRequest,
    turn_id: String,
) -> psychevo::Result<GatewayTurnResult> {
    send_framework_turn_inner(application, gateway, request, Some(turn_id), None).await
}

pub(super) async fn send_framework_turn_with_handle(
    application: Application,
    gateway: Gateway,
    request: SendTurnRequest,
    accepted_handle: oneshot::Sender<psychevo::TurnHandle>,
) -> psychevo::Result<GatewayTurnResult> {
    send_framework_turn_inner(application, gateway, request, None, Some(accepted_handle)).await
}

async fn send_framework_turn_inner(
    application: Application,
    gateway: Gateway,
    mut request: SendTurnRequest,
    turn_id: Option<String>,
    accepted_handle: Option<oneshot::Sender<psychevo::TurnHandle>>,
) -> psychevo::Result<GatewayTurnResult> {
    let client = application.client();
    let explicit_thread_id = request.thread_id.clone();
    let mapped_thread_id = if explicit_thread_id.is_none()
        && !request.reset_source_binding
        && let Some(source) = request.source.as_ref()
    {
        gateway.resolve_source_thread(source).await?
    } else {
        None
    };
    let continued_thread_id = if explicit_thread_id.is_none()
        && mapped_thread_id.is_none()
        && request.policy.continue_latest
    {
        let continue_sources = if request.continue_sources.is_empty() {
            vec![request.runtime_source.as_deref().unwrap_or("test")]
        } else {
            request
                .continue_sources
                .iter()
                .map(String::as_str)
                .collect()
        };
        client
            .list_threads(psychevo::ThreadListQuery {
                cwd: Some(request.cwd.clone()),
                archived: false,
                sources: continue_sources.into_iter().map(str::to_string).collect(),
                cursor: None,
                limit: 1,
            })
            .await?
            .threads
            .into_iter()
            .next()
            .map(|thread| thread.id)
    } else {
        None
    };
    let thread = if let Some(thread_id) = explicit_thread_id
        .or(mapped_thread_id)
        .or(continued_thread_id)
    {
        client.resume_thread(&thread_id).await?
    } else {
        let mut start = psychevo::StartThreadRequest::new(&request.cwd);
        start.source = request
            .runtime_source
            .clone()
            .unwrap_or_else(|| "test".to_string());
        start.metadata = request.lineage.clone();
        let thread = client.start_thread(start).await?;
        if let Some(source) = request.bind_source.as_ref().or(request.source.as_ref())
            && source.lifetime != GatewaySourceLifetime::Invocation
        {
            gateway
                .bind_source_thread(
                    source,
                    thread.id(),
                    &GatewayBackendInfo {
                        kind: BackendKind::Native,
                        runtime_ref: request.policy.runtime_profile_ref.clone(),
                        native_id: None,
                    },
                    request.lineage.clone(),
                )
                .await?;
        }
        thread
    };

    let input = std::mem::take(&mut request.input);
    let mut caller = ThreadCallerContext::new(
        ThreadSurface::Other("conformance".to_string()),
        request.cwd.clone(),
    );
    caller.runtime_source = request
        .runtime_source
        .take()
        .unwrap_or_else(|| "test".to_string());
    caller.continue_sources = std::mem::take(&mut request.continue_sources);
    if let Some(observer) = request.turn_events.take() {
        caller.set_turn_event_observer(observer);
    }
    if let Some(event_sink) = request.event_sink.take() {
        caller.set_event_observer(event_sink);
    }
    if let Some(workspace_mutations) = request.workspace_mutations.take() {
        caller.set_workspace_mutations(workspace_mutations);
    }
    caller.set_runtime_tools(std::mem::take(&mut request.runtime_tools));
    let mut intent = ThreadTurnIntent::new(input);
    intent.thread_id = Some(thread.id().to_string());
    intent.source = request.source;
    intent.turn_id = Some(turn_id.unwrap_or_else(|| Uuid::now_v7().to_string()));
    request.policy.continue_latest = false;
    intent.policy = request.policy;
    let submission = intent.into_framework_request(caller)?;
    let observers = submission.observers;
    let handle = thread.start_turn(submission.request).await?;
    observers.attach(&gateway, handle.clone());
    if let Some(accepted_handle) = accepted_handle {
        let _ = accepted_handle.send(handle.clone());
    }
    let receipt = handle.receipt().clone();
    let result = handle.wait().await?;
    let outcome = match result.outcome {
        psychevo::TurnOutcome::Completed => Outcome::Normal,
        psychevo::TurnOutcome::Stopped => Outcome::Stopped,
        psychevo::TurnOutcome::Failed => Outcome::Failed,
        psychevo::TurnOutcome::Interrupted => Outcome::Aborted,
    };
    let status = match outcome {
        Outcome::Normal => GatewayTurnStatus::Completed,
        Outcome::Stopped | Outcome::Aborted => GatewayTurnStatus::Interrupted,
        Outcome::Failed => GatewayTurnStatus::Failed,
    };
    let committed_entries = gateway.thread_transcript(&receipt.thread_id).await?;
    Ok(GatewayTurnResult {
        thread: GatewayThread {
            id: receipt.thread_id.clone(),
            backend: GatewayBackendInfo {
                kind: BackendKind::Native,
                runtime_ref: None,
                native_id: None,
            },
            source_key: None,
            forked_from_thread_id: None,
        },
        turn: GatewayTurn {
            id: receipt.turn_id,
            thread_id: Some(receipt.thread_id.clone()),
            status,
            outcome: Some(outcome.as_str().to_string()),
            error: None,
            started_at_ms: None,
            completed_at_ms: Some(gateway_now_ms()),
        },
        result,
        committed_entries,
    })
}

pub(super) async fn compose_test_application(
    harness: &Harness,
    executor: FrameworkNativeTestExecutor,
) -> (Application, Gateway) {
    let home = harness._temp.path().join("home");
    let runtime = GatewayApplication::open_with_native_test_executor(
        home.clone(),
        harness.db_path.clone(),
        None,
        BTreeMap::new(),
        executor,
    )
    .await
    .expect("test composition");
    (runtime.application().clone(), runtime.gateway().clone())
}

pub(super) fn test_acp_command_toml(cwd: &std::path::Path) -> String {
    let fixture = crate::test_support::acp_fixture(cwd, "fake_acp_lifecycle");
    crate::test_support::toml_path(&fixture.program)
}

pub(super) fn copied_acp_fixture(
    cwd: &std::path::Path,
    directory: &std::path::Path,
    name: &str,
    target_stem: &str,
) -> crate::test_support::AcpFixture {
    let fixture = crate::test_support::acp_fixture(cwd, name);
    let target = directory
        .join(target_stem)
        .with_extension(fixture.script.extension().expect("ACP fixture extension"));
    std::fs::copy(&fixture.script, &target).expect("copy ACP test fixture");
    crate::test_support::AcpFixture {
        program: fixture.program,
        script: target,
    }
}

pub(super) fn request(harness: &Harness, source: GatewaySource, prompt: &str) -> SendTurnRequest {
    SendTurnRequest {
        thread_id: None,
        source: Some(source),
        bind_source: None,
        reset_source_binding: false,
        input: vec![GatewayInputPart::Text {
            text: prompt.to_string(),
        }],
        cwd: harness.cwd.clone(),
        policy: ThreadTurnPolicy {
            permission_mode: Some(PermissionMode::Default),
            ..ThreadTurnPolicy::default()
        },
        workspace_mutations: None,
        runtime_tools: Vec::new(),
        runtime_source: Some("test".to_string()),
        continue_sources: vec!["test".to_string()],
        turn_events: None,
        event_sink: None,
        lineage: None,
    }
}
