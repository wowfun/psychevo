use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psychevo_runtime_host::{
    CodexRuntimeModule, ControlState, ExecuteRequest, ExecuteResult, HistoryFidelity,
    ReadinessStatus, RuntimeAuthOperation, RuntimeAuthRequest, RuntimeCompactionRequest,
    RuntimeCompactionStatus, RuntimeControl, RuntimeControlSetRequest, RuntimeErrorStage,
    RuntimeExtensionRequest, RuntimeGoalStatus, RuntimeIntent, RuntimeInteractionExposure,
    RuntimeInteractionResponse, RuntimeKind, RuntimeModule, RuntimeObservation, RuntimeObserver,
    RuntimePlanStepStatus, RuntimeProfile, RuntimeSessionOperation, RuntimeSessionRequest,
    RuntimeSessionResult, RuntimeStability, RuntimeTurnOutcome, RuntimeTurnRequest,
    SessionOwnership, ShutdownMode, SnapshotMode, SnapshotQuery, SnapshotScope,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::mpsc;

struct Harness {
    _temp: TempDir,
    executable: PathBuf,
    log: PathBuf,
}

#[tokio::test]
async fn codex_managed_login_uses_the_stable_account_contract_without_echoing_credentials() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("ordering");
    let result = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: profile.revision,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Auth(RuntimeAuthRequest {
                    operation: RuntimeAuthOperation::LoginChatgpt,
                    cwd: harness._temp.path().to_path_buf(),
                }),
                profile,
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("managed login start");
    let ExecuteResult::Auth(result) = result else {
        panic!("auth result");
    };
    assert!(result.accepted);
    assert_eq!(result.status, "login_pending");
    assert_eq!(result.output["loginId"], "login-fixture");
    assert_eq!(result.output["authUrl"], "https://chatgpt.example/login");
    let log = harness.log();
    assert!(log.contains("\"method\":\"account/login/start\""));
    assert!(log.contains("\"type\":\"chatgpt\""));
    assert!(!log.contains("apiKey"));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn codex_stable_path_rejects_experimental_post_bind_control_mutation() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("ordering");
    let error = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: profile.revision,
                expected_capability_revision: Some(1),
                expected_binding_revision: Some(1),
                intent: RuntimeIntent::Control(RuntimeControlSetRequest {
                    thread_id: "gateway-thread-1".to_string(),
                    native_session_id: "native-1".to_string(),
                    cwd: harness._temp.path().to_path_buf(),
                    control_id: "model".to_string(),
                    value: json!("gpt-next"),
                }),
                profile,
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("stable Codex path must not use thread/settings/update");
    assert_eq!(error.code, "codex_control_mutation_experimental");
    assert!(
        error
            .message
            .contains("thread/settings/update is experimental")
    );
    assert!(
        !harness.log.exists(),
        "unsupported control must not spawn Codex"
    );
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp
            .path()
            .join(format!("fake-codex{}", std::env::consts::EXE_SUFFIX));
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex_app_server.rs");
        let status = Command::new("rustc")
            .arg("--edition=2024")
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .status()
            .expect("compile fake Codex app-server");
        assert!(status.success());
        let log = temp.path().join("requests.jsonl");
        Self {
            _temp: temp,
            executable,
            log,
        }
    }

    fn profile(&self, scenario: &str) -> RuntimeProfile {
        RuntimeProfile {
            id: format!("codex-{scenario}"),
            label: "Codex fixture".to_string(),
            kind: RuntimeKind::Codex,
            enabled: true,
            command: Some(self.executable.to_string_lossy().to_string()),
            args: vec!["app-server".to_string(), "--stdio".to_string()],
            env: BTreeMap::from([
                ("CODEX_FAKE_SCENARIO".to_string(), scenario.to_string()),
                (
                    "CODEX_FAKE_LOG".to_string(),
                    self.log.to_string_lossy().to_string(),
                ),
                (
                    "CODEX_FAKE_CWD".to_string(),
                    self._temp.path().to_string_lossy().to_string(),
                ),
            ]),
            backend_ref: None,
            default_model: Some("gpt-fixture".to_string()),
            default_mode: None,
            default_agent: None,
            approval_mode: Some("on-request".to_string()),
            sandbox: Some("workspace-write".to_string()),
            workspace_roots: Vec::new(),
            options: Value::Null,
            revision: 1,
            fingerprint: format!("fixture-{scenario}"),
        }
    }

    fn request(&self, scenario: &str, index: usize) -> ExecuteRequest {
        let profile = self.profile(scenario);
        ExecuteRequest {
            expected_profile_revision: profile.revision,
            expected_capability_revision: Some(1),
            expected_binding_revision: Some(1),
            intent: RuntimeIntent::Turn(RuntimeTurnRequest {
                turn_id: format!("gateway-turn-{index}"),
                thread_id: "gateway-thread-1".to_string(),
                native_session_id: (index > 1).then(|| "native-1".to_string()),
                cwd: self._temp.path().to_path_buf(),
                prompt: format!("prompt {index}"),
                instructions: None,
                model: None,
                mode: None,
                agent: None,
                features: BTreeMap::new(),
                interaction_exposure: RuntimeInteractionExposure::Standard,
                binding_epoch: 1,
            }),
            profile: profile.clone(),
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

async fn execute_session(
    module: &CodexRuntimeModule,
    profile: &RuntimeProfile,
    cwd: &Path,
    operation: RuntimeSessionOperation,
    native_session_id: Option<&str>,
    argument: Option<Value>,
) -> RuntimeSessionResult {
    let result = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: profile.revision,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Session(RuntimeSessionRequest {
                    operation,
                    thread_id: Some("gateway-thread-1".to_string()),
                    native_session_id: native_session_id.map(str::to_string),
                    cwd: cwd.to_path_buf(),
                    cursor: None,
                    argument,
                }),
                profile: profile.clone(),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("session operation");
    let ExecuteResult::Session(result) = result else {
        panic!("session result");
    };
    result
}

async fn execute_extension(
    module: &CodexRuntimeModule,
    profile: &RuntimeProfile,
    namespace: &str,
    operation: &str,
    argument: Option<Value>,
) -> Value {
    let result = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: profile.revision,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Extension(RuntimeExtensionRequest {
                    namespace: namespace.to_string(),
                    operation: operation.to_string(),
                    argument,
                }),
                profile: profile.clone(),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("extension operation");
    let ExecuteResult::Extension(value) = result else {
        panic!("extension result");
    };
    value
}

fn compaction_request(
    harness: &Harness,
    scenario: &str,
    instructions: Option<&str>,
) -> ExecuteRequest {
    let profile = harness.profile(scenario);
    ExecuteRequest {
        expected_profile_revision: profile.revision,
        expected_capability_revision: Some(1),
        expected_binding_revision: Some(1),
        intent: RuntimeIntent::Compaction(RuntimeCompactionRequest {
            thread_id: "gateway-thread-1".to_string(),
            native_session_id: "native-1".to_string(),
            cwd: harness._temp.path().to_path_buf(),
            instructions: instructions.map(str::to_string),
        }),
        profile,
    }
}

async fn wait_for_log(harness: &Harness, needle: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if harness.log().contains(needle) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fake Codex request log timeout");
}

#[tokio::test]
async fn cached_snapshot_does_not_spawn_and_early_notifications_are_replayed() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("ordering");
    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: profile.clone(),
            scope: SnapshotScope::Profile,
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("snapshot");
    assert!(snapshot.process_epoch.is_none());
    assert!(snapshot.controls.is_empty());
    assert_eq!(snapshot.stability, RuntimeStability::Stable);
    assert_eq!(
        snapshot
            .capabilities
            .iter()
            .find(|capability| capability.id == "auth.login")
            .map(|capability| (capability.enabled, capability.stability)),
        Some((false, RuntimeStability::Stable))
    );
    assert_eq!(
        snapshot
            .capabilities
            .iter()
            .find(|capability| capability.id == "interaction.question")
            .map(|capability| capability.stability),
        Some(RuntimeStability::Experimental)
    );
    for capability_id in [
        "thread.compact",
        "thread.goal.read",
        "thread.goal.set",
        "thread.goal.clear",
        "thread.usage",
        "account.rate_limits.read",
        "timeline.plan",
        "timeline.diff",
    ] {
        assert_eq!(
            snapshot
                .capabilities
                .iter()
                .find(|capability| capability.id == capability_id)
                .map(|capability| (capability.enabled, capability.stability)),
            Some((false, RuntimeStability::Stable)),
            "unhydrated stable capability {capability_id}"
        );
    }
    assert!(!harness.log.exists());

    let deltas = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&deltas);
    let native_bindings = Arc::new(Mutex::new(Vec::new()));
    let captured_bindings = Arc::clone(&native_bindings);
    let request_log = harness.log.clone();
    let observer = RuntimeObserver::new(move |event| {
        if let RuntimeObservation::TextDelta { text, .. } = event {
            observed.lock().expect("deltas").push(text);
        }
    })
    .with_session_binder(move |binding| {
        let captured_bindings = Arc::clone(&captured_bindings);
        let request_log = request_log.clone();
        async move {
            let log = std::fs::read_to_string(request_log).unwrap_or_default();
            assert!(log.contains("\"method\":\"thread/start\""));
            assert!(!log.contains("\"method\":\"turn/start\""));
            captured_bindings
                .lock()
                .expect("native bindings")
                .push(binding);
            Ok(())
        }
    });
    let result = module
        .execute(
            harness.request("ordering", 1),
            observer,
            RuntimeControl::default(),
        )
        .await
        .expect("turn");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.outcome, RuntimeTurnOutcome::Completed);
    assert_eq!(turn.final_answer, "hello");
    assert_eq!(turn.history_fidelity, HistoryFidelity::Partial);
    assert_eq!(&*deltas.lock().expect("deltas"), &["hel"]);
    {
        let bindings = native_bindings.lock().expect("native bindings");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].native_session_id, "native-1");
    }
    let log = harness.log();
    assert!(log.contains("\"method\":\"initialize\""));
    assert!(log.contains("\"method\":\"initialized\""));
    assert!(log.contains("\"clientUserMessageId\":\"gateway-turn-1\""));
    assert!(!log.contains("\"jsonrpc\""));
    let session_snapshot = module
        .snapshot(SnapshotQuery {
            profile: profile.clone(),
            scope: SnapshotScope::Session {
                cwd: harness._temp.path().to_path_buf(),
                thread_id: "gateway-thread-1".to_string(),
                native_session_id: Some("native-1".to_string()),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("session snapshot");
    assert_eq!(session_snapshot.controls.len(), 1);
    assert_eq!(session_snapshot.controls[0].id, "model");
    assert_eq!(
        session_snapshot.controls[0].state,
        ControlState::ReadOnlyCurrent
    );
    assert_eq!(
        session_snapshot.controls[0].current_value,
        Some(json!("gpt-fixture"))
    );
    for stage_id in [
        "configuration",
        "transport",
        "version",
        "authentication",
        "catalog",
        "capabilities",
        "policy",
    ] {
        assert_eq!(
            session_snapshot
                .readiness
                .iter()
                .find(|stage| stage.id == stage_id)
                .map(|stage| stage.status),
            Some(ReadinessStatus::Ready),
            "ready stage {stage_id}"
        );
    }
    assert!(
        session_snapshot
            .capabilities
            .iter()
            .filter(|capability| capability.stability == RuntimeStability::Stable)
            .all(|capability| capability.enabled)
    );
    let workspace_snapshot = module
        .snapshot(SnapshotQuery {
            profile,
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("workspace snapshot");
    let model = workspace_snapshot
        .controls
        .iter()
        .find(|control| control.id == "model")
        .expect("hydrated model catalog");
    assert_eq!(model.state, ControlState::Selectable);
    assert_eq!(model.current_value, None);
    assert_eq!(
        model
            .choices
            .iter()
            .map(|choice| choice.value.clone())
            .collect::<Vec<_>>(),
        vec![json!("gpt-fixture"), json!("gpt-fixture-mini")]
    );
    let effort = workspace_snapshot
        .controls
        .iter()
        .find(|control| control.id == "effort")
        .expect("catalog-backed reasoning effort");
    assert_eq!(effort.state, ControlState::Selectable);
    assert_eq!(
        effort
            .depends_on
            .as_ref()
            .map(|dependency| (dependency.control_id.as_str(), dependency.value.clone())),
        Some(("model", json!("gpt-fixture")))
    );
    assert_eq!(
        effort
            .choices
            .iter()
            .map(|choice| choice.value.clone())
            .collect::<Vec<_>>(),
        vec![json!("medium"), json!("high")]
    );
    let personality = workspace_snapshot
        .controls
        .iter()
        .find(|control| control.id == "personality")
        .expect("catalog-backed personality support");
    assert_eq!(
        personality
            .choices
            .iter()
            .map(|choice| choice.value.clone())
            .collect::<Vec<_>>(),
        vec![json!("none"), json!("friendly"), json!("pragmatic")]
    );
    let service_tier = workspace_snapshot
        .controls
        .iter()
        .find(|control| control.id == "serviceTier")
        .expect("catalog-backed service tiers");
    assert_eq!(service_tier.choices[0].value, json!("fast"));
    assert_eq!(
        service_tier
            .depends_on
            .as_ref()
            .map(|dependency| dependency.value.clone()),
        Some(json!("gpt-fixture"))
    );
    assert!(
        workspace_snapshot
            .controls
            .iter()
            .all(|control| control.id != "summary")
    );
    assert_eq!(
        workspace_snapshot
            .extension
            .as_ref()
            .and_then(|extension| { extension["codex"]["controlModel"].as_str() }),
        Some("gpt-fixture")
    );
    assert!(harness.log().contains("\"method\":\"model/list\""));

    let turn_starts_before_invalid = harness.log().matches("\"method\":\"turn/start\"").count();
    let mut stale_model_options = harness.request("ordering", 2);
    let RuntimeIntent::Turn(stale_turn) = &mut stale_model_options.intent else {
        panic!("turn request");
    };
    stale_turn.model = Some("gpt-fixture-mini".to_string());
    stale_turn
        .features
        .insert("effort".to_string(), json!("high"));
    let stale_error = module
        .execute(
            stale_model_options,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("model B must reject model A's reasoning effort");
    assert_eq!(stale_error.code, "codex_catalog_choice_unsupported");
    assert!(stale_error.message.contains("gpt-fixture-mini"));
    assert_eq!(
        harness.log().matches("\"method\":\"turn/start\"").count(),
        turn_starts_before_invalid,
        "invalid dependent choice must fail before native prompt delivery"
    );

    let mut valid_model_options = harness.request("ordering", 2);
    let RuntimeIntent::Turn(valid_turn) = &mut valid_model_options.intent else {
        panic!("turn request");
    };
    valid_turn.model = Some("gpt-fixture-mini".to_string());
    valid_turn
        .features
        .insert("effort".to_string(), json!("low"));
    valid_turn
        .features
        .insert("serviceTier".to_string(), json!("flex"));
    let valid_result = module
        .execute(
            valid_model_options,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("model B exact catalog choices");
    let ExecuteResult::Turn(valid_turn) = valid_result else {
        panic!("turn result");
    };
    assert_eq!(valid_turn.outcome, RuntimeTurnOutcome::Completed);
    let log = harness.log();
    assert!(log.contains("\"model\":\"gpt-fixture-mini\""));
    assert!(log.contains("\"effort\":\"low\""));
    assert!(log.contains("\"serviceTier\":\"flex\""));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn explicit_probe_starts_only_the_local_codex_handshake() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: harness.profile("ordering"),
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("bounded probe");

    assert!(snapshot.process_epoch.is_some());
    assert_eq!(
        snapshot
            .readiness
            .iter()
            .find(|stage| stage.id == "capabilities")
            .map(|stage| stage.status),
        Some(ReadinessStatus::Unchecked)
    );
    let log = harness.log();
    assert!(log.contains("\"method\":\"initialize\""));
    assert!(log.contains("\"method\":\"initialized\""));
    assert!(!log.contains("\"method\":\"thread/start\""));
    assert!(!log.contains("\"method\":\"thread/resume\""));
    assert!(!log.contains("\"method\":\"turn/start\""));
    assert!(!log.contains("\"method\":\"model/list\""));

    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn explicit_catalog_refresh_hydrates_model_list_without_claiming_a_stable_turn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: harness.profile("ordering"),
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::CatalogRefresh,
        })
        .await
        .expect("catalog refresh");

    let log = harness.log();
    assert!(log.contains("\"method\":\"initialize\""));
    assert!(log.contains("\"method\":\"account/read\""));
    assert!(log.contains("\"method\":\"model/list\""));
    assert!(!log.contains("\"method\":\"thread/start\""));
    assert!(!log.contains("\"method\":\"turn/start\""));
    assert_eq!(
        readiness_status(&snapshot, "catalog"),
        Some(ReadinessStatus::Ready)
    );
    assert_eq!(
        readiness_status(&snapshot, "capabilities"),
        Some(ReadinessStatus::Unchecked)
    );
    assert!(
        snapshot
            .controls
            .iter()
            .any(|control| control.id == "model")
    );
    assert!(
        snapshot
            .capabilities
            .iter()
            .find(|capability| capability.id == "turn.start")
            .is_some_and(|capability| !capability.enabled)
    );

    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn profile_default_model_starts_without_catalog_but_explicit_override_requires_one() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut explicit = harness.request("ordering", 1);
    let RuntimeIntent::Turn(turn) = &mut explicit.intent else {
        panic!("turn request");
    };
    turn.model = Some("gpt-fixture-mini".to_string());
    let error = module
        .execute(
            explicit,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("explicit model override requires catalog proof");
    assert_eq!(error.code, "codex_model_catalog_required");
    assert!(
        !harness.log.exists(),
        "invalid override must not spawn Codex"
    );

    let result = module
        .execute(
            harness.request("ordering", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("Profile default model does not require pre-refreshed catalog");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.outcome, RuntimeTurnOutcome::Completed);
    assert!(harness.log().contains("\"model\":\"gpt-fixture\""));

    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn legacy_codex_version_cannot_promote_the_complete_stable_matrix() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("legacy_version");
    let result = module
        .execute(
            harness.request("legacy_version", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("basic legacy turn");
    assert!(
        matches!(result, ExecuteResult::Turn(turn) if turn.outcome == RuntimeTurnOutcome::Completed)
    );

    let snapshot = module
        .snapshot(SnapshotQuery {
            profile,
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("legacy snapshot");
    for stage_id in ["version", "capabilities"] {
        assert_eq!(
            snapshot
                .readiness
                .iter()
                .find(|stage| stage.id == stage_id)
                .map(|stage| stage.status),
            Some(ReadinessStatus::Unsupported),
            "unsupported stage {stage_id}"
        );
    }
    assert!(
        snapshot
            .capabilities
            .iter()
            .filter(|capability| capability.stability == RuntimeStability::Stable)
            .all(|capability| !capability.enabled)
    );
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn snapshots_and_probe_workers_are_isolated_by_profile_revision() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();

    let mut old_request = harness.request("legacy_version", 1);
    old_request.profile.fingerprint = "old-fingerprint".to_string();
    let RuntimeIntent::Turn(old_turn) = &mut old_request.intent else {
        panic!("old turn");
    };
    old_turn.thread_id = "old-public-thread".to_string();
    module
        .execute(
            old_request.clone(),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("old turn");

    let mut new_request = harness.request("ordering", 2);
    new_request.profile.revision = 2;
    new_request.expected_profile_revision = 2;
    new_request.profile.fingerprint = "new-fingerprint".to_string();
    let RuntimeIntent::Turn(new_turn) = &mut new_request.intent else {
        panic!("new turn");
    };
    new_turn.thread_id = "new-public-thread".to_string();
    module
        .execute(
            new_request.clone(),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("new turn");

    let old_snapshot = module
        .snapshot(SnapshotQuery {
            profile: old_request.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("old snapshot");
    let new_snapshot = module
        .snapshot(SnapshotQuery {
            profile: new_request.profile.clone(),
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("new snapshot");
    assert_eq!(
        readiness_status(&old_snapshot, "version"),
        Some(ReadinessStatus::Unsupported)
    );
    assert_eq!(
        readiness_status(&new_snapshot, "version"),
        Some(ReadinessStatus::Ready)
    );

    module
        .snapshot(SnapshotQuery {
            profile: old_request.profile,
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("old probe");
    module
        .snapshot(SnapshotQuery {
            profile: new_request.profile,
            scope: SnapshotScope::Workspace {
                cwd: harness._temp.path().to_path_buf(),
            },
            mode: SnapshotMode::BoundedProbe,
        })
        .await
        .expect("new probe rotates instead of stale-revision failure");
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn runtime_scoped_shutdown_disposes_every_selected_thread_worker() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let selected_profile = harness.profile("ordering");

    for (turn_id, thread_id) in [
        ("selected-turn-1", "selected-thread-1"),
        ("selected-turn-2", "selected-thread-2"),
    ] {
        let mut request = harness.request("ordering", 1);
        let RuntimeIntent::Turn(turn) = &mut request.intent else {
            panic!("turn request");
        };
        turn.turn_id = turn_id.to_string();
        turn.thread_id = thread_id.to_string();
        module
            .execute(
                request,
                RuntimeObserver::default(),
                RuntimeControl::default(),
            )
            .await
            .expect("selected worker turn");
    }

    let mut retained_request = harness.request("ordering", 1);
    retained_request.profile.id = "codex-retained".to_string();
    retained_request.profile.fingerprint = "retained-profile".to_string();
    let retained_profile = retained_request.profile.clone();
    let RuntimeIntent::Turn(turn) = &mut retained_request.intent else {
        panic!("turn request");
    };
    turn.turn_id = "retained-turn".to_string();
    turn.thread_id = "retained-thread".to_string();
    module
        .execute(
            retained_request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("unselected worker turn");

    assert_eq!(
        harness.log().matches("\"method\":\"initialize\"").count(),
        3,
        "two selected thread workers and one unselected worker should exist"
    );
    module
        .shutdown(ShutdownMode::Runtime {
            kind: RuntimeKind::Codex,
            runtime_ref: Some(selected_profile.id.clone()),
            force: true,
        })
        .await
        .expect("runtime-scoped shutdown");

    let selected = module
        .snapshot(SnapshotQuery {
            profile: selected_profile,
            scope: SnapshotScope::Profile,
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("selected profile snapshot");
    let retained = module
        .snapshot(SnapshotQuery {
            profile: retained_profile,
            scope: SnapshotScope::Profile,
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("retained profile snapshot");
    assert!(
        selected.process_epoch.is_none(),
        "every worker selected from the registry should be disposed"
    );
    assert!(
        retained.process_epoch.is_some(),
        "runtime-scoped shutdown should retain non-matching workers"
    );

    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("cleanup retained worker");
}

fn readiness_status(
    snapshot: &psychevo_runtime_host::RuntimeSnapshot,
    stage_id: &str,
) -> Option<ReadinessStatus> {
    snapshot
        .readiness
        .iter()
        .find(|stage| stage.id == stage_id)
        .map(|stage| stage.status)
}

#[tokio::test]
async fn unknown_codex_policy_is_rejected_before_process_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    for (approval_mode, sandbox) in [
        (Some("granular".to_string()), None),
        (None, Some("typo-sandbox".to_string())),
    ] {
        let mut request = harness.request("ordering", 1);
        request.profile.approval_mode = approval_mode;
        request.profile.sandbox = sandbox;
        let error = module
            .execute(
                request,
                RuntimeObserver::default(),
                RuntimeControl::default(),
            )
            .await
            .expect_err("unsupported policy");
        assert_eq!(error.code, "policy_not_enforceable");
    }
    assert!(!harness.log.exists());
}

#[tokio::test]
async fn default_turns_keep_agent_instructions_on_stable_thread_fields_only() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut first = harness.request("ordering", 1);
    let RuntimeIntent::Turn(turn) = &mut first.intent else {
        panic!("turn request");
    };
    turn.instructions = Some("Stable reviewer persona.".to_string());
    module
        .execute(first, RuntimeObserver::default(), RuntimeControl::default())
        .await
        .expect("first default turn");

    let mut continued = harness.request("ordering", 2);
    let RuntimeIntent::Turn(turn) = &mut continued.intent else {
        panic!("turn request");
    };
    turn.instructions = Some("Stable reviewer persona.".to_string());
    module
        .execute(
            continued,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("continued default turn");

    let log = harness.log();
    let thread_start = log
        .lines()
        .find(|line| line.contains("\"method\":\"thread/start\""))
        .expect("thread/start");
    assert!(thread_start.contains("\"developerInstructions\":\"Stable reviewer persona.\""));
    let turn_starts = log
        .lines()
        .filter(|line| line.contains("\"method\":\"turn/start\""))
        .collect::<Vec<_>>();
    assert_eq!(turn_starts.len(), 2);
    assert!(
        turn_starts
            .iter()
            .all(|line| !line.contains("\"collaborationMode\""))
    );
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn failed_gateway_binding_ack_prevents_codex_prompt_delivery() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let observer = RuntimeObserver::default().with_session_binder(|_| async {
        Err(psychevo_runtime_host::RuntimeError::new(
            "fake_binding_failed",
            psychevo_runtime_host::RuntimeErrorStage::Binding,
            psychevo_runtime_host::RetryClass::Never,
            "fake Gateway binding failed",
        ))
    });
    let error = module
        .execute(
            harness.request("ordering", 1),
            observer,
            RuntimeControl::default(),
        )
        .await
        .expect_err("binding acknowledgement");
    assert_eq!(error.code, "fake_binding_failed");
    let log = harness.log();
    assert!(log.contains("\"method\":\"thread/start\""));
    assert!(!log.contains("\"method\":\"turn/start\""));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn eof_during_an_accepted_turn_wakes_the_waiter_with_one_failed_result() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        module.execute(
            harness.request("eof", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        ),
    )
    .await
    .expect("turn did not hang")
    .expect("accepted turn result");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.outcome, RuntimeTurnOutcome::Failed);
    let terminal_error = turn.terminal_error.expect("typed EOF terminal error");
    assert_eq!(terminal_error.code, "process_exit");
    assert_eq!(terminal_error.stage, RuntimeErrorStage::Transport);
    assert_eq!(
        terminal_error.retry_class,
        psychevo_runtime_host::RetryClass::UnknownDelivery
    );
    assert_eq!(
        terminal_error.message,
        "Codex exited before the turn completed."
    );
    assert!(terminal_error.diagnostic_ref.starts_with("codex-process-"));
    assert_eq!(
        harness.log().matches("\"method\":\"turn/start\"").count(),
        1
    );
}

#[tokio::test]
async fn rejected_prompt_is_not_retried() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let error = module
        .execute(
            harness.request("no_retry", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("turn rejection");
    assert_eq!(error.code, "codex_rpc_error");
    assert_eq!(
        harness.log().matches("\"method\":\"turn/start\"").count(),
        1
    );
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn stale_turn_notifications_cannot_mutate_the_next_turn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    module
        .execute(
            harness.request("stale", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("first turn");
    let deltas = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&deltas);
    let result = module
        .execute(
            harness.request("stale", 2),
            RuntimeObserver::new(move |event| {
                if let RuntimeObservation::TextDelta { text, .. } = event {
                    observed.lock().expect("deltas").push(text);
                }
            }),
            RuntimeControl::default(),
        )
        .await
        .expect("second turn");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.final_answer, "second");
    assert_eq!(&*deltas.lock().expect("deltas"), &["sec"]);
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn four_native_interactions_round_trip_without_plan_approval() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("interactions");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let turn_module = module.clone();
    let turn_request = harness.request("interactions", 1);
    let turn = tokio::spawn(async move {
        turn_module
            .execute(
                turn_request,
                RuntimeObserver::new(move |event| {
                    let _ = event_tx.send(event);
                }),
                RuntimeControl::default(),
            )
            .await
    });

    let mut kinds = Vec::new();
    while kinds.len() < 4 {
        let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("interaction timeout")
            .expect("interaction stream");
        let RuntimeObservation::Interaction(interaction) = event else {
            continue;
        };
        kinds.push(interaction.kind.clone());
        let response = match interaction.kind.as_str() {
            "command" | "file_change" => json!({"decision": "accept"}),
            "permission" => json!({"decision": "acceptForSession"}),
            "question" => {
                assert!(
                    interaction.choices.is_empty(),
                    "questions must not be flattened"
                );
                assert_eq!(interaction.questions.len(), 2);
                assert_eq!(interaction.questions[0].header.as_deref(), Some("Confirm"));
                assert_eq!(interaction.questions[0].question, "Continue?");
                assert!(interaction.questions[0].custom);
                assert!(!interaction.questions[0].secret);
                assert_eq!(interaction.questions[0].options[1].label, "No");
                assert_eq!(interaction.questions[0].options[1].description, "Stop");
                assert_eq!(interaction.questions[1].question, "Anything else?");
                assert!(interaction.questions[1].custom);
                assert!(interaction.questions[1].secret);
                assert!(interaction.questions[1].options.is_empty());
                json!({"answers": [["Yes"], ["classified"]]})
            }
            other => panic!("unexpected interaction: {other}"),
        };
        let result = module
            .execute(
                ExecuteRequest {
                    profile: profile.clone(),
                    expected_profile_revision: 1,
                    expected_capability_revision: Some(1),
                    expected_binding_revision: Some(1),
                    intent: RuntimeIntent::Interaction(RuntimeInteractionResponse {
                        interaction_id: interaction.id,
                        process_epoch: interaction.process_epoch,
                        instance_epoch: None,
                        response,
                    }),
                },
                RuntimeObserver::default(),
                RuntimeControl::default(),
            )
            .await
            .expect("interaction response");
        let ExecuteResult::Interaction(result) = result else {
            panic!("interaction result");
        };
        assert!(result.accepted);
    }
    kinds.sort();
    assert_eq!(kinds, ["command", "file_change", "permission", "question"]);
    let result = turn.await.expect("turn task").expect("turn result");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.final_answer, "approved");
    let log = harness.log();
    assert!(log.contains("\"scope\":\"session\""));
    assert!(log.contains("\"confirm\":{\"answers\":[\"Yes\"]}"));
    assert!(log.contains("\"details\":{\"answers\":[\"classified\"]}"));
    assert!(!log.contains("plan"));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn native_child_permission_follows_child_identity_and_keeps_codex_session_lifetime() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("child_interaction");
    let child = Arc::new(Mutex::new(None));
    let observed_child = Arc::clone(&child);
    let first = module
        .execute(
            harness.request("child_interaction", 1),
            RuntimeObserver::new(move |event| {
                if let RuntimeObservation::ChildChanged {
                    parent_native_session_id,
                    native_session_id,
                    read_only,
                    ..
                } = event
                {
                    *observed_child.lock().expect("child observation") =
                        Some((parent_native_session_id, native_session_id, read_only));
                }
            }),
            RuntimeControl::default(),
        )
        .await
        .expect("child identity turn");
    let ExecuteResult::Turn(first) = first else {
        panic!("first turn result");
    };
    assert_eq!(first.final_answer, "child ready");
    assert_eq!(
        child.lock().expect("child observation").clone(),
        Some(("native-1".to_string(), "child-1".to_string(), true))
    );

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let turn_module = module.clone();
    let turn_request = harness.request("child_interaction", 2);
    let turn = tokio::spawn(async move {
        turn_module
            .execute(
                turn_request,
                RuntimeObserver::new(move |event| {
                    let _ = event_tx.send(event);
                }),
                RuntimeControl::default(),
            )
            .await
    });

    let interaction = loop {
        let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("child interaction timeout")
            .expect("child interaction stream");
        if let RuntimeObservation::Interaction(interaction) = event {
            break interaction;
        }
    };
    assert_eq!(interaction.native_session_id, "child-1");
    assert_eq!(
        interaction.parent_native_session_id.as_deref(),
        Some("native-1")
    );
    assert_eq!(
        interaction.child_native_session_id.as_deref(),
        Some("child-1")
    );
    assert_eq!(
        interaction.authorization_lifetime.as_deref(),
        Some("codex_session")
    );
    assert!(
        interaction
            .choices
            .iter()
            .any(|choice| choice.decision == "acceptForSession")
    );

    let response = module
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: 1,
                expected_capability_revision: Some(1),
                expected_binding_revision: Some(1),
                intent: RuntimeIntent::Interaction(RuntimeInteractionResponse {
                    interaction_id: interaction.id,
                    process_epoch: interaction.process_epoch,
                    instance_epoch: None,
                    response: json!({"decision": "acceptForSession"}),
                }),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("child interaction response");
    assert!(matches!(response, ExecuteResult::Interaction(result) if result.accepted));

    let result = turn.await.expect("turn task").expect("turn result");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.final_answer, "child approved");
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn native_children_and_imported_history_remain_read_only_and_partial() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let children = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&children);
    module
        .execute(
            harness.request("child", 1),
            RuntimeObserver::new(move |event| {
                if let RuntimeObservation::ChildChanged {
                    native_session_id,
                    read_only,
                    ..
                } = event
                {
                    observed
                        .lock()
                        .expect("children")
                        .push((native_session_id, read_only));
                }
            }),
            RuntimeControl::default(),
        )
        .await
        .expect("child turn");
    assert_eq!(
        &*children.lock().expect("children"),
        &[("child-1".to_string(), true)]
    );

    let profile = harness.profile("child");
    let list = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: 1,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Session(RuntimeSessionRequest {
                    operation: RuntimeSessionOperation::List,
                    thread_id: None,
                    native_session_id: None,
                    cwd: harness._temp.path().to_path_buf(),
                    cursor: None,
                    argument: None,
                }),
                profile: profile.clone(),
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("session list");
    let ExecuteResult::Session(list) = list else {
        panic!("session list");
    };
    let child = list
        .sessions
        .iter()
        .find(|session| session.native_session_id == "child-1")
        .expect("child session");
    assert_eq!(child.ownership, SessionOwnership::ReadOnly);
    assert_eq!(child.actions, vec!["read"]);
    let active = list
        .sessions
        .iter()
        .find(|session| session.native_session_id == "root-1")
        .expect("active root session");
    assert!(!active.archived);
    assert!(active.actions.iter().any(|action| action == "archive"));
    assert!(!active.actions.iter().any(|action| action == "unarchive"));
    let archived = list
        .sessions
        .iter()
        .find(|session| session.native_session_id == "archived-1")
        .expect("archived root session");
    assert!(archived.archived);
    assert_eq!(archived.actions, ["read", "unarchive", "delete"]);
    assert_eq!(
        harness.log().matches("\"method\":\"thread/list\"").count(),
        2
    );

    let read = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: 1,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Session(RuntimeSessionRequest {
                    operation: RuntimeSessionOperation::Read,
                    thread_id: None,
                    native_session_id: Some("child-1".to_string()),
                    cwd: harness._temp.path().to_path_buf(),
                    cursor: None,
                    argument: None,
                }),
                profile,
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("session read");
    let ExecuteResult::Session(read) = read else {
        panic!("session read");
    };
    let child = &read.sessions[0];
    assert_eq!(child.fidelity, HistoryFidelity::Partial);
    assert_eq!(child.messages.len(), 2);
    assert_eq!(
        child.messages[0].dedup_key,
        "codex:child-1:history-turn:user-1"
    );
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn native_child_turn_is_rejected_before_turn_start() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut request = harness.request("child", 2);
    let RuntimeIntent::Turn(turn) = &mut request.intent else {
        panic!("turn request");
    };
    turn.thread_id = "gateway-child-thread".to_string();
    turn.native_session_id = Some("child-1".to_string());

    let error = module
        .execute(
            request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("read-only child turn");
    assert_eq!(error.code, "codex_read_only_child");
    let log = harness.log();
    assert!(log.contains("\"method\":\"thread/resume\""));
    assert!(!log.contains("\"method\":\"turn/start\""));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn existing_native_session_resumes_and_basic_mutations_use_native_methods() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("ordering");

    let mut resumed_request = harness.request("ordering", 2);
    let RuntimeIntent::Turn(turn_request) = &mut resumed_request.intent else {
        panic!("turn request");
    };
    turn_request.instructions = Some("Stable resumed persona.".to_string());
    let turn = module
        .execute(
            resumed_request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("resumed turn");
    let ExecuteResult::Turn(turn) = turn else {
        panic!("turn result");
    };
    assert_eq!(turn.outcome, RuntimeTurnOutcome::Completed);
    assert_eq!(turn.native_session_id, "native-1");
    assert_eq!(
        harness
            .log()
            .matches("\"method\":\"thread/resume\"")
            .count(),
        1
    );
    assert!(!harness.log().contains("\"method\":\"thread/start\""));
    let resume_line = harness
        .log()
        .lines()
        .find(|line| line.contains("\"method\":\"thread/resume\""))
        .expect("thread/resume")
        .to_string();
    assert!(resume_line.contains("\"developerInstructions\":\"Stable resumed persona.\""));
    let first_turn_start = harness
        .log()
        .lines()
        .find(|line| line.contains("\"method\":\"turn/start\""))
        .expect("turn/start")
        .to_string();
    assert!(!first_turn_start.contains("\"collaborationMode\""));

    let resumed = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Resume,
        Some("native-1"),
        None,
    )
    .await;
    assert!(resumed.changed);
    assert_eq!(resumed.sessions[0].native_session_id, "native-1");

    let forked = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Fork,
        Some("native-1"),
        Some(json!({"lastTurnId": "turn-native-1"})),
    )
    .await;
    assert!(forked.changed);
    assert_eq!(forked.sessions[0].native_session_id, "fork-1");

    let renamed = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Rename,
        Some("native-1"),
        Some(json!({"title": "Renamed"})),
    )
    .await;
    assert!(renamed.changed);
    assert_eq!(renamed.sessions[0].fidelity, HistoryFidelity::Partial);

    let archived = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Archive,
        Some("native-1"),
        None,
    )
    .await;
    assert!(archived.changed);
    assert!(
        archived
            .message
            .as_deref()
            .is_some_and(|message| message.contains("descendants"))
    );

    let unarchived = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Unarchive,
        Some("native-1"),
        None,
    )
    .await;
    assert!(unarchived.changed);
    assert!(!unarchived.sessions[0].archived);

    let deleted = execute_session(
        &module,
        &profile,
        harness._temp.path(),
        RuntimeSessionOperation::Delete,
        Some("native-1"),
        None,
    )
    .await;
    assert!(deleted.changed);
    assert!(deleted.sessions.is_empty());

    let log = harness.log();
    for method in [
        "thread/fork",
        "thread/name/set",
        "thread/archive",
        "thread/unarchive",
        "thread/delete",
    ] {
        assert!(log.contains(&format!("\"method\":\"{method}\"")));
    }
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn session_mutation_rejects_a_cwd_mismatch_before_the_native_mutation() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("ordering");
    let other_cwd = harness._temp.path().join("other-workspace");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let error = module
        .execute(
            ExecuteRequest {
                expected_profile_revision: profile.revision,
                expected_capability_revision: Some(1),
                expected_binding_revision: None,
                intent: RuntimeIntent::Session(RuntimeSessionRequest {
                    operation: RuntimeSessionOperation::Archive,
                    thread_id: Some("gateway-thread-1".to_string()),
                    native_session_id: Some("native-1".to_string()),
                    cwd: other_cwd,
                    cursor: None,
                    argument: None,
                }),
                profile,
            },
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("cwd mismatch");
    assert_eq!(error.code, "codex_session_cwd_mismatch");
    let log = harness.log();
    assert!(log.contains("\"method\":\"thread/read\""));
    assert!(!log.contains("\"method\":\"thread/archive\""));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn supported_modes_and_turn_features_are_projected_to_codex_fields() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();

    let mut plan = harness.request("ordering", 1);
    let RuntimeIntent::Turn(turn) = &mut plan.intent else {
        panic!("turn request");
    };
    turn.mode = Some("plan".to_string());
    turn.interaction_exposure = RuntimeInteractionExposure::GuiAdvancedOnly;
    turn.instructions = Some("Follow the paired Agent Definition.".to_string());
    module
        .execute(plan, RuntimeObserver::default(), RuntimeControl::default())
        .await
        .expect("plan turn");

    let mut auto_review = harness.request("ordering", 2);
    let RuntimeIntent::Turn(turn) = &mut auto_review.intent else {
        panic!("turn request");
    };
    turn.mode = Some("auto-review".to_string());
    turn.features.insert("effort".to_string(), json!("high"));
    turn.features.insert(
        "outputSchema".to_string(),
        json!({"type": "object", "additionalProperties": false}),
    );
    module
        .execute(
            auto_review,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("auto-review turn");
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown first policy worker");

    let full_access_module = CodexRuntimeModule::new();
    let mut full_access = harness.request("ordering", 3);
    full_access.profile.sandbox = Some("danger-full-access".to_string());
    let RuntimeIntent::Turn(turn) = &mut full_access.intent else {
        panic!("turn request");
    };
    turn.mode = Some("full-access".to_string());
    full_access_module
        .execute(
            full_access,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("full-access turn");

    let log = harness.log();
    assert!(log.contains("\"collaborationMode\":{\"mode\":\"plan\""));
    assert!(log.contains("\"developerInstructions\":\"Follow the paired Agent Definition.\""));
    let turn_starts = log
        .lines()
        .filter(|line| line.contains("\"method\":\"turn/start\""))
        .collect::<Vec<_>>();
    assert_eq!(turn_starts.len(), 3);
    assert!(
        turn_starts[0]
            .contains("\"developer_instructions\":\"Follow the paired Agent Definition.\"")
    );
    assert!(!turn_starts[1].contains("\"collaborationMode\""));
    assert!(!turn_starts[2].contains("\"collaborationMode\""));
    assert!(log.contains("\"approvalsReviewer\":\"auto_review\""));
    assert!(log.contains("\"effort\":\"high\""));
    assert!(log.contains("\"outputSchema\":{\"additionalProperties\":false,\"type\":\"object\"}"));
    assert!(log.contains("\"sandbox\":\"danger-full-access\""));
    assert!(!log.contains("\"permissions\":\":danger-full-access\""));
    full_access_module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn plan_collaboration_mode_is_rejected_for_standard_turns_before_process_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut request = harness.request("ordering", 1);
    let RuntimeIntent::Turn(turn) = &mut request.intent else {
        panic!("turn request");
    };
    turn.mode = Some("plan".to_string());

    let error = module
        .execute(
            request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("standard turn must reject experimental collaboration mode");
    assert_eq!(error.code, "codex_experimental_mode_requires_gui_advanced");
    assert!(error.message.contains("GUI Advanced"));
    assert!(
        !harness.log.exists(),
        "rejection happens before prompt or spawn"
    );
}

#[tokio::test]
async fn experimental_workspace_roots_are_rejected_before_process_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut request = harness.request("ordering", 1);
    request
        .profile
        .workspace_roots
        .push(harness._temp.path().to_path_buf());

    let error = module
        .execute(
            request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("Stable direct Codex must reject experimental workspace roots");
    assert_eq!(error.code, "codex_experimental_workspace_roots_unsupported");
    assert!(!harness.log.exists(), "rejection happens before spawn");
}

#[tokio::test]
async fn unsupported_mode_is_rejected_before_process_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let mut request = harness.request("ordering", 1);
    let RuntimeIntent::Turn(turn) = &mut request.intent else {
        panic!("turn request");
    };
    turn.mode = Some("invented-mode".to_string());
    let error = module
        .execute(
            request,
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("unsupported mode");
    assert_eq!(error.code, "unsupported");
    assert!(!harness.log.exists());
}

#[tokio::test]
async fn codex_auxiliary_matrix_projects_strict_updates_and_caches_stable_state() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("auxiliary");
    let cwd = harness._temp.path().to_string_lossy().to_string();

    let rate_limits = execute_extension(
        &module,
        &profile,
        "codex.account",
        "rateLimits/read",
        Some(json!({"cwd": cwd})),
    )
    .await;
    assert_eq!(rate_limits["rateLimits"]["primary"]["usedPercent"], 10);
    assert_eq!(rate_limits["rateLimits"]["credits"]["balance"], "12.50");
    assert_eq!(rate_limits["resetCreditsAvailable"], 2);

    let goal_target = json!({
        "threadId": "gateway-thread-1",
        "nativeSessionId": "native-1",
        "cwd": harness._temp.path(),
    });
    let goal = execute_extension(
        &module,
        &profile,
        "codex.goal",
        "read",
        Some(goal_target.clone()),
    )
    .await;
    assert_eq!(goal["goal"]["objective"], "Ship auxiliary state");
    let goal = execute_extension(
        &module,
        &profile,
        "codex.goal",
        "set",
        Some(json!({
            "threadId": "gateway-thread-1",
            "nativeSessionId": "native-1",
            "cwd": harness._temp.path(),
            "objective": "Ship auxiliary state",
            "status": "active",
            "tokenBudget": 1000,
        })),
    )
    .await;
    assert_eq!(goal["goal"]["status"], "active");
    assert_eq!(goal["goal"]["tokenBudget"], 1000);

    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let result = module
        .execute(
            harness.request("auxiliary", 1),
            RuntimeObserver::new(move |observation| {
                captured.lock().expect("observations").push(observation);
            }),
            RuntimeControl::default(),
        )
        .await
        .expect("auxiliary turn");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.final_answer, "auxiliary done");

    {
        let observations = observations.lock().expect("observations");
        let plan = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeObservation::PlanUpdated(update) => Some(update),
                _ => None,
            })
            .expect("plan observation");
        assert_eq!(plan.runtime_ref, profile.id);
        assert_eq!(plan.thread_id, "gateway-thread-1");
        assert_eq!(plan.turn_id, "gateway-turn-1");
        assert_eq!(plan.explanation.as_deref(), Some("Check then ship"));
        assert_eq!(plan.steps[0].status, RuntimePlanStepStatus::Completed);
        assert_eq!(plan.steps[1].status, RuntimePlanStepStatus::InProgress);

        let diff = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeObservation::DiffUpdated(update) => Some(update),
                _ => None,
            })
            .expect("diff observation");
        assert_eq!(diff.thread_id, "gateway-thread-1");
        assert!(diff.diff.contains("+typed"));

        let usage = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeObservation::UsageUpdated(update) => Some(update),
                _ => None,
            })
            .expect("usage observation");
        assert_eq!(usage.usage.total.total_tokens, 120);
        assert_eq!(usage.usage.total.cached_input_tokens, 20);
        assert_eq!(usage.usage.model_context_window, Some(200_000));

        let goal = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeObservation::GoalChanged(update) => Some(update),
                _ => None,
            })
            .expect("goal observation");
        assert_eq!(goal.thread_id, "gateway-thread-1");
        let goal = goal.goal.as_ref().expect("updated goal");
        assert_eq!(goal.status, RuntimeGoalStatus::Active);
        assert_eq!(goal.tokens_used, 150);

        let rate_update = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeObservation::AccountRateLimitsUpdated(update) => Some(update),
                _ => None,
            })
            .expect("rate-limit observation");
        let rate_limit = &rate_update.rate_limits.rate_limits;
        assert_eq!(
            rate_limit
                .primary
                .as_ref()
                .map(|window| window.used_percent),
            Some(42)
        );
        assert!(
            rate_limit.secondary.is_none(),
            "sparse update clears native windows"
        );
        assert_eq!(
            rate_limit
                .credits
                .as_ref()
                .and_then(|credits| credits.balance.as_deref()),
            Some("12.50")
        );
        assert_eq!(rate_limit.plan_type.as_deref(), Some("pro"));
        assert_eq!(rate_update.rate_limits.reset_credits_available, Some(2));
    }

    let snapshot = module
        .snapshot(SnapshotQuery {
            profile: profile.clone(),
            scope: SnapshotScope::Session {
                cwd: harness._temp.path().to_path_buf(),
                thread_id: "gateway-thread-1".to_string(),
                native_session_id: Some("native-1".to_string()),
            },
            mode: SnapshotMode::Cached,
        })
        .await
        .expect("cached auxiliary snapshot");
    let extension = snapshot.extension.expect("auxiliary snapshot extension");
    assert_eq!(extension["plan"]["turnId"], "gateway-turn-1");
    assert!(
        extension["diff"]["diff"]
            .as_str()
            .is_some_and(|diff| diff.contains("+typed"))
    );
    assert_eq!(extension["usage"]["usage"]["last"]["outputTokens"], 10);
    assert_eq!(extension["goal"]["tokensUsed"], 150);
    assert_eq!(
        extension["accountRateLimits"]["rateLimits"]["primary"]["usedPercent"],
        42
    );
    assert_eq!(
        extension["accountRateLimits"]["rateLimits"]["credits"]["balance"],
        "12.50"
    );

    let cleared =
        execute_extension(&module, &profile, "codex.goal", "clear", Some(goal_target)).await;
    assert_eq!(cleared, json!({"cleared": true}));
    let log = harness.log();
    for method in [
        "thread/goal/get",
        "thread/goal/set",
        "thread/goal/clear",
        "account/rateLimits/read",
    ] {
        assert!(log.contains(&format!("\"method\":\"{method}\"")));
    }
    assert!(log.contains("\"status\":\"active\""));
    assert!(log.contains("\"tokenBudget\":1000"));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn codex_extensions_reject_unknown_or_empty_argument_shapes_before_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let profile = harness.profile("auxiliary");
    for (operation, argument) in [
        (
            "read",
            json!({
                "threadId": "gateway-thread-1",
                "nativeSessionId": "native-1",
                "cwd": harness._temp.path(),
                "unexpected": true,
            }),
        ),
        (
            "set",
            json!({
                "threadId": "gateway-thread-1",
                "nativeSessionId": "native-1",
                "cwd": harness._temp.path(),
            }),
        ),
        (
            "set",
            json!({
                "threadId": "gateway-thread-1",
                "nativeSessionId": "native-1",
                "cwd": harness._temp.path(),
                "tokenBudget": -1,
            }),
        ),
    ] {
        let error = module
            .execute(
                ExecuteRequest {
                    expected_profile_revision: 1,
                    expected_capability_revision: Some(1),
                    expected_binding_revision: None,
                    intent: RuntimeIntent::Extension(RuntimeExtensionRequest {
                        namespace: "codex.goal".to_string(),
                        operation: operation.to_string(),
                        argument: Some(argument),
                    }),
                    profile: profile.clone(),
                },
                RuntimeObserver::default(),
                RuntimeControl::default(),
            )
            .await
            .expect_err("strict extension schema");
        assert_eq!(error.code, "codex_invalid_extension_schema");
    }
    assert!(
        !harness.log.exists(),
        "invalid extension must not spawn Codex"
    );
}

#[tokio::test]
async fn codex_compaction_waits_past_rpc_ack_for_matching_item_completion() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let request = compaction_request(&harness, "compact", None);
    let compact_module = module.clone();
    let compact = tokio::spawn(async move {
        compact_module
            .execute(
                request,
                RuntimeObserver::new(move |observation| {
                    captured.lock().expect("observations").push(observation);
                }),
                RuntimeControl::default(),
            )
            .await
    });
    wait_for_log(&harness, "\"method\":\"thread/compact/start\"").await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !compact.is_finished(),
        "thread/compact/start acknowledgement is not completion"
    );
    let result = tokio::time::timeout(Duration::from_secs(5), compact)
        .await
        .expect("compaction timeout")
        .expect("compaction task")
        .expect("compaction result");
    let ExecuteResult::Compaction(result) = result else {
        panic!("compaction result");
    };
    assert!(result.compacted);
    assert_eq!(result.thread_id, "gateway-thread-1");
    assert_eq!(result.native_session_id, "native-1");
    assert!(result.item_id.starts_with("cx_"));
    assert_ne!(result.item_id, "native-compaction-1");

    let changes = observations
        .lock()
        .expect("observations")
        .iter()
        .filter_map(|observation| match observation {
            RuntimeObservation::CompactionChanged(change) => Some(change.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].status, RuntimeCompactionStatus::Started);
    assert_eq!(changes[1].status, RuntimeCompactionStatus::Completed);
    assert_eq!(changes[0].item_id.as_deref(), Some(result.item_id.as_str()));
    assert_eq!(changes[1].item_id.as_deref(), Some(result.item_id.as_str()));
    assert!(
        !serde_json::to_string(&changes)
            .expect("serialize changes")
            .contains("native-compaction-1")
    );
    let log = harness.log();
    assert!(log.contains("\"params\":{\"threadId\":\"native-1\"}"));
    assert!(!log.contains("instructions"));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn codex_compaction_eof_fails_without_a_completed_observation() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        module.execute(
            compaction_request(&harness, "compact_eof", None),
            RuntimeObserver::new(move |observation| {
                captured.lock().expect("observations").push(observation);
            }),
            RuntimeControl::default(),
        ),
    )
    .await
    .expect("compaction EOF timeout")
    .expect_err("EOF before item completion");
    assert_eq!(error.code, "codex_process_exit");
    let statuses = observations
        .lock()
        .expect("observations")
        .iter()
        .filter_map(|observation| match observation {
            RuntimeObservation::CompactionChanged(change) => Some(change.status),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(statuses, [RuntimeCompactionStatus::Failed]);
    assert!(
        harness
            .log()
            .contains("\"method\":\"thread/compact/start\"")
    );
}

#[tokio::test]
async fn codex_compaction_rejects_custom_instructions_before_process_spawn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let error = module
        .execute(
            compaction_request(&harness, "compact", Some("Preserve only decisions")),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect_err("custom compaction instructions");
    assert_eq!(error.code, "codex_compaction_instructions_unsupported");
    assert!(!harness.log.exists(), "rejection happens before spawn");
}

#[tokio::test]
async fn steer_maps_to_the_active_native_codex_turn() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    module
        .execute(
            harness.request("steer", 1),
            RuntimeObserver::default(),
            RuntimeControl::default(),
        )
        .await
        .expect("prime stable Codex turn");

    let control = RuntimeControl::default();
    let pending_steer = control.clone();
    let request_log = harness.log.clone();
    let steer_task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let trace = std::fs::read_to_string(&request_log).unwrap_or_default();
                if trace.matches("\"method\":\"turn/start\"").count() >= 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("second native turn should start before steer");
        pending_steer.steer("steer through Host control");
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        module.execute(
            harness.request("steer", 2),
            RuntimeObserver::default(),
            control,
        ),
    )
    .await
    .expect("steered turn timeout")
    .expect("steered turn");
    steer_task.await.expect("steer task");

    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.final_answer, "steered through public control");
    let trace = harness.log();
    assert_eq!(trace.matches("\"method\":\"turn/steer\"").count(), 1);
    assert!(trace.contains("steer through Host control"));
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn abort_maps_to_native_turn_interrupt() {
    let harness = Harness::new();
    let module = CodexRuntimeModule::new();
    let control = RuntimeControl::default();
    let abort = control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        abort.abort();
    });
    let result = module
        .execute(
            harness.request("abort", 1),
            RuntimeObserver::default(),
            control,
        )
        .await
        .expect("aborted turn");
    let ExecuteResult::Turn(turn) = result else {
        panic!("turn result");
    };
    assert_eq!(turn.outcome, RuntimeTurnOutcome::Interrupted);
    assert_eq!(
        harness
            .log()
            .matches("\"method\":\"turn/interrupt\"")
            .count(),
        1
    );
    module
        .shutdown(ShutdownMode::Force)
        .await
        .expect("shutdown");
}
