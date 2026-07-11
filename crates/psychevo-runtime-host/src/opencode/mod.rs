mod http;
mod process;
mod reconcile;
mod sse;
mod types;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};

use crate::{
    ControlState, ExecuteRequest, ExecuteResult, HistoryFidelity, ReadinessStage, ReadinessStatus,
    RetryClass, RuntimeCapability, RuntimeControl, RuntimeControlChoice, RuntimeControlDescriptor,
    RuntimeError, RuntimeErrorStage, RuntimeFuture, RuntimeIntent, RuntimeInteraction,
    RuntimeInteractionChoice, RuntimeInteractionExposure, RuntimeInteractionKind,
    RuntimeInteractionPolicy, RuntimeInteractionQuestion, RuntimeInteractionQuestionOption,
    RuntimeInteractionResult, RuntimeKind, RuntimeModule, RuntimeObserver, RuntimeProfile,
    RuntimeSessionBinding, RuntimeSessionOperation, RuntimeSessionRequest, RuntimeSessionResult,
    RuntimeSnapshot, RuntimeStability, RuntimeTerminalError, RuntimeTurnOutcome,
    RuntimeTurnRequest, RuntimeTurnResult, SessionOwnership, ShutdownMode, SnapshotMode,
    SnapshotQuery, SnapshotScope,
};

use process::{
    ADAPTER_VERSION, Generation, GenerationSignal, generation_lookup_key, prepare_launch,
    spawn_generation,
};
use reconcile::{
    HydratedInstance, child_map, diff_from_event, event_matches, final_answer, hydrate,
    is_disposed, matching_assistants, message_from_event, permission_from_event,
    question_from_event, resolved_interaction_id, runtime_diff_update, runtime_plan_update,
    runtime_session, session_from_event, status_is_idle, status_ownership, text_delta,
    todos_from_event, tool_observation,
};
use types::{
    PermissionRequest, PromptBody, PromptPart, QuestionRequest, SessionCreateBody, SessionInfo,
    parse_model,
};

const OPENCODE_STABLE_MATRIX_MIN_VERSION: (u64, u64, u64) = (1, 17, 17);

#[derive(Debug, Clone)]
enum PendingKind {
    Permission,
    Question,
}

#[derive(Debug, Clone)]
struct PendingInteraction {
    id: String,
    native_id: String,
    kind: PendingKind,
    generation: Arc<Generation>,
    cwd: PathBuf,
    session_id: String,
    process_epoch: u64,
    instance_epoch: u64,
}

#[derive(Debug, Default)]
struct ModuleState {
    generations: HashMap<String, Arc<Generation>>,
    pending: HashMap<String, PendingInteraction>,
}

#[derive(Debug)]
struct Inner {
    state: Mutex<ModuleState>,
    spawn: Mutex<()>,
    next_process_epoch: AtomicU64,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            state: Mutex::new(ModuleState::default()),
            spawn: Mutex::new(()),
            next_process_epoch: AtomicU64::new(1),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpenCodeRuntimeModule {
    inner: Arc<Inner>,
}

impl OpenCodeRuntimeModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RuntimeModule for OpenCodeRuntimeModule {
    fn snapshot(&self, query: SnapshotQuery) -> RuntimeFuture<RuntimeSnapshot> {
        let inner = self.inner.clone();
        Box::pin(async move { snapshot(inner, query).await })
    }

    fn execute(
        &self,
        request: ExecuteRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
    ) -> RuntimeFuture<ExecuteResult> {
        let inner = self.inner.clone();
        Box::pin(async move {
            validate_request(&request)?;
            match request.intent {
                RuntimeIntent::Turn(turn) => {
                    execute_turn(inner, request.profile, turn, observer, control)
                        .await
                        .map(ExecuteResult::Turn)
                }
                RuntimeIntent::Session(session) => execute_session(inner, request.profile, session)
                    .await
                    .map(ExecuteResult::Session),
                RuntimeIntent::Interaction(response) => execute_interaction(inner, response)
                    .await
                    .map(ExecuteResult::Interaction),
                RuntimeIntent::Auth(_) => Err(RuntimeError::new(
                    "opencode_auth_cli_required",
                    RuntimeErrorStage::Authentication,
                    RetryClass::UserAction,
                    "OpenCode does not expose a compatible stable provider-auth contract; run `opencode auth login`.",
                )),
                RuntimeIntent::Control(control) => Err(RuntimeError::new(
                    "opencode_control_mutation_unsupported",
                    RuntimeErrorStage::Control,
                    RetryClass::UserAction,
                    format!(
                        "OpenCode cannot mutate `{}` as observed session state; choose it before the next prompt or start a new thread.",
                        control.control_id
                    ),
                )),
                RuntimeIntent::Mcp(_)
                | RuntimeIntent::Extension(_)
                | RuntimeIntent::Compaction(_) => Err(RuntimeError::new(
                    "unsupported",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    "this OpenCode runtime intent is not supported by the stable adapter",
                )),
            }
        })
    }

    fn shutdown(&self, mode: ShutdownMode) -> RuntimeFuture<()> {
        let inner = self.inner.clone();
        Box::pin(async move { shutdown(inner, mode).await })
    }
}

async fn snapshot(
    inner: Arc<Inner>,
    query: SnapshotQuery,
) -> Result<RuntimeSnapshot, RuntimeError> {
    if query.profile.kind != RuntimeKind::OpenCode {
        return Err(wrong_kind());
    }
    let cwd = match &query.scope {
        SnapshotScope::Profile => query.profile.workspace_roots.first().map(PathBuf::as_path),
        SnapshotScope::Workspace { cwd } | SnapshotScope::Session { cwd, .. } => {
            Some(cwd.as_path())
        }
    };
    if matches!(
        query.mode,
        SnapshotMode::BoundedProbe | SnapshotMode::CatalogRefresh
    ) {
        let probe_cwd = cwd
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| {
                RuntimeError::new(
                    "probe_cwd_unavailable",
                    RuntimeErrorStage::Discovery,
                    RetryClass::UserAction,
                    "OpenCode probe requires a workspace cwd",
                )
            })?;
        let generation = acquire_generation(&inner, &query.profile, &probe_cwd).await?;
        if query.mode == SnapshotMode::CatalogRefresh {
            let agents = generation.http.agents(&probe_cwd).await?;
            generation.set_agents(&probe_cwd, agents).await;
        }
    }
    let generation_key = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .and_then(|cwd| generation_lookup_key(&query.profile, &cwd).ok());
    let generation = {
        let state = inner.state.lock().await;
        generation_key
            .as_ref()
            .and_then(|key| state.generations.get(key))
            .cloned()
    };
    let command_found = discover_command(&query.profile, cwd);
    let usable = generation.as_ref().is_some_and(|item| item.is_usable());
    let (todos_validated, diff_validated) = match (&generation, cwd) {
        (Some(generation), Some(cwd)) => generation.timeline_validation(cwd).await,
        _ => (false, false),
    };
    let timeline_validated = todos_validated && diff_validated;
    let runtime_version = generation
        .as_ref()
        .map(|generation| generation.runtime_version.as_str());
    let version_compatible = runtime_version
        .and_then(opencode_runtime_version)
        .is_some_and(|version| version >= OPENCODE_STABLE_MATRIX_MIN_VERSION);
    let policy_supported = query.profile.sandbox.as_deref().is_none_or(str::is_empty)
        && query.profile.workspace_roots.is_empty()
        && query
            .profile
            .approval_mode
            .as_deref()
            .is_none_or(|value| value.is_empty() || value == "default");
    let readiness = vec![
        ReadinessStage {
            id: "command".to_string(),
            status: if command_found {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Missing
            },
            summary: if command_found {
                "OpenCode command resolved".to_string()
            } else {
                "OpenCode command is unavailable".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "server".to_string(),
            status: if usable {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if usable {
                "authenticated loopback server connected".to_string()
            } else {
                "server handshake has not been run".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "version".to_string(),
            status: if runtime_version.is_none() {
                ReadinessStatus::Unchecked
            } else if version_compatible {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unsupported
            },
            summary: if version_compatible {
                format!(
                    "OpenCode runtime version {} satisfies the Stable matrix manifest",
                    generation
                        .as_ref()
                        .map(|generation| generation.runtime_version.as_str())
                        .unwrap_or_default()
                )
            } else if let Some(version) = runtime_version {
                format!(
                    "OpenCode runtime version {version} is below or outside the Stable matrix manifest"
                )
            } else {
                "OpenCode runtime version has not been observed".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "authentication".to_string(),
            status: if usable {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if usable {
                "OpenCode loopback Basic authentication succeeded".to_string()
            } else {
                "OpenCode local authentication has not been checked".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "capabilities".to_string(),
            status: if runtime_version.is_some() && !version_compatible {
                ReadinessStatus::Unsupported
            } else if timeline_validated {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if runtime_version.is_some() && !version_compatible {
                "OpenCode runtime version cannot prove the complete Stable capability matrix"
                    .to_string()
            } else if timeline_validated {
                "OpenCode todo/diff HTTP snapshots and a correlated authenticated SSE event were reconciled"
                    .to_string()
            } else {
                "OpenCode todo/diff HTTP hydration and correlated SSE reconciliation are incomplete"
                    .to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "policy".to_string(),
            status: if !policy_supported {
                ReadinessStatus::Unsupported
            } else if usable {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if !policy_supported {
                "OpenCode cannot exactly enforce this Runtime Profile safety policy".to_string()
            } else if usable {
                "OpenCode is bound to authenticated loopback with mDNS disabled".to_string()
            } else {
                "OpenCode launch safety policy has not been observed".to_string()
            },
            observed_at_ms: None,
        },
    ];
    let agents = match (&generation, cwd) {
        (Some(generation), Some(cwd)) => generation.cached_agents(cwd).await,
        _ => Vec::new(),
    };
    let observed_session = match (&generation, &query.scope) {
        (
            Some(generation),
            SnapshotScope::Session {
                cwd,
                native_session_id: Some(native_session_id),
                ..
            },
        ) => generation.cached_session(cwd, native_session_id).await,
        _ => None,
    };
    let visible_agents = agents
        .iter()
        .filter(|agent| !agent.hidden && matches!(agent.mode.as_str(), "primary" | "all"))
        .collect::<Vec<_>>();
    let observed_agent = observed_session
        .as_ref()
        .and_then(|session| session.agent.clone());
    let mut controls = Vec::new();
    if !visible_agents.is_empty() || observed_agent.is_some() {
        controls.push(RuntimeControlDescriptor {
            id: "agent".to_string(),
            label: "Agent".to_string(),
            state: if visible_agents.is_empty() {
                ControlState::ReadOnlyCurrent
            } else {
                ControlState::Selectable
            },
            current_value: observed_agent.map(Value::String),
            choices: visible_agents
                .iter()
                .map(|agent| RuntimeControlChoice {
                    value: Value::String(agent.name.clone()),
                    label: agent.name.clone(),
                    description: agent.description.clone(),
                })
                .collect(),
            depends_on: None,
            channel_safe: true,
            capability_revision: query.profile.revision,
        });
    }
    if let Some(model) = observed_session.and_then(|session| session.model) {
        controls.push(RuntimeControlDescriptor {
            id: "model".to_string(),
            label: "Model".to_string(),
            state: ControlState::ReadOnlyCurrent,
            current_value: Some(Value::String(format!("{}/{}", model.provider_id, model.id))),
            choices: Vec::new(),
            depends_on: None,
            channel_safe: false,
            capability_revision: query.profile.revision,
        });
    }
    let process_epoch = generation.as_ref().map(|item| item.process_epoch);
    let instance_epoch = match (&generation, cwd) {
        (Some(generation), Some(cwd)) => Some(generation.instance_epoch(cwd).await),
        _ => None,
    };
    Ok(RuntimeSnapshot {
        runtime_ref: query.profile.id,
        kind: RuntimeKind::OpenCode,
        profile_revision: query.profile.revision,
        capability_revision: query.profile.revision,
        adapter_version: ADAPTER_VERSION.to_string(),
        runtime_version: generation.as_ref().map(|item| item.runtime_version.clone()),
        stability: RuntimeStability::Stable,
        provenance: "direct".to_string(),
        readiness,
        controls,
        capabilities: capabilities(version_compatible, todos_validated, diff_validated),
        process_epoch,
        instance_epoch,
        binding_epoch: None,
        extension: (!agents.is_empty()).then(|| json!({ "agents": agents })),
    })
}

async fn execute_turn(
    inner: Arc<Inner>,
    profile: RuntimeProfile,
    turn: RuntimeTurnRequest,
    observer: RuntimeObserver,
    control: RuntimeControl,
) -> Result<RuntimeTurnResult, RuntimeError> {
    validate_turn_policy(&profile, &turn)?;
    let generation = acquire_generation(&inner, &profile, &turn.cwd).await?;
    let mut events = generation.subscribe();
    let instance_epoch = generation.instance_epoch(&turn.cwd).await;
    let session = ensure_session(&generation, &profile, &turn).await?;
    ensure_session_directory(&turn.cwd, &session)?;
    generation.observe_session(&turn.cwd, &session).await;
    observer
        .bind_native_session(RuntimeSessionBinding {
            runtime_ref: profile.id.clone(),
            thread_id: turn.thread_id.clone(),
            native_session_id: session.id.clone(),
            cwd: turn.cwd.clone(),
            binding_epoch: turn.binding_epoch,
            process_epoch: generation.process_epoch,
            instance_epoch: Some(instance_epoch),
        })
        .await?;

    let hydrated = hydrate(&generation, &turn.cwd, &session.id).await?;
    if matches!(
        hydrated
            .statuses
            .get(&session.id)
            .map(|status| status.kind.as_str()),
        Some("busy" | "retry")
    ) {
        return Err(RuntimeError::new(
            "busy",
            RuntimeErrorStage::Prompt,
            RetryClass::UserAction,
            "OpenCode native session is already active and cannot be taken over",
        ));
    }
    let mut bootstrap = BootstrapState::new(&session, hydrated);
    drain_bootstrap_events(
        &generation,
        &turn.cwd,
        instance_epoch,
        &mut events,
        &mut bootstrap,
    )
    .await?;
    publish_bootstrap(
        &inner,
        &generation,
        &turn,
        instance_epoch,
        &bootstrap,
        &observer,
    )
    .await;

    let selected_agent = select_agent(&profile, &turn, &bootstrap.hydrated.agents)?;
    let selected_model = turn.model.as_deref().or(profile.default_model.as_deref());
    if selected_model.is_some() && parse_model(selected_model).is_none() {
        return Err(RuntimeError::new(
            "invalid_model",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode model must use a provider/model id",
        ));
    }
    let user_message_id = format!("msg_{}", uuid::Uuid::now_v7().simple());
    let prompt = PromptBody {
        message_id: user_message_id.clone(),
        model: parse_model(selected_model),
        agent: selected_agent,
        system: turn.instructions.clone(),
        parts: vec![PromptPart {
            kind: "text",
            text: turn.prompt.clone(),
        }],
    };
    if let Err(error) = generation
        .http
        .prompt_async(&turn.cwd, &session.id, &prompt)
        .await
    {
        if error.retry_class == RetryClass::UnknownDelivery
            && generation
                .http
                .message(&turn.cwd, &session.id, &user_message_id)
                .await?
                .is_some()
        {
            // The caller-owned native message id proves that OpenCode accepted it.
        } else {
            return Err(error);
        }
    }
    observer.emit(crate::RuntimeObservation::StateChanged {
        runtime_ref: profile.id.clone(),
        process_epoch: generation.process_epoch,
        instance_epoch: Some(instance_epoch),
        state: "turn_started".to_string(),
        detail: None,
    });

    run_accepted_turn(
        inner,
        profile,
        generation,
        turn,
        session,
        instance_epoch,
        user_message_id,
        bootstrap.children,
        events,
        observer,
        control,
    )
    .await
}

#[derive(Debug)]
struct BootstrapState {
    hydrated: HydratedInstance,
    children: HashMap<String, String>,
    permissions: BTreeMap<String, PermissionRequest>,
    questions: BTreeMap<String, QuestionRequest>,
    root_session_id: String,
}

impl BootstrapState {
    fn new(session: &SessionInfo, hydrated: HydratedInstance) -> Self {
        let children = child_map(&hydrated.children);
        let permissions = hydrated
            .permissions
            .iter()
            .cloned()
            .map(|request| (request.id.clone(), request))
            .collect();
        let questions = hydrated
            .questions
            .iter()
            .cloned()
            .map(|request| (request.id.clone(), request))
            .collect();
        Self {
            hydrated,
            children,
            permissions,
            questions,
            root_session_id: session.id.clone(),
        }
    }

    fn known_sessions(&self) -> HashSet<String> {
        std::iter::once(self.root_session_id.clone())
            .chain(self.children.keys().cloned())
            .collect()
    }

    fn accepts_session(&self, session_id: &str) -> bool {
        session_id == self.root_session_id || self.children.contains_key(session_id)
    }

    fn apply(&mut self, event: &types::NativeEvent) {
        if let Some(session) = session_from_event(event)
            && let Some(parent_id) = session.parent_id
            && self.accepts_session(&parent_id)
        {
            self.children.insert(session.id, parent_id);
        }
        if let Some(request) = permission_from_event(event)
            && self.accepts_session(&request.session_id)
        {
            self.permissions.insert(request.id.clone(), request);
        }
        if let Some(request) = question_from_event(event)
            && self.accepts_session(&request.session_id)
        {
            self.questions.insert(request.id.clone(), request);
        }
        if let Some(request_id) = resolved_interaction_id(event) {
            self.permissions.remove(request_id);
            self.questions.remove(request_id);
        }
        if let Some((session_id, todos)) = todos_from_event(event)
            && self.accepts_session(session_id)
        {
            self.hydrated.todos.insert(session_id.to_string(), todos);
        }
        if let Some((session_id, diff)) = diff_from_event(event)
            && self.accepts_session(session_id)
        {
            self.hydrated.diffs.insert(session_id.to_string(), diff);
        }
    }
}

async fn drain_bootstrap_events(
    generation: &Generation,
    cwd: &Path,
    instance_epoch: u64,
    events: &mut broadcast::Receiver<GenerationSignal>,
    state: &mut BootstrapState,
) -> Result<(), RuntimeError> {
    loop {
        if generation.instance_epoch(cwd).await != instance_epoch {
            return Err(stale_instance_error(RuntimeErrorStage::Hydration));
        }
        match events.try_recv() {
            Ok(GenerationSignal::Event(event)) => {
                if is_disposed(&event, cwd) {
                    generation.bump_instance_epoch(cwd).await;
                    return Err(stale_instance_error(RuntimeErrorStage::Hydration));
                }
                let known_sessions = state.known_sessions();
                if event_matches(&event, cwd, known_sessions.iter().cloned()) {
                    if let Some((session_id, _)) = todos_from_event(&event)
                        && known_sessions.contains(session_id)
                    {
                        generation.mark_todo_sse_reconciled(cwd).await;
                    }
                    if let Some((session_id, _)) = diff_from_event(&event)
                        && known_sessions.contains(session_id)
                    {
                        generation.mark_diff_sse_reconciled(cwd).await;
                    }
                    if let Some(session) = session_from_event(&event) {
                        generation.observe_session(cwd, &session).await;
                    }
                    state.apply(&event);
                }
            }
            Ok(GenerationSignal::StreamClosed(message)) => {
                return Err(RuntimeError::new(
                    "event_gap",
                    RuntimeErrorStage::Hydration,
                    RetryClass::Reconnect,
                    message,
                ));
            }
            Ok(GenerationSignal::ProcessExited(code)) => {
                return Err(RuntimeError::new(
                    "process_exit",
                    RuntimeErrorStage::Hydration,
                    RetryClass::Reconnect,
                    format!("OpenCode exited during hydration with code {code:?}"),
                ));
            }
            Err(broadcast::error::TryRecvError::Empty) => return Ok(()),
            Err(broadcast::error::TryRecvError::Closed) => {
                return Err(RuntimeError::new(
                    "event_gap",
                    RuntimeErrorStage::Hydration,
                    RetryClass::Reconnect,
                    "OpenCode event channel closed during hydration",
                ));
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                return Err(RuntimeError::new(
                    "event_gap",
                    RuntimeErrorStage::Hydration,
                    RetryClass::Reconnect,
                    "OpenCode events exceeded the hydration buffer",
                ));
            }
        }
    }
}

async fn publish_bootstrap(
    inner: &Arc<Inner>,
    generation: &Arc<Generation>,
    turn: &RuntimeTurnRequest,
    instance_epoch: u64,
    state: &BootstrapState,
    observer: &RuntimeObserver,
) {
    publish_timeline(
        observer,
        &generation.runtime_ref,
        turn,
        state
            .hydrated
            .todos
            .get(&state.root_session_id)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        state
            .hydrated
            .diffs
            .get(&state.root_session_id)
            .map(Vec::as_slice)
            .unwrap_or_default(),
    );
    for child in &state.hydrated.children {
        observer.emit(crate::RuntimeObservation::ChildChanged {
            runtime_ref: generation.runtime_ref.clone(),
            parent_native_session_id: child
                .parent_id
                .clone()
                .unwrap_or_else(|| state.root_session_id.clone()),
            native_session_id: child.id.clone(),
            thread_id: None,
            status: state
                .hydrated
                .statuses
                .get(&child.id)
                .map(|status| status.kind.clone())
                .unwrap_or_else(|| "idle".to_string()),
            read_only: true,
        });
    }
    for permission in state.permissions.values() {
        publish_permission(
            inner,
            generation,
            turn,
            instance_epoch,
            &state.children,
            permission.clone(),
            observer,
        )
        .await;
    }
    for question in state.questions.values() {
        publish_question(
            inner,
            generation,
            turn,
            instance_epoch,
            &state.children,
            question.clone(),
            observer,
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_accepted_turn(
    inner: Arc<Inner>,
    profile: RuntimeProfile,
    generation: Arc<Generation>,
    turn: RuntimeTurnRequest,
    session: SessionInfo,
    instance_epoch: u64,
    user_message_id: String,
    mut children: HashMap<String, String>,
    mut events: broadcast::Receiver<GenerationSignal>,
    observer: RuntimeObserver,
    control: RuntimeControl,
) -> Result<RuntimeTurnResult, RuntimeError> {
    let mut assistant_ids = HashSet::new();
    let mut saw_session_error = false;
    loop {
        tokio::select! {
            _ = control.cancelled() => {
                let abort = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    generation.http.abort(&turn.cwd, &session.id),
                ).await;
                let sessions = std::iter::once(session.id.clone())
                    .chain(children.keys().cloned())
                    .collect::<HashSet<_>>();
                expire_sessions(
                    &inner,
                    generation.process_epoch,
                    instance_epoch,
                    &sessions,
                )
                .await;
                return Ok(RuntimeTurnResult {
                    turn_id: turn.turn_id,
                    thread_id: turn.thread_id,
                    native_session_id: session.id,
                    outcome: RuntimeTurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: String::new(),
                    model: String::new(),
                    history_fidelity: HistoryFidelity::Partial,
                    process_epoch: generation.process_epoch,
                    instance_epoch: Some(instance_epoch),
                    terminal_error: None,
                    metadata: Some(json!({
                        "nativeUserMessageId": user_message_id,
                        "nativeAbortObserved": matches!(abort, Ok(Ok(()))),
                    })),
                });
            }
            received = events.recv() => {
                let signal = match received {
                    Ok(signal) => signal,
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        return Ok(failed_turn(
                            &turn,
                            &session.id,
                            &generation,
                            instance_epoch,
                            "event_gap",
                            format!("OpenCode event stream dropped {count} events"),
                            &user_message_id,
                        ));
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Ok(failed_turn(
                            &turn,
                            &session.id,
                            &generation,
                            instance_epoch,
                            "event_gap",
                            "OpenCode event channel closed".to_string(),
                            &user_message_id,
                        ));
                    }
                };
                match signal {
                    GenerationSignal::StreamClosed(message) => {
                        return Ok(failed_turn(
                            &turn,
                            &session.id,
                            &generation,
                            instance_epoch,
                            "event_gap",
                            message,
                            &user_message_id,
                        ));
                    }
                    GenerationSignal::ProcessExited(code) => {
                        expire_process(&inner, generation.process_epoch).await;
                        return Ok(failed_turn(
                            &turn,
                            &session.id,
                            &generation,
                            instance_epoch,
                            "process_exit",
                            format!("OpenCode exited with code {code:?}"),
                            &user_message_id,
                        ));
                    }
                    GenerationSignal::Event(event) => {
                        if generation.instance_epoch(&turn.cwd).await != instance_epoch {
                            return Ok(failed_turn(
                                &turn,
                                &session.id,
                                &generation,
                                instance_epoch,
                                "stale_epoch",
                                "OpenCode directory instance changed".to_string(),
                                &user_message_id,
                            ));
                        }
                        if is_disposed(&event, &turn.cwd) {
                            generation.bump_instance_epoch(&turn.cwd).await;
                            expire_instance(&inner, generation.process_epoch, instance_epoch).await;
                            return Ok(failed_turn(
                                &turn,
                                &session.id,
                                &generation,
                                instance_epoch,
                                "stale_epoch",
                                "OpenCode disposed the directory instance".to_string(),
                                &user_message_id,
                            ));
                        }
                        let known = std::iter::once(session.id.clone())
                            .chain(children.keys().cloned())
                            .collect::<Vec<_>>();
                        if !event_matches(&event, &turn.cwd, known.iter().cloned()) {
                            continue;
                        }
                        if let Some(request_id) = resolved_interaction_id(&event) {
                            remove_pending_native(&inner, generation.process_epoch, request_id).await;
                        }
                        if let Some(child) = session_from_event(&event) {
                            generation.observe_session(&turn.cwd, &child).await;
                            if let Some(parent_id) = child.parent_id.clone()
                                && (parent_id == session.id || children.contains_key(&parent_id))
                            {
                                children.insert(child.id.clone(), parent_id.clone());
                                observer.emit(crate::RuntimeObservation::ChildChanged {
                                    runtime_ref: profile.id.clone(),
                                    parent_native_session_id: parent_id,
                                    native_session_id: child.id,
                                    thread_id: None,
                                    status: "idle".to_string(),
                                    read_only: true,
                                });
                            }
                        }
                        if let Some(permission) = permission_from_event(&event)
                            && (permission.session_id == session.id || children.contains_key(&permission.session_id))
                        {
                            publish_permission(
                                &inner,
                                &generation,
                                &turn,
                                instance_epoch,
                                &children,
                                permission,
                                &observer,
                            ).await;
                        }
                        if let Some(question) = question_from_event(&event)
                            && (question.session_id == session.id || children.contains_key(&question.session_id))
                        {
                            publish_question(
                                &inner,
                                &generation,
                                &turn,
                                instance_epoch,
                                &children,
                                question,
                                &observer,
                            ).await;
                        }
                        if let Some((session_id, todos)) = todos_from_event(&event)
                            && session_id == session.id
                        {
                            generation.mark_todo_sse_reconciled(&turn.cwd).await;
                            publish_plan(
                                &observer,
                                &profile.id,
                                &turn,
                                &todos,
                            );
                        }
                        if let Some((session_id, diff)) = diff_from_event(&event)
                            && session_id == session.id
                        {
                            generation.mark_diff_sse_reconciled(&turn.cwd).await;
                            observer.emit(crate::RuntimeObservation::DiffUpdated(
                                runtime_diff_update(
                                    &profile.id,
                                    &turn.thread_id,
                                    &turn.turn_id,
                                    &diff,
                                ),
                            ));
                        }
                        if let Some(message) = message_from_event(&event)
                            && message.role == "assistant"
                            && message.parent_id.as_deref() == Some(&user_message_id)
                        {
                            assistant_ids.insert(message.id);
                        }
                        if let Some(delta) = text_delta(&event, &assistant_ids) {
                            observer.emit(crate::RuntimeObservation::TextDelta {
                                turn_id: turn.turn_id.clone(),
                                text: delta.to_string(),
                            });
                        }
                        if let Some(tool) = tool_observation(&event, &assistant_ids) {
                            observer.emit(crate::RuntimeObservation::Tool {
                                turn_id: turn.turn_id.clone(),
                                item_id: tool.id,
                                name: tool.name,
                                status: tool.status,
                                detail: tool.detail,
                            });
                        }
                        if event.event_type == "session.error" && event.session_id() == Some(&session.id) {
                            saw_session_error = true;
                        }
                        if event.event_type == "session.deleted" && event.session_id() == Some(&session.id) {
                            return Ok(failed_turn(
                                &turn,
                                &session.id,
                                &generation,
                                instance_epoch,
                                "session_deleted",
                                "OpenCode deleted the active native session".to_string(),
                                &user_message_id,
                            ));
                        }
                        if status_is_idle(&event, &session.id) {
                            let (messages, _) = generation.http.messages(&turn.cwd, &session.id, None).await?;
                            let matching = matching_assistants(&messages, &user_message_id);
                            if let Some(last) = matching.last() {
                                let failed = last.info.error.is_some();
                                return Ok(RuntimeTurnResult {
                                    turn_id: turn.turn_id,
                                    thread_id: turn.thread_id,
                                    native_session_id: session.id,
                                    outcome: if failed {
                                        RuntimeTurnOutcome::Failed
                                    } else {
                                        RuntimeTurnOutcome::Completed
                                    },
                                    final_answer: final_answer(&matching),
                                    provider: last.info.provider_id.clone().unwrap_or_default(),
                                    model: last.info.model_id.clone().unwrap_or_default(),
                                    history_fidelity: HistoryFidelity::Partial,
                                    process_epoch: generation.process_epoch,
                                    instance_epoch: Some(instance_epoch),
                                    terminal_error: failed.then(|| opencode_terminal_error(
                                        "runtime_error",
                                        generation.process_epoch,
                                    )),
                                    metadata: Some(json!({
                                        "nativeUserMessageId": user_message_id,
                                        "nativeAssistantMessageIds": matching.iter().map(|message| &message.info.id).collect::<Vec<_>>(),
                                        "error": last.info.error,
                                    })),
                                });
                            }
                            if saw_session_error {
                                return Ok(failed_turn(
                                    &turn,
                                    &session.id,
                                    &generation,
                                    instance_epoch,
                                    "runtime_error",
                                    "OpenCode ended the turn with a session error".to_string(),
                                    &user_message_id,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn publish_timeline(
    observer: &RuntimeObserver,
    runtime_ref: &str,
    turn: &RuntimeTurnRequest,
    todos: &[types::TodoInfo],
    diff: &[types::FileDiffInfo],
) {
    publish_plan(observer, runtime_ref, turn, todos);
    observer.emit(crate::RuntimeObservation::DiffUpdated(runtime_diff_update(
        runtime_ref,
        &turn.thread_id,
        &turn.turn_id,
        diff,
    )));
}

fn publish_plan(
    observer: &RuntimeObserver,
    runtime_ref: &str,
    turn: &RuntimeTurnRequest,
    todos: &[types::TodoInfo],
) {
    if let Some(update) = runtime_plan_update(runtime_ref, &turn.thread_id, &turn.turn_id, todos) {
        observer.emit(crate::RuntimeObservation::PlanUpdated(update));
    } else {
        observer.emit(crate::RuntimeObservation::Warning {
            code: "opencode_todo_status_unsupported".to_string(),
            message: "OpenCode returned a todo status this adapter cannot project safely"
                .to_string(),
            diagnostic_ref: None,
        });
    }
}

fn stale_instance_error(stage: RuntimeErrorStage) -> RuntimeError {
    RuntimeError::new(
        "stale_epoch",
        stage,
        RetryClass::Reconnect,
        "OpenCode directory instance changed during hydration",
    )
}

async fn publish_permission(
    inner: &Arc<Inner>,
    generation: &Arc<Generation>,
    turn: &RuntimeTurnRequest,
    instance_epoch: u64,
    children: &HashMap<String, String>,
    request: PermissionRequest,
    observer: &RuntimeObserver,
) {
    let id = interaction_id(
        generation.process_epoch,
        instance_epoch,
        "permission",
        &request.id,
    );
    let pending = PendingInteraction {
        id: id.clone(),
        native_id: request.id.clone(),
        kind: PendingKind::Permission,
        generation: generation.clone(),
        cwd: turn.cwd.clone(),
        session_id: request.session_id.clone(),
        process_epoch: generation.process_epoch,
        instance_epoch,
    };
    if !insert_pending(inner, pending).await {
        return;
    }
    let parent = children.get(&request.session_id).cloned();
    let allow_always = !request.always.is_empty();
    let mut choices = vec![RuntimeInteractionChoice {
        id: "once".to_string(),
        label: "Allow once".to_string(),
        decision: "once".to_string(),
    }];
    if allow_always {
        choices.push(RuntimeInteractionChoice {
            id: "always".to_string(),
            label: "Allow until restart".to_string(),
            decision: "always".to_string(),
        });
    }
    choices.push(RuntimeInteractionChoice {
        id: "reject".to_string(),
        label: "Reject".to_string(),
        decision: "reject".to_string(),
    });
    observer.emit(crate::RuntimeObservation::Interaction(Box::new(
        RuntimeInteraction {
            id,
            policy: RuntimeInteractionPolicy {
                kind: RuntimeInteractionKind::Permission,
                stability: RuntimeStability::Stable,
                exposure: RuntimeInteractionExposure::Standard,
            },
            kind: "permission".to_string(),
            runtime_ref: generation.runtime_ref.clone(),
            thread_id: turn.thread_id.clone(),
            native_session_id: request.session_id.clone(),
            parent_native_session_id: parent.clone(),
            child_native_session_id: parent.map(|_| request.session_id.clone()),
            process_epoch: generation.process_epoch,
            instance_epoch: Some(instance_epoch),
            prompt: permission_prompt(&request),
            questions: Vec::new(),
            choices,
            authorization_lifetime: allow_always
                .then(|| "until_runtime_instance_restarts".to_string()),
            expires_at_ms: None,
            metadata: Some(json!({
                "permission": request.permission,
                "patterns": request.patterns,
                "always": request.always,
                "runtimeMetadata": request.metadata,
            })),
        },
    )));
}

async fn publish_question(
    inner: &Arc<Inner>,
    generation: &Arc<Generation>,
    turn: &RuntimeTurnRequest,
    instance_epoch: u64,
    children: &HashMap<String, String>,
    request: QuestionRequest,
    observer: &RuntimeObserver,
) {
    let id = interaction_id(
        generation.process_epoch,
        instance_epoch,
        "question",
        &request.id,
    );
    let pending = PendingInteraction {
        id: id.clone(),
        native_id: request.id.clone(),
        kind: PendingKind::Question,
        generation: generation.clone(),
        cwd: turn.cwd.clone(),
        session_id: request.session_id.clone(),
        process_epoch: generation.process_epoch,
        instance_epoch,
    };
    if !insert_pending(inner, pending).await {
        return;
    }
    let parent = children.get(&request.session_id).cloned();
    let questions = request
        .questions
        .iter()
        .map(|question| RuntimeInteractionQuestion {
            header: (!question.header.trim().is_empty()).then(|| question.header.clone()),
            question: question.question.clone(),
            options: question
                .options
                .iter()
                .map(|option| RuntimeInteractionQuestionOption {
                    label: option.label.clone(),
                    description: option.description.clone(),
                })
                .collect(),
            multiple: question.multiple,
            custom: question.custom,
            secret: false,
        })
        .collect();
    observer.emit(crate::RuntimeObservation::Interaction(Box::new(
        RuntimeInteraction {
            id,
            policy: RuntimeInteractionPolicy {
                kind: RuntimeInteractionKind::Question,
                stability: RuntimeStability::Stable,
                exposure: RuntimeInteractionExposure::Standard,
            },
            kind: "question".to_string(),
            runtime_ref: generation.runtime_ref.clone(),
            thread_id: turn.thread_id.clone(),
            native_session_id: request.session_id.clone(),
            parent_native_session_id: parent.clone(),
            child_native_session_id: parent.map(|_| request.session_id.clone()),
            process_epoch: generation.process_epoch,
            instance_epoch: Some(instance_epoch),
            prompt: request
                .questions
                .iter()
                .map(|question| format!("{}: {}", question.header, question.question))
                .collect::<Vec<_>>()
                .join("\n"),
            questions,
            choices: Vec::new(),
            authorization_lifetime: None,
            expires_at_ms: None,
            metadata: None,
        },
    )));
}

async fn insert_pending(inner: &Arc<Inner>, pending: PendingInteraction) -> bool {
    let mut state = inner.state.lock().await;
    if state.pending.contains_key(&pending.id) {
        return false;
    }
    state.pending.insert(pending.id.clone(), pending);
    true
}

async fn remove_pending_native(inner: &Arc<Inner>, process_epoch: u64, native_id: &str) {
    inner.state.lock().await.pending.retain(|_, pending| {
        pending.process_epoch != process_epoch || pending.native_id != native_id
    });
}

async fn expire_process(inner: &Arc<Inner>, process_epoch: u64) {
    inner
        .state
        .lock()
        .await
        .pending
        .retain(|_, pending| pending.process_epoch != process_epoch);
}

async fn expire_instance(inner: &Arc<Inner>, process_epoch: u64, instance_epoch: u64) {
    inner.state.lock().await.pending.retain(|_, pending| {
        pending.process_epoch != process_epoch || pending.instance_epoch != instance_epoch
    });
}

async fn expire_sessions(
    inner: &Arc<Inner>,
    process_epoch: u64,
    instance_epoch: u64,
    sessions: &HashSet<String>,
) {
    inner.state.lock().await.pending.retain(|_, pending| {
        pending.process_epoch != process_epoch
            || pending.instance_epoch != instance_epoch
            || !sessions.contains(&pending.session_id)
    });
}

async fn execute_interaction(
    inner: Arc<Inner>,
    response: crate::RuntimeInteractionResponse,
) -> Result<RuntimeInteractionResult, RuntimeError> {
    let pending = {
        let state = inner.state.lock().await;
        state.pending.get(&response.interaction_id).cloned()
    };
    let Some(pending) = pending else {
        return Ok(RuntimeInteractionResult {
            accepted: false,
            expired: true,
            message: Some("OpenCode interaction is no longer pending".to_string()),
        });
    };
    if response.process_epoch != pending.process_epoch
        || response.instance_epoch != Some(pending.instance_epoch)
        || !pending.generation.is_usable()
    {
        inner.state.lock().await.pending.remove(&pending.id);
        return Ok(RuntimeInteractionResult {
            accepted: false,
            expired: true,
            message: Some("OpenCode interaction belongs to a stale runtime epoch".to_string()),
        });
    }
    let accepted = match pending.kind {
        PendingKind::Permission => {
            let body = permission_response_body(&response.response)?;
            pending
                .generation
                .http
                .reply_permission(&pending.cwd, &pending.native_id, &body)
                .await?
        }
        PendingKind::Question => {
            if response
                .response
                .get("reject")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                pending
                    .generation
                    .http
                    .reject_question(&pending.cwd, &pending.native_id)
                    .await?
            } else {
                let body = question_response_body(&response.response)?;
                pending
                    .generation
                    .http
                    .reply_question(&pending.cwd, &pending.native_id, &body)
                    .await?
            }
        }
    };
    inner.state.lock().await.pending.remove(&pending.id);
    Ok(RuntimeInteractionResult {
        accepted,
        expired: !accepted,
        message: (!accepted)
            .then(|| "OpenCode interaction expired before the response arrived".to_string()),
    })
}

fn permission_response_body(response: &Value) -> Result<Value, RuntimeError> {
    let decision = response
        .as_str()
        .or_else(|| response.get("decision").and_then(Value::as_str))
        .or_else(|| response.get("reply").and_then(Value::as_str))
        .ok_or_else(|| interaction_error("permission response must contain a decision"))?;
    if !matches!(decision, "once" | "always" | "reject") {
        return Err(interaction_error(
            "permission decision must be once, always, or reject",
        ));
    }
    Ok(json!({
        "reply": decision,
        "message": response.get("message").and_then(Value::as_str),
    }))
}

fn question_response_body(response: &Value) -> Result<Value, RuntimeError> {
    let answers = response
        .get("answers")
        .cloned()
        .unwrap_or_else(|| response.clone());
    if !answers.is_array() {
        return Err(interaction_error(
            "question response must contain an answers array",
        ));
    }
    Ok(json!({ "answers": answers }))
}

fn interaction_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(
        "invalid_interaction_response",
        RuntimeErrorStage::Interaction,
        RetryClass::UserAction,
        message,
    )
}

fn permission_prompt(request: &PermissionRequest) -> String {
    if request.patterns.is_empty() {
        format!("Allow OpenCode permission `{}`?", request.permission)
    } else {
        format!(
            "Allow OpenCode permission `{}` for {}?",
            request.permission,
            request.patterns.join(", ")
        )
    }
}

fn interaction_id(process_epoch: u64, instance_epoch: u64, kind: &str, native_id: &str) -> String {
    format!("opencode:{process_epoch}:{instance_epoch}:{kind}:{native_id}")
}

async fn execute_session(
    inner: Arc<Inner>,
    profile: RuntimeProfile,
    request: RuntimeSessionRequest,
) -> Result<RuntimeSessionResult, RuntimeError> {
    let generation = acquire_generation(&inner, &profile, &request.cwd).await?;
    match request.operation {
        RuntimeSessionOperation::List => {
            let (sessions, statuses) = tokio::try_join!(
                generation.http.sessions(&request.cwd),
                generation.http.statuses(&request.cwd),
            )?;
            generation.observe_sessions(&request.cwd, &sessions).await;
            let sessions = sessions
                .iter()
                .map(|session| {
                    runtime_session(
                        session,
                        Vec::new(),
                        None,
                        status_ownership(&statuses, &session.id),
                    )
                })
                .collect();
            Ok(RuntimeSessionResult {
                changed: false,
                sessions,
                cursor: None,
                message: None,
            })
        }
        RuntimeSessionOperation::Read | RuntimeSessionOperation::Resume => {
            let session_id = required_session_id(&request)?;
            let info = required_session(&generation, &request.cwd, session_id).await?;
            generation.observe_session(&request.cwd, &info).await;
            let ((messages, cursor), statuses) = tokio::try_join!(
                generation
                    .http
                    .messages(&request.cwd, session_id, request.cursor.as_deref()),
                generation.http.statuses(&request.cwd),
            )?;
            let ownership = if request.operation == RuntimeSessionOperation::Resume
                && !matches!(
                    statuses.get(session_id).map(|status| status.kind.as_str()),
                    Some("busy" | "retry")
                ) {
                SessionOwnership::ReadWrite
            } else {
                status_ownership(&statuses, session_id)
            };
            Ok(RuntimeSessionResult {
                changed: request.operation == RuntimeSessionOperation::Resume,
                sessions: vec![runtime_session(&info, messages, cursor.clone(), ownership)],
                cursor,
                message: None,
            })
        }
        RuntimeSessionOperation::Fork => {
            let session_id = required_session_id(&request)?;
            let info = generation
                .http
                .fork(&request.cwd, session_id, request.argument.as_ref())
                .await?;
            generation.observe_session(&request.cwd, &info).await;
            changed_session(info, SessionOwnership::ReadOnly, "OpenCode session forked")
        }
        RuntimeSessionOperation::Revert => {
            let session_id = required_session_id(&request)?;
            let argument = request.argument.as_ref().ok_or_else(|| {
                RuntimeError::new(
                    "missing_argument",
                    RuntimeErrorStage::History,
                    RetryClass::UserAction,
                    "OpenCode revert requires a native message argument",
                )
            })?;
            let info = generation
                .http
                .revert(&request.cwd, session_id, argument)
                .await?;
            generation.observe_session(&request.cwd, &info).await;
            changed_session(info, SessionOwnership::ReadWrite, "OpenCode revert staged")
        }
        RuntimeSessionOperation::Unrevert => {
            let session_id = required_session_id(&request)?;
            let info = generation.http.unrevert(&request.cwd, session_id).await?;
            generation.observe_session(&request.cwd, &info).await;
            changed_session(info, SessionOwnership::ReadWrite, "OpenCode revert cleared")
        }
        RuntimeSessionOperation::Rename => {
            let session_id = required_session_id(&request)?;
            let title = request
                .argument
                .as_ref()
                .and_then(|argument| {
                    argument
                        .as_str()
                        .or_else(|| argument.get("title").and_then(Value::as_str))
                })
                .ok_or_else(|| {
                    RuntimeError::new(
                        "missing_argument",
                        RuntimeErrorStage::History,
                        RetryClass::UserAction,
                        "OpenCode rename requires a title",
                    )
                })?;
            let info = generation
                .http
                .rename(&request.cwd, session_id, title)
                .await?;
            generation.observe_session(&request.cwd, &info).await;
            changed_session(
                info,
                SessionOwnership::ReadWrite,
                "OpenCode session renamed",
            )
        }
        RuntimeSessionOperation::Archive => {
            let session_id = required_session_id(&request)?;
            let now = now_ms();
            let info = generation
                .http
                .archive(&request.cwd, session_id, now)
                .await?;
            generation.observe_session(&request.cwd, &info).await;
            changed_session(
                info,
                SessionOwnership::ReadWrite,
                "OpenCode session archived",
            )
        }
        RuntimeSessionOperation::Unarchive => Err(RuntimeError::new(
            "unsupported",
            RuntimeErrorStage::History,
            RetryClass::UserAction,
            "this OpenCode stable HTTP surface cannot safely express unarchive",
        )),
        RuntimeSessionOperation::Delete => {
            let session_id = required_session_id(&request)?;
            generation.http.delete(&request.cwd, session_id).await?;
            generation.forget_session(&request.cwd, session_id).await;
            Ok(RuntimeSessionResult {
                changed: true,
                sessions: Vec::new(),
                cursor: None,
                message: Some(
                    "OpenCode session and its recursive native children were deleted".to_string(),
                ),
            })
        }
    }
}

fn changed_session(
    info: SessionInfo,
    ownership: SessionOwnership,
    message: &str,
) -> Result<RuntimeSessionResult, RuntimeError> {
    Ok(RuntimeSessionResult {
        changed: true,
        sessions: vec![runtime_session(&info, Vec::new(), None, ownership)],
        cursor: None,
        message: Some(message.to_string()),
    })
}

fn required_session_id(request: &RuntimeSessionRequest) -> Result<&str, RuntimeError> {
    request.native_session_id.as_deref().ok_or_else(|| {
        RuntimeError::new(
            "missing_native_session",
            RuntimeErrorStage::History,
            RetryClass::UserAction,
            "OpenCode session operation requires a native session id",
        )
    })
}

async fn required_session(
    generation: &Generation,
    cwd: &Path,
    session_id: &str,
) -> Result<SessionInfo, RuntimeError> {
    generation
        .http
        .session(cwd, session_id)
        .await?
        .ok_or_else(|| {
            RuntimeError::new(
                "session_not_found",
                RuntimeErrorStage::History,
                RetryClass::UserAction,
                "OpenCode native session was not found",
            )
        })
}

async fn ensure_session(
    generation: &Generation,
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
) -> Result<SessionInfo, RuntimeError> {
    if let Some(session_id) = turn.native_session_id.as_deref() {
        return required_session(generation, &turn.cwd, session_id).await;
    }
    let selected_agent = turn
        .agent
        .clone()
        .or(turn.mode.clone())
        .or(profile.default_agent.clone())
        .or(profile.default_mode.clone());
    generation
        .http
        .create_session(
            &turn.cwd,
            &SessionCreateBody {
                title: format!("Psychevo {}", turn.thread_id),
                agent: selected_agent,
                model: parse_model(turn.model.as_deref().or(profile.default_model.as_deref())),
            },
        )
        .await
}

async fn acquire_generation(
    inner: &Arc<Inner>,
    profile: &RuntimeProfile,
    cwd: &Path,
) -> Result<Arc<Generation>, RuntimeError> {
    let launch = prepare_launch(profile, cwd)?;
    {
        let state = inner.state.lock().await;
        if let Some(generation) = state.generations.get(&launch.key)
            && generation.is_usable()
        {
            return Ok(generation.clone());
        }
    }
    let _spawn = inner.spawn.lock().await;
    {
        let state = inner.state.lock().await;
        if let Some(generation) = state.generations.get(&launch.key)
            && generation.is_usable()
        {
            return Ok(generation.clone());
        }
    }
    let previous_epoch = {
        let mut state = inner.state.lock().await;
        state
            .generations
            .remove(&launch.key)
            .map(|generation| generation.process_epoch)
    };
    if let Some(previous_epoch) = previous_epoch {
        expire_process(inner, previous_epoch).await;
    }
    let process_epoch = inner.next_process_epoch.fetch_add(1, Ordering::SeqCst);
    let key = launch.key.clone();
    let generation = spawn_generation(launch, profile.id.clone(), process_epoch).await?;
    let mut state = inner.state.lock().await;
    state.generations.insert(key, generation.clone());
    Ok(generation)
}

async fn shutdown(inner: Arc<Inner>, mode: ShutdownMode) -> Result<(), RuntimeError> {
    let (generations, force) = {
        let mut state = inner.state.lock().await;
        let force = match &mode {
            ShutdownMode::Force => true,
            ShutdownMode::Graceful => false,
            ShutdownMode::Runtime { force, .. } => *force,
        };
        let selected = match &mode {
            ShutdownMode::Runtime {
                kind, runtime_ref, ..
            } if *kind == RuntimeKind::OpenCode => state
                .generations
                .iter()
                .filter(|(_, generation)| {
                    runtime_ref
                        .as_ref()
                        .is_none_or(|runtime_ref| generation.runtime_ref == *runtime_ref)
                })
                .map(|(key, generation)| (key.clone(), generation.clone()))
                .collect::<Vec<_>>(),
            ShutdownMode::Runtime { .. } => Vec::new(),
            ShutdownMode::Graceful | ShutdownMode::Force => state
                .generations
                .iter()
                .map(|(key, generation)| (key.clone(), generation.clone()))
                .collect(),
        };
        if force {
            for (key, generation) in &selected {
                state.generations.remove(key);
                state
                    .pending
                    .retain(|_, pending| pending.process_epoch != generation.process_epoch);
            }
        }
        (selected, force)
    };
    let shutdowns = generations
        .into_iter()
        .map(|(key, generation)| {
            let inner = Arc::clone(&inner);
            Box::pin(async move {
                let shutdown_completed = generation.shutdown(force).await;
                if !force && shutdown_completed {
                    remove_generation_after_graceful_shutdown(&inner, &key, &generation).await;
                }
            }) as DetachedShutdown
        })
        .collect();
    run_shutdowns_concurrently(shutdowns).await;
    Ok(())
}

async fn remove_generation_after_graceful_shutdown(
    inner: &Arc<Inner>,
    key: &str,
    generation: &Arc<Generation>,
) {
    let mut state = inner.state.lock().await;
    let is_current = state
        .generations
        .get(key)
        .is_some_and(|current| Arc::ptr_eq(current, generation));
    if !is_current {
        return;
    }
    state.generations.remove(key);
    state
        .pending
        .retain(|_, pending| pending.process_epoch != generation.process_epoch);
}

type DetachedShutdown = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

async fn run_shutdowns_concurrently(shutdowns: Vec<DetachedShutdown>) {
    let tasks = shutdowns.into_iter().map(tokio::spawn).collect::<Vec<_>>();
    for task in tasks {
        let _ = task.await;
    }
}

fn validate_request(request: &ExecuteRequest) -> Result<(), RuntimeError> {
    if request.profile.kind != RuntimeKind::OpenCode {
        return Err(wrong_kind());
    }
    if !request.profile.enabled {
        return Err(RuntimeError::new(
            "runtime_disabled",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode Runtime Profile is disabled",
        ));
    }
    if request.expected_profile_revision != request.profile.revision {
        return Err(RuntimeError::new(
            "stale_revision",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode Runtime Profile revision changed before execution",
        ));
    }
    if request
        .expected_capability_revision
        .is_some_and(|revision| revision != request.profile.revision)
    {
        return Err(RuntimeError::new(
            "stale_revision",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode capability revision changed before execution",
        ));
    }
    Ok(())
}

fn validate_turn_policy(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
) -> Result<(), RuntimeError> {
    if profile
        .sandbox
        .as_deref()
        .is_some_and(|value| !value.is_empty())
        || !profile.workspace_roots.is_empty()
        || profile
            .approval_mode
            .as_deref()
            .is_some_and(|value| !value.is_empty() && value != "default")
    {
        return Err(RuntimeError::new(
            "policy_not_enforceable",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode cannot exactly enforce this Runtime Profile safety policy",
        ));
    }
    if !turn.features.is_empty() {
        return Err(RuntimeError::new(
            "unsupported",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "OpenCode turn features contain unsupported required contributions",
        ));
    }
    Ok(())
}

fn select_agent(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
    agents: &[types::AgentInfo],
) -> Result<Option<String>, RuntimeError> {
    let selected = turn
        .agent
        .as_deref()
        .or(turn.mode.as_deref())
        .or(profile.default_agent.as_deref())
        .or(profile.default_mode.as_deref());
    let Some(selected) = selected else {
        return Ok(None);
    };
    let agent = agents
        .iter()
        .find(|agent| agent.name == selected)
        .ok_or_else(|| {
            RuntimeError::new(
                "unsupported_agent",
                RuntimeErrorStage::Configuration,
                RetryClass::UserAction,
                format!("OpenCode agent `{selected}` was not observed in the active directory"),
            )
        })?;
    if agent.hidden || agent.mode == "subagent" {
        return Err(RuntimeError::new(
            "unsupported_agent",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            format!("OpenCode agent `{selected}` is not a visible primary agent"),
        ));
    }
    Ok(Some(selected.to_string()))
}

fn ensure_session_directory(cwd: &Path, session: &SessionInfo) -> Result<(), RuntimeError> {
    let Some(directory) = session.directory.as_deref() else {
        return Ok(());
    };
    let expected = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let observed = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.into());
    if expected == observed {
        return Ok(());
    }
    Err(RuntimeError::new(
        "binding_mismatch",
        RuntimeErrorStage::Binding,
        RetryClass::UserAction,
        "OpenCode native session belongs to a different working directory",
    ))
}

fn failed_turn(
    turn: &RuntimeTurnRequest,
    session_id: &str,
    generation: &Generation,
    instance_epoch: u64,
    code: &str,
    message: String,
    user_message_id: &str,
) -> RuntimeTurnResult {
    RuntimeTurnResult {
        turn_id: turn.turn_id.clone(),
        thread_id: turn.thread_id.clone(),
        native_session_id: session_id.to_string(),
        outcome: RuntimeTurnOutcome::Failed,
        final_answer: String::new(),
        provider: String::new(),
        model: String::new(),
        history_fidelity: HistoryFidelity::Partial,
        process_epoch: generation.process_epoch,
        instance_epoch: Some(instance_epoch),
        terminal_error: Some(opencode_terminal_error(code, generation.process_epoch)),
        metadata: Some(json!({
            "code": code,
            "message": message,
            "nativeUserMessageId": user_message_id,
        })),
    }
}

fn opencode_terminal_error(code: &str, process_epoch: u64) -> RuntimeTerminalError {
    let (stage, retry_class, message) = match code {
        "event_gap" => (
            RuntimeErrorStage::Transport,
            RetryClass::UnknownDelivery,
            "OpenCode event continuity was lost.",
        ),
        "process_exit" => (
            RuntimeErrorStage::Transport,
            RetryClass::UnknownDelivery,
            "OpenCode exited before the turn completed.",
        ),
        "stale_epoch" => (
            RuntimeErrorStage::Binding,
            RetryClass::UnknownDelivery,
            "The OpenCode runtime instance changed before the turn completed.",
        ),
        "session_deleted" => (
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            "The active OpenCode session was deleted.",
        ),
        _ => (
            RuntimeErrorStage::Prompt,
            RetryClass::Never,
            "OpenCode failed the turn.",
        ),
    };
    RuntimeTerminalError {
        code: code.to_string(),
        stage,
        retry_class,
        message: message.to_string(),
        diagnostic_ref: format!("opencode-process-{process_epoch}-{code}"),
    }
}

fn discover_command(profile: &RuntimeProfile, cwd: Option<&Path>) -> bool {
    let Some(command) = profile.command.as_deref() else {
        return false;
    };
    let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
    env.extend(profile.env.clone());
    let fallback;
    let cwd = if let Some(cwd) = cwd {
        cwd
    } else {
        fallback = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        &fallback
    };
    psychevo_runtime::resolve_executable_path(
        command,
        cwd,
        &psychevo_runtime::ExecutableResolveOptions {
            platform: psychevo_runtime::HostPlatform::current(),
            env: &env,
        },
    )
    .is_some()
}

fn capabilities(
    version_compatible: bool,
    todos_validated: bool,
    diff_validated: bool,
) -> Vec<RuntimeCapability> {
    [
        "session.persistence",
        "session.list",
        "session.read",
        "session.resume",
        "session.fork",
        "session.revert",
        "session.unrevert",
        "session.rename",
        "session.archive",
        "session.delete",
        "turn.start",
        "turn.interrupt",
        "interaction.permission",
        "interaction.question",
        "timeline.todos",
        "timeline.diff",
        "children.read_only",
        "history.partial",
    ]
    .into_iter()
    .map(|id| RuntimeCapability {
        id: id.to_string(),
        enabled: match id {
            "timeline.todos" => version_compatible && todos_validated,
            "timeline.diff" => version_compatible && diff_validated,
            _ => version_compatible,
        },
        stability: RuntimeStability::Stable,
    })
    .collect()
}

fn opencode_runtime_version(version: &str) -> Option<(u64, u64, u64)> {
    let numeric = version.trim().split(['-', '+']).next()?;
    let mut components = numeric.split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    let patch = components.next()?.parse().ok()?;
    (components.next().is_none()).then_some((major, minor, patch))
}

fn wrong_kind() -> RuntimeError {
    RuntimeError::new(
        "wrong_runtime_kind",
        RuntimeErrorStage::Configuration,
        RetryClass::Never,
        "OpenCode adapter received a non-OpenCode Runtime Profile",
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod shutdown_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::Notify;
    use tokio::sync::mpsc;

    use super::process::{ProcessCommand, shutdown_test_generation};
    use super::*;

    #[test]
    fn event_gap_terminal_is_typed_as_unknown_delivery() {
        assert_eq!(
            opencode_terminal_error("event_gap", 41),
            RuntimeTerminalError {
                code: "event_gap".to_string(),
                stage: RuntimeErrorStage::Transport,
                retry_class: RetryClass::UnknownDelivery,
                message: "OpenCode event continuity was lost.".to_string(),
                diagnostic_ref: "opencode-process-41-event_gap".to_string(),
            }
        );
    }

    #[test]
    fn process_exit_terminal_is_typed_without_native_process_output() {
        let error = opencode_terminal_error("process_exit", 42);
        assert_eq!(error.code, "process_exit");
        assert_eq!(error.stage, RuntimeErrorStage::Transport);
        assert_eq!(error.retry_class, RetryClass::UnknownDelivery);
        assert_eq!(error.message, "OpenCode exited before the turn completed.");
        assert_eq!(error.diagnostic_ref, "opencode-process-42-process_exit");
        let public = serde_json::to_string(&error).expect("terminal error JSON");
        assert!(!public.contains("native-user-message"));
        assert!(!public.contains("stderr"));
    }

    async fn next_shutdown(
        commands: &mut mpsc::Receiver<ProcessCommand>,
    ) -> (bool, tokio::sync::oneshot::Sender<()>) {
        let command = tokio::time::timeout(Duration::from_secs(1), commands.recv())
            .await
            .expect("shutdown command should be sent before the test deadline")
            .expect("shutdown command channel should remain open");
        match command {
            ProcessCommand::Shutdown { force, done } => (force, done),
        }
    }

    #[tokio::test]
    async fn force_after_graceful_timeout_reaches_every_retained_generation() {
        let module = OpenCodeRuntimeModule::new();
        let (first, mut first_commands) = shutdown_test_generation("first", 1);
        let (second, mut second_commands) = shutdown_test_generation("second", 2);
        {
            let mut state = module.inner.state.lock().await;
            state.generations.insert("first".to_string(), first);
            state.generations.insert("second".to_string(), second);
        }

        let graceful_module = module.clone();
        let graceful = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_millis(100),
                graceful_module.shutdown(ShutdownMode::Graceful),
            )
            .await
        });
        let (first_force, first_graceful_done) = next_shutdown(&mut first_commands).await;
        let (second_force, second_graceful_done) = next_shutdown(&mut second_commands).await;
        assert!(!first_force);
        assert!(!second_force);
        assert!(
            graceful.await.expect("graceful task should join").is_err(),
            "the held generation shutdowns should exhaust the graceful deadline"
        );
        {
            let state = module.inner.state.lock().await;
            assert_eq!(state.generations.len(), 2);
            assert!(state.generations.contains_key("first"));
            assert!(state.generations.contains_key("second"));
        }
        module
            .shutdown(ShutdownMode::Graceful)
            .await
            .expect("a repeated graceful request should remain idempotent");
        assert_eq!(module.inner.state.lock().await.generations.len(), 2);

        let force_module = module.clone();
        let force = tokio::spawn(async move { force_module.shutdown(ShutdownMode::Force).await });
        let (first_force, first_force_done) = next_shutdown(&mut first_commands).await;
        let (second_force, second_force_done) = next_shutdown(&mut second_commands).await;
        assert!(first_force);
        assert!(second_force);
        {
            let state = module.inner.state.lock().await;
            assert!(state.generations.is_empty());
        }

        let _ = first_force_done.send(());
        let _ = second_force_done.send(());
        force
            .await
            .expect("forced task should join")
            .expect("forced shutdown should complete");
        let _ = first_graceful_done.send(());
        let _ = second_graceful_done.send(());
    }

    #[tokio::test]
    async fn cancellation_after_a_hanging_first_generation_still_starts_later_shutdowns() {
        let first_started = Arc::new(AtomicUsize::new(0));
        let second_started = Arc::new(AtomicUsize::new(0));
        let second_completed = Arc::new(AtomicUsize::new(0));
        let release_first = Arc::new(Notify::new());

        let first = {
            let first_started = Arc::clone(&first_started);
            let release_first = Arc::clone(&release_first);
            Box::pin(async move {
                first_started.fetch_add(1, Ordering::SeqCst);
                release_first.notified().await;
            }) as DetachedShutdown
        };
        let second = {
            let second_started = Arc::clone(&second_started);
            let second_completed = Arc::clone(&second_completed);
            Box::pin(async move {
                second_started.fetch_add(1, Ordering::SeqCst);
                second_completed.fetch_add(1, Ordering::SeqCst);
            }) as DetachedShutdown
        };

        let result = tokio::time::timeout(
            Duration::from_millis(20),
            run_shutdowns_concurrently(vec![first, second]),
        )
        .await;
        assert!(
            result.is_err(),
            "the first generation should hold the caller"
        );
        assert_eq!(first_started.load(Ordering::SeqCst), 1);
        assert_eq!(second_started.load(Ordering::SeqCst), 1);
        assert_eq!(second_completed.load(Ordering::SeqCst), 1);

        release_first.notify_waiters();
        tokio::task::yield_now().await;
    }
}
