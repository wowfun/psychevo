use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::{ByteStreams, Error};
use psychevo::application::{MAX_QUEUED_STEER_BYTES, MAX_QUEUED_STEERS, user_text_message};
use psychevo::{
    Application, Client as FrameworkClient, ContextSnapshot, McpServerInput, PermissionMode,
    RunMode, Thread, ThreadSummary, TurnHandle,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::protocol::resolve_path;
use crate::protocol::{AcpUsageAccumulator, env_path_or_default};

#[derive(Debug, Clone)]
pub struct AcpOptions {
    pub home: PathBuf,
    pub db_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub inherited_env: BTreeMap<String, String>,
}

impl AcpOptions {
    pub fn from_env() -> Self {
        let inherited_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::from_env_map(inherited_env, cwd)
    }

    pub fn from_env_map(inherited_env: BTreeMap<String, String>, cwd: PathBuf) -> Self {
        let home = env_path_or_default(&inherited_env, "PSYCHEVO_HOME", "~/.psychevo", &cwd);
        let db_path = env_path_or_default(
            &inherited_env,
            "PSYCHEVO_DB",
            &home.join("state.db").to_string_lossy(),
            &cwd,
        );
        let config_path = inherited_env
            .get("PSYCHEVO_CONFIG")
            .filter(|value| !value.trim().is_empty())
            .map(|value| resolve_path(value, &inherited_env, &cwd));
        Self {
            home,
            db_path,
            config_path,
            inherited_env,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use agent_client_protocol::schema::ProtocolVersion;
    use agent_client_protocol::schema::v2::{
        AuthCapabilities, CancelSessionNotification, ClientCapabilities, ContentBlock,
        Implementation, InitializeRequest, ListSessionsRequest, NewSessionRequest, OtherReplayFrom,
        PromptRequest, ReplayFrom, ReplayFromStart, ResumeSessionRequest,
        SessionConfigOptionCategory, SessionId, SessionUpdate, SetSessionConfigOptionRequest,
        TerminalAuthCapabilities, TextContent, UpdateSessionNotification,
    };
    use agent_client_protocol::{Agent, Client, ConnectTo, Error, ErrorCode};
    use futures::future::BoxFuture;
    use psychevo::application::{
        AssistantBlock, AssistantSource, MAX_QUEUED_STEER_BYTES, MAX_QUEUED_STEERS, Message,
        Outcome,
    };
    use psychevo::{
        Application, StartThreadRequest, ThreadListQuery, TurnOutcome, TurnRequest, TurnResult,
    };
    use serde_json::Value;
    use sqlx::Connection;
    use tokio::sync::Semaphore;
    use uuid::Uuid;

    use super::{AcpOptions, AcpSession, PsychevoAcpAgent};

    #[derive(Debug)]
    struct SnapshotRootAdapter {
        observed: Arc<Mutex<Option<PathBuf>>>,
    }

    #[derive(Debug)]
    struct PreparedSnapshotRootTurn {
        observed: Arc<Mutex<Option<PathBuf>>>,
    }

    #[derive(Debug, Default)]
    struct ReplayHistoryAdapter {
        calls: AtomicUsize,
    }

    #[derive(Debug)]
    struct PreparedReplayHistoryTurn(Arc<ReplayHistoryAdapter>);

    #[derive(Debug)]
    struct AdmissionGateAdapter {
        prepare_entered: Arc<Semaphore>,
        release_prepare: Arc<Semaphore>,
        invoke_entered: Arc<Semaphore>,
        release_invoke: Arc<Semaphore>,
        interrupted: AtomicBool,
    }

    #[derive(Debug)]
    struct PreparedAdmissionGateTurn(Arc<AdmissionGateAdapter>);

    impl AdmissionGateAdapter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                prepare_entered: Arc::new(Semaphore::new(0)),
                release_prepare: Arc::new(Semaphore::new(0)),
                invoke_entered: Arc::new(Semaphore::new(0)),
                release_invoke: Arc::new(Semaphore::new(0)),
                interrupted: AtomicBool::new(false),
            })
        }
    }

    impl psychevo::AgentSessionAdapter for SnapshotRootAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: psychevo::AgentTurnPreparation,
        ) -> BoxFuture<'static, psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>> {
            Box::pin(async move {
                Ok(Box::new(PreparedSnapshotRootTurn {
                    observed: self.observed.clone(),
                }) as Box<dyn psychevo::PreparedAgentTurn>)
            })
        }
    }

    impl psychevo::PreparedAgentTurn for PreparedSnapshotRootTurn {
        fn invoke(
            self: Box<Self>,
            invocation: psychevo::AgentTurnInvocation,
        ) -> BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
            Box::pin(async move {
                *self.observed.lock().expect("snapshot root observation") =
                    invocation.execution.snapshot_root;
                Err(psychevo::Error::Message(
                    "snapshot root observed".to_string(),
                ))
            })
        }
    }

    impl psychevo::AgentSessionAdapter for ReplayHistoryAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: psychevo::AgentTurnPreparation,
        ) -> BoxFuture<'static, psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>> {
            Box::pin(async move {
                Ok(Box::new(PreparedReplayHistoryTurn(self))
                    as Box<dyn psychevo::PreparedAgentTurn>)
            })
        }
    }

    impl psychevo::PreparedAgentTurn for PreparedReplayHistoryTurn {
        fn invoke(
            self: Box<Self>,
            invocation: psychevo::AgentTurnInvocation,
        ) -> BoxFuture<'static, psychevo::Result<TurnResult>> {
            Box::pin(async move {
                let call = self.0.calls.fetch_add(1, Ordering::SeqCst);
                invocation.persistence.confirm_delivery().await?;
                invocation
                    .persistence
                    .append_message(Message::User {
                        content: vec![psychevo::application::UserContentBlock::text(format!(
                            "user {call}"
                        ))],
                        timestamp_ms: call as i64 * 2 + 1,
                    })
                    .await?;
                let content = if call == 0 {
                    vec![AssistantBlock::Source {
                        source: AssistantSource::Provider {
                            kind: "future_source".to_string(),
                            data: serde_json::json!({ "call": call }),
                        },
                    }]
                } else {
                    vec![AssistantBlock::Text {
                        text: format!("assistant {call}"),
                    }]
                };
                invocation
                    .persistence
                    .append_message(Message::Assistant {
                        content,
                        timestamp_ms: call as i64 * 2 + 2,
                        finish_reason: Some("stop".to_string()),
                        outcome: Outcome::Normal,
                        model: Some("fake-model".to_string()),
                        provider: Some("fake".to_string()),
                    })
                    .await?;
                Ok(TurnResult {
                    thread_id: invocation.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: format!("assistant {call}"),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl psychevo::AgentSessionAdapter for AdmissionGateAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: psychevo::AgentTurnPreparation,
        ) -> BoxFuture<'static, psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>> {
            Box::pin(async move {
                self.prepare_entered.add_permits(1);
                self.release_prepare
                    .acquire()
                    .await
                    .expect("release admission preparation")
                    .forget();
                Ok(Box::new(PreparedAdmissionGateTurn(self))
                    as Box<dyn psychevo::PreparedAgentTurn>)
            })
        }
    }

    impl psychevo::PreparedAgentTurn for PreparedAdmissionGateTurn {
        fn invoke(
            self: Box<Self>,
            invocation: psychevo::AgentTurnInvocation,
        ) -> BoxFuture<'static, psychevo::Result<TurnResult>> {
            Box::pin(async move {
                invocation.persistence.confirm_delivery().await?;
                self.0.invoke_entered.add_permits(1);
                self.0
                    .release_invoke
                    .acquire()
                    .await
                    .expect("release gated invocation")
                    .forget();
                let interrupted = invocation.control.is_interrupted();
                let thread_id = invocation.receipt.thread_id.clone();
                self.0.interrupted.store(interrupted, Ordering::SeqCst);
                drop(invocation);
                Ok(TurnResult {
                    thread_id,
                    outcome: if interrupted {
                        TurnOutcome::Interrupted
                    } else {
                        TurnOutcome::Completed
                    },
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    struct TestAcpServer(Arc<PsychevoAcpAgent>);

    impl ConnectTo<Client> for TestAcpServer {
        async fn connect_to(self, client: impl ConnectTo<Agent>) -> Result<(), Error> {
            self.0.serve(client).await
        }
    }

    async fn test_agent() -> (Arc<PsychevoAcpAgent>, PathBuf) {
        let root = std::env::temp_dir().join(format!("psychevo-acp-v2-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create acp test root");
        let home = root.join("home");
        let inherited_env = BTreeMap::from([
            ("HOME".to_string(), root.display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
        ]);
        let agent = Arc::new(
            PsychevoAcpAgent::new(AcpOptions {
                home,
                db_path: root.join("state.db"),
                config_path: None,
                inherited_env,
            })
            .await
            .expect("test acp agent"),
        );
        (agent, root)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_list_round_trips_framework_pagination_cursor() {
        let (agent, root) = test_agent().await;
        let cwd = root.join("workspace");
        std::fs::create_dir_all(&cwd).expect("workspace");
        for _ in 0..51 {
            agent
                .framework
                .start_thread(StartThreadRequest::new(&cwd))
                .await
                .expect("thread");
        }

        let first = agent
            .list_sessions(ListSessionsRequest::new().cwd(&cwd))
            .await
            .expect("first page");
        assert_eq!(first.sessions.len(), 50);
        let cursor = first.next_cursor.clone().expect("next cursor");
        let second = agent
            .list_sessions(ListSessionsRequest::new().cwd(&cwd).cursor(cursor))
            .await
            .expect("second page");
        assert_eq!(second.sessions.len(), 1);
        let first_ids = first
            .sessions
            .iter()
            .map(|session| session.session_id.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(!first_ids.contains(&second.sessions[0].session_id.to_string()));
        assert!(second.next_cursor.is_none());

        agent.application.shutdown().await.expect("shutdown");
        let _ = std::fs::remove_dir_all(root);
    }

    async fn admission_gate_agent() -> (
        Arc<PsychevoAcpAgent>,
        Arc<AdmissionGateAdapter>,
        PathBuf,
        SessionId,
    ) {
        let root = std::env::temp_dir().join(format!("psychevo-acp-gate-{}", Uuid::now_v7()));
        let home = root.join("home");
        let cwd = root.join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let gate = AdmissionGateAdapter::new();
        let application = Application::builder()
            .home(&home)
            .database_path(root.join("state.db"))
            .agent_session_adapter(gate.clone())
            .build()
            .await
            .expect("gated application");
        let session_id = SessionId::new(format!("acp-admission-{}", Uuid::now_v7()));
        let agent = Arc::new(PsychevoAcpAgent {
            options: AcpOptions {
                home,
                db_path: root.join("state.db"),
                config_path: None,
                inherited_env: BTreeMap::new(),
            },
            framework: application.client(),
            application,
            sessions: Arc::new(Mutex::new(HashMap::from([(
                session_id.to_string(),
                AcpSession::new(cwd, Vec::new()),
            )]))),
            client_terminal_auth: Arc::new(Mutex::new(false)),
            client_terminal_output: Arc::new(Mutex::new(false)),
        });
        (agent, gate, root, session_id)
    }

    async fn wait_for_gate(signal: &Semaphore, description: &str) {
        tokio::time::timeout(Duration::from_secs(5), signal.acquire())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {description}"))
            .expect("admission gate remains open")
            .forget();
    }

    #[test]
    fn admission_steer_fifo_preserves_order_and_framework_capacity() {
        let mut session = AcpSession::new(PathBuf::from("."), Vec::new());
        session.starting_turn = true;
        session
            .retain_admission_steer("first".to_string())
            .expect("first steer");
        session
            .retain_admission_steer("second".to_string())
            .expect("second steer");
        assert_eq!(
            session
                .pending_admission_steers
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );

        while session.pending_admission_steers.len() < MAX_QUEUED_STEERS {
            session
                .retain_admission_steer("bounded".to_string())
                .expect("within count bound");
        }
        let count_error = session
            .retain_admission_steer("newest".to_string())
            .expect_err("newest steer must be rejected at count capacity");
        assert_eq!(count_error.code, ErrorCode::InvalidParams);

        let mut byte_session = AcpSession::new(PathBuf::from("."), Vec::new());
        byte_session.starting_turn = true;
        let byte_error = byte_session
            .retain_admission_steer("x".repeat(MAX_QUEUED_STEER_BYTES))
            .expect_err("serialized steer must be rejected at byte capacity");
        assert_eq!(byte_error.code, ErrorCode::InvalidParams);

        assert_eq!(session.clear_pending_admission_steers(), MAX_QUEUED_STEERS);
        assert!(session.pending_admission_steers.is_empty());
        assert_eq!(session.pending_admission_steer_bytes, 0);

        session.starting_turn = true;
        session.cancel_starting_turn = true;
        assert_eq!(
            session
                .retain_admission_steer("after cancel".to_string())
                .expect_err("canceled admission must reject later steer")
                .code,
            ErrorCode::InvalidParams
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn admission_steers_transfer_to_the_first_handle_in_fifo_order() -> Result<(), Error> {
        let (agent, gate, root, session_id) = admission_gate_agent().await;
        let request_session_id = session_id.clone();
        let request_gate = gate.clone();
        let request_agent = agent.clone();
        let result = Client
            .v2()
            .connect_with(TestAcpServer(agent.clone()), async move |cx| {
                cx.send_request(InitializeRequest::new(
                    ProtocolVersion::V2,
                    Implementation::new("psychevo-acp-test-client", "1"),
                ))
                .block_task()
                .await?;

                let prompt_cx = cx.clone();
                let prompt_session_id = request_session_id.clone();
                let prompt_task = tokio::spawn(async move {
                    prompt_cx
                        .send_request(PromptRequest::new(
                            prompt_session_id,
                            vec![ContentBlock::Text(TextContent::new("initial"))],
                        ))
                        .block_task()
                        .await
                });
                wait_for_gate(&request_gate.prepare_entered, "turn preparation").await;

                for steer in ["first", "second"] {
                    cx.send_request(PromptRequest::new(
                        request_session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(format!(
                            "/steer {steer}"
                        )))],
                    ))
                    .block_task()
                    .await?;
                }
                assert_eq!(
                    request_agent
                        .sessions
                        .lock()
                        .expect("sessions")
                        .get(&request_session_id.to_string())
                        .expect("session")
                        .pending_admission_steers
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    vec!["first", "second"]
                );

                request_gate.release_prepare.add_permits(1);
                wait_for_gate(&request_gate.invoke_entered, "turn invocation").await;
                let turn = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if let Some(turn) = request_agent
                            .sessions
                            .lock()
                            .expect("sessions")
                            .get(&request_session_id.to_string())
                            .and_then(|session| session.turn.clone())
                        {
                            break turn;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("first handle publication");
                assert_eq!(turn.cancel_all_queued_steers(), 2);
                assert!(
                    request_agent
                        .sessions
                        .lock()
                        .expect("sessions")
                        .get(&request_session_id.to_string())
                        .expect("session")
                        .pending_admission_steers
                        .is_empty()
                );

                request_gate.release_invoke.add_permits(1);
                prompt_task.await.expect("prompt task")?;
                Ok(())
            })
            .await;

        agent
            .application
            .shutdown()
            .await
            .map_err(Error::into_internal_error)?;
        let _ = std::fs::remove_dir_all(root);
        result
    }

    #[tokio::test(flavor = "current_thread")]
    async fn admission_cancel_clears_steers_interrupts_the_handle_and_allows_new_session()
    -> Result<(), Error> {
        let (agent, gate, root, session_id) = admission_gate_agent().await;
        let request_session_id = session_id.clone();
        let request_gate = gate.clone();
        let request_agent = agent.clone();
        let result = Client
            .v2()
            .connect_with(TestAcpServer(agent.clone()), async move |cx| {
                cx.send_request(InitializeRequest::new(
                    ProtocolVersion::V2,
                    Implementation::new("psychevo-acp-test-client", "1"),
                ))
                .block_task()
                .await?;

                let prompt_cx = cx.clone();
                let prompt_session_id = request_session_id.clone();
                let prompt_task = tokio::spawn(async move {
                    prompt_cx
                        .send_request(PromptRequest::new(
                            prompt_session_id,
                            vec![ContentBlock::Text(TextContent::new("initial"))],
                        ))
                        .block_task()
                        .await
                });
                wait_for_gate(&request_gate.prepare_entered, "turn preparation").await;

                cx.send_request(PromptRequest::new(
                    request_session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("/steer retained"))],
                ))
                .block_task()
                .await?;
                cx.send_request(PromptRequest::new(
                    request_session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("/new"))],
                ))
                .block_task()
                .await?;
                assert_eq!(
                    request_agent
                        .sessions
                        .lock()
                        .expect("sessions")
                        .get(&request_session_id.to_string())
                        .expect("session")
                        .pending_admission_steers
                        .len(),
                    1,
                    "active-turn /new guidance must not reset retained steer state"
                );

                cx.send_notification(CancelSessionNotification::new(request_session_id.clone()))?;
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let canceled = request_agent
                            .sessions
                            .lock()
                            .expect("sessions")
                            .get(&request_session_id.to_string())
                            .is_some_and(|session| {
                                session.cancel_starting_turn
                                    && session.pending_admission_steers.is_empty()
                            });
                        if canceled {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("admission cancel observation");

                request_gate.release_prepare.add_permits(1);
                wait_for_gate(&request_gate.invoke_entered, "turn invocation").await;
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let published = request_agent
                            .sessions
                            .lock()
                            .expect("sessions")
                            .get(&request_session_id.to_string())
                            .is_some_and(|session| session.turn.is_some());
                        if published {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("canceled handle publication");
                request_gate.release_invoke.add_permits(1);
                prompt_task.await.expect("prompt task")?;
                assert!(request_gate.interrupted.load(Ordering::SeqCst));

                cx.send_request(PromptRequest::new(
                    request_session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("/new"))],
                ))
                .block_task()
                .await?;
                let sessions = request_agent.sessions.lock().expect("sessions");
                let session = sessions
                    .get(&request_session_id.to_string())
                    .expect("session");
                assert!(session.thread.is_none());
                assert!(session.pending_admission_steers.is_empty());
                Ok(())
            })
            .await;

        agent
            .application
            .shutdown()
            .await
            .map_err(Error::into_internal_error)?;
        let _ = std::fs::remove_dir_all(root);
        result
    }

    #[tokio::test(flavor = "current_thread")]
    async fn v2_client_negotiates_v2_and_receives_session_config_options() -> Result<(), Error> {
        let (agent, root) = test_agent().await;
        std::fs::create_dir_all(root.join("home")).expect("home");
        std::fs::write(
            root.join("home/config.toml"),
            r#"
model = "mock/default"

[provider.mock]
api = "http://127.0.0.1:9"
no_auth = true

[provider.mock.models.default]
[provider.mock.models.other]
"#,
        )
        .expect("config");
        let cwd = std::env::current_dir().map_err(Error::into_internal_error)?;
        let thread_cwd = cwd.clone();
        let framework = agent.framework.clone();

        let result = Client
            .v2()
            .connect_with(TestAcpServer(Arc::clone(&agent)), async move |cx| {
                let initialize = cx
                    .send_request(
                        InitializeRequest::new(
                            ProtocolVersion::V2,
                            Implementation::new("psychevo-acp-test-client", "1"),
                        )
                        .capabilities(ClientCapabilities::new().auth(
                            AuthCapabilities::new().terminal(TerminalAuthCapabilities::new()),
                        )),
                    )
                    .block_task()
                    .await?;
                assert_eq!(initialize.protocol_version, ProtocolVersion::V2);
                let session_capabilities = initialize
                    .capabilities
                    .session
                    .as_ref()
                    .expect("session capabilities");
                assert!(
                    session_capabilities
                        .prompt
                        .as_ref()
                        .is_some_and(|prompt| prompt.embedded_context.is_some())
                );

                let session = cx
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task()
                    .await?;
                let options = session.config_options;
                assert!(options.iter().any(|option| {
                    option.config_id.to_string() == "mode"
                        && matches!(option.category, Some(SessionConfigOptionCategory::Mode))
                }));
                let options_value = serde_json::to_value(&options).expect("options json");
                assert_eq!(
                    select_current_value(&options_value, "model").as_deref(),
                    Some("mock/default")
                );
                assert_eq!(
                    select_current_value(&options_value, "effort").as_deref(),
                    Some("none")
                );

                let options = cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        session.session_id.clone(),
                        "model",
                        "mock/other",
                    ))
                    .block_task()
                    .await?
                    .config_options;
                let options_value = serde_json::to_value(&options).expect("model options json");
                assert_eq!(
                    select_current_value(&options_value, "model").as_deref(),
                    Some("mock/other")
                );

                let options = cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        session.session_id,
                        "effort",
                        "high",
                    ))
                    .block_task()
                    .await?
                    .config_options;
                let options_value = serde_json::to_value(&options).expect("effort options json");
                assert_eq!(
                    select_current_value(&options_value, "effort").as_deref(),
                    Some("high")
                );
                Ok(())
            })
            .await;

        let threads = framework
            .list_threads(ThreadListQuery {
                cwd: Some(thread_cwd),
                ..Default::default()
            })
            .await
            .map_err(Error::into_internal_error)?;
        assert!(
            threads.threads.is_empty(),
            "session/new and config changes must not materialize a Framework thread"
        );
        agent
            .application
            .shutdown()
            .await
            .map_err(Error::into_internal_error)?;

        let _ = std::fs::remove_dir_all(root);
        result
    }

    #[tokio::test(flavor = "current_thread")]
    async fn framework_turn_request_keeps_the_acp_snapshot_root() {
        let root = std::env::temp_dir().join(format!("psychevo-acp-v2-{}", Uuid::now_v7()));
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let observed = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(&home)
            .database_path(root.join("state.db"))
            .agent_session_adapter(Arc::new(SnapshotRootAdapter {
                observed: observed.clone(),
            }))
            .build()
            .await
            .expect("application");
        let framework = application.client();
        let agent = PsychevoAcpAgent {
            options: AcpOptions {
                home: home.clone(),
                db_path: root.join("state.db"),
                config_path: None,
                inherited_env: BTreeMap::new(),
            },
            framework: framework.clone(),
            application,
            sessions: Arc::default(),
            client_terminal_auth: Arc::new(Mutex::new(false)),
            client_terminal_output: Arc::new(Mutex::new(false)),
        };
        let cwd = root.join("workspace");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let thread = agent
            .framework
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let session = AcpSession::loaded(cwd.clone(), thread.clone(), Vec::new());
        let request = agent.turn_request(&session, "snapshot".to_string(), Vec::new(), None);
        let handle = thread.start_turn(request).await.expect("accepted turn");
        let error = handle
            .wait()
            .await
            .expect_err("fixture stops after observation");

        assert_eq!(error.to_string(), "snapshot root observed");
        assert_eq!(
            *observed.lock().expect("snapshot root observation"),
            Some(home.join("snapshots"))
        );
        agent.application.shutdown().await.expect("shutdown");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn new_slash_command_detaches_the_acp_actor_from_its_backing_thread() -> Result<(), Error>
    {
        let (agent, root) = test_agent().await;
        let cwd = root.join("workspace");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let thread = agent
            .framework
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .map_err(Error::into_internal_error)?;
        let session_id = thread.id().to_string();
        let request_session_id = session_id.clone();
        let request_cwd = cwd.clone();

        let result = Client
            .v2()
            .connect_with(TestAcpServer(Arc::clone(&agent)), async move |cx| {
                cx.send_request(InitializeRequest::new(
                    ProtocolVersion::V2,
                    Implementation::new("psychevo-acp-test-client", "1"),
                ))
                .block_task()
                .await?;
                cx.send_request(ResumeSessionRequest::new(
                    request_session_id.clone(),
                    request_cwd,
                ))
                .block_task()
                .await?;
                cx.send_request(PromptRequest::new(
                    request_session_id,
                    vec![ContentBlock::Text(TextContent::new("/new"))],
                ))
                .block_task()
                .await?;
                Ok(())
            })
            .await;

        assert!(
            agent
                .sessions
                .lock()
                .expect("sessions")
                .get(&session_id)
                .is_some_and(|session| session.thread.is_none()),
            "/new must keep the ACP actor while detaching its backing Framework thread"
        );
        let threads = agent
            .framework
            .list_threads(ThreadListQuery {
                cwd: Some(cwd),
                ..Default::default()
            })
            .await
            .map_err(Error::into_internal_error)?;
        assert_eq!(threads.threads.len(), 1, "/new must not create a thread");

        agent
            .application
            .shutdown()
            .await
            .map_err(Error::into_internal_error)?;
        let _ = std::fs::remove_dir_all(root);
        result
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_load_honors_replay_cursor_order_and_warning_bound() -> Result<(), Error> {
        let root = std::env::temp_dir().join(format!("psychevo-acp-replay-{}", Uuid::now_v7()));
        let home = root.join("home");
        let cwd = root.join("workspace");
        let db_path = root.join("state.db");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let application = Application::builder()
            .home(&home)
            .database_path(&db_path)
            .agent_session_adapter(Arc::new(ReplayHistoryAdapter::default()))
            .build()
            .await
            .map_err(Error::into_internal_error)?;
        let framework = application.client();
        let thread = framework
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .map_err(Error::into_internal_error)?;
        for call in 0..40 {
            thread
                .start_turn(TurnRequest::new(format!("turn {call}")))
                .await
                .map_err(Error::into_internal_error)?
                .wait()
                .await
                .map_err(Error::into_internal_error)?;
        }
        let mut connection = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(&db_path),
        )
        .await
        .map_err(Error::into_internal_error)?;
        sqlx::query(
            "UPDATE messages SET message_json = 'not-json' \
             WHERE session_id = ?1 AND session_seq % 2 = 1",
        )
        .bind(thread.id())
        .execute(&mut connection)
        .await
        .map_err(Error::into_internal_error)?;
        connection
            .close()
            .await
            .map_err(Error::into_internal_error)?;

        let agent = Arc::new(PsychevoAcpAgent {
            options: AcpOptions {
                home,
                db_path,
                config_path: None,
                inherited_env: BTreeMap::new(),
            },
            framework,
            application,
            sessions: Arc::default(),
            client_terminal_auth: Arc::new(Mutex::new(false)),
            client_terminal_output: Arc::new(Mutex::new(false)),
        });
        let observed = Arc::new(Mutex::new(Vec::<String>::new()));
        let notification_observed = Arc::clone(&observed);
        let request_session_id = thread.id().to_string();
        let request_cwd = cwd.clone();
        let response_observed = Arc::clone(&observed);
        let response = Client
            .v2()
            .on_receive_notification(
                async move |notification: UpdateSessionNotification, _cx| {
                    if let Some(message_id) = replay_message_id(&notification.update) {
                        notification_observed
                            .lock()
                            .expect("observed replay")
                            .push(message_id);
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(TestAcpServer(Arc::clone(&agent)), async move |cx| {
                cx.send_request(InitializeRequest::new(
                    ProtocolVersion::V2,
                    Implementation::new("psychevo-acp-test-client", "1"),
                ))
                .block_task()
                .await?;
                let omitted = cx
                    .send_request(ResumeSessionRequest::new(
                        request_session_id.clone(),
                        request_cwd.clone(),
                    ))
                    .block_task()
                    .await?;
                assert!(omitted.meta.is_none());
                response_observed
                    .lock()
                    .expect("observed replay")
                    .push("omitted_response".to_string());

                let error = cx
                    .send_request(
                        ResumeSessionRequest::new(request_session_id.clone(), request_cwd.clone())
                            .replay_from(ReplayFrom::Other(OtherReplayFrom::new(
                                "_checkpoint",
                                BTreeMap::new(),
                            ))),
                    )
                    .block_task()
                    .await
                    .expect_err("unknown replay cursor must be rejected");
                assert_eq!(error.code, ErrorCode::InvalidParams);

                let response = cx
                    .send_request(
                        ResumeSessionRequest::new(request_session_id, request_cwd)
                            .replay_from(ReplayFrom::from(ReplayFromStart::new())),
                    )
                    .block_task()
                    .await?;
                response_observed
                    .lock()
                    .expect("observed replay")
                    .push("response".to_string());
                Ok(response)
            })
            .await?;

        let observed = observed.lock().expect("observed replay").clone();
        assert_eq!(observed.len(), 82);
        assert_eq!(
            observed.first().map(String::as_str),
            Some("omitted_response")
        );
        assert_eq!(
            observed.get(1).map(String::as_str),
            Some("history:1:unavailable")
        );
        assert_eq!(
            observed.get(2).map(String::as_str),
            Some("history:2:source")
        );
        assert_eq!(
            observed.get(3).map(String::as_str),
            Some("history:3:unavailable")
        );
        assert_eq!(observed.last().map(String::as_str), Some("response"));
        let warnings = response
            .meta
            .as_ref()
            .and_then(|meta| meta.get("psychevo"))
            .and_then(|value| value.get("replay_warnings"))
            .expect("bounded replay warnings");
        assert_eq!(warnings["items"].as_array().map(Vec::len), Some(32));
        assert_eq!(warnings["omitted_count"].as_u64(), Some(9));

        agent
            .application
            .shutdown()
            .await
            .map_err(Error::into_internal_error)?;
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    fn replay_message_id(update: &SessionUpdate) -> Option<String> {
        let chunk = match update {
            SessionUpdate::UserMessageChunk(chunk)
            | SessionUpdate::AgentMessageChunk(chunk)
            | SessionUpdate::AgentThoughtChunk(chunk) => chunk,
            _ => return None,
        };
        let message_id = chunk.message_id.to_string();
        message_id.starts_with("history:").then_some(message_id)
    }

    fn select_current_value(options: &Value, id: &str) -> Option<String> {
        options
            .as_array()?
            .iter()
            .find(|option| {
                option
                    .get("configId")
                    .or_else(|| option.get("id"))
                    .and_then(Value::as_str)
                    == Some(id)
            })?
            .get("currentValue")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }
}

pub async fn run_stdio(options: AcpOptions) -> std::io::Result<()> {
    let _ = std::fs::create_dir_all(&options.home);
    let agent = Arc::new(
        PsychevoAcpAgent::new(options)
            .await
            .map_err(|err| std::io::Error::other(format!("state DB error: {err}")))?,
    );
    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();
    let result = Arc::clone(&agent)
        .serve(ByteStreams::new(stdout, stdin))
        .await;
    let shutdown = agent
        .application
        .shutdown()
        .await
        .and_then(psychevo::ShutdownReport::require_clean);
    match (result, shutdown) {
        (Err(error), _) => Err(std::io::Error::other(format!("ACP error: {error}"))),
        (Ok(()), Err(error)) => Err(std::io::Error::other(format!(
            "ACP shutdown error: {error}"
        ))),
        (Ok(()), Ok(_)) => Ok(()),
    }
}

pub(crate) struct PsychevoAcpAgent {
    pub(crate) options: AcpOptions,
    pub(crate) application: Application,
    pub(crate) framework: FrameworkClient,
    pub(crate) sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
    pub(crate) client_terminal_auth: Arc<Mutex<bool>>,
    pub(crate) client_terminal_output: Arc<Mutex<bool>>,
}

pub(super) struct AcpUsageUpdateContext<'a> {
    pub(super) snapshot: Option<&'a ContextSnapshot>,
    pub(super) context_limit: Option<u64>,
    pub(super) provider: &'a str,
    pub(super) model: &'a str,
    pub(super) usage: &'a Arc<Mutex<AcpUsageAccumulator>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AcpSession {
    pub(crate) cwd: PathBuf,
    pub(crate) mode: RunMode,
    pub(crate) permission_mode: Option<PermissionMode>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) thread: Option<Thread>,
    pub(crate) starting_turn: bool,
    pub(crate) cancel_starting_turn: bool,
    pub(crate) turn: Option<TurnHandle>,
    pub(crate) pending_admission_steers: VecDeque<String>,
    pub(crate) pending_admission_steer_bytes: usize,
    pub(crate) queued_prompts: VecDeque<String>,
    pub(crate) last_session_list: Vec<ThreadSummary>,
}

impl AcpSession {
    pub(crate) fn new(cwd: PathBuf, mcp_servers: Vec<McpServerInput>) -> Self {
        Self {
            cwd,
            mode: RunMode::Default,
            permission_mode: None,
            model: None,
            reasoning_effort: None,
            mcp_servers,
            thread: None,
            starting_turn: false,
            cancel_starting_turn: false,
            turn: None,
            pending_admission_steers: VecDeque::new(),
            pending_admission_steer_bytes: 0,
            queued_prompts: VecDeque::new(),
            last_session_list: Vec::new(),
        }
    }

    pub(crate) fn loaded(cwd: PathBuf, thread: Thread, mcp_servers: Vec<McpServerInput>) -> Self {
        Self {
            thread: Some(thread),
            ..Self::new(cwd, mcp_servers)
        }
    }

    pub(crate) fn active_turn(&self) -> bool {
        self.starting_turn || self.turn.is_some()
    }

    pub(crate) fn retain_admission_steer(&mut self, prompt: String) -> Result<(), Error> {
        if !self.starting_turn || self.cancel_starting_turn || self.turn.is_some() {
            return Err(Error::invalid_params().data("session is not awaiting a Turn handle"));
        }
        if self.pending_admission_steers.len() >= MAX_QUEUED_STEERS {
            return Err(Error::invalid_params().data(format!(
                "control input count limit reached ({MAX_QUEUED_STEERS})"
            )));
        }
        let bytes = serde_json::to_vec(&user_text_message(&prompt))
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        if bytes > MAX_QUEUED_STEER_BYTES
            || self.pending_admission_steer_bytes.saturating_add(bytes) > MAX_QUEUED_STEER_BYTES
        {
            return Err(Error::invalid_params().data(format!(
                "control input byte limit reached ({MAX_QUEUED_STEER_BYTES})"
            )));
        }
        self.pending_admission_steers.push_back(prompt);
        self.pending_admission_steer_bytes += bytes;
        Ok(())
    }

    pub(crate) fn take_pending_admission_steers(&mut self) -> VecDeque<String> {
        self.pending_admission_steer_bytes = 0;
        std::mem::take(&mut self.pending_admission_steers)
    }

    pub(crate) fn clear_pending_admission_steers(&mut self) -> usize {
        let count = self.pending_admission_steers.len();
        self.pending_admission_steers.clear();
        self.pending_admission_steer_bytes = 0;
        count
    }
}
