#[allow(unused_imports)]
pub(crate) use super::*;

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
    use super::*;
    use agent_client_protocol::schema::v2::{AuthCapabilities, TerminalAuthCapabilities};

    struct TestAcpServer(Arc<PsychevoAcpAgent>);

    impl ConnectTo<Client> for TestAcpServer {
        async fn connect_to(self, client: impl ConnectTo<Agent>) -> Result<(), Error> {
            self.0.serve(client).await
        }
    }

    fn test_agent() -> (Arc<PsychevoAcpAgent>, PathBuf) {
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
            .expect("test acp agent"),
        );
        (agent, root)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn v2_client_negotiates_v2_and_receives_session_config_options() -> Result<(), Error> {
        let (agent, root) = test_agent();
        std::fs::create_dir_all(root.join("home")).expect("home");
        std::fs::write(
            root.join("home/config.toml"),
            r#"
model = "mock/default"

[provider.mock.options]
base_url = "http://127.0.0.1:9"
no_auth = true

[provider.mock.models.default]
[provider.mock.models.other]
"#,
        )
        .expect("config");
        let cwd = std::env::current_dir().map_err(Error::into_internal_error)?;

        let result = Client
            .v2()
            .connect_with(TestAcpServer(agent), async move |cx| {
                let initialize = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::V2).capabilities(
                        ClientCapabilities::new().auth(
                            AuthCapabilities::new().terminal(TerminalAuthCapabilities::new()),
                        ),
                    ))
                    .block_task()
                    .await?;
                assert_eq!(initialize.protocol_version, ProtocolVersion::V2);
                assert!(initialize.capabilities.prompt.embedded_context.is_some());
                assert!(initialize.capabilities.session.load.is_some());

                let session = cx
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task()
                    .await?;
                let options = session.config_options.expect("session config options");
                assert!(options.iter().any(|option| {
                    option.id.to_string() == "mode"
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

        let _ = std::fs::remove_dir_all(root);
        result
    }

    fn select_current_value(options: &Value, id: &str) -> Option<String> {
        options
            .as_array()?
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some(id))?
            .get("currentValue")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }
}

pub async fn run_stdio(options: AcpOptions) -> std::io::Result<()> {
    let _ = std::fs::create_dir_all(&options.home);
    let agent = Arc::new(
        PsychevoAcpAgent::new(options)
            .map_err(|err| std::io::Error::other(format!("state DB error: {err}")))?,
    );
    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();
    agent
        .serve(ByteStreams::new(stdout, stdin))
        .await
        .map_err(|err| std::io::Error::other(format!("ACP error: {err}")))
}

pub(crate) struct PsychevoAcpAgent {
    pub(crate) options: AcpOptions,
    pub(crate) state: StateRuntime,
    pub(crate) gateway: Gateway,
    pub(crate) sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
    pub(crate) client_terminal_auth: Arc<Mutex<bool>>,
    pub(crate) client_terminal_output: Arc<Mutex<bool>>,
}

struct AcpUsageUpdateContext<'a> {
    snapshot: Option<&'a ContextSnapshot>,
    context_limit: Option<u64>,
    provider: &'a str,
    model: &'a str,
    usage: &'a Arc<Mutex<AcpUsageAccumulator>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AcpSession {
    pub(crate) cwd: PathBuf,
    pub(crate) runtime_session_id: Option<String>,
    pub(crate) mode: RunMode,
    pub(crate) permission_mode: Option<PermissionMode>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) control: Option<RunControlHandle>,
    pub(crate) queued_prompts: VecDeque<String>,
    pub(crate) pending_steers: Vec<psychevo_runtime::PendingInputId>,
    pub(crate) last_session_list: Vec<SessionSummary>,
}

impl AcpSession {
    pub(crate) fn new(
        cwd: PathBuf,
        runtime_session_id: Option<String>,
        mcp_servers: Vec<McpServerInput>,
    ) -> Self {
        Self {
            cwd,
            runtime_session_id,
            mode: RunMode::Default,
            permission_mode: None,
            model: None,
            reasoning_effort: None,
            mcp_servers,
            control: None,
            queued_prompts: VecDeque::new(),
            pending_steers: Vec::new(),
            last_session_list: Vec::new(),
        }
    }
}
