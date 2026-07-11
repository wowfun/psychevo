use super::*;
use crate::im::FakeImAdapter;
use futures::future::BoxFuture;
use psychevo_runtime::{Outcome, RunResult, StateRuntime};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct TestBackend {
    prompts: Arc<Mutex<Vec<String>>>,
    runs: AtomicUsize,
    request_permission: AtomicBool,
}

#[derive(Debug, Clone, Default)]
struct ChannelQuestionRuntime {
    pending: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Value>>>>,
    responses: Arc<Mutex<Vec<Value>>>,
    interactions: Arc<AtomicUsize>,
}

impl psychevo_runtime_host::RuntimeModule for ChannelQuestionRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::UserAction,
                "snapshot is outside the Channel interaction test",
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
        let responses = Arc::clone(&self.responses);
        let interactions = Arc::clone(&self.interactions);
        Box::pin(async move {
            match request.intent {
                psychevo_runtime_host::RuntimeIntent::Turn(turn) => {
                    let interaction_number = interactions.fetch_add(1, Ordering::SeqCst) + 1;
                    let gui_advanced_only = turn.prompt.contains("experimental");
                    assert_eq!(
                        turn.interaction_exposure,
                        psychevo_runtime_host::RuntimeInteractionExposure::Standard
                    );
                    let native_session_id = format!("question-native:{}", turn.thread_id);
                    observer
                        .bind_native_session(psychevo_runtime_host::RuntimeSessionBinding {
                            runtime_ref: request.profile.id.clone(),
                            thread_id: turn.thread_id.clone(),
                            native_session_id: native_session_id.clone(),
                            cwd: turn.cwd.clone(),
                            binding_epoch: turn.binding_epoch,
                            process_epoch: 1,
                            instance_epoch: Some(1),
                        })
                        .await?;
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    *pending.lock().expect("question pending poisoned") = Some(sender);
                    let mut questions = vec![psychevo_runtime_host::RuntimeInteractionQuestion {
                        header: Some("Workspace".to_string()),
                        question: "Which workspace should I use?".to_string(),
                        options: vec![
                            psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                label: "Repository root".to_string(),
                                description: "Use the current repository".to_string(),
                            },
                            psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                label: "Another directory".to_string(),
                                description: "Choose a different workspace".to_string(),
                            },
                        ],
                        multiple: false,
                        custom: true,
                        secret: false,
                    }];
                    if turn.prompt.contains("multiple") {
                        questions.push(psychevo_runtime_host::RuntimeInteractionQuestion {
                            header: Some("Checks".to_string()),
                            question: "Which checks should I run?".to_string(),
                            options: vec![
                                psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                    label: "Tests".to_string(),
                                    description: "Run focused tests".to_string(),
                                },
                                psychevo_runtime_host::RuntimeInteractionQuestionOption {
                                    label: "Clippy".to_string(),
                                    description: "Run lint checks".to_string(),
                                },
                            ],
                            multiple: true,
                            custom: false,
                            secret: false,
                        });
                    }
                    if gui_advanced_only {
                        questions[0].question =
                            "channel-native-experimental-question-secret".to_string();
                    }
                    observer.emit(psychevo_runtime_host::RuntimeObservation::Interaction(
                        Box::new(psychevo_runtime_host::RuntimeInteraction {
                            id: format!("native-question-{interaction_number}"),
                            policy: psychevo_runtime_host::RuntimeInteractionPolicy {
                                kind: psychevo_runtime_host::RuntimeInteractionKind::Question,
                                stability: if gui_advanced_only {
                                    psychevo_runtime_host::RuntimeStability::Experimental
                                } else {
                                    psychevo_runtime_host::RuntimeStability::Stable
                                },
                                exposure: if gui_advanced_only {
                                    psychevo_runtime_host::RuntimeInteractionExposure::GuiAdvancedOnly
                                } else {
                                    psychevo_runtime_host::RuntimeInteractionExposure::Standard
                                },
                            },
                            kind: "question".to_string(),
                            runtime_ref: request.profile.id,
                            thread_id: turn.thread_id.clone(),
                            native_session_id: native_session_id.clone(),
                            parent_native_session_id: None,
                            child_native_session_id: None,
                            process_epoch: 1,
                            instance_epoch: Some(1),
                            prompt: if gui_advanced_only {
                                "channel-native-experimental-question-secret".to_string()
                            } else {
                                "Which workspace should I use?".to_string()
                            },
                            questions,
                            choices: Vec::new(),
                            authorization_lifetime: None,
                            expires_at_ms: None,
                            metadata: None,
                        }),
                    ));
                    let response = receiver.await.map_err(|_| {
                        psychevo_runtime_host::RuntimeError::new(
                            "interaction_cancelled",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::Never,
                            "question response channel closed",
                        )
                    })?;
                    let cancelled = response
                        .get("reject")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    responses
                        .lock()
                        .expect("question responses poisoned")
                        .push(response);
                    Ok(psychevo_runtime_host::ExecuteResult::Turn(
                        psychevo_runtime_host::RuntimeTurnResult {
                            turn_id: turn.turn_id,
                            thread_id: turn.thread_id,
                            native_session_id,
                            outcome: psychevo_runtime_host::RuntimeTurnOutcome::Completed,
                            final_answer: if gui_advanced_only && cancelled {
                                "experimental question declined".to_string()
                            } else if cancelled {
                                "question cancelled".to_string()
                            } else {
                                "question answered".to_string()
                            },
                            provider: "opencode".to_string(),
                            model: "fake-opencode".to_string(),
                            history_fidelity: psychevo_runtime_host::HistoryFidelity::Partial,
                            process_epoch: 1,
                            instance_epoch: Some(1),
                            terminal_error: None,
                            metadata: None,
                        },
                    ))
                }
                psychevo_runtime_host::RuntimeIntent::Interaction(response) => {
                    let sender = pending
                        .lock()
                        .expect("question pending poisoned")
                        .take()
                        .ok_or_else(|| {
                            psychevo_runtime_host::RuntimeError::new(
                                "interaction_expired",
                                psychevo_runtime_host::RuntimeErrorStage::Interaction,
                                psychevo_runtime_host::RetryClass::UserAction,
                                "question is no longer pending",
                            )
                        })?;
                    sender.send(response.response).map_err(|_| {
                        psychevo_runtime_host::RuntimeError::new(
                            "interaction_expired",
                            psychevo_runtime_host::RuntimeErrorStage::Interaction,
                            psychevo_runtime_host::RetryClass::UserAction,
                            "question receiver closed",
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
                    "unsupported Channel question intent",
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

impl TestBackend {
    fn request_permission(&self) {
        self.request_permission.store(true, Ordering::SeqCst);
    }

    fn stop_requesting_permission(&self) {
        self.request_permission.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct ErrorImAdapter {
    polls: Arc<AtomicUsize>,
}

impl crate::im::ImAdapter for ErrorImAdapter {
    fn platform(&self) -> &str {
        "wechat"
    }

    fn poll(&self) -> BoxFuture<'static, psychevo_runtime::Result<Vec<ImInboundMessage>>> {
        let polls = Arc::clone(&self.polls);
        Box::pin(async move {
            polls.fetch_add(1, Ordering::SeqCst);
            Err(Error::Message(
                "WeChat iLink getupdates failed: needs_qr_login errcode=-14: session timeout"
                    .to_string(),
            ))
        })
    }

    fn send(
        &self,
        _message: ImOutboundMessage,
    ) -> BoxFuture<'static, psychevo_runtime::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

impl crate::GatewayBackend for TestBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Psychevo
    }

    fn run_turn(
        &self,
        request: crate::BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
        let prompts = Arc::clone(&self.prompts);
        let run_number = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        let request_permission = self.request_permission.load(Ordering::SeqCst);
        Box::pin(async move {
            if request_permission {
                let Some(handler) = request.options.approval_handler.clone() else {
                    return Err(Error::Message("approval handler missing".to_string()));
                };
                let decision = handler
                    .request_permission(psychevo_runtime::PermissionApprovalRequest {
                        tool_call_id: "permission-1".to_string(),
                        tool_name: "fake_tool".to_string(),
                        summary: "fake permission".to_string(),
                        reason: "test permission".to_string(),
                        matched_rule: None,
                        suggested_rule: None,
                        allow_always: false,
                        timeout_secs: 300,
                    })
                    .await;
                if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
                    return Err(Error::Message("permission denied".to_string()));
                }
            }
            prompts
                .lock()
                .expect("prompts poisoned")
                .push(request.options.prompt.clone());
            let session_id = if let Some(session_id) = request.options.session.clone() {
                session_id
            } else {
                request.options.state.store().create_session_with_metadata(
                    &request.options.cwd,
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
                cwd: request.options.cwd,
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
                terminal_error: None,
                events: Vec::new(),
                warnings: Vec::new(),
            })
        })
    }
}

fn ready_wechat_connection(cwd: Option<String>) -> ChannelRuntimeConnection {
    ChannelRuntimeConnection {
        id: "wechat".to_string(),
        channel: "wechat".to_string(),
        domain: Some("wechat".to_string()),
        enabled: true,
        label: "WeChat".to_string(),
        transport: "polling".to_string(),
        cwd,
        runtime_ref: None,
        model: None,
        permission_mode: None,
        require_mention: true,
        credential: None,
        app_id: None,
        app_secret: None,
        account_id: None,
        base_url: None,
        allow_users: vec!["wx-user".to_string()],
        allow_groups: Vec::new(),
        config_status: "ready".to_string(),
    }
}

fn wechat_message(text: &str, message_id: &str) -> ImInboundMessage {
    ImInboundMessage {
        identity: ImIdentity {
            connection_id: Some("wechat".to_string()),
            platform: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            workspace_id: None,
            chat_type: Some("dm".to_string()),
            chat_id: "wx-user".to_string(),
            thread_id: None,
            user_id: Some("wx-user".to_string()),
            operator_id: None,
            reply_to: None,
        },
        message_id: message_id.to_string(),
        text: text.to_string(),
        attachments: Vec::new(),
        task_key: None,
    }
}

async fn wait_for_sent(adapter: &FakeImAdapter, count: usize) -> Vec<ImOutboundMessage> {
    for _ in 0..100 {
        let sent = adapter.sent();
        if sent.len() >= count {
            return sent;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    adapter.sent()
}

#[tokio::test]
async fn channel_message_runs_gateway_turn_and_sends_final_answer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd.clone(),
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);
    let message = wechat_message("ping", "wx-message");

    handle_channel_message(&state, &runtime, &connection, &channel_gateway, message)
        .await
        .expect("message handled");

    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(
        prompts.lock().expect("prompts poisoned").as_slice(),
        ["ping"]
    );
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, "answer 1");
    assert_eq!(runtime.runner_view("wechat").state, "running");
    assert!(runtime.runner_view("wechat").last_outbound_at_ms.is_some());
}

#[tokio::test]
async fn channel_mission_records_team_metadata_before_running_prompt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(cwd.join(".psychevo/agents")).expect("agents");
    std::fs::create_dir_all(cwd.join(".psychevo/teams")).expect("teams");
    std::fs::write(
        cwd.join(".psychevo/agents/general.md"),
        "---\nname: general\ndescription: General agent\n---\nGeneral agent.\n",
    )
    .expect("agent");
    std::fs::write(
        cwd.join(".psychevo/teams/release.md"),
        concat!(
            "---\n",
            "name: release\n",
            "description: Release team\n",
            "leader: general\n",
            "members:\n",
            "  - id: reviewer\n",
            "    agent: general\n",
            "    role: review\n",
            "maxParallelAgents: 2\n",
            "---\n",
            "Coordinate the release.\n"
        ),
    )
    .expect("team");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd.clone(),
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/mission --team release Ship it", "wx-mission"),
    )
    .await
    .expect("mission handled");

    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent.len(), 1);
    let prompt_log = prompts.lock().expect("prompts poisoned").clone();
    assert_eq!(prompt_log.len(), 1, "sent={sent:?}");
    assert!(
        prompt_log[0].contains("Ship it"),
        "{:?}",
        prompt_log.as_slice()
    );
    let team = state
        .inner
        .state
        .store()
        .find_active_agent_team_run(&sent[0].thread_id)
        .expect("team lookup")
        .expect("team run");
    let mission = state
        .inner
        .state
        .store()
        .find_active_agent_mission_run(&sent[0].thread_id)
        .expect("mission lookup")
        .expect("mission run");
    assert_eq!(team.team_name, "release");
    assert_eq!(team.max_parallel_agents, 2);
    assert_eq!(mission.goal, "Ship it");
}

#[tokio::test]
async fn channel_help_command_replies_without_running_gateway_turn() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let env = BTreeMap::from([
        ("HOME".to_string(), home.to_string_lossy().to_string()),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
        ("PSYCHEVO_CHANNEL_RUNTIME".to_string(), "0".to_string()),
    ]);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/help", "wx-help"),
    )
    .await
    .expect("help handled");

    assert!(prompts.lock().expect("prompts poisoned").is_empty());
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("/status"));
    assert!(sent[0].text.contains("/compact"));
    assert!(sent[0].thread_id.starts_with("im.wechat:"));
}

#[tokio::test]
async fn channel_voice_command_updates_source_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let backend = Arc::new(TestBackend::default());
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);
    let message = wechat_message("/voice on", "wx-voice-on");
    let source = gateway_source_for_im(&message);

    handle_channel_message(&state, &runtime, &connection, &channel_gateway, message)
        .await
        .expect("voice handled");

    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(
        sent[0].text,
        "Voice replies will follow voice inputs. Text fallback remains active."
    );
    assert_eq!(
        voice_policy_for_source(&state, &source),
        wire::VoicePolicyMode::VoiceOnly
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("/voice off", "wx-voice-off"),
    )
    .await
    .expect("voice off handled");
    let sent = wait_for_sent(&adapter, 2).await;
    assert_eq!(sent[1].text, "Voice replies are off.");
    assert_eq!(
        voice_policy_for_source(&state, &source),
        wire::VoicePolicyMode::Off
    );
}

#[tokio::test]
async fn channel_shared_compact_command_runs_native_compaction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let env = BTreeMap::from([
        ("HOME".to_string(), home.to_string_lossy().to_string()),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
        ("PSYCHEVO_CHANNEL_RUNTIME".to_string(), "0".to_string()),
    ]);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("hello", "wx-start"),
    )
    .await
    .expect("first turn handled");
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent[0].text, "answer 1");
    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/compact keep decisions", "wx-compact"),
    )
    .await
    .expect("compact handled");

    let sent = wait_for_sent(&adapter, 2).await;
    let prompts = prompts.lock().expect("prompts poisoned");
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0], "hello");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].text, "not enough messages to compact");
}

#[tokio::test]
async fn channel_compact_queues_during_active_turn_without_blocking_later_control_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    let backend = Arc::new(TestBackend::default());
    backend.request_permission();
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend.clone());
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("first", "wx-first"),
    )
    .await
    .expect("first turn accepted");
    let sent = wait_for_sent(&adapter, 1).await;
    assert!(sent[0].text.contains("Permission required for fake_tool"));
    let permission_token = sent[0]
        .text
        .split("/approve ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("permission token")
        .to_string();
    backend.stop_requesting_permission();

    tokio::time::timeout(
        Duration::from_millis(200),
        handle_channel_message(
            &state,
            &runtime,
            &connection,
            &channel_gateway,
            wechat_message("/compact keep decisions", "wx-compact-active"),
        ),
    )
    .await
    .expect("active-turn /compact must enqueue without waiting")
    .expect("compact command accepted");
    assert_eq!(
        adapter.sent().len(),
        1,
        "queued compaction must not send unavailable or completion before the active turn finishes"
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("later", "wx-later"),
    )
    .await
    .expect("later prompt accepted");
    assert!(prompts.lock().expect("prompts poisoned").is_empty());

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            &format!("/approve {permission_token}"),
            "wx-approve-compact",
        ),
    )
    .await
    .expect("approval command remains responsive");

    let sent = wait_for_sent(&adapter, 5).await;
    let compact_reply = sent
        .iter()
        .position(|message| message.text == "not enough messages to compact")
        .expect("compaction reply");
    let later_reply = sent
        .iter()
        .position(|message| message.text == "answer 2")
        .expect("later turn reply");
    assert!(compact_reply < later_reply, "{sent:#?}");
    assert_eq!(
        prompts.lock().expect("prompts poisoned").as_slice(),
        ["first", "later"]
    );
}

#[tokio::test]
async fn channel_dynamic_skill_command_runs_gateway_turn() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    let skill_dir = cwd.join(".psychevo/skills/reviewer");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: reviewer\ndescription: Review the current change.\n---\n\nReview carefully.\n",
    )
    .expect("skill");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/reviewer focus security", "wx-reviewer"),
    )
    .await
    .expect("dynamic skill handled");

    let sent = wait_for_sent(&adapter, 1).await;
    let prompts = prompts.lock().expect("prompts poisoned");
    assert_eq!(prompts.as_slice(), ["$reviewer focus security"]);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, "answer 1");
}

#[tokio::test]
async fn channel_agents_command_lists_callable_subagents_before_peer_runtimes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    let agent_dir = cwd.join(".psychevo/agents");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        agent_dir.join("reviewer.md"),
        "---\ndescription: Review code changes.\n---\n\nReview carefully.\n",
    )
    .expect("agent");
    std::fs::write(
        home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP backend."
command = "opencode"
entrypoints = ["peer", "subagent"]

[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP backend."
command = "cursor-agent"
entrypoints = ["peer"]
"#,
    )
    .expect("config");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/agents", "wx-agents"),
    )
    .await
    .expect("agents handled");

    assert!(prompts.lock().expect("prompts poisoned").is_empty());
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent.len(), 1);
    let text = &sent[0].text;
    let callable = text.find("Callable agents:").expect("callable group");
    let reviewer = text
        .find("@reviewer - Review code changes.")
        .expect("reviewer");
    let opencode = text
        .find("@opencode - OpenCode ACP backend.")
        .expect("opencode");
    let peer = text.find("Peer runtimes:").expect("peer group");
    let cursor = text.find("@cursor - Cursor ACP backend.").expect("cursor");
    assert!(
        callable < reviewer && reviewer < peer,
        "reviewer should be callable before peer runtimes:\n{text}"
    );
    assert!(
        callable < opencode && opencode < peer,
        "peer+subagent backend should stay callable:\n{text}"
    );
    assert!(
        peer < cursor,
        "peer-only backend should be grouped after callable agents:\n{text}"
    );
    assert!(text.contains("Use @agent-name followed by a task."));
    assert!(text.contains("Peer runtimes are listed for visibility"));
}

#[tokio::test]
async fn channel_profile_command_starts_new_immutably_bound_thread() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "").expect("config");
    let backend = Arc::new(TestBackend::default());
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            state_runtime
                .db_path()
                .parent()
                .unwrap()
                .display()
                .to_string(),
        ),
    ]);
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend(state_runtime, backend),
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);

    let first_message = wechat_message("hello", "wx-profile-1");
    let source_key = gateway_source_for_im(&first_message).source_key().0;
    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        first_message,
    )
    .await
    .expect("message handled");
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent[0].text, "answer 1");
    let original_thread_id = state
        .inner
        .state
        .store()
        .gateway_source_binding(&source_key)
        .expect("original binding")
        .expect("original binding exists")
        .thread_id;

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("/profile use opencode", "wx-profile-2"),
    )
    .await
    .expect("profile use handled");
    let sent = wait_for_sent(&adapter, 2).await;
    assert!(sent[1].text.contains("Runtime Profile `opencode`"));
    assert!(sent[1].text.contains("previous thread is unchanged"));

    let binding = state
        .inner
        .state
        .store()
        .gateway_source_binding(&source_key)
        .expect("binding")
        .expect("binding exists");
    assert_ne!(binding.thread_id, original_thread_id);
    let runtime_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&binding.thread_id)
        .expect("runtime binding")
        .expect("runtime binding exists");
    assert_eq!(runtime_binding.runtime_ref.as_deref(), Some("opencode"));
    assert!(
        state
            .inner
            .state
            .store()
            .session_summary(&original_thread_id)
            .expect("original thread")
            .is_some()
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("/profile status", "wx-profile-3"),
    )
    .await
    .expect("profile status handled");
    let sent = wait_for_sent(&adapter, 3).await;
    assert!(sent[2].text.contains("Runtime Profile `opencode`"));
}

#[tokio::test]
async fn channel_profile_command_saves_pre_thread_lane_preference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "").expect("config");
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
        home,
        cwd,
        None,
        BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )]),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let message = wechat_message("/profile use codex", "wx-profile-draft");
    let source = gateway_source_for_im(&message);

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        message,
    )
    .await
    .expect("profile draft handled");

    let sent = wait_for_sent(&adapter, 1).await;
    assert!(sent[0].text.contains("saved for the next channel thread"));
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source.source_key().0)
        .expect("lane")
        .expect("lane exists");
    assert_eq!(lane.thread_id, None);
    assert_eq!(lane.draft_runtime_ref.as_deref(), Some("codex"));
}

#[tokio::test]
async fn channel_new_command_clears_binding_for_next_default_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let changed_cwd = temp.path().join("changed");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&changed_cwd).expect("changed cwd");
    let backend = Arc::new(TestBackend::default());
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let store_state = state_runtime.clone();
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd.clone(),
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("first", "wx-first"),
    )
    .await
    .expect("first handled");
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent.len(), 1);
    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(Some(changed_cwd.to_string_lossy().to_string())),
        &channel_gateway,
        wechat_message("/new", "wx-new"),
    )
    .await
    .expect("new handled");
    let sent = wait_for_sent(&adapter, 2).await;
    assert_eq!(sent.len(), 2);
    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(Some(changed_cwd.to_string_lossy().to_string())),
        &channel_gateway,
        wechat_message("second", "wx-second"),
    )
    .await
    .expect("second handled");

    let sent = wait_for_sent(&adapter, 3).await;
    assert_eq!(sent.len(), 3);
    assert_ne!(sent[0].thread_id, sent[2].thread_id);
    let active_sessions = store_state
        .store()
        .list_sessions_with_sources(&["channel/wechat"])
        .expect("sessions");
    let archived_sessions = store_state
        .store()
        .list_archived_sessions_with_sources(&["channel/wechat"])
        .expect("archived sessions");
    let cwds = active_sessions
        .iter()
        .chain(archived_sessions.iter())
        .map(|session| session.cwd.as_str())
        .collect::<BTreeSet<_>>();
    assert!(cwds.contains(cwd.to_string_lossy().as_ref()));
    assert!(cwds.contains(changed_cwd.to_string_lossy().as_ref()));
}

#[tokio::test]
async fn channel_permission_request_can_be_approved_by_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let backend = Arc::new(TestBackend::default());
    backend.request_permission();
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("needs approval", "wx-approval-turn"),
    )
    .await
    .expect("approval turn accepted");

    let sent = wait_for_sent(&adapter, 1).await;
    assert!(sent[0].text.contains("Permission required for fake_tool"));
    assert!(!sent[0].text.contains("permission-1"));
    let token = sent[0]
        .text
        .split("/approve ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("opaque permission token")
        .to_string();
    assert!(token.starts_with("ia_"));

    let mut wrong_lane = wechat_message(&format!("/approve {token}"), "wx-wrong-lane");
    wrong_lane.identity.chat_id = "wx-other".to_string();
    wrong_lane.identity.user_id = Some("wx-other".to_string());
    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wrong_lane,
    )
    .await
    .expect("cross-lane token rejected");
    let sent = wait_for_sent(&adapter, 2).await;
    assert_eq!(sent[1].text, "No matching permission request token.");

    let mut wrong_connection = ready_wechat_connection(None);
    wrong_connection.id = "other-connection".to_string();
    handle_channel_message(
        &state,
        &runtime,
        &wrong_connection,
        &channel_gateway,
        wechat_message(&format!("/approve {token}"), "wx-wrong-connection"),
    )
    .await
    .expect("cross-connection token rejected");
    let sent = wait_for_sent(&adapter, 3).await;
    assert_eq!(sent[2].text, "No matching permission request token.");

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/approve permission-1", "wx-raw-approve"),
    )
    .await
    .expect("raw action id rejected");
    let sent = wait_for_sent(&adapter, 4).await;
    assert_eq!(sent[3].text, "No matching permission request token.");

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message(&format!("/approve {token}"), "wx-approve"),
    )
    .await
    .expect("approval command handled");

    let sent = wait_for_sent(&adapter, 6).await;
    assert!(
        sent.iter()
            .any(|message| message.text == format!("Approved request {token}.")),
        "{sent:#?}"
    );
    assert!(sent.iter().any(|message| message.text == "answer 1"));

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message(&format!("/approve {token}"), "wx-replay-approve"),
    )
    .await
    .expect("consumed token rejected");
    let sent = wait_for_sent(&adapter, 7).await;
    assert_eq!(sent[6].text, "No matching permission request token.");

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("needs denial", "wx-denial-turn"),
    )
    .await
    .expect("denial turn accepted");
    let sent = wait_for_sent(&adapter, 8).await;
    let denial_prompt = sent
        .iter()
        .skip(7)
        .find(|message| message.text.contains("Permission required"))
        .expect("denial prompt");
    let deny_token = denial_prompt
        .text
        .split("/deny ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|token| token.trim_end_matches('.'))
        .expect("deny token")
        .to_string();
    assert_ne!(token, deny_token);
    assert!(!denial_prompt.text.contains("permission-1"));

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message(&format!("/deny {deny_token}"), "wx-deny"),
    )
    .await
    .expect("permission denied");
    let sent = wait_for_sent(&adapter, 9).await;
    assert_eq!(sent[8].text, format!("Denied request {deny_token}."));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !adapter
            .sent()
            .iter()
            .any(|message| message.text == "answer 2")
    );
}

#[tokio::test]
async fn channel_answer_command_reports_missing_ask_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let backend = Arc::new(TestBackend::default());
    let prompts = Arc::clone(&backend.prompts);
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(state_runtime, backend);
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/answer ask-1 use repo root", "wx-answer"),
    )
    .await
    .expect("answer command handled");

    assert!(prompts.lock().expect("prompts poisoned").is_empty());
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent[0].text, "No matching Ask request token.");
}

#[tokio::test]
async fn channel_question_tokens_answer_and_cancel_without_exposing_gateway_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "").expect("config");
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let question_runtime = Arc::new(ChannelQuestionRuntime::default());
    let responses = Arc::clone(&question_runtime.responses);
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(
        psychevo_runtime_host::RuntimeKind::OpenCode,
        question_runtime.clone(),
    );
    host.register(psychevo_runtime_host::RuntimeKind::Codex, question_runtime);
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            state_runtime
                .db_path()
                .parent()
                .expect("state parent")
                .display()
                .to_string(),
        ),
    ]);
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend_and_runtime_host(
            state_runtime,
            Arc::new(TestBackend::default()),
            host,
        ),
        home,
        cwd,
        None,
        env,
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let mut connection = ready_wechat_connection(None);
    connection.runtime_ref = Some("opencode".to_string());

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("ask me", "wx-question-answer"),
    )
    .await
    .expect("question turn accepted");
    let sent = wait_for_sent(&adapter, 1).await;
    assert!(sent[0].text.contains("Which workspace should I use?"));
    assert!(!sent[0].text.contains("native-question"));
    assert!(!sent[0].text.contains("rt_"));
    let answer_token = sent[0]
        .text
        .split("/answer ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("answer token")
        .to_string();

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            &format!("/answer {answer_token} use the repo root"),
            "wx-question-answer-response",
        ),
    )
    .await
    .expect("question answered");
    let sent = wait_for_sent(&adapter, 3).await;
    assert!(
        sent.iter()
            .any(|message| message.text == format!("Answered request {answer_token}.")),
        "{sent:#?}"
    );
    assert!(
        sent.iter()
            .any(|message| message.text == "question answered")
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("ask again", "wx-question-cancel"),
    )
    .await
    .expect("second question turn accepted");
    let sent = wait_for_sent(&adapter, 4).await;
    let second_prompt = sent
        .iter()
        .skip(3)
        .find(|message| message.text.contains("Psychevo asks:"))
        .expect("second question prompt");
    let cancel_token = second_prompt
        .text
        .split("/cancel ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|token| token.trim_end_matches('.'))
        .expect("cancel token")
        .to_string();
    assert_ne!(answer_token, cancel_token);

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            &format!("/cancel {cancel_token}"),
            "wx-question-cancel-response",
        ),
    )
    .await
    .expect("question cancelled");
    let sent = wait_for_sent(&adapter, 6).await;
    assert!(
        sent.iter()
            .any(|message| message.text == format!("Cancelled request {cancel_token}."))
    );
    assert!(
        sent.iter()
            .any(|message| message.text == "question cancelled")
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("ask multiple", "wx-question-multiple"),
    )
    .await
    .expect("multiple-question turn accepted");
    let sent = wait_for_sent(&adapter, 7).await;
    let multi_prompt = sent.last().expect("multiple-question guidance");
    let multi_token = multi_prompt
        .text
        .split("/cancel ")
        .nth(1)
        .map(|token| token.trim_end_matches('.'))
        .expect("multiple-question cancel token")
        .to_string();
    assert_eq!(
        multi_prompt.text,
        channel_multi_question_guidance(&multi_token)
    );
    assert!(!multi_prompt.text.contains("/answer"));

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            &format!("/answer {multi_token} Tests"),
            "wx-question-multiple-partial",
        ),
    )
    .await
    .expect("partial multi-question answer rejected");
    let sent = wait_for_sent(&adapter, 8).await;
    assert_eq!(
        sent.last().expect("partial-answer guidance").text,
        channel_multi_question_guidance(&multi_token)
    );
    assert_eq!(
        responses.lock().expect("question responses poisoned").len(),
        2,
        "Channel must not answer only the first native question"
    );

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            &format!("/cancel {multi_token}"),
            "wx-question-multiple-cancel",
        ),
    )
    .await
    .expect("multiple-question request cancelled");
    let sent = wait_for_sent(&adapter, 10).await;
    assert!(
        sent.iter()
            .any(|message| message.text == format!("Cancelled request {multi_token}."))
    );

    {
        let responses = responses.lock().expect("question responses poisoned");
        assert_eq!(responses[0]["answers"][0][0], "use the repo root");
        assert_eq!(responses[1]["reject"], true);
        assert_eq!(responses[1]["decision"], "cancel");
        assert_eq!(responses[2]["reject"], true);
    }

    connection.runtime_ref = Some("codex".to_string());
    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(
            "ask experimental question",
            "wx-question-experimental-blocked",
        ),
    )
    .await
    .expect("experimental question is declined without blocking the turn");
    let sent = wait_for_sent(&adapter, 12).await;
    let blocked_turn_messages = &sent[10..];
    assert!(blocked_turn_messages.iter().any(|message| {
        message.text
            == "A runtime interaction requires GUI Advanced mode and was declined on this Channel."
    }));
    assert!(
        blocked_turn_messages
            .iter()
            .any(|message| message.text == "experimental question declined"),
        "declining the native request must preserve final progress: {blocked_turn_messages:#?}"
    );
    let blocked_text = blocked_turn_messages
        .iter()
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!blocked_text.contains("/answer"));
    assert!(!blocked_text.contains("ia_"));
    assert!(!blocked_text.contains("channel-native-experimental-question-secret"));
    let responses = responses.lock().expect("question responses poisoned");
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[3], json!({"reject": true, "decision": "cancel"}));
}

#[tokio::test]
async fn channel_event_sink_sends_clarify_prompt() {
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::default();
    let identity = wechat_message("ignored", "wx-event").identity;
    let sink = channel_event_sink(
        runtime,
        "wechat".to_string(),
        channel_gateway,
        identity,
        SourceKey("im.wechat:fallback".to_string()),
    );

    sink(GatewayEvent::ActionRequested {
        action: PendingActionView {
            action_id: "ask-1".to_string(),
            kind: GatewayActionKind::Clarify,
            title: Some("Clarify".to_string()),
            summary: Some("Which workspace should I use?".to_string()),
            payload: json!({
                "raw": {
                    "questions": [
                        { "question": "Which workspace should I use?" }
                    ]
                }
            }),
            thread_id: Some("thread-1".to_string()),
            turn_id: None,
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        },
    });

    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent[0].thread_id, "thread-1");
    assert!(sent[0].text.contains("Which workspace should I use?"));
    assert!(!sent[0].text.contains("ask-1"));
    let token = sent[0]
        .text
        .split("/answer ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("opaque question token");
    assert!(token.starts_with("ia_"));
    assert!(sent[0].text.contains(&format!("/answer {token} <answer>")));
    assert!(sent[0].text.contains(&format!("/cancel {token}")));

    sink(GatewayEvent::ActionRequested {
        action: PendingActionView {
            action_id: "expired-permission-id".to_string(),
            kind: GatewayActionKind::Permission,
            title: Some("Expired".to_string()),
            summary: Some("Expired request".to_string()),
            payload: json!({
                "toolName": "expired",
                "interactionExpiresAtMs": gateway_now_ms() - 1,
            }),
            thread_id: Some("thread-1".to_string()),
            turn_id: None,
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: Some(gateway_now_ms() - 1),
        },
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        adapter.sent().len(),
        1,
        "expired actions must not mint tokens"
    );
}

#[tokio::test]
async fn channel_profile_sessions_lists_opaque_handles_that_resume() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let adapter = FakeImAdapter::new("wechat");
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("create a resumable session", "wx-session-create"),
    )
    .await
    .expect("session turn accepted");
    let sent = wait_for_sent(&adapter, 1).await;
    let public_thread_id = sent[0].thread_id.clone();

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("/profile sessions", "wx-session-list"),
    )
    .await
    .expect("sessions listed");
    let sent = wait_for_sent(&adapter, 2).await;
    assert!(
        sent[1]
            .text
            .contains("Sessions for Runtime Profile `native`:")
    );
    assert!(!sent[1].text.contains(&public_thread_id));
    let handle = sent[1]
        .text
        .split_whitespace()
        .find(|word| word.starts_with("rs_"))
        .expect("opaque resume handle")
        .trim_end_matches(':')
        .to_string();

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message(&format!("/profile resume {handle}"), "wx-session-resume"),
    )
    .await
    .expect("session resumed from listed handle");
    let sent = wait_for_sent(&adapter, 3).await;
    assert_eq!(
        sent[2].text,
        format!("Resumed `{handle}` on Runtime Profile `native`.")
    );
    assert!(!sent[2].text.contains(&public_thread_id));
}

#[tokio::test]
async fn wechat_session_timeout_blocks_runner_without_retrying() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let polls = Arc::new(AtomicUsize::new(0));
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(ErrorImAdapter {
            polls: Arc::clone(&polls),
        }),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ChannelRuntimeConnection {
        id: "wechat".to_string(),
        channel: "wechat".to_string(),
        domain: Some("wechat".to_string()),
        enabled: true,
        label: "WeChat".to_string(),
        transport: "polling".to_string(),
        cwd: None,
        runtime_ref: None,
        model: None,
        permission_mode: None,
        require_mention: true,
        credential: None,
        app_id: None,
        app_secret: None,
        account_id: None,
        base_url: None,
        allow_users: vec!["wx-user".to_string()],
        allow_groups: Vec::new(),
        config_status: "ready".to_string(),
    };

    run_channel_loop(
        state,
        runtime.clone(),
        connection,
        channel_gateway,
        CancellationToken::new(),
    )
    .await;

    let runner = runtime.runner_view("wechat");
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert_eq!(runner.state, "blocked");
    assert_eq!(runner.reason.as_deref(), Some("needs_qr_login"));
    assert_eq!(runner.last_ilink_errcode, Some(-14));
}

#[tokio::test]
async fn wechat_session_timeout_during_login_grace_reports_pending() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let state = WebState::new(GatewayWebServerConfig::new(
        Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
        home,
        cwd,
        None,
        BTreeMap::new(),
        temp.path().join("static"),
    ));
    let polls = Arc::new(AtomicUsize::new(0));
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(ErrorImAdapter {
            polls: Arc::clone(&polls),
        }),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    runtime.start_wechat_login_grace("wechat");
    let connection = ChannelRuntimeConnection {
        id: "wechat".to_string(),
        channel: "wechat".to_string(),
        domain: Some("wechat".to_string()),
        enabled: true,
        label: "WeChat".to_string(),
        transport: "polling".to_string(),
        cwd: None,
        runtime_ref: None,
        model: None,
        permission_mode: None,
        require_mention: true,
        credential: None,
        app_id: None,
        app_secret: None,
        account_id: None,
        base_url: None,
        allow_users: vec!["wx-user".to_string()],
        allow_groups: Vec::new(),
        config_status: "ready".to_string(),
    };
    let cancel = CancellationToken::new();
    let handle = tokio::spawn(run_channel_loop(
        state,
        runtime.clone(),
        connection,
        channel_gateway,
        cancel.clone(),
    ));

    for _ in 0..100 {
        let runner = runtime.runner_view("wechat");
        if polls.load(Ordering::SeqCst) >= 1 && runner.reason.as_deref() == Some("qr_login_pending")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let runner = runtime.runner_view("wechat");
    assert!(polls.load(Ordering::SeqCst) >= 1);
    assert_eq!(runner.state, "running");
    assert_eq!(runner.reason.as_deref(), Some("qr_login_pending"));
    assert_eq!(runner.last_ilink_errcode, Some(-14));
    assert!(runner.last_error.is_none());
    cancel.cancel();
    handle.abort();
}
