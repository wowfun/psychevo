
    use psychevo_ai::Outcome;
    use psychevo_runtime::{
        AssistantBlock, Message, PermissionMode, RunMode, UserContentBlock,
        UserShellContextOptions,
    };
    use tokio::sync::{Notify, mpsc};

    #[derive(Debug, Clone)]
    struct FakeRun {
        prompt: String,
        session: Option<String>,
        cwd: PathBuf,
        model: Option<String>,
        reasoning_effort: Option<String>,
        mode: RunMode,
        permission_mode: Option<PermissionMode>,
        runtime_options: BTreeMap<String, String>,
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
        binding_before_run: Mutex<Vec<bool>>,
        next_run: AtomicUsize,
        wait_first: Mutex<Option<WaitFirst>>,
        request_permission: AtomicBool,
        emit_stream_terminal: AtomicBool,
        persist_history: AtomicBool,
        context_snapshot: Mutex<Option<psychevo_runtime::ContextSnapshot>>,
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

        fn binding_before_run(&self) -> Vec<bool> {
            self.inner
                .binding_before_run
                .lock()
                .expect("fake binding observation lock poisoned")
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

        fn emit_stream_terminal(&self) {
            self.inner
                .emit_stream_terminal
                .store(true, Ordering::SeqCst);
        }

        fn persist_history(&self) {
            self.inner.persist_history.store(true, Ordering::SeqCst);
        }

    }

    impl fmt::Debug for FakeBackend {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("FakeBackend")
        }
    }

    impl GatewayBackend for FakeBackend {
        fn kind(&self) -> BackendKind {
            BackendKind::Native
        }

        fn run_turn(
            &self,
            request: BackendTurnRequest,
        ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
            let inner = Arc::clone(&self.inner);
            Box::pin(async move {
                let run_number = inner.next_run.fetch_add(1, Ordering::SeqCst) + 1;
                let binding_before_run = request
                    .options
                    .session
                    .as_deref()
                    .and_then(|thread_id| {
                        request
                            .options
                            .state
                            .store()
                            .gateway_runtime_binding(thread_id)
                            .ok()
                            .flatten()
                    })
                    .is_some_and(|binding| {
                        binding.status == GatewayRuntimeBindingStatus::Resolved
                            && binding.runtime_ref.as_deref() == Some("native")
                            && binding.native_session_id.as_deref()
                                == request.options.session.as_deref()
                    });
                inner
                    .binding_before_run
                    .lock()
                    .expect("fake binding observation lock poisoned")
                    .push(binding_before_run);
                {
                    let mut runs = inner.runs.lock().expect("fake run lock poisoned");
                    runs.push(FakeRun {
                        prompt: request.options.prompt.clone(),
                        session: request.options.session.clone(),
                        cwd: request.options.cwd.clone(),
                        model: request.options.model.clone(),
                        reasoning_effort: request.options.reasoning_effort.clone(),
                        mode: request.options.mode,
                        permission_mode: request.options.permission_mode,
                        runtime_options: request.options.runtime_options.clone(),
                    });
                }

                let wait_first = inner
                    .wait_first
                    .lock()
                    .expect("fake wait lock poisoned")
                    .clone();
                let mut aborted = false;
                if let Some(wait) = wait_first
                    && run_number == wait.run_number
                {
                    wait.started.notify_one();
                    if let Some(mut abort) = request
                        .control
                        .as_ref()
                        .map(psychevo_runtime::RunControl::abort_signal)
                    {
                        tokio::select! {
                            _ = wait.release.notified() => {}
                            _ = abort.wait_for_abort() => aborted = true,
                        }
                    } else {
                        wait.release.notified().await;
                    }
                }

                if !aborted
                    && inner.request_permission.swap(false, Ordering::SeqCst)
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
                        &request.options.cwd,
                        &request.runtime_source,
                        "fake-model",
                        "fake-provider",
                        None,
                    )?
                };
                let outcome = if aborted {
                    Outcome::Aborted
                } else {
                    Outcome::Normal
                };
                let final_answer = format!("answer {run_number}");
                if outcome == Outcome::Normal && inner.persist_history.load(Ordering::SeqCst) {
                    let timestamp_ms = crate::gateway_now_ms();
                    request.options.state.store().append_message(
                        &session_id,
                        &Message::User {
                            content: vec![UserContentBlock::text(request.options.prompt.clone())],
                            timestamp_ms,
                        },
                    )?;
                    request.options.state.store().append_message(
                        &session_id,
                        &Message::Assistant {
                            content: vec![AssistantBlock::Text {
                                text: final_answer.clone(),
                            }],
                            timestamp_ms: timestamp_ms.saturating_add(1),
                            finish_reason: Some("stop".to_string()),
                            outcome,
                            model: Some("fake-model".to_string()),
                            provider: Some("fake-provider".to_string()),
                        },
                    )?;
                }
                if inner.emit_stream_terminal.load(Ordering::SeqCst)
                    && let Some(stream) = request.stream.as_ref()
                {
                    stream(RunStreamEvent::value(json!({
                        "type": "turn_complete",
                        "session_id": session_id.clone(),
                        "source": "native_conformance_fake",
                        "outcome": outcome.as_str(),
                    })));
                }

                Ok(RunResult {
                    session_id,
                    outcome,
                    terminal_reason: None,
                    final_answer,
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
                    context_snapshot: inner
                        .context_snapshot
                        .lock()
                        .expect("fake context snapshot lock poisoned")
                        .clone(),
                    terminal_error: None,
                    events: Vec::new(),
                    warnings: Vec::new(),
                })
            })
        }
    }

    struct Harness {
        _temp: tempfile::TempDir,
        cwd: PathBuf,
        state: StateRuntime,
        gateway: Gateway,
    }

    fn harness(backend: Arc<FakeBackend>) -> Harness {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state runtime");
        let gateway = Gateway::with_backend(state.clone(), backend);
        Harness {
            _temp: temp,
            cwd,
            state,
            gateway,
        }
    }

    fn test_python_command_toml(cwd: &std::path::Path) -> String {
        let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let python = psychevo_runtime::resolve_executable_path(
            "python3",
            cwd,
            &psychevo_runtime::ExecutableResolveOptions {
                platform: psychevo_runtime::HostPlatform::current(),
                env: &host_env,
            },
        )
        .expect("resolve ACP test fixture python");
        serde_json::to_string(&python.to_string_lossy()).expect("quote ACP fixture python")
    }

    fn run_options(harness: &Harness, prompt: &str) -> RunOptions {
        RunOptions {
            state: harness.state.clone(),
            cwd: harness.cwd.clone(),
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
            selected_capability_roots: Vec::new(),
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
            initial_thread_preferences: BTreeMap::new(),
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
            format!(
                r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = {}
args = ["fake_acp.py"]
entrypoints = ["subagent"]
"#,
                test_python_command_toml(&harness.cwd),
            ),
        )
        .expect("config");
        let agents_dir = harness.cwd.join(".psychevo").join("agents");
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
                runtime_ref: "acp:fake".to_string(),
                backend_ref: Some("fake".to_string()),
                instructions: Some("Delegate.".to_string()),
                prompt: "list tools".to_string(),
                task_name: "opencode-run".to_string(),
                model: None,
                runtime_options: BTreeMap::new(),
                expected_runtime_profile_revision: None,
                abort: {
                    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
                    AbortSignal::new(abort_rx)
                },
            },
            "test-profile-fingerprint",
        )
        .expect("delegate peer");
        assert_eq!(peer.backend.id, "fake");
        assert!(peer.agent.supports_entrypoint(AgentEntrypoint::Subagent));
        assert!(!peer.agent.supports_entrypoint(AgentEntrypoint::Peer));
    }
