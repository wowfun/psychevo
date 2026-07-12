#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentConformanceRuntime {
    Native,
    Acp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentConformanceProcessExitDelivery {
    Unknown,
}

impl AgentConformanceRuntime {
    fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Acp => "acp",
        }
    }

    fn process_exit_delivery(self) -> Option<AgentConformanceProcessExitDelivery> {
        match self {
            Self::Native => None,
            Self::Acp => Some(AgentConformanceProcessExitDelivery::Unknown),
        }
    }

    fn runtime_ref(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Acp => "acp:conformance",
        }
    }
}

struct AgentConformanceHarness {
    runtime: AgentConformanceRuntime,
    inner: Harness,
    backend: Arc<FakeBackend>,
    home: PathBuf,
    log: PathBuf,
    release_dir: PathBuf,
}

enum AgentConformanceHold {
    Native(WaitFirst),
    Acp(PathBuf),
}

impl AgentConformanceHarness {
    fn new(runtime: AgentConformanceRuntime) -> Self {
        let backend = Arc::new(FakeBackend::default());
        let inner = harness(backend.clone());
        let home = inner._temp.path().join("conformance-home");
        let log = inner._temp.path().join("agent-conformance.jsonl");
        let release_dir = inner._temp.path().join("agent-conformance-releases");
        std::fs::create_dir_all(&home).expect("conformance home");
        std::fs::create_dir_all(&release_dir).expect("conformance release directory");

        if runtime == AgentConformanceRuntime::Native {
            backend.emit_stream_terminal();
            backend.persist_history();
        }

        if runtime == AgentConformanceRuntime::Acp {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fake_agent_conformance.py");
            std::fs::write(
                home.join("config.toml"),
                format!(
                    r#"[agents.backends.conformance]
    kind = "acp"
    description = "Deterministic ACP Agent conformance fixture."
    command = {}
    args = ["{}", "{}", "{}", "{}"]
    entrypoints = ["peer"]
    "#,
                    test_python_command_toml(&inner.cwd),
                    fixture.display(),
                    log.display(),
                    inner.state.db_path().display(),
                    release_dir.display(),
                ),
            )
            .expect("ACP conformance config");
            let agents_dir = inner.cwd.join(".psychevo").join("agents");
            std::fs::create_dir_all(&agents_dir).expect("ACP conformance agents directory");
            std::fs::write(
                agents_dir.join("conformance.md"),
                r#"---
    name: conformance
    description: Deterministic ACP Agent conformance fixture.
    backend:
      ref: conformance
    entrypoints: [peer]
    ---
    Conformance fixture instructions.
    "#,
            )
            .expect("ACP conformance Agent Definition");
        }

        Self {
            runtime,
            inner,
            backend,
            home,
            log,
            release_dir,
        }
    }

    fn source(&self, case: &str, lane: &str) -> GatewaySource {
        GatewaySource::new(
            "agent-conformance",
            format!("{}:{case}:{lane}", self.runtime.label()),
        )
        .persistent()
    }

    fn request(&self, source: GatewaySource, prompt: &str) -> SendTurnRequest {
        let mut request = request(&self.inner, source, prompt);
        if self.runtime == AgentConformanceRuntime::Acp {
            request.options.agent = Some("conformance".to_string());
            request.options.runtime_ref = Some("acp:conformance".to_string());
            request.options.inherited_env = Some(BTreeMap::from([
                (
                    "HOME".to_string(),
                    self.inner._temp.path().display().to_string(),
                ),
                ("PSYCHEVO_HOME".to_string(), self.home.display().to_string()),
            ]));
        }
        request
    }

    fn records(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("ACP conformance record"))
            .collect()
    }

    fn observed_prompts(&self) -> Vec<String> {
        match self.runtime {
            AgentConformanceRuntime::Native => self
                .backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect(),
            AgentConformanceRuntime::Acp => self
                .records()
                .into_iter()
                .filter(|record| record["event"] == "prompt")
                .filter_map(|record| record["prompt"].as_str().map(str::to_string))
                .collect(),
        }
    }

    fn prepare_hold(&self, token: &str) -> AgentConformanceHold {
        match self.runtime {
            AgentConformanceRuntime::Native => {
                AgentConformanceHold::Native(self.backend.wait_on_next_run())
            }
            AgentConformanceRuntime::Acp => {
                AgentConformanceHold::Acp(self.release_dir.join(format!("release-{token}")))
            }
        }
    }

    async fn wait_for_hold(&self, hold: &AgentConformanceHold, prompt: &str) {
        match hold {
            AgentConformanceHold::Native(wait) => {
                tokio::time::timeout(Duration::from_secs(5), wait.started.notified())
                    .await
                    .unwrap_or_else(|_| {
                        panic!(
                            "{} Agent did not enter held prompt `{prompt}`",
                            self.runtime.label()
                        )
                    });
            }
            AgentConformanceHold::Acp(_) => {
                wait_for_agent_conformance_condition(
                    format!("ACP Agent prompt `{prompt}`"),
                    || self.observed_prompts().iter().any(|value| value == prompt),
                )
                .await;
            }
        }
    }

    fn release(&self, hold: AgentConformanceHold) {
        match hold {
            AgentConformanceHold::Native(wait) => wait.release.notify_one(),
            AgentConformanceHold::Acp(path) => {
                std::fs::write(path, "release").expect("release ACP conformance prompt");
            }
        }
    }

    fn assert_binding_was_visible_before_prompt(&self) {
        match self.runtime {
            AgentConformanceRuntime::Native => assert_eq!(
                self.backend.binding_before_run(),
                vec![true],
                "Native Adapter must observe its immutable binding before execution"
            ),
            AgentConformanceRuntime::Acp => {
                let prompt = self
                    .records()
                    .into_iter()
                    .find(|record| record["event"] == "prompt")
                    .expect("ACP prompt record");
                assert_eq!(
                    prompt["bindingBeforePrompt"], true,
                    "ACP native session id must be durably attached before prompt"
                );
            }
        }
    }

    fn assert_exactly_one_terminal(
        &self,
        thread_id: &str,
        turn_id: &str,
        expected_status: &str,
        expected_outcome: Option<&str>,
    ) {
        let terminals = self
            .inner
            .state
            .store()
            .list_gateway_turn_terminals_for_thread(thread_id)
            .expect("conformance terminals");
        let matching = terminals
            .iter()
            .filter(|terminal| terminal.turn_id == turn_id)
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "{} Agent must persist exactly one terminal for turn {turn_id}",
            self.runtime.label()
        );
        assert_eq!(matching[0].status, expected_status);
        assert_eq!(matching[0].outcome.as_deref(), expected_outcome);
    }

    fn assert_certain_terminal_delivery(&self, turn_id: &str) {
        let delivery = self
            .inner
            .state
            .store()
            .gateway_turn_delivery(turn_id)
            .expect("conformance delivery lookup")
            .expect("conformance delivery");
        assert_eq!(delivery.runtime_ref, self.runtime.runtime_ref());
        assert_eq!(delivery.status, "terminal");
        assert_eq!(delivery.input_json, None);
        assert!(delivery.delivery_confirmed_at_ms.is_some());
        assert!(delivery.terminal_at_ms.is_some());
    }
}

async fn wait_for_agent_conformance_condition(
    label: impl Into<String>,
    mut condition: impl FnMut() -> bool,
) {
    let label = label.into();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
}

fn completion_turn_ids(events: &Arc<Mutex<Vec<GatewayEvent>>>) -> Vec<String> {
    events
        .lock()
        .expect("conformance events lock")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::TurnCompleted { turn_id, .. } => Some(turn_id.clone()),
            _ => None,
        })
        .collect()
}

type ConformanceBlockSignature = (TranscriptBlockKind, Option<String>, Option<String>);
type ConformanceMessageSignature = (TranscriptEntryRole, Vec<ConformanceBlockSignature>);

fn conformance_message_history_signature(
    entries: &[TranscriptEntry],
) -> Vec<ConformanceMessageSignature> {
    entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.role,
                TranscriptEntryRole::User | TranscriptEntryRole::Assistant
            )
        })
        .map(|entry| {
            (
                entry.role,
                entry
                    .blocks
                    .iter()
                    .map(|block| (block.kind, block.title.clone(), block.body.clone()))
                    .collect(),
            )
        })
        .collect()
}

fn assert_visible_conformance_history(
    runtime: AgentConformanceRuntime,
    entries: &[TranscriptEntry],
    prompt: &str,
    answer: &str,
) {
    let history = conformance_message_history_signature(entries);
    assert_eq!(
        history.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![TranscriptEntryRole::User, TranscriptEntryRole::Assistant],
        "{} Agent history must expose one user/assistant pair",
        runtime.label()
    );
    assert!(
        history[0]
            .1
            .iter()
            .any(|(_, _, body)| body.as_deref() == Some(prompt)),
        "{} Agent history must expose the submitted prompt",
        runtime.label()
    );
    assert!(
        history[1]
            .1
            .iter()
            .any(|(_, _, body)| body.as_deref() == Some(answer)),
        "{} Agent history must expose the terminal answer",
        runtime.label()
    );
}

async fn conformance_success_persists_visible_history_single_terminal_and_certain_delivery(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime);
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let events_for_sink = events.clone();
    let mut request = harness.request(harness.source("terminal", "one"), "legacy prompt");
    request.input = vec![GatewayInputPart::Text {
        text: "binding-terminal".to_string(),
    }];
    request.event_sink = Some(Arc::new(move |event| {
        events_for_sink
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));

    let result = harness
        .inner
        .gateway
        .send_turn(request)
        .await
        .unwrap_or_else(|error| panic!("{} conformance turn: {error}", runtime.label()));

    harness.assert_binding_was_visible_before_prompt();
    assert_eq!(result.turn.status, GatewayTurnStatus::Completed);
    assert_eq!(completion_turn_ids(&events), vec![result.turn.id.clone()]);
    harness.assert_exactly_one_terminal(
        &result.thread.id,
        &result.turn.id,
        "completed",
        Some("normal"),
    );
    harness.assert_certain_terminal_delivery(&result.turn.id);
    assert_visible_conformance_history(
        runtime,
        &result.committed_entries,
        "binding-terminal",
        &result.result.final_answer,
    );
    let committed_signature = conformance_message_history_signature(&result.committed_entries);

    let visible_history = harness
        .inner
        .gateway
        .thread_transcript(&result.thread.id)
        .expect("visible conformance history");
    assert_visible_conformance_history(
        runtime,
        &visible_history,
        "binding-terminal",
        &result.result.final_answer,
    );
    let visible_signature = conformance_message_history_signature(&visible_history);
    assert_eq!(visible_signature, committed_signature);
    let db_path = harness.inner.state.db_path().to_path_buf();

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown binding conformance harness");

    let reopened_state = StateRuntime::open(db_path).expect("reopen conformance state");
    let reopened_gateway = Gateway::with_backend(reopened_state, harness.backend.clone());
    let persisted_history = reopened_gateway
        .thread_transcript(&result.thread.id)
        .expect("persisted conformance history");
    assert_eq!(
        conformance_message_history_signature(&persisted_history),
        visible_signature,
        "{} history must survive a fresh StateRuntime/Gateway reader",
        runtime.label()
    );
}

async fn conformance_per_thread_ordering_and_cross_thread_concurrency(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime);
    let same_source = harness.source("ordering", "same");
    let same_hold = harness.prepare_hold("same");
    let first_gateway = harness.inner.gateway.clone();
    let first_request = harness.request(same_source.clone(), "hold:same");
    let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
    harness.wait_for_hold(&same_hold, "hold:same").await;

    let second_gateway = harness.inner.gateway.clone();
    let second_request = harness.request(same_source.clone(), "second:same");
    let second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });
    wait_for_agent_conformance_condition("same-thread turn to enter Gateway queue", || {
        harness
            .inner
            .gateway
            .activity_for_selector(GatewayThreadSelector::source(same_source.source_key()))
            .queued_turns
            == 1
    })
    .await;
    assert_eq!(
        harness.observed_prompts(),
        vec!["hold:same".to_string()],
        "{} Agent must not deliver the queued same-thread prompt early",
        runtime.label()
    );
    harness.release(same_hold);
    let first = first
        .await
        .expect("first same-thread task")
        .expect("first same-thread turn");
    let second = second
        .await
        .expect("second same-thread task")
        .expect("second same-thread turn");
    assert_eq!(first.thread.id, second.thread.id);
    assert_eq!(
        harness.observed_prompts(),
        vec!["hold:same".to_string(), "second:same".to_string()]
    );
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown ordering harness");

    let harness = AgentConformanceHarness::new(runtime);
    let concurrent_hold = harness.prepare_hold("concurrent");
    let first_gateway = harness.inner.gateway.clone();
    let first_request = harness.request(harness.source("concurrent", "one"), "hold:concurrent");
    let first = tokio::spawn(async move { first_gateway.send_turn(first_request).await });
    harness
        .wait_for_hold(&concurrent_hold, "hold:concurrent")
        .await;

    let second_gateway = harness.inner.gateway.clone();
    let second_request = harness.request(harness.source("concurrent", "two"), "second:concurrent");
    let mut second = tokio::spawn(async move { second_gateway.send_turn(second_request).await });
    let second = tokio::time::timeout(Duration::from_secs(5), &mut second)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{} Agent serialized a different thread behind the held thread",
                runtime.label()
            )
        })
        .expect("concurrent second task")
        .expect("concurrent second turn");
    assert!(
        harness
            .observed_prompts()
            .iter()
            .any(|prompt| prompt == "second:concurrent")
    );
    harness.release(concurrent_hold);
    let first = first
        .await
        .expect("concurrent first task")
        .expect("concurrent first turn");
    assert_ne!(first.thread.id, second.thread.id);
    if runtime == AgentConformanceRuntime::Acp {
        assert_eq!(
            harness
                .records()
                .iter()
                .filter(|record| record["event"] == "initialize")
                .count(),
            1,
            "different ACP sessions must multiplex on one resident generation"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown concurrency harness");
}

async fn conformance_rejects_unsupported_control_and_structured_input(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime);
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));

    let mut control_request = harness.request(
        harness.source("reject", "control"),
        "must-not-reach-agent-control",
    );
    control_request
        .options
        .runtime_options
        .insert("conformance-unsupported".to_string(), "value".to_string());
    let control_events = events.clone();
    control_request.event_sink = Some(Arc::new(move |event| {
        control_events
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let control_error = harness
        .inner
        .gateway
        .send_turn(control_request)
        .await
        .expect_err("unsupported control must be rejected");
    assert!(
        control_error
            .to_string()
            .contains("conformance-unsupported"),
        "{control_error}"
    );
    assert!(harness.observed_prompts().is_empty());
    assert_eq!(completion_turn_ids(&events).len(), 1);

    let mut resource_request = harness.request(
        harness.source("reject", "resource"),
        "must-not-reach-agent-resource",
    );
    resource_request.input = vec![GatewayInputPart::Resource {
        uri: "psychevo://conformance/resource".to_string(),
        mime_type: Some("text/plain".to_string()),
        text: Some("structured resource".to_string()),
        blob: None,
    }];
    let resource_events = events.clone();
    resource_request.event_sink = Some(Arc::new(move |event| {
        resource_events
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let resource_error = harness
        .inner
        .gateway
        .send_turn(resource_request)
        .await
        .expect_err("unsupported structured input must be rejected");
    assert!(
        resource_error.to_string().contains("resource")
            || resource_error.to_string().contains("embedded-context"),
        "{resource_error}"
    );
    assert!(harness.observed_prompts().is_empty());
    assert_eq!(completion_turn_ids(&events).len(), 2);

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown rejection harness");
}

async fn conformance_cancel_produces_one_interrupted_terminal(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime);
    let source = harness.source("cancel", "one");
    let hold = harness.prepare_hold("cancel");
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let events_for_sink = events.clone();
    let (handle, control) = run_control();
    let mut request = harness.request(source, "hold:cancel");
    request.control_handle = Some(handle.clone());
    request.control = Some(control);
    request.event_sink = Some(Arc::new(move |event| {
        events_for_sink
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let gateway = harness.inner.gateway.clone();
    let turn = tokio::spawn(async move { gateway.send_turn(request).await });
    harness.wait_for_hold(&hold, "hold:cancel").await;
    handle.abort();

    let result = tokio::time::timeout(Duration::from_secs(5), turn)
        .await
        .unwrap_or_else(|_| panic!("{} Agent ignored cancel", runtime.label()))
        .expect("cancel task")
        .expect("cancel result");
    assert_eq!(result.result.outcome, Outcome::Aborted);
    assert_eq!(result.turn.status, GatewayTurnStatus::Interrupted);
    assert_eq!(completion_turn_ids(&events), vec![result.turn.id.clone()]);
    harness.assert_exactly_one_terminal(
        &result.thread.id,
        &result.turn.id,
        "interrupted",
        Some("aborted"),
    );
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "cancel"),
            "ACP cancellation must send session/cancel"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown cancel harness");
}

async fn conformance_permission_interaction_is_accepted_once(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime);
    if runtime == AgentConformanceRuntime::Native {
        harness.backend.request_permission();
    }
    let source = harness.source("interaction", "one");
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut request = harness.request(source.clone(), "permission");
    request.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));
    let gateway = harness.inner.gateway.clone();
    let turn = tokio::spawn(async move { gateway.send_turn(request).await });

    let action_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(GatewayEvent::ActionRequested { action }) = event_rx.recv().await
                && action.kind == GatewayActionKind::Permission
            {
                break action.action_id;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{} Agent did not request permission", runtime.label()));
    assert_eq!(action_id, "permission-1");

    let concurrent_source = harness.source("interaction", "concurrent");
    let concurrent_gateway = harness.inner.gateway.clone();
    let concurrent_request = harness.request(concurrent_source, "interaction-concurrent");
    let mut concurrent =
        tokio::spawn(async move { concurrent_gateway.send_turn(concurrent_request).await });
    let concurrent_result = tokio::time::timeout(Duration::from_secs(5), &mut concurrent).await;
    if concurrent_result.is_err() {
        let _ = harness.inner.gateway.submit_permission(
            GatewayThreadSelector::source(source.source_key()),
            &action_id,
            PermissionApprovalDecision::deny(),
        );
        let _ = turn.await;
        panic!(
            "{} pending interaction blocked an independent thread",
            runtime.label()
        );
    }
    concurrent_result
        .expect("checked interaction concurrency timeout")
        .expect("interaction concurrency task")
        .expect("interaction concurrency turn");

    let selector = GatewayThreadSelector::source(source.source_key());
    assert!(harness.inner.gateway.submit_permission(
        selector.clone(),
        &action_id,
        PermissionApprovalDecision::allow_once(),
    ));
    assert!(
        !harness.inner.gateway.submit_permission(
            selector,
            &action_id,
            PermissionApprovalDecision::allow_once(),
        ),
        "{} interaction token must be consumed exactly once",
        runtime.label()
    );
    turn.await
        .expect("permission turn task")
        .expect("permission turn");

    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(GatewayEvent::ActionResolved {
                kind: GatewayActionKind::Permission,
                outcome,
                ..
            }) = event_rx.recv().await
            {
                break outcome;
            }
        }
    })
    .await
    .expect("permission resolved event");
    assert_eq!(outcome, GatewayActionOutcome::Accepted);
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "permission_response"),
            "ACP Agent must receive the accepted interaction response"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown interaction harness");
}

async fn conformance_shutdown_is_graceful(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime);
    let source = harness.source("shutdown", "one");
    harness
        .inner
        .gateway
        .send_turn(harness.request(source.clone(), "shutdown"))
        .await
        .expect("turn before shutdown");
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .unwrap_or_else(|error| panic!("{} graceful shutdown: {error}", runtime.label()));
    assert!(
        !harness
            .inner
            .gateway
            .activity_for_selector(GatewayThreadSelector::source(source.source_key()))
            .running
    );
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "close"),
            "graceful ACP shutdown must close resident sessions"
        );
    }
}

async fn conformance_shared_agent_session_transact_seam(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime);
    let source = harness.source("transact", "one");
    let turn = harness
        .inner
        .gateway
        .send_turn(harness.request(source.clone(), "transact-run"))
        .await
        .unwrap_or_else(|error| panic!("{} transact run: {error}", runtime.label()));
    assert_eq!(harness.observed_prompts(), vec!["transact-run".to_string()]);

    let binding = harness
        .inner
        .state
        .store()
        .gateway_runtime_binding(&turn.thread.id)
        .expect("transact binding lookup")
        .expect("transact binding");
    let mut options = harness.request(source, "unused").options;
    options.session = Some(turn.thread.id.clone());
    let (profile, _, _) = resolve_gateway_runtime_profile(&options).expect("captured profile");
    let peer = if runtime == AgentConformanceRuntime::Acp {
        let mut peer_options = options.clone();
        peer_options.runtime_ref = profile.backend_ref.clone();
        Some(
            resolve_peer_turn(&peer_options)
                .expect("transact ACP peer resolution")
                .expect("transact ACP peer is available"),
        )
    } else {
        None
    };
    let mcp_servers = peer
        .as_ref()
        .map(|peer| acp_peer::resolve_peer_mcp_server_handoffs(peer, &options))
        .transpose()
        .expect("transact MCP handoff")
        .unwrap_or_default();
    let target = CapturedAgentSessionTarget::bound(&binding, profile, peer)
        .expect("capture bound Agent session target");
    let session = AgentSessionRef {
        cwd: options.cwd,
        local_session_id: turn.thread.id.clone(),
        native_session_id: binding
            .native_session_id
            .clone()
            .expect("transact native session id"),
        mcp_servers,
    };

    let attached = harness
        .inner
        .gateway
        .agent_sessions
        .attach(target)
        .expect("attach shared Agent session target");
    let inspected = attached
        .transact(AgentSessionCommand::Inspect(session.clone()))
        .await
        .expect("typed inspection response")
        .into_inspection()
        .expect("inspection response kind");
    match (runtime, inspected) {
        (AgentConformanceRuntime::Native, AgentSessionSnapshot::Native { profile_id }) => {
            assert_eq!(profile_id, "native");
            let error = attached
                .transact(AgentSessionCommand::SetControl {
                    session,
                    control_id: "fast".to_string(),
                    value: Value::Bool(true),
                })
                .await
                .expect_err("Native live session control must fail closed");
            assert_eq!(
                error
                    .structured_data()
                    .and_then(|data| data["code"].as_str()),
                Some("unsupported_control")
            );
        }
        (AgentConformanceRuntime::Acp, AgentSessionSnapshot::Acp(snapshot)) => {
            assert!(snapshot.options.iter().any(|option| {
                option.id == "fast" && option.current_value.as_deref() == Some("false")
            }));
            let controlled = attached
                .transact(AgentSessionCommand::SetControl {
                    session,
                    control_id: "fast".to_string(),
                    value: Value::Bool(true),
                })
                .await
                .expect("typed ACP control response")
                .into_control()
                .expect("control response kind");
            let AgentSessionSnapshot::Acp(snapshot) = controlled else {
                panic!("ACP control returned a non-ACP snapshot");
            };
            assert!(snapshot.options.iter().any(|option| {
                option.id == "fast" && option.current_value.as_deref() == Some("true")
            }));
            assert!(harness.records().iter().any(|record| {
                record["event"] == "set_control"
                    && record["controlId"] == "fast"
                    && record["value"] == true
            }));
        }
        (runtime, _) => panic!(
            "{} inspection returned the wrong snapshot kind",
            runtime.label()
        ),
    }

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown transact conformance harness");
}

async fn run_agent_conformance(runtime: AgentConformanceRuntime) {
    conformance_success_persists_visible_history_single_terminal_and_certain_delivery(runtime)
        .await;
    conformance_per_thread_ordering_and_cross_thread_concurrency(runtime).await;
    conformance_rejects_unsupported_control_and_structured_input(runtime).await;
    conformance_cancel_produces_one_interrupted_terminal(runtime).await;
    conformance_permission_interaction_is_accepted_once(runtime).await;
    conformance_shutdown_is_graceful(runtime).await;
    if let Some(AgentConformanceProcessExitDelivery::Unknown) = runtime.process_exit_delivery() {
        conformance_process_exit_preserves_unknown_delivery_without_retry(runtime).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_conformance_native() {
    run_agent_conformance(AgentConformanceRuntime::Native).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_conformance_acp() {
    run_agent_conformance(AgentConformanceRuntime::Acp).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_session_transact_conformance_native() {
    conformance_shared_agent_session_transact_seam(AgentConformanceRuntime::Native).await;
}

#[tokio::test]
async fn agent_session_attach_is_idempotent_for_captured_binding_and_rejects_conflict() {
    let harness = AgentConformanceHarness::new(AgentConformanceRuntime::Native);
    let source = harness.source("attachment-identity", "one");
    let turn = harness
        .inner
        .gateway
        .send_turn(harness.request(source.clone(), "capture binding"))
        .await
        .expect("initial Native turn");
    let binding = harness
        .inner
        .state
        .store()
        .gateway_runtime_binding(&turn.thread.id)
        .expect("binding lookup")
        .expect("captured binding");
    let mut options = harness.request(source, "unused").options;
    options.session = Some(turn.thread.id);
    let (profile, _, _) = resolve_gateway_runtime_profile(&options).expect("Native profile");

    let attachment_count = harness
        .inner
        .gateway
        .agent_sessions
        .bound_attachments
        .lock()
        .expect("attachment registry")
        .len();
    harness
        .inner
        .gateway
        .agent_sessions
        .attach(
            CapturedAgentSessionTarget::bound(&binding, profile.clone(), None)
                .expect("same captured target"),
        )
        .expect("same captured target reattaches idempotently");
    assert_eq!(
        harness
            .inner
            .gateway
            .agent_sessions
            .bound_attachments
            .lock()
            .expect("attachment registry")
            .len(),
        attachment_count,
        "idempotent attach must not create another ordering owner"
    );

    let mut conflicting_binding = binding;
    conflicting_binding.agent_fingerprint = Some("different-agent-fingerprint".to_string());
    let error = harness
        .inner
        .gateway
        .agent_sessions
        .attach(
            CapturedAgentSessionTarget::bound(&conflicting_binding, profile, None)
                .expect("construct conflicting captured target"),
        )
        .err()
        .expect("same thread/revision with another capture must fail");
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["code"].as_str()),
        Some("agent_session_attachment_conflict")
    );
}

#[tokio::test]
async fn thread_application_run_turn_lowers_typed_caller_intent() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone());
    let source = GatewaySource::new("application-conformance", "typed-turn").invocation();
    let mut request = ThreadTurnRequest::new(
        harness.cwd.clone(),
        vec![GatewayInputPart::Text {
            text: "typed application input".to_string(),
        }],
    );
    request.source = Some(source);
    request.policy.runtime_profile_ref = Some("native".to_string());
    request.policy.model = Some("surface-model".to_string());
    request.policy.reasoning_effort = Some("high".to_string());
    request.policy.mode = RunMode::Plan;
    request.policy.permission_mode = Some(PermissionMode::DontAsk);
    request.runtime_source = Some("application-conformance".to_string());

    harness
        .gateway
        .run_turn(request)
        .await
        .expect("typed Thread Application turn");
    let runs = backend.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].prompt, "typed application input");
    assert_eq!(runs[0].model.as_deref(), Some("surface-model"));
    assert_eq!(runs[0].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(runs[0].mode, RunMode::Plan);
    assert_eq!(runs[0].permission_mode, Some(PermissionMode::DontAsk));
    assert!(
        harness
            .gateway
            .state()
            .store()
            .gateway_runtime_binding(runs[0].session.as_deref().expect("public Thread session"))
            .expect("binding lookup")
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_session_transact_conformance_acp() {
    conformance_shared_agent_session_transact_seam(AgentConformanceRuntime::Acp).await;
}

#[test]
fn agent_session_response_kind_mismatch_fails_closed() {
    let error = AgentSessionResponse::Inspected(AgentSessionSnapshot::Native {
        profile_id: "native".to_string(),
    })
    .into_turn()
    .expect_err("an inspection response cannot satisfy a turn command");
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["code"].as_str()),
        Some("agent_session_response_mismatch")
    );
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["delivery"].as_str()),
        Some("unknown")
    );
}

async fn conformance_process_exit_preserves_unknown_delivery_without_retry(
    runtime: AgentConformanceRuntime,
) {
    assert_eq!(
        runtime.process_exit_delivery(),
        Some(AgentConformanceProcessExitDelivery::Unknown)
    );
    let harness = AgentConformanceHarness::new(runtime);
    let mut request = harness.request(harness.source("unknown-delivery", "one"), "legacy prompt");
    request.input = vec![GatewayInputPart::Text {
        text: "crash-on-prompt".to_string(),
    }];
    let turn_id = "agent-conformance-unknown-delivery";
    let error = harness
        .inner
        .gateway
        .run_turn_now(
            "thread:agent-conformance-unknown",
            request,
            turn_id.to_string(),
        )
        .await
        .expect_err("connection loss after prompt acceptance must remain unknown");
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["delivery"].as_str()),
        Some("unknown"),
        "{error}"
    );
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["retryClass"].as_str()),
        Some("unknown_delivery"),
        "{error}"
    );
    let delivery = harness
        .inner
        .state
        .store()
        .gateway_turn_delivery(turn_id)
        .expect("unknown delivery lookup")
        .expect("unknown delivery record");
    assert_eq!(delivery.status, "unknown");
    assert!(delivery.input_json.is_some());
    assert_eq!(delivery.delivery_confirmed_at_ms, None);
    assert_eq!(delivery.terminal_at_ms, None);
    harness.assert_exactly_one_terminal(&delivery.thread_id, turn_id, "failed", None);
    assert_eq!(
        harness
            .records()
            .iter()
            .filter(|record| record["event"] == "prompt")
            .count(),
        1,
        "unknown delivery must not automatically retry the prompt"
    );
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown process-exit conformance harness");
}
