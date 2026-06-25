
    use psychevo_ai::Outcome;
    use psychevo_runtime::{
        Message, PermissionMode, RunMode, UserContentBlock, UserShellContextOptions,
    };
    use tokio::sync::{Notify, mpsc};

    #[derive(Debug, Clone)]
    struct FakeRun {
        prompt: String,
        session: Option<String>,
        workdir: PathBuf,
    }

    #[derive(Debug, Clone)]
    struct WaitFirst {
        run_number: usize,
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Default)]
    struct FakeBackendInner {
        runs: Mutex<Vec<FakeRun>>,
        next_run: AtomicUsize,
        wait_first: Mutex<Option<WaitFirst>>,
        request_permission: AtomicBool,
    }

    #[derive(Clone, Default)]
    struct FakeBackend {
        inner: Arc<FakeBackendInner>,
    }

    impl FakeBackend {
        fn runs(&self) -> Vec<FakeRun> {
            self.inner
                .runs
                .lock()
                .expect("fake run lock poisoned")
                .clone()
        }

        fn wait_on_first_run(&self) -> WaitFirst {
            self.wait_on_next_run()
        }

        fn wait_on_next_run(&self) -> WaitFirst {
            let run_number = self.inner.next_run.load(Ordering::SeqCst) + 1;
            let wait = WaitFirst {
                run_number,
                started: Arc::new(Notify::new()),
                release: Arc::new(Notify::new()),
            };
            *self
                .inner
                .wait_first
                .lock()
                .expect("fake wait lock poisoned") = Some(wait.clone());
            wait
        }

        fn request_permission(&self) {
            self.inner.request_permission.store(true, Ordering::SeqCst);
        }
    }

    impl fmt::Debug for FakeBackend {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("FakeBackend")
        }
    }

    impl GatewayBackend for FakeBackend {
        fn kind(&self) -> BackendKind {
            BackendKind::Psychevo
        }

        fn run_turn(
            &self,
            request: BackendTurnRequest,
        ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
            let inner = Arc::clone(&self.inner);
            Box::pin(async move {
                let run_number = inner.next_run.fetch_add(1, Ordering::SeqCst) + 1;
                {
                    let mut runs = inner.runs.lock().expect("fake run lock poisoned");
                    runs.push(FakeRun {
                        prompt: request.options.prompt.clone(),
                        session: request.options.session.clone(),
                        workdir: request.options.workdir.clone(),
                    });
                }

                let wait_first = inner
                    .wait_first
                    .lock()
                    .expect("fake wait lock poisoned")
                    .clone();
                if let Some(wait) = wait_first
                    && run_number == wait.run_number
                {
                    wait.started.notify_one();
                    wait.release.notified().await;
                }

                if inner.request_permission.load(Ordering::SeqCst)
                    && let Some(handler) = request.options.approval_handler.clone()
                {
                    let _decision = handler
                        .request_permission(PermissionApprovalRequest {
                            tool_call_id: "permission-1".to_string(),
                            tool_name: "fake_tool".to_string(),
                            summary: "fake permission".to_string(),
                            reason: "test permission".to_string(),
                            matched_rule: None,
                            suggested_rule: None,
                            allow_always: true,
                            timeout_secs: 300,
                        })
                        .await;
                }

                let session_id = if let Some(session_id) = request.options.session.clone() {
                    request.options.state.store().resume_session(&session_id)?;
                    session_id
                } else {
                    request.options.state.store().create_session_with_metadata(
                        &request.options.workdir,
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
                    workdir: request.options.workdir,
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

    struct Harness {
        _temp: tempfile::TempDir,
        workdir: PathBuf,
        state: StateRuntime,
        gateway: Gateway,
    }

    fn harness(backend: Arc<FakeBackend>) -> Harness {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state runtime");
        let gateway = Gateway::with_backend(state.clone(), backend);
        Harness {
            _temp: temp,
            workdir,
            state,
            gateway,
        }
    }

    fn run_options(harness: &Harness, prompt: &str) -> RunOptions {
        RunOptions {
            state: harness.state.clone(),
            workdir: harness.workdir.clone(),
            snapshot_root: None,
            session: None,
            continue_latest: false,
            prompt: prompt.to_string(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: None,
            project_context_override: None,
            sandbox_override: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: Some(PermissionMode::Default),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            agent: None,
            external_agent_delegate: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            runtime_tools: Vec::new(),
        }
    }

    fn request(harness: &Harness, source: GatewaySource, prompt: &str) -> SendTurnRequest {
        SendTurnRequest {
            thread_id: None,
            source: Some(source),
            bind_source: None,
            reset_source_binding: false,
            input: Vec::new(),
            options: run_options(harness, prompt),
            runtime_source: Some("test".to_string()),
            continue_sources: vec!["test".to_string()],
            stream: None,
            event_sink: None,
            control_handle: None,
            control: None,
            lineage: None,
        }
    }

    #[test]
    fn peer_delegate_resolver_accepts_subagent_only_backend_agent() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend);
        let home = harness._temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            home.join("config.toml"),
            r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = "python3"
args = ["fake_acp.py"]
entrypoints = ["subagent"]
"#,
        )
        .expect("config");
        let agents_dir = harness.workdir.join(".psychevo").join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir");
        std::fs::write(
            agents_dir.join("opencode.md"),
            r#"---
name: opencode
description: Delegate to fake ACP.
backend:
  ref: fake
entrypoints: [subagent]
---
Delegate.
"#,
        )
        .expect("agent");
        let mut options = run_options(&harness, "@opencode list tools");
        options.inherited_env = Some(BTreeMap::from([
            (
                "HOME".to_string(),
                harness._temp.path().display().to_string(),
            ),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
        ]));

        let peer = resolve_peer_delegate(
            &options,
            &ExternalAgentDelegateRequest {
                run_id: "run-1".to_string(),
                parent_session_id: "parent".to_string(),
                child_session_id: "child".to_string(),
                agent_name: "opencode".to_string(),
                agent_description: "Delegate to fake ACP.".to_string(),
                backend_ref: "fake".to_string(),
                prompt: "list tools".to_string(),
                task_name: "opencode-run".to_string(),
                model: None,
                runtime_options: BTreeMap::new(),
                abort: {
                    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
                    AbortSignal::new(abort_rx)
                },
            },
        )
        .expect("delegate peer");
        assert_eq!(peer.backend.id, "fake");
        assert!(peer.agent.supports_entrypoint(AgentEntrypoint::Subagent));
        assert!(!peer.agent.supports_entrypoint(AgentEntrypoint::Peer));
    }
