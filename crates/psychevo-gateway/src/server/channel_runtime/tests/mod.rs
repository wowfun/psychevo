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

impl TestBackend {
    fn request_permission(&self) {
        self.request_permission.store(true, Ordering::SeqCst);
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
            let session_id = request.options.state.store().create_session_with_metadata(
                &request.options.cwd,
                &request.runtime_source,
                "fake-model",
                "fake-provider",
                None,
            )?;
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
async fn channel_help_command_replies_without_running_gateway_turn() {
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
async fn channel_shared_compact_command_runs_gateway_turn() {
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
        wechat_message("/compact keep decisions", "wx-compact"),
    )
    .await
    .expect("compact handled");

    let sent = wait_for_sent(&adapter, 1).await;
    let prompts = prompts.lock().expect("prompts poisoned");
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("Compact this session"));
    assert!(prompts[0].contains("keep decisions"));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, "answer 1");
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
    assert!(sent[0].text.contains("/approve permission-1"));

    handle_channel_message(
        &state,
        &runtime,
        &ready_wechat_connection(None),
        &channel_gateway,
        wechat_message("/approve permission-1", "wx-approve"),
    )
    .await
    .expect("approval command handled");

    let sent = wait_for_sent(&adapter, 3).await;
    assert!(
        sent.iter()
            .any(|message| message.text == "Approved request permission-1.")
    );
    assert!(sent.iter().any(|message| message.text == "answer 1"));
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
    assert_eq!(sent[0].text, "No matching Ask request for ask-1.");
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
    assert!(sent[0].text.contains("/answer ask-1 <answer>"));
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
