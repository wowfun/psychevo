
    use psychevo::__ai::Outcome;
    use psychevo::{__product::runtime::PermissionMode, __product::runtime::RunMode, __product::runtime::UserShellContextOptions};
    use psychevo::__agent_core::{AssistantBlock, Message, UserContentBlock};
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
        context_snapshot: Mutex<Option<psychevo::__product::usage::ContextSnapshot>>,
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
        ) -> BoxFuture<'static, psychevo::Result<RunResult>> {
            let inner = Arc::clone(&self.inner);
            Box::pin(async move {
                let run_number = inner.next_run.fetch_add(1, Ordering::SeqCst) + 1;
                let binding_before_run = if let Some(thread_id) =
                    request.options.session.as_deref()
                {
                    request
                        .options
                        .state
                        .gateway_runtime_binding(thread_id)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                }
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
                        .map(psychevo::__product::runtime::RunControl::abort_signal)
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
                            filesystem: None,
                            mcp_startup: None,
                            timeout_secs: 300,
                        })
                        .await;
                }

                let session_id = if let Some(session_id) = request.options.session.clone() {
                    request.options.state.resume_session(&session_id).await?;
                    session_id
                } else {
                    request
                        .options
                        .state
                        .create_session_with_metadata(
                            &request.options.cwd,
                            &request.runtime_source,
                            "fake-model",
                            "fake-provider",
                            None,
                        )
                        .await?
                };
                let outcome = if aborted {
                    Outcome::Aborted
                } else {
                    Outcome::Normal
                };
                let final_answer = format!("answer {run_number}");
                if outcome == Outcome::Normal && inner.persist_history.load(Ordering::SeqCst) {
                    let timestamp_ms = crate::gateway_now_ms();
                    request
                        .options
                        .state
                        .append_message(
                            &session_id,
                            &Message::User {
                                content: vec![UserContentBlock::text(
                                    request.options.prompt.clone(),
                                )],
                                timestamp_ms,
                            },
                        )
                        .await?;
                    request
                        .options
                        .state
                        .append_message(
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
                        )
                        .await?;
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
        _application: Application,
    }

    impl Harness {
        async fn send(&self, request: SendTurnRequest) -> psychevo::Result<GatewayTurnResult> {
            send_framework_turn(
                self._application.clone(),
                self.gateway.clone(),
                request,
            )
            .await
        }

        fn runner(&self) -> (Application, Gateway) {
            (self._application.clone(), self.gateway.clone())
        }
    }

    async fn harness(backend: Arc<FakeBackend>) -> Harness {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(temp.path().join("state.db")).await.expect("state runtime");
        let gateway = Gateway::with_backend(state.clone(), backend);
        let application = Application::__from_open_state(
            temp.path().join("home"),
            None,
            state.clone(),
            Arc::new(GatewayAgentSessionAdapter::new(gateway.clone())),
        );
        gateway
            .attach_framework_application(application.clone())
            .expect("attach Framework Application");
        Harness {
            _temp: temp,
            cwd,
            state,
            gateway,
            _application: application,
        }
    }

    async fn send_framework_turn(
        application: Application,
        gateway: Gateway,
        request: SendTurnRequest,
    ) -> psychevo::Result<GatewayTurnResult> {
        send_framework_turn_inner(application, gateway, request, None).await
    }

    async fn send_framework_turn_with_id(
        application: Application,
        gateway: Gateway,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo::Result<GatewayTurnResult> {
        send_framework_turn_inner(application, gateway, request, Some(turn_id)).await
    }

    async fn send_framework_turn_inner(
        application: Application,
        gateway: Gateway,
        mut request: SendTurnRequest,
        turn_id: Option<String>,
    ) -> psychevo::Result<GatewayTurnResult> {
        let client = application.client();
        let explicit_thread_id = request
            .thread_id
            .clone()
            .or_else(|| request.options.session.clone());
        let mapped_thread_id = if explicit_thread_id.is_none()
            && !request.reset_source_binding
            && let Some(source) = request.source.as_ref()
        {
            gateway.resolve_source_thread(source).await?
        } else {
            None
        };
        let continued_thread_id = if explicit_thread_id.is_none()
            && mapped_thread_id.is_none()
            && request.options.continue_latest
        {
            let continue_sources = if request.continue_sources.is_empty() {
                vec![
                    request
                        .runtime_source
                        .as_deref()
                        .unwrap_or("test"),
                ]
            } else {
                request
                    .continue_sources
                    .iter()
                    .map(String::as_str)
                    .collect()
            };
            gateway
                .state
                .latest_session_for_cwd_with_sources(&request.options.cwd, &continue_sources)
                .await?
        } else {
            None
        };
        let thread =
            if let Some(thread_id) = explicit_thread_id.or(mapped_thread_id).or(continued_thread_id)
        {
            client.resume_thread(&thread_id).await?
        } else {
            let mut start = psychevo::StartThreadRequest::new(&request.options.cwd);
            start.source = request
                .runtime_source
                .clone()
                .unwrap_or_else(|| "test".to_string());
            start.metadata = request.lineage.clone();
            let thread = client.start_thread(start).await?;
            if let Some(source) = request.bind_source.as_ref().or(request.source.as_ref())
                && source.lifetime != GatewaySourceLifetime::Invocation
            {
                gateway
                    .bind_source_thread(
                        source,
                        thread.id(),
                        &GatewayBackendInfo {
                            kind: BackendKind::Native,
                            runtime_ref: request.options.runtime_ref.clone(),
                            native_id: None,
                        },
                        request.lineage.clone(),
                    )
                    .await?;
            }
            thread
        };

        let input = if request.input.is_empty() {
            let mut input = vec![GatewayInputPart::Text {
                text: request.options.prompt.clone(),
            }];
            input.extend(request.options.image_inputs.iter().cloned().map(|image| {
                GatewayInputPart::Image {
                    input: match image {
                        ImageInput::LocalPath(path) => {
                            GatewayImageInput::LocalPath {
                                path: path.display().to_string(),
                            }
                        }
                        ImageInput::ImageUrl(url) => GatewayImageInput::Url { url },
                    },
                }
            }));
            input
        } else {
            std::mem::take(&mut request.input)
        };
        let mut caller = ThreadCallerContext::new(
            ThreadSurface::Other("conformance".to_string()),
            request.options.cwd.clone(),
        );
        caller.runtime_source = request
            .runtime_source
            .take()
            .unwrap_or_else(|| "test".to_string());
        caller.continue_sources = std::mem::take(&mut request.continue_sources);
        caller.stream_observer = request.stream.take();
        caller.event_observer = request.event_sink.take();
        caller.workspace_mutations = request.options.workspace_mutations.take();
        caller.runtime_tools = std::mem::take(&mut request.options.runtime_tools);
        if let (Some(handle), Some(control)) =
            (request.control_handle.take(), request.control.take())
        {
            caller.set_control(handle, control);
        }

        let mut intent = ThreadTurnIntent::new(input);
        intent.thread_id = Some(thread.id().to_string());
        intent.source = request.source;
        intent.bind_source = request.bind_source;
        intent.reset_source_binding = request.reset_source_binding;
        intent.lineage = request.lineage;
        intent.turn_id = Some(turn_id.unwrap_or_else(|| Uuid::now_v7().to_string()));
        intent.policy = ThreadTurnPolicy {
            snapshot_root: request.options.snapshot_root,
            continue_latest: false,
            extract_prompt_image_sources: request.options.extract_prompt_image_sources,
            prompt_display: request.options.prompt_display,
            max_context_messages: request.options.max_context_messages,
            config_path: request.options.config_path,
            project_context_override: request.options.project_context_override,
            sandbox_override: request.options.sandbox_override,
            model: request.options.model,
            reasoning_effort: request.options.reasoning_effort,
            runtime_profile_ref: request.options.runtime_ref,
            control_values: request.options.runtime_options,
            initial_thread_preferences: request.initial_thread_preferences,
            include_reasoning: request.options.include_reasoning,
            mode: request.options.mode,
            permission_mode: request.options.permission_mode,
            approval_handler: request.options.approval_handler,
            clarify_enabled: request.options.clarify_enabled,
            inherited_env: request.options.inherited_env,
            agent_ref: request.options.agent,
            no_agents: request.options.no_agents,
            no_skills: request.options.no_skills,
            selected_capability_roots: request.options.selected_capability_roots,
            skill_inputs: request.options.skill_inputs,
            mcp_servers: request.options.mcp_servers,
        };
        let submission = intent.into_framework_request(caller)?;
        let handle = thread.start_turn(submission.request).await?;
        let receipt = handle.receipt().clone();
        let result = handle.wait().await?;
        let outcome = match result.outcome {
            psychevo::TurnOutcome::Completed => Outcome::Normal,
            psychevo::TurnOutcome::Stopped => Outcome::Stopped,
            psychevo::TurnOutcome::Failed => Outcome::Failed,
            psychevo::TurnOutcome::Interrupted => Outcome::Aborted,
        };
        let status = match outcome {
            Outcome::Normal => GatewayTurnStatus::Completed,
            Outcome::Stopped | Outcome::Aborted => GatewayTurnStatus::Interrupted,
            Outcome::Failed => GatewayTurnStatus::Failed,
        };
        let committed_entries = gateway.thread_transcript(&receipt.thread_id).await?;
        Ok(GatewayTurnResult {
            thread: GatewayThread {
                id: receipt.thread_id.clone(),
                backend: GatewayBackendInfo {
                    kind: BackendKind::Native,
                    runtime_ref: None,
                    native_id: None,
                },
                source_key: None,
                forked_from_thread_id: None,
            },
            turn: GatewayTurn {
                id: receipt.turn_id,
                thread_id: Some(receipt.thread_id.clone()),
                status,
                outcome: Some(outcome.as_str().to_string()),
                error: None,
                started_at_ms: None,
                completed_at_ms: Some(gateway_now_ms()),
            },
            result: RunResult {
                session_id: result.thread_id,
                outcome,
                terminal_reason: result.terminal_reason,
                final_answer: result.final_answer,
                db_path: gateway.state.db_path().to_path_buf(),
                cwd: request.options.cwd,
                provider: result.provider,
                model: result.model,
                base_url: String::new(),
                api_key_env: None,
                reasoning_effort: result.reasoning_effort,
                context_limit: result.context_limit,
                tool_failures: result.tool_failures,
                selected_agent: result.selected_agent,
                selected_skills: result.selected_skills,
                context_snapshot: result.context_snapshot,
                terminal_error: result.terminal_error,
                events: Vec::new(),
                warnings: result.warnings,
            },
            committed_entries,
        })
    }

    fn attach_test_application(harness: &Harness, gateway: &Gateway) -> Application {
        let application = Application::__from_open_state(
            harness._temp.path().join("home"),
            None,
            harness.state.clone(),
            Arc::new(GatewayAgentSessionAdapter::new(gateway.clone())),
        );
        gateway
            .attach_framework_application(application.clone())
            .expect("attach test Framework Application");
        application
    }

    fn test_acp_command_toml(cwd: &std::path::Path) -> String {
        let fixture = crate::test_support::acp_fixture(cwd, "fake_acp_lifecycle");
        crate::test_support::toml_path(&fixture.program)
    }

    fn copied_acp_fixture(
        cwd: &std::path::Path,
        directory: &std::path::Path,
        name: &str,
        target_stem: &str,
    ) -> crate::test_support::AcpFixture {
        let fixture = crate::test_support::acp_fixture(cwd, name);
        let target = directory.join(target_stem).with_extension(
            fixture
                .script
                .extension()
                .expect("ACP fixture extension"),
        );
        std::fs::copy(&fixture.script, &target).expect("copy ACP test fixture");
        crate::test_support::AcpFixture {
            program: fixture.program,
            script: target,
        }
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
            mcp_runtime: None,
            workspace_mutations: None,
            runtime_tools: Vec::new(),
        }
    }

    fn request(harness: &Harness, source: GatewaySource, prompt: &str) -> SendTurnRequest {
        SendTurnRequest {
            thread_id: None,
            explicit_thread: false,
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

    #[tokio::test]
    async fn peer_delegate_resolver_accepts_subagent_only_backend_agent() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend).await;
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
                test_acp_command_toml(&harness.cwd),
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
