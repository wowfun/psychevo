use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

use crate::{
    ControlState, ExecuteRequest, ExecuteResult, HistoryFidelity, ReadinessStage, ReadinessStatus,
    RetryClass, RuntimeAccountRateLimits, RuntimeAccountRateLimitsUpdate, RuntimeAuthOperation,
    RuntimeAuthRequest, RuntimeAuthResult, RuntimeCapability, RuntimeCompactionChange,
    RuntimeCompactionRequest, RuntimeCompactionResult, RuntimeCompactionStatus, RuntimeControl,
    RuntimeControlChoice, RuntimeControlDependency, RuntimeControlDescriptor,
    RuntimeCreditsSnapshot, RuntimeDiffUpdate, RuntimeError, RuntimeErrorStage,
    RuntimeExtensionRequest, RuntimeFuture, RuntimeGoal, RuntimeGoalChange, RuntimeGoalStatus,
    RuntimeIntent, RuntimeInteraction, RuntimeInteractionChoice, RuntimeInteractionExposure,
    RuntimeInteractionKind, RuntimeInteractionPolicy, RuntimeInteractionQuestion,
    RuntimeInteractionQuestionOption, RuntimeInteractionResult, RuntimeKind, RuntimeModule,
    RuntimeObservation, RuntimeObserver, RuntimePlanUpdate, RuntimeProfile,
    RuntimeRateLimitReachedType, RuntimeRateLimitSnapshot, RuntimeRateLimitWindow,
    RuntimeSessionBinding, RuntimeSessionOperation, RuntimeSessionRequest, RuntimeSessionResult,
    RuntimeSnapshot, RuntimeSpendControlLimitSnapshot, RuntimeStability, RuntimeTerminalError,
    RuntimeTurnOutcome, RuntimeTurnRequest, RuntimeTurnResult, RuntimeUsageUpdate, ShutdownMode,
    SnapshotMode, SnapshotQuery, SnapshotScope,
};

use super::projection::{ActiveTurn, NativeTerminal, session_from_thread};
use super::transport::{CodexTransport, TransportEvent, TransportEventSink};
use super::wire::{RequestId, string};

const ADAPTER_VERSION: &str = "codex-app-server-v1";
const CAPABILITY_REVISION: u64 = 1;
const INTERRUPT_TERMINAL_TIMEOUT: Duration = Duration::from_secs(5);
const CODEX_STABLE_MATRIX_MIN_VERSION: (u64, u64, u64) = (0, 143, 0);
const MODEL_CATALOG_PAGE_LIMIT: usize = 32;

fn codex_process_terminal_error(
    diagnostic_ref: Option<&str>,
    process_epoch: u64,
) -> RuntimeTerminalError {
    RuntimeTerminalError {
        code: "process_exit".to_string(),
        stage: RuntimeErrorStage::Transport,
        retry_class: RetryClass::UnknownDelivery,
        message: "Codex exited before the turn completed.".to_string(),
        diagnostic_ref: diagnostic_ref
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("codex-process-{process_epoch}-process_exit")),
    }
}

fn codex_terminal_channel_error(process_epoch: u64) -> RuntimeTerminalError {
    RuntimeTerminalError {
        code: "event_gap".to_string(),
        stage: RuntimeErrorStage::Transport,
        retry_class: RetryClass::UnknownDelivery,
        message: "Codex terminal event continuity was lost.".to_string(),
        diagnostic_ref: format!("codex-process-{process_epoch}-event_gap"),
    }
}

#[derive(Clone, Default)]
pub struct CodexRuntimeModule {
    inner: Arc<ModuleState>,
}

#[derive(Default)]
struct ModuleState {
    workers: Mutex<HashMap<String, Arc<CodexWorker>>>,
    next_process_epoch: AtomicU64,
    account_rate_limits_by_profile: Arc<Mutex<HashMap<String, Option<RuntimeAccountRateLimits>>>>,
}

impl std::fmt::Debug for CodexRuntimeModule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let workers = self
            .inner
            .workers
            .lock()
            .expect("Codex worker registry poisoned")
            .len();
        formatter
            .debug_struct("CodexRuntimeModule")
            .field("workers", &workers)
            .finish()
    }
}

impl CodexRuntimeModule {
    pub fn new() -> Self {
        Self::default()
    }

    async fn worker_for(
        &self,
        key: String,
        profile: &RuntimeProfile,
        cwd: &Path,
    ) -> Result<Arc<CodexWorker>, RuntimeError> {
        if let Some(worker) = self
            .inner
            .workers
            .lock()
            .expect("Codex worker registry poisoned")
            .get(&key)
            .filter(|worker| !worker.is_disposed())
            .cloned()
        {
            worker.ensure_profile(profile)?;
            return Ok(worker);
        }

        let epoch = self
            .inner
            .next_process_epoch
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        let candidate = CodexWorker::spawn(
            profile.clone(),
            cwd,
            epoch,
            Arc::clone(&self.inner.account_rate_limits_by_profile),
        )
        .await?;
        let existing = {
            let mut workers = self
                .inner
                .workers
                .lock()
                .expect("Codex worker registry poisoned");
            match workers.get(&key).filter(|worker| !worker.is_disposed()) {
                Some(existing) => Some(Arc::clone(existing)),
                None => {
                    workers.insert(key, Arc::clone(&candidate));
                    None
                }
            }
        };
        if let Some(existing) = existing {
            candidate.shutdown(true).await?;
            existing.ensure_profile(profile)?;
            Ok(existing)
        } else {
            Ok(candidate)
        }
    }

    fn cached_workers(&self, runtime_ref: Option<&str>) -> Vec<Arc<CodexWorker>> {
        self.inner
            .workers
            .lock()
            .expect("Codex worker registry poisoned")
            .values()
            .filter(|worker| runtime_ref.is_none_or(|id| worker.profile.id == id))
            .cloned()
            .collect()
    }

    fn cached_workers_for_snapshot(
        &self,
        profile: &RuntimeProfile,
        scope: &SnapshotScope,
    ) -> Vec<Arc<CodexWorker>> {
        self.cached_workers(Some(&profile.id))
            .into_iter()
            .filter(|worker| {
                worker.profile.revision == profile.revision
                    && worker.profile.fingerprint == profile.fingerprint
                    && match scope {
                        SnapshotScope::Workspace { cwd } | SnapshotScope::Session { cwd, .. } => {
                            same_cwd(&worker.state.cwd, cwd)
                        }
                        SnapshotScope::Profile => true,
                    }
            })
            .collect()
    }

    fn cached_model_catalog(
        &self,
        profile: &RuntimeProfile,
        cwd: &Path,
    ) -> Option<Vec<CodexModelCatalogEntry>> {
        self.cached_workers_for_snapshot(
            profile,
            &SnapshotScope::Workspace {
                cwd: cwd.to_path_buf(),
            },
        )
        .into_iter()
        .filter_map(|worker| {
            worker
                .state
                .model_catalog
                .lock()
                .expect("Codex model catalog poisoned")
                .clone()
                .map(|catalog| (worker.state.process_epoch, catalog))
        })
        .max_by_key(|(process_epoch, _)| *process_epoch)
        .map(|(_, catalog)| catalog)
    }

    async fn execute_request(
        &self,
        request: ExecuteRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
    ) -> Result<ExecuteResult, RuntimeError> {
        validate_request(&request)?;
        match request.intent {
            RuntimeIntent::Turn(turn) => {
                validate_turn_request(&request.profile, &turn)?;
                let model_catalog = self.cached_model_catalog(&request.profile, &turn.cwd);
                if codex_turn_requires_catalog(&turn) && model_catalog.is_none() {
                    return Err(RuntimeError::new(
                        "codex_model_catalog_required",
                        RuntimeErrorStage::Configuration,
                        RetryClass::UserAction,
                        "Codex model and catalog-backed turn options require an observed model/list catalog; refresh the Runtime Profile catalog before retrying",
                    ));
                }
                let key = format!("{}:thread:{}", request.profile.id, turn.thread_id);
                let worker = self.worker_for(key, &request.profile, &turn.cwd).await?;
                worker
                    .execute_turn(turn, observer, control, model_catalog)
                    .await
                    .map(ExecuteResult::Turn)
            }
            RuntimeIntent::Session(session) => {
                let key = session
                    .thread_id
                    .as_deref()
                    .map(|thread_id| format!("{}:thread:{thread_id}", request.profile.id))
                    .unwrap_or_else(|| {
                        format!(
                            "{}:sessions:{}",
                            profile_worker_namespace(&request.profile),
                            session.cwd.to_string_lossy()
                        )
                    });
                let worker = self.worker_for(key, &request.profile, &session.cwd).await?;
                worker
                    .execute_session(session)
                    .await
                    .map(ExecuteResult::Session)
            }
            RuntimeIntent::Compaction(compaction) => {
                if compaction
                    .instructions
                    .as_deref()
                    .is_some_and(|instructions| !instructions.trim().is_empty())
                {
                    return Err(RuntimeError::new(
                        "codex_compaction_instructions_unsupported",
                        RuntimeErrorStage::Control,
                        RetryClass::UserAction,
                        "Codex native compaction does not accept custom instructions; remove them and retry.",
                    ));
                }
                let key = format!("{}:thread:{}", request.profile.id, compaction.thread_id);
                let worker = self
                    .worker_for(key, &request.profile, &compaction.cwd)
                    .await?;
                worker
                    .execute_compaction(compaction, observer)
                    .await
                    .map(ExecuteResult::Compaction)
            }
            RuntimeIntent::Interaction(response) => {
                for worker in self.cached_workers(Some(&request.profile.id)) {
                    if worker.has_interaction(&response.interaction_id) {
                        return worker
                            .respond_interaction(response)
                            .await
                            .map(ExecuteResult::Interaction);
                    }
                }
                Ok(ExecuteResult::Interaction(RuntimeInteractionResult {
                    accepted: false,
                    expired: true,
                    message: Some("Codex interaction is no longer pending".to_string()),
                }))
            }
            RuntimeIntent::Mcp(mcp) => {
                let key = format!(
                    "{}:mcp:{}",
                    profile_worker_namespace(&request.profile),
                    mcp.cwd.to_string_lossy()
                );
                let worker = self.worker_for(key, &request.profile, &mcp.cwd).await?;
                let (method, params) = match mcp.operation.as_str() {
                    "status/list" => (
                        "mcpServerStatus/list",
                        mcp.argument.unwrap_or_else(|| json!({})),
                    ),
                    "reload" => ("config/mcpServer/reload", Value::Null),
                    _ => return Err(unsupported("Codex MCP operation", &mcp.operation)),
                };
                worker
                    .transport()
                    .request(method, params)
                    .await
                    .map(ExecuteResult::Mcp)
            }
            RuntimeIntent::Auth(auth) => {
                let key = format!(
                    "{}:auth:{}",
                    profile_worker_namespace(&request.profile),
                    auth.cwd.to_string_lossy()
                );
                let worker = self.worker_for(key, &request.profile, &auth.cwd).await?;
                execute_auth(&worker, auth).await.map(ExecuteResult::Auth)
            }
            RuntimeIntent::Control(control) => Err(RuntimeError::new(
                "codex_control_mutation_experimental",
                RuntimeErrorStage::Control,
                RetryClass::UserAction,
                format!(
                    "Codex cannot stably mutate `{}` after binding; thread/settings/update is experimental. Start a new thread with the requested control.",
                    control.control_id
                ),
            )),
            RuntimeIntent::Extension(extension) => self
                .execute_extension(&request.profile, extension)
                .await
                .map(ExecuteResult::Extension),
        }
    }

    async fn execute_extension(
        &self,
        profile: &RuntimeProfile,
        extension: RuntimeExtensionRequest,
    ) -> Result<Value, RuntimeError> {
        match (extension.namespace.as_str(), extension.operation.as_str()) {
            ("codex.goal", "read") => {
                let target = parse_extension_argument::<GoalTarget>(extension.argument)?;
                let worker = self.goal_worker(profile, &target).await?;
                worker.goal_read(&target).await
            }
            ("codex.goal", "set") => {
                let argument = parse_extension_argument::<GoalSetArgument>(extension.argument)?;
                if argument.objective.is_none()
                    && argument.status.is_none()
                    && argument.token_budget == GoalTokenBudgetInput::Missing
                {
                    return Err(invalid_extension_schema(
                        "codex.goal/set requires objective, status, or tokenBudget",
                    ));
                }
                validate_goal_set_argument(&argument)?;
                let target = argument.target();
                let worker = self.goal_worker(profile, &target).await?;
                worker.goal_set(argument).await
            }
            ("codex.goal", "clear") => {
                let target = parse_extension_argument::<GoalTarget>(extension.argument)?;
                let worker = self.goal_worker(profile, &target).await?;
                worker.goal_clear(&target).await
            }
            ("codex.account", "rateLimits/read") => {
                let argument =
                    parse_extension_argument::<AccountRateLimitsReadArgument>(extension.argument)?;
                let key = format!(
                    "{}:account:{}",
                    profile_worker_namespace(profile),
                    argument.cwd.to_string_lossy()
                );
                let worker = self.worker_for(key, profile, &argument.cwd).await?;
                worker.account_rate_limits_read().await
            }
            _ => Err(unsupported(
                "Codex extension operation",
                &format!("{}/{}", extension.namespace, extension.operation),
            )),
        }
    }

    async fn goal_worker(
        &self,
        profile: &RuntimeProfile,
        target: &GoalTarget,
    ) -> Result<Arc<CodexWorker>, RuntimeError> {
        let key = format!("{}:thread:{}", profile.id, target.thread_id);
        self.worker_for(key, profile, &target.cwd).await
    }
}

fn profile_worker_namespace(profile: &RuntimeProfile) -> String {
    format!(
        "{}:revision:{}:fingerprint:{}",
        profile.id, profile.revision, profile.fingerprint
    )
}

async fn execute_auth(
    worker: &CodexWorker,
    request: RuntimeAuthRequest,
) -> Result<RuntimeAuthResult, RuntimeError> {
    let (method, params) = match request.operation {
        RuntimeAuthOperation::Status { refresh } => {
            ("account/read", json!({ "refreshToken": refresh }))
        }
        RuntimeAuthOperation::LoginChatgpt => ("account/login/start", json!({ "type": "chatgpt" })),
        RuntimeAuthOperation::LoginDeviceCode => (
            "account/login/start",
            json!({ "type": "chatgptDeviceCode" }),
        ),
        RuntimeAuthOperation::Cancel { login_id } => {
            ("account/login/cancel", json!({ "loginId": login_id }))
        }
        RuntimeAuthOperation::Logout => ("account/logout", Value::Null),
    };
    let result = worker.transport().request(method, params).await?;
    let (status, message) = match method {
        "account/read" => {
            if result
                .get("account")
                .is_some_and(|account| !account.is_null())
            {
                ("authenticated", "Codex account is authenticated.")
            } else if result
                .get("requiresOpenaiAuth")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                ("login_required", "Codex requires managed account login.")
            } else {
                (
                    "not_required",
                    "The selected Codex provider does not require OpenAI login.",
                )
            }
        }
        "account/login/start" => (
            "login_pending",
            "Codex started a managed login flow and continues to own the credentials.",
        ),
        "account/login/cancel" => ("login_cancelled", "Codex login was cancelled."),
        "account/logout" => ("logged_out", "Codex account was logged out."),
        _ => unreachable!("Codex auth method is selected above"),
    };
    Ok(RuntimeAuthResult {
        accepted: true,
        status: status.to_string(),
        message: message.to_string(),
        output: sanitize_auth_output(method, result),
    })
}

fn sanitize_auth_output(method: &str, result: Value) -> Value {
    match method {
        "account/login/start" => {
            let mut safe = serde_json::Map::new();
            for key in ["type", "loginId", "authUrl", "verificationUrl", "userCode"] {
                if let Some(value) = result.get(key) {
                    safe.insert(key.to_string(), value.clone());
                }
            }
            Value::Object(safe)
        }
        "account/login/cancel" => result
            .get("status")
            .cloned()
            .map(|status| json!({ "status": status }))
            .unwrap_or(Value::Null),
        // Account details and logout responses are deliberately not projected:
        // this seam only needs the classified status above.
        "account/read" | "account/logout" => Value::Null,
        _ => Value::Null,
    }
}

impl RuntimeModule for CodexRuntimeModule {
    fn snapshot(&self, query: SnapshotQuery) -> RuntimeFuture<RuntimeSnapshot> {
        let module = self.clone();
        Box::pin(async move {
            if matches!(
                query.mode,
                SnapshotMode::BoundedProbe | SnapshotMode::CatalogRefresh
            ) {
                let cwd = snapshot_probe_cwd(&query)?;
                let key = format!(
                    "{}:probe:{}",
                    profile_worker_namespace(&query.profile),
                    cwd.to_string_lossy()
                );
                let worker = module.worker_for(key, &query.profile, &cwd).await?;
                worker.probe_auth_readiness().await;
                if query.mode == SnapshotMode::CatalogRefresh {
                    worker.hydrate_model_catalog().await?;
                }
            }
            let workers = module.cached_workers_for_snapshot(&query.profile, &query.scope);
            Ok(snapshot_from_cache(query.profile, query.scope, &workers))
        })
    }

    fn execute(
        &self,
        request: ExecuteRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
    ) -> RuntimeFuture<ExecuteResult> {
        let module = self.clone();
        Box::pin(async move { module.execute_request(request, observer, control).await })
    }

    fn shutdown(&self, mode: ShutdownMode) -> RuntimeFuture<()> {
        let module = self.clone();
        Box::pin(async move {
            let (runtime_ref, force) = match mode {
                ShutdownMode::Graceful => (None, false),
                ShutdownMode::Force => (None, true),
                ShutdownMode::Runtime {
                    kind,
                    runtime_ref,
                    force,
                } => {
                    if kind != RuntimeKind::Codex {
                        return Ok(());
                    }
                    (runtime_ref, force)
                }
            };
            let workers = module.cached_workers(runtime_ref.as_deref());
            let shutdowns = workers
                .into_iter()
                .map(|worker| {
                    Box::pin(async move { worker.shutdown(force).await }) as DetachedWorkerShutdown
                })
                .collect();
            run_worker_shutdowns_concurrently(shutdowns).await?;
            module
                .inner
                .workers
                .lock()
                .expect("Codex worker registry poisoned")
                .retain(|_, worker| !worker.is_disposed());
            Ok(())
        })
    }
}

type DetachedWorkerShutdown = RuntimeFuture<()>;

async fn run_worker_shutdowns_concurrently(
    shutdowns: Vec<DetachedWorkerShutdown>,
) -> Result<(), RuntimeError> {
    let tasks = shutdowns.into_iter().map(tokio::spawn).collect::<Vec<_>>();
    let mut first_error = None;
    for task in tasks {
        let result = match task.await {
            Ok(result) => result,
            Err(error) => Err(internal_error(&format!(
                "Codex worker shutdown task failed: {error}"
            ))),
        };
        if let Err(error) = result
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct PendingInteraction {
    native_request_id: RequestId,
    method: String,
    process_epoch: u64,
    question_ids: Vec<String>,
    question_multiple: Vec<bool>,
    requested_permissions: Option<Value>,
}

struct PendingCompaction {
    gateway_thread_id: String,
    native_session_id: String,
    public_item_id: String,
    native_item_id: Option<String>,
    native_turn_id: Option<String>,
    observer: RuntimeObserver,
    responder: Option<oneshot::Sender<Result<String, RuntimeError>>>,
}

struct WorkerState {
    profile: RuntimeProfile,
    cwd: std::path::PathBuf,
    process_epoch: u64,
    transport: OnceLock<Arc<CodexTransport>>,
    active_turn: Mutex<Option<Arc<ActiveTurn>>>,
    pending_compaction: Mutex<Option<PendingCompaction>>,
    next_compaction_id: AtomicU64,
    interactions: Mutex<HashMap<String, PendingInteraction>>,
    session_id: Mutex<Option<String>>,
    session_model: Mutex<Option<String>>,
    session_provider: Mutex<Option<String>>,
    runtime_version: Mutex<Option<String>>,
    latest_plan: Mutex<Option<RuntimePlanUpdate>>,
    latest_diff: Mutex<Option<RuntimeDiffUpdate>>,
    latest_usage: Mutex<Option<RuntimeUsageUpdate>>,
    latest_goal: Mutex<Option<RuntimeGoal>>,
    auth_readiness: Mutex<Option<(ReadinessStatus, String)>>,
    stable_turn_hydrated: AtomicBool,
    model_catalog: Mutex<Option<Vec<CodexModelCatalogEntry>>>,
    account_rate_limits_by_profile: Arc<Mutex<HashMap<String, Option<RuntimeAccountRateLimits>>>>,
}

impl WorkerState {
    fn handle_event(&self, event: TransportEvent) {
        match event {
            TransportEvent::Notification { method, params } => {
                if method == "serverRequest/resolved" {
                    self.resolve_server_request(&params);
                    return;
                }
                if self.handle_compaction_notification(&method, &params) {
                    return;
                }
                if self.handle_goal_notification(&method, &params)
                    || self.handle_rate_limit_notification(&method, &params)
                {
                    return;
                }
                if let Some(active) = self
                    .active_turn
                    .lock()
                    .expect("Codex active turn poisoned")
                    .clone()
                {
                    active.handle_notification(&method, &params);
                }
            }
            TransportEvent::Request { id, method, params } => {
                self.handle_server_request(id, method, params);
            }
            TransportEvent::Exited(error) => {
                self.interactions
                    .lock()
                    .expect("Codex interactions poisoned")
                    .clear();
                self.fail_pending_compaction(error.clone());
                if let Some(active) = self
                    .active_turn
                    .lock()
                    .expect("Codex active turn poisoned")
                    .clone()
                {
                    let terminal_error = codex_process_terminal_error(
                        error.diagnostic_ref.as_deref(),
                        active.process_epoch,
                    );
                    active.fail_process(
                        terminal_error,
                        Some(json!({
                            "code": error.code,
                            "stage": error.stage,
                            "retryClass": error.retry_class,
                            "message": error.message,
                            "diagnosticRef": error.diagnostic_ref,
                        })),
                    );
                }
            }
        }
    }

    fn handle_compaction_notification(&self, method: &str, params: &Value) -> bool {
        if matches!(method, "item/started" | "item/completed")
            && params
                .get("item")
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                == Some("contextCompaction")
        {
            let Some(native_session_id) = params.get("threadId").and_then(Value::as_str) else {
                return false;
            };
            let Some(native_turn_id) = params.get("turnId").and_then(Value::as_str) else {
                return false;
            };
            let Some(native_item_id) = params
                .get("item")
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
            else {
                return false;
            };
            let mut slot = self
                .pending_compaction
                .lock()
                .expect("Codex compaction waiter poisoned");
            let Some(mut pending) = slot.take() else {
                return false;
            };
            if pending.native_session_id != native_session_id
                || pending
                    .native_item_id
                    .as_deref()
                    .is_some_and(|item_id| item_id != native_item_id)
                || pending
                    .native_turn_id
                    .as_deref()
                    .is_some_and(|turn_id| turn_id != native_turn_id)
            {
                *slot = Some(pending);
                return false;
            }
            pending.native_item_id = Some(native_item_id.to_string());
            pending.native_turn_id = Some(native_turn_id.to_string());
            let status = if method == "item/completed" {
                RuntimeCompactionStatus::Completed
            } else {
                RuntimeCompactionStatus::Started
            };
            let change = RuntimeCompactionChange {
                runtime_ref: self.profile.id.clone(),
                thread_id: pending.gateway_thread_id.clone(),
                turn_id: None,
                item_id: Some(pending.public_item_id.clone()),
                status,
            };
            let observer = pending.observer.clone();
            if method == "item/completed" {
                let responder = pending.responder.take();
                let public_item_id = pending.public_item_id;
                drop(slot);
                observer.emit(RuntimeObservation::CompactionChanged(change));
                if let Some(responder) = responder {
                    let _ = responder.send(Ok(public_item_id));
                }
            } else {
                *slot = Some(pending);
                drop(slot);
                observer.emit(RuntimeObservation::CompactionChanged(change));
            }
            return true;
        }

        if method == "turn/completed" {
            let Some(native_session_id) = params.get("threadId").and_then(Value::as_str) else {
                return false;
            };
            let turn = params.get("turn").unwrap_or(params);
            let Some(native_turn_id) = turn.get("id").and_then(Value::as_str) else {
                return false;
            };
            let slot = self
                .pending_compaction
                .lock()
                .expect("Codex compaction waiter poisoned");
            let matches_pending = slot.as_ref().is_some_and(|pending| {
                pending.native_session_id == native_session_id
                    && pending.native_turn_id.as_deref() == Some(native_turn_id)
            });
            if !matches_pending {
                return false;
            }
            let status = turn
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("failed");
            let message = if status == "completed" {
                "Codex compact turn completed without a matching contextCompaction item completion"
                    .to_string()
            } else {
                turn.get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex native compaction failed")
                    .to_string()
            };
            let error = RuntimeError::new(
                "codex_compaction_failed",
                RuntimeErrorStage::Control,
                RetryClass::UserAction,
                message,
            );
            drop(slot);
            self.fail_pending_compaction(error);
            return true;
        }
        false
    }

    fn fail_pending_compaction(&self, error: RuntimeError) {
        let Some(mut pending) = self
            .pending_compaction
            .lock()
            .expect("Codex compaction waiter poisoned")
            .take()
        else {
            return;
        };
        pending.observer.emit(RuntimeObservation::CompactionChanged(
            RuntimeCompactionChange {
                runtime_ref: self.profile.id.clone(),
                thread_id: pending.gateway_thread_id,
                turn_id: None,
                item_id: Some(pending.public_item_id),
                status: RuntimeCompactionStatus::Failed,
            },
        ));
        if let Some(responder) = pending.responder.take() {
            let _ = responder.send(Err(error));
        }
    }

    fn handle_goal_notification(&self, method: &str, params: &Value) -> bool {
        let goal_change = match method {
            "thread/goal/updated" => {
                let update = match serde_json::from_value::<NativeGoalUpdated>(params.clone()) {
                    Ok(update) => update,
                    Err(error) => {
                        self.emit_auxiliary_warning(
                            "codex_invalid_goal_update",
                            format!("Codex goal update did not match the stable schema: {error}"),
                        );
                        return true;
                    }
                };
                let goal = match validated_runtime_goal(&update.thread_id, update.goal) {
                    Ok(goal) => goal,
                    Err(error) => {
                        self.emit_auxiliary_warning("codex_invalid_goal_update", error.message);
                        return true;
                    }
                };
                *self.latest_goal.lock().expect("Codex goal cache poisoned") = Some(goal.clone());
                Some((update.thread_id, update.turn_id, Some(goal)))
            }
            "thread/goal/cleared" => {
                let update = match serde_json::from_value::<NativeGoalCleared>(params.clone()) {
                    Ok(update) => update,
                    Err(error) => {
                        self.emit_auxiliary_warning(
                            "codex_invalid_goal_update",
                            format!("Codex goal clear did not match the stable schema: {error}"),
                        );
                        return true;
                    }
                };
                *self.latest_goal.lock().expect("Codex goal cache poisoned") = None;
                Some((update.thread_id, None, None))
            }
            _ => None,
        };
        let Some((native_thread_id, native_turn_id, goal)) = goal_change else {
            return false;
        };
        if let Some(active) = self.active_for_native_thread(&native_thread_id) {
            let turn_id = native_turn_id
                .as_deref()
                .filter(|native| active.native_turn_id().as_deref() == Some(*native))
                .map(|_| active.gateway_turn_id.clone());
            active.emit_observation(RuntimeObservation::GoalChanged(RuntimeGoalChange {
                runtime_ref: self.profile.id.clone(),
                thread_id: active.gateway_thread_id.clone(),
                turn_id,
                goal,
            }));
        }
        true
    }

    fn handle_rate_limit_notification(&self, method: &str, params: &Value) -> bool {
        if method != "account/rateLimits/updated" {
            return false;
        }
        let update = match serde_json::from_value::<NativeRateLimitsUpdated>(params.clone()) {
            Ok(update) => update,
            Err(error) => {
                self.emit_auxiliary_warning(
                    "codex_invalid_rate_limit_update",
                    format!("Codex rate-limit update did not match the stable schema: {error}"),
                );
                return true;
            }
        };
        let incoming = match runtime_rate_limit_snapshot(update.rate_limits) {
            Ok(incoming) => incoming,
            Err(error) => {
                self.emit_auxiliary_warning("codex_invalid_rate_limit_update", error.message);
                return true;
            }
        };
        let merged = {
            let mut cache = self
                .account_rate_limits_by_profile
                .lock()
                .expect("Codex rate-limit cache poisoned");
            let profile_cache = cache
                .entry(profile_worker_namespace(&self.profile))
                .or_default();
            merge_sparse_rate_limit_update(profile_cache, incoming);
            profile_cache
                .clone()
                .expect("rate-limit merge initializes the cache")
        };
        if let Some(active) = self
            .active_turn
            .lock()
            .expect("Codex active turn poisoned")
            .clone()
        {
            active.emit_observation(RuntimeObservation::AccountRateLimitsUpdated(
                RuntimeAccountRateLimitsUpdate {
                    runtime_ref: self.profile.id.clone(),
                    rate_limits: merged,
                },
            ));
        }
        true
    }

    fn active_for_native_thread(&self, native_thread_id: &str) -> Option<Arc<ActiveTurn>> {
        self.active_turn
            .lock()
            .expect("Codex active turn poisoned")
            .clone()
            .filter(|active| active.native_session_id == native_thread_id)
    }

    fn emit_auxiliary_warning(&self, code: &str, message: String) {
        if let Some(active) = self
            .active_turn
            .lock()
            .expect("Codex active turn poisoned")
            .clone()
        {
            active.emit_warning(code, message);
        }
    }

    fn cache_turn_auxiliary(&self, active: &ActiveTurn) {
        if let Some(plan) = active.latest_plan() {
            *self.latest_plan.lock().expect("Codex plan cache poisoned") = Some(plan);
        }
        if let Some(diff) = active.latest_diff() {
            *self.latest_diff.lock().expect("Codex diff cache poisoned") = Some(diff);
        }
        if let Some(usage) = active.latest_usage() {
            *self
                .latest_usage
                .lock()
                .expect("Codex usage cache poisoned") = Some(usage);
        }
    }

    fn resolve_server_request(&self, params: &Value) {
        let Some(request_id) = params.get("requestId") else {
            return;
        };
        let Ok(request_id) = serde_json::from_value::<RequestId>(request_id.clone()) else {
            return;
        };
        self.interactions
            .lock()
            .expect("Codex interactions poisoned")
            .retain(|_, interaction| interaction.native_request_id.key() != request_id.key());
    }

    fn handle_server_request(&self, id: RequestId, method: String, params: Value) {
        let Some(active) = self
            .active_turn
            .lock()
            .expect("Codex active turn poisoned")
            .clone()
        else {
            return;
        };
        let normalized = match normalized_interaction(&method, &params) {
            Some(value) => value,
            None => {
                active.handle_notification(
                    "warning",
                    &json!({"message": format!("Unsupported Codex server request: {method}")}),
                );
                return;
            }
        };
        let interaction_id = format!("codex:{}:{}", self.process_epoch, id.key());
        let request_thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or(&active.native_session_id)
            .to_string();
        let is_child = request_thread_id != active.native_session_id;
        let question_multiple = normalized
            .questions
            .iter()
            .map(|question| question.multiple)
            .collect();
        self.interactions
            .lock()
            .expect("Codex interactions poisoned")
            .insert(
                interaction_id.clone(),
                PendingInteraction {
                    native_request_id: id,
                    method,
                    process_epoch: self.process_epoch,
                    question_ids: normalized.question_ids,
                    question_multiple,
                    requested_permissions: params.get("permissions").cloned(),
                },
            );
        active.emit_interaction(RuntimeInteraction {
            id: interaction_id,
            policy: normalized.policy,
            kind: normalized.kind,
            runtime_ref: self.profile.id.clone(),
            thread_id: active.gateway_thread_id.clone(),
            native_session_id: request_thread_id.clone(),
            parent_native_session_id: is_child.then(|| active.native_session_id.clone()),
            child_native_session_id: is_child.then_some(request_thread_id),
            process_epoch: self.process_epoch,
            instance_epoch: None,
            prompt: normalized.prompt,
            questions: normalized.questions,
            choices: normalized.choices,
            authorization_lifetime: normalized.authorization_lifetime,
            expires_at_ms: normalized.expires_at_ms,
            metadata: normalized_interaction_metadata(&params),
        });
    }
}

struct CodexWorker {
    profile: RuntimeProfile,
    state: Arc<WorkerState>,
    session_gate: AsyncMutex<()>,
    activity_gate: AsyncMutex<()>,
}

impl std::fmt::Debug for CodexWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexWorker")
            .field("runtime_ref", &self.profile.id)
            .field("process_epoch", &self.state.process_epoch)
            .field("disposed", &self.is_disposed())
            .finish()
    }
}

impl CodexWorker {
    async fn goal_read(&self, target: &GoalTarget) -> Result<Value, RuntimeError> {
        let response = self
            .transport()
            .request(
                "thread/goal/get",
                json!({"threadId": target.native_session_id}),
            )
            .await?;
        let response: NativeGoalGetResponse =
            decode_extension_response("thread/goal/get", response)?;
        let goal = response
            .goal
            .map(|goal| validated_runtime_goal(&target.native_session_id, goal))
            .transpose()?;
        *self
            .state
            .latest_goal
            .lock()
            .expect("Codex goal cache poisoned") = goal.clone();
        Ok(json!({"goal": goal}))
    }

    async fn goal_set(&self, argument: GoalSetArgument) -> Result<Value, RuntimeError> {
        let mut params = serde_json::Map::from_iter([(
            "threadId".to_string(),
            Value::String(argument.native_session_id.clone()),
        )]);
        if let Some(objective) = argument.objective {
            params.insert("objective".to_string(), Value::String(objective));
        }
        if let Some(status) = argument.status {
            params.insert(
                "status".to_string(),
                Value::String(native_goal_status(status).to_string()),
            );
        }
        match argument.token_budget {
            GoalTokenBudgetInput::Missing => {}
            GoalTokenBudgetInput::Clear => {
                params.insert("tokenBudget".to_string(), Value::Null);
            }
            GoalTokenBudgetInput::Set(token_budget) => {
                params.insert("tokenBudget".to_string(), Value::from(token_budget));
            }
        }
        let response = self
            .transport()
            .request("thread/goal/set", Value::Object(params))
            .await?;
        let response: NativeGoalSetResponse =
            decode_extension_response("thread/goal/set", response)?;
        let goal = validated_runtime_goal(&argument.native_session_id, response.goal)?;
        *self
            .state
            .latest_goal
            .lock()
            .expect("Codex goal cache poisoned") = Some(goal.clone());
        Ok(json!({"goal": goal}))
    }

    async fn goal_clear(&self, target: &GoalTarget) -> Result<Value, RuntimeError> {
        let response = self
            .transport()
            .request(
                "thread/goal/clear",
                json!({"threadId": target.native_session_id}),
            )
            .await?;
        let response: NativeGoalClearResponse =
            decode_extension_response("thread/goal/clear", response)?;
        if response.cleared {
            *self
                .state
                .latest_goal
                .lock()
                .expect("Codex goal cache poisoned") = None;
        }
        Ok(json!({"cleared": response.cleared}))
    }

    async fn account_rate_limits_read(&self) -> Result<Value, RuntimeError> {
        let response = self
            .transport()
            .request("account/rateLimits/read", Value::Null)
            .await?;
        let response: NativeRateLimitsRead =
            decode_extension_response("account/rateLimits/read", response)?;
        let rate_limits = runtime_rate_limits(response)?;
        *self
            .state
            .account_rate_limits_by_profile
            .lock()
            .expect("Codex rate-limit cache poisoned")
            .entry(profile_worker_namespace(&self.profile))
            .or_default() = Some(rate_limits.clone());
        serde_json::to_value(rate_limits).map_err(|error| {
            internal_error(&format!(
                "Codex rate-limit result could not be encoded: {error}"
            ))
        })
    }

    async fn execute_compaction(
        &self,
        request: RuntimeCompactionRequest,
        observer: RuntimeObserver,
    ) -> Result<RuntimeCompactionResult, RuntimeError> {
        let _activity = self.activity_gate.lock().await;
        self.ensure_compaction_session(&request).await?;
        let public_item_id = format!(
            "cx_{}_{}",
            self.state.process_epoch,
            self.state.next_compaction_id.fetch_add(1, Ordering::SeqCst)
        );
        let (response_tx, response_rx) = oneshot::channel();
        {
            let mut pending = self
                .state
                .pending_compaction
                .lock()
                .expect("Codex compaction waiter poisoned");
            if pending.is_some() {
                return Err(RuntimeError::new(
                    "codex_compaction_busy",
                    RuntimeErrorStage::Control,
                    RetryClass::UserAction,
                    "Codex already has a native compaction in progress for this thread",
                ));
            }
            *pending = Some(PendingCompaction {
                gateway_thread_id: request.thread_id.clone(),
                native_session_id: request.native_session_id.clone(),
                public_item_id: public_item_id.clone(),
                native_item_id: None,
                native_turn_id: None,
                observer,
                responder: Some(response_tx),
            });
        }
        if let Err(error) = self
            .transport()
            .request(
                "thread/compact/start",
                json!({"threadId": request.native_session_id}),
            )
            .await
        {
            self.state.fail_pending_compaction(error.clone());
            return Err(error);
        }
        let completed_item_id = response_rx.await.map_err(|_| {
            RuntimeError::new(
                "codex_compaction_waiter_closed",
                RuntimeErrorStage::Transport,
                RetryClass::Reconnect,
                "Codex compaction completion waiter closed before a terminal item",
            )
        })??;
        Ok(RuntimeCompactionResult {
            thread_id: request.thread_id,
            native_session_id: request.native_session_id,
            item_id: completed_item_id,
            compacted: true,
            process_epoch: self.state.process_epoch,
        })
    }

    async fn ensure_compaction_session(
        &self,
        request: &RuntimeCompactionRequest,
    ) -> Result<(), RuntimeError> {
        let _session = self.session_gate.lock().await;
        let current = self
            .state
            .session_id
            .lock()
            .expect("Codex session id poisoned")
            .clone();
        if current.as_deref() == Some(request.native_session_id.as_str()) {
            return Ok(());
        }
        if current.is_some() {
            return Err(RuntimeError::new(
                "stale_binding_epoch",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                "The Codex native thread changed before compaction",
            ));
        }
        let response = self
            .transport()
            .request(
                "thread/resume",
                json!({
                    "threadId": request.native_session_id,
                    "cwd": request.cwd.to_string_lossy(),
                }),
            )
            .await?;
        let thread = response
            .get("thread")
            .ok_or_else(|| protocol_error("thread/resume", "response did not include thread"))?;
        let observed = thread
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("thread/resume", "thread did not include id"))?;
        if observed != request.native_session_id {
            return Err(RuntimeError::new(
                "runtime_native_binding_mismatch",
                RuntimeErrorStage::Binding,
                RetryClass::Never,
                "Codex resumed a different native thread for compaction",
            ));
        }
        *self
            .state
            .session_id
            .lock()
            .expect("Codex session id poisoned") = Some(observed.to_string());
        Ok(())
    }

    async fn spawn(
        profile: RuntimeProfile,
        cwd: &Path,
        process_epoch: u64,
        account_rate_limits_by_profile: Arc<
            Mutex<HashMap<String, Option<RuntimeAccountRateLimits>>>,
        >,
    ) -> Result<Arc<Self>, RuntimeError> {
        let state = Arc::new(WorkerState {
            profile: profile.clone(),
            cwd: std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
            process_epoch,
            transport: OnceLock::new(),
            active_turn: Mutex::new(None),
            pending_compaction: Mutex::new(None),
            next_compaction_id: AtomicU64::new(1),
            interactions: Mutex::new(HashMap::new()),
            session_id: Mutex::new(None),
            session_model: Mutex::new(None),
            session_provider: Mutex::new(None),
            runtime_version: Mutex::new(None),
            latest_plan: Mutex::new(None),
            latest_diff: Mutex::new(None),
            latest_usage: Mutex::new(None),
            latest_goal: Mutex::new(None),
            auth_readiness: Mutex::new(None),
            stable_turn_hydrated: AtomicBool::new(false),
            model_catalog: Mutex::new(None),
            account_rate_limits_by_profile,
        });
        let weak: Weak<WorkerState> = Arc::downgrade(&state);
        let event_sink: TransportEventSink = Arc::new(move |event| {
            if let Some(state) = weak.upgrade() {
                state.handle_event(event);
            }
        });
        let transport = CodexTransport::spawn(&profile, cwd, process_epoch, event_sink).await?;
        state
            .transport
            .set(transport)
            .map_err(|_| internal_error("Codex transport was initialized twice"))?;
        let worker = Arc::new(Self {
            profile,
            state,
            session_gate: AsyncMutex::new(()),
            activity_gate: AsyncMutex::new(()),
        });
        if let Err(error) = worker.initialize().await {
            let _ = worker.shutdown(true).await;
            return Err(error);
        }
        Ok(worker)
    }

    fn transport(&self) -> &Arc<CodexTransport> {
        self.state
            .transport
            .get()
            .expect("Codex transport not initialized")
    }

    fn is_disposed(&self) -> bool {
        self.transport().is_disposed()
    }

    fn ensure_profile(&self, profile: &RuntimeProfile) -> Result<(), RuntimeError> {
        if self.profile.fingerprint == profile.fingerprint
            && self.profile.revision == profile.revision
        {
            Ok(())
        } else {
            Err(RuntimeError::new(
                "stale_profile_revision",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                "The bound Codex thread uses a different Runtime Profile revision",
            ))
        }
    }

    async fn initialize(&self) -> Result<(), RuntimeError> {
        let response = self
            .transport()
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "psychevo",
                        "title": "Psychevo Gateway",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "experimentalApi": false,
                    }
                }),
            )
            .await
            .map_err(|mut error| {
                error.stage = RuntimeErrorStage::Handshake;
                error
            })?;
        *self
            .state
            .runtime_version
            .lock()
            .expect("Codex runtime version poisoned") = string(&response, "userAgent");
        self.transport().notify("initialized", Value::Null).await
    }

    async fn probe_auth_readiness(&self) {
        let readiness = match self
            .transport()
            .request("account/read", json!({ "refreshToken": false }))
            .await
        {
            Ok(response)
                if response
                    .get("account")
                    .is_some_and(|account| !account.is_null()) =>
            {
                (
                    ReadinessStatus::Ready,
                    "Codex account authentication is available".to_string(),
                )
            }
            Ok(response)
                if response
                    .get("requiresOpenaiAuth")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                (
                    ReadinessStatus::NeedsAuth,
                    "Codex requires managed account login".to_string(),
                )
            }
            Ok(_) => (
                ReadinessStatus::Ready,
                "Codex runtime reports that managed account login is not required".to_string(),
            ),
            Err(_) => (
                ReadinessStatus::Error,
                "Codex authentication status could not be read".to_string(),
            ),
        };
        *self
            .state
            .auth_readiness
            .lock()
            .expect("Codex auth readiness poisoned") = Some(readiness);
    }

    async fn hydrate_model_catalog(&self) -> Result<(), RuntimeError> {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_models = HashSet::new();
        let mut default_model = None;
        let mut catalog = Vec::new();
        for _ in 0..MODEL_CATALOG_PAGE_LIMIT {
            let response = self
                .transport()
                .request(
                    "model/list",
                    json!({
                        "cursor": cursor,
                        "limit": 100,
                        "includeHidden": false,
                    }),
                )
                .await?;
            let page: NativeModelListResponse = decode_extension_response("model/list", response)?;
            for model in page.data.into_iter().filter(|model| !model.hidden) {
                if model.model.trim().is_empty() || model.display_name.trim().is_empty() {
                    return Err(codex_auxiliary_protocol_mismatch(
                        "Codex model/list returned an empty model identity or label",
                    ));
                }
                if !seen_models.insert(model.model.clone()) {
                    return Err(codex_auxiliary_protocol_mismatch(
                        "Codex model/list returned a duplicate visible model identity",
                    ));
                }
                if model.is_default && default_model.replace(model.model.clone()).is_some() {
                    return Err(codex_auxiliary_protocol_mismatch(
                        "Codex model/list returned more than one visible default model",
                    ));
                }
                catalog.push(validated_codex_model_catalog_entry(model)?);
            }
            let Some(next_cursor) = page.next_cursor else {
                *self
                    .state
                    .model_catalog
                    .lock()
                    .expect("Codex model catalog poisoned") = Some(catalog);
                return Ok(());
            };
            if next_cursor.trim().is_empty() || !seen_cursors.insert(next_cursor.clone()) {
                return Err(codex_auxiliary_protocol_mismatch(
                    "Codex model/list returned an invalid pagination cursor",
                ));
            }
            cursor = Some(next_cursor);
        }
        Err(codex_auxiliary_protocol_mismatch(
            "Codex model/list exceeded the bounded catalog page limit",
        ))
    }

    async fn ensure_session(&self, turn: &RuntimeTurnRequest) -> Result<String, RuntimeError> {
        let _guard = self.session_gate.lock().await;
        if let Some(session_id) = self
            .state
            .session_id
            .lock()
            .expect("Codex session id poisoned")
            .clone()
        {
            if turn
                .native_session_id
                .as_deref()
                .is_some_and(|requested| requested != session_id)
            {
                return Err(RuntimeError::new(
                    "stale_binding_epoch",
                    RuntimeErrorStage::Binding,
                    RetryClass::UserAction,
                    "The Codex native thread changed for this public thread",
                ));
            }
            return Ok(session_id);
        }
        let (method, params) = if let Some(native_session_id) = turn.native_session_id.as_deref() {
            (
                "thread/resume",
                thread_open_params(&self.profile, turn, Some(native_session_id)),
            )
        } else {
            (
                "thread/start",
                thread_open_params(&self.profile, turn, None),
            )
        };
        let response = self
            .transport()
            .request(method, params)
            .await
            .map_err(|mut error| {
                error.stage = RuntimeErrorStage::Hydration;
                error
            })?;
        let thread = response
            .get("thread")
            .ok_or_else(|| protocol_error(method, "response did not include thread"))?;
        if thread
            .get("parentThreadId")
            .is_some_and(|parent| !parent.is_null())
        {
            return Err(RuntimeError::new(
                "codex_read_only_child",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                "Codex native child sessions are read-only",
            ));
        }
        let session_id = thread
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(method, "thread did not include id"))?
            .to_string();
        *self
            .state
            .session_id
            .lock()
            .expect("Codex session id poisoned") = Some(session_id.clone());
        *self
            .state
            .session_model
            .lock()
            .expect("Codex session model poisoned") = string(&response, "model");
        *self
            .state
            .session_provider
            .lock()
            .expect("Codex session provider poisoned") =
            string(&response, "modelProvider").or_else(|| string(thread, "modelProvider"));
        Ok(session_id)
    }

    async fn execute_turn(
        &self,
        turn: RuntimeTurnRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
        model_catalog: Option<Vec<CodexModelCatalogEntry>>,
    ) -> Result<RuntimeTurnResult, RuntimeError> {
        let _activity = self.activity_gate.lock().await;
        let native_session_id = self.ensure_session(&turn).await?;
        observer
            .bind_native_session(RuntimeSessionBinding {
                runtime_ref: self.profile.id.clone(),
                thread_id: turn.thread_id.clone(),
                native_session_id: native_session_id.clone(),
                cwd: turn.cwd.clone(),
                binding_epoch: turn.binding_epoch,
                process_epoch: self.state.process_epoch,
                instance_epoch: None,
            })
            .await?;
        let session_model = self
            .state
            .session_model
            .lock()
            .expect("Codex session model poisoned")
            .clone();
        validate_catalog_backed_turn_options(
            &self.profile,
            &turn,
            session_model.as_deref(),
            model_catalog.as_deref(),
        )?;
        let (active, mut terminal_rx) = ActiveTurn::new(
            turn.turn_id.clone(),
            turn.thread_id.clone(),
            native_session_id.clone(),
            self.profile.id.clone(),
            self.state.process_epoch,
            observer,
        );
        let active = Arc::new(active);
        {
            let mut current = self
                .state
                .active_turn
                .lock()
                .expect("Codex active turn poisoned");
            if current.is_some() {
                return Err(RuntimeError::new(
                    "codex_busy",
                    RuntimeErrorStage::Prompt,
                    RetryClass::UserAction,
                    "Codex thread already has an active turn",
                ));
            }
            *current = Some(Arc::clone(&active));
        }
        let start_params = turn_start_params(
            &self.profile,
            &turn,
            &native_session_id,
            session_model.as_deref(),
        );
        let start_result = self.transport().request("turn/start", start_params).await;
        let response = match start_result {
            Ok(response) => response,
            Err(error) => {
                self.clear_interactions();
                self.clear_active(&active);
                if error.code == "codex_process_exit" {
                    let terminal = terminal_rx.await.unwrap_or_else(|_| NativeTerminal {
                        outcome: RuntimeTurnOutcome::Failed,
                        terminal_error: Some(codex_terminal_channel_error(
                            self.state.process_epoch,
                        )),
                        metadata: None,
                    });
                    return Ok(self.turn_result(&turn, &native_session_id, &active, terminal));
                }
                return Err(error);
            }
        };
        let Some(native_turn_id) = response
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            self.clear_interactions();
            self.clear_active(&active);
            return Err(protocol_error(
                "turn/start",
                "response did not include turn.id",
            ));
        };
        active.activate(native_turn_id.clone());

        let mut interrupt_sent = false;
        let mut interrupt_deadline = None;
        let terminal = loop {
            tokio::select! {
                terminal = &mut terminal_rx => {
                    break terminal.unwrap_or_else(|_| NativeTerminal {
                        outcome: RuntimeTurnOutcome::Failed,
                        terminal_error: Some(codex_terminal_channel_error(
                            self.state.process_epoch,
                        )),
                        metadata: None,
                    });
                }
                _ = control.cancelled(), if !interrupt_sent => {
                    interrupt_sent = true;
                    let _ = self.transport().request(
                        "turn/interrupt",
                        json!({"threadId": native_session_id, "turnId": native_turn_id}),
                    ).await;
                    interrupt_deadline = Some(tokio::time::Instant::now() + INTERRUPT_TERMINAL_TIMEOUT);
                }
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    for text in control.take_steer() {
                        let _ = self.transport().request(
                            "turn/steer",
                            json!({
                                "threadId": native_session_id,
                                "expectedTurnId": native_turn_id,
                                "clientUserMessageId": format!("{}:steer", turn.turn_id),
                                "input": [{"type": "text", "text": text, "textElements": []}],
                            }),
                        ).await;
                    }
                    if interrupt_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                        active.interrupt_after_timeout();
                    }
                }
            }
        };
        self.state.cache_turn_auxiliary(&active);
        if terminal.outcome == RuntimeTurnOutcome::Completed {
            *self
                .state
                .auth_readiness
                .lock()
                .expect("Codex auth readiness poisoned") = Some((
                ReadinessStatus::Ready,
                "Codex completed an authenticated turn".to_string(),
            ));
            self.state
                .stable_turn_hydrated
                .store(true, Ordering::SeqCst);
            if let Err(error) = self.hydrate_model_catalog().await {
                active.emit_warning(
                    "codex_model_catalog_unavailable",
                    format!("Codex model catalog was not hydrated: {}", error.message),
                );
            }
        }
        self.clear_interactions();
        self.clear_active(&active);
        Ok(self.turn_result(&turn, &native_session_id, &active, terminal))
    }

    fn clear_active(&self, completed: &Arc<ActiveTurn>) {
        let mut active = self
            .state
            .active_turn
            .lock()
            .expect("Codex active turn poisoned");
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, completed))
        {
            *active = None;
        }
    }

    fn clear_interactions(&self) {
        self.state
            .interactions
            .lock()
            .expect("Codex interactions poisoned")
            .clear();
    }

    fn turn_result(
        &self,
        turn: &RuntimeTurnRequest,
        native_session_id: &str,
        active: &ActiveTurn,
        terminal: NativeTerminal,
    ) -> RuntimeTurnResult {
        let model = self
            .state
            .session_model
            .lock()
            .expect("Codex session model poisoned")
            .clone()
            .or_else(|| turn.model.clone())
            .unwrap_or_else(|| "runtime-default".to_string());
        let provider = self
            .state
            .session_provider
            .lock()
            .expect("Codex session provider poisoned")
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let metadata = Some(json!({
            "nativeTurnId": active.native_turn_id(),
            "bindingEpoch": turn.binding_epoch,
            "terminal": terminal.metadata,
            "diagnosticRef": format!("codex-process-{}", self.state.process_epoch),
        }));
        RuntimeTurnResult {
            turn_id: turn.turn_id.clone(),
            thread_id: turn.thread_id.clone(),
            native_session_id: native_session_id.to_string(),
            outcome: terminal.outcome,
            final_answer: active.final_answer(),
            provider,
            model,
            history_fidelity: HistoryFidelity::Partial,
            process_epoch: self.state.process_epoch,
            instance_epoch: None,
            terminal_error: terminal.terminal_error,
            metadata,
        }
    }

    async fn execute_session(
        &self,
        request: RuntimeSessionRequest,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        match request.operation {
            RuntimeSessionOperation::List => self.list_sessions(request).await,
            RuntimeSessionOperation::Read => self.read_session(request, false).await,
            RuntimeSessionOperation::Resume => self.resume_session(request).await,
            RuntimeSessionOperation::Fork => self.fork_session(request).await,
            RuntimeSessionOperation::Archive => {
                self.simple_mutation(request, "thread/archive", true).await
            }
            RuntimeSessionOperation::Unarchive => {
                self.simple_mutation(request, "thread/unarchive", true)
                    .await
            }
            RuntimeSessionOperation::Delete => {
                self.simple_mutation(request, "thread/delete", false).await
            }
            RuntimeSessionOperation::Rename => self.rename_session(request).await,
            RuntimeSessionOperation::Revert | RuntimeSessionOperation::Unrevert => {
                Err(unsupported("Codex session operation", "revert/unrevert"))
            }
        }
    }

    async fn list_sessions(
        &self,
        request: RuntimeSessionRequest,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        let continuing = request.cursor.is_some();
        let (active_cursor, archived_cursor) =
            decode_session_list_cursor(request.cursor.as_deref())?;
        let mut sessions = Vec::new();
        let mut next_active = None;
        let mut next_archived = None;
        if !continuing || active_cursor.is_some() {
            let (page, next_cursor) = self
                .list_sessions_page(&request.cwd, false, active_cursor)
                .await?;
            sessions.extend(page);
            next_active = next_cursor;
        }
        if !continuing || archived_cursor.is_some() {
            let (page, next_cursor) = self
                .list_sessions_page(&request.cwd, true, archived_cursor)
                .await?;
            sessions.extend(page);
            next_archived = next_cursor;
        }
        sessions.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.native_session_id.cmp(&right.native_session_id))
        });
        let mut seen = HashSet::new();
        sessions.retain(|session| seen.insert(session.native_session_id.clone()));
        Ok(RuntimeSessionResult {
            changed: false,
            sessions,
            cursor: encode_session_list_cursor(next_active, next_archived),
            message: None,
        })
    }

    async fn list_sessions_page(
        &self,
        cwd: &Path,
        archived: bool,
        cursor: Option<String>,
    ) -> Result<(Vec<crate::RuntimeSession>, Option<String>), RuntimeError> {
        let response = self
            .transport()
            .request(
                "thread/list",
                json!({
                    "cursor": cursor,
                    "limit": 100,
                    "sortKey": "updated_at",
                    "sortDirection": "desc",
                    "sourceKinds": [
                        "cli",
                        "vscode",
                        "exec",
                        "appServer",
                        "subAgent",
                        "subAgentReview",
                        "subAgentCompact",
                        "subAgentThreadSpawn",
                        "subAgentOther",
                        "unknown"
                    ],
                    "archived": archived,
                    "cwd": cwd.to_string_lossy(),
                }),
            )
            .await?;
        let sessions = response
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|thread| session_from_thread(thread, archived))
            .collect();
        Ok((sessions, string(&response, "nextCursor")))
    }

    async fn read_session(
        &self,
        request: RuntimeSessionRequest,
        changed: bool,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        let native_session_id = require_native_session_id(&request)?;
        let response = self
            .transport()
            .request(
                "thread/read",
                json!({"threadId": native_session_id, "includeTurns": true}),
            )
            .await?;
        let session = response
            .get("thread")
            .and_then(|thread| session_from_thread(thread, false))
            .ok_or_else(|| protocol_error("thread/read", "response did not include thread"))?;
        Ok(RuntimeSessionResult {
            changed,
            sessions: vec![session],
            cursor: None,
            message: Some(
                "Codex history is a partial reconstruction of persisted app-server items"
                    .to_string(),
            ),
        })
    }

    async fn resume_session(
        &self,
        request: RuntimeSessionRequest,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        self.ensure_session_cwd(&request).await?;
        let native_session_id = require_native_session_id(&request)?;
        let response = self
            .transport()
            .request(
                "thread/resume",
                json!({
                    "threadId": native_session_id,
                    "cwd": request.cwd.to_string_lossy(),
                }),
            )
            .await?;
        let thread = response
            .get("thread")
            .ok_or_else(|| protocol_error("thread/resume", "response did not include thread"))?;
        let session = session_from_thread(thread, false)
            .ok_or_else(|| protocol_error("thread/resume", "thread did not include id"))?;
        *self
            .state
            .session_id
            .lock()
            .expect("Codex session id poisoned") = Some(session.native_session_id.clone());
        Ok(RuntimeSessionResult {
            changed: true,
            sessions: vec![session],
            cursor: None,
            message: None,
        })
    }

    async fn fork_session(
        &self,
        request: RuntimeSessionRequest,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        self.ensure_session_cwd(&request).await?;
        let native_session_id = require_native_session_id(&request)?;
        let last_turn_id = request
            .argument
            .as_ref()
            .and_then(|value| value.get("lastTurnId"))
            .cloned()
            .unwrap_or(Value::Null);
        let response = self
            .transport()
            .request(
                "thread/fork",
                json!({
                    "threadId": native_session_id,
                    "lastTurnId": last_turn_id,
                    "cwd": request.cwd.to_string_lossy(),
                }),
            )
            .await?;
        let session = response
            .get("thread")
            .and_then(|thread| session_from_thread(thread, false))
            .ok_or_else(|| protocol_error("thread/fork", "response did not include thread"))?;
        Ok(RuntimeSessionResult {
            changed: true,
            sessions: vec![session],
            cursor: None,
            message: None,
        })
    }

    async fn simple_mutation(
        &self,
        request: RuntimeSessionRequest,
        method: &str,
        retain_session: bool,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        self.ensure_session_cwd(&request).await?;
        let native_session_id = require_native_session_id(&request)?;
        let response = self
            .transport()
            .request(method, json!({"threadId": native_session_id}))
            .await?;
        let sessions = if retain_session {
            response
                .get("thread")
                .and_then(|thread| session_from_thread(thread, method == "thread/archive"))
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        Ok(RuntimeSessionResult {
            changed: true,
            sessions,
            cursor: None,
            message: Some(if matches!(method, "thread/archive" | "thread/delete") {
                "Codex applies this operation to spawned descendants as well".to_string()
            } else {
                "Codex session updated".to_string()
            }),
        })
    }

    async fn rename_session(
        &self,
        request: RuntimeSessionRequest,
    ) -> Result<RuntimeSessionResult, RuntimeError> {
        self.ensure_session_cwd(&request).await?;
        let native_session_id = require_native_session_id(&request)?;
        let name = request
            .argument
            .as_ref()
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("title").and_then(Value::as_str))
            })
            .ok_or_else(|| {
                RuntimeError::new(
                    "codex_missing_session_name",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    "Codex session rename requires a title",
                )
            })?;
        self.transport()
            .request(
                "thread/name/set",
                json!({"threadId": native_session_id, "name": name}),
            )
            .await?;
        self.read_session(request, true).await
    }

    async fn ensure_session_cwd(
        &self,
        request: &RuntimeSessionRequest,
    ) -> Result<(), RuntimeError> {
        let native_session_id = require_native_session_id(request)?;
        let response = self
            .transport()
            .request(
                "thread/read",
                json!({"threadId": native_session_id, "includeTurns": false}),
            )
            .await?;
        let native_cwd = response
            .get("thread")
            .and_then(|thread| thread.get("cwd"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                protocol_error("thread/read", "session mutation cwd could not be verified")
            })?;
        if !same_cwd(&request.cwd, Path::new(native_cwd)) {
            return Err(RuntimeError::new(
                "codex_session_cwd_mismatch",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                format!(
                    "Codex session cwd `{native_cwd}` does not match the bound Gateway cwd `{}`",
                    request.cwd.display()
                ),
            ));
        }
        Ok(())
    }

    fn has_interaction(&self, interaction_id: &str) -> bool {
        self.state
            .interactions
            .lock()
            .expect("Codex interactions poisoned")
            .contains_key(interaction_id)
    }

    async fn respond_interaction(
        &self,
        response: crate::RuntimeInteractionResponse,
    ) -> Result<RuntimeInteractionResult, RuntimeError> {
        let (native_request_id, result) = {
            let mut interactions = self
                .state
                .interactions
                .lock()
                .expect("Codex interactions poisoned");
            let Some(interaction) = interactions.get(&response.interaction_id) else {
                return Ok(RuntimeInteractionResult {
                    accepted: false,
                    expired: true,
                    message: Some("Codex interaction is no longer pending".to_string()),
                });
            };
            if response.process_epoch != interaction.process_epoch {
                return Ok(RuntimeInteractionResult {
                    accepted: false,
                    expired: true,
                    message: Some("Codex interaction belongs to an earlier process".to_string()),
                });
            }
            let result = interaction_result(
                &interaction.method,
                response.response,
                &interaction.question_ids,
                &interaction.question_multiple,
                interaction.requested_permissions.as_ref(),
            )?;
            let interaction = interactions
                .remove(&response.interaction_id)
                .expect("Codex pending interaction disappeared");
            (interaction.native_request_id, result)
        };
        self.transport().respond(native_request_id, result).await?;
        Ok(RuntimeInteractionResult {
            accepted: true,
            expired: false,
            message: None,
        })
    }

    async fn shutdown(&self, force: bool) -> Result<(), RuntimeError> {
        self.transport().shutdown(force).await
    }
}

fn validate_request(request: &ExecuteRequest) -> Result<(), RuntimeError> {
    if request.profile.kind != RuntimeKind::Codex {
        return Err(RuntimeError::new(
            "codex_wrong_runtime_kind",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex adapter received a non-Codex Runtime Profile",
        ));
    }
    if !request.profile.enabled {
        return Err(RuntimeError::new(
            "codex_profile_disabled",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex Runtime Profile is disabled",
        ));
    }
    if request.expected_profile_revision != request.profile.revision {
        return Err(RuntimeError::new(
            "stale_profile_revision",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            "Codex Runtime Profile revision is stale",
        ));
    }
    if request
        .expected_capability_revision
        .is_some_and(|revision| revision != CAPABILITY_REVISION)
    {
        return Err(RuntimeError::new(
            "stale_capability_revision",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            "Codex capability revision is stale",
        ));
    }
    Ok(())
}

fn codex_stable_policy_issue(profile: &RuntimeProfile) -> Option<String> {
    if let Some(approval) = profile
        .approval_mode
        .as_deref()
        .filter(|value| !value.is_empty())
        && !matches!(approval, "untrusted" | "on-request" | "never")
    {
        return Some(format!(
            "Codex approval policy `{approval}` is not available on the Stable direct-runtime path"
        ));
    }
    if let Some(sandbox) = profile.sandbox.as_deref().filter(|value| !value.is_empty())
        && !matches!(
            sandbox,
            "read-only" | "workspace-write" | "danger-full-access"
        )
    {
        return Some(format!(
            "Codex sandbox `{sandbox}` is not available on the Stable direct-runtime path"
        ));
    }
    None
}

fn codex_user_agent_version(user_agent: &str) -> Option<(u64, u64, u64)> {
    let version = user_agent
        .split_ascii_whitespace()
        .find_map(|part| part.strip_prefix("codex_cli_rs/"))?;
    let numeric = version.split(['-', '+']).next()?;
    let mut components = numeric.split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    let patch = components.next()?.parse().ok()?;
    (components.next().is_none()).then_some((major, minor, patch))
}

fn validate_turn_request(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
) -> Result<(), RuntimeError> {
    if !profile.workspace_roots.is_empty() {
        return Err(RuntimeError::new(
            "codex_experimental_workspace_roots_unsupported",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex runtime workspace roots are experimental upstream and are unavailable on the Stable direct-runtime path.",
        ));
    }
    if let Some(issue) = codex_stable_policy_issue(profile) {
        return Err(RuntimeError::new(
            "policy_not_enforceable",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            issue,
        ));
    }
    if turn.agent.as_deref().is_some_and(|agent| !agent.is_empty()) {
        return Err(unsupported(
            "Codex agent selection",
            turn.agent.as_deref().unwrap_or_default(),
        ));
    }
    if let Some(mode) = turn.mode.as_deref().or(profile.default_mode.as_deref())
        && !matches!(mode, "default" | "plan" | "auto-review" | "full-access")
    {
        return Err(unsupported("Codex mode", mode));
    }
    if turn.mode.as_deref().or(profile.default_mode.as_deref()) == Some("plan")
        && !turn
            .interaction_exposure
            .allows(RuntimeInteractionExposure::GuiAdvancedOnly)
    {
        return Err(RuntimeError::new(
            "codex_experimental_mode_requires_gui_advanced",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex plan collaboration mode is experimental and requires a GUI Advanced turn; ordinary GUI and Channel turns use Standard exposure.",
        ));
    }
    if turn.mode.as_deref().or(profile.default_mode.as_deref()) == Some("full-access")
        && profile.sandbox.as_deref() != Some("danger-full-access")
    {
        return Err(RuntimeError::new(
            "policy_not_enforceable",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex full-access mode requires an explicitly bound danger-full-access sandbox policy",
        ));
    }
    for (feature, value) in &turn.features {
        let supported = match feature.as_str() {
            "effort" | "summary" | "personality" => value.is_string(),
            "serviceTier" => value.is_string() || value.is_null(),
            "outputSchema" => value.is_object(),
            _ => false,
        };
        if !supported {
            return Err(unsupported("Codex turn feature", feature));
        }
    }
    Ok(())
}

fn codex_turn_requires_catalog(turn: &RuntimeTurnRequest) -> bool {
    turn.model.is_some()
        || turn
            .features
            .keys()
            .any(|feature| matches!(feature.as_str(), "effort" | "personality" | "serviceTier"))
}

fn validate_catalog_backed_turn_options(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
    session_model: Option<&str>,
    catalog: Option<&[CodexModelCatalogEntry]>,
) -> Result<(), RuntimeError> {
    if !codex_turn_requires_catalog(turn) {
        return Ok(());
    }
    let catalog = catalog.ok_or_else(|| {
        RuntimeError::new(
            "codex_model_catalog_required",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            "Codex model and catalog-backed turn options require an observed model/list catalog; refresh the Runtime Profile catalog before retrying",
        )
    })?;
    let effective_model = turn
        .model
        .as_deref()
        .or(session_model)
        .or(profile.default_model.as_deref())
        .or_else(|| {
            catalog
                .iter()
                .find(|model| model.is_default)
                .map(|model| model.model.as_str())
        })
        .ok_or_else(|| {
            RuntimeError::new(
                "codex_effective_model_unavailable",
                RuntimeErrorStage::Configuration,
                RetryClass::UserAction,
                "Codex catalog-backed turn options require an exact effective model",
            )
        })?;
    let model = catalog
        .iter()
        .find(|model| model.model == effective_model)
        .ok_or_else(|| {
            RuntimeError::new(
                "codex_model_not_in_catalog",
                RuntimeErrorStage::Configuration,
                RetryClass::UserAction,
                format!(
                    "Codex model `{effective_model}` is not an exact visible model/list choice"
                ),
            )
        })?;
    if let Some(effort) = turn.features.get("effort").and_then(Value::as_str)
        && !model
            .reasoning_efforts
            .iter()
            .any(|choice| choice.value.as_str() == Some(effort))
    {
        return Err(codex_catalog_choice_error(
            "effort",
            effort,
            effective_model,
        ));
    }
    if let Some(personality) = turn.features.get("personality").and_then(Value::as_str)
        && (!model.supports_personality
            || !matches!(personality, "none" | "friendly" | "pragmatic"))
    {
        return Err(codex_catalog_choice_error(
            "personality",
            personality,
            effective_model,
        ));
    }
    if let Some(service_tier) = turn.features.get("serviceTier") {
        let Some(service_tier) = service_tier.as_str() else {
            return Err(codex_catalog_choice_error(
                "serviceTier",
                "null",
                effective_model,
            ));
        };
        if !model
            .service_tiers
            .iter()
            .any(|choice| choice.value.as_str() == Some(service_tier))
        {
            return Err(codex_catalog_choice_error(
                "serviceTier",
                service_tier,
                effective_model,
            ));
        }
    }
    Ok(())
}

fn codex_catalog_choice_error(control: &str, value: &str, model: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_catalog_choice_unsupported",
        RuntimeErrorStage::Configuration,
        RetryClass::UserAction,
        format!(
            "Codex {control} `{value}` is not an exact model/list choice for effective model `{model}`"
        ),
    )
}

fn codex_catalog_control_model<'a>(
    profile: &RuntimeProfile,
    catalog: &'a [CodexModelCatalogEntry],
) -> Option<&'a CodexModelCatalogEntry> {
    match profile.default_model.as_deref() {
        Some(default_model) => catalog.iter().find(|model| model.model == default_model),
        None => catalog.iter().find(|model| model.is_default),
    }
}

fn codex_catalog_controls(
    catalog: &[CodexModelCatalogEntry],
    control_model: Option<&CodexModelCatalogEntry>,
) -> Vec<RuntimeControlDescriptor> {
    if catalog.is_empty() {
        return Vec::new();
    }
    let mut controls = vec![RuntimeControlDescriptor {
        id: "model".to_string(),
        label: "Model".to_string(),
        state: ControlState::Selectable,
        current_value: None,
        choices: catalog
            .iter()
            .map(|model| RuntimeControlChoice {
                value: Value::String(model.model.clone()),
                label: model.display_name.clone(),
                description: model.description.clone(),
            })
            .collect(),
        depends_on: None,
        channel_safe: false,
        capability_revision: CAPABILITY_REVISION,
    }];
    let Some(model) = control_model else {
        return controls;
    };
    if !model.reasoning_efforts.is_empty() {
        controls.push(RuntimeControlDescriptor {
            id: "effort".to_string(),
            label: "Reasoning effort".to_string(),
            state: ControlState::Selectable,
            current_value: None,
            choices: model.reasoning_efforts.clone(),
            depends_on: Some(RuntimeControlDependency {
                control_id: "model".to_string(),
                value: Value::String(model.model.clone()),
            }),
            channel_safe: false,
            capability_revision: CAPABILITY_REVISION,
        });
    }
    if model.supports_personality {
        controls.push(RuntimeControlDescriptor {
            id: "personality".to_string(),
            label: "Personality".to_string(),
            state: ControlState::Selectable,
            current_value: None,
            choices: [
                ("none", "None"),
                ("friendly", "Friendly"),
                ("pragmatic", "Pragmatic"),
            ]
            .into_iter()
            .map(|(value, label)| RuntimeControlChoice {
                value: Value::String(value.to_string()),
                label: label.to_string(),
                description: None,
            })
            .collect(),
            depends_on: Some(RuntimeControlDependency {
                control_id: "model".to_string(),
                value: Value::String(model.model.clone()),
            }),
            channel_safe: false,
            capability_revision: CAPABILITY_REVISION,
        });
    }
    if !model.service_tiers.is_empty() {
        controls.push(RuntimeControlDescriptor {
            id: "serviceTier".to_string(),
            label: "Service tier".to_string(),
            state: ControlState::Selectable,
            current_value: None,
            choices: model.service_tiers.clone(),
            depends_on: Some(RuntimeControlDependency {
                control_id: "model".to_string(),
                value: Value::String(model.model.clone()),
            }),
            channel_safe: false,
            capability_revision: CAPABILITY_REVISION,
        });
    }
    controls
}

fn snapshot_from_cache(
    profile: RuntimeProfile,
    scope: SnapshotScope,
    workers: &[Arc<CodexWorker>],
) -> RuntimeSnapshot {
    let worker = workers
        .iter()
        .filter(|worker| !worker.is_disposed())
        .max_by_key(|worker| worker.state.process_epoch);
    let configured = profile.enabled
        && profile
            .command
            .as_deref()
            .is_some_and(|command| !command.trim().is_empty());
    let runtime_version = worker.and_then(|worker| {
        worker
            .state
            .runtime_version
            .lock()
            .expect("Codex runtime version poisoned")
            .clone()
    });
    let auth_readiness = worker.and_then(|worker| {
        worker
            .state
            .auth_readiness
            .lock()
            .expect("Codex auth readiness poisoned")
            .clone()
    });
    let stable_turn_hydrated =
        worker.is_some_and(|worker| worker.state.stable_turn_hydrated.load(Ordering::SeqCst));
    let catalog_hydrated = worker.is_some_and(|worker| {
        worker
            .state
            .model_catalog
            .lock()
            .expect("Codex model catalog poisoned")
            .is_some()
    });
    let version_compatible = runtime_version
        .as_deref()
        .and_then(codex_user_agent_version)
        .is_some_and(|version| version >= CODEX_STABLE_MATRIX_MIN_VERSION);
    let matrix_hydrated = version_compatible && stable_turn_hydrated && catalog_hydrated;
    let policy_issue = (!profile.workspace_roots.is_empty())
        .then_some("Codex workspace roots require the disabled experimental API".to_string())
        .or_else(|| codex_stable_policy_issue(&profile));
    let readiness = vec![
        ReadinessStage {
            id: "configuration".to_string(),
            status: if configured {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Missing
            },
            summary: if configured {
                "Codex Runtime Profile is configured".to_string()
            } else {
                "Codex runtime command is missing or disabled".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "transport".to_string(),
            status: if worker.is_some() {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if worker.is_some() {
                "Codex app-server handshake was observed".to_string()
            } else {
                "Codex app-server has not been started".to_string()
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
            summary: match runtime_version.as_ref() {
                None => "Codex runtime identity has not been observed".to_string(),
                Some(version) if version_compatible => format!(
                    "Codex runtime identity `{version}` satisfies the Stable matrix manifest"
                ),
                Some(version) => format!(
                    "Codex runtime identity `{version}` is below or outside the Stable matrix manifest"
                ),
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "authentication".to_string(),
            status: auth_readiness
                .as_ref()
                .map(|(status, _)| *status)
                .unwrap_or(ReadinessStatus::Unchecked),
            summary: auth_readiness
                .as_ref()
                .map(|(_, summary)| summary.clone())
                .unwrap_or_else(|| "Codex authentication has not been checked".to_string()),
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "catalog".to_string(),
            status: if catalog_hydrated {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if catalog_hydrated {
                "Codex model/list catalog was hydrated from a stable catalog read".to_string()
            } else {
                "Codex model/list catalog has not been hydrated".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "capabilities".to_string(),
            status: if runtime_version.is_some() && !version_compatible {
                ReadinessStatus::Unsupported
            } else if matrix_hydrated {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if matrix_hydrated {
                "Codex completed a correlated Stable turn and hydrated model/list under a compatible manifest"
                    .to_string()
            } else if runtime_version.is_some() && !version_compatible {
                "Codex runtime version cannot prove the complete Stable capability matrix"
                    .to_string()
            } else if catalog_hydrated && !stable_turn_hydrated {
                "Codex model catalog is hydrated, but no correlated Stable turn has completed"
                    .to_string()
            } else if stable_turn_hydrated && !catalog_hydrated {
                "Codex completed a correlated Stable turn, but model/list is not hydrated"
                    .to_string()
            } else {
                "Codex Stable turn and model catalog hydration are incomplete".to_string()
            },
            observed_at_ms: None,
        },
        ReadinessStage {
            id: "policy".to_string(),
            status: if policy_issue.is_some() {
                ReadinessStatus::Unsupported
            } else if worker.is_some() {
                ReadinessStatus::Ready
            } else {
                ReadinessStatus::Unchecked
            },
            summary: if let Some(issue) = policy_issue {
                issue
            } else if worker.is_some() {
                "Codex launch and sandbox policy passed Stable-path validation".to_string()
            } else {
                "Codex Stable launch policy has not been exercised".to_string()
            },
            observed_at_ms: None,
        },
    ];
    let session_worker = match &scope {
        SnapshotScope::Session {
            native_session_id: Some(native_session_id),
            ..
        } => workers.iter().find(|worker| {
            worker
                .state
                .session_id
                .lock()
                .expect("Codex session id poisoned")
                .as_deref()
                == Some(native_session_id.as_str())
        }),
        _ => worker,
    };
    let observed_model = match &scope {
        SnapshotScope::Session {
            native_session_id: Some(native_session_id),
            ..
        } => workers.iter().find_map(|worker| {
            let observed_session_id = worker
                .state
                .session_id
                .lock()
                .expect("Codex session id poisoned")
                .clone();
            (observed_session_id.as_deref() == Some(native_session_id.as_str()))
                .then(|| {
                    worker
                        .state
                        .session_model
                        .lock()
                        .expect("Codex session model poisoned")
                        .clone()
                })
                .flatten()
        }),
        SnapshotScope::Profile
        | SnapshotScope::Workspace { .. }
        | SnapshotScope::Session {
            native_session_id: None,
            ..
        } => None,
    };
    let model_catalog = worker
        .and_then(|worker| {
            worker
                .state
                .model_catalog
                .lock()
                .expect("Codex model catalog poisoned")
                .clone()
        })
        .unwrap_or_default();
    let control_model = codex_catalog_control_model(&profile, &model_catalog);
    let controls = if let Some(model) = observed_model {
        vec![RuntimeControlDescriptor {
            id: "model".to_string(),
            label: "Model".to_string(),
            state: ControlState::ReadOnlyCurrent,
            current_value: Some(Value::String(model)),
            choices: Vec::new(),
            depends_on: None,
            channel_safe: false,
            capability_revision: CAPABILITY_REVISION,
        }]
    } else {
        codex_catalog_controls(&model_catalog, control_model)
    };
    let latest_plan = session_worker.and_then(|worker| {
        worker
            .state
            .latest_plan
            .lock()
            .expect("Codex plan cache poisoned")
            .clone()
    });
    let latest_diff = session_worker.and_then(|worker| {
        worker
            .state
            .latest_diff
            .lock()
            .expect("Codex diff cache poisoned")
            .clone()
    });
    let latest_usage = session_worker.and_then(|worker| {
        worker
            .state
            .latest_usage
            .lock()
            .expect("Codex usage cache poisoned")
            .clone()
    });
    let latest_goal = session_worker.and_then(|worker| {
        worker
            .state
            .latest_goal
            .lock()
            .expect("Codex goal cache poisoned")
            .clone()
    });
    let account_rate_limits = workers.first().and_then(|worker| {
        worker
            .state
            .account_rate_limits_by_profile
            .lock()
            .expect("Codex rate-limit cache poisoned")
            .get(&profile_worker_namespace(&profile))
            .cloned()
            .flatten()
    });
    let extension = (control_model.is_some()
        || latest_plan.is_some()
        || latest_diff.is_some()
        || latest_usage.is_some()
        || latest_goal.is_some()
        || account_rate_limits.is_some())
    .then(|| {
        json!({
            "codex": {
                "controlModel": control_model.map(|model| model.model.as_str()),
            },
            "plan": latest_plan,
            "diff": latest_diff,
            "usage": latest_usage,
            "goal": latest_goal,
            "accountRateLimits": account_rate_limits,
        })
    });
    RuntimeSnapshot {
        runtime_ref: profile.id.clone(),
        kind: RuntimeKind::Codex,
        profile_revision: profile.revision,
        capability_revision: CAPABILITY_REVISION,
        adapter_version: ADAPTER_VERSION.to_string(),
        runtime_version,
        // The ordinary app-server turn/session path is stable. Optional
        // experimental capabilities (for example requestUserInput) carry
        // their own stability below and do not downgrade the adapter itself.
        stability: RuntimeStability::Stable,
        provenance: "runtime_profile".to_string(),
        readiness,
        controls,
        capabilities: codex_capabilities(matrix_hydrated),
        process_epoch: worker.map(|worker| worker.state.process_epoch),
        instance_epoch: None,
        binding_epoch: None,
        extension,
    }
}

fn snapshot_probe_cwd(query: &SnapshotQuery) -> Result<std::path::PathBuf, RuntimeError> {
    match &query.scope {
        SnapshotScope::Workspace { cwd } | SnapshotScope::Session { cwd, .. } => Ok(cwd.clone()),
        SnapshotScope::Profile => query
            .profile
            .workspace_roots
            .first()
            .cloned()
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| {
                RuntimeError::new(
                    "codex_probe_cwd_unavailable",
                    RuntimeErrorStage::Discovery,
                    RetryClass::UserAction,
                    "Codex probe requires a workspace cwd",
                )
            }),
    }
}

fn same_cwd(expected: &Path, actual: &Path) -> bool {
    match (
        std::fs::canonicalize(expected),
        std::fs::canonicalize(actual),
    ) {
        (Ok(expected), Ok(actual)) => expected == actual,
        _ => expected == actual,
    }
}

fn codex_capabilities(hydrated: bool) -> Vec<RuntimeCapability> {
    let mut capabilities = [
        "session.list",
        "session.read",
        "session.resume",
        "session.fork",
        "session.archive",
        "session.unarchive",
        "session.rename",
        "session.delete",
        "turn.start",
        "turn.steer",
        "turn.interrupt",
        "model.catalog",
        "thread.compact",
        "thread.goal.read",
        "thread.goal.set",
        "thread.goal.clear",
        "thread.usage",
        "account.rate_limits.read",
        "timeline.plan",
        "timeline.diff",
        "interaction.command",
        "interaction.file",
        "interaction.permission",
        "children.read_only",
        "auth.status",
        "auth.login",
        "auth.cancel",
        "auth.logout",
    ]
    .into_iter()
    .map(|id| RuntimeCapability {
        id: id.to_string(),
        enabled: hydrated,
        stability: RuntimeStability::Stable,
    })
    .collect::<Vec<_>>();
    capabilities.push(RuntimeCapability {
        id: "interaction.question".to_string(),
        enabled: hydrated,
        stability: RuntimeStability::Experimental,
    });
    capabilities
}

fn decode_session_list_cursor(
    cursor: Option<&str>,
) -> Result<(Option<String>, Option<String>), RuntimeError> {
    let Some(cursor) = cursor else {
        return Ok((None, None));
    };
    let encoded = cursor.strip_prefix("codex:list:").ok_or_else(|| {
        RuntimeError::new(
            "codex_invalid_session_cursor",
            RuntimeErrorStage::History,
            RetryClass::UserAction,
            "Codex session cursor is invalid or belongs to another runtime",
        )
    })?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        RuntimeError::new(
            "codex_invalid_session_cursor",
            RuntimeErrorStage::History,
            RetryClass::UserAction,
            "Codex session cursor could not be decoded",
        )
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        RuntimeError::new(
            "codex_invalid_session_cursor",
            RuntimeErrorStage::History,
            RetryClass::UserAction,
            "Codex session cursor did not contain valid state",
        )
    })?;
    Ok((
        value
            .get("active")
            .and_then(Value::as_str)
            .map(str::to_string),
        value
            .get("archived")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

fn encode_session_list_cursor(active: Option<String>, archived: Option<String>) -> Option<String> {
    if active.is_none() && archived.is_none() {
        return None;
    }
    let value = json!({ "active": active, "archived": archived });
    Some(format!(
        "codex:list:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).expect("session cursor serializes"))
    ))
}

fn thread_open_params(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
    resume_id: Option<&str>,
) -> Value {
    let mut params = serde_json::Map::new();
    if let Some(thread_id) = resume_id {
        params.insert("threadId".to_string(), Value::String(thread_id.to_string()));
    }
    params.insert(
        "cwd".to_string(),
        Value::String(turn.cwd.to_string_lossy().to_string()),
    );
    if let Some(instructions) = turn
        .instructions
        .as_deref()
        .filter(|instructions| !instructions.trim().is_empty())
    {
        params.insert(
            "developerInstructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }
    if let Some(model) = turn.model.as_ref().or(profile.default_model.as_ref()) {
        params.insert("model".to_string(), Value::String(model.clone()));
    }
    if let Some(approval_policy) = profile.approval_mode.as_ref() {
        params.insert(
            "approvalPolicy".to_string(),
            Value::String(approval_policy.clone()),
        );
    }
    let mode = turn.mode.as_deref().or(profile.default_mode.as_deref());
    if mode == Some("auto-review") {
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String("auto_review".to_string()),
        );
    }
    if let Some(sandbox) = profile.sandbox.as_ref() {
        params.insert("sandbox".to_string(), Value::String(sandbox.clone()));
    }
    Value::Object(params)
}

fn turn_start_params(
    profile: &RuntimeProfile,
    turn: &RuntimeTurnRequest,
    native_session_id: &str,
    session_model: Option<&str>,
) -> Value {
    let mut params = serde_json::Map::new();
    params.insert(
        "threadId".to_string(),
        Value::String(native_session_id.to_string()),
    );
    params.insert(
        "clientUserMessageId".to_string(),
        Value::String(turn.turn_id.clone()),
    );
    params.insert(
        "input".to_string(),
        json!([{"type": "text", "text": turn.prompt, "textElements": []}]),
    );
    params.insert(
        "cwd".to_string(),
        Value::String(turn.cwd.to_string_lossy().to_string()),
    );
    let model = turn
        .model
        .as_deref()
        .or(session_model)
        .or(profile.default_model.as_deref());
    if let Some(model) = model {
        params.insert("model".to_string(), Value::String(model.to_string()));
    }
    let selected_mode = turn.mode.as_deref().or(profile.default_mode.as_deref());
    match selected_mode {
        Some("plan") => {
            // Upstream marks turn/start.collaborationMode experimental. It is
            // reserved for an explicitly selected Advanced mode; default turns
            // keep Agent instructions on stable thread start/resume fields.
            let reasoning_effort = turn
                .features
                .get("effort")
                .cloned()
                .unwrap_or_else(|| Value::String("medium".to_string()));
            params.insert(
                "collaborationMode".to_string(),
                json!({
                    "mode": "plan",
                    "settings": {
                        "model": model.unwrap_or_default(),
                        "reasoning_effort": reasoning_effort,
                        "developer_instructions": turn.instructions,
                    }
                }),
            );
        }
        Some("auto-review") => {
            params.insert(
                "approvalsReviewer".to_string(),
                Value::String("auto_review".to_string()),
            );
        }
        // The immutable thread already carries the Stable sandbox mode. Do not
        // switch to upstream's experimental named `permissions` field.
        Some("full-access") => {}
        Some("default") | None => {}
        Some(_) => unreachable!("Codex mode was validated before turn start"),
    }
    for (feature, value) in &turn.features {
        params.insert(feature.clone(), value.clone());
    }
    Value::Object(params)
}

struct NormalizedInteraction {
    policy: RuntimeInteractionPolicy,
    kind: String,
    prompt: String,
    questions: Vec<RuntimeInteractionQuestion>,
    choices: Vec<RuntimeInteractionChoice>,
    authorization_lifetime: Option<String>,
    expires_at_ms: Option<i64>,
    question_ids: Vec<String>,
}

fn codex_permission_interaction(
    kind: &str,
    prompt: String,
    authorization_lifetime: Option<String>,
) -> NormalizedInteraction {
    NormalizedInteraction {
        policy: RuntimeInteractionPolicy {
            kind: RuntimeInteractionKind::Permission,
            stability: RuntimeStability::Stable,
            exposure: RuntimeInteractionExposure::Standard,
        },
        kind: kind.to_string(),
        prompt,
        questions: Vec::new(),
        choices: approval_choices(true),
        authorization_lifetime,
        expires_at_ms: None,
        question_ids: Vec::new(),
    }
}

fn normalized_interaction(method: &str, params: &Value) -> Option<NormalizedInteraction> {
    match method {
        "item/commandExecution/requestApproval" => Some(codex_permission_interaction(
            "command",
            params
                .get("reason")
                .and_then(Value::as_str)
                .or_else(|| params.get("command").and_then(Value::as_str))
                .unwrap_or("Allow this command?")
                .to_string(),
            Some("codex_session".to_string()),
        )),
        "item/fileChange/requestApproval" => Some(codex_permission_interaction(
            "file_change",
            params
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("Allow this file change?")
                .to_string(),
            Some("codex_session".to_string()),
        )),
        "item/permissions/requestApproval" => Some(codex_permission_interaction(
            "permission",
            params
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("Allow these additional permissions?")
                .to_string(),
            Some("codex_session".to_string()),
        )),
        "item/tool/requestUserInput" => {
            let native_questions = params.get("questions")?.as_array()?;
            let question_ids = native_questions
                .iter()
                .filter_map(|question| question.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            let questions = native_questions
                .iter()
                .map(|question| {
                    let options = question
                        .get("options")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|option| RuntimeInteractionQuestionOption {
                            label: option
                                .get("label")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect::<Vec<_>>();
                    RuntimeInteractionQuestion {
                        header: question
                            .get("header")
                            .and_then(Value::as_str)
                            .filter(|header| !header.trim().is_empty())
                            .map(str::to_string),
                        question: question
                            .get("question")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        custom: question
                            .get("isOther")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            || options.is_empty(),
                        secret: question
                            .get("isSecret")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        multiple: false,
                        options,
                    }
                })
                .collect::<Vec<_>>();
            let prompt = questions
                .iter()
                .map(|question| question.question.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            Some(NormalizedInteraction {
                policy: RuntimeInteractionPolicy {
                    kind: RuntimeInteractionKind::Question,
                    stability: RuntimeStability::Experimental,
                    exposure: RuntimeInteractionExposure::GuiAdvancedOnly,
                },
                kind: "question".to_string(),
                prompt,
                questions,
                choices: Vec::new(),
                authorization_lifetime: None,
                expires_at_ms: codex_interaction_expiry(params),
                question_ids,
            })
        }
        _ => None,
    }
}

fn approval_choices(include_session: bool) -> Vec<RuntimeInteractionChoice> {
    let mut choices = vec![
        RuntimeInteractionChoice {
            id: "accept".to_string(),
            label: "Allow once".to_string(),
            decision: "accept".to_string(),
        },
        RuntimeInteractionChoice {
            id: "decline".to_string(),
            label: "Deny".to_string(),
            decision: "decline".to_string(),
        },
        RuntimeInteractionChoice {
            id: "cancel".to_string(),
            label: "Cancel turn".to_string(),
            decision: "cancel".to_string(),
        },
    ];
    if include_session {
        choices.insert(
            1,
            RuntimeInteractionChoice {
                id: "accept_for_session".to_string(),
                label: "Allow for this Codex session".to_string(),
                decision: "acceptForSession".to_string(),
            },
        );
    }
    choices
}

fn normalized_interaction_metadata(params: &Value) -> Option<Value> {
    let mut metadata = serde_json::Map::new();
    for key in [
        "itemId",
        "command",
        "cwd",
        "grantRoot",
        "startedAtMs",
        "permissions",
        "environmentId",
    ] {
        if let Some(value) = params.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}

fn codex_interaction_expiry(params: &Value) -> Option<i64> {
    let timeout_ms = params.get("autoResolutionMs")?.as_u64()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())?;
    Some(now_ms.saturating_add(i64::try_from(timeout_ms).unwrap_or(i64::MAX)))
}

fn interaction_result(
    method: &str,
    response: Value,
    question_ids: &[String],
    question_multiple: &[bool],
    requested_permissions: Option<&Value>,
) -> Result<Value, RuntimeError> {
    if let Some(result) = response.get("result") {
        return Ok(result.clone());
    }
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let decision = response
                .get("decision")
                .and_then(Value::as_str)
                .or_else(|| response.as_str())
                .ok_or_else(|| invalid_interaction("approval response requires a decision"))?;
            let decision = match decision {
                "allow_once" | "accept" => "accept",
                "allow_session" | "accept_for_session" | "acceptForSession" => "acceptForSession",
                "deny" | "decline" => "decline",
                "cancel" => "cancel",
                _ => return Err(invalid_interaction("unsupported Codex approval decision")),
            };
            Ok(json!({"decision": decision}))
        }
        "item/permissions/requestApproval" => {
            let decision = response
                .get("decision")
                .and_then(Value::as_str)
                .or_else(|| response.as_str())
                .ok_or_else(|| invalid_interaction("permission response requires a decision"))?;
            let (permissions, scope) = match decision {
                "allow_once" | "accept" => (
                    requested_permissions.cloned().unwrap_or_else(|| json!({})),
                    "turn",
                ),
                "allow_session" | "accept_for_session" | "acceptForSession" => (
                    requested_permissions.cloned().unwrap_or_else(|| json!({})),
                    "session",
                ),
                "deny" | "decline" | "cancel" => (json!({}), "turn"),
                _ => {
                    return Err(invalid_interaction("unsupported Codex permission decision"));
                }
            };
            Ok(json!({"permissions": permissions, "scope": scope}))
        }
        "item/tool/requestUserInput" => {
            if response
                .get("reject")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || response.get("decision").and_then(Value::as_str) == Some("cancel")
            {
                // Codex's native fail-closed response is an empty typed answer
                // map. The app-server completes the request without exposing
                // or fabricating answers.
                return Ok(json!({"answers": {}}));
            }
            if let Some(answers) = response.get("answers")
                && let Some(answers) = answers.as_object()
            {
                validate_keyed_question_answers(answers, question_ids, question_multiple)?;
                return Ok(json!({"answers": answers}));
            }
            let answer_lists = response
                .get("answers")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid_interaction("question response requires answers"))?;
            if answer_lists.len() != question_ids.len() {
                return Err(invalid_interaction(
                    "question response must contain exactly one answer row per question",
                ));
            }
            if question_multiple.len() != question_ids.len() {
                return Err(invalid_interaction(
                    "question response shape does not match the native questions",
                ));
            }
            for (answers, multiple) in answer_lists.iter().zip(question_multiple) {
                let answers = answers.as_array().ok_or_else(|| {
                    invalid_interaction("each question response must be an answer array")
                })?;
                validate_question_answer_values(answers, *multiple)?;
            }
            let answers = question_ids
                .iter()
                .zip(answer_lists)
                .map(|(id, answers)| {
                    (
                        id.clone(),
                        json!({"answers": answers.as_array().cloned().unwrap_or_default()}),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            Ok(json!({"answers": answers}))
        }
        _ => Err(invalid_interaction("unsupported Codex interaction")),
    }
}

fn validate_keyed_question_answers(
    answers: &serde_json::Map<String, Value>,
    question_ids: &[String],
    question_multiple: &[bool],
) -> Result<(), RuntimeError> {
    if answers.len() != question_ids.len() || question_multiple.len() != question_ids.len() {
        return Err(invalid_interaction(
            "question response must contain exactly one answer row per question",
        ));
    }
    for (question_id, multiple) in question_ids.iter().zip(question_multiple) {
        let values = answers
            .get(question_id)
            .and_then(|answer| answer.get("answers"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                invalid_interaction("question response is missing a typed answer row")
            })?;
        validate_question_answer_values(values, *multiple)?;
    }
    Ok(())
}

fn validate_question_answer_values(answers: &[Value], multiple: bool) -> Result<(), RuntimeError> {
    if answers.is_empty() {
        return Err(invalid_interaction(
            "question response must contain at least one answer",
        ));
    }
    if !multiple && answers.len() != 1 {
        return Err(invalid_interaction(
            "single-selection question response must contain exactly one answer",
        ));
    }
    if !answers.iter().all(Value::is_string) {
        return Err(invalid_interaction("question answers must be strings"));
    }
    Ok(())
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoalTarget {
    thread_id: String,
    native_session_id: String,
    cwd: std::path::PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoalSetArgument {
    thread_id: String,
    native_session_id: String,
    cwd: std::path::PathBuf,
    objective: Option<String>,
    status: Option<RuntimeGoalStatus>,
    #[serde(default)]
    token_budget: GoalTokenBudgetInput,
}

impl GoalSetArgument {
    fn target(&self) -> GoalTarget {
        GoalTarget {
            thread_id: self.thread_id.clone(),
            native_session_id: self.native_session_id.clone(),
            cwd: self.cwd.clone(),
        }
    }
}

fn validate_goal_set_argument(argument: &GoalSetArgument) -> Result<(), RuntimeError> {
    if argument
        .objective
        .as_deref()
        .is_some_and(|objective| objective.trim().is_empty())
    {
        return Err(invalid_extension_schema(
            "Codex goal objective must not be empty",
        ));
    }
    if matches!(argument.token_budget, GoalTokenBudgetInput::Set(value) if value < 0) {
        return Err(invalid_extension_schema(
            "Codex goal tokenBudget must be non-negative",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum GoalTokenBudgetInput {
    #[default]
    Missing,
    Clear,
    Set(i64),
}

impl<'de> Deserialize<'de> for GoalTokenBudgetInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<i64>::deserialize(deserializer)? {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountRateLimitsReadArgument {
    cwd: std::path::PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoalGetResponse {
    goal: Option<NativeGoal>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoalSetResponse {
    goal: NativeGoal,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoalClearResponse {
    cleared: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeModelListResponse {
    data: Vec<NativeModelListEntry>,
    next_cursor: Option<String>,
}

#[derive(Clone)]
struct CodexModelCatalogEntry {
    model: String,
    display_name: String,
    description: Option<String>,
    reasoning_efforts: Vec<RuntimeControlChoice>,
    supports_personality: bool,
    service_tiers: Vec<RuntimeControlChoice>,
    is_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeModelListEntry {
    id: String,
    model: String,
    display_name: String,
    description: String,
    hidden: bool,
    supported_reasoning_efforts: Vec<NativeReasoningEffortOption>,
    default_reasoning_effort: String,
    #[serde(default)]
    supports_personality: bool,
    #[serde(default)]
    service_tiers: Vec<NativeModelServiceTier>,
    is_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeReasoningEffortOption {
    reasoning_effort: String,
    description: String,
}

#[derive(Deserialize)]
struct NativeModelServiceTier {
    id: String,
    name: String,
    description: String,
}

fn validated_codex_model_catalog_entry(
    model: NativeModelListEntry,
) -> Result<CodexModelCatalogEntry, RuntimeError> {
    if model.id.trim().is_empty() || model.default_reasoning_effort.trim().is_empty() {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex model/list returned an empty model id or default reasoning effort",
        ));
    }
    let mut seen_efforts = HashSet::new();
    let mut reasoning_efforts = Vec::with_capacity(model.supported_reasoning_efforts.len());
    for effort in model.supported_reasoning_efforts {
        if effort.reasoning_effort.trim().is_empty()
            || !seen_efforts.insert(effort.reasoning_effort.clone())
        {
            return Err(codex_auxiliary_protocol_mismatch(
                "Codex model/list returned an empty or duplicate reasoning effort",
            ));
        }
        reasoning_efforts.push(RuntimeControlChoice {
            label: codex_reasoning_effort_label(&effort.reasoning_effort),
            value: Value::String(effort.reasoning_effort),
            description: (!effort.description.trim().is_empty()).then_some(effort.description),
        });
    }
    let mut seen_service_tiers = HashSet::new();
    let mut service_tiers = Vec::with_capacity(model.service_tiers.len());
    for tier in model.service_tiers {
        if tier.id.trim().is_empty()
            || tier.name.trim().is_empty()
            || !seen_service_tiers.insert(tier.id.clone())
        {
            return Err(codex_auxiliary_protocol_mismatch(
                "Codex model/list returned an empty or duplicate service tier",
            ));
        }
        service_tiers.push(RuntimeControlChoice {
            value: Value::String(tier.id),
            label: tier.name,
            description: (!tier.description.trim().is_empty()).then_some(tier.description),
        });
    }
    Ok(CodexModelCatalogEntry {
        model: model.model,
        display_name: model.display_name,
        description: (!model.description.trim().is_empty()).then_some(model.description),
        reasoning_efforts,
        supports_personality: model.supports_personality,
        service_tiers,
        is_default: model.is_default,
    })
}

fn codex_reasoning_effort_label(value: &str) -> String {
    match value {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra high",
        "max" => "Max",
        "ultra" => "Ultra",
        value => value,
    }
    .to_string()
}

fn parse_extension_argument<T: DeserializeOwned>(
    argument: Option<Value>,
) -> Result<T, RuntimeError> {
    serde_json::from_value(argument.unwrap_or_else(|| json!({}))).map_err(|error| {
        invalid_extension_schema(&format!("Codex extension argument is invalid: {error}"))
    })
}

fn decode_extension_response<T: DeserializeOwned>(
    method: &str,
    response: Value,
) -> Result<T, RuntimeError> {
    serde_json::from_value(response).map_err(|error| {
        RuntimeError::new(
            "codex_extension_protocol_mismatch",
            RuntimeErrorStage::Transport,
            RetryClass::Never,
            format!("Codex {method} response did not match the stable schema: {error}"),
        )
    })
}

fn invalid_extension_schema(message: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_invalid_extension_schema",
        RuntimeErrorStage::Configuration,
        RetryClass::UserAction,
        message,
    )
}

fn validated_runtime_goal(
    native_session_id: &str,
    goal: NativeGoal,
) -> Result<RuntimeGoal, RuntimeError> {
    if goal.thread_id != native_session_id {
        return Err(RuntimeError::new(
            "codex_goal_thread_mismatch",
            RuntimeErrorStage::Binding,
            RetryClass::Never,
            "Codex goal response belonged to a different native thread",
        ));
    }
    if goal.token_budget.is_some_and(|value| value < 0)
        || goal.tokens_used < 0
        || goal.time_used_seconds < 0
        || goal.created_at < 0
        || goal.updated_at < goal.created_at
    {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex goal response contained invalid numeric or timestamp values",
        ));
    }
    Ok(goal.into())
}

fn codex_auxiliary_protocol_mismatch(message: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_extension_protocol_mismatch",
        RuntimeErrorStage::Transport,
        RetryClass::Never,
        message,
    )
}

fn native_goal_status(status: RuntimeGoalStatus) -> &'static str {
    match status {
        RuntimeGoalStatus::Active => "active",
        RuntimeGoalStatus::Paused => "paused",
        RuntimeGoalStatus::Blocked => "blocked",
        RuntimeGoalStatus::UsageLimited => "usageLimited",
        RuntimeGoalStatus::BudgetLimited => "budgetLimited",
        RuntimeGoalStatus::Complete => "complete",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoalUpdated {
    thread_id: String,
    turn_id: Option<String>,
    goal: NativeGoal,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoalCleared {
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeGoal {
    thread_id: String,
    objective: String,
    status: NativeGoalStatus,
    token_budget: Option<i64>,
    tokens_used: i64,
    time_used_seconds: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

impl From<NativeGoal> for RuntimeGoal {
    fn from(value: NativeGoal) -> Self {
        Self {
            objective: value.objective,
            status: match value.status {
                NativeGoalStatus::Active => RuntimeGoalStatus::Active,
                NativeGoalStatus::Paused => RuntimeGoalStatus::Paused,
                NativeGoalStatus::Blocked => RuntimeGoalStatus::Blocked,
                NativeGoalStatus::UsageLimited => RuntimeGoalStatus::UsageLimited,
                NativeGoalStatus::BudgetLimited => RuntimeGoalStatus::BudgetLimited,
                NativeGoalStatus::Complete => RuntimeGoalStatus::Complete,
            },
            token_budget: value.token_budget,
            tokens_used: value.tokens_used,
            time_used_seconds: value.time_used_seconds,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitsUpdated {
    rate_limits: NativeRateLimitSnapshot,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitsRead {
    rate_limits: NativeRateLimitSnapshot,
    rate_limits_by_limit_id: Option<HashMap<String, NativeRateLimitSnapshot>>,
    rate_limit_reset_credits: Option<NativeRateLimitResetCreditsSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitResetCreditsSummary {
    available_count: i64,
    credits: Option<Vec<NativeRateLimitResetCredit>>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitResetCredit {
    id: String,
    reset_type: NativeRateLimitResetType,
    status: NativeRateLimitResetCreditStatus,
    granted_at: i64,
    expires_at: Option<i64>,
    title: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeRateLimitResetType {
    CodexRateLimits,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeRateLimitResetCreditStatus {
    Available,
    Redeeming,
    Redeemed,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitSnapshot {
    limit_id: Option<String>,
    limit_name: Option<String>,
    primary: Option<NativeRateLimitWindow>,
    secondary: Option<NativeRateLimitWindow>,
    credits: Option<NativeCreditsSnapshot>,
    individual_limit: Option<NativeSpendControlLimitSnapshot>,
    plan_type: Option<String>,
    rate_limit_reached_type: Option<NativeRateLimitReachedType>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativeRateLimitReachedType {
    RateLimitReached,
    WorkspaceOwnerCreditsDepleted,
    WorkspaceMemberCreditsDepleted,
    WorkspaceOwnerUsageLimitReached,
    WorkspaceMemberUsageLimitReached,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeRateLimitWindow {
    used_percent: i32,
    window_duration_mins: Option<i64>,
    resets_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeCreditsSnapshot {
    has_credits: bool,
    unlimited: bool,
    balance: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeSpendControlLimitSnapshot {
    limit: String,
    used: String,
    remaining_percent: i32,
    resets_at: i64,
}

impl From<NativeRateLimitSnapshot> for RuntimeRateLimitSnapshot {
    fn from(value: NativeRateLimitSnapshot) -> Self {
        Self {
            limit_id: value.limit_id,
            limit_name: value.limit_name,
            primary: value.primary.map(Into::into),
            secondary: value.secondary.map(Into::into),
            credits: value.credits.map(Into::into),
            individual_limit: value.individual_limit.map(Into::into),
            plan_type: value.plan_type,
            rate_limit_reached_type: value.rate_limit_reached_type.map(|value| match value {
                NativeRateLimitReachedType::RateLimitReached => {
                    RuntimeRateLimitReachedType::RateLimitReached
                }
                NativeRateLimitReachedType::WorkspaceOwnerCreditsDepleted => {
                    RuntimeRateLimitReachedType::WorkspaceOwnerCreditsDepleted
                }
                NativeRateLimitReachedType::WorkspaceMemberCreditsDepleted => {
                    RuntimeRateLimitReachedType::WorkspaceMemberCreditsDepleted
                }
                NativeRateLimitReachedType::WorkspaceOwnerUsageLimitReached => {
                    RuntimeRateLimitReachedType::WorkspaceOwnerUsageLimitReached
                }
                NativeRateLimitReachedType::WorkspaceMemberUsageLimitReached => {
                    RuntimeRateLimitReachedType::WorkspaceMemberUsageLimitReached
                }
            }),
        }
    }
}

impl From<NativeRateLimitWindow> for RuntimeRateLimitWindow {
    fn from(value: NativeRateLimitWindow) -> Self {
        Self {
            used_percent: value.used_percent,
            window_duration_mins: value.window_duration_mins,
            resets_at: value.resets_at,
        }
    }
}

impl From<NativeCreditsSnapshot> for RuntimeCreditsSnapshot {
    fn from(value: NativeCreditsSnapshot) -> Self {
        Self {
            has_credits: value.has_credits,
            unlimited: value.unlimited,
            balance: value.balance,
        }
    }
}

impl From<NativeSpendControlLimitSnapshot> for RuntimeSpendControlLimitSnapshot {
    fn from(value: NativeSpendControlLimitSnapshot) -> Self {
        Self {
            limit: value.limit,
            used: value.used,
            remaining_percent: value.remaining_percent,
            resets_at: value.resets_at,
        }
    }
}

fn runtime_rate_limits(
    value: NativeRateLimitsRead,
) -> Result<RuntimeAccountRateLimits, RuntimeError> {
    let rate_limits = runtime_rate_limit_snapshot(value.rate_limits)?;
    let rate_limits_by_limit_id = value
        .rate_limits_by_limit_id
        .unwrap_or_default()
        .into_iter()
        .map(|(id, snapshot)| runtime_rate_limit_snapshot(snapshot).map(|snapshot| (id, snapshot)))
        .collect::<Result<_, _>>()?;
    let reset_credits_available = value
        .rate_limit_reset_credits
        .map(validated_reset_credits)
        .transpose()?;
    Ok(RuntimeAccountRateLimits {
        rate_limits,
        rate_limits_by_limit_id,
        reset_credits_available,
    })
}

fn runtime_rate_limit_snapshot(
    value: NativeRateLimitSnapshot,
) -> Result<RuntimeRateLimitSnapshot, RuntimeError> {
    validate_rate_limit_window(value.primary.as_ref())?;
    validate_rate_limit_window(value.secondary.as_ref())?;
    if let Some(credits) = value.credits.as_ref()
        && credits
            .balance
            .as_deref()
            .is_some_and(|balance| !is_non_negative_decimal(balance))
    {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex rate-limit credit balance was not a non-negative decimal",
        ));
    }
    if let Some(limit) = value.individual_limit.as_ref()
        && (!is_non_negative_decimal(&limit.limit)
            || !is_non_negative_decimal(&limit.used)
            || !(0..=100).contains(&limit.remaining_percent)
            || limit.resets_at < 0)
    {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex spend-control snapshot contained invalid numeric values",
        ));
    }
    Ok(value.into())
}

fn validate_rate_limit_window(window: Option<&NativeRateLimitWindow>) -> Result<(), RuntimeError> {
    if window.is_some_and(|window| {
        !(0..=100).contains(&window.used_percent)
            || window.window_duration_mins.is_some_and(|value| value < 0)
            || window.resets_at.is_some_and(|value| value < 0)
    }) {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex rate-limit window contained invalid percent, duration, or reset values",
        ));
    }
    Ok(())
}

fn validated_reset_credits(
    credits: NativeRateLimitResetCreditsSummary,
) -> Result<i64, RuntimeError> {
    if credits.available_count < 0 {
        return Err(codex_auxiliary_protocol_mismatch(
            "Codex reset-credit count was negative",
        ));
    }
    for credit in credits.credits.unwrap_or_default() {
        if matches!(credit.reset_type, NativeRateLimitResetType::Unknown)
            || matches!(credit.status, NativeRateLimitResetCreditStatus::Unknown)
            || credit.granted_at < 0
            || credit
                .expires_at
                .is_some_and(|expires_at| expires_at < 0 || expires_at < credit.granted_at)
        {
            return Err(codex_auxiliary_protocol_mismatch(
                "Codex reset-credit row contained unknown or invalid values",
            ));
        }
    }
    Ok(credits.available_count)
}

fn is_non_negative_decimal(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(whole) = parts.next() else {
        return false;
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(fraction), None) => {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

fn merge_sparse_rate_limit_update(
    cache: &mut Option<RuntimeAccountRateLimits>,
    incoming: RuntimeRateLimitSnapshot,
) {
    let key = incoming
        .limit_id
        .clone()
        .unwrap_or_else(|| "codex".to_string());
    let Some(cache) = cache.as_mut() else {
        let mut rate_limits_by_limit_id = BTreeMap::new();
        rate_limits_by_limit_id.insert(key, incoming.clone());
        *cache = Some(RuntimeAccountRateLimits {
            rate_limits: incoming,
            rate_limits_by_limit_id,
            reset_credits_available: None,
        });
        return;
    };
    let previous = cache
        .rate_limits_by_limit_id
        .get(&key)
        .cloned()
        .or_else(|| {
            (cache.rate_limits.limit_id.as_deref().unwrap_or("codex") == key)
                .then(|| cache.rate_limits.clone())
        })
        .unwrap_or(RuntimeRateLimitSnapshot {
            limit_id: None,
            limit_name: None,
            primary: None,
            secondary: None,
            credits: None,
            individual_limit: None,
            plan_type: None,
            rate_limit_reached_type: None,
        });
    let merged = RuntimeRateLimitSnapshot {
        limit_id: incoming.limit_id.or(previous.limit_id),
        limit_name: incoming.limit_name.or(previous.limit_name),
        primary: incoming.primary,
        secondary: incoming.secondary,
        credits: incoming.credits.or(previous.credits),
        individual_limit: incoming.individual_limit.or(previous.individual_limit),
        plan_type: incoming.plan_type.or(previous.plan_type),
        rate_limit_reached_type: incoming
            .rate_limit_reached_type
            .or(previous.rate_limit_reached_type),
    };
    cache.rate_limits = merged.clone();
    cache.rate_limits_by_limit_id.insert(key, merged);
}

fn require_native_session_id(request: &RuntimeSessionRequest) -> Result<&str, RuntimeError> {
    request
        .native_session_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "codex_missing_native_session",
                RuntimeErrorStage::Configuration,
                RetryClass::UserAction,
                "Codex session operation requires a native session id",
            )
        })
}

fn invalid_interaction(message: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_invalid_interaction_response",
        RuntimeErrorStage::Interaction,
        RetryClass::UserAction,
        message,
    )
}

fn unsupported(kind: &str, operation: &str) -> RuntimeError {
    RuntimeError::new(
        "unsupported",
        RuntimeErrorStage::Configuration,
        RetryClass::UserAction,
        format!("{kind} is unsupported: {operation}"),
    )
}

fn internal_error(message: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_internal_error",
        RuntimeErrorStage::Transport,
        RetryClass::Never,
        message,
    )
}

fn protocol_error(method: &str, message: &str) -> RuntimeError {
    RuntimeError::new(
        "codex_protocol_mismatch",
        RuntimeErrorStage::Transport,
        RetryClass::Never,
        format!("Codex {method} {message}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_after_hanging_first_codex_worker_still_starts_later_shutdowns() {
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (first_release_tx, first_release_rx) = oneshot::channel();
        let (first_completed_tx, first_completed_rx) = oneshot::channel();
        let first = Box::pin(async move {
            let _ = first_started_tx.send(());
            let _ = first_release_rx.await;
            let _ = first_completed_tx.send(());
            Ok(())
        }) as DetachedWorkerShutdown;

        let (second_started_tx, second_started_rx) = oneshot::channel();
        let (second_release_tx, second_release_rx) = oneshot::channel();
        let (second_completed_tx, second_completed_rx) = oneshot::channel();
        let second = Box::pin(async move {
            let _ = second_started_tx.send(());
            let _ = second_release_rx.await;
            let _ = second_completed_tx.send(());
            Ok(())
        }) as DetachedWorkerShutdown;

        let result = tokio::time::timeout(
            Duration::from_millis(20),
            run_worker_shutdowns_concurrently(vec![first, second]),
        )
        .await;
        assert!(
            result.is_err(),
            "the held worker shutdowns should cancel the outer wait"
        );
        tokio::time::timeout(Duration::from_secs(1), first_started_rx)
            .await
            .expect("the first worker shutdown should start")
            .expect("the first worker start sender should remain live");
        tokio::time::timeout(Duration::from_secs(1), second_started_rx)
            .await
            .expect("the second worker shutdown should start")
            .expect("the second worker start sender should remain live");

        let _ = first_release_tx.send(());
        let _ = second_release_tx.send(());
        tokio::time::timeout(Duration::from_secs(1), first_completed_rx)
            .await
            .expect("the first detached worker shutdown should complete")
            .expect("the first worker completion sender should remain live");
        tokio::time::timeout(Duration::from_secs(1), second_completed_rx)
            .await
            .expect("the second detached worker shutdown should complete")
            .expect("the second worker completion sender should remain live");
    }

    #[test]
    fn normalized_approvals_do_not_invent_plan_approval() {
        assert!(normalized_interaction("item/plan/requestApproval", &json!({})).is_none());
        let command = normalized_interaction(
            "item/commandExecution/requestApproval",
            &json!({"command": "cargo test"}),
        )
        .expect("command interaction");
        assert_eq!(command.kind, "command");
        assert_eq!(command.policy.kind, RuntimeInteractionKind::Permission);
        assert_eq!(command.policy.stability, RuntimeStability::Stable);
        assert_eq!(
            command.policy.exposure,
            RuntimeInteractionExposure::Standard
        );
    }

    #[test]
    fn question_answers_are_keyed_by_native_question_id() {
        let result = interaction_result(
            "item/tool/requestUserInput",
            json!({"answers": [["yes"]]}),
            &["confirm".to_string()],
            &[false],
            None,
        )
        .expect("answers");
        assert_eq!(result["answers"]["confirm"]["answers"], json!(["yes"]));
    }

    #[test]
    fn request_user_input_is_gui_advanced_only_and_cancel_unblocks_with_empty_answers() {
        let interaction = normalized_interaction(
            "item/tool/requestUserInput",
            &json!({
                "questions": [{
                    "id": "secret-question-id",
                    "header": "Choice",
                    "question": "native experimental question",
                    "options": []
                }]
            }),
        )
        .expect("question interaction");
        assert_eq!(interaction.policy.kind, RuntimeInteractionKind::Question);
        assert_eq!(interaction.policy.stability, RuntimeStability::Experimental);
        assert_eq!(
            interaction.policy.exposure,
            RuntimeInteractionExposure::GuiAdvancedOnly
        );

        let result = interaction_result(
            "item/tool/requestUserInput",
            json!({"reject": true, "decision": "cancel"}),
            &["secret-question-id".to_string()],
            &[false],
            None,
        )
        .expect("cancel response");
        assert_eq!(result, json!({"answers": {}}));
    }

    #[test]
    fn request_user_input_rejects_partial_answer_rows() {
        let error = interaction_result(
            "item/tool/requestUserInput",
            json!({"answers": [["first"]]}),
            &["first".to_string(), "second".to_string()],
            &[false, false],
            None,
        )
        .expect_err("partial answers must fail");
        assert_eq!(error.code, "codex_invalid_interaction_response");
    }

    #[test]
    fn request_user_input_enforces_single_and_multiple_answer_shapes() {
        let single_error = interaction_result(
            "item/tool/requestUserInput",
            json!({"answers": [["first", "second"]]}),
            &["single".to_string()],
            &[false],
            None,
        )
        .expect_err("single selection must contain one value");
        assert_eq!(single_error.code, "codex_invalid_interaction_response");

        let multiple = interaction_result(
            "item/tool/requestUserInput",
            json!({"answers": [["first", "second"]]}),
            &["multiple".to_string()],
            &[true],
            None,
        )
        .expect("multiple selection may contain several values");
        assert_eq!(
            multiple["answers"]["multiple"]["answers"],
            json!(["first", "second"])
        );

        interaction_result(
            "item/tool/requestUserInput",
            json!({"answers": [[]]}),
            &["multiple".to_string()],
            &[true],
            None,
        )
        .expect_err("multiple selection must contain at least one value");
    }

    #[test]
    fn request_user_input_preserves_native_auto_resolution_expiry() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_millis() as i64;
        let interaction = normalized_interaction(
            "item/tool/requestUserInput",
            &json!({
                "autoResolutionMs": 5_000,
                "questions": [{
                    "id": "expiring",
                    "header": "Choice",
                    "question": "Choose",
                    "options": []
                }]
            }),
        )
        .expect("question interaction");
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_millis() as i64;
        let expires_at_ms = interaction.expires_at_ms.expect("native expiry");
        assert!(expires_at_ms >= before.saturating_add(5_000));
        assert!(expires_at_ms <= after.saturating_add(5_000));
    }

    #[test]
    fn additional_permissions_echo_only_the_native_request_and_exact_scope() {
        let requested = json!({
            "network": {"enabled": true},
            "fileSystem": null
        });
        let accepted = interaction_result(
            "item/permissions/requestApproval",
            json!({"decision": "acceptForSession"}),
            &[],
            &[],
            Some(&requested),
        )
        .expect("permission approval");
        assert_eq!(accepted["permissions"], requested);
        assert_eq!(accepted["scope"], "session");

        let declined = interaction_result(
            "item/permissions/requestApproval",
            json!({"decision": "decline"}),
            &[],
            &[],
            Some(&requested),
        )
        .expect("permission decline");
        assert_eq!(declined["permissions"], json!({}));
        assert_eq!(declined["scope"], "turn");
    }

    #[test]
    fn goal_set_schema_distinguishes_missing_clear_and_set_token_budget() {
        let base = json!({
            "threadId": "gateway-thread",
            "nativeSessionId": "native-thread",
            "cwd": "/tmp",
            "objective": "Ship",
        });
        let missing: GoalSetArgument =
            serde_json::from_value(base.clone()).expect("missing token budget");
        assert_eq!(missing.token_budget, GoalTokenBudgetInput::Missing);

        let mut clear = base.clone();
        clear["tokenBudget"] = Value::Null;
        let clear: GoalSetArgument = serde_json::from_value(clear).expect("clear token budget");
        assert_eq!(clear.token_budget, GoalTokenBudgetInput::Clear);

        let mut set = base.clone();
        set["tokenBudget"] = json!(1_000);
        let set: GoalSetArgument = serde_json::from_value(set).expect("set token budget");
        assert_eq!(set.token_budget, GoalTokenBudgetInput::Set(1_000));

        let mut unknown = base;
        unknown["unexpected"] = json!(true);
        assert!(serde_json::from_value::<GoalSetArgument>(unknown).is_err());
    }

    #[test]
    fn codex_version_manifest_requires_a_parseable_compatible_identity() {
        assert_eq!(
            codex_user_agent_version("codex_cli_rs/0.143.0-alpha.10 (Linux; x86_64) rust"),
            Some((0, 143, 0))
        );
        assert_eq!(
            codex_user_agent_version("daemon/1.0 codex_cli_rs/1.2.3"),
            Some((1, 2, 3))
        );
        assert_eq!(codex_user_agent_version("codex-cli/0.143.0"), None);
        assert_eq!(codex_user_agent_version("codex_cli_rs/not-a-version"), None);
    }

    #[test]
    fn malformed_goal_and_rate_limit_numbers_fail_closed() {
        let goal: NativeGoal = serde_json::from_value(json!({
            "threadId": "native-1",
            "objective": "Ship",
            "status": "active",
            "tokenBudget": 100,
            "tokensUsed": -1,
            "timeUsedSeconds": 1,
            "createdAt": 10,
            "updatedAt": 11,
        }))
        .expect("native goal schema");
        assert_eq!(
            validated_runtime_goal("native-1", goal)
                .expect_err("negative goal usage")
                .code,
            "codex_extension_protocol_mismatch"
        );

        let rate_limits: NativeRateLimitsRead = serde_json::from_value(json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 101, "windowDurationMins": 5, "resetsAt": 10 }
            },
            "rateLimitsByLimitId": null,
            "rateLimitResetCredits": null
        }))
        .expect("native rate-limit schema");
        assert_eq!(
            runtime_rate_limits(rate_limits)
                .expect_err("out-of-range percent")
                .code,
            "codex_extension_protocol_mismatch"
        );

        let unknown_credit: NativeRateLimitsRead = serde_json::from_value(json!({
            "rateLimits": {},
            "rateLimitsByLimitId": null,
            "rateLimitResetCredits": {
                "availableCount": 1,
                "credits": [{
                    "id": "credit",
                    "resetType": "futureType",
                    "status": "futureStatus",
                    "grantedAt": 1,
                    "expiresAt": 2,
                    "title": null,
                    "description": null
                }]
            }
        }))
        .expect("unknown credit schema");
        assert_eq!(
            runtime_rate_limits(unknown_credit)
                .expect_err("unknown reset credit")
                .code,
            "codex_extension_protocol_mismatch"
        );
    }

    #[test]
    fn sparse_rate_limit_update_replaces_windows_and_preserves_full_read_metadata() {
        let previous = RuntimeRateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: Some("Codex".to_string()),
            primary: Some(RuntimeRateLimitWindow {
                used_percent: 10,
                window_duration_mins: Some(300),
                resets_at: Some(1_000),
            }),
            secondary: Some(RuntimeRateLimitWindow {
                used_percent: 20,
                window_duration_mins: Some(10_080),
                resets_at: Some(2_000),
            }),
            credits: Some(RuntimeCreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some("12.50".to_string()),
            }),
            individual_limit: Some(RuntimeSpendControlLimitSnapshot {
                limit: "100".to_string(),
                used: "20".to_string(),
                remaining_percent: 80,
                resets_at: 3_000,
            }),
            plan_type: Some("pro".to_string()),
            rate_limit_reached_type: Some(RuntimeRateLimitReachedType::RateLimitReached),
        };
        let mut cache = Some(RuntimeAccountRateLimits {
            rate_limits: previous.clone(),
            rate_limits_by_limit_id: BTreeMap::from([("codex".to_string(), previous)]),
            reset_credits_available: Some(2),
        });
        merge_sparse_rate_limit_update(
            &mut cache,
            RuntimeRateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: None,
                primary: Some(RuntimeRateLimitWindow {
                    used_percent: 42,
                    window_duration_mins: Some(300),
                    resets_at: Some(1_100),
                }),
                secondary: None,
                credits: None,
                individual_limit: None,
                plan_type: None,
                rate_limit_reached_type: None,
            },
        );
        let cache = cache.expect("merged cache");
        assert_eq!(cache.rate_limits.limit_name.as_deref(), Some("Codex"));
        assert_eq!(
            cache
                .rate_limits
                .primary
                .as_ref()
                .map(|window| window.used_percent),
            Some(42)
        );
        assert!(cache.rate_limits.secondary.is_none());
        assert_eq!(
            cache
                .rate_limits
                .credits
                .as_ref()
                .and_then(|credits| credits.balance.as_deref()),
            Some("12.50")
        );
        assert_eq!(cache.rate_limits.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            cache.rate_limits.rate_limit_reached_type,
            Some(RuntimeRateLimitReachedType::RateLimitReached)
        );
        assert_eq!(cache.reset_credits_available, Some(2));
    }
}
