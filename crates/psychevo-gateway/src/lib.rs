pub mod im;
pub mod protocol;
pub mod server;

mod acp_peer;
mod projection;
mod transcript;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::future::BoxFuture;
use psychevo_runtime::{
    AgentDiscoveryOptions, AgentEntrypoint, ApprovalHandler, ClarifyResult, Error,
    GatewaySourceBindingInput, ImageInput, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionApprovalRequest, RunControl, RunControlHandle, RunOptions, RunResult, RunStreamEvent,
    RunStreamSink, StateRuntime, UserShellContextOptions, UserShellOptions, UserShellResult,
    discover_agents, load_agent_backend_configs, resolve_agent_definition, resolve_skills_home,
    run_control, run_live, run_live_streaming, run_live_streaming_controlled,
    run_user_shell_command_streaming_controlled,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::timeout;
use uuid::Uuid;

use projection::GatewayLiveProjector;
pub use projection::gateway_event_from_run_stream;
pub use protocol::{
    BackendKind, GatewayBackendInfo, GatewayEvent, GatewayImageInput, GatewayInputPart,
    GatewaySelectedSkill, GatewaySource, GatewaySourceLifetime, GatewayThread,
    GatewayThreadSelector, GatewayTurn, GatewayTurnStatus, PermissionDecision, SourceKey,
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole, TranscriptToolResult,
};
pub use server::{BoundGatewayWebServer, GatewayWebServerConfig, bind_gateway_web_server};

pub type GatewayEventSink = Arc<dyn Fn(GatewayEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Gateway {
    state: StateRuntime,
    backend: Arc<dyn GatewayBackend>,
    active: Arc<Mutex<HashMap<String, ActiveThreadState>>>,
    process_bindings: Arc<Mutex<HashMap<String, String>>>,
    pending_permissions: PendingPermissionMap,
}

impl fmt::Debug for Gateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Gateway")
            .field("state", &self.state)
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

impl Gateway {
    pub fn new(state: StateRuntime) -> Self {
        Self::with_backend(state, Arc::new(PsychevoRuntimeBackend))
    }

    pub fn with_backend(state: StateRuntime, backend: Arc<dyn GatewayBackend>) -> Self {
        Self {
            state,
            backend,
            active: Arc::new(Mutex::new(HashMap::new())),
            process_bindings: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn state(&self) -> &StateRuntime {
        &self.state
    }

    pub fn resolve_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        self.lookup_source_thread(source)
    }

    pub fn thread_transcript(
        &self,
        thread_id: &str,
    ) -> psychevo_runtime::Result<Vec<TranscriptEntry>> {
        let summaries = self.state.store().load_tui_message_summaries(thread_id)?;
        Ok(transcript::project_transcript_entries(
            thread_id, &summaries,
        ))
    }

    pub fn activity_for_selector(&self, selector: GatewayThreadSelector) -> GatewayActivity {
        let selector_keys = self.selector_keys(&selector);
        let active = self.active.lock().expect("gateway active map poisoned");
        let mut activity = GatewayActivity::default();
        for key in selector_keys {
            if let Some(state) = active.get(&key) {
                activity.running |= state.running;
                if activity.active_turn_id.is_none() {
                    activity.active_turn_id = state.active_turn_id.clone();
                }
                activity.queued_turns += state.queued.len();
            }
        }
        activity
    }

    pub async fn send_turn(
        &self,
        request: SendTurnRequest,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let queue_key = self.queue_key_for_request(&request)?;
        let turn_id = Uuid::now_v7().to_string();
        let mut request = Some(request);
        let queued = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let state = active.entry(queue_key.clone()).or_default();
            if state.running {
                let (responder, receiver) = oneshot::channel();
                let queue_position = state.queued.len() + 1;
                let queued_request = request.take().expect("gateway request missing");
                let event_sink = queued_request.event_sink.clone();
                let thread_id = queued_request.thread_id.clone();
                state
                    .queued
                    .push_back(PendingQueuedActivity::Turn(Box::new(PendingQueuedTurn {
                        turn_id: turn_id.clone(),
                        request: queued_request,
                        responder,
                    })));
                Some((receiver, event_sink, thread_id, queue_position))
            } else {
                state.running = true;
                None
            }
        };

        if let Some((receiver, event_sink, thread_id, queue_position)) = queued {
            if let Some(event_sink) = event_sink {
                event_sink(GatewayEvent::TurnQueued {
                    thread_id,
                    turn_id,
                    queue_position,
                });
            }
            return receiver
                .await
                .map_err(|_| Error::Message("gateway turn queue closed".to_string()))?;
        }

        let result = self
            .run_turn_now(
                &queue_key,
                request.take().expect("gateway request missing"),
                turn_id,
            )
            .await;
        self.finish_activity_and_spawn_next(queue_key);
        result
    }

    pub async fn send_shell(
        &self,
        request: SendShellRequest,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let queue_key = self.queue_key_for_shell_request(&request)?;
        let shell_id = Uuid::now_v7().to_string();
        let mut request = Some(request);
        let active = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let state = active.entry(queue_key.clone()).or_default();
            if state.running {
                if state.active_kind == Some(ActiveActivityKind::Turn)
                    && let Some(control) = state.control.clone()
                {
                    ShellStartState::Auxiliary(control)
                } else {
                    let (responder, receiver) = oneshot::channel();
                    state
                        .queued
                        .push_back(PendingQueuedActivity::Shell(Box::new(PendingQueuedShell {
                            shell_id: shell_id.clone(),
                            request: request.take().expect("gateway shell request missing"),
                            responder,
                        })));
                    ShellStartState::Queued(receiver)
                }
            } else {
                state.running = true;
                ShellStartState::Standalone
            }
        };

        match active {
            ShellStartState::Queued(receiver) => receiver
                .await
                .map_err(|_| Error::Message("gateway shell queue closed".to_string()))?,
            ShellStartState::Auxiliary(inject_into) => {
                self.run_shell_auxiliary(
                    request.take().expect("gateway shell request missing"),
                    shell_id,
                    inject_into,
                )
                .await
            }
            ShellStartState::Standalone => {
                let result = self
                    .run_shell_now(
                        &queue_key,
                        request.take().expect("gateway shell request missing"),
                        shell_id,
                    )
                    .await;
                self.finish_activity_and_spawn_next(queue_key);
                result
            }
        }
    }

    pub fn steer_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_runtime::Message,
    ) -> Option<psychevo_runtime::PendingInputId> {
        self.control_for_selector(&selector, expected_turn_id)
            .and_then(|control| control.steer_user_message(message))
    }

    pub fn cancel_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_runtime::PendingInputId,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.cancel_pending_user_message(input_id))
    }

    pub fn update_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_runtime::PendingInputId,
        message: psychevo_runtime::Message,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.update_pending_user_message(input_id, message))
    }

    pub fn interrupt_turn(&self, selector: GatewayThreadSelector) -> bool {
        if let Some(control) = self.control_for_selector(&selector, None) {
            control.abort();
            true
        } else {
            false
        }
    }

    pub fn submit_clarify(
        &self,
        selector: GatewayThreadSelector,
        call_id: &str,
        result: ClarifyResult,
    ) -> bool {
        self.control_for_selector(&selector, None)
            .is_some_and(|control| control.submit_clarify_result(call_id, result))
    }

    pub fn submit_permission(
        &self,
        selector: GatewayThreadSelector,
        request_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        let selector_keys = self.selector_keys(&selector);
        let pending = {
            let mut permissions = self
                .pending_permissions
                .lock()
                .expect("gateway pending permission map poisoned");
            match permissions.get(request_id) {
                Some(pending)
                    if pending.selector_key.as_deref().is_some_and(|pending_key| {
                        !selector_keys.iter().any(|key| key == pending_key)
                    }) =>
                {
                    return false;
                }
                Some(_) => permissions.remove(request_id),
                None => None,
            }
        };
        pending
            .and_then(|pending| pending.responder.send(decision).ok())
            .is_some()
    }

    pub fn clear_queue(&self, selector: GatewayThreadSelector) -> usize {
        let selector_keys = self.selector_keys(&selector);
        let mut dropped = Vec::new();
        {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            for key in selector_keys {
                if let Some(state) = active.get_mut(&key) {
                    dropped.extend(state.queued.drain(..));
                }
            }
        }
        let count = dropped.len();
        for pending in dropped {
            match pending {
                PendingQueuedActivity::Turn(pending) => {
                    let _ = pending.responder.send(Err(Error::Message(
                        "gateway turn queue cleared".to_string(),
                    )));
                }
                PendingQueuedActivity::Shell(pending) => {
                    let _ = pending.responder.send(Err(Error::Message(
                        "gateway shell queue cleared".to_string(),
                    )));
                }
            }
        }
        count
    }

    pub fn reset_source(
        &self,
        source: &GatewaySource,
        new_thread_id: &str,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot reset invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.state.store().resume_session(new_thread_id)?;
                let previous = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0, new_thread_id.to_string());
                if let Some(previous) = previous {
                    self.state
                        .store()
                        .mark_session_ended_with_reason(&previous, "gateway_reset")?;
                    self.state.store().archive_session(&previous)?;
                }
            }
            GatewaySourceLifetime::Persistent => {
                if let Some(previous) = self.state.store().gateway_source_binding(&source_key.0)? {
                    self.state
                        .store()
                        .mark_session_ended_with_reason(&previous.thread_id, "gateway_reset")?;
                    self.state.store().archive_session(&previous.thread_id)?;
                }
                self.state
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: new_thread_id,
                        backend_kind: self.backend.kind().as_str(),
                        backend_native_id: Some(new_thread_id),
                        lineage: Some(json!({"reason": "gateway_reset"})),
                    })?;
            }
        }
        Ok(())
    }

    pub fn bind_source_thread(
        &self,
        source: &GatewaySource,
        thread_id: &str,
        backend: &GatewayBackendInfo,
        lineage: Option<Value>,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot bind invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.state.store().resume_session(thread_id)?;
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0, thread_id.to_string());
            }
            GatewaySourceLifetime::Persistent => {
                self.state
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id,
                        backend_kind: backend.kind.as_str(),
                        backend_native_id: backend.native_id.as_deref(),
                        lineage,
                    })?;
            }
        }
        Ok(())
    }

    async fn run_turn_now(
        &self,
        queue_key: &str,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let event_sink = request.event_sink.clone();
        let event_sink_for_completion = request.event_sink.clone();
        let mut options = request.options;
        options.state = self.state.clone();
        apply_input_parts(&mut options, &request.input)?;

        let mapped_thread_id = if !request.reset_source_binding
            && let Some(source) = &request.source
        {
            self.lookup_source_thread(source)?
        } else {
            None
        };
        let active_thread_id = request.thread_id.clone().or(mapped_thread_id);
        if let Some(thread_id) = active_thread_id.clone() {
            options.session = Some(thread_id);
            options.continue_latest = false;
        }
        let first_committed_seq = active_thread_id
            .as_deref()
            .and_then(|thread_id| {
                self.state
                    .store()
                    .load_tui_message_summaries(thread_id)
                    .ok()
            })
            .and_then(|summaries| summaries.last().map(|summary| summary.session_seq + 1))
            .unwrap_or(1);

        if options.approval_handler.is_none()
            && let Some(event_sink) = event_sink.clone()
        {
            options.approval_handler = Some(Arc::new(GatewayApprovalHandler::new(
                Some(queue_key.to_string()),
                self.pending_permissions.clone(),
                event_sink,
            )));
        }

        let (control_handle, control) = match request.control {
            Some(control) => (request.control_handle.clone(), Some(control)),
            None if options.clarify_enabled => {
                let (handle, control) = run_control();
                (Some(handle), Some(control))
            }
            None => (None, None),
        };

        self.register_active(
            queue_key,
            turn_id.clone(),
            control_handle,
            ActiveActivityKind::Turn,
        );

        let stream = wrap_stream(
            request.stream,
            event_sink,
            turn_id.clone(),
            active_thread_id.clone(),
        );
        let result_source = request.source.clone();
        let result_lineage = request.lineage.clone();
        let source_name = request
            .runtime_source
            .unwrap_or_else(|| "gateway".to_string());
        let continue_sources = if request.continue_sources.is_empty() {
            vec![source_name.clone()]
        } else {
            request.continue_sources
        };

        let peer = resolve_peer_turn(&options)?;
        let backend_request = BackendTurnRequest {
            options,
            runtime_source: source_name,
            continue_sources,
            stream,
            control,
        };
        let (result, backend_info) = match peer {
            Some(peer) => {
                let result =
                    acp_peer::run_acp_peer_turn(peer, backend_request, turn_id.clone()).await?;
                (
                    result.run,
                    GatewayBackendInfo {
                        kind: BackendKind::PeerAgent,
                        native_id: Some(result.native_session_id),
                    },
                )
            }
            None => {
                let result = self.backend.run_turn(backend_request).await?;
                (
                    result,
                    GatewayBackendInfo {
                        kind: self.backend.kind(),
                        native_id: None,
                    },
                )
            }
        };
        let backend_info = GatewayBackendInfo {
            native_id: backend_info
                .native_id
                .or_else(|| Some(result.session_id.clone())),
            ..backend_info
        };

        if let Some(source) = &result_source {
            self.bind_source_to_result(source, &result, &backend_info, result_lineage)?;
        }
        let summaries = self
            .state
            .store()
            .load_tui_message_summaries(&result.session_id)?;
        let committed_entries = transcript::project_committed_turn_entries(
            &result.session_id,
            &summaries,
            first_committed_seq,
        );
        if let Some(event_sink) = event_sink_for_completion {
            event_sink(GatewayEvent::TurnCompleted {
                thread_id: Some(result.session_id.clone()),
                turn_id: turn_id.clone(),
                outcome: Some(result.outcome.as_str().to_string()),
                committed_entries: committed_entries.clone(),
            });
        }

        Ok(GatewayTurnResult {
            thread: GatewayThread {
                id: result.session_id.clone(),
                backend: backend_info,
                source_key: result_source.as_ref().map(GatewaySource::source_key),
            },
            turn: GatewayTurn {
                id: turn_id,
                thread_id: result.session_id.clone(),
                status: GatewayTurnStatus::Completed,
            },
            result,
            committed_entries,
        })
    }

    async fn run_shell_now(
        &self,
        queue_key: &str,
        request: SendShellRequest,
        shell_id: String,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let (control_handle, control) = run_control();
        self.register_active(
            queue_key,
            shell_id.clone(),
            Some(control_handle),
            ActiveActivityKind::Shell,
        );
        self.run_shell_with_control(request, shell_id, control, None)
            .await
    }

    async fn run_shell_auxiliary(
        &self,
        request: SendShellRequest,
        shell_id: String,
        inject_into: RunControlHandle,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let (_control_handle, control) = run_control();
        self.run_shell_with_control(request, shell_id, control, Some(inject_into))
            .await
    }

    async fn run_shell_with_control(
        &self,
        request: SendShellRequest,
        shell_id: String,
        control: RunControl,
        inject_into: Option<RunControlHandle>,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let mut context = request.context;
        context.state = self.state.clone();
        let active_thread_id = request
            .thread_id
            .clone()
            .or_else(|| context.session.clone())
            .or_else(|| {
                request
                    .source
                    .as_ref()
                    .and_then(|source| self.lookup_source_thread(source).ok().flatten())
            });
        if let Some(thread_id) = active_thread_id.clone() {
            context.session = Some(thread_id);
            context.continue_latest = false;
        }
        let first_committed_seq = active_thread_id
            .as_deref()
            .and_then(|thread_id| {
                self.state
                    .store()
                    .load_tui_message_summaries(thread_id)
                    .ok()
            })
            .and_then(|summaries| summaries.last().map(|summary| summary.session_seq + 1))
            .unwrap_or(1);
        let event_sink_for_completion = request.event_sink.clone();
        let shell_event_id = shell_id.clone();
        let stream = wrap_stream(
            request.stream,
            request.event_sink,
            shell_id,
            active_thread_id.clone(),
        );
        let stream = stream.unwrap_or_else(|| Arc::new(|_| {}));
        let result = run_user_shell_command_streaming_controlled(
            UserShellOptions {
                workdir: request.workdir,
                command: request.command,
                context: Some(context),
                inject_into,
            },
            stream,
            control,
        )
        .await?;
        let session_id = result
            .session_id
            .clone()
            .or(active_thread_id)
            .ok_or_else(|| Error::Message("shell command did not resolve a session".to_string()))?;
        let backend = GatewayBackendInfo {
            kind: BackendKind::Psychevo,
            native_id: Some(session_id.clone()),
        };
        if let Some(source) = &request.source {
            self.bind_source_thread(source, &session_id, &backend, request.lineage)?;
        }
        let summaries = self.state.store().load_tui_message_summaries(&session_id)?;
        let committed_entries = transcript::project_committed_turn_entries(
            &session_id,
            &summaries,
            first_committed_seq,
        );
        if let Some(event_sink) = event_sink_for_completion {
            for entry in committed_entries.clone() {
                event_sink(GatewayEvent::EntryUpdated {
                    turn_id: shell_event_id.clone(),
                    entry,
                });
            }
        }
        Ok(GatewayShellResult {
            thread: GatewayThread {
                id: session_id,
                backend,
                source_key: request.source.as_ref().map(GatewaySource::source_key),
            },
            result,
            committed_entries,
        })
    }

    fn finish_activity_and_spawn_next(&self, queue_key: String) {
        let next = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let Some(state) = active.get_mut(&queue_key) else {
                return;
            };
            state.control = None;
            state.active_turn_id = None;
            state.active_kind = None;
            if let Some(next) = state.queued.pop_front() {
                state.running = true;
                Some(next)
            } else {
                active.remove(&queue_key);
                None
            }
        };
        if let Some(next) = next {
            let gateway = self.clone();
            let run_key = queue_key.clone();
            match next {
                PendingQueuedActivity::Turn(next) => {
                    tokio::spawn(async move {
                        let result = gateway
                            .run_turn_now(&run_key, next.request, next.turn_id)
                            .await;
                        let _ = next.responder.send(result);
                        gateway.finish_activity_and_spawn_next(run_key);
                    });
                }
                PendingQueuedActivity::Shell(next) => {
                    tokio::spawn(async move {
                        let result = gateway
                            .run_shell_now(&run_key, next.request, next.shell_id)
                            .await;
                        let _ = next.responder.send(result);
                        gateway.finish_activity_and_spawn_next(run_key);
                    });
                }
            }
        }
    }

    fn queue_key_for_request(&self, request: &SendTurnRequest) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(thread_key(thread_id));
        }
        if let Some(source) = &request.source {
            if !request.reset_source_binding
                && let Some(thread_id) = self.lookup_source_thread(source)?
            {
                return Ok(thread_key(&thread_id));
            }
            return Ok(source_key_key(&source.source_key()));
        }
        if let Some(thread_id) = &request.options.session {
            return Ok(thread_key(thread_id));
        }
        Ok(format!("invocation:{}", Uuid::now_v7()))
    }

    fn queue_key_for_shell_request(
        &self,
        request: &SendShellRequest,
    ) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(thread_key(thread_id));
        }
        if let Some(source) = &request.source {
            if let Some(thread_id) = self.lookup_source_thread(source)? {
                return Ok(thread_key(&thread_id));
            }
            return Ok(source_key_key(&source.source_key()));
        }
        if let Some(thread_id) = &request.context.session {
            return Ok(thread_key(thread_id));
        }
        Ok(format!("shell:{}", Uuid::now_v7()))
    }

    fn lookup_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        match source.lifetime {
            GatewaySourceLifetime::Invocation => Ok(None),
            GatewaySourceLifetime::Process => Ok(self
                .process_bindings
                .lock()
                .expect("gateway process binding map poisoned")
                .get(&source.source_key().0)
                .cloned()),
            GatewaySourceLifetime::Persistent => Ok(self
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)?
                .map(|binding| binding.thread_id)),
        }
    }

    fn bind_source_to_result(
        &self,
        source: &GatewaySource,
        result: &RunResult,
        backend: &GatewayBackendInfo,
        lineage: Option<Value>,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {}
            GatewaySourceLifetime::Process => {
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0, result.session_id.clone());
            }
            GatewaySourceLifetime::Persistent => {
                self.state
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: &result.session_id,
                        backend_kind: backend.kind.as_str(),
                        backend_native_id: backend.native_id.as_deref(),
                        lineage,
                    })?;
            }
        }
        Ok(())
    }

    fn register_active(
        &self,
        key: &str,
        turn_id: String,
        control: Option<RunControlHandle>,
        kind: ActiveActivityKind,
    ) {
        let mut active = self.active.lock().expect("gateway active map poisoned");
        let state = active.entry(key.to_string()).or_default();
        state.active_turn_id = Some(turn_id);
        state.control = control;
        state.active_kind = Some(kind);
    }

    fn control_for_selector(
        &self,
        selector: &GatewayThreadSelector,
        expected_turn_id: Option<&str>,
    ) -> Option<RunControlHandle> {
        let selector_keys = self.selector_keys(selector);
        let active = self.active.lock().expect("gateway active map poisoned");
        for key in selector_keys {
            if let Some(state) = active.get(&key) {
                if expected_turn_id
                    .is_some_and(|expected| state.active_turn_id.as_deref() != Some(expected))
                {
                    continue;
                }
                if let Some(control) = &state.control {
                    return Some(control.clone());
                }
            }
        }
        None
    }

    fn selector_keys(&self, selector: &GatewayThreadSelector) -> Vec<String> {
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
                if let Ok(Some(binding)) = self.state.store().gateway_source_binding(&source_key.0)
                {
                    keys.push(thread_key(&binding.thread_id));
                }
                keys
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedPeerTurn {
    pub(crate) agent: psychevo_runtime::AgentDefinition,
    pub(crate) backend: psychevo_runtime::AgentBackendConfig,
}

fn resolve_peer_turn(options: &RunOptions) -> psychevo_runtime::Result<Option<ResolvedPeerTurn>> {
    if options.no_agents {
        return Ok(None);
    }
    let Some(agent_input) = options.agent.as_ref() else {
        return Ok(None);
    };
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let agents_home = resolve_skills_home(&env, &options.workdir)?;
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home.clone(),
        workdir: options.workdir.clone(),
        env: env.clone(),
        explicit_inputs: vec![agent_input.clone()],
        no_agents: false,
    })?;
    let agent = resolve_agent_definition(&catalog, agent_input, &options.workdir, &env)?;
    let Some(backend_ref) = agent.backend.as_ref() else {
        return Ok(None);
    };
    if !agent.supports_entrypoint(AgentEntrypoint::Peer) {
        return Err(Error::Message(format!(
            "agent `{}` references backend `{}` but does not support the peer entrypoint",
            agent.name, backend_ref.name
        )));
    }
    let backends = load_agent_backend_configs(&agents_home, &options.workdir, &env)?;
    let backend = backends
        .get(&backend_ref.name)
        .cloned()
        .ok_or_else(|| Error::Message(format!("unknown agent backend: {}", backend_ref.name)))?;
    if !backend.enabled {
        return Err(Error::Message(format!(
            "agent backend `{}` is disabled",
            backend.id
        )));
    }
    if backend
        .command
        .as_deref()
        .is_none_or(|command| command.trim().is_empty())
    {
        return Err(Error::Message(format!(
            "agent backend `{}` is missing command",
            backend.id
        )));
    }
    Ok(Some(ResolvedPeerTurn { agent, backend }))
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayActivity {
    pub running: bool,
    pub active_turn_id: Option<String>,
    pub queued_turns: usize,
}

#[derive(Debug, Default)]
struct ActiveThreadState {
    running: bool,
    active_turn_id: Option<String>,
    active_kind: Option<ActiveActivityKind>,
    control: Option<RunControlHandle>,
    queued: VecDeque<PendingQueuedActivity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveActivityKind {
    Turn,
    Shell,
}

enum ShellStartState {
    Standalone,
    Auxiliary(RunControlHandle),
    Queued(oneshot::Receiver<psychevo_runtime::Result<GatewayShellResult>>),
}

#[derive(Debug)]
enum PendingQueuedActivity {
    Turn(Box<PendingQueuedTurn>),
    Shell(Box<PendingQueuedShell>),
}

#[derive(Debug)]
struct PendingQueuedTurn {
    turn_id: String,
    request: SendTurnRequest,
    responder: oneshot::Sender<psychevo_runtime::Result<GatewayTurnResult>>,
}

#[derive(Debug)]
struct PendingQueuedShell {
    shell_id: String,
    request: SendShellRequest,
    responder: oneshot::Sender<psychevo_runtime::Result<GatewayShellResult>>,
}

type PendingPermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;

struct PendingPermission {
    selector_key: Option<String>,
    responder: oneshot::Sender<PermissionApprovalDecision>,
}

#[derive(Clone)]
struct GatewayApprovalHandler {
    selector_key: Option<String>,
    pending_permissions: PendingPermissionMap,
    event_sink: GatewayEventSink,
    timeout_secs: u64,
}

impl GatewayApprovalHandler {
    fn new(
        selector_key: Option<String>,
        pending_permissions: PendingPermissionMap,
        event_sink: GatewayEventSink,
    ) -> Self {
        Self {
            selector_key,
            pending_permissions,
            event_sink,
            timeout_secs: 300,
        }
    }
}

impl fmt::Debug for GatewayApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayApprovalHandler")
            .field("selector_key", &self.selector_key)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for GatewayApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let request_id = if request.tool_call_id.trim().is_empty() {
            Uuid::now_v7().to_string()
        } else {
            request.tool_call_id.clone()
        };
        let selector_key = self.selector_key.clone();
        let pending_permissions = self.pending_permissions.clone();
        let event_sink = self.event_sink.clone();
        let timeout_secs = self.timeout_secs;
        Box::pin(async move {
            let (responder, receiver) = oneshot::channel();
            {
                let mut pending = pending_permissions
                    .lock()
                    .expect("gateway pending permission map poisoned");
                pending.insert(
                    request_id.clone(),
                    PendingPermission {
                        selector_key,
                        responder,
                    },
                );
            }
            event_sink(GatewayEvent::PermissionRequested {
                request_id: request_id.clone(),
                tool_name: request.tool_name.clone(),
                summary: request.summary.clone(),
                reason: request.reason.clone(),
                matched_rule: request.matched_rule.clone(),
                suggested_rule: request.suggested_rule.clone(),
                allow_always: request.allow_always,
                timeout_secs: request.timeout_secs,
            });
            let decision = timeout(Duration::from_secs(timeout_secs), receiver)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(PermissionApprovalDecision::deny);
            {
                let mut pending = pending_permissions
                    .lock()
                    .expect("gateway pending permission map poisoned");
                pending.remove(&request_id);
            }
            event_sink(GatewayEvent::PermissionResolved {
                request_id,
                decision: permission_decision_from_runtime(&decision),
            });
            decision
        })
    }
}

#[derive(Debug, Clone)]
pub struct QueuedGatewayInput {
    pub input: Vec<GatewayInputPart>,
}

pub struct SendTurnRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub reset_source_binding: bool,
    pub input: Vec<GatewayInputPart>,
    pub options: RunOptions,
    pub runtime_source: Option<String>,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub control_handle: Option<RunControlHandle>,
    pub control: Option<RunControl>,
    pub lineage: Option<Value>,
}

impl fmt::Debug for SendTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendTurnRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("reset_source_binding", &self.reset_source_binding)
            .field("input", &self.input)
            .field("options", &self.options)
            .field("runtime_source", &self.runtime_source)
            .field("continue_sources", &self.continue_sources)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .field("has_control_handle", &self.control_handle.is_some())
            .field("has_control", &self.control.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}

pub struct SendShellRequest {
    pub thread_id: Option<String>,
    pub source: Option<GatewaySource>,
    pub workdir: PathBuf,
    pub command: String,
    pub context: UserShellContextOptions,
    pub stream: Option<RunStreamSink>,
    pub event_sink: Option<GatewayEventSink>,
    pub lineage: Option<Value>,
}

impl fmt::Debug for SendShellRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendShellRequest")
            .field("thread_id", &self.thread_id)
            .field("source", &self.source)
            .field("workdir", &self.workdir)
            .field("command", &self.command)
            .field("context", &self.context)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
            .field("lineage", &self.lineage)
            .finish()
    }
}

#[derive(Debug)]
pub struct GatewayTurnResult {
    pub thread: GatewayThread,
    pub turn: GatewayTurn,
    pub result: RunResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

#[derive(Debug)]
pub struct GatewayShellResult {
    pub thread: GatewayThread,
    pub result: UserShellResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

pub struct BackendTurnRequest {
    pub options: RunOptions,
    pub runtime_source: String,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub control: Option<RunControl>,
}

pub trait GatewayBackend: Send + Sync + fmt::Debug {
    fn kind(&self) -> BackendKind;
    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>>;
}

#[derive(Debug)]
pub struct PsychevoRuntimeBackend;

impl GatewayBackend for PsychevoRuntimeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Psychevo
    }

    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
        Box::pin(async move {
            let continue_sources = request
                .continue_sources
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            match (request.stream, request.control) {
                (Some(stream), Some(control)) => {
                    run_live_streaming_controlled(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                        control,
                    )
                    .await
                }
                (Some(stream), None) => {
                    run_live_streaming(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                    )
                    .await
                }
                (None, Some(control)) => {
                    let stream: RunStreamSink = Arc::new(|_| {});
                    run_live_streaming_controlled(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                        control,
                    )
                    .await
                }
                (None, None)
                    if request.runtime_source == "run"
                        && continue_sources.len() == 1
                        && continue_sources[0] == "run" =>
                {
                    run_live(request.options).await
                }
                (None, None) => {
                    let stream: RunStreamSink = Arc::new(|_| {});
                    run_live_streaming(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                    )
                    .await
                }
            }
        })
    }
}

fn thread_key(thread_id: &str) -> String {
    format!("thread:{thread_id}")
}

fn source_key_key(source_key: &SourceKey) -> String {
    format!("source:{}", source_key.0)
}

fn wrap_stream(
    stream: Option<RunStreamSink>,
    event_sink: Option<GatewayEventSink>,
    turn_id: String,
    thread_id: Option<String>,
) -> Option<RunStreamSink> {
    match (stream, event_sink) {
        (None, None) => None,
        (stream, event_sink) => {
            let projector = Arc::new(Mutex::new(GatewayLiveProjector::new(thread_id)));
            Some(Arc::new(move |event: RunStreamEvent| {
                if let Some(event_sink) = &event_sink
                    && let Some(event) = projector
                        .lock()
                        .expect("gateway live projector poisoned")
                        .project(&turn_id, &event)
                {
                    event_sink(event);
                }
                if let Some(stream) = &stream {
                    stream(event);
                }
            }))
        }
    }
}

fn apply_input_parts(
    options: &mut RunOptions,
    input: &[GatewayInputPart],
) -> psychevo_runtime::Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    let mut prompt_parts = Vec::new();
    let mut image_inputs = Vec::new();
    for part in input {
        match part {
            GatewayInputPart::Text { text } => prompt_parts.push(text.clone()),
            GatewayInputPart::Context {
                text,
                visible_to_model,
                ..
            } if *visible_to_model => prompt_parts.push(text.clone()),
            GatewayInputPart::Context { .. } => {}
            GatewayInputPart::Image { input } => {
                image_inputs.push(gateway_image_input_into_runtime(input.clone()))
            }
        }
    }
    options.prompt = prompt_parts.join("\n");
    options.image_inputs = image_inputs;
    if options.prompt.trim().is_empty() && options.image_inputs.is_empty() {
        return Err(Error::Message("gateway turn input is empty".to_string()));
    }
    Ok(())
}

fn gateway_image_input_into_runtime(input: GatewayImageInput) -> ImageInput {
    match input {
        GatewayImageInput::LocalPath { path } => ImageInput::LocalPath(path.into()),
        GatewayImageInput::Url { url } => ImageInput::ImageUrl(url),
    }
}

fn permission_decision_from_runtime(decision: &PermissionApprovalDecision) -> PermissionDecision {
    match decision.outcome {
        PermissionApprovalOutcome::AllowOnce => PermissionDecision::AllowOnce,
        PermissionApprovalOutcome::AllowSession => PermissionDecision::AllowSession,
        PermissionApprovalOutcome::AllowAlways => PermissionDecision::AllowAlways,
        PermissionApprovalOutcome::Deny => PermissionDecision::Deny,
    }
}

pub(crate) fn gateway_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use psychevo_ai::Outcome;
    use psychevo_runtime::{Message, PermissionMode, RunMode, UserContentBlock};
    use tokio::sync::{Notify, mpsc};

    #[derive(Debug, Clone)]
    struct FakeRun {
        prompt: String,
        session: Option<String>,
    }

    #[derive(Debug, Clone)]
    struct WaitFirst {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Default)]
    struct FakeBackendInner {
        runs: Mutex<Vec<FakeRun>>,
        next_run: AtomicUsize,
        wait_first: Mutex<Option<WaitFirst>>,
        request_permission: AtomicBool,
    }

    #[derive(Clone, Default)]
    struct FakeBackend {
        inner: Arc<FakeBackendInner>,
    }

    impl FakeBackend {
        fn runs(&self) -> Vec<FakeRun> {
            self.inner
                .runs
                .lock()
                .expect("fake run lock poisoned")
                .clone()
        }

        fn wait_on_first_run(&self) -> WaitFirst {
            let wait = WaitFirst {
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

        fn request_permission(&self) {
            self.inner.request_permission.store(true, Ordering::SeqCst);
        }
    }

    impl fmt::Debug for FakeBackend {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("FakeBackend")
        }
    }

    impl GatewayBackend for FakeBackend {
        fn kind(&self) -> BackendKind {
            BackendKind::Psychevo
        }

        fn run_turn(
            &self,
            request: BackendTurnRequest,
        ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
            let inner = Arc::clone(&self.inner);
            Box::pin(async move {
                let run_number = inner.next_run.fetch_add(1, Ordering::SeqCst) + 1;
                {
                    let mut runs = inner.runs.lock().expect("fake run lock poisoned");
                    runs.push(FakeRun {
                        prompt: request.options.prompt.clone(),
                        session: request.options.session.clone(),
                    });
                }

                let wait_first = inner
                    .wait_first
                    .lock()
                    .expect("fake wait lock poisoned")
                    .clone();
                if run_number == 1
                    && let Some(wait) = wait_first
                {
                    wait.started.notify_one();
                    wait.release.notified().await;
                }

                if inner.request_permission.load(Ordering::SeqCst)
                    && let Some(handler) = request.options.approval_handler.clone()
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
                            timeout_secs: 300,
                        })
                        .await;
                }

                let session_id = if let Some(session_id) = request.options.session.clone() {
                    request.options.state.store().resume_session(&session_id)?;
                    session_id
                } else {
                    request.options.state.store().create_session_with_metadata(
                        &request.options.workdir,
                        &request.runtime_source,
                        "fake-model",
                        "fake-provider",
                        None,
                    )?
                };

                Ok(RunResult {
                    session_id,
                    outcome: Outcome::Normal,
                    terminal_reason: None,
                    final_answer: format!("answer {run_number}"),
                    db_path: request.options.state.db_path().to_path_buf(),
                    workdir: request.options.workdir,
                    provider: "fake-provider".to_string(),
                    model: "fake-model".to_string(),
                    base_url: String::new(),
                    api_key_env: None,
                    reasoning_effort: None,
                    context_limit: None,
                    tool_failures: 0,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                    context_snapshot: None,
                    events: Vec::new(),
                    warnings: Vec::new(),
                })
            })
        }
    }

    struct Harness {
        _temp: tempfile::TempDir,
        workdir: PathBuf,
        state: StateRuntime,
        gateway: Gateway,
    }

    fn harness(backend: Arc<FakeBackend>) -> Harness {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state runtime");
        let gateway = Gateway::with_backend(state.clone(), backend);
        Harness {
            _temp: temp,
            workdir,
            state,
            gateway,
        }
    }

    fn run_options(harness: &Harness, prompt: &str) -> RunOptions {
        RunOptions {
            state: harness.state.clone(),
            workdir: harness.workdir.clone(),
            snapshot_root: None,
            session: None,
            continue_latest: false,
            prompt: prompt.to_string(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: None,
            project_context_override: None,
            model: None,
            reasoning_effort: None,
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: Some(PermissionMode::Default),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    fn request(harness: &Harness, source: GatewaySource, prompt: &str) -> SendTurnRequest {
        SendTurnRequest {
            thread_id: None,
            source: Some(source),
            reset_source_binding: false,
            input: Vec::new(),
            options: run_options(harness, prompt),
            runtime_source: Some("test".to_string()),
            continue_sources: vec!["test".to_string()],
            stream: None,
            event_sink: None,
            control_handle: None,
            control: None,
            lineage: None,
        }
    }

    #[tokio::test]
    async fn invocation_source_does_not_bind_or_reuse() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("cli", "run-1").invocation();

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let second = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");

        assert_ne!(first.result.session_id, second.result.session_id);
        assert!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
        assert_eq!(backend.runs()[1].session, None);
    }

    #[tokio::test]
    async fn process_source_reuses_only_within_gateway_instance() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "workdir").process();

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let second = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");
        let rebuilt_gateway = Gateway::with_backend(harness.state.clone(), backend.clone());
        let third = rebuilt_gateway
            .send_turn(request(&harness, source.clone(), "third"))
            .await
            .expect("third turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        assert_ne!(first.result.session_id, third.result.session_id);
        assert_eq!(
            backend.runs()[1].session.as_deref(),
            Some(first.result.session_id.as_str())
        );
        assert!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn persistent_source_round_trips_through_store() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let source = GatewaySource::new("acp", "client-session").persistent();

        let first = harness
            .gateway
            .send_turn(request(&harness, source.clone(), "first"))
            .await
            .expect("first turn");
        let rebuilt_gateway = Gateway::with_backend(harness.state.clone(), backend.clone());
        let second = rebuilt_gateway
            .send_turn(request(&harness, source.clone(), "second"))
            .await
            .expect("second turn");

        assert_eq!(first.result.session_id, second.result.session_id);
        assert_eq!(
            harness
                .state
                .store()
                .gateway_source_binding(&source.source_key().0)
                .expect("binding lookup")
                .expect("binding")
                .thread_id,
            first.result.session_id
        );
    }

    #[tokio::test]
    async fn send_turn_serializes_same_source_fifo() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "workdir").process();

        let first_gateway = harness.gateway.clone();
        let first_request = request(&harness, source.clone(), "first");
        let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let second_gateway = harness.gateway.clone();
        let second_request = request(&harness, source.clone(), "second");
        let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });

        tokio::task::yield_now().await;
        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string()]
        );

        wait.release.notify_one();
        let first = first.await.expect("first task").expect("first turn");
        let second = second.await.expect("second task").expect("second turn");
        assert_eq!(first.result.session_id, second.result.session_id);
        assert_eq!(
            backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[tokio::test]
    async fn explicit_thread_turn_allows_source_rebind_while_running() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend);
        let source = GatewaySource::new("web", "workdir").persistent();
        let first = harness
            .state
            .store()
            .create_session_with_metadata(&harness.workdir, "web", "model", "provider", None)
            .expect("first session");
        let second = harness
            .state
            .store()
            .create_session_with_metadata(&harness.workdir, "web", "model", "provider", None)
            .expect("second session");
        harness
            .gateway
            .bind_source_thread(
                &source,
                &first,
                &GatewayBackendInfo {
                    kind: BackendKind::Psychevo,
                    native_id: Some(first.clone()),
                },
                None,
            )
            .expect("bind first");

        let mut first_request = request(&harness, source.clone(), "first");
        first_request.thread_id = Some(first.clone());
        let gateway = harness.gateway.clone();
        let running = tokio::spawn(async move { gateway.send_turn(first_request).await });
        wait.started.notified().await;

        harness
            .gateway
            .bind_source_thread(
                &source,
                &second,
                &GatewayBackendInfo {
                    kind: BackendKind::Psychevo,
                    native_id: Some(second.clone()),
                },
                None,
            )
            .expect("bind second");

        assert!(
            harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(&first))
                .running
        );
        assert!(
            !harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::source(source.source_key()))
                .running
        );

        wait.release.notify_one();
        running
            .await
            .expect("running task")
            .expect("running result");
    }

    #[tokio::test]
    async fn typed_steer_requires_expected_turn_id() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend);
        let source = GatewaySource::new("tui", "workdir").process();
        let selector = GatewayThreadSelector::source(source.source_key());

        let (handle, control) = run_control();
        let mut first_request = request(&harness, source.clone(), "first");
        first_request.control_handle = Some(handle);
        first_request.control = Some(control);
        let gateway = harness.gateway.clone();
        let first = tokio::spawn(async move { gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let active_turn_id = harness
            .gateway
            .activity_for_selector(selector.clone())
            .active_turn_id
            .expect("active turn id");
        let message = Message::User {
            content: vec![UserContentBlock::text("steer")],
            timestamp_ms: 0,
        };

        assert!(
            harness
                .gateway
                .steer_turn(selector.clone(), Some("stale-turn"), message.clone())
                .is_none()
        );
        let input_id = harness
            .gateway
            .steer_turn(selector.clone(), Some(&active_turn_id), message.clone())
            .expect("current turn steer");
        assert!(!harness.gateway.update_steer(
            selector.clone(),
            Some("stale-turn"),
            input_id,
            message.clone()
        ));
        assert!(harness.gateway.update_steer(
            selector.clone(),
            Some(&active_turn_id),
            input_id,
            message.clone()
        ));
        assert!(
            !harness
                .gateway
                .cancel_steer(selector.clone(), Some("stale-turn"), input_id)
        );
        assert!(
            harness
                .gateway
                .cancel_steer(selector, Some(&active_turn_id), input_id)
        );

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
    }

    #[tokio::test]
    async fn interrupt_aborts_active_and_clear_queue_drops_pending_turns() {
        let backend = Arc::new(FakeBackend::default());
        let wait = backend.wait_on_first_run();
        let harness = harness(backend.clone());
        let source = GatewaySource::new("tui", "workdir").process();

        let (handle, control) = run_control();
        let mut first_request = request(&harness, source.clone(), "first");
        first_request.control_handle = Some(handle);
        first_request.control = Some(control);
        let first_gateway = harness.gateway.clone();
        let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
        wait.started.notified().await;

        let second_gateway = harness.gateway.clone();
        let second_request = request(&harness, source.clone(), "second");
        let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });
        tokio::task::yield_now().await;

        let selector = GatewayThreadSelector::source(source.source_key());
        assert!(harness.gateway.interrupt_turn(selector.clone()));
        let mut cleared = harness.gateway.clear_queue(selector);
        for _ in 0..10 {
            if cleared > 0 {
                break;
            }
            tokio::task::yield_now().await;
            cleared = harness
                .gateway
                .clear_queue(GatewayThreadSelector::source(source.source_key()));
        }
        assert_eq!(cleared, 1);

        let second_err = second
            .await
            .expect("second task")
            .expect_err("queued turn should be cleared");
        assert!(second_err.to_string().contains("queue cleared"));

        wait.release.notify_one();
        first.await.expect("first task").expect("first turn");
    }

    #[tokio::test]
    async fn acp_peer_agent_turn_routes_to_backend_and_persists_native_session() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let home = harness._temp.path().join("home");
        let script = harness._temp.path().join("fake_acp.py");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

loaded_session = None

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-1"}})
    elif method == "session/load":
        loaded_session = params.get("sessionId")
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-1"
        chunks = []
        for block in params.get("prompt") or []:
            if block.get("type") == "text":
                chunks.append(block.get("text") or "")
        prefix = "loaded:" + loaded_session if loaded_session else "new:" + session_id
        text = prefix + ":" + "\n".join(chunks)
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }
        }})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
"#,
        )
        .expect("fake acp script");
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = "python3"
args = ["{}"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
                script.display()
            ),
        )
        .expect("config");
        let agents_dir = harness.workdir.join(".psychevo").join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir");
        std::fs::write(
            agents_dir.join("reviewer.md"),
            r#"---
name: reviewer
description: Review with fake ACP.
backend:
  ref: fake
entrypoints: [peer]
tools: [read]
---
Peer instructions.
"#,
        )
        .expect("agent file");

        let env = BTreeMap::from([
            (
                "HOME".to_string(),
                harness._temp.path().display().to_string(),
            ),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
        ]);
        let source = GatewaySource::new("web", "peer").persistent();
        let mut first_request = request(&harness, source.clone(), "hello");
        first_request.options.agent = Some("reviewer".to_string());
        first_request.options.inherited_env = Some(env.clone());
        let first = harness
            .gateway
            .send_turn(first_request)
            .await
            .expect("first peer turn");

        assert_eq!(first.thread.backend.kind, BackendKind::PeerAgent);
        assert_eq!(first.thread.backend.native_id.as_deref(), Some("native-1"));
        assert_eq!(
            first
                .result
                .selected_agent
                .as_ref()
                .map(|agent| agent.name.as_str()),
            Some("reviewer")
        );
        assert!(first.result.final_answer.contains("new:native-1"));
        assert!(first.result.final_answer.contains("Peer instructions."));
        assert!(first.result.final_answer.contains("hello"));

        let binding = harness
            .state
            .store()
            .gateway_source_binding(&source.source_key().0)
            .expect("binding lookup")
            .expect("binding");
        assert_eq!(binding.backend_kind, "peer_agent");
        assert_eq!(binding.backend_native_id.as_deref(), Some("native-1"));
        let metadata = harness
            .state
            .store()
            .session_metadata(&first.result.session_id)
            .expect("metadata")
            .expect("metadata value");
        assert_eq!(metadata["peer_agent"]["nativeSessionId"], "native-1");
        let transcript = harness
            .gateway
            .thread_transcript(&first.result.session_id)
            .expect("transcript");
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].role, TranscriptEntryRole::User);
        assert_eq!(transcript[1].role, TranscriptEntryRole::Assistant);

        let mut second_request = request(&harness, source.clone(), "again");
        second_request.options.agent = Some("reviewer".to_string());
        second_request.options.inherited_env = Some(env);
        let second = harness
            .gateway
            .send_turn(second_request)
            .await
            .expect("second peer turn");
        assert_eq!(second.result.session_id, first.result.session_id);
        assert!(second.result.final_answer.contains("loaded:native-1"));
    }

    #[tokio::test]
    async fn submit_permission_resolves_gateway_permission_request() {
        let backend = Arc::new(FakeBackend::default());
        backend.request_permission();
        let harness = harness(backend);
        let source = GatewaySource::new("tui", "workdir").process();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut request = request(&harness, source.clone(), "permission");
        request.event_sink = Some(Arc::new(move |event| {
            let _ = event_tx.send(event);
        }));

        let gateway = harness.gateway.clone();
        let turn = tokio::spawn(async move { gateway.send_turn(request).await });

        loop {
            let event = event_rx.recv().await.expect("gateway event");
            if let GatewayEvent::PermissionRequested { request_id, .. } = event {
                assert_eq!(request_id, "permission-1");
                break;
            }
        }

        assert!(harness.gateway.submit_permission(
            GatewayThreadSelector::source(source.source_key()),
            "permission-1",
            PermissionApprovalDecision::allow_once(),
        ));
        turn.await.expect("turn task").expect("turn");

        let resolved = event_rx.recv().await.expect("permission resolved event");
        assert!(matches!(
            resolved,
            GatewayEvent::PermissionResolved {
                decision: PermissionDecision::AllowOnce,
                ..
            }
        ));
    }
}
