use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use crate::{
    ExecuteRequest, ExecuteResult, RetryClass, RuntimeControl, RuntimeError, RuntimeErrorStage,
    RuntimeFuture, RuntimeKind, RuntimeModule, RuntimeObserver, RuntimeSnapshot, ShutdownMode,
    SnapshotQuery,
};

#[derive(Clone, Default)]
pub struct RuntimeHost {
    modules: Arc<RwLock<BTreeMap<RuntimeKind, Arc<dyn RuntimeModule>>>>,
}

impl RuntimeHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, kind: RuntimeKind, module: Arc<dyn RuntimeModule>) {
        self.modules
            .write()
            .expect("runtime host registry poisoned")
            .insert(kind, module);
    }

    pub fn contains(&self, kind: RuntimeKind) -> bool {
        self.modules
            .read()
            .expect("runtime host registry poisoned")
            .contains_key(&kind)
    }

    fn module(&self, kind: RuntimeKind) -> Result<Arc<dyn RuntimeModule>, RuntimeError> {
        self.modules
            .read()
            .expect("runtime host registry poisoned")
            .get(&kind)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    "unsupported",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    format!("runtime adapter is unavailable: {kind:?}"),
                )
            })
    }
}

impl fmt::Debug for RuntimeHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kinds = self
            .modules
            .read()
            .expect("runtime host registry poisoned")
            .keys()
            .copied()
            .collect::<Vec<_>>();
        formatter
            .debug_struct("RuntimeHost")
            .field("kinds", &kinds)
            .finish()
    }
}

impl RuntimeModule for RuntimeHost {
    fn snapshot(&self, query: SnapshotQuery) -> RuntimeFuture<RuntimeSnapshot> {
        match self.module(query.profile.kind) {
            Ok(module) => module.snapshot(query),
            Err(error) => Box::pin(async move { Err(error) }),
        }
    }

    fn execute(
        &self,
        request: ExecuteRequest,
        observer: RuntimeObserver,
        control: RuntimeControl,
    ) -> RuntimeFuture<ExecuteResult> {
        match self.module(request.profile.kind) {
            Ok(module) => module.execute(request, observer, control),
            Err(error) => Box::pin(async move { Err(error) }),
        }
    }

    fn shutdown(&self, mode: ShutdownMode) -> RuntimeFuture<()> {
        let modules = self
            .modules
            .read()
            .expect("runtime host registry poisoned")
            .clone();
        Box::pin(async move {
            match &mode {
                ShutdownMode::Runtime { kind, .. } => {
                    if let Some(module) = modules.get(kind) {
                        module.shutdown(mode).await?;
                    }
                }
                ShutdownMode::Graceful | ShutdownMode::Force => {
                    let mut first_error = None;
                    for module in modules.values() {
                        if let Err(error) = module.shutdown(mode.clone()).await
                            && first_error.is_none()
                        {
                            first_error = Some(error);
                        }
                    }
                    if let Some(error) = first_error {
                        return Err(error);
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::{
        HistoryFidelity, RuntimeCapability, RuntimeExtensionRequest, RuntimeIntent,
        RuntimeInteractionExposure, RuntimeObservation, RuntimeProfile, RuntimeSessionBinding,
        RuntimeStability, RuntimeTurnOutcome, RuntimeTurnRequest, RuntimeTurnResult, ShutdownMode,
        SnapshotMode, SnapshotScope,
    };

    const RUNTIME_KINDS: [RuntimeKind; 4] = [
        RuntimeKind::Native,
        RuntimeKind::Acp,
        RuntimeKind::Codex,
        RuntimeKind::OpenCode,
    ];
    const CURRENT_PROCESS_EPOCH: u64 = 7;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeTurnBehavior {
        Complete,
        UnknownDelivery,
        EpochRace,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeSafetyDisposition {
        Exact,
        Narrowed,
    }

    impl FakeSafetyDisposition {
        fn as_str(self) -> &'static str {
            match self {
                Self::Exact => "exact",
                Self::Narrowed => "narrowed",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeTrace {
        Snapshot(SnapshotMode),
        BindRequested,
        BindAcknowledged,
        PromptDelivered,
        NativeEventObserved { process_epoch: u64 },
        EventProjected { process_epoch: u64 },
        TerminalProjected { process_epoch: u64 },
        Shutdown,
    }

    #[derive(Debug, Default)]
    struct FakeRuntimeState {
        snapshots: AtomicUsize,
        worker_launches: AtomicUsize,
        executions: AtomicUsize,
        prompt_deliveries: AtomicUsize,
        terminals: AtomicUsize,
        shutdowns: AtomicUsize,
        disposals: AtomicUsize,
        disposed: AtomicBool,
        trace: Mutex<Vec<FakeTrace>>,
    }

    impl FakeRuntimeState {
        fn record(&self, item: FakeTrace) {
            self.trace
                .lock()
                .expect("fake runtime trace poisoned")
                .push(item);
        }

        fn trace(&self) -> Vec<FakeTrace> {
            self.trace
                .lock()
                .expect("fake runtime trace poisoned")
                .clone()
        }
    }

    #[derive(Debug)]
    struct FakeRuntimeModule {
        kind: RuntimeKind,
        behavior: FakeTurnBehavior,
        state: Arc<FakeRuntimeState>,
        shutdown_error: bool,
    }

    impl FakeRuntimeModule {
        fn new(kind: RuntimeKind) -> Self {
            Self::with_behavior(kind, FakeTurnBehavior::Complete)
        }

        fn with_behavior(kind: RuntimeKind, behavior: FakeTurnBehavior) -> Self {
            Self {
                kind,
                behavior,
                state: Arc::new(FakeRuntimeState::default()),
                shutdown_error: false,
            }
        }

        fn with_shutdown_error(mut self) -> Self {
            self.shutdown_error = true;
            self
        }

        fn count(&self, counter: &AtomicUsize) -> usize {
            counter.load(Ordering::SeqCst)
        }

        fn observer(&self, observations: Arc<Mutex<Vec<RuntimeObservation>>>) -> RuntimeObserver {
            let binding_state = Arc::clone(&self.state);
            let expected_kind = self.kind;
            RuntimeObserver::new(move |observation| {
                observations
                    .lock()
                    .expect("fake observations poisoned")
                    .push(observation);
            })
            .with_session_binder(move |binding| {
                let binding_state = Arc::clone(&binding_state);
                async move {
                    assert_eq!(binding.runtime_ref, profile(expected_kind).id);
                    assert_eq!(binding.process_epoch, CURRENT_PROCESS_EPOCH);
                    binding_state.record(FakeTrace::BindAcknowledged);
                    Ok(())
                }
            })
        }
    }

    impl RuntimeModule for FakeRuntimeModule {
        fn snapshot(&self, query: SnapshotQuery) -> RuntimeFuture<RuntimeSnapshot> {
            assert_eq!(query.profile.kind, self.kind);
            self.state.snapshots.fetch_add(1, Ordering::SeqCst);
            self.state.record(FakeTrace::Snapshot(query.mode));
            let process_epoch = match query.mode {
                SnapshotMode::Cached => None,
                SnapshotMode::BoundedProbe | SnapshotMode::CatalogRefresh => {
                    self.state.worker_launches.fetch_add(1, Ordering::SeqCst);
                    Some(CURRENT_PROCESS_EPOCH)
                }
            };
            let kind = self.kind;
            let runtime_ref = query.profile.id;
            Box::pin(async move {
                Ok(RuntimeSnapshot {
                    runtime_ref,
                    kind,
                    profile_revision: 1,
                    capability_revision: 1,
                    adapter_version: "fake".to_string(),
                    runtime_version: None,
                    stability: RuntimeStability::Stable,
                    provenance: "conformance".to_string(),
                    readiness: Vec::new(),
                    controls: Vec::new(),
                    capabilities: vec![RuntimeCapability {
                        id: "execute".to_string(),
                        enabled: true,
                        stability: RuntimeStability::Stable,
                    }],
                    process_epoch,
                    instance_epoch: None,
                    binding_epoch: None,
                    extension: None,
                })
            })
        }

        fn execute(
            &self,
            request: ExecuteRequest,
            observer: RuntimeObserver,
            _control: RuntimeControl,
        ) -> RuntimeFuture<ExecuteResult> {
            assert_eq!(request.profile.kind, self.kind);
            self.state.executions.fetch_add(1, Ordering::SeqCst);
            let kind = self.kind;
            let behavior = self.behavior;
            let state = Arc::clone(&self.state);
            Box::pin(async move {
                if request.expected_profile_revision != request.profile.revision
                    || request
                        .expected_capability_revision
                        .is_some_and(|revision| revision != request.profile.revision)
                {
                    return Err(RuntimeError::new(
                        "stale_revision",
                        RuntimeErrorStage::Configuration,
                        RetryClass::UserAction,
                        "fake runtime profile revision changed before execution",
                    ));
                }
                match request.intent {
                    RuntimeIntent::Extension(_) => {
                        Ok(ExecuteResult::Extension(json!({"ok": true})))
                    }
                    RuntimeIntent::Turn(turn) => {
                        execute_fake_turn(kind, request.profile, turn, behavior, state, observer)
                            .await
                    }
                    _ => Err(RuntimeError::new(
                        "unsupported",
                        RuntimeErrorStage::Configuration,
                        RetryClass::UserAction,
                        "fake conformance runtime only implements Turn and Extension",
                    )),
                }
            })
        }

        fn shutdown(&self, _mode: ShutdownMode) -> RuntimeFuture<()> {
            self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
            self.state.record(FakeTrace::Shutdown);
            let kind = self.kind;
            let shutdown_error = self.shutdown_error;
            let state = Arc::clone(&self.state);
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(1)).await;
                if !state.disposed.swap(true, Ordering::SeqCst) {
                    state.disposals.fetch_add(1, Ordering::SeqCst);
                }
                if shutdown_error {
                    Err(RuntimeError::new(
                        "fake_shutdown_failed",
                        RuntimeErrorStage::Shutdown,
                        RetryClass::SafeRetry,
                        format!("{kind:?} fake shutdown failed"),
                    ))
                } else {
                    Ok(())
                }
            })
        }
    }

    async fn execute_fake_turn(
        kind: RuntimeKind,
        runtime_profile: RuntimeProfile,
        turn: RuntimeTurnRequest,
        behavior: FakeTurnBehavior,
        state: Arc<FakeRuntimeState>,
        observer: RuntimeObserver,
    ) -> Result<ExecuteResult, RuntimeError> {
        // The common fake advertises workspace-write as its maximum authority.
        // Adapter-specific safety support differs; this deliberately exercises
        // only the shared exact/narrowed/unsupported contract.
        let safety = match runtime_profile.sandbox.as_deref() {
            None | Some("workspace-write") => FakeSafetyDisposition::Exact,
            Some("read-only") => FakeSafetyDisposition::Narrowed,
            Some(_) => {
                return Err(RuntimeError::new(
                    "policy_not_enforceable",
                    RuntimeErrorStage::Configuration,
                    RetryClass::UserAction,
                    "fake runtime cannot enforce the requested authority without expansion",
                ));
            }
        };
        let native_session_id = format!("fake-{kind:?}-session").to_ascii_lowercase();
        state.record(FakeTrace::BindRequested);
        observer
            .bind_native_session(RuntimeSessionBinding {
                runtime_ref: runtime_profile.id,
                thread_id: turn.thread_id.clone(),
                native_session_id: native_session_id.clone(),
                cwd: turn.cwd,
                binding_epoch: turn.binding_epoch,
                process_epoch: CURRENT_PROCESS_EPOCH,
                instance_epoch: Some(1),
            })
            .await?;
        state.prompt_deliveries.fetch_add(1, Ordering::SeqCst);
        state.record(FakeTrace::PromptDelivered);
        if behavior == FakeTurnBehavior::UnknownDelivery {
            return Err(RuntimeError::new(
                "delivery_unknown",
                RuntimeErrorStage::Prompt,
                RetryClass::UnknownDelivery,
                "fake prompt delivery could not be confirmed",
            ));
        }

        let event_candidates = match behavior {
            FakeTurnBehavior::EpochRace => vec![
                (CURRENT_PROCESS_EPOCH - 1, "stale"),
                (CURRENT_PROCESS_EPOCH, "current"),
            ],
            FakeTurnBehavior::Complete => vec![(CURRENT_PROCESS_EPOCH, "current")],
            FakeTurnBehavior::UnknownDelivery => unreachable!("returned above"),
        };
        for (process_epoch, text) in event_candidates {
            state.record(FakeTrace::NativeEventObserved { process_epoch });
            if process_epoch == CURRENT_PROCESS_EPOCH {
                observer.emit(RuntimeObservation::TextDelta {
                    turn_id: turn.turn_id.clone(),
                    text: text.to_string(),
                });
                state.record(FakeTrace::EventProjected { process_epoch });
            }
        }

        let mut terminal_projected = false;
        for process_epoch in [
            CURRENT_PROCESS_EPOCH - 1,
            CURRENT_PROCESS_EPOCH,
            CURRENT_PROCESS_EPOCH,
        ] {
            state.record(FakeTrace::NativeEventObserved { process_epoch });
            if process_epoch == CURRENT_PROCESS_EPOCH && !terminal_projected {
                terminal_projected = true;
                state.terminals.fetch_add(1, Ordering::SeqCst);
                state.record(FakeTrace::TerminalProjected { process_epoch });
            }
        }
        Ok(ExecuteResult::Turn(RuntimeTurnResult {
            turn_id: turn.turn_id,
            thread_id: turn.thread_id,
            native_session_id,
            outcome: RuntimeTurnOutcome::Completed,
            final_answer: "current".to_string(),
            provider: "fake".to_string(),
            model: "fake-model".to_string(),
            history_fidelity: HistoryFidelity::Full,
            process_epoch: CURRENT_PROCESS_EPOCH,
            instance_epoch: Some(1),
            terminal_error: None,
            metadata: Some(json!({"safetyPolicy": safety.as_str()})),
        }))
    }

    fn profile(kind: RuntimeKind) -> RuntimeProfile {
        RuntimeProfile {
            id: format!("{kind:?}").to_ascii_lowercase(),
            label: format!("{kind:?}"),
            kind,
            enabled: true,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            backend_ref: None,
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
            revision: 1,
            fingerprint: format!("fingerprint-{kind:?}"),
        }
    }

    fn turn_request(runtime_profile: RuntimeProfile, suffix: &str) -> ExecuteRequest {
        ExecuteRequest {
            expected_profile_revision: runtime_profile.revision,
            expected_capability_revision: Some(runtime_profile.revision),
            expected_binding_revision: Some(1),
            intent: RuntimeIntent::Turn(RuntimeTurnRequest {
                turn_id: format!("turn-{suffix}"),
                thread_id: format!("thread-{suffix}"),
                native_session_id: None,
                cwd: PathBuf::from("/workspace"),
                prompt: "conformance prompt".to_string(),
                instructions: None,
                model: None,
                mode: None,
                agent: None,
                features: BTreeMap::new(),
                interaction_exposure: RuntimeInteractionExposure::Standard,
                binding_epoch: 1,
            }),
            profile: runtime_profile,
        }
    }

    fn registered_host(
        behavior: FakeTurnBehavior,
    ) -> (RuntimeHost, Vec<(RuntimeKind, Arc<FakeRuntimeModule>)>) {
        let host = RuntimeHost::new();
        let modules = RUNTIME_KINDS
            .into_iter()
            .map(|kind| {
                let module = Arc::new(FakeRuntimeModule::with_behavior(kind, behavior));
                host.register(kind, module.clone());
                (kind, module)
            })
            .collect();
        (host, modules)
    }

    fn turn_result(result: ExecuteResult) -> RuntimeTurnResult {
        let ExecuteResult::Turn(result) = result else {
            panic!("expected fake turn result");
        };
        result
    }

    #[tokio::test]
    async fn host_dispatches_the_same_three_method_contract_for_every_runtime_kind() {
        let (host, modules) = registered_host(FakeTurnBehavior::Complete);

        for (kind, _) in &modules {
            let runtime_profile = profile(*kind);
            let snapshot = host
                .snapshot(SnapshotQuery {
                    profile: runtime_profile.clone(),
                    scope: SnapshotScope::Workspace {
                        cwd: PathBuf::from("/workspace"),
                    },
                    mode: SnapshotMode::Cached,
                })
                .await
                .expect("snapshot");
            assert_eq!(snapshot.kind, *kind);
            let result = host
                .execute(
                    ExecuteRequest {
                        profile: runtime_profile,
                        expected_profile_revision: 1,
                        expected_capability_revision: Some(1),
                        expected_binding_revision: None,
                        intent: RuntimeIntent::Extension(RuntimeExtensionRequest {
                            namespace: "test.conformance".to_string(),
                            operation: "ping".to_string(),
                            argument: None,
                        }),
                    },
                    RuntimeObserver::default(),
                    RuntimeControl::default(),
                )
                .await
                .expect("execute");
            assert_eq!(result, ExecuteResult::Extension(json!({"ok": true})));
        }
        host.shutdown(ShutdownMode::Graceful)
            .await
            .expect("shutdown");
        for (_, module) in modules {
            assert_eq!(module.count(&module.state.snapshots), 1);
            assert_eq!(module.count(&module.state.executions), 1);
            assert_eq!(module.count(&module.state.shutdowns), 1);
        }
    }

    #[tokio::test]
    async fn shared_conformance_cached_snapshot_is_no_spawn_for_every_runtime_kind() {
        let (host, modules) = registered_host(FakeTurnBehavior::Complete);

        for kind in RUNTIME_KINDS {
            let cached = host
                .snapshot(SnapshotQuery {
                    profile: profile(kind),
                    scope: SnapshotScope::Workspace {
                        cwd: PathBuf::from("/workspace"),
                    },
                    mode: SnapshotMode::Cached,
                })
                .await
                .expect("cached snapshot");
            assert_eq!(cached.process_epoch, None);
        }
        for (_, module) in &modules {
            assert_eq!(module.count(&module.state.snapshots), 1);
            assert_eq!(module.count(&module.state.worker_launches), 0);
            assert_eq!(
                module.state.trace(),
                vec![FakeTrace::Snapshot(SnapshotMode::Cached)]
            );
        }

        for kind in RUNTIME_KINDS {
            let probed = host
                .snapshot(SnapshotQuery {
                    profile: profile(kind),
                    scope: SnapshotScope::Workspace {
                        cwd: PathBuf::from("/workspace"),
                    },
                    mode: SnapshotMode::BoundedProbe,
                })
                .await
                .expect("bounded snapshot probe");
            assert_eq!(probed.process_epoch, Some(CURRENT_PROCESS_EPOCH));
        }
        for (_, module) in modules {
            assert_eq!(module.count(&module.state.worker_launches), 1);
        }
    }

    #[tokio::test]
    async fn shared_conformance_binding_precedes_prompt_and_terminal_is_exactly_once() {
        let (host, modules) = registered_host(FakeTurnBehavior::Complete);

        for (kind, module) in &modules {
            let observations = Arc::new(Mutex::new(Vec::new()));
            let result = host
                .execute(
                    turn_request(profile(*kind), &format!("{kind:?}")),
                    module.observer(observations),
                    RuntimeControl::default(),
                )
                .await
                .expect("completed fake turn");
            assert_eq!(turn_result(result).outcome, RuntimeTurnOutcome::Completed);
            let trace = module.state.trace();
            let bind = trace
                .iter()
                .position(|item| *item == FakeTrace::BindAcknowledged)
                .expect("binding acknowledgement trace");
            let prompt = trace
                .iter()
                .position(|item| *item == FakeTrace::PromptDelivered)
                .expect("prompt trace");
            assert!(bind < prompt, "{kind:?} prompt preceded binding: {trace:?}");
            assert_eq!(module.count(&module.state.prompt_deliveries), 1);
            assert_eq!(module.count(&module.state.terminals), 1);

            let prompt_count = module.count(&module.state.prompt_deliveries);
            let error = host
                .execute(
                    turn_request(profile(*kind), &format!("{kind:?}-bind-failure")),
                    RuntimeObserver::default().with_session_binder(|_| async {
                        Err(RuntimeError::new(
                            "binding_rejected",
                            RuntimeErrorStage::Binding,
                            RetryClass::Never,
                            "fake binding was not persisted",
                        ))
                    }),
                    RuntimeControl::default(),
                )
                .await
                .expect_err("binding failure must stop delivery");
            assert_eq!(error.code, "binding_rejected");
            assert_eq!(module.count(&module.state.prompt_deliveries), prompt_count);
            assert_eq!(module.count(&module.state.terminals), 1);
        }
    }

    #[tokio::test]
    async fn shared_conformance_unknown_delivery_is_never_retried_or_fallen_back() {
        for target_kind in RUNTIME_KINDS {
            let (host, modules) = registered_host(FakeTurnBehavior::UnknownDelivery);
            let target = modules
                .iter()
                .find(|(kind, _)| *kind == target_kind)
                .map(|(_, module)| module)
                .expect("target fake module");
            let error = host
                .execute(
                    turn_request(profile(target_kind), "unknown-delivery"),
                    target.observer(Arc::new(Mutex::new(Vec::new()))),
                    RuntimeControl::default(),
                )
                .await
                .expect_err("unknown delivery");
            assert_eq!(error.retry_class, RetryClass::UnknownDelivery);

            for (kind, module) in modules {
                let expected = usize::from(kind == target_kind);
                assert_eq!(module.count(&module.state.executions), expected);
                assert_eq!(module.count(&module.state.prompt_deliveries), expected);
                assert_eq!(module.count(&module.state.terminals), 0);
            }
        }
    }

    #[tokio::test]
    async fn shared_conformance_shutdown_is_bounded_and_idempotent() {
        let (host, modules) = registered_host(FakeTurnBehavior::Complete);

        tokio::time::timeout(
            Duration::from_millis(250),
            host.shutdown(ShutdownMode::Graceful),
        )
        .await
        .expect("graceful shutdown exceeded conformance bound")
        .expect("graceful shutdown");
        tokio::time::timeout(
            Duration::from_millis(250),
            host.shutdown(ShutdownMode::Force),
        )
        .await
        .expect("forced shutdown exceeded conformance bound")
        .expect("forced shutdown");

        for (_, module) in modules {
            assert_eq!(module.count(&module.state.shutdowns), 2);
            assert_eq!(module.count(&module.state.disposals), 1);
        }
    }

    #[tokio::test]
    async fn shared_conformance_stale_process_epochs_cannot_mutate_current_state() {
        let (host, modules) = registered_host(FakeTurnBehavior::EpochRace);

        for (kind, module) in modules {
            let observations = Arc::new(Mutex::new(Vec::new()));
            let result = turn_result(
                host.execute(
                    turn_request(profile(kind), &format!("{kind:?}-epoch")),
                    module.observer(Arc::clone(&observations)),
                    RuntimeControl::default(),
                )
                .await
                .expect("epoch-isolated turn"),
            );
            assert_eq!(result.process_epoch, CURRENT_PROCESS_EPOCH);
            assert_eq!(result.final_answer, "current");
            assert_eq!(module.count(&module.state.terminals), 1);
            assert!(
                module
                    .state
                    .trace()
                    .contains(&FakeTrace::NativeEventObserved {
                        process_epoch: CURRENT_PROCESS_EPOCH - 1,
                    })
            );
            let texts = observations
                .lock()
                .expect("fake observations poisoned")
                .iter()
                .filter_map(|observation| match observation {
                    RuntimeObservation::TextDelta { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(texts, vec!["current"]);
        }
    }

    #[tokio::test]
    async fn shared_conformance_safety_is_exact_narrowed_or_fail_closed() {
        let (host, modules) = registered_host(FakeTurnBehavior::Complete);

        for (kind, module) in modules {
            let mut exact = profile(kind);
            exact.sandbox = Some("workspace-write".to_string());
            let result = turn_result(
                host.execute(
                    turn_request(exact, &format!("{kind:?}-exact")),
                    module.observer(Arc::new(Mutex::new(Vec::new()))),
                    RuntimeControl::default(),
                )
                .await
                .expect("exact fake safety policy"),
            );
            assert_eq!(result.metadata, Some(json!({"safetyPolicy": "exact"})));

            let mut narrowed = profile(kind);
            narrowed.sandbox = Some("read-only".to_string());
            let result = turn_result(
                host.execute(
                    turn_request(narrowed, &format!("{kind:?}-narrowed")),
                    module.observer(Arc::new(Mutex::new(Vec::new()))),
                    RuntimeControl::default(),
                )
                .await
                .expect("narrowed fake safety policy"),
            );
            assert_eq!(result.metadata, Some(json!({"safetyPolicy": "narrowed"})));

            let mut expanded = profile(kind);
            expanded.sandbox = Some("danger-full-access".to_string());
            let prompt_count = module.count(&module.state.prompt_deliveries);
            let error = host
                .execute(
                    turn_request(expanded, &format!("{kind:?}-expanded")),
                    module.observer(Arc::new(Mutex::new(Vec::new()))),
                    RuntimeControl::default(),
                )
                .await
                .expect_err("unsupported authority must fail closed");
            assert_eq!(error.code, "policy_not_enforceable");
            assert_eq!(error.stage, RuntimeErrorStage::Configuration);
            assert_eq!(module.count(&module.state.prompt_deliveries), prompt_count);
        }
    }

    #[tokio::test]
    async fn global_shutdown_attempts_every_module_before_returning_the_first_error() {
        let host = RuntimeHost::new();
        let first = Arc::new(FakeRuntimeModule::new(RuntimeKind::Native).with_shutdown_error());
        let later = Arc::new(FakeRuntimeModule::new(RuntimeKind::OpenCode));
        host.register(RuntimeKind::Native, first.clone());
        host.register(RuntimeKind::OpenCode, later.clone());

        let error = host
            .shutdown(ShutdownMode::Graceful)
            .await
            .expect_err("first shutdown error is returned after all attempts");
        assert_eq!(error.code, "fake_shutdown_failed");
        assert_eq!(first.count(&first.state.shutdowns), 1);
        assert_eq!(later.count(&later.state.shutdowns), 1);
    }
}
