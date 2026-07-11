#[derive(Debug, Clone)]
struct BindingAssertingRuntime {
    state: StateRuntime,
    executions: Arc<AtomicUsize>,
    turns: Arc<Mutex<Vec<psychevo_runtime_host::RuntimeTurnRequest>>>,
}

#[derive(Debug, Clone, Default)]
struct PostBindUnknownDeliveryRuntime {
    calls: Arc<AtomicUsize>,
    requested_native_sessions: Arc<Mutex<Vec<Option<String>>>>,
}

#[derive(Debug, Clone, Default)]
struct ProbeRecordingRuntime {
    modes: Arc<Mutex<Vec<psychevo_runtime_host::SnapshotMode>>>,
}

#[derive(Debug, Clone, Default)]
struct SteerRecordingRuntime {
    executions: Arc<AtomicUsize>,
    started: Arc<tokio::sync::Notify>,
    turn: Arc<Mutex<Option<(String, String)>>>,
    steered: Arc<Mutex<Vec<String>>>,
}

impl psychevo_runtime_host::RuntimeModule for SteerRecordingRuntime {
    fn snapshot(
        &self,
        query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async move {
            Ok(psychevo_runtime_host::RuntimeSnapshot {
                runtime_ref: query.profile.id,
                kind: query.profile.kind,
                profile_revision: query.profile.revision,
                capability_revision: 1,
                adapter_version: "steer-recording-fake".to_string(),
                runtime_version: Some("steer-recording-fake-1".to_string()),
                stability: psychevo_runtime_host::RuntimeStability::Stable,
                provenance: "direct".to_string(),
                readiness: Vec::new(),
                controls: Vec::new(),
                capabilities: Vec::new(),
                process_epoch: Some(1),
                instance_epoch: None,
                binding_epoch: None,
                extension: None,
            })
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let started = Arc::clone(&self.started);
        let execution = self.executions.fetch_add(1, Ordering::SeqCst);
        let captured_turn = Arc::clone(&self.turn);
        let steered = Arc::clone(&self.steered);
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Turn(turn) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "steer recording fake only supports turns",
                ));
            };
            observer
                .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                    runtime_ref: request.profile.id,
                    thread_id: turn.thread_id.clone(),
                    native_session_id: "codex-native-steer".to_string(),
                    cwd: turn.cwd.clone(),
                    binding_epoch: turn.binding_epoch,
                    process_epoch: 1,
                    instance_epoch: None,
                })
                .await?;
            if execution > 0 {
                *captured_turn.lock().expect("captured steer turn poisoned") =
                    Some((turn.thread_id.clone(), turn.turn_id.clone()));
                started.notify_one();

                let forwarded = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    loop {
                        let messages = control.take_steer();
                        if !messages.is_empty() {
                            break messages;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .map_err(|_| {
                    psychevo_runtime_host::RuntimeError::new(
                        "steer_not_forwarded",
                        psychevo_runtime_host::RuntimeErrorStage::Prompt,
                        psychevo_runtime_host::RetryClass::Never,
                        "Gateway did not forward the public steer into Host runtime control",
                    )
                })?;
                steered
                    .lock()
                    .expect("recorded steers poisoned")
                    .extend(forwarded);
            }

            Ok(psychevo_runtime_host::ExecuteResult::Turn(
                psychevo_runtime_host::RuntimeTurnResult {
                    turn_id: turn.turn_id,
                    thread_id: turn.thread_id,
                    native_session_id: "codex-native-steer".to_string(),
                    outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                    final_answer: if execution == 0 {
                        "prime complete".to_string()
                    } else {
                        "steer forwarded".to_string()
                    },
                    provider: "codex".to_string(),
                    model: "fake-codex".to_string(),
                    history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                    process_epoch: 1,
                    instance_epoch: None,
                    terminal_error: None,
                    metadata: None,
                },
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

impl psychevo_runtime_host::RuntimeModule for ProbeRecordingRuntime {
    fn snapshot(
        &self,
        query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        self.modes
            .lock()
            .expect("snapshot modes poisoned")
            .push(query.mode);
        Box::pin(async move {
            Ok(psychevo_runtime_host::RuntimeSnapshot {
                runtime_ref: query.profile.id,
                kind: query.profile.kind,
                profile_revision: query.profile.revision,
                capability_revision: 1,
                adapter_version: "probe-fake".to_string(),
                runtime_version: Some("local-handshake".to_string()),
                stability: psychevo_runtime_host::RuntimeStability::Stable,
                provenance: "direct".to_string(),
                readiness: Vec::new(),
                controls: Vec::new(),
                capabilities: Vec::new(),
                process_epoch: Some(1),
                instance_epoch: None,
                binding_epoch: None,
                extension: None,
            })
        })
    }

    fn execute(
        &self,
        _request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Configuration,
                psychevo_runtime_host::RetryClass::UserAction,
                "probe fake does not execute turns",
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

impl psychevo_runtime_host::RuntimeModule for PostBindUnknownDeliveryRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::UserAction,
                "snapshot is outside this delivery test",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let calls = Arc::clone(&self.calls);
        let requested_native_sessions = Arc::clone(&self.requested_native_sessions);
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Turn(turn) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "delivery fake only supports turns",
                ));
            };
            requested_native_sessions
                .lock()
                .expect("requested native sessions poisoned")
                .push(turn.native_session_id.clone());
            let call = calls.fetch_add(1, Ordering::SeqCst);
            observer
                .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                    runtime_ref: request.profile.id,
                    thread_id: turn.thread_id.clone(),
                    native_session_id: "codex-native-retained".to_string(),
                    cwd: turn.cwd.clone(),
                    binding_epoch: turn.binding_epoch,
                    process_epoch: 9,
                    instance_epoch: None,
                })
                .await?;
            if call == 0 {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "prompt_delivery_unknown",
                    psychevo_runtime_host::RuntimeErrorStage::Prompt,
                    psychevo_runtime_host::RetryClass::UnknownDelivery,
                    "the fake lost the post-bind prompt response",
                ));
            }
            if turn.native_session_id.as_deref() != Some("codex-native-retained") {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "native_session_not_resumed",
                    psychevo_runtime_host::RuntimeErrorStage::Binding,
                    psychevo_runtime_host::RetryClass::Never,
                    "the next Gateway request did not resume the retained native session",
                ));
            }
            Ok(psychevo_runtime_host::ExecuteResult::Turn(
                psychevo_runtime_host::RuntimeTurnResult {
                    turn_id: turn.turn_id,
                    thread_id: turn.thread_id,
                    native_session_id: "codex-native-retained".to_string(),
                    outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                    final_answer: "resumed retained native session".to_string(),
                    provider: "codex".to_string(),
                    model: "fake-codex".to_string(),
                    history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                    process_epoch: 9,
                    instance_epoch: None,
                    terminal_error: None,
                    metadata: None,
                },
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

impl psychevo_runtime_host::RuntimeModule for BindingAssertingRuntime {
    fn snapshot(
        &self,
        query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async move {
            Ok(psychevo_runtime_host::RuntimeSnapshot {
                runtime_ref: query.profile.id,
                kind: query.profile.kind,
                profile_revision: query.profile.revision,
                capability_revision: 1,
                adapter_version: "binding-fake".to_string(),
                runtime_version: Some("binding-fake-1".to_string()),
                stability: psychevo_runtime_host::RuntimeStability::Stable,
                provenance: "direct".to_string(),
                readiness: vec![psychevo_runtime_host::ReadinessStage {
                    id: "session".to_string(),
                    status: psychevo_runtime_host::ReadinessStatus::Ready,
                    summary: "session observed".to_string(),
                    observed_at_ms: None,
                }],
                controls: matches!(
                    query.scope,
                    psychevo_runtime_host::SnapshotScope::Session { .. }
                )
                .then(|| psychevo_runtime_host::RuntimeControlDescriptor {
                    id: "model".to_string(),
                    label: "Model".to_string(),
                    state: psychevo_runtime_host::ControlState::ReadOnlyCurrent,
                    current_value: Some(json!("fake-codex")),
                    choices: Vec::new(),
                    depends_on: None,
                    channel_safe: false,
                    capability_revision: 1,
                })
                .into_iter()
                .collect(),
                capabilities: Vec::new(),
                process_epoch: Some(1),
                instance_epoch: None,
                binding_epoch: None,
                extension: None,
            })
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let state = self.state.clone();
        let executions = Arc::clone(&self.executions);
        let turns = Arc::clone(&self.turns);
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Turn(turn) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "test runtime only supports turns",
                ));
            };
            let binding = state
                .store()
                .gateway_runtime_binding(&turn.thread_id)
                .map_err(|error| {
                    psychevo_runtime_host::RuntimeError::new(
                        "binding_read",
                        psychevo_runtime_host::RuntimeErrorStage::Binding,
                        psychevo_runtime_host::RetryClass::Never,
                        error.to_string(),
                    )
                })?
                .ok_or_else(|| {
                    psychevo_runtime_host::RuntimeError::new(
                        "binding_missing",
                        psychevo_runtime_host::RuntimeErrorStage::Binding,
                        psychevo_runtime_host::RetryClass::Never,
                        "runtime binding must exist before native prompt delivery",
                    )
                })?;
            if binding.runtime_ref.as_deref() != Some("codex")
                || binding.profile_fingerprint.as_deref()
                    != Some(request.profile.fingerprint.as_str())
            {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "binding_mismatch",
                    psychevo_runtime_host::RuntimeErrorStage::Binding,
                    psychevo_runtime_host::RetryClass::Never,
                    "runtime binding did not match the effective profile",
                ));
            }
            turns
                .lock()
                .expect("captured direct turns poisoned")
                .push(turn.clone());
            executions.fetch_add(1, Ordering::SeqCst);
            observer
                .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                    runtime_ref: request.profile.id,
                    thread_id: turn.thread_id.clone(),
                    native_session_id: "codex-native-1".to_string(),
                    cwd: turn.cwd.clone(),
                    binding_epoch: turn.binding_epoch,
                    process_epoch: 1,
                    instance_epoch: None,
                })
                .await?;
            if turn.prompt == "tool with native metadata" {
                observer.emit(psychevo_runtime_host::RuntimeObservation::Tool {
                    turn_id: turn.turn_id.clone(),
                    item_id: "native-tool-secret".to_string(),
                    name: "shell".to_string(),
                    status: "completed".to_string(),
                    detail: Some(json!({
                        "id": "native-tool-secret",
                        "sessionId": "native-session-secret",
                        "messageId": "native-message-secret",
                        "command": "cargo test",
                        "status": "completed",
                        "output": {
                            "text": "ok",
                            "requestId": "native-request-secret",
                            "echo": "native-tool-secret",
                        },
                        "rawNativeEvent": "must-not-project",
                    })),
                });
            }
            if turn.prompt == "auxiliary runtime observations" {
                observer.emit(psychevo_runtime_host::RuntimeObservation::PlanUpdated(
                    psychevo_runtime_host::RuntimePlanUpdate {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                        explanation: Some("Obsolete native plan".to_string()),
                        steps: vec![psychevo_runtime_host::RuntimePlanStep {
                            step: "Discard stale evidence".to_string(),
                            status: psychevo_runtime_host::RuntimePlanStepStatus::Pending,
                        }],
                    },
                ));
                observer.emit(psychevo_runtime_host::RuntimeObservation::DiffUpdated(
                    psychevo_runtime_host::RuntimeDiffUpdate {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                        diff: "--- a/obsolete.md\n+++ b/obsolete.md".to_string(),
                    },
                ));
                observer.emit(psychevo_runtime_host::RuntimeObservation::PlanUpdated(
                    psychevo_runtime_host::RuntimePlanUpdate {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                        explanation: Some("Native plan".to_string()),
                        steps: vec![psychevo_runtime_host::RuntimePlanStep {
                            step: "Verify evidence".to_string(),
                            status: psychevo_runtime_host::RuntimePlanStepStatus::InProgress,
                        }],
                    },
                ));
                observer.emit(psychevo_runtime_host::RuntimeObservation::DiffUpdated(
                    psychevo_runtime_host::RuntimeDiffUpdate {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                        diff: "--- a/spec.md\n+++ b/spec.md\n@@ -1 +1 @@\n-old\n+new".to_string(),
                    },
                ));
                observer.emit(psychevo_runtime_host::RuntimeObservation::UsageUpdated(
                    psychevo_runtime_host::RuntimeUsageUpdate {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                        usage: psychevo_runtime_host::RuntimeTokenUsage {
                            total: psychevo_runtime_host::RuntimeTokenUsageBreakdown {
                                total_tokens: 1_000,
                                input_tokens: 800,
                                cached_input_tokens: 300,
                                output_tokens: 200,
                                reasoning_output_tokens: 50,
                            },
                            last: psychevo_runtime_host::RuntimeTokenUsageBreakdown {
                                total_tokens: 150,
                                input_tokens: 110,
                                cached_input_tokens: 40,
                                output_tokens: 40,
                                reasoning_output_tokens: 10,
                            },
                            model_context_window: Some(8_192),
                        },
                    },
                ));
                observer.emit(psychevo_runtime_host::RuntimeObservation::GoalChanged(
                    psychevo_runtime_host::RuntimeGoalChange {
                        runtime_ref: "codex".to_string(),
                        thread_id: turn.thread_id.clone(),
                        turn_id: Some(turn.turn_id.clone()),
                        goal: Some(psychevo_runtime_host::RuntimeGoal {
                            objective: "Ship evidence".to_string(),
                            status: psychevo_runtime_host::RuntimeGoalStatus::Active,
                            token_budget: Some(2_000),
                            tokens_used: 1_000,
                            time_used_seconds: 30,
                            created_at: 1,
                            updated_at: 2,
                        }),
                    },
                ));
                observer.emit(
                    psychevo_runtime_host::RuntimeObservation::AccountRateLimitsUpdated(
                        psychevo_runtime_host::RuntimeAccountRateLimitsUpdate {
                            runtime_ref: "codex".to_string(),
                            rate_limits: psychevo_runtime_host::RuntimeAccountRateLimits {
                                rate_limits: psychevo_runtime_host::RuntimeRateLimitSnapshot {
                                    limit_id: Some("primary".to_string()),
                                    limit_name: Some("Primary".to_string()),
                                    primary: Some(
                                        psychevo_runtime_host::RuntimeRateLimitWindow {
                                            used_percent: 25,
                                            window_duration_mins: Some(300),
                                            resets_at: Some(42),
                                        },
                                    ),
                                    secondary: None,
                                    credits: None,
                                    individual_limit: None,
                                    plan_type: Some("team".to_string()),
                                    rate_limit_reached_type: None,
                                },
                                rate_limits_by_limit_id: std::collections::BTreeMap::new(),
                                reset_credits_available: None,
                            },
                        },
                    ),
                );
            }
            let failed_with_native_metadata = turn.prompt == "fail with native metadata";
            Ok(psychevo_runtime_host::ExecuteResult::Turn(
                psychevo_runtime_host::RuntimeTurnResult {
                    turn_id: turn.turn_id,
                    thread_id: turn.thread_id,
                    native_session_id: "codex-native-1".to_string(),
                    outcome: if failed_with_native_metadata {
                        psychevo_runtime_host::RuntimeTurnOutcome::Failed
                    } else {
                        psychevo_runtime_host::RuntimeTurnOutcome::Completed
                    },
                    final_answer: if failed_with_native_metadata {
                        String::new()
                    } else {
                        "direct answer".to_string()
                    },
                    provider: "codex".to_string(),
                    model: "fake-codex".to_string(),
                    history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                    process_epoch: 1,
                    instance_epoch: None,
                    terminal_error: failed_with_native_metadata.then(|| {
                        psychevo_runtime_host::RuntimeTerminalError {
                            code: "runtime_failed".to_string(),
                            stage: psychevo_runtime_host::RuntimeErrorStage::Prompt,
                            retry_class: psychevo_runtime_host::RetryClass::Never,
                            message: "Codex failed the turn.".to_string(),
                            diagnostic_ref: "runtime-process-1".to_string(),
                        }
                    }),
                    metadata: failed_with_native_metadata.then(|| {
                        json!({
                            "nativeTurnId": "native-turn-secret",
                            "nativeUserMessageId": "native-message-secret",
                            "error": {
                                "code": "runtime_failed",
                                "message": "native-message-secret failed",
                            },
                            "terminal": {
                                "nativeTurn": {
                                    "id": "native-turn-secret",
                                    "status": "failed",
                                    "error": {"message": "native-message-secret failed"},
                                    "rawNativeEvent": "must-not-project",
                                }
                            },
                            "diagnosticRef": "runtime-process-1",
                        })
                    }),
                },
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_steer_reaches_the_active_direct_runtime_control() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("cwd");
    std::fs::write(
        cwd.join(".psychevo/config.toml"),
        r#"[runtime_profiles.codex]
runtime = "codex"
enabled = true
label = "Codex"
command = "codex"
args = ["app-server", "--stdio"]
"#,
    )
    .expect("Runtime Profile config");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let runtime = SteerRecordingRuntime::default();
    let host = RuntimeHost::new();
    host.register(RuntimeKind::Codex, Arc::new(runtime.clone()));
    let gateway = Gateway::with_backend_and_runtime_host(
        state.clone(),
        Arc::new(FakeBackend::default()),
        host,
    );
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let source = GatewaySource::new("web", "direct-runtime-steer").persistent();
    let (terminal_tx, terminal_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    let blocked_terminal = Arc::new(AtomicBool::new(false));
    let mut prime = request(&harness, source.clone(), "prime direct runtime");
    prime.options.runtime_ref = Some("codex".to_string());
    prime.options.clarify_enabled = true;
    prime.event_sink = Some(Arc::new({
        let release_rx = Arc::clone(&release_rx);
        let blocked_terminal = Arc::clone(&blocked_terminal);
        move |event| {
            if let GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id),
                turn_id,
                ..
            } = event
                && !blocked_terminal.swap(true, Ordering::SeqCst)
            {
                terminal_tx
                    .send((thread_id, turn_id))
                    .expect("report persisted terminal");
                release_rx
                    .lock()
                    .expect("terminal release gate poisoned")
                    .take()
                    .expect("terminal release receiver")
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("release terminal projection");
            }
        }
    }));
    let prime_gateway = harness.gateway.clone();
    let prime = tokio::spawn(async move { prime_gateway.send_turn(prime).await });
    let (thread_id, finished_turn_id) = tokio::task::spawn_blocking(move || {
        terminal_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("first terminal should be persisted")
    })
        .await
        .expect("terminal wait task");

    assert!(
        harness
            .gateway
            .steer_turn(
                GatewayThreadSelector::thread_id(thread_id.clone()),
                Some(&finished_turn_id),
                psychevo_agent_core::user_text_message("must reject completed turn"),
            )
            .is_none(),
        "a persisted terminal must synchronously retire the old steer control"
    );
    assert!(
        !harness.gateway.steer_foreign_turn(
            GatewayThreadSelector::thread_id(thread_id.clone()),
            Some(&finished_turn_id),
            psychevo_agent_core::user_text_message("must reject completed foreign turn"),
        ),
        "the durable fallback must reject a terminal expected turn"
    );

    let mut turn = request(&harness, source, "wait for steer");
    turn.thread_id = Some(thread_id.clone());
    turn.options.session = Some(thread_id.clone());
    turn.options.runtime_ref = Some("codex".to_string());
    turn.options.clarify_enabled = true;

    let turn_gateway = harness.gateway.clone();
    let active = tokio::spawn(async move { turn_gateway.send_turn(turn).await });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(thread_id.clone()))
                .queued_turns
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("follow-up should remain queued behind terminal cleanup");
    release_tx.send(()).expect("release first terminal");
    let primed = prime
        .await
        .expect("prime direct task")
        .expect("prime direct turn");
    assert_eq!(primed.result.final_answer, "prime complete");
    tokio::time::timeout(std::time::Duration::from_secs(1), runtime.started.notified())
        .await
        .expect("direct runtime turn should start");
    let (thread_id, turn_id) = runtime
        .turn
        .lock()
        .expect("captured steer turn poisoned")
        .clone()
        .expect("captured direct turn identity");

    let input_id = harness
        .gateway
        .steer_turn(
            GatewayThreadSelector::thread_id(thread_id),
            Some(&turn_id),
            psychevo_agent_core::user_text_message("steer through Gateway"),
        )
        .expect("public steer should be accepted for the active direct turn");
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), active)
        .await
        .expect("direct turn should finish after steer")
        .expect("direct turn task")
        .expect("direct turn result");

    assert_eq!(result.result.final_answer, "steer forwarded");
    assert_eq!(
        runtime.steered.lock().expect("recorded steers poisoned").as_slice(),
        ["steer through Gateway"]
    );
    assert!(
        !harness.gateway.update_steer(
            GatewayThreadSelector::thread_id(result.result.session_id),
            Some(&turn_id),
            input_id,
            psychevo_agent_core::user_text_message("must be too late"),
        ),
        "the direct bridge must consume the accepted steer exactly once"
    );
}

#[tokio::test]
async fn direct_codex_public_steer_reaches_the_native_transport() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("cwd");
    let executable = temp
        .path()
        .join(format!("fake-codex{}", std::env::consts::EXE_SUFFIX));
    let fake_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../psychevo-runtime-host/tests/fixtures/fake_codex_app_server.rs");
    let status = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(fake_source)
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("compile fake Codex app-server");
    assert!(status.success());
    let native_log = temp.path().join("codex-requests.jsonl");
    std::fs::write(
        cwd.join(".psychevo/config.toml"),
        format!(
            r#"[runtime_profiles.codex]
runtime = "codex"
enabled = true
label = "Codex"
command = "{}"
args = ["app-server", "--stdio"]
default_model = "gpt-fixture"
approval_mode = "on-request"
sandbox = "workspace-write"

[runtime_profiles.codex.env]
CODEX_FAKE_SCENARIO = "steer"
CODEX_FAKE_LOG = "{}"
CODEX_FAKE_CWD = "{}"
"#,
            executable.display(),
            native_log.display(),
            cwd.display(),
        ),
    )
    .expect("Runtime Profile config");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(psychevo_runtime_host::CodexRuntimeModule::new()),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        state.clone(),
        Arc::new(FakeBackend::default()),
        host,
    );
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let source = GatewaySource::new("web", "direct-codex-native-steer").persistent();
    let mut prime = request(&harness, source.clone(), "prime Codex");
    prime.options.runtime_ref = Some("codex".to_string());
    prime.options.clarify_enabled = true;
    let primed = harness.gateway.send_turn(prime).await.expect("prime Codex turn");
    assert_eq!(primed.result.final_answer, "hello");

    let mut turn = request(&harness, source, "wait for public steer");
    turn.thread_id = Some(primed.result.session_id.clone());
    turn.options.session = Some(primed.result.session_id.clone());
    turn.options.runtime_ref = Some("codex".to_string());
    turn.options.clarify_enabled = true;
    let turn_gateway = harness.gateway.clone();
    let active = tokio::spawn(async move { turn_gateway.send_turn(turn).await });
    let turn_id = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(turn_id) = harness
                .gateway
                .activity_for_selector(GatewayThreadSelector::thread_id(
                    primed.result.session_id.clone(),
                ))
                .active_turn_id
            {
                break turn_id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second public turn should start");
    harness
        .gateway
        .steer_turn(
            GatewayThreadSelector::thread_id(primed.result.session_id),
            Some(&turn_id),
            psychevo_agent_core::user_text_message("steer through Gateway now"),
        )
        .expect("public steer should be accepted");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), active)
        .await
        .expect("steered direct Codex turn timeout")
        .expect("steered direct Codex task")
        .expect("steered direct Codex turn");

    assert_eq!(result.result.final_answer, "steered through public control");
    let trace = std::fs::read_to_string(native_log).expect("native Codex trace");
    assert_eq!(trace.matches("\"method\":\"turn/steer\"").count(), 1);
    assert!(trace.contains("steer through Gateway now"));
}

#[tokio::test]
async fn direct_runtime_auxiliary_observations_project_and_persist_native_usage() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(BindingAssertingRuntime {
            state: state.clone(),
            executions: Arc::new(AtomicUsize::new(0)),
            turns: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    let native_backend = Arc::new(FakeBackend::default());
    let gateway = Gateway::with_backend_and_runtime_host(
        state.clone(),
        native_backend.clone(),
        host,
    );
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let stream_values = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&stream_values);
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "direct-runtime-auxiliary").persistent(),
        "auxiliary runtime observations",
    );
    turn.options.runtime_ref = Some("codex".to_string());
    turn.stream = Some(Arc::new(move |event| {
        if let Some(value) = event.legacy_value() {
            captured
                .lock()
                .expect("stream values poisoned")
                .push(value.clone());
        }
    }));
    let gateway_events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let captured_events = Arc::clone(&gateway_events);
    turn.event_sink = Some(Arc::new(move |event| {
        captured_events
            .lock()
            .expect("gateway events poisoned")
            .push(event);
    }));

    let result = harness
        .gateway
        .send_turn(turn)
        .await
        .expect("direct auxiliary turn");
    let values = stream_values.lock().expect("stream values poisoned");
    assert!(values.iter().any(|value| {
        value["type"] == "runtime_plan"
            && value["body"]
                .as_str()
                .is_some_and(|body| body.contains("Verify evidence"))
    }));
    assert!(values.iter().any(|value| {
        value["type"] == "runtime_diff"
            && value["diff"]
                .as_str()
                .is_some_and(|diff| diff.contains("+++ b/spec.md"))
    }));
    drop(values);

    let snapshot = result.result.context_snapshot.expect("native usage snapshot");
    assert_eq!(snapshot.total.tokens, 110);
    assert_eq!(snapshot.context_limit, Some(8_192));
    assert!(!snapshot.total.estimated);
    assert_eq!(snapshot.total.source, "runtime_usage");

    let messages = harness
        .state
        .store()
        .load_sanitized_message_summaries(&result.result.session_id)
        .expect("durable messages");
    let usage = messages
        .last()
        .and_then(|message| message.usage.as_ref())
        .expect("assistant usage");
    assert_eq!(usage["input_tokens"], 110);
    assert_eq!(usage["cached_input_tokens"], 40);
    assert_eq!(usage["output_tokens"], 40);
    assert_eq!(usage["reasoning_tokens"], 10);
    assert_eq!(usage["total_tokens"], 150);

    let events = gateway_events.lock().expect("gateway events poisoned");
    let started = events
        .iter()
        .filter(|event| matches!(event, GatewayEvent::TurnStarted { .. }))
        .count();
    assert_eq!(started, 1, "direct runtime must own one public start event");
    let started_at = events
        .iter()
        .position(|event| matches!(event, GatewayEvent::TurnStarted { .. }))
        .expect("direct runtime start event");
    let completed_at = events
        .iter()
        .position(|event| matches!(event, GatewayEvent::TurnCompleted { .. }))
        .expect("direct runtime completion event");
    let first_observation_at = events
        .iter()
        .position(|event| {
            matches!(
                event,
                GatewayEvent::EntryStarted { .. }
                    | GatewayEvent::EntryUpdated { .. }
                    | GatewayEvent::EntryCompleted { .. }
            )
        })
        .expect("direct runtime live observation");
    assert!(started_at < first_observation_at, "start must precede observations");
    assert!(started_at < completed_at, "start must precede completion");
    let terminal_entries = events
        .iter()
        .find_map(|event| match event {
            GatewayEvent::TurnCompleted {
                committed_entries, ..
            } => Some(committed_entries.clone()),
            _ => None,
        })
        .expect("terminal committed entries");
    drop(events);
    assert!(native_backend.runs().is_empty(), "direct runtime must not fall back");
    let reloaded_entries = crate::transcript::project_transcript_entries(
        &result.result.session_id,
        &harness
            .state
            .store()
            .load_tui_message_summaries(&result.result.session_id)
            .expect("reloaded messages"),
    );
    for entries in [&terminal_entries, &reloaded_entries] {
        let assistant = entries
            .iter()
            .find(|entry| entry.role == TranscriptEntryRole::Assistant)
            .expect("assistant transcript entry");
        let plan = assistant
            .blocks
            .iter()
            .find(|block| block.title.as_deref() == Some("Plan"))
            .expect("durable plan observation");
        assert_eq!(plan.kind, TranscriptBlockKind::Status);
        assert_eq!(plan.status, TranscriptBlockStatus::Completed);
        assert!(
            plan.body
                .as_deref()
                .is_some_and(|body| body.contains("Verify evidence"))
        );
        let diff = assistant
            .blocks
            .iter()
            .find(|block| block.title.as_deref() == Some("Diff"))
            .expect("durable diff observation");
        assert_eq!(diff.kind, TranscriptBlockKind::Diff);
        assert_eq!(diff.status, TranscriptBlockStatus::Completed);
        assert!(
            diff.body
                .as_deref()
                .is_some_and(|body| body.contains("+++ b/spec.md"))
        );
        let projected = serde_json::to_string(entries).expect("public transcript JSON");
        assert!(!projected.contains("Obsolete native plan"), "{projected}");
        assert!(!projected.contains("obsolete.md"), "{projected}");
        assert!(!projected.contains("codex-native-1"), "{projected}");
    }

    let metadata = harness
        .state
        .store()
        .session_metadata(&result.result.session_id)
        .expect("session metadata")
        .expect("session metadata object");
    assert_eq!(metadata["runtimeGoal"]["objective"], "Ship evidence");
    assert_eq!(
        metadata["runtimeAccountRateLimits"]["rateLimits"]["primary"]["usedPercent"],
        25
    );
}

#[tokio::test]
async fn direct_runtime_tool_projection_uses_opaque_ids_and_allowlisted_detail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(BindingAssertingRuntime {
            state: state.clone(),
            executions: Arc::new(AtomicUsize::new(0)),
            turns: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        state,
        Arc::new(FakeBackend::default()),
        host,
    );
    let harness = Harness {
        _temp: temp,
        cwd,
        state: gateway.state.clone(),
        gateway,
    };
    let stream_values = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&stream_values);
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "direct-runtime-tool-redaction").persistent(),
        "tool with native metadata",
    );
    turn.options.runtime_ref = Some("codex".to_string());
    turn.stream = Some(Arc::new(move |event| {
        if let Some(value) = event.legacy_value() {
            captured
                .lock()
                .expect("stream values poisoned")
                .push(value.clone());
        }
    }));

    harness.gateway.send_turn(turn).await.expect("direct tool turn");

    let values = stream_values.lock().expect("stream values poisoned");
    let tool = values
        .iter()
        .find(|value| value["type"] == "tool_execution_end")
        .expect("public tool event");
    assert!(
        tool["tool_call_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("rtd_")),
        "{tool:#}"
    );
    assert_eq!(tool["metadata"]["detail"]["command"], "cargo test");
    assert_eq!(tool["metadata"]["detail"]["output"]["text"], "ok");
    let serialized = serde_json::to_string(tool).expect("public tool JSON");
    for secret in [
        "native-tool-secret",
        "native-session-secret",
        "native-message-secret",
        "native-request-secret",
        "rawNativeEvent",
    ] {
        assert!(!serialized.contains(secret), "{tool:#}");
    }
}

#[tokio::test]
async fn direct_runtime_binds_before_prompt_and_profile_change_requires_new_thread() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("cwd");
    std::fs::write(
        cwd.join(".psychevo/config.toml"),
        r#"[runtime_profiles.codex]
runtime = "codex"
enabled = true
label = "Codex"
command = "codex"
args = ["app-server", "--stdio"]
default_model = "gpt-fixture"
"#,
    )
    .expect("Runtime Profile default model");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let executions = Arc::new(AtomicUsize::new(0));
    let turns = Arc::new(Mutex::new(Vec::new()));
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(BindingAssertingRuntime {
            state: state.clone(),
            executions: Arc::clone(&executions),
            turns: Arc::clone(&turns),
        }),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state: state.clone(),
        gateway,
    };
    let source = GatewaySource::new("web", "direct-runtime-binding").persistent();
    let mut first = request(&harness, source.clone(), "first direct prompt");
    first.options.runtime_ref = Some("codex".to_string());
    first.options.model = Some("native-provider/model-must-not-cross".to_string());

    let result = harness.gateway.send_turn(first).await.expect("direct turn");
    assert_eq!(result.result.final_answer, "direct answer");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        turns.lock().expect("captured turns")[0].model,
        None,
        "native Psychevo model selection must not cross the direct Runtime Profile boundary"
    );
    assert!(
        native_backend.runs().is_empty(),
        "native fallback must not run"
    );
    let binding = state
        .store()
        .gateway_runtime_binding(&result.result.session_id)
        .expect("binding")
        .expect("binding exists");
    assert_eq!(binding.runtime_ref.as_deref(), Some("codex"));
    assert_eq!(binding.native_session_id.as_deref(), Some("codex-native-1"));
    let bound_profile: RuntimeProfileConfig = serde_json::from_str(
        binding
            .profile_config_json
            .as_deref()
            .expect("bound profile snapshot"),
    )
    .expect("bound profile config");
    assert_eq!(bound_profile.default_model.as_deref(), Some("gpt-fixture"));
    let fingerprint = runtime_profile_config_fingerprint(&bound_profile);
    let session_snapshot = harness
        .gateway
        .cached_runtime_snapshot(&psychevo_runtime_host::SnapshotQuery {
            profile: gateway_runtime_profile(
                bound_profile,
                runtime_profile_config_revision(&fingerprint),
                fingerprint,
            ),
            scope: psychevo_runtime_host::SnapshotScope::Session {
                cwd: PathBuf::from(&binding.cwd),
                thread_id: result.result.session_id.clone(),
                native_session_id: Some("codex-native-1".to_string()),
            },
            mode: psychevo_runtime_host::SnapshotMode::Cached,
        })
        .expect("exact-session snapshot cached after direct turn");
    assert_eq!(session_snapshot.controls[0].current_value, Some(json!("fake-codex")));
    let terminals = state
        .store()
        .list_gateway_turn_terminals_for_thread(&result.result.session_id)
        .expect("turn terminals");
    assert_eq!(terminals.len(), 1, "accepted turn has one terminal");
    assert_eq!(terminals[0].status, "completed");

    let mut failed = request(&harness, source.clone(), "fail with native metadata");
    failed.thread_id = Some(result.result.session_id.clone());
    failed.options.session = Some(result.result.session_id.clone());
    failed.options.runtime_ref = Some("codex".to_string());
    let failed = harness
        .gateway
        .send_turn(failed)
        .await
        .expect("failed runtime outcome still resolves one public terminal");
    assert_eq!(failed.result.outcome, Outcome::Failed);
    assert_eq!(
        failed.result.terminal_error,
        Some(psychevo_runtime::RunTerminalError {
            code: "runtime_failed".to_string(),
            stage: "prompt".to_string(),
            retry_class: "never".to_string(),
            message: "Codex failed the turn.".to_string(),
            diagnostic_ref: "runtime-process-1".to_string(),
        })
    );
    assert!(
        failed.result.events.is_empty(),
        "adapter-native terminal metadata must not enter public run events"
    );
    let terminals = state
        .store()
        .list_gateway_turn_terminals_for_thread(&result.result.session_id)
        .expect("failed terminal");
    assert_eq!(terminals.len(), 2);
    let public_terminal =
        serde_json::to_string(&terminals[1].metadata).expect("public terminal metadata JSON");
    assert_eq!(
        terminals[1].error_message.as_deref(),
        Some("Codex failed the turn.")
    );
    assert!(!public_terminal.contains("native-turn-secret"));
    assert!(!public_terminal.contains("native-message-secret"));
    assert!(!public_terminal.contains("must-not-project"));
    assert!(public_terminal.contains("runtime_failed"));
    assert!(public_terminal.contains("runtime-process-1"));
    let projected = transcript::project_turn_terminal_entries(&terminals[1..]);
    let projected = serde_json::to_string(&projected).expect("projected terminal JSON");
    assert!(!projected.contains("native-turn-secret"));
    assert!(!projected.contains("native-message-secret"));
    assert!(!projected.contains("must-not-project"));

    let mut changed = request(&harness, source, "must not reach another runtime");
    changed.options.runtime_ref = Some("opencode".to_string());
    let error = harness
        .gateway
        .send_turn(changed)
        .await
        .expect_err("profile changes require a new thread");
    assert!(error.to_string().contains("start a new thread"), "{error}");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert!(
        native_backend.runs().is_empty(),
        "native fallback must not run"
    );
    assert_eq!(
        state
            .store()
            .gateway_runtime_binding(&result.result.session_id)
            .expect("binding")
            .expect("binding exists")
            .runtime_ref
            .as_deref(),
        Some("codex")
    );
}

#[tokio::test]
async fn post_bind_unknown_delivery_retains_native_identity_and_next_turn_resumes_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let runtime = PostBindUnknownDeliveryRuntime::default();
    let requested_native_sessions = Arc::clone(&runtime.requested_native_sessions);
    let calls = Arc::clone(&runtime.calls);
    let host = RuntimeHost::new();
    host.register(RuntimeKind::Codex, Arc::new(runtime));
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state: state.clone(),
        gateway,
    };
    let source = GatewaySource::new("web", "direct-runtime-delivery").persistent();
    let mut first = request(&harness, source.clone(), "first delivery is uncertain");
    first.options.runtime_ref = Some("codex".to_string());

    let error = harness
        .gateway
        .send_turn(first)
        .await
        .expect_err("unknown delivery remains failed and is not retried");
    assert!(error.to_string().contains("post-bind"), "{error}");
    let sessions = state
        .store()
        .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
        .expect("accepted session");
    assert_eq!(sessions.len(), 1);
    let thread_id = sessions[0].id.clone();
    let retained = state
        .store()
        .gateway_runtime_binding(&thread_id)
        .expect("binding read")
        .expect("binding exists");
    assert_eq!(
        retained.native_session_id.as_deref(),
        Some("codex-native-retained")
    );
    assert_eq!(retained.binding_revision, 2);

    let mut second = request(&harness, source, "resume instead of creating again");
    second.options.runtime_ref = Some("codex".to_string());
    second.options.session = Some(thread_id.clone());
    let result = harness
        .gateway
        .send_turn(second)
        .await
        .expect("explicit next turn resumes retained session");
    assert_eq!(result.result.session_id, thread_id);
    assert_eq!(
        result.result.final_answer,
        "resumed retained native session"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        requested_native_sessions
            .lock()
            .expect("requested native sessions")
            .as_slice(),
        &[None, Some("codex-native-retained".to_string())]
    );
    assert!(
        native_backend.runs().is_empty(),
        "native fallback must not run"
    );
}

#[tokio::test]
async fn gateway_doctor_and_catalog_refresh_use_distinct_explicit_snapshot_modes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let runtime = ProbeRecordingRuntime::default();
    let modes = Arc::clone(&runtime.modes);
    let host = RuntimeHost::new();
    host.register(RuntimeKind::Codex, Arc::new(runtime));
    let gateway =
        Gateway::with_backend_and_runtime_host(state, Arc::new(FakeBackend::default()), host);
    let query = psychevo_runtime_host::SnapshotQuery {
        profile: psychevo_runtime_host::RuntimeProfile {
            id: "codex".to_string(),
            label: "Codex".to_string(),
            kind: RuntimeKind::Codex,
            enabled: true,
            command: Some("fake-codex".to_string()),
            args: vec!["app-server".to_string(), "--stdio".to_string()],
            env: BTreeMap::new(),
            backend_ref: None,
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
            revision: 11,
            fingerprint: "probe-profile".to_string(),
        },
        scope: psychevo_runtime_host::SnapshotScope::Workspace { cwd },
        mode: psychevo_runtime_host::SnapshotMode::Cached,
    };
    assert!(gateway.cached_runtime_snapshot(&query).is_none());

    gateway
        .refresh_runtime_snapshot(query.clone())
        .await
        .expect("explicit refresh");
    assert_eq!(
        modes.lock().expect("snapshot modes").as_slice(),
        &[psychevo_runtime_host::SnapshotMode::BoundedProbe]
    );
    assert!(gateway.cached_runtime_snapshot(&query).is_some());

    gateway
        .refresh_runtime_catalog_snapshot(query)
        .await
        .expect("explicit catalog refresh");
    assert_eq!(
        modes.lock().expect("snapshot modes").as_slice(),
        &[
            psychevo_runtime_host::SnapshotMode::BoundedProbe,
            psychevo_runtime_host::SnapshotMode::CatalogRefresh,
        ]
    );
}

#[tokio::test]
async fn direct_runtime_injects_compatible_agent_instructions_without_conflating_native_agent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo/agents")).expect("agent dir");
    std::fs::write(
        cwd.join(".psychevo/agents/reviewer.md"),
        "---\ndescription: Direct reviewer\n---\nReview carefully with source evidence.\n",
    )
    .expect("agent definition");
    std::fs::write(
        cwd.join(".psychevo/agents/tool-review.md"),
        "---\ndescription: Tool-backed reviewer\ntools: [read]\n---\nReview with the required tool policy.\n",
    )
    .expect("tool-policy agent definition");
    std::fs::write(
        cwd.join(".psychevo/agents/writer.md"),
        "---\ndescription: Direct writer\n---\nWrite the implementation.\n",
    )
    .expect("second agent definition");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let executions = Arc::new(AtomicUsize::new(0));
    let turns = Arc::new(Mutex::new(Vec::new()));
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(BindingAssertingRuntime {
            state: state.clone(),
            executions: Arc::clone(&executions),
            turns: Arc::clone(&turns),
        }),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "direct-agent-pairing").persistent(),
        "review this change",
    );
    turn_request.options.runtime_ref = Some("codex".to_string());
    turn_request.options.agent = Some("reviewer".to_string());
    turn_request
        .options
        .runtime_options
        .insert("agent".to_string(), "explore".to_string());

    let result = harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect("direct turn");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(native_backend.runs().is_empty());
    {
        let captured = turns.lock().expect("captured turns");
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].instructions.as_deref(),
            Some("Review carefully with source evidence.")
        );
        assert_eq!(captured[0].agent.as_deref(), Some("explore"));
    }
    std::fs::write(
        harness.cwd.join(".psychevo/agents/reviewer.md"),
        "---\ndescription: Mutated reviewer\n---\nThese mutable instructions must not replace the bound snapshot.\n",
    )
    .expect("mutate agent definition after binding");

    let mut continued_agent = request(
        &harness,
        GatewaySource::new("web", "direct-agent-pairing-continued").persistent(),
        "continue with the bound persona",
    );
    continued_agent.thread_id = Some(result.result.session_id.clone());
    continued_agent.options.session = Some(result.result.session_id.clone());
    continued_agent.options.runtime_ref = Some("codex".to_string());
    continued_agent.options.agent = Some("reviewer".to_string());
    harness
        .gateway
        .send_turn(continued_agent)
        .await
        .expect("the bound direct Agent Definition continues on the same thread");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    {
        let captured = turns.lock().expect("captured turns");
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[1].instructions.as_deref(),
            Some("Review carefully with source evidence.")
        );
    }

    std::fs::remove_file(harness.cwd.join(".psychevo/agents/reviewer.md"))
        .expect("delete mutable agent definition after binding");
    let mut continued_after_delete = request(
        &harness,
        GatewaySource::new("web", "direct-agent-pairing-reconnected").persistent(),
        "continue after the mutable definition was deleted",
    );
    continued_after_delete.thread_id = Some(result.result.session_id.clone());
    continued_after_delete.options.session = Some(result.result.session_id.clone());
    continued_after_delete.options.runtime_ref = Some("codex".to_string());
    harness
        .gateway
        .send_turn(continued_after_delete)
        .await
        .expect("captured direct Agent instructions survive deletion and reconnect");
    assert_eq!(executions.load(Ordering::SeqCst), 3);
    {
        let captured = turns.lock().expect("captured turns");
        assert_eq!(captured.len(), 3);
        assert_eq!(
            captured[2].instructions.as_deref(),
            Some("Review carefully with source evidence.")
        );
    }

    let mut changed_agent = request(
        &harness,
        GatewaySource::new("web", "direct-agent-pairing-changed").persistent(),
        "change persona in place",
    );
    changed_agent.thread_id = Some(result.result.session_id.clone());
    changed_agent.options.session = Some(result.result.session_id);
    changed_agent.options.runtime_ref = Some("codex".to_string());
    changed_agent.options.agent = Some("writer".to_string());
    let error = harness
        .gateway
        .send_turn(changed_agent)
        .await
        .expect_err("stable direct Agent Definition binding is immutable");
    assert!(error.to_string().contains("Start a new thread"), "{error}");
    assert_eq!(executions.load(Ordering::SeqCst), 3);

    let mut unsupported = request(
        &harness,
        GatewaySource::new("web", "direct-agent-required-tools").persistent(),
        "must fail before direct prompt delivery",
    );
    unsupported.options.runtime_ref = Some("codex".to_string());
    unsupported.options.agent = Some("tool-review".to_string());
    let error = harness
        .gateway
        .send_turn(unsupported)
        .await
        .expect_err("required tool contribution must fail closed");
    assert!(error.to_string().contains("tool policy"), "{error}");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        3,
        "incompatible pairing must not reach the direct adapter"
    );
}

#[tokio::test]
async fn bound_direct_thread_keeps_its_effective_profile_after_config_is_deleted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let project_config = cwd.join(".psychevo/config.toml");
    std::fs::create_dir_all(project_config.parent().expect("project config parent"))
        .expect("project config dir");
    std::fs::write(
        &project_config,
        r#"
[runtime_profiles.codex]
runtime = "codex"
label = "Bound Codex"
command = "bound-codex"
args = ["app-server", "--stdio"]
[runtime_profiles.codex.env]
BOUND_RUNTIME_TOKEN = "captured-value"
"#,
    )
    .expect("project Runtime Profile");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let executions = Arc::new(AtomicUsize::new(0));
    let turns = Arc::new(Mutex::new(Vec::new()));
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(BindingAssertingRuntime {
            state: state.clone(),
            executions: Arc::clone(&executions),
            turns,
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        state.clone(),
        Arc::new(FakeBackend::default()),
        host,
    );
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let mut first = request(
        &harness,
        GatewaySource::new("web", "bound-profile-snapshot").persistent(),
        "capture the profile",
    );
    first.options.runtime_ref = Some("codex".to_string());
    let first = harness.gateway.send_turn(first).await.expect("first turn");
    let binding = harness
        .state
        .store()
        .gateway_runtime_binding(&first.result.session_id)
        .expect("binding")
        .expect("bound profile");
    let snapshot = binding
        .profile_config_json
        .as_deref()
        .expect("effective profile snapshot");
    assert!(snapshot.contains("bound-codex"));
    assert!(snapshot.contains("captured-value"));

    std::fs::remove_file(&project_config).expect("delete mutable Profile config");
    let mut continued = request(
        &harness,
        GatewaySource::new("web", "bound-profile-snapshot-continued").persistent(),
        "continue from the captured profile",
    );
    continued.thread_id = Some(first.result.session_id.clone());
    continued.options.session = Some(first.result.session_id.clone());
    continued.options.runtime_ref = Some("codex".to_string());
    let continued = harness
        .gateway
        .send_turn(continued)
        .await
        .expect("bound thread continues after mutable Profile deletion");
    assert_eq!(continued.result.session_id, first.result.session_id);
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn accepted_turn_records_one_failed_terminal_when_profile_resolution_fails() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone());
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "missing-runtime-profile").persistent(),
        "must fail before adapter execution",
    );
    turn.options.runtime_ref = Some("missing-runtime-profile".to_string());

    let error = harness
        .gateway
        .send_turn(turn)
        .await
        .expect_err("unknown Runtime Profile must fail");
    assert!(
        error.to_string().contains("missing-runtime-profile"),
        "{error}"
    );
    assert!(
        backend.runs().is_empty(),
        "configuration failure must not fall back native"
    );

    let sessions = harness
        .state
        .store()
        .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
        .expect("accepted session");
    assert_eq!(
        sessions.len(),
        1,
        "durable claim creates exactly one public thread"
    );
    let terminals = harness
        .state
        .store()
        .list_gateway_turn_terminals_for_thread(&sessions[0].id)
        .expect("turn terminals");
    assert_eq!(terminals.len(), 1, "accepted turn has exactly one terminal");
    assert_eq!(terminals[0].status, "failed");
    assert!(
        terminals[0]
            .error_message
            .as_ref()
            .is_some_and(|error| error.contains("missing-runtime-profile")),
        "{terminals:?}"
    );
}

#[derive(Debug)]
struct MissingResultSessionBackend;

impl GatewayBackend for MissingResultSessionBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Psychevo
    }

    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
        Box::pin(async move {
            Ok(RunResult {
                session_id: "missing-backend-result-session".to_string(),
                outcome: Outcome::Normal,
                terminal_reason: None,
                final_answer: "backend returned an invalid session".to_string(),
                db_path: request.options.state.db_path().to_path_buf(),
                cwd: request.options.cwd,
                provider: "fake".to_string(),
                model: "fake".to_string(),
                base_url: String::new(),
                api_key_env: None,
                reasoning_effort: None,
                context_limit: None,
                tool_failures: 0,
                selected_agent: None,
                selected_skills: Vec::new(),
                context_snapshot: None,
                terminal_error: None,
                events: Vec::new(),
                warnings: Vec::new(),
            })
        })
    }
}

#[tokio::test]
async fn accepted_turn_records_failed_terminal_when_post_backend_projection_fails() {
    let harness = harness(Arc::new(FakeBackend::default()));
    let gateway =
        Gateway::with_backend(harness.state.clone(), Arc::new(MissingResultSessionBackend));
    let error = gateway
        .send_turn(request(
            &harness,
            GatewaySource::new("web", "post-backend-terminal").persistent(),
            "must retain a terminal",
        ))
        .await
        .expect_err("missing backend result session must fail projection");
    assert!(
        error.to_string().contains("missing-backend-result-session"),
        "{error}"
    );

    let sessions = harness
        .state
        .store()
        .list_sessions_for_cwd_with_sources(&harness.cwd, &["test"])
        .expect("accepted public thread");
    assert_eq!(sessions.len(), 1);
    let terminals = harness
        .state
        .store()
        .list_gateway_turn_terminals_for_thread(&sessions[0].id)
        .expect("terminal lookup");
    assert_eq!(
        terminals.len(),
        1,
        "accepted turn must have exactly one terminal"
    );
    assert_eq!(terminals[0].status, "failed");
}

#[derive(Debug, Clone)]
struct TeamDirectRuntime {
    state: StateRuntime,
    parent_thread_id: String,
    turns: Arc<Mutex<Vec<psychevo_runtime_host::RuntimeTurnRequest>>>,
}

impl psychevo_runtime_host::RuntimeModule for TeamDirectRuntime {
    fn snapshot(
        &self,
        query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async move {
            let dependency = Some(psychevo_runtime_host::RuntimeControlDependency {
                control_id: "model".to_string(),
                value: json!("team-model"),
            });
            Ok(psychevo_runtime_host::RuntimeSnapshot {
                runtime_ref: query.profile.id,
                kind: query.profile.kind,
                profile_revision: query.profile.revision,
                capability_revision: 1,
                adapter_version: "team-direct-fixture".to_string(),
                runtime_version: Some("codex_cli_rs/0.143.0-fixture".to_string()),
                stability: psychevo_runtime_host::RuntimeStability::Stable,
                provenance: "direct".to_string(),
                readiness: Vec::new(),
                controls: vec![
                    psychevo_runtime_host::RuntimeControlDescriptor {
                        id: "model".to_string(),
                        label: "Model".to_string(),
                        state: psychevo_runtime_host::ControlState::Selectable,
                        current_value: None,
                        choices: vec![psychevo_runtime_host::RuntimeControlChoice {
                            value: json!("team-model"),
                            label: "Team model".to_string(),
                            description: Some("Deterministic Team model".to_string()),
                        }],
                        depends_on: None,
                        channel_safe: false,
                        capability_revision: 1,
                    },
                    psychevo_runtime_host::RuntimeControlDescriptor {
                        id: "effort".to_string(),
                        label: "Reasoning effort".to_string(),
                        state: psychevo_runtime_host::ControlState::Selectable,
                        current_value: None,
                        choices: vec![psychevo_runtime_host::RuntimeControlChoice {
                            value: json!("high"),
                            label: "High".to_string(),
                            description: Some("Deeper reasoning".to_string()),
                        }],
                        depends_on: dependency,
                        channel_safe: false,
                        capability_revision: 1,
                    },
                ],
                capabilities: Vec::new(),
                process_epoch: Some(1),
                instance_epoch: None,
                binding_epoch: None,
                extension: Some(json!({
                    "codex": { "controlModel": "team-model" }
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
        let state = self.state.clone();
        let parent_thread_id = self.parent_thread_id.clone();
        let turns = Arc::clone(&self.turns);
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Turn(turn) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "Team delegate fake only supports turns",
                ));
            };
            let binding = state
                .store()
                .gateway_runtime_binding(&turn.thread_id)
                .map_err(|error| {
                    psychevo_runtime_host::RuntimeError::new(
                        "binding_read",
                        psychevo_runtime_host::RuntimeErrorStage::Binding,
                        psychevo_runtime_host::RetryClass::Never,
                        error.to_string(),
                    )
                })?
                .ok_or_else(|| {
                    psychevo_runtime_host::RuntimeError::new(
                        "binding_missing",
                        psychevo_runtime_host::RuntimeErrorStage::Binding,
                        psychevo_runtime_host::RetryClass::Never,
                        "Team child binding must exist before native prompt delivery",
                    )
                })?;
            if binding.runtime_ref.as_deref() != Some("review-codex")
                || binding.parent_thread_id.as_deref() != Some(parent_thread_id.as_str())
                || binding.native_session_id.is_some()
            {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "binding_mismatch",
                    psychevo_runtime_host::RuntimeErrorStage::Binding,
                    psychevo_runtime_host::RetryClass::Never,
                    "Team child binding lost runtime or parent provenance",
                ));
            }
            turns
                .lock()
                .expect("Team turn capture poisoned")
                .push(turn.clone());
            Ok(psychevo_runtime_host::ExecuteResult::Turn(
                psychevo_runtime_host::RuntimeTurnResult {
                    turn_id: turn.turn_id,
                    thread_id: turn.thread_id,
                    native_session_id: "team-codex-native-1".to_string(),
                    outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                    final_answer: "runtime-backed review".to_string(),
                    provider: "codex".to_string(),
                    model: "team-model".to_string(),
                    history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                    process_epoch: 1,
                    instance_epoch: None,
                    terminal_error: None,
                    metadata: None,
                },
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn team_delegate_executes_configured_direct_profile_without_native_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("project config dir");
    std::fs::write(
        cwd.join(".psychevo/config.toml"),
        r#"[runtime_profiles.review-codex]
runtime = "codex"
label = "Review Codex"
command = "fake-codex"
default_mode = "default"
"#,
    )
    .expect("runtime profile config");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let parent_thread_id = state
        .store()
        .create_session_with_metadata(&cwd, "run", "leader", "fake", None)
        .expect("parent session");
    let child_thread_id = state
        .store()
        .create_child_session_with_metadata(
            &parent_thread_id,
            &cwd,
            "runtime",
            "team-model",
            "review-codex",
            Some(json!({
                "teamRunId": "team-run-1",
                "teamMemberId": "reviewer",
                "runtimeRef": "review-codex",
            })),
        )
        .expect("precreated child");
    let native_backend = Arc::new(FakeBackend::default());
    let turns = Arc::new(Mutex::new(Vec::new()));
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(TeamDirectRuntime {
            state: state.clone(),
            parent_thread_id: parent_thread_id.clone(),
            turns: Arc::clone(&turns),
        }),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway: gateway.clone(),
    };
    let mut base_options = run_options(&harness, "unused leader prompt");
    base_options.runtime_ref = Some("review-codex".to_string());
    let (captured_profile, captured_profile_revision, captured_profile_fingerprint) =
        resolve_gateway_runtime_profile(&base_options).expect("Team Runtime Profile");
    gateway
        .observe_runtime_snapshot(psychevo_runtime_host::SnapshotQuery {
            profile: crate::gateway_runtime_profile(
                captured_profile,
                captured_profile_revision,
                captured_profile_fingerprint,
            ),
            scope: psychevo_runtime_host::SnapshotScope::Workspace {
                cwd: harness.cwd.clone(),
            },
            mode: psychevo_runtime_host::SnapshotMode::Cached,
        })
        .await
        .expect("cache Team model catalog");
    let delegate = GatewayExternalAgentDelegate {
        gateway,
        base_options,
        stream: None,
        event_sink: None,
    };
    let (_stale_abort_tx, stale_abort_rx) = tokio::sync::watch::channel(false);
    let stale = delegate
        .run(ExternalAgentDelegateRequest {
            run_id: "managed-child-stale-turn".to_string(),
            parent_session_id: parent_thread_id.clone(),
            child_session_id: child_thread_id.clone(),
            agent_name: "reviewer".to_string(),
            agent_description: "Review the change".to_string(),
            runtime_ref: "review-codex".to_string(),
            backend_ref: None,
            instructions: Some("Review carefully and report evidence.".to_string()),
            prompt: "Review this patch.".to_string(),
            task_name: "review_patch".to_string(),
            model: Some("team-model".to_string()),
            runtime_options: BTreeMap::from([(
                "mode".to_string(),
                "auto-review".to_string(),
            )]),
            expected_runtime_profile_revision: Some(captured_profile_revision.wrapping_add(1)),
            abort: AbortSignal::new(stale_abort_rx),
        })
        .await
        .expect_err("stale Team Profile revision must fail before adapter execution");
    assert!(
        format!("{stale:?}").contains("Re-save or reactivate the Team"),
        "unexpected stale Team error: {stale:?}"
    );
    assert!(turns.lock().expect("Team turns").is_empty());
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let result = delegate
        .run(ExternalAgentDelegateRequest {
            run_id: "managed-child-turn-1".to_string(),
            parent_session_id: parent_thread_id.clone(),
            child_session_id: child_thread_id.clone(),
            agent_name: "reviewer".to_string(),
            agent_description: "Review the change".to_string(),
            runtime_ref: "review-codex".to_string(),
            backend_ref: None,
            instructions: Some("Review carefully and report evidence.".to_string()),
            prompt: "Review this patch.".to_string(),
            task_name: "review_patch".to_string(),
            model: Some("team-model".to_string()),
            runtime_options: BTreeMap::from([
                ("mode".to_string(), "auto-review".to_string()),
                ("effort".to_string(), "high".to_string()),
            ]),
            expected_runtime_profile_revision: Some(captured_profile_revision),
            abort: AbortSignal::new(abort_rx),
        })
        .await
        .expect("direct Team delegate");

    assert_eq!(result.child_session_id, child_thread_id);
    assert_eq!(result.final_answer, "runtime-backed review");
    assert!(
        native_backend.runs().is_empty(),
        "native fallback must not run"
    );
    let captured = turns.lock().expect("Team turns");
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0].instructions.as_deref(),
        Some("Review carefully and report evidence.")
    );
    assert_eq!(captured[0].model.as_deref(), Some("team-model"));
    assert_eq!(captured[0].mode.as_deref(), Some("auto-review"));
    assert_eq!(captured[0].agent, None);
    assert_eq!(captured[0].features["effort"], json!("high"));
    assert!(!captured[0].features.contains_key("model"));
    assert!(!captured[0].features.contains_key("mode"));
    assert!(!captured[0].features.contains_key("agent"));
    drop(captured);
    let binding = harness
        .state
        .store()
        .gateway_runtime_binding(&child_thread_id)
        .expect("binding")
        .expect("child binding");
    assert_eq!(binding.runtime_ref.as_deref(), Some("review-codex"));
    assert_eq!(
        binding.native_session_id.as_deref(),
        Some("team-codex-native-1")
    );
    assert_eq!(
        binding.parent_thread_id.as_deref(),
        Some(parent_thread_id.as_str())
    );
    assert_eq!(
        harness
            .state
            .store()
            .load_messages(&child_thread_id)
            .expect("child messages")
            .len(),
        2
    );
    let terminals = harness
        .state
        .store()
        .list_gateway_turn_terminals_for_thread(&child_thread_id)
        .expect("Team child terminals");
    assert_eq!(
        terminals.len(),
        2,
        "each accepted Team turn has one terminal"
    );
    assert!(terminals.iter().any(|terminal| {
        terminal.turn_id == "managed-child-stale-turn" && terminal.status == "failed"
    }));
    assert!(terminals.iter().any(|terminal| {
        terminal.turn_id == "managed-child-turn-1" && terminal.status == "completed"
    }));
}

#[test]
fn runtime_lifecycle_events_are_durable_and_keep_public_thread_scope() {
    let state = GatewayEvent::RuntimeStateChanged {
        runtime_ref: "codex".to_string(),
        thread_id: Some("parent-thread".to_string()),
        state: "ready".to_string(),
        detail: None,
        process_epoch: 7,
        instance_epoch: None,
    };
    assert!(should_append_gateway_live_event(
        &DurableGatewayActivity {
            activity_id: "turn-1".to_string(),
            owner_id: "owner-1".to_string(),
            generation: 1,
            turn_id: Some("turn-1".to_string()),
            kind: "turn".to_string(),
        },
        &state,
    ));
    assert_eq!(
        gateway_event_thread_id(&state).as_deref(),
        Some("parent-thread")
    );

    let child = GatewayEvent::RuntimeChildChanged {
        runtime_ref: "opencode".to_string(),
        parent_thread_id: "parent-thread".to_string(),
        thread_id: Some("public-child-thread".to_string()),
        native_dedup_key: "opaque-child-key".to_string(),
        status: "running".to_string(),
        read_only: true,
    };
    assert!(should_append_gateway_live_event(
        &DurableGatewayActivity {
            activity_id: "turn-1".to_string(),
            owner_id: "owner-1".to_string(),
            generation: 1,
            turn_id: Some("turn-1".to_string()),
            kind: "turn".to_string(),
        },
        &child,
    ));
    assert_eq!(
        gateway_event_thread_id(&child).as_deref(),
        Some("public-child-thread")
    );
}

#[derive(Debug, Clone, Default)]
struct InteractiveRuntime {
    pending: Arc<Mutex<Option<oneshot::Sender<Value>>>>,
    asks_questions: bool,
    gui_advanced_question: bool,
}

#[derive(Debug, Clone, Default)]
struct CrossTurnChildInteractionRuntime {
    turn_count: Arc<AtomicUsize>,
    pending: Arc<Mutex<Option<oneshot::Sender<Value>>>>,
}

pub(super) fn gateway_with_cross_turn_child_interaction(state: StateRuntime) -> Gateway {
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(CrossTurnChildInteractionRuntime::default()),
    );
    Gateway::with_backend_and_runtime_host(state, Arc::new(FakeBackend::default()), host)
}

impl psychevo_runtime_host::RuntimeModule for CrossTurnChildInteractionRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::UserAction,
                "snapshot is outside this cross-turn interaction test",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let turn_count = Arc::clone(&self.turn_count);
        let pending = Arc::clone(&self.pending);
        Box::pin(async move {
            match request.intent {
                psychevo_runtime_host::RuntimeIntent::Turn(turn) => {
                    let turn_number = turn_count.fetch_add(1, Ordering::SeqCst);
                    observer
                        .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                            runtime_ref: request.profile.id.clone(),
                            thread_id: turn.thread_id.clone(),
                            native_session_id: "codex-native-parent-cross-turn".to_string(),
                            cwd: turn.cwd.clone(),
                            binding_epoch: turn.binding_epoch,
                            process_epoch: 13,
                            instance_epoch: None,
                        })
                        .await?;
                    if turn_number == 0 {
                        observer.emit(psychevo_runtime_host::RuntimeObservation::ChildChanged {
                            runtime_ref: request.profile.id,
                            parent_native_session_id: "codex-native-parent-cross-turn".to_string(),
                            native_session_id: "codex-native-child-cross-turn".to_string(),
                            thread_id: None,
                            status: "idle".to_string(),
                            read_only: true,
                        });
                    } else {
                        let (sender, receiver) = oneshot::channel();
                        *pending.lock().expect("cross-turn interaction pending poisoned") =
                            Some(sender);
                        observer.emit(psychevo_runtime_host::RuntimeObservation::Interaction(
                            Box::new(psychevo_runtime_host::RuntimeInteraction {
                                id: "codex-native-request-cross-turn".to_string(),
                                policy: psychevo_runtime_host::RuntimeInteractionPolicy {
                                    kind: psychevo_runtime_host::RuntimeInteractionKind::Permission,
                                    stability: psychevo_runtime_host::RuntimeStability::Stable,
                                    exposure:
                                        psychevo_runtime_host::RuntimeInteractionExposure::Standard,
                                },
                                kind: "command".to_string(),
                                runtime_ref: request.profile.id,
                                thread_id: turn.thread_id.clone(),
                                native_session_id: "codex-native-child-cross-turn".to_string(),
                                parent_native_session_id: Some(
                                    "codex-native-parent-cross-turn".to_string(),
                                ),
                                child_native_session_id: Some(
                                    "codex-native-child-cross-turn".to_string(),
                                ),
                                process_epoch: 13,
                                instance_epoch: None,
                                prompt: "Allow the child-scoped command?".to_string(),
                                questions: Vec::new(),
                                choices: vec![
                                    psychevo_runtime_host::RuntimeInteractionChoice {
                                        id: "accept".to_string(),
                                        label: "Allow once".to_string(),
                                        decision: "accept".to_string(),
                                    },
                                    psychevo_runtime_host::RuntimeInteractionChoice {
                                        id: "accept_for_session".to_string(),
                                        label: "Allow for this Codex session".to_string(),
                                        decision: "acceptForSession".to_string(),
                                    },
                                    psychevo_runtime_host::RuntimeInteractionChoice {
                                        id: "decline".to_string(),
                                        label: "Deny".to_string(),
                                        decision: "decline".to_string(),
                                    },
                                ],
                                authorization_lifetime: Some("codex_session".to_string()),
                                expires_at_ms: None,
                                metadata: None,
                            }),
                        ));
                        let response = receiver.await.map_err(|_| {
                            psychevo_runtime_host::RuntimeError::new(
                                "interaction_cancelled",
                                psychevo_runtime_host::RuntimeErrorStage::Interaction,
                                psychevo_runtime_host::RetryClass::Never,
                                "cross-turn interaction response channel closed",
                            )
                        })?;
                        if response != json!({"decision": "acceptForSession"}) {
                            return Err(psychevo_runtime_host::RuntimeError::new(
                                "interaction_rejected",
                                psychevo_runtime_host::RuntimeErrorStage::Interaction,
                                psychevo_runtime_host::RetryClass::Never,
                                "unexpected cross-turn interaction response",
                            ));
                        }
                    }
                    Ok(psychevo_runtime_host::ExecuteResult::Turn(
                        psychevo_runtime_host::RuntimeTurnResult {
                            turn_id: turn.turn_id,
                            thread_id: turn.thread_id,
                            native_session_id: "codex-native-parent-cross-turn".to_string(),
                            outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                            final_answer: if turn_number == 0 {
                                "child observed"
                            } else {
                                "child permission accepted"
                            }
                            .to_string(),
                            provider: "codex".to_string(),
                            model: "fake-codex".to_string(),
                            history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                            process_epoch: 13,
                            instance_epoch: None,
                            terminal_error: None,
                            metadata: None,
                        },
                    ))
                }
                psychevo_runtime_host::RuntimeIntent::Interaction(response) => {
                    let sender = pending
                        .lock()
                        .expect("cross-turn interaction pending poisoned")
                        .take()
                        .ok_or_else(|| {
                            psychevo_runtime_host::RuntimeError::new(
                                "interaction_expired",
                                psychevo_runtime_host::RuntimeErrorStage::Interaction,
                                psychevo_runtime_host::RetryClass::UserAction,
                                "cross-turn interaction is no longer pending",
                            )
                        })?;
                    sender.send(response.response).map_err(|_| {
                        psychevo_runtime_host::RuntimeError::new(
                            "interaction_expired",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::UserAction,
                            "cross-turn interaction receiver closed",
                        )
                    })?;
                    Ok(psychevo_runtime_host::ExecuteResult::Interaction(
                        psychevo_runtime_host::RuntimeInteractionResult {
                            accepted: true,
                            expired: false,
                            message: None,
                        },
                    ))
                }
                _ => Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "unsupported cross-turn test intent",
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

impl InteractiveRuntime {
    fn question() -> Self {
        Self {
            asks_questions: true,
            ..Self::default()
        }
    }

    fn gui_advanced_question() -> Self {
        Self {
            asks_questions: true,
            gui_advanced_question: true,
            ..Self::default()
        }
    }
}

impl psychevo_runtime_host::RuntimeModule for InteractiveRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::UserAction,
                "snapshot is outside this interaction test",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let pending = Arc::clone(&self.pending);
        let asks_questions = self.asks_questions;
        let gui_advanced_question = self.gui_advanced_question;
        Box::pin(async move {
            match request.intent {
                psychevo_runtime_host::RuntimeIntent::Turn(turn) => {
                    let (sender, receiver) = oneshot::channel();
                    *pending.lock().expect("interaction pending poisoned") = Some(sender);
                    observer
                        .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                            runtime_ref: request.profile.id.clone(),
                            thread_id: turn.thread_id.clone(),
                            native_session_id: "codex-native-interaction".to_string(),
                            cwd: turn.cwd.clone(),
                            binding_epoch: 1,
                            process_epoch: 7,
                            instance_epoch: None,
                        })
                        .await?;
                    observer.emit(psychevo_runtime_host::RuntimeObservation::ChildChanged {
                        runtime_ref: request.profile.id.clone(),
                        parent_native_session_id: "codex-native-interaction".to_string(),
                        native_session_id: "codex-native-child".to_string(),
                        thread_id: None,
                        status: "running".to_string(),
                        read_only: true,
                    });
                    observer.emit(psychevo_runtime_host::RuntimeObservation::ChildChanged {
                        runtime_ref: request.profile.id.clone(),
                        parent_native_session_id: "codex-native-child".to_string(),
                        native_session_id: "codex-native-grandchild".to_string(),
                        thread_id: None,
                        status: "idle".to_string(),
                        read_only: true,
                    });
                    let interaction = if asks_questions {
                        psychevo_runtime_host::RuntimeInteraction {
                            id: "native-question-1".to_string(),
                            policy: psychevo_runtime_host::RuntimeInteractionPolicy {
                                kind: psychevo_runtime_host::RuntimeInteractionKind::Question,
                                stability: if gui_advanced_question {
                                    psychevo_runtime_host::RuntimeStability::Experimental
                                } else {
                                    psychevo_runtime_host::RuntimeStability::Stable
                                },
                                exposure: if gui_advanced_question {
                                    psychevo_runtime_host::RuntimeInteractionExposure::GuiAdvancedOnly
                                } else {
                                    psychevo_runtime_host::RuntimeInteractionExposure::Standard
                                },
                            },
                            kind: "question".to_string(),
                            runtime_ref: request.profile.id,
                            thread_id: turn.thread_id.clone(),
                            native_session_id: "codex-native-child".to_string(),
                            parent_native_session_id: Some("codex-native-interaction".to_string()),
                            child_native_session_id: Some("codex-native-child".to_string()),
                            process_epoch: 7,
                            instance_epoch: None,
                            prompt: if gui_advanced_question {
                                "native-experimental-question-secret".to_string()
                            } else {
                                "Choose a target and checks".to_string()
                            },
                            questions: vec![
                                psychevo_runtime_host::RuntimeInteractionQuestion {
                                    header: Some("Target".to_string()),
                                    question: if gui_advanced_question {
                                        "native-experimental-question-secret".to_string()
                                    } else {
                                        "Which target?".to_string()
                                    },
                                    options: vec![
                                        psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                            label: "Core".to_string(),
                                            description: "Inspect core".to_string(),
                                        },
                                        psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                            label: "TUI".to_string(),
                                            description: "Inspect TUI".to_string(),
                                        },
                                    ],
                                    multiple: false,
                                    custom: true,
                                    secret: false,
                                },
                                psychevo_runtime_host::RuntimeInteractionQuestion {
                                    header: Some("Checks".to_string()),
                                    question: "Which checks?".to_string(),
                                    options: vec![
                                        psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                            label: "Tests".to_string(),
                                            description: "Run tests".to_string(),
                                        },
                                        psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                            label: "Clippy".to_string(),
                                            description: "Run lint".to_string(),
                                        },
                                    ],
                                    multiple: true,
                                    custom: false,
                                    secret: false,
                                },
                            ],
                            choices: vec![psychevo_runtime_host::RuntimeInteractionChoice {
                                id: "flattened-wrong-choice".to_string(),
                                label: "Must not be projected as a question".to_string(),
                                decision: "wrong".to_string(),
                            }],
                            authorization_lifetime: None,
                            expires_at_ms: None,
                            metadata: Some(json!({"questions": "must not be used"})),
                        }
                    } else {
                        psychevo_runtime_host::RuntimeInteraction {
                            id: "native-permission-1".to_string(),
                            policy: psychevo_runtime_host::RuntimeInteractionPolicy {
                                kind: psychevo_runtime_host::RuntimeInteractionKind::Permission,
                                stability: psychevo_runtime_host::RuntimeStability::Stable,
                                exposure:
                                    psychevo_runtime_host::RuntimeInteractionExposure::Standard,
                            },
                            kind: "permission".to_string(),
                            runtime_ref: request.profile.id,
                            thread_id: turn.thread_id.clone(),
                            native_session_id: "codex-native-child".to_string(),
                            parent_native_session_id: Some("codex-native-interaction".to_string()),
                            child_native_session_id: Some("codex-native-child".to_string()),
                            process_epoch: 7,
                            instance_epoch: None,
                            prompt: "Allow the deterministic fake tool?".to_string(),
                            questions: Vec::new(),
                            choices: vec![
                                psychevo_runtime_host::RuntimeInteractionChoice {
                                    id: "allow_once".to_string(),
                                    label: "Allow once".to_string(),
                                    decision: "accept".to_string(),
                                },
                                psychevo_runtime_host::RuntimeInteractionChoice {
                                    id: "allow_session".to_string(),
                                    label: "Allow for session".to_string(),
                                    decision: "acceptForSession".to_string(),
                                },
                                psychevo_runtime_host::RuntimeInteractionChoice {
                                    id: "deny".to_string(),
                                    label: "Deny".to_string(),
                                    decision: "decline".to_string(),
                                },
                            ],
                            authorization_lifetime: Some("codex_session".to_string()),
                            expires_at_ms: None,
                            metadata: None,
                        }
                    };
                    observer.emit(psychevo_runtime_host::RuntimeObservation::Interaction(
                        Box::new(interaction),
                    ));
                    let response = receiver.await.map_err(|_| {
                        psychevo_runtime_host::RuntimeError::new(
                            "interaction_cancelled",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::Never,
                            "interaction response channel closed",
                        )
                    })?;
                    let response_is_valid = if gui_advanced_question {
                        response == json!({"reject": true, "decision": "cancel"})
                    } else if asks_questions {
                        response == json!({"answers": [["TUI"], ["Tests", "Clippy"]]})
                    } else {
                        response["decision"] == "accept"
                    };
                    if !response_is_valid {
                        return Err(psychevo_runtime_host::RuntimeError::new(
                            "interaction_rejected",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::Never,
                            "unexpected interaction response",
                        ));
                    }
                    Ok(psychevo_runtime_host::ExecuteResult::Turn(
                        psychevo_runtime_host::RuntimeTurnResult {
                            turn_id: turn.turn_id,
                            thread_id: turn.thread_id,
                            native_session_id: "codex-native-interaction".to_string(),
                            outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                            final_answer: if gui_advanced_question {
                                "experimental question declined".to_string()
                            } else if asks_questions {
                                "questions answered".to_string()
                            } else {
                                "permission accepted".to_string()
                            },
                            provider: "codex".to_string(),
                            model: "fake-codex".to_string(),
                            history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                            process_epoch: 7,
                            instance_epoch: None,
                            terminal_error: None,
                            metadata: None,
                        },
                    ))
                }
                psychevo_runtime_host::RuntimeIntent::Interaction(response) => {
                    let sender = pending
                        .lock()
                        .expect("interaction pending poisoned")
                        .take()
                        .ok_or_else(|| {
                            psychevo_runtime_host::RuntimeError::new(
                                "interaction_expired",
                                psychevo_runtime_host::RuntimeErrorStage::Interaction,
                                psychevo_runtime_host::RetryClass::UserAction,
                                "interaction is no longer pending",
                            )
                        })?;
                    sender.send(response.response).map_err(|_| {
                        psychevo_runtime_host::RuntimeError::new(
                            "interaction_expired",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::UserAction,
                            "interaction receiver closed",
                        )
                    })?;
                    Ok(psychevo_runtime_host::ExecuteResult::Interaction(
                        psychevo_runtime_host::RuntimeInteractionResult {
                            accepted: true,
                            expired: false,
                            message: None,
                        },
                    ))
                }
                _ => Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "unsupported test intent",
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

#[test]
fn direct_runtime_permission_choices_require_an_enforceable_declared_lifetime() {
    let mut interaction = psychevo_runtime_host::RuntimeInteraction {
        id: "permission".to_string(),
        policy: psychevo_runtime_host::RuntimeInteractionPolicy {
            kind: psychevo_runtime_host::RuntimeInteractionKind::Permission,
            stability: psychevo_runtime_host::RuntimeStability::Stable,
            exposure: psychevo_runtime_host::RuntimeInteractionExposure::Standard,
        },
        kind: "permission".to_string(),
        runtime_ref: "opencode".to_string(),
        thread_id: "public-parent".to_string(),
        native_session_id: "native-parent".to_string(),
        parent_native_session_id: None,
        child_native_session_id: None,
        process_epoch: 1,
        instance_epoch: Some(2),
        prompt: "Allow?".to_string(),
        questions: Vec::new(),
        choices: vec![
            psychevo_runtime_host::RuntimeInteractionChoice {
                id: "once".to_string(),
                label: "Allow once".to_string(),
                decision: "once".to_string(),
            },
            psychevo_runtime_host::RuntimeInteractionChoice {
                id: "always".to_string(),
                label: "Allow until restart".to_string(),
                decision: "always".to_string(),
            },
        ],
        authorization_lifetime: None,
        expires_at_ms: None,
        metadata: None,
    };

    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowOnce).is_some()
    );
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowSession).is_none()
    );
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowAlways).is_none()
    );

    interaction.authorization_lifetime = Some("until_runtime_instance_restarts".to_string());
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowSession).is_some()
    );
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowAlways).is_none()
    );

    interaction.authorization_lifetime = Some("permanent".to_string());
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowAlways).is_some()
    );

    interaction.kind = "command".to_string();
    assert_eq!(
        runtime_interaction_action_kind(&interaction),
        GatewayActionKind::Permission
    );
    assert!(
        runtime_permission_choice(&interaction, PermissionApprovalOutcome::AllowOnce).is_some()
    );
}

#[tokio::test]
async fn gateway_permission_attention_declares_session_only_for_an_enforcing_profile() {
    async fn projected_action(lifetime: Option<&'static str>) -> PendingActionView {
        let pending: PendingPermissionMap = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let handler = GatewayApprovalHandler::new(
            None,
            Arc::clone(&pending),
            Arc::new(move |event| {
                let _ = event_tx.send(event);
            }),
            lifetime,
        );
        let task = tokio::spawn(async move {
            handler
                .request_permission(PermissionApprovalRequest {
                    tool_call_id: "permission-lifetime".to_string(),
                    tool_name: "fake_tool".to_string(),
                    summary: "fake permission".to_string(),
                    reason: "test permission".to_string(),
                    matched_rule: None,
                    suggested_rule: None,
                    allow_always: true,
                    timeout_secs: 300,
                })
                .await
        });
        let action = match event_rx.recv().await.expect("permission attention event") {
            GatewayEvent::ActionRequested { action } => action,
            event => panic!("unexpected event: {event:?}"),
        };
        pending
            .lock()
            .expect("pending permissions poisoned")
            .remove("permission-lifetime")
            .expect("pending permission")
            .responder
            .send(PermissionApprovalDecision::deny())
            .expect("permission response");
        task.await.expect("permission task");
        action
    }

    let acp = projected_action(None).await;
    assert_eq!(acp.payload["allowSession"], false);
    assert!(acp.payload["authorizationLifetime"].is_null());
    assert_eq!(acp.payload["allowAlways"], true);
    assert_eq!(acp.payload["alwaysAuthorizationLifetime"], "permanent");

    let native = projected_action(Some("psychevo_session")).await;
    assert_eq!(native.payload["allowSession"], true);
    assert_eq!(native.payload["authorizationLifetime"], "psychevo_session");
}

#[tokio::test]
async fn child_interaction_from_a_later_turn_keeps_public_shared_attention_origin() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(CrossTurnChildInteractionRuntime::default()),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway: gateway.clone(),
    };
    let source = GatewaySource::new("web", "cross-turn-child-attention").persistent();
    let mut first = request(&harness, source.clone(), "observe child");
    first.options.runtime_ref = Some("codex".to_string());
    let first = harness
        .gateway
        .send_turn(first)
        .await
        .expect("first direct turn");
    let parent_thread_id = first.result.session_id;
    let child = harness
        .state
        .store()
        .gateway_runtime_binding_by_native_session("codex", "codex-native-child-cross-turn")
        .expect("child binding read")
        .expect("first turn projects child binding");

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut second = request(&harness, source, "request child command approval");
    second.thread_id = Some(parent_thread_id.clone());
    second.options.session = Some(parent_thread_id.clone());
    second.options.runtime_ref = Some("codex".to_string());
    second.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));
    let run = tokio::spawn(async move { gateway.send_turn(second).await });

    let action = loop {
        let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("cross-turn attention timeout")
            .expect("cross-turn attention event channel");
        if let GatewayEvent::ActionRequested { action } = event {
            break action;
        }
    };
    assert_eq!(action.kind, GatewayActionKind::Permission);
    assert_eq!(action.thread_id.as_deref(), Some(parent_thread_id.as_str()));
    assert_eq!(
        action.payload["origin"]["parentThreadId"],
        parent_thread_id
    );
    assert_eq!(action.payload["origin"]["childThreadId"], child.thread_id);
    assert_eq!(action.payload["allowSession"], true);
    assert_eq!(action.payload["allowAlways"], false);
    assert_eq!(action.payload["authorizationLifetime"], "codex_session");
    assert!(
        harness.gateway.has_pending_permission_for_selector(
            &GatewayThreadSelector::thread_id(&parent_thread_id),
            &action.action_id,
        ),
        "the session-view liveness predicate must retain pending direct-runtime permissions",
    );
    let public_action = serde_json::to_string(&action).expect("public action JSON");
    for native_id in [
        "codex-native-parent-cross-turn",
        "codex-native-child-cross-turn",
        "codex-native-request-cross-turn",
    ] {
        assert!(!public_action.contains(native_id), "{public_action}");
    }

    assert!(harness.gateway.submit_permission(
        GatewayThreadSelector::thread_id(parent_thread_id),
        &action.action_id,
        PermissionApprovalDecision::allow_session(),
    ));
    let result = run
        .await
        .expect("second turn join")
        .expect("second direct turn");
    assert_eq!(result.result.final_answer, "child permission accepted");
    assert!(native_backend.runs().is_empty());
}

#[tokio::test]
async fn direct_runtime_interaction_round_trips_through_shared_attention() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let host = RuntimeHost::new();
    host.register(RuntimeKind::Codex, Arc::new(InteractiveRuntime::default()));
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway: gateway.clone(),
    };
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "direct-runtime-interaction").persistent(),
        "request permission",
    );
    turn.options.runtime_ref = Some("codex".to_string());
    turn.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));
    let run = tokio::spawn(async move { gateway.send_turn(turn).await });

    let action = loop {
        let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("attention timeout")
            .expect("attention event channel");
        if let GatewayEvent::ActionRequested { action } = event {
            break action;
        }
    };
    let action_id = action.action_id.clone();
    let thread_id = action
        .thread_id
        .clone()
        .expect("runtime interaction thread");
    assert!(action_id.starts_with("rt_"));
    assert!(action_id.len() <= 13, "Channel token stays short");
    assert_eq!(action.payload["runtimeRef"], "codex");
    assert_eq!(action.payload["runtimeKind"], "codex");
    assert_eq!(action.payload["profileLabel"], "Codex");
    assert_eq!(action.payload["allowSession"], true);
    assert_eq!(action.payload["allowAlways"], false);
    assert_eq!(action.payload["authorizationLifetime"], "codex_session");
    assert_eq!(action.payload["origin"]["parentThreadId"], thread_id);
    let child_thread_id = action.payload["origin"]["childThreadId"]
        .as_str()
        .expect("public child origin")
        .to_string();
    let action_payload = action.payload.to_string();
    assert!(!action_payload.contains("codex-native-child"));
    assert!(!action_payload.contains("codex-native-interaction"));
    assert!(harness.gateway.submit_permission(
        GatewayThreadSelector::thread_id(thread_id),
        &action_id,
        PermissionApprovalDecision::allow_once(),
    ));
    let result = run
        .await
        .expect("turn join")
        .expect("runtime interaction turn");
    assert_eq!(result.result.final_answer, "permission accepted");
    assert!(
        native_backend.runs().is_empty(),
        "native fallback must not run"
    );
    let child = harness
        .state
        .store()
        .gateway_runtime_binding_by_native_session("codex", "codex-native-child")
        .expect("child binding")
        .expect("runtime child projected");
    assert_eq!(child.ownership, GatewayRuntimeBindingOwnership::ReadOnly);
    assert_eq!(child.thread_id, child_thread_id);
    assert_eq!(
        child.parent_thread_id.as_deref(),
        Some(result.result.session_id.as_str())
    );
    let grandchild = harness
        .state
        .store()
        .gateway_runtime_binding_by_native_session("codex", "codex-native-grandchild")
        .expect("grandchild binding")
        .expect("nested runtime child projected");
    assert_eq!(grandchild.parent_thread_id.as_deref(), Some(child.thread_id.as_str()));
    let grandchild_metadata = harness
        .state
        .store()
        .session_metadata(&grandchild.thread_id)
        .expect("grandchild metadata")
        .expect("grandchild metadata row");
    assert_eq!(grandchild_metadata["runtimeStatus"], "idle");

    let mut child_turn = request(
        &harness,
        GatewaySource::new("web", "runtime-child-send").persistent(),
        "must not send",
    );
    child_turn.thread_id = Some(child.thread_id.clone());
    child_turn.options.session = Some(child.thread_id);
    child_turn.options.runtime_ref = Some("codex".to_string());
    let error = harness
        .gateway
        .send_turn(child_turn)
        .await
        .expect_err("runtime-native child is read-only");
    assert!(error.to_string().contains("read-only runtime-native child"));
}

#[tokio::test]
async fn direct_runtime_questions_project_the_typed_native_list_without_reconstruction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::OpenCode,
        Arc::new(InteractiveRuntime::question()),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway: gateway.clone(),
    };
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "direct-runtime-questions").persistent(),
        "request questions",
    );
    turn.options.runtime_ref = Some("opencode".to_string());
    turn.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));
    let run = tokio::spawn(async move { gateway.send_turn(turn).await });

    let action = loop {
        let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("attention timeout")
            .expect("attention event channel");
        if let GatewayEvent::ActionRequested { action } = event {
            break action;
        }
    };
    assert_eq!(action.kind, GatewayActionKind::Clarify);
    assert_eq!(
        action.payload["raw"]["questions"],
        json!([
            {
                "header": "Target",
                "question": "Which target?",
                "options": [
                    {"label": "Core", "description": "Inspect core"},
                    {"label": "TUI", "description": "Inspect TUI"}
                ],
                "multiple": false,
                "custom": true,
                "secret": false
            },
            {
                "header": "Checks",
                "question": "Which checks?",
                "options": [
                    {"label": "Tests", "description": "Run tests"},
                    {"label": "Clippy", "description": "Run lint"}
                ],
                "multiple": true,
                "custom": false,
                "secret": false
            }
        ])
    );
    let payload = action.payload.to_string();
    assert!(!payload.contains("flattened-wrong-choice"));
    assert!(!payload.contains("must not be used"));
    let thread_id = action.thread_id.clone().expect("question public thread");
    assert!(harness.gateway.submit_clarify(
        GatewayThreadSelector::thread_id(thread_id),
        &action.action_id,
        ClarifyResult::Answered(ClarifyResponse {
            answers: vec![
                ClarifyAnswer {
                    answers: vec!["TUI".to_string()],
                },
                ClarifyAnswer {
                    answers: vec!["Tests".to_string(), "Clippy".to_string()],
                },
            ],
        }),
    ));
    let result = run.await.expect("turn join").expect("question turn");
    assert_eq!(result.result.final_answer, "questions answered");
    assert!(native_backend.runs().is_empty());
}

#[tokio::test]
async fn direct_runtime_gui_advanced_interaction_fails_closed_without_public_question() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let native_backend = Arc::new(FakeBackend::default());
    let host = RuntimeHost::new();
    host.register(
        RuntimeKind::Codex,
        Arc::new(InteractiveRuntime::gui_advanced_question()),
    );
    let gateway =
        Gateway::with_backend_and_runtime_host(state.clone(), native_backend.clone(), host);
    let harness = Harness {
        _temp: temp,
        cwd,
        state,
        gateway,
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&events);
    let mut turn = request(
        &harness,
        GatewaySource::new("web", "direct-runtime-advanced-question").persistent(),
        "trigger a native experimental question",
    );
    turn.options.runtime_ref = Some("codex".to_string());
    turn.event_sink = Some(Arc::new(move |event| {
        event_log.lock().expect("event log poisoned").push(event);
    }));

    let result = tokio::time::timeout(Duration::from_secs(2), harness.gateway.send_turn(turn))
        .await
        .expect("experimental interaction must not deadlock")
        .expect("experimental question is declined safely");
    assert_eq!(result.result.final_answer, "experimental question declined");
    assert!(native_backend.runs().is_empty());

    let events = events.lock().expect("event log poisoned");
    assert!(events.iter().any(|event| matches!(
        event,
        GatewayEvent::ActionCancelled {
            kind: GatewayActionKind::Clarify,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        GatewayEvent::Warning { kind, .. }
            if kind == "runtime_interaction_exposure_blocked"
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, GatewayEvent::ActionRequested { .. })),
        "blocked interaction must never enter Shared Attention"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, GatewayEvent::TurnCompleted { .. }))
            .count(),
        1,
        "the declined interaction still reaches one public terminal"
    );
    let public_events = serde_json::to_string(&*events).expect("public events JSON");
    assert!(!public_events.contains("native-experimental-question-secret"));
    assert!(!public_events.contains("/answer"));
    drop(events);

    let terminals = harness
        .state
        .store()
        .list_gateway_turn_terminals_for_thread(&result.result.session_id)
        .expect("turn terminals");
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].status, "completed");
}
