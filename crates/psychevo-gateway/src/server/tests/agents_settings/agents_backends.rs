#[derive(Debug)]
struct SnapshotCountingRuntime {
    snapshots: Arc<std::sync::atomic::AtomicUsize>,
}

const ABOVE_JS_SAFE_RUNTIME_REVISION: u64 = 9_007_199_254_740_993;

fn fake_snapshot_control(
    id: &str,
    state: psychevo_runtime_host::ControlState,
    current_value: Option<Value>,
    choices: Vec<Value>,
) -> psychevo_runtime_host::RuntimeControlDescriptor {
    psychevo_runtime_host::RuntimeControlDescriptor {
        id: id.to_string(),
        label: id.to_string(),
        state,
        current_value,
        choices: choices
            .into_iter()
            .map(|value| psychevo_runtime_host::RuntimeControlChoice {
                label: value.to_string(),
                value,
                description: None,
            })
            .collect(),
        depends_on: matches!(id, "effort" | "personality" | "serviceTier").then(|| {
            psychevo_runtime_host::RuntimeControlDependency {
                control_id: "model".to_string(),
                value: json!("gpt-fixture"),
            }
        }),
        channel_safe: true,
        capability_revision: ABOVE_JS_SAFE_RUNTIME_REVISION,
    }
}

fn bound_runtime_profile_fixture(runtime_ref: &str) -> (String, String, String) {
    let profile = super::runtime_profiles::generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == runtime_ref)
        .expect("generated runtime profile fixture");
    let fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let revision = crate::runtime_profile_config_revision(&fingerprint).to_string();
    let encoded = serde_json::to_string(&profile).expect("serialize runtime profile fixture");
    (encoded, fingerprint, revision)
}

impl psychevo_runtime_host::RuntimeModule for SnapshotCountingRuntime {
    fn snapshot(
        &self,
        query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        self.snapshots
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            Ok(psychevo_runtime_host::RuntimeSnapshot {
                runtime_ref: query.profile.id,
                kind: query.profile.kind,
                profile_revision: query.profile.revision,
                capability_revision: ABOVE_JS_SAFE_RUNTIME_REVISION,
                adapter_version: "fake-snapshot-v1".to_string(),
                runtime_version: Some("fake-native-v2".to_string()),
                stability: psychevo_runtime_host::RuntimeStability::Stable,
                provenance: "fake-cache".to_string(),
                readiness: vec![
                    psychevo_runtime_host::ReadinessStage {
                        id: "configuration".to_string(),
                        status: psychevo_runtime_host::ReadinessStatus::Ready,
                        summary: "fake profile configured".to_string(),
                        observed_at_ms: None,
                    },
                    psychevo_runtime_host::ReadinessStage {
                        id: "authentication".to_string(),
                        status: psychevo_runtime_host::ReadinessStatus::NeedsAuth,
                        summary: "fake login required".to_string(),
                        observed_at_ms: None,
                    },
                ],
                controls: vec![
                    fake_snapshot_control(
                        "agent",
                        psychevo_runtime_host::ControlState::Selectable,
                        Some(json!("review")),
                        vec![json!("review")],
                    ),
                    fake_snapshot_control(
                        "model",
                        psychevo_runtime_host::ControlState::Selectable,
                        None,
                        vec![json!("gpt-fixture"), json!("gpt-fixture-mini")],
                    ),
                    fake_snapshot_control(
                        "effort",
                        psychevo_runtime_host::ControlState::Selectable,
                        None,
                        vec![json!("medium"), json!("high")],
                    ),
                    fake_snapshot_control(
                        "personality",
                        psychevo_runtime_host::ControlState::Selectable,
                        None,
                        vec![json!("none"), json!("friendly"), json!("pragmatic")],
                    ),
                    fake_snapshot_control(
                        "serviceTier",
                        psychevo_runtime_host::ControlState::Selectable,
                        None,
                        vec![json!("fast")],
                    ),
                ],
                capabilities: vec![psychevo_runtime_host::RuntimeCapability {
                    id: "turn.start".to_string(),
                    enabled: true,
                    stability: psychevo_runtime_host::RuntimeStability::Stable,
                }, psychevo_runtime_host::RuntimeCapability {
                    id: "control.agent.set".to_string(),
                    enabled: true,
                    stability: psychevo_runtime_host::RuntimeStability::Stable,
                }],
                process_epoch: Some(4),
                instance_epoch: Some(5),
                binding_epoch: None,
                extension: Some(json!({
                    "codex": {
                        "controlModel": "gpt-fixture"
                    }
                })),
            })
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        Box::pin(async move {
            match request.intent {
                psychevo_runtime_host::RuntimeIntent::Auth(_) => {
                    Ok(psychevo_runtime_host::ExecuteResult::Auth(
                        psychevo_runtime_host::RuntimeAuthResult {
                            accepted: true,
                            status: "auth_checked".to_string(),
                            message: "fake adapter checked authentication".to_string(),
                            output: Value::Null,
                        },
                    ))
                }
                psychevo_runtime_host::RuntimeIntent::Control(_) => {
                    Err(psychevo_runtime_host::RuntimeError::new(
                        "fake_control_unsupported",
                        psychevo_runtime_host::RuntimeErrorStage::Control,
                        psychevo_runtime_host::RetryClass::UserAction,
                        "fake adapter has no observed control mutation",
                    ))
                }
                _ => Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "fake snapshot runtime does not execute this intent",
                )),
            }
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn web_state_with_snapshot_runtime() -> (
    tempfile::TempDir,
    WebState,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);
    let runtime_state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let snapshots = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(
        psychevo_runtime_host::RuntimeKind::Codex,
        Arc::new(SnapshotCountingRuntime {
            snapshots: Arc::clone(&snapshots),
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        runtime_state,
        Arc::new(crate::PsychevoRuntimeBackend),
        host,
    );
    let config =
        GatewayWebServerConfig::new(gateway, home, cwd, None, env, temp.path().join("static"));
    (temp, WebState::new(config), snapshots)
}

#[test]
fn direct_runtime_ready_requires_stable_adapter_and_complete_capability_matrix() {
    let profile = super::runtime_profiles::generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "codex")
        .expect("generated Codex profile");
    let mut snapshot = psychevo_runtime_host::RuntimeSnapshot {
        runtime_ref: "codex".to_string(),
        kind: psychevo_runtime_host::RuntimeKind::Codex,
        profile_revision: 1,
        capability_revision: 1,
        adapter_version: "fake".to_string(),
        runtime_version: Some("fake".to_string()),
        stability: psychevo_runtime_host::RuntimeStability::Experimental,
        provenance: "test".to_string(),
        readiness: vec![psychevo_runtime_host::ReadinessStage {
            id: "transport".to_string(),
            status: psychevo_runtime_host::ReadinessStatus::Ready,
            summary: "transport ready".to_string(),
            observed_at_ms: Some(1),
        }],
        controls: Vec::new(),
        capabilities: super::runtime_profiles::mandatory_runtime_capability_ids(profile.runtime)
            .iter()
            .map(|id| psychevo_runtime_host::RuntimeCapability {
                id: (*id).to_string(),
                enabled: true,
                stability: psychevo_runtime_host::RuntimeStability::Stable,
            })
            .collect(),
        process_epoch: Some(1),
        instance_epoch: None,
        binding_epoch: None,
        extension: None,
    };

    let experimental_adapter =
        super::runtime_profiles::runtime_profile_health(&profile, Some(&snapshot), None);
    assert_eq!(experimental_adapter.status, "unsupported");

    snapshot.stability = psychevo_runtime_host::RuntimeStability::Stable;
    let turn = snapshot
        .capabilities
        .iter_mut()
        .find(|capability| capability.id == "turn.start")
        .expect("turn.start capability");
    turn.stability = psychevo_runtime_host::RuntimeStability::Experimental;
    let experimental_turn =
        super::runtime_profiles::runtime_profile_health(&profile, Some(&snapshot), None);
    assert_eq!(experimental_turn.status, "unsupported");

    let turn = snapshot
        .capabilities
        .iter_mut()
        .find(|capability| capability.id == "turn.start")
        .expect("turn.start capability");
    turn.stability = psychevo_runtime_host::RuntimeStability::Stable;
    let stable = super::runtime_profiles::runtime_profile_health(&profile, Some(&snapshot), None);
    assert_eq!(stable.status, "ready");

    snapshot
        .capabilities
        .retain(|capability| capability.id != "thread.usage");
    let incomplete =
        super::runtime_profiles::runtime_profile_health(&profile, Some(&snapshot), None);
    assert_eq!(incomplete.status, "unsupported");
    assert!(incomplete.summary.contains("thread.usage"));
}

#[test]
fn direct_runtime_ready_is_withheld_until_the_counterpart_adapter_is_ready() {
    let mut codex = wire::RuntimeHealthView {
        status: "ready".to_string(),
        summary: "Codex ready".to_string(),
        command_path: None,
        checked_at_ms: Some(1),
    };
    super::runtime_profiles::apply_direct_runtime_milestone_health(
        RuntimeProfileKind::Codex,
        false,
        &mut codex,
    );
    assert_eq!(codex.status, "unsupported");
    assert!(codex.summary.contains("opencode"), "{}", codex.summary);

    let mut opencode = wire::RuntimeHealthView {
        status: "ready".to_string(),
        summary: "OpenCode ready".to_string(),
        command_path: None,
        checked_at_ms: Some(1),
    };
    super::runtime_profiles::apply_direct_runtime_milestone_health(
        RuntimeProfileKind::OpenCode,
        true,
        &mut opencode,
    );
    assert_eq!(opencode.status, "ready");
}

#[tokio::test]
async fn runtime_control_and_auth_rpc_cross_typed_host_intents() {
    let (_temp, state, _snapshots) = web_state_with_snapshot_runtime();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();
    let auth = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("typed-auth")),
            method: "runtime/auth/action".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "action": "repair",
                "input": null,
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("typed auth action");
    assert_eq!(auth["accepted"], true);
    assert_eq!(auth["status"], "auth_checked");

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("control-snapshot")),
            method: "runtime/snapshot".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("cache control descriptor");
    let (profile_config_json, profile_fingerprint, profile_revision) =
        bound_runtime_profile_fixture("codex");
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&scope.cwd, "test", "pending", "codex", None)
        .expect("thread");
    let binding = state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref: "codex",
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("control-native"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("runtime binding");
    let captured_profile: RuntimeProfileConfig =
        serde_json::from_str(&profile_config_json).expect("captured Runtime Profile");
    state
        .inner
        .gateway
        .refresh_runtime_snapshot(psychevo_runtime_host::SnapshotQuery {
            profile: crate::gateway_runtime_profile(
                captured_profile,
                profile_revision.parse().expect("Profile revision"),
                profile_fingerprint.clone(),
            ),
            scope: psychevo_runtime_host::SnapshotScope::Session {
                cwd: scope.cwd.clone(),
                thread_id: thread_id.clone(),
                native_session_id: Some("control-native".to_string()),
            },
            mode: psychevo_runtime_host::SnapshotMode::BoundedProbe,
        })
        .await
        .expect("cache bound control descriptor");
    let source_key = scope.source.source_key();
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &scope.source.kind,
            raw_identity: scope.source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: scope.source.visible_name.as_deref(),
            thread_id: Some(&thread_id),
            draft_runtime_ref: None,
            lineage: None,
        })
        .expect("bound source lane");
    let error = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("typed-control")),
            method: "runtime/control/set".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "controlId": "agent",
                "value": "write",
                "expectedCapabilityRevision": "9007199254740993",
                "expectedBindingRevision": binding.binding_revision,
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect_err("adapter must reject unsupported observed mutation");
    assert_eq!(
        error.structured_data().and_then(|data| data.get("code")),
        Some(&json!("fake_control_unsupported"))
    );
    assert_eq!(
        error.structured_data().and_then(|data| data.get("stage")),
        Some(&json!("control"))
    );
}

#[tokio::test]
async fn bound_runtime_context_keeps_its_effective_profile_after_edit_and_delete() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let pinned = RuntimeProfileConfig {
        id: "pinned-codex".to_string(),
        runtime: RuntimeProfileKind::Codex,
        enabled: true,
        label: "Pinned Codex".to_string(),
        backend_ref: None,
        command: Some("codex-old".to_string()),
        args: vec!["app-server".to_string(), "--stdio".to_string()],
        env: BTreeMap::new(),
        default_model: Some("pinned-model".to_string()),
        default_mode: None,
        default_agent: None,
        approval_mode: Some("on-request".to_string()),
        sandbox: Some("workspace-write".to_string()),
        workspace_roots: vec![scope.cwd.display().to_string()],
        options: Value::Null,
    };
    let fingerprint = crate::runtime_profile_config_fingerprint(&pinned);
    let revision = crate::runtime_profile_config_revision(&fingerprint).to_string();
    let encoded = serde_json::to_string(&pinned).expect("serialize pinned profile");
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&scope.cwd, "test", "pending", "codex", None)
        .expect("thread");
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref: &pinned.id,
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("pinned-native"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &fingerprint,
            profile_revision: &revision,
            profile_config_json: &encoded,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("runtime binding");
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("edit-current-profile")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "pinned-codex",
                "target": "profile",
                "runtime": "codex",
                "enabled": true,
                "label": "Changed Codex",
                "command": "codex-new",
                "args": ["app-server", "--stdio"],
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("edit mutable current profile");

    let read_context = |id: &'static str| {
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: "runtime/context/read".to_string(),
                params: Some(json!({
                    "threadId": thread_id,
                    "scope": scope.to_wire_scope()
                })),
            },
        )
    };
    let edited = read_context("bound-context-after-edit")
        .await
        .expect("bound context after edit");
    let selected = edited["profiles"]
        .as_array()
        .and_then(|profiles| profiles.iter().find(|profile| profile["id"] == "pinned-codex"))
        .expect("bound effective profile row");
    assert_eq!(selected["label"], "Pinned Codex");
    assert_eq!(selected["command"], "codex-old");
    assert_eq!(selected["sourceTargets"], json!([]));

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("delete-current-profile")),
            method: "runtime/profile/delete".to_string(),
            params: Some(json!({
                "id": "pinned-codex",
                "target": "profile",
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("delete mutable current profile");
    let deleted = read_context("bound-context-after-delete")
        .await
        .expect("bound context after delete");
    assert_eq!(deleted["runtimeRef"], "pinned-codex");
    assert_eq!(deleted["selectionState"], "bound");
    assert!(deleted["profiles"].as_array().is_some_and(|profiles| {
        profiles.iter().any(|profile| {
            profile["id"] == "pinned-codex" && profile["label"] == "Pinned Codex"
        })
    }));
}

#[derive(Debug)]
struct SessionAuthorizationRuntime {
    cwd: PathBuf,
    foreign_cwd: PathBuf,
    operations: Arc<Mutex<Vec<psychevo_runtime_host::RuntimeSessionOperation>>>,
}

impl SessionAuthorizationRuntime {
    fn session(&self, native_session_id: &str) -> psychevo_runtime_host::RuntimeSession {
        use psychevo_runtime_host::SessionOwnership;

        let (cwd, ownership, actions, parent_native_session_id) = match native_session_id {
            "active-session" => (
                self.cwd.clone(),
                SessionOwnership::Active,
                vec!["read", "fork", "resume", "archive"],
                None,
            ),
            "readonly-session" => (
                self.cwd.clone(),
                SessionOwnership::ReadOnly,
                vec!["read", "fork", "resume", "archive"],
                Some("native-parent".to_string()),
            ),
            "transferable-session" => (
                self.cwd.clone(),
                SessionOwnership::ReadOnly,
                vec!["read", "fork", "resume", "archive"],
                None,
            ),
            "unsupported-session" => (
                self.cwd.clone(),
                SessionOwnership::ReadWrite,
                vec!["read"],
                None,
            ),
            "foreign-session" => (
                self.foreign_cwd.clone(),
                SessionOwnership::ReadWrite,
                vec!["read", "archive"],
                None,
            ),
            _ => (
                self.cwd.clone(),
                SessionOwnership::ReadWrite,
                vec![
                    "read",
                    "resume",
                    "fork",
                    "archive",
                    "unarchive",
                    "rename",
                    "delete",
                    "revert",
                    "unrevert",
                ],
                None,
            ),
        };
        psychevo_runtime_host::RuntimeSession {
            native_session_id: native_session_id.to_string(),
            thread_id: None,
            parent_native_session_id,
            title: Some(format!("Fake {native_session_id}")),
            cwd: Some(cwd),
            archived: false,
            updated_at_ms: Some(1),
            cursor: None,
            native_dedup_key: format!("dedup:{native_session_id}"),
            fidelity: psychevo_runtime_host::HistoryFidelity::Full,
            ownership,
            actions: actions.into_iter().map(str::to_string).collect(),
            messages: vec![
                psychevo_runtime_host::RuntimeHistoryMessage {
                    dedup_key: "history-user-1".to_string(),
                    role: "user".to_string(),
                    text: "Imported question".to_string(),
                    created_at_ms: Some(10),
                    metadata: None,
                },
                psychevo_runtime_host::RuntimeHistoryMessage {
                    dedup_key: "history-assistant-1".to_string(),
                    role: "assistant".to_string(),
                    text: "Imported answer".to_string(),
                    created_at_ms: Some(11),
                    metadata: None,
                },
            ],
        }
    }
}

impl psychevo_runtime_host::RuntimeModule for SessionAuthorizationRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Configuration,
                psychevo_runtime_host::RetryClass::Never,
                "session authorization fake does not expose snapshots",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        use psychevo_runtime_host::{ExecuteResult, RuntimeIntent, RuntimeSessionResult};

        let RuntimeIntent::Session(request) = request.intent else {
            return Box::pin(async {
                Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::Never,
                    "session authorization fake only accepts session requests",
                ))
            });
        };
        if request.operation == psychevo_runtime_host::RuntimeSessionOperation::List {
            let mut sessions = [
                "active-session",
                "readonly-session",
                "transferable-session",
                "unsupported-session",
                "foreign-session",
                "idle-session",
            ]
            .into_iter()
            .map(|native_session_id| self.session(native_session_id))
            .collect::<Vec<_>>();
            let mut missing_cwd = self.session("missing-cwd-session");
            missing_cwd.cwd = None;
            missing_cwd.title = Some("Unverified missing-cwd session".to_string());
            sessions.push(missing_cwd);
            return Box::pin(async move {
                Ok(ExecuteResult::Session(RuntimeSessionResult {
                    changed: false,
                    sessions,
                    cursor: None,
                    message: None,
                }))
            });
        }
        self.operations
            .lock()
            .expect("session operations")
            .push(request.operation);
        let native_session_id = request
            .native_session_id
            .as_deref()
            .unwrap_or("idle-session");
        let mut session = self.session(native_session_id);
        if request.operation == psychevo_runtime_host::RuntimeSessionOperation::Fork {
            session.native_session_id = format!("fork-of-{native_session_id}");
            session.native_dedup_key = format!("dedup:fork-of-{native_session_id}");
            session.ownership = psychevo_runtime_host::SessionOwnership::ReadWrite;
        } else if !matches!(
            request.operation,
            psychevo_runtime_host::RuntimeSessionOperation::List
                | psychevo_runtime_host::RuntimeSessionOperation::Read
        ) {
            session.ownership = psychevo_runtime_host::SessionOwnership::ReadWrite;
            session.actions.retain(|action| action != "resume");
        }
        let changed = !matches!(
            request.operation,
            psychevo_runtime_host::RuntimeSessionOperation::List
                | psychevo_runtime_host::RuntimeSessionOperation::Read
        );
        Box::pin(async move {
            Ok(ExecuteResult::Session(RuntimeSessionResult {
                changed,
                sessions: vec![session],
                cursor: None,
                message: None,
            }))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn web_state_with_session_authorization_runtime() -> (
    tempfile::TempDir,
    WebState,
    Arc<Mutex<Vec<psychevo_runtime_host::RuntimeSessionOperation>>>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let foreign_cwd = temp.path().join("foreign-work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&foreign_cwd).expect("foreign cwd");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);
    let runtime_state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let operations = Arc::new(Mutex::new(Vec::new()));
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(
        psychevo_runtime_host::RuntimeKind::Codex,
        Arc::new(SessionAuthorizationRuntime {
            cwd: cwd.canonicalize().expect("canonical cwd"),
            foreign_cwd: foreign_cwd.canonicalize().expect("canonical foreign cwd"),
            operations: Arc::clone(&operations),
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        runtime_state,
        Arc::new(crate::PsychevoRuntimeBackend),
        host,
    );
    let config =
        GatewayWebServerConfig::new(gateway, home, cwd, None, env, temp.path().join("static"));
    (temp, WebState::new(config), operations)
}

#[derive(Debug, Clone)]
struct RevisionSessionCall {
    operation: psychevo_runtime_host::RuntimeSessionOperation,
    cursor: Option<String>,
    argument: Option<Value>,
}

#[derive(Debug)]
struct RevisionSessionRuntime {
    cwd: PathBuf,
    calls: Arc<Mutex<Vec<RevisionSessionCall>>>,
}

impl RevisionSessionRuntime {
    fn session(
        &self,
        messages: Vec<psychevo_runtime_host::RuntimeHistoryMessage>,
        cursor: Option<String>,
    ) -> psychevo_runtime_host::RuntimeSession {
        psychevo_runtime_host::RuntimeSession {
            native_session_id: "native-session-secret".to_string(),
            thread_id: None,
            parent_native_session_id: None,
            title: Some("Revision history".to_string()),
            cwd: Some(self.cwd.clone()),
            archived: false,
            updated_at_ms: Some(20),
            cursor,
            native_dedup_key: "native-session-dedup-secret".to_string(),
            fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
            ownership: psychevo_runtime_host::SessionOwnership::ReadWrite,
            actions: ["read", "revert", "unrevert"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            messages,
        }
    }

    fn message(native_message_id: &str, created_at_ms: i64) -> psychevo_runtime_host::RuntimeHistoryMessage {
        psychevo_runtime_host::RuntimeHistoryMessage {
            dedup_key: format!("native-dedup-{native_message_id}"),
            role: "user".to_string(),
            text: "Private native history text".to_string(),
            created_at_ms: Some(created_at_ms),
            metadata: Some(json!({"nativeMessageId": native_message_id})),
        }
    }
}

impl psychevo_runtime_host::RuntimeModule for RevisionSessionRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Configuration,
                psychevo_runtime_host::RetryClass::Never,
                "revision fake does not expose snapshots",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        use psychevo_runtime_host::{ExecuteResult, RuntimeIntent, RuntimeSessionOperation, RuntimeSessionResult};

        let RuntimeIntent::Session(request) = request.intent else {
            return Box::pin(async {
                Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::Never,
                    "revision fake only accepts session requests",
                ))
            });
        };
        self.calls.lock().expect("revision calls").push(RevisionSessionCall {
            operation: request.operation,
            cursor: request.cursor.clone(),
            argument: request.argument.clone(),
        });
        let result = match request.operation {
            RuntimeSessionOperation::List => {
                let (sessions, cursor) = match request.cursor.as_deref() {
                    None => (
                        vec![self.session(Vec::new(), None)],
                        Some("native-list-cursor-secret".to_string()),
                    ),
                    Some("native-list-cursor-secret") =>
                        (vec![self.session(Vec::new(), None)], None),
                    Some(_) => (Vec::new(), None),
                };
                RuntimeSessionResult {
                    changed: false,
                    sessions,
                    cursor,
                    message: None,
                }
            }
            RuntimeSessionOperation::Read => {
                let (messages, cursor) = match request.cursor.as_deref() {
                    None => (
                        vec![Self::message("msg-native-new-secret", 20)],
                        Some("msg-native-page-boundary-secret".to_string()),
                    ),
                    Some("msg-native-page-boundary-secret") => (
                        vec![Self::message("msg-native-old-secret", 10)],
                        None,
                    ),
                    Some(_) => (Vec::new(), None),
                };
                RuntimeSessionResult {
                    changed: false,
                    sessions: vec![self.session(messages, cursor.clone())],
                    cursor,
                    message: None,
                }
            }
            RuntimeSessionOperation::Revert | RuntimeSessionOperation::Unrevert => {
                RuntimeSessionResult {
                    changed: true,
                    sessions: vec![self.session(Vec::new(), None)],
                    cursor: None,
                    message: Some("OpenCode staged history changed".to_string()),
                }
            }
            _ => RuntimeSessionResult {
                changed: false,
                sessions: vec![self.session(Vec::new(), None)],
                cursor: None,
                message: None,
            },
        };
        Box::pin(async move { Ok(ExecuteResult::Session(result)) })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn web_state_with_revision_session_runtime() -> (
    tempfile::TempDir,
    WebState,
    Arc<Mutex<Vec<RevisionSessionCall>>>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let env = BTreeMap::from([
        ("HOME".to_string(), temp.path().display().to_string()),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let runtime_state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(
        psychevo_runtime_host::RuntimeKind::OpenCode,
        Arc::new(RevisionSessionRuntime {
            cwd: cwd.canonicalize().expect("canonical cwd"),
            calls: Arc::clone(&calls),
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        runtime_state,
        Arc::new(crate::PsychevoRuntimeBackend),
        host,
    );
    let config =
        GatewayWebServerConfig::new(gateway, home, cwd, None, env, temp.path().join("static"));
    (temp, WebState::new(config), calls)
}

async fn direct_session_rpc(
    state: &WebState,
    method: &str,
    native_session_id: &str,
    extra: Value,
) -> Value {
    direct_session_rpc_for_runtime(state, method, "codex", native_session_id, extra).await
}

async fn direct_session_rpc_for_runtime(
    state: &WebState,
    method: &str,
    runtime_ref: &str,
    native_session_id: &str,
    extra: Value,
) -> Value {
    let scope = default_resolved_scope(state, &AuthContext::Bearer).expect("scope");
    let has_persisted_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session(runtime_ref, native_session_id)
        .expect("native session binding lookup")
        .is_some();
    if !has_persisted_binding {
        let (prime_tx, _prime_rx) = mpsc::unbounded_channel();
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            prime_tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("prime-session-handles")),
                method: "runtime/session/list".to_string(),
                params: Some(json!({
                    "runtimeRef": runtime_ref,
                    "scope": scope.to_wire_scope(),
                })),
            },
        )
        .await
        .expect("prime opaque session handles");
    }
    let session_handle =
        crate::runtime_session_handle(runtime_ref, &scope.cwd, native_session_id);
    let mut params = json!({
        "runtimeRef": runtime_ref,
        "sessionHandle": session_handle,
        "scope": scope.to_wire_scope(),
    });
    params
        .as_object_mut()
        .expect("RPC params")
        .extend(extra.as_object().expect("extra params").clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(method)),
            method: method.to_string(),
            params: Some(params),
        },
    )
    .await
    .expect("direct runtime session RPC")
}

#[tokio::test]
async fn direct_runtime_public_session_contract_uses_only_opaque_handles() {
    let (_temp, state, _operations) = web_state_with_session_authorization_runtime();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();
    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("opaque-list")),
            method: "runtime/session/list".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect("runtime/session/list");
    let serialized = serde_json::to_string(&listed).expect("serialize public result");
    assert!(!serialized.contains("nativeSessionId"), "{listed}");
    assert!(!serialized.contains("nativeDedupKey"), "{listed}");
    for native_id in [
        "active-session",
        "readonly-session",
        "transferable-session",
        "unsupported-session",
        "owned-session",
    ] {
        assert!(!serialized.contains(native_id), "{listed}");
        assert!(!serialized.contains(&format!("dedup:{native_id}")), "{listed}");
    }
    assert_eq!(listed["sessions"].as_array().map(Vec::len), Some(5));
    assert!(
        listed["sessions"]
            .as_array()
            .is_some_and(|sessions| sessions.iter().all(|session| {
                session["title"] != "Unverified missing-cwd session"
            })),
        "a session without a verified cwd must not enter the public list: {listed}"
    );
    let first = &listed["sessions"][0];
    assert!(
        first["sessionHandle"]
            .as_str()
            .is_some_and(|handle| handle.starts_with("rts_")),
        "{listed}"
    );
    assert!(
        first["dedupKey"]
            .as_str()
            .is_some_and(|key| key.starts_with("rtd_")),
        "{listed}"
    );

    let native_session_id = "context-native-secret";
    let runtime_ref = "captured-codex";
    let captured_profile = RuntimeProfileConfig {
        id: runtime_ref.to_string(),
        runtime: RuntimeProfileKind::Codex,
        enabled: true,
        label: "Captured Codex".to_string(),
        backend_ref: None,
        command: Some("captured-codex-command".to_string()),
        args: vec!["app-server".to_string(), "--stdio".to_string()],
        env: BTreeMap::new(),
        default_model: None,
        default_mode: None,
        default_agent: None,
        approval_mode: Some("on-request".to_string()),
        sandbox: Some("workspace-write".to_string()),
        workspace_roots: vec![scope.cwd.display().to_string()],
        options: Value::Null,
    };
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&captured_profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let profile_config_json =
        serde_json::to_string(&captured_profile).expect("captured Profile JSON");
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&scope.cwd, "test", "pending", "codex", None)
        .expect("thread");
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref,
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some(native_session_id),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("runtime binding");
    let child_thread_id = state
        .inner
        .state
        .store()
        .create_child_session_with_metadata(
            &thread_id,
            &scope.cwd,
            "runtime_child",
            "pending",
            "codex",
            None,
        )
        .expect("child thread");
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &child_thread_id,
            runtime_ref,
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("transferable-session"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadOnly,
            parent_thread_id: Some(&thread_id),
        })
        .expect("child runtime binding");
    state
        .inner
        .state
        .store()
        .set_session_metadata_field(
            &child_thread_id,
            "runtimeStatus",
            Some(json!("idle")),
        )
        .expect("child runtime status");
    let session_handle = crate::runtime_session_handle(runtime_ref, &scope.cwd, native_session_id);
    state
        .inner
        .gateway
        .bind_source_thread(
            &scope.source,
            &thread_id,
            &GatewayBackendInfo {
                kind: BackendKind::Runtime,
                runtime_ref: Some(runtime_ref.to_string()),
                native_id: Some(session_handle.clone()),
            },
            None,
        )
        .expect("source binding");
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("opaque-context")),
            method: "runtime/context/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("runtime/context/read");
    assert_eq!(context["binding"]["sessionHandle"], session_handle);
    assert_eq!(context["activeSession"]["sessionHandle"], session_handle);
    assert_eq!(context["children"].as_array().map(Vec::len), Some(1));
    assert_eq!(context["children"][0]["threadId"], child_thread_id);
    assert_eq!(context["children"][0]["parentThreadId"], thread_id);
    assert_eq!(context["children"][0]["status"], "idle");
    assert!(
        context["children"][0]["sessionHandle"]
            .as_str()
            .is_some_and(|handle| handle.starts_with("rts_") && !handle.contains("transferable")),
        "{context}"
    );
    let serialized = serde_json::to_string(&context).expect("serialize runtime context");
    assert!(!serialized.contains(native_session_id), "{context}");
    assert!(!serialized.contains("nativeSessionId"), "{context}");

    let first_read = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/read",
        runtime_ref,
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(first_read["session"]["threadId"], child_thread_id);
    assert_eq!(first_read["session"]["status"], "idle");
    state
        .inner
        .state
        .store()
        .set_session_metadata_field(
            &child_thread_id,
            "runtimeStatus",
            Some(json!("idle; nativeSession=secret")),
        )
        .expect("malformed child runtime status fixture");
    let second_read = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/read",
        runtime_ref,
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(second_read["session"]["threadId"], child_thread_id);
    assert!(second_read["session"]["status"].is_null(), "{second_read}");
    assert_eq!(
        state
            .inner
            .state
            .store()
            .load_messages(&child_thread_id)
            .expect("imported child history")
            .len(),
        2,
        "repeated lazy reads must deduplicate native history"
    );
}

#[tokio::test]
async fn active_direct_session_attach_creates_and_reuses_read_only_public_thread() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();
    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list-active-attach")),
            method: "runtime/session/list".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect("list active direct session");
    let active = listed["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|session| session["ownership"] == "active")
        })
        .expect("active session row");
    assert_eq!(active["threadId"], Value::Null);
    assert!(
        active["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action == "attach")),
        "{active}"
    );
    let session_handle = active["sessionHandle"]
        .as_str()
        .expect("opaque active session handle")
        .to_string();

    let attach = |id: &'static str| {
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: "runtime/session/attach".to_string(),
                params: Some(json!({
                    "runtimeRef": "codex",
                    "sessionHandle": session_handle,
                    "scope": scope.to_wire_scope(),
                })),
            },
        )
    };
    let attached = attach("attach-active")
        .await
        .expect("attach active session read-only");
    assert_eq!(attached["changed"], true, "{attached}");
    assert_eq!(attached["session"]["ownership"], "readOnly", "{attached}");
    let thread_id = attached["session"]["threadId"]
        .as_str()
        .expect("attached public thread")
        .to_string();
    let serialized = serde_json::to_string(&attached).expect("serialize attach result");
    assert!(!serialized.contains("active-session"), "{attached}");
    assert!(!serialized.contains("nativeSessionId"), "{attached}");

    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)
        .expect("attached binding lookup")
        .expect("attached binding");
    assert_eq!(binding.ownership, GatewayRuntimeBindingOwnership::ReadOnly);
    assert_eq!(binding.status, GatewayRuntimeBindingStatus::Resolved);
    assert_eq!(binding.parent_thread_id, None);
    assert_eq!(binding.runtime_ref.as_deref(), Some("codex"));
    assert!(binding.profile_fingerprint.as_deref().is_some_and(|value| !value.is_empty()));
    assert!(binding.profile_revision.as_deref().is_some_and(|value| !value.is_empty()));
    assert!(binding.profile_config_json.as_deref().is_some_and(|value| !value.is_empty()));
    assert_eq!(
        state
            .inner
            .state
            .store()
            .load_messages(&thread_id)
            .expect("attached history")
            .len(),
        2
    );
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&scope.source)
            .expect("attached source lane")
            .as_deref(),
        Some(thread_id.as_str())
    );

    let reopened = attach("reattach-active")
        .await
        .expect("reuse active read-only attachment");
    assert_eq!(reopened["changed"], false, "{reopened}");
    assert_eq!(reopened["session"]["threadId"], thread_id, "{reopened}");
    assert_eq!(
        state
            .inner
            .state
            .store()
            .load_messages(&thread_id)
            .expect("deduplicated attached history")
            .len(),
        2
    );

    let resume = direct_session_rpc(
        &state,
        "runtime/session/resume",
        "active-session",
        json!({}),
    )
    .await;
    assert_eq!(resume["changed"], false, "{resume}");
    assert_eq!(resume["session"]["ownership"], "active", "{resume}");
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Read,
        ],
        "Attach and rejected Resume must never invoke native takeover"
    );
}

#[tokio::test]
async fn opencode_session_list_cursor_is_gateway_opaque() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, calls) = web_state_with_revision_session_runtime();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();
    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list-first-page")),
            method: "runtime/session/list".to_string(),
            params: Some(json!({
                "runtimeRef": "opencode",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect("first session list page");
    let cursor = first["nextCursor"]
        .as_str()
        .expect("opaque list cursor")
        .to_string();
    assert!(cursor.starts_with("rtl_"), "{first}");
    assert!(
        !serde_json::to_string(&first)
            .expect("serialize list")
            .contains("native-list-cursor-secret"),
        "{first}"
    );

    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list-second-page")),
            method: "runtime/session/list".to_string(),
            params: Some(json!({
                "runtimeRef": "opencode",
                "cursor": cursor,
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect("second session list page");
    assert!(second["nextCursor"].is_null(), "{second}");
    assert!(calls.lock().expect("revision calls").iter().any(|call| {
        call.operation == RuntimeSessionOperation::List
            && call.cursor.as_deref() == Some("native-list-cursor-secret")
    }));
    let list_calls_before_raw = calls
        .lock()
        .expect("revision calls")
        .iter()
        .filter(|call| call.operation == RuntimeSessionOperation::List)
        .count();

    let raw = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("list-raw-cursor")),
            method: "runtime/session/list".to_string(),
            params: Some(json!({
                "runtimeRef": "opencode",
                "cursor": "native-list-cursor-secret",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect_err("raw native list cursor must be rejected");
    assert!(raw.to_string().contains("opaque Native Sessions cursor"), "{raw}");
    assert_eq!(
        calls
            .lock()
            .expect("revision calls")
            .iter()
            .filter(|call| call.operation == RuntimeSessionOperation::List)
            .count(),
        list_calls_before_raw,
        "a raw public cursor must fail before adapter invocation"
    );
}

#[tokio::test]
async fn opencode_revision_and_history_cursors_are_opaque_and_resolve_only_inside_gateway() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, calls) = web_state_with_revision_session_runtime();
    let first = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/read",
        "opencode",
        "native-session-secret",
        json!({}),
    )
    .await;
    let serialized = serde_json::to_string(&first).expect("serialize first page");
    for secret in [
        "native-session-secret",
        "msg-native-new-secret",
        "msg-native-page-boundary-secret",
        "native-session-dedup-secret",
    ] {
        assert!(!serialized.contains(secret), "{first}");
    }
    let revision = first["revisions"][0]["revisionHandle"]
        .as_str()
        .expect("first opaque revision")
        .to_string();
    let cursor = first["nextCursor"]
        .as_str()
        .expect("first opaque cursor")
        .to_string();
    assert!(revision.starts_with("rtr_"), "{first}");
    assert!(cursor.starts_with("rtc_"), "{first}");
    assert_eq!(first["revisions"][0]["role"], "user");

    let second = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/read",
        "opencode",
        "native-session-secret",
        json!({"cursor": cursor}),
    )
    .await;
    assert!(second["nextCursor"].is_null(), "{second}");
    let old_revision = second["revisions"][0]["revisionHandle"]
        .as_str()
        .expect("second opaque revision")
        .to_string();
    let serialized = serde_json::to_string(&second).expect("serialize second page");
    assert!(!serialized.contains("msg-native-old-secret"), "{second}");
    assert!(!serialized.contains("msg-native-page-boundary-secret"), "{second}");
    let read_cursors = calls
        .lock()
        .expect("revision calls")
        .iter()
        .filter(|call| call.operation == RuntimeSessionOperation::Read)
        .map(|call| call.cursor.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        read_cursors,
        vec![None, Some("msg-native-page-boundary-secret".to_string())],
        "opaque cursor lookup must not replay every preceding native page"
    );

    let reverted = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/revert",
        "opencode",
        "native-session-secret",
        json!({"revisionHandle": old_revision}),
    )
    .await;
    assert_eq!(reverted["changed"], true, "{reverted}");
    let serialized = serde_json::to_string(&reverted).expect("serialize revert result");
    assert!(!serialized.contains("msg-native-old-secret"), "{reverted}");
    let revert_call = calls
        .lock()
        .expect("revision calls")
        .iter()
        .find(|call| call.operation == RuntimeSessionOperation::Revert)
        .cloned()
        .expect("native revert call");
    assert_eq!(
        revert_call.argument,
        Some(json!({"messageID": "msg-native-old-secret"})),
        "only the internal adapter request may receive the native message id"
    );
    assert!(
        calls.lock().expect("revision calls").iter().any(|call| {
            call.operation == RuntimeSessionOperation::Read
                && call.cursor.as_deref() == Some("msg-native-page-boundary-secret")
        }),
        "Gateway must resolve its opaque cursor before the adapter read"
    );

    let unreverted = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/unrevert",
        "opencode",
        "native-session-secret",
        json!({}),
    )
    .await;
    assert_eq!(unreverted["changed"], true, "{unreverted}");
}

#[tokio::test]
async fn direct_runtime_revision_rpc_rejects_raw_ids_unknown_handles_and_codex() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, calls) = web_state_with_revision_session_runtime();
    let _ = direct_session_rpc_for_runtime(
        &state,
        "runtime/session/read",
        "opencode",
        "native-session-secret",
        json!({}),
    )
    .await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_handle = crate::runtime_session_handle(
        "opencode",
        &scope.cwd,
        "native-session-secret",
    );
    let (tx, _rx) = mpsc::unbounded_channel();
    let raw_id = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("raw-revision-id")),
            method: "runtime/session/revert".to_string(),
            params: Some(json!({
                "runtimeRef": "opencode",
                "sessionHandle": session_handle,
                "itemId": "msg-native-old-secret",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect_err("raw itemId must be rejected by the public RPC");
    assert!(raw_id.to_string().contains("unknown field"), "{raw_id}");

    let raw_cursor = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("raw-history-cursor")),
            method: "runtime/session/read".to_string(),
            params: Some(json!({
                "runtimeRef": "opencode",
                "sessionHandle": session_handle,
                "cursor": "msg-native-page-boundary-secret",
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect_err("raw native cursor must not resolve as a Gateway cursor");
    assert!(raw_cursor.to_string().contains("opaque history cursor"), "{raw_cursor}");
    assert!(
        calls
            .lock()
            .expect("revision calls")
            .iter()
            .all(|call| call.operation != RuntimeSessionOperation::Revert),
        "invalid public handles must not invoke the mutation"
    );

    let (_temp, codex_state, codex_operations) = web_state_with_session_authorization_runtime();
    let codex = direct_session_rpc(
        &codex_state,
        "runtime/session/revert",
        "idle-session",
        json!({"revisionHandle": "rtr_not-supported"}),
    )
    .await;
    assert_eq!(codex["supported"], false, "{codex}");
    assert_eq!(codex["changed"], false, "{codex}");
    assert!(
        codex_operations.lock().expect("codex operations").is_empty(),
        "Codex must fail before a native session mutation"
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_preflights_active_sessions_before_every_takeover_mutation() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    for (method, extra) in [
        ("runtime/session/resume", json!({})),
        ("runtime/session/archive", json!({})),
        ("runtime/session/unarchive", json!({})),
        ("runtime/session/delete", json!({})),
        ("runtime/session/rename", json!({"title": "Renamed"})),
    ] {
        let result = direct_session_rpc(&state, method, "active-session", extra).await;
        assert_eq!(result["changed"], false, "{method}: {result}");
        assert_eq!(
            result["session"]["ownership"], "active",
            "{method}: {result}"
        );
    }
    for method in ["runtime/session/revert", "runtime/session/unrevert"] {
        let result = direct_session_rpc(
            &state,
            method,
            "active-session",
            json!({"revisionHandle": "rtr_not-supported"}),
        )
        .await;
        assert_eq!(result["changed"], false, "{method}: {result}");
        assert_eq!(result["supported"], false, "{method}: {result}");
        assert!(result["session"].is_null(), "{method}: {result}");
    }
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![RuntimeSessionOperation::Read; 5],
        "an active session must only be live-read, never mutated or taken over"
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_rejects_read_only_and_undeclared_actions_after_live_read() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let read_only = direct_session_rpc(
        &state,
        "runtime/session/archive",
        "readonly-session",
        json!({}),
    )
    .await;
    assert_eq!(read_only["changed"], false, "{read_only}");
    assert_eq!(read_only["supported"], true, "{read_only}");
    assert_eq!(read_only["session"]["ownership"], "readOnly");

    let unsupported = direct_session_rpc(
        &state,
        "runtime/session/archive",
        "unsupported-session",
        json!({}),
    )
    .await;
    assert_eq!(unsupported["changed"], false, "{unsupported}");
    assert_eq!(unsupported["supported"], false, "{unsupported}");
    assert!(
        unsupported["message"]
            .as_str()
            .unwrap_or_default()
            .contains("does not declare the `archive` action"),
        "{unsupported}"
    );
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![RuntimeSessionOperation::Read; 2]
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_rejects_sessions_outside_the_requested_workspace() {
    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let foreign_handle = crate::runtime_session_handle("codex", &scope.cwd, "foreign-session");
    let (tx, _rx) = mpsc::unbounded_channel();
    let error = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("foreign-session")),
            method: "runtime/session/archive".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "sessionHandle": foreign_handle,
                "scope": scope.to_wire_scope(),
            })),
        },
    )
    .await
    .expect_err("foreign session handle must not resolve in this workspace");
    assert!(
        error.to_string().contains("does not recognize the opaque session handle"),
        "{error}"
    );
    assert!(operations.lock().expect("session operations").is_empty());
}

#[tokio::test]
async fn direct_runtime_session_rpc_applies_declared_idle_actions_after_live_read() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let result =
        direct_session_rpc(&state, "runtime/session/archive", "idle-session", json!({})).await;
    assert_eq!(result["changed"], true, "{result}");
    assert_eq!(result["supported"], true, "{result}");
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Archive,
        ]
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_takes_over_a_declared_idle_session_after_live_read() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let result =
        direct_session_rpc(&state, "runtime/session/resume", "idle-session", json!({})).await;
    assert_eq!(result["changed"], true, "{result}");
    assert_eq!(result["supported"], true, "{result}");
    assert!(
        result["session"]["threadId"].as_str().is_some(),
        "an accepted takeover must return its public Gateway thread: {result}"
    );
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Resume,
        ]
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_transfers_declared_read_only_root_then_allows_mutation() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let resumed = direct_session_rpc(
        &state,
        "runtime/session/resume",
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(resumed["changed"], true, "{resumed}");
    assert_eq!(resumed["session"]["ownership"], "readWrite", "{resumed}");

    let archived = direct_session_rpc(
        &state,
        "runtime/session/archive",
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(archived["changed"], true, "{archived}");
    assert_eq!(archived["session"]["ownership"], "readWrite", "{archived}");
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Resume,
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Archive,
        ]
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_keeps_persisted_read_only_child_fork_only() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let store = state.inner.state.store();
    let (profile_config_json, profile_fingerprint, profile_revision) =
        bound_runtime_profile_fixture("codex");
    let parent_thread_id = store
        .create_session_with_metadata(&scope.cwd, "test", "pending", "codex", None)
        .expect("parent thread");
    store
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &parent_thread_id,
            runtime_ref: "codex",
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("native-parent"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("parent binding");
    let child_thread_id = store
        .create_session_with_metadata(&scope.cwd, "test", "pending", "codex", None)
        .expect("child thread");
    store
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &child_thread_id,
            runtime_ref: "codex",
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("transferable-session"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadOnly,
            parent_thread_id: Some(&parent_thread_id),
        })
        .expect("child binding");

    let resume = direct_session_rpc(
        &state,
        "runtime/session/resume",
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(resume["changed"], false, "{resume}");

    let fork = direct_session_rpc(
        &state,
        "runtime/session/fork",
        "transferable-session",
        json!({}),
    )
    .await;
    assert_eq!(fork["changed"], true, "{fork}");
    assert!(
        fork["sessionHandle"]
            .as_str()
            .is_some_and(|handle| handle.starts_with("rts_") && !handle.contains("transferable")),
        "{fork}"
    );
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Read,
            RuntimeSessionOperation::Fork,
        ]
    );
}

#[tokio::test]
async fn direct_runtime_session_rpc_preserves_declared_fork_for_active_sessions() {
    use psychevo_runtime_host::RuntimeSessionOperation;

    let (_temp, state, operations) = web_state_with_session_authorization_runtime();
    let result =
        direct_session_rpc(&state, "runtime/session/fork", "active-session", json!({})).await;
    assert_eq!(result["changed"], true, "{result}");
    assert!(
        result["sessionHandle"]
            .as_str()
            .is_some_and(|handle| handle.starts_with("rts_") && !handle.contains("active")),
        "{result}"
    );
    assert_eq!(
        *operations.lock().expect("session operations"),
        vec![RuntimeSessionOperation::Read, RuntimeSessionOperation::Fork,]
    );
}

#[tokio::test]
async fn runtime_snapshot_bridge_keeps_reads_cache_only_and_refreshes_explicitly() {
    let (_temp, state, snapshots) = web_state_with_snapshot_runtime();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let source_key = scope.source.source_key();
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &scope.source.kind,
            raw_identity: scope.source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: scope.source.visible_name.as_deref(),
            thread_id: None,
            draft_runtime_ref: Some("codex"),
            lineage: None,
        })
        .expect("codex draft lane");
    let (tx, _rx) = mpsc::unbounded_channel();

    let initial_profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-list-before")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("cache-only profile list");
    let initial_codex = initial_profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .find(|profile| profile["id"] == "codex")
        .expect("codex profile");
    assert_eq!(initial_codex["health"]["status"], "unchecked");
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "ordinary list must not invoke the runtime host"
    );

    let initial_context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-context-before")),
            method: "runtime/context/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("cache-only runtime context");
    assert_eq!(initial_context["runtimeRef"], "codex");
    assert_eq!(initial_context["controls"], json!([]));
    assert_eq!(initial_context["stability"], Value::Null);
    assert_eq!(initial_context["capabilities"], json!([]));
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "ordinary context must not invoke the runtime host"
    );

    let prospective_native = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prospective-native-context")),
            method: "runtime/context/read".to_string(),
            params: Some(json!({
                "runtimeRef": "native",
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("prospective native runtime context");
    assert_eq!(prospective_native["runtimeRef"], "native");
    assert_eq!(prospective_native["selectionState"], "prospective");
    assert_eq!(prospective_native["controls"][0]["id"], "mode");
    assert_eq!(prospective_native["controls"][0]["state"], "selectable");
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "prospective context reads must remain cache-only"
    );

    let refreshed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-refresh")),
            method: "runtime/snapshot".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("explicit snapshot refresh");
    assert_eq!(snapshots.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(refreshed["profiles"][0]["health"]["status"], "needs_auth");
    assert_eq!(refreshed["profiles"][0]["stability"], "stable");
    assert_eq!(refreshed["profiles"][0]["capabilities"][0]["id"], "turn.start");
    assert!(
        refreshed["profiles"][0]["readinessStages"][0]["observedAtMs"]
            .as_i64()
            .is_some()
    );
    assert_eq!(refreshed["agents"][0]["name"], "codex-review");
    assert_eq!(refreshed["agents"][0]["nativeId"], Value::Null);

    let cached_profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-list-after")),
            method: "runtime/profile/list".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("cached profile list");
    let cached_codex = cached_profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .find(|profile| profile["id"] == "codex")
        .expect("codex profile");
    assert_eq!(cached_codex["health"]["status"], "needs_auth");
    assert_eq!(cached_codex["capabilityRevision"], "9007199254740993");
    assert!(cached_codex["profileRevision"].is_string(), "{cached_codex}");
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "ordinary list must consume the cached observation"
    );

    let cached_context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-context-after")),
            method: "runtime/context/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("cached runtime context");
    assert_eq!(cached_context["controls"][0]["id"], "agent");
    assert_eq!(cached_context["controls"][0]["state"], "selectable");
    assert_eq!(cached_context["controls"][0]["channelSafe"], true);
    assert_eq!(
        cached_context["controls"][0]["capabilityRevision"],
        "9007199254740993"
    );
    assert_eq!(cached_context["stability"], "stable");
    assert_eq!(cached_context["capabilities"][0]["id"], "turn.start");
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "cached context must not refresh the host"
    );

    let changed_profile = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-profile-change")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "codex",
                "target": "profile",
                "runtime": "codex",
                "enabled": true,
                "label": "Codex changed",
                "command": "codex",
                "args": ["app-server", "--stdio"],
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("change runtime profile");
    assert_eq!(changed_profile["profile"]["health"]["status"], "unchecked");
    assert_eq!(
        snapshots.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a stale fingerprint must not refresh or reuse its cached observation"
    );

    let checked = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("cache-doctor")),
            method: "runtime/health/check".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("explicit runtime doctor");
    assert_eq!(checked["health"]["status"], "needs_auth");
    assert_eq!(snapshots.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn agent_and_backend_rpc_list_generated_peer_backend() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP coding agent."
command = "cursor-agent"
"#,
    )
    .expect("config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    assert_eq!(backends["backends"][0]["id"], "cursor");
    assert_eq!(backends["backends"][0]["sourceTargets"], json!(["profile"]));

    let write = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project",
                "enabled": true,
                "label": "OpenCode",
                "description": "OpenCode ACP coding agent.",
                "command": "opencode",
                "args": ["acp"],
                "entrypoints": ["peer", "subagent"],
                "clientCapabilities": ["fs.read", "fs.write", "terminal"]
            })),
        },
    )
    .await
    .expect("backend/write");
    assert_eq!(write["backend"]["id"], "opencode");
    assert_eq!(write["backend"]["sourceTargets"], json!(["project"]));

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list after write");
    let opencode_backend = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .find(|backend| backend["id"] == "opencode")
        .expect("opencode backend");
    assert_eq!(opencode_backend["sourceTargets"], json!(["project"]));
    assert_eq!(opencode_backend["args"], json!(["acp"]));

    let minimal = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "minimal-acp",
                "target": "profile",
                "enabled": true,
                "command": "minimal-agent",
                "args": ["acp"],
                "entrypoints": ["peer", "subagent"],
                "clientCapabilities": ["fs.read", "fs.write", "terminal"]
            })),
        },
    )
    .await
    .expect("backend/write minimal");
    assert_eq!(minimal["backend"]["label"], "minimal-acp");
    assert_eq!(minimal["backend"]["description"], Value::Null);
    assert_eq!(minimal["backend"]["diagnostics"], json!([]));

    let agents = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("5")),
            method: "agent/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("agent/list");
    let cursor = agents["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "cursor")
        .expect("cursor agent");
    assert_eq!(cursor["generated"], true);
    assert_eq!(cursor["backend"]["ref"], "cursor");
    assert!(agents.get("shadowedAgents").is_some());
    let opencode = agents["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "opencode")
        .expect("opencode agent");
    assert_eq!(opencode["backend"]["ref"], "opencode");
    let minimal_agent = agents["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "minimal-acp")
        .expect("minimal agent");
    assert_eq!(minimal_agent["description"], "minimal-acp");
    assert_eq!(minimal_agent["backend"]["ref"], "minimal-acp");

    let status = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("6")),
            method: "agent/status".to_string(),
            params: None,
        },
    )
    .await
    .expect("agent/status");
    assert!(status.get("control").is_some());
    assert!(status.get("agents").is_some());

    let delete = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("7")),
            method: "backend/delete".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project"
            })),
        },
    )
    .await
    .expect("backend/delete");
    assert_eq!(delete["deleted"], true);
}

#[tokio::test]
async fn runtime_profile_rpc_lists_generated_profiles_and_writes_overrides() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let (tx, _rx) = mpsc::unbounded_channel();

    let profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-1")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("runtime/profile/list");
    let ids = profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .map(|profile| profile["id"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"native"));
    assert!(ids.contains(&"codex"));
    assert!(ids.contains(&"opencode"));
    assert_eq!(
        profiles["profiles"]
            .as_array()
            .expect("profiles")
            .iter()
            .find(|profile| profile["id"] == "native")
            .expect("native")["generated"],
        true
    );

    let write = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-2")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "codex",
                "target": "profile",
                "runtime": "codex",
                "enabled": false,
                "label": "Codex local",
                "command": "codex",
                "args": ["app-server", "--stdio"],
                "env": { "CODEX_TOKEN": "keep-me" },
                "defaultMode": "auto-review"
            })),
        },
    )
    .await
    .expect("runtime/profile/write");
    assert_eq!(write["profile"]["id"], "codex");
    assert_eq!(write["profile"]["generated"], false);
    assert_eq!(write["profile"]["enabled"], false);
    assert_eq!(write["profile"]["defaultMode"], "auto-review");

    let edited = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-2-edit")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "codex",
                "target": "profile",
                "runtime": "codex",
                "enabled": false,
                "label": "Codex edited",
                "command": "codex",
                "args": ["app-server", "--stdio"],
                "defaultMode": "auto-review"
            })),
        },
    )
    .await
    .expect("runtime/profile/write editor round-trip");
    let config_text = std::fs::read_to_string(
        edited["path"]
            .as_str()
            .expect("runtime profile config path"),
    )
    .expect("runtime profile config");
    assert!(config_text.contains("CODEX_TOKEN"), "{config_text}");
    assert!(config_text.contains("keep-me"), "{config_text}");

    let health = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-3")),
            method: "runtime/health/check".to_string(),
            params: Some(json!({ "runtimeRef": "codex" })),
        },
    )
    .await
    .expect("runtime/health/check");
    assert_eq!(health["health"]["status"], "disabled");
    assert!(health["health"]["checkedAtMs"].as_i64().is_some());

    let options = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-4")),
            method: "runtime/options".to_string(),
            params: Some(json!({
                "scope": default_resolved_scope(&state, &AuthContext::Bearer)
                    .expect("scope")
                    .to_wire_scope(),
                "runtimeRef": "codex"
            })),
        },
    )
    .await
    .expect("runtime/options");
    assert_eq!(options["runtimeRef"], "codex");
    assert_eq!(options["options"], json!([]));
}

#[tokio::test]
async fn direct_runtime_failure_keeps_immutable_binding_and_never_falls_back_native() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[runtime_profiles.opencode]
runtime = "opencode"
command = "definitely-missing-opencode-for-gateway-test"
args = ["serve"]
"#,
    )
    .expect("runtime profile");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-turn")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "runtimeRef": "opencode",
                "input": [{"type": "text", "text": "use direct opencode"}]
            })),
        },
    )
    .await
    .expect("direct runtime turn accepted");
    let thread_id = accepted["threadId"]
        .as_str()
        .expect("thread id")
        .to_string();
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let terminals = state
                .inner
                .state
                .store()
                .list_gateway_turn_terminals_for_thread(&thread_id)
                .expect("terminals");
            if let Some(terminal) = terminals.into_iter().next() {
                break terminal;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("direct runtime terminal timeout");
    assert_eq!(terminal.status, "failed");
    assert!(
        terminal
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("OpenCode") || message.contains("opencode")),
        "{terminal:?}"
    );
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)
        .expect("runtime binding")
        .expect("binding exists before direct launch");
    assert_eq!(binding.runtime_ref.as_deref(), Some("opencode"));
    assert_eq!(binding.backend_kind.as_deref(), Some("runtime"));
    assert!(binding.native_session_id.is_none());
    assert!(
        state
            .inner
            .state
            .store()
            .load_messages(&thread_id)
            .expect("messages")
            .is_empty(),
        "native execution must not receive the direct-runtime prompt"
    );
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&scope.source)
            .expect("source binding"),
        Some(thread_id)
    );
}

#[tokio::test]
async fn runtime_profile_registry_keeps_direct_and_generated_acp_identity_distinct() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
command = "opencode"
args = ["acp"]
entrypoints = ["peer", "subagent"]
"#,
    )
    .expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

    let profiles = runtime_profile_list_result(&state, &scope).expect("profiles");
    let direct = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "opencode")
        .expect("direct profile");
    let compatibility = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "acp:opencode")
        .expect("ACP compatibility profile");
    assert_eq!(direct.runtime, "opencode");
    assert_eq!(direct.provenance, "Direct");
    assert_eq!(compatibility.runtime, "acp");
    assert_eq!(compatibility.backend_ref.as_deref(), Some("opencode"));
    assert_eq!(compatibility.label, "opencode (ACP)");
    assert!(
        resolve_runtime_ref_peer_turn(&state, &scope, "opencode")
            .expect("direct resolution")
            .is_none()
    );
    assert!(
        resolve_runtime_ref_peer_turn(&state, &scope, "acp:opencode")
            .expect("ACP resolution")
            .is_some()
    );
}

#[test]
fn thread_and_workbench_projection_restore_persisted_direct_runtime_identity() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (profile_config_json, profile_fingerprint, profile_revision) =
        bound_runtime_profile_fixture("codex");
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&scope.cwd, "web", "pending", "pending", None)
        .expect("thread");
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref: "codex",
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("codex-native-projection"),
            cwd: &scope.cwd.display().to_string(),
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_config_json,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("runtime binding");

    let snapshot = thread_snapshot(&state, &scope, Some(&thread_id)).expect("snapshot");
    assert_eq!(snapshot["thread"]["backend"]["kind"], "runtime");
    assert_eq!(snapshot["thread"]["backend"]["runtimeRef"], "codex");
    let session_handle = crate::runtime_session_handle(
        "codex",
        &scope.cwd,
        "codex-native-projection",
    );
    assert_eq!(snapshot["thread"]["backend"]["sessionHandle"], session_handle);
    assert_ne!(
        snapshot["thread"]["backend"]["sessionHandle"],
        "codex-native-projection"
    );
    let controls =
        workbench_controls_value(&state, &scope.cwd, Some(&thread_id)).expect("workbench controls");
    assert_eq!(controls.runtime_ref, "codex");
}

#[tokio::test]
async fn backend_list_auto_creates_detected_local_acp_backends() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    write_command_shim(&bin.path().join("opencode"));
    write_command_shim(&bin.path().join("hermes"));
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    let records = backends["backends"].as_array().expect("backends");
    let opencode = records
        .iter()
        .find(|backend| backend["id"] == "opencode")
        .expect("opencode backend");
    assert_eq!(opencode["command"], "opencode");
    assert_eq!(opencode["args"], json!(["acp"]));
    assert_eq!(opencode["sourceTargets"], json!(["profile"]));
    assert_eq!(opencode["entrypoints"], json!(["peer", "subagent"]));
    let hermes = records
        .iter()
        .find(|backend| backend["id"] == "hermes")
        .expect("hermes backend");
    assert_eq!(hermes["command"], "hermes");
    assert_eq!(hermes["args"], json!(["acp"]));
    assert_eq!(hermes["sourceTargets"], json!(["profile"]));

    let config = std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config");
    assert!(config.contains("[agents.backends.opencode]"));
    assert!(config.contains("[agents.backends.hermes]"));
    assert!(config.contains("command = \"opencode\""));
    assert!(config.contains("command = \"hermes\""));

    let agents = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "agent/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("agent/list");
    let agent_records = agents["agents"].as_array().expect("agents");
    assert!(
        agent_records
            .iter()
            .any(|agent| agent["name"] == "opencode")
    );
    assert!(agent_records.iter().any(|agent| agent["name"] == "hermes"));
}

#[tokio::test]
async fn backend_list_does_not_auto_create_over_existing_effective_backend() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    write_command_shim(&bin.path().join("hermes"));
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
    let project_config = state.inner.cwd.join(".psychevo").join("config.toml");
    std::fs::create_dir_all(project_config.parent().expect("project config parent"))
        .expect("project config dir");
    std::fs::write(
        &project_config,
        r#"[agents.backends.hermes]
kind = "acp"
label = "Project Hermes"
command = "custom-hermes"
args = ["serve-acp"]
"#,
    )
    .expect("project config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    let hermes = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .find(|backend| backend["id"] == "hermes")
        .expect("hermes backend");
    assert_eq!(hermes["label"], "Project Hermes");
    assert_eq!(hermes["command"], "custom-hermes");
    assert_eq!(hermes["args"], json!(["serve-acp"]));
    assert_eq!(hermes["sourceTargets"], json!(["project"]));
    assert!(!state.inner.home.join("config.toml").exists());
}

fn write_command_shim(path: &Path) {
    std::fs::write(path, "#!/bin/sh\n").expect("command shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("command shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("command shim permissions");
    }
}

#[tokio::test]
async fn agent_rpc_manages_project_profile_disabled_and_raw_definitions() {
    let (_temp, state) = web_state();
    let project_agents = state.inner.cwd.join(".psychevo/agents");
    let profile_agents = state.inner.home.join("agents");
    std::fs::create_dir_all(&project_agents).expect("project agents");
    std::fs::create_dir_all(&profile_agents).expect("profile agents");
    std::fs::write(
        project_agents.join("review.md"),
        "---\ndescription: Project review\nenabled: false\nmodel: keep-model\n---\nProject body.\n",
    )
    .expect("project agent");
    std::fs::write(
        profile_agents.join("review.md"),
        "---\ndescription: Profile review\n---\nProfile body.\n",
    )
    .expect("profile agent");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "agent/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("agent/list");
    let active = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("active review");
    assert_eq!(active["description"], "Profile review");
    assert_eq!(active["enabled"], true);
    assert_eq!(active["target"], "profile");
    let disabled = list["disabledAgents"]
        .as_array()
        .expect("disabled agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("disabled project review");
    assert_eq!(disabled["target"], "project");
    assert_eq!(disabled["mutable"], true);

    let read_project = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "agent/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "review",
                "target": "project"
            })),
        },
    )
    .await
    .expect("agent/read project");
    assert_eq!(read_project["agent"]["enabled"], false);
    assert!(
        read_project["rawMarkdown"]
            .as_str()
            .expect("raw")
            .contains("model: keep-model")
    );

    let write_project = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "review",
                "target": "project",
                "description": "Project review updated",
                "enabled": true,
                "instructions": "Updated body.",
                "entrypoints": ["subagent"],
                "tools": ["read"],
                "mcpServers": ["repo"],
                "optionalContributions": ["tools", "mcp"]
            })),
        },
    )
    .await
    .expect("agent/write project");
    assert_eq!(write_project["agent"]["enabled"], true);
    assert_eq!(write_project["agent"]["target"], "project");
    assert_eq!(
        write_project["agent"]["optionalContributions"],
        json!(["tools", "mcp"])
    );
    let text = std::fs::read_to_string(project_agents.join("review.md")).expect("project text");
    assert!(text.contains("model: keep-model"));
    assert!(text.contains("enabled: true"));
    assert!(text.contains("mcpServers"));
    assert!(text.contains("optionalContributions"));

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "agent/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("agent/list after enable");
    let active = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("project active review");
    assert_eq!(active["description"], "Project review updated");
    assert_eq!(active["target"], "project");
    assert_eq!(
        active["contributions"],
        json!(["instructions", "tools", "mcp"])
    );
    assert!(
        list["shadowedAgents"]
            .as_array()
            .expect("shadowed")
            .iter()
            .any(|agent| agent["name"] == "review" && agent["target"] == "profile")
    );

    let invalid_raw = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("5")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "description": "Ignored",
                "rawMarkdown": "---\nname: other\ndescription: Other\n---\nOther.\n"
            })),
        },
    )
    .await;
    assert!(invalid_raw.is_err());

    let raw = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("6")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "description": "Ignored",
                "rawMarkdown": "---\nname: raw-agent\ndescription: Raw agent\nenabled: false\n---\nRaw body.\n"
            })),
        },
    )
    .await
    .expect("raw write");
    assert_eq!(raw["agent"]["enabled"], false);
    assert_eq!(raw["target"], "profile");

    let enabled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("7")),
            method: "agent/setEnabled".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "enabled": true
            })),
        },
    )
    .await
    .expect("set enabled");
    assert_eq!(enabled["agent"]["enabled"], true);

    let deleted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("8")),
            method: "agent/delete".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile"
            })),
        },
    )
    .await
    .expect("delete raw");
    assert_eq!(deleted["deleted"], true);
}

#[tokio::test]
async fn team_rpc_round_trips_project_definition() {
    let (_temp, state, _snapshots) = web_state_with_snapshot_runtime();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("catalog")),
            method: "runtime/snapshot".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.clone()
            })),
        },
    )
    .await
    .expect("cache Codex model catalog");

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project",
                "description": "Ship changes",
                "enabled": true,
                "leader": "general",
                "maxParallelAgents": 8,
                "members": [
                    {"id": "researcher", "agent": "general", "role": "research"},
                    {
                        "id": "tester",
                        "agent": "general",
                        "runtimeRef": "codex",
                        "runtimeOptions": {"model": "gpt-fixture", "mode": "auto-review"},
                        "maxTurns": 2
                    }
                ],
                "instructions": "Coordinate the release."
            })),
        },
    )
    .await
    .expect("team/write");
    assert_eq!(written["team"]["name"], "ship");
    assert_eq!(written["team"]["maxParallelAgents"], 4);
    assert_eq!(written["team"]["members"][1]["runtimeRef"], "codex");
    assert!(
        written["team"]["members"][1]["runtimeProfileRevision"]
            .as_str()
            .and_then(|revision| revision.parse::<u64>().ok())
            .is_some_and(|revision| revision > 0)
    );
    assert_eq!(
        written["team"]["members"][1]["runtimeOptions"]["mode"],
        "auto-review"
    );

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "team/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("team/list");
    assert!(
        list["teams"]
            .as_array()
            .expect("teams")
            .iter()
            .any(|team| team["name"] == "ship" && team["target"] == "project")
    );

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "team/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project"
            })),
        },
    )
    .await
    .expect("team/read");
    assert_eq!(read["team"]["leader"], "general");
    assert_eq!(read["team"]["members"][1]["runtimeRef"], "codex");
    assert_eq!(
        read["team"]["members"][1]["runtimeProfileRevision"],
        written["team"]["members"][1]["runtimeProfileRevision"]
    );
    assert!(
        read["rawMarkdown"]
            .as_str()
            .expect("raw")
            .contains("runtimeOptions")
    );
    assert!(
        read["rawMarkdown"]
            .as_str()
            .expect("raw")
            .contains("runtimeProfileRevision")
    );

    let disabled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "team/setEnabled".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project",
                "enabled": false
            })),
        },
    )
    .await
    .expect("team/setEnabled");
    assert_eq!(disabled["team"]["enabled"], false);

    let deleted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("5")),
            method: "team/delete".to_string(),
            params: Some(json!({
                "scope": scope,
                "name": "ship",
                "target": "project"
            })),
        },
    )
    .await
    .expect("team/delete");
    assert_eq!(deleted["deleted"], true);
}

#[tokio::test]
async fn team_write_fails_closed_for_unknown_profiles_pairings_and_overrides() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let unknown = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("unknown")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "unknown-runtime",
                "target": "project",
                "description": "Must not guess",
                "leader": "general",
                "members": [{"id": "reviewer", "agent": "general", "runtimeRef": "missing"}]
            })),
        },
    )
    .await
    .expect_err("unknown Runtime Profile must fail");
    assert!(unknown.to_string().contains("unknown Runtime Profile `missing`"));

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("agent")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "restricted-reviewer",
                "target": "project",
                "description": "Requires a tool policy",
                "entrypoints": ["subagent"],
                "tools": ["read"],
                "instructions": "Review carefully."
            })),
        },
    )
    .await
    .expect("write restricted Agent Definition");
    let incompatible = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("pairing")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "bad-pairing",
                "target": "project",
                "description": "Must preserve required contributions",
                "leader": "general",
                "members": [{"id": "reviewer", "agent": "restricted-reviewer", "runtimeRef": "codex"}]
            })),
        },
    )
    .await
    .expect_err("required contribution must fail");
    assert!(incompatible.to_string().contains("cannot faithfully inject"));

    for (key, value) in [
        ("model", "gpt-fixture"),
        ("effort", "high"),
        ("personality", "pragmatic"),
        ("serviceTier", "fast"),
    ] {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(format!("uncached-{key}"))),
                method: "team/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "name": format!("uncached-{}", key.replace('.', "-").to_ascii_lowercase()),
                    "target": "project",
                    "description": "Must require an observed selectable choice",
                    "leader": "general",
                    "members": [{
                        "id": "reviewer",
                        "agent": "general",
                        "runtimeRef": "codex",
                        "runtimeOptions": BTreeMap::from([(key, value)])
                    }]
                })),
            },
        )
        .await
        .expect_err("uncached Codex advanced option must fail");
        assert!(
            error.to_string().contains("cached"),
            "unexpected error for {key}: {error:?}"
        );
    }

    for key in ["summary", "feature.fast", "outputSchema"] {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(format!("unsupported-{key}"))),
                method: "team/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "name": format!("unsupported-{}", key.replace('.', "-").to_ascii_lowercase()),
                    "target": "project",
                    "description": "Must reject fields absent from the stable catalog",
                    "leader": "general",
                    "members": [{
                        "id": "reviewer",
                        "agent": "general",
                        "runtimeRef": "codex",
                        "runtimeOptions": BTreeMap::from([(key, "fixture")])
                    }]
                })),
            },
        )
        .await
        .expect_err("non-catalog Codex Team option must fail");
        assert!(
            error
                .to_string()
                .contains("unsupported by the stable catalog-backed Team contract"),
            "unexpected error for {key}: {error:?}"
        );
    }

    for (name, runtime_ref, runtime_options, expected) in [
        (
            "bad-mode",
            "codex",
            json!({"mode": "turbo"}),
            "Codex mode `turbo` is unsupported",
        ),
        (
            "advanced-mode",
            "codex",
            json!({"mode": "plan"}),
            "Codex plan mode requires GUI Advanced interaction exposure",
        ),
        (
            "bad-model",
            "opencode",
            json!({"model": "gpt-5"}),
            "must use a provider/model id",
        ),
        (
            "bad-safety",
            "codex",
            json!({"sandbox": "danger-full-access"}),
            "safety override `sandbox` is not an exact selectable runtime control",
        ),
    ] {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(name)),
                method: "team/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "name": name,
                    "target": "project",
                    "description": "Must validate overrides",
                    "leader": "general",
                    "members": [{
                        "id": "reviewer",
                        "agent": "general",
                        "runtimeRef": runtime_ref,
                        "runtimeOptions": runtime_options
                    }]
                })),
            },
        )
        .await
        .expect_err("unsupported Team override must fail");
        assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
    }
}

#[tokio::test]
async fn team_write_accepts_only_exact_cached_selectable_codex_advanced_choices() {
    let (_temp, state, _snapshots) = web_state_with_snapshot_runtime();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("advanced-option-snapshot")),
            method: "runtime/snapshot".to_string(),
            params: Some(json!({
                "runtimeRef": "codex",
                "scope": scope.clone()
            })),
        },
    )
    .await
    .expect("cache exact Codex advanced controls");

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("exact-advanced-options")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "exact-advanced-options",
                "target": "project",
                "description": "Uses only observed choices",
                "leader": "general",
                "members": [{
                    "id": "reviewer",
                    "agent": "general",
                    "runtimeRef": "codex",
                    "runtimeOptions": {
                        "model": "gpt-fixture",
                        "mode": "auto-review",
                        "effort": "high",
                        "personality": "pragmatic",
                        "serviceTier": "fast"
                    }
                }]
            })),
        },
    )
    .await
    .expect("exact cached advanced choices");
    assert_eq!(
        written["team"]["members"][0]["runtimeOptions"],
        json!({
            "model": "gpt-fixture",
            "mode": "auto-review",
            "effort": "high",
            "personality": "pragmatic",
            "serviceTier": "fast"
        })
    );

    for (name, runtime_options, expected) in [
        (
            "wrong-choice",
            json!({"effort": "ultra"}),
            "requires an exact selectable control choice",
        ),
        (
            "unknown-model",
            json!({"model": "gpt-invented"}),
            "requires an exact selectable control choice",
        ),
        (
            "different-model-dependent-choice",
            json!({"model": "gpt-fixture-mini", "effort": "high"}),
            "cached controls for the same effective model",
        ),
        (
            "summary-without-catalog",
            json!({"summary": "concise"}),
            "unsupported by the stable catalog-backed Team contract",
        ),
        (
            "arbitrary-feature",
            json!({"feature.fast": "true"}),
            "unsupported by the stable catalog-backed Team contract",
        ),
    ] {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(name)),
                method: "team/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "name": name,
                    "target": "project",
                    "description": "Must reject a non-exact advanced choice",
                    "leader": "general",
                    "members": [{
                        "id": "reviewer",
                        "agent": "general",
                        "runtimeRef": "codex",
                        "runtimeOptions": runtime_options
                    }]
                })),
            },
        )
        .await
        .expect_err("non-exact cached advanced choice must fail");
        assert!(
            error.to_string().contains(expected),
            "unexpected error: {error:?}"
        );
    }
}

#[test]
fn backend_doctor_resolves_windows_pathext_command_shim() {
    let temp = tempfile::tempdir().expect("temp");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let shim = bin.join("opencode.cmd");
    std::fs::write(&shim, "@echo off\n").expect("shim");
    let backend = AgentBackendConfig {
        id: "opencode".to_string(),
        kind: psychevo_runtime::AgentBackendKind::Acp,
        enabled: true,
        label: "OpenCode".to_string(),
        description: None,
        command: Some("opencode".to_string()),
        args: vec!["acp".to_string()],
        env: BTreeMap::new(),
        cwd: "invocation".to_string(),
        entrypoints: [AgentEntrypoint::Peer].into_iter().collect(),
        client_capabilities: std::collections::BTreeSet::new(),
        mcp_servers: std::collections::BTreeSet::new(),
    };
    let env = BTreeMap::from([
        ("PATH".to_string(), bin.display().to_string()),
        ("PATHEXT".to_string(), ".CMD".to_string()),
    ]);

    let result = super::agents::backend_doctor_value_for_platform(
        &backend,
        &env,
        temp.path(),
        HostPlatform::Windows,
    )
    .expect("doctor");
    let command = result
        .checks
        .iter()
        .find(|check| check.name == "command")
        .expect("command check");

    assert!(command.ok);
    assert_eq!(command.message, "command resolved");
    assert_eq!(
        command.path.as_deref(),
        Some(shim.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn backend_profile_write_uses_explicit_config_when_set() {
    let temp = tempfile::tempdir().expect("tempdir");
    let explicit_config = temp.path().join("explicit").join("config.toml");
    let (_state_temp, state) = web_state_with_env(BTreeMap::from([(
        "PSYCHEVO_CONFIG".to_string(),
        explicit_config.to_string_lossy().to_string(),
    )]));
    let (tx, _rx) = mpsc::unbounded_channel();

    let write = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "minimal-acp",
                "target": "profile",
                "command": "minimal-agent",
                "entrypoints": ["peer", "subagent"],
                "clientCapabilities": ["fs.read", "fs.write", "terminal"]
            })),
        },
    )
    .await
    .expect("backend/write");
    assert_eq!(write["backend"]["id"], "minimal-acp");
    assert_eq!(write["backend"]["label"], "minimal-acp");
    assert_eq!(write["backend"]["description"], Value::Null);

    let backends = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    assert!(backends["backends"].as_array().is_some_and(|backends| {
        backends
            .iter()
            .any(|backend| backend["id"] == "minimal-acp")
    }));
}
