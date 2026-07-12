use super::*;
use crate::im::FakeImAdapter;
use futures::future::BoxFuture;
use psychevo_runtime::{Outcome, RunResult, StateRuntime};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct TestBackend {
    prompts: Arc<Mutex<Vec<String>>>,
    runs: AtomicUsize,
    request_permission: AtomicBool,
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

#[derive(Debug, Clone, Default)]
struct FailOnceImAdapter {
    attempts: Arc<AtomicUsize>,
    sent: Arc<Mutex<Vec<ImOutboundMessage>>>,
}

impl crate::im::ImAdapter for FailOnceImAdapter {
    fn platform(&self) -> &str {
        "wechat"
    }

    fn poll(&self) -> BoxFuture<'static, psychevo_runtime::Result<Vec<ImInboundMessage>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, psychevo_runtime::Result<()>> {
        let attempts = Arc::clone(&self.attempts);
        let sent = Arc::clone(&self.sent);
        Box::pin(async move {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::Message(
                    "deterministic first send failure".to_string(),
                ));
            }
            sent.lock().expect("sent messages poisoned").push(message);
            Ok(())
        })
    }
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
        BackendKind::Native
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
        model: Some("fake-model".to_string()),
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
async fn channel_outbox_retry_sends_saved_final_without_rerunning_the_turn() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let backend = Arc::new(TestBackend::default());
    let runs = Arc::clone(&backend);
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
    let adapter = FailOnceImAdapter::default();
    let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
        "wechat",
        Arc::new(adapter.clone()),
        ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
    )]);
    let runtime = ChannelRuntimeState::new(temp.path());
    let connection = ready_wechat_connection(None);
    let message = wechat_message("original prompt must not rerun", "wx-outbox");
    let source = gateway_source_for_im(&message);
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&cwd, "channel", "pending", "pending", None)
        .expect("thread");
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source.source_key().0,
            source_kind: &source.kind,
            raw_identity: source.raw_identity.clone().expect("raw identity"),
            visible_name: source.visible_name.as_deref(),
            thread_id: Some(&thread_id),
            draft_agent_ref: None,
            draft_profile_ref: None,
            draft_control_values: &BTreeMap::new(),
            lineage: None,
        })
        .expect("source lane");
    let payload = "saved final answer";
    let payload_hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
    state
        .inner
        .state
        .store()
        .upsert_gateway_channel_outbox(psychevo_runtime::GatewayChannelOutboxInput {
            delivery_id: "out-retry",
            thread_id: &thread_id,
            turn_id: "turn-already-completed",
            connection_id: "wechat",
            source_key: &source.source_key().0,
            payload_text: payload,
            payload_hash: &payload_hash,
        })
        .expect("outbox");
    runner::retry_unacknowledged_channel_outbox(&state, &runtime, &connection, &channel_gateway)
        .await
        .expect_err("first outbox delivery fails");
    let failed = state
        .inner
        .state
        .store()
        .gateway_channel_outbox("out-retry")
        .expect("failed outbox read")
        .expect("failed outbox row");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.payload_text.as_deref(), Some(payload));
    assert_eq!(runs.runs.load(Ordering::SeqCst), 0);

    let delivered = runner::retry_unacknowledged_channel_outbox(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
    )
    .await
    .expect("outbox retry");

    assert_eq!(delivered, 1);
    assert_eq!(runs.runs.load(Ordering::SeqCst), 0);
    let sent = adapter.sent.lock().expect("sent messages poisoned");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].thread_id, thread_id);
    assert_eq!(sent[0].text, payload);
    let acknowledged = state
        .inner
        .state
        .store()
        .gateway_channel_outbox("out-retry")
        .expect("outbox read")
        .expect("outbox row");
    assert_eq!(acknowledged.status, "acknowledged");
    assert_eq!(acknowledged.payload_text, None);
    assert_eq!(acknowledged.payload_hash, payload_hash);
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
async fn channel_profile_command_starts_new_unbound_target_draft() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        "[agents.backends.opencode]\nkind = \"acp\"\ncommand = \"opencode\"\nentrypoints = [\"peer\", \"subagent\"]\n",
    )
    .expect("config");
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
        .expect("runtime binding");
    assert!(
        runtime_binding.is_none(),
        "selection must bind only at turn delivery"
    );
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source_key)
        .expect("source lane")
        .expect("source lane exists");
    assert_eq!(lane.draft_profile_ref.as_deref(), Some("opencode"));
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
async fn channel_profile_command_rejects_unknown_pre_thread_target() {
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
    assert_eq!(sent[0].text, "Unknown Runtime Profile `codex`.");
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source.source_key().0)
        .expect("lane");
    assert!(
        lane.is_none(),
        "unavailable targets must not mutate the source draft"
    );
}

#[tokio::test]
async fn channel_agent_command_rotates_to_an_unbound_top_level_target() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(cwd.join(".psychevo/agents")).expect("agents");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "").expect("config");
    std::fs::write(
        cwd.join(".psychevo/agents/reviewer.md"),
        "---\ndescription: Review the top-level task.\n---\n\nReview carefully.\n",
    )
    .expect("agent");
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
    let connection = ready_wechat_connection(None);

    let first = wechat_message("hello", "wx-agent-1");
    let source = gateway_source_for_im(&first);
    handle_channel_message(&state, &runtime, &connection, &channel_gateway, first)
        .await
        .expect("first turn");
    let sent = wait_for_sent(&adapter, 1).await;
    assert_eq!(sent[0].text, "answer 1");
    let previous_thread_id = state
        .inner
        .gateway
        .resolve_source_thread(&source)
        .expect("source")
        .expect("bound thread");

    handle_channel_message(
        &state,
        &runtime,
        &connection,
        &channel_gateway,
        wechat_message("/agent reviewer", "wx-agent-2"),
    )
    .await
    .expect("agent select");
    let sent = wait_for_sent(&adapter, 2).await;
    assert!(sent[1].text.contains("top-level Agent `reviewer`"));
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source.source_key().0)
        .expect("lane")
        .expect("lane exists");
    assert_ne!(lane.thread_id.as_deref(), Some(previous_thread_id.as_str()));
    assert_eq!(lane.draft_agent_ref.as_deref(), Some("reviewer"));
    assert_eq!(lane.draft_profile_ref.as_deref(), Some("native"));
    assert!(
        state
            .inner
            .state
            .store()
            .gateway_runtime_binding(lane.thread_id.as_deref().expect("draft thread"))
            .expect("runtime binding")
            .is_none(),
        "Agent/Profile selection must remain an unbound target until the next turn"
    );
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
