#[tokio::test]
async fn native_turn_delivery_ledger_scrubs_confirmed_input_at_terminal() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend).await;
    let input = vec![GatewayInputPart::Text {
        text: "private native prompt".to_string(),
    }];
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "native-delivery-ledger").persistent(),
        "legacy prompt is replaced by structured input",
    );
    turn_request.input = input;

    let result = harness.send(turn_request).await.expect("native turn");
    let delivery = harness
        .state

        .gateway_turn_delivery(&result.turn.id)
        .await.expect("delivery lookup")
        .expect("delivery record");
    assert_eq!(delivery.status, "terminal");
    assert_eq!(delivery.runtime_ref, "native");
    assert_eq!(delivery.input_hash.len(), 64);
    assert_eq!(delivery.input_json, None);
    assert!(delivery.delivery_confirmed_at_ms.is_some());
    assert!(delivery.terminal_at_ms.is_some());
}

#[tokio::test]
async fn public_turn_terminal_observes_completed_thread_activity() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend).await;
    let observed_status = Arc::new(Mutex::new(None));
    let status_for_event = Arc::clone(&observed_status);
    let gateway_for_event = harness.gateway.clone();
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "terminal-activity-order").persistent(),
        "finish activity before terminal",
    );
    turn_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        if let GatewayEvent::TurnCompleted {
            thread_id: Some(thread_id),
            ..
        } = event
        {
            let activity = gateway_for_event.local_activity_for_selector(
                &GatewayThreadSelector::thread_id(thread_id),
            );
            *status_for_event.lock().expect("terminal status lock") =
                Some((activity.running, activity.active_turn_id));
        }
    }));

    harness.send(turn_request).await.expect("native turn");

    assert_eq!(
        observed_status.lock().expect("observed status").as_ref(),
        Some(&(false, None))
    );
}

#[tokio::test]
async fn delegated_acp_child_owns_activity_turn_identity_and_terminal_order() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = harness.cwd.join("delegated-child-activity.jsonl");
    let release = harness.cwd.join("delegated-child.release");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
command = {}
args = [{}]
entrypoints = ["subagent"]

[agents.backends.fake.env]
ACP_LIFECYCLE_LOG = {}
ACP_LIFECYCLE_MODE = "blocking-prompt"
ACP_LIFECYCLE_RELEASE = {}
"#,
            test_python_command_toml(&harness.cwd),
            serde_json::to_string(&fixture.to_string_lossy()).expect("fixture path"),
            serde_json::to_string(&log.to_string_lossy()).expect("log path"),
            serde_json::to_string(&release.to_string_lossy()).expect("release path"),
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo/agents");
    std::fs::create_dir_all(&agents_dir).expect("agents");
    std::fs::write(
        agents_dir.join("opencode.md"),
        r#"---
name: opencode
description: Delegated ACP child.
backend:
  ref: fake
entrypoints: [subagent]
---
Use the captured child session.
"#,
    )
    .expect("Agent Definition");

    let parent_thread_id = harness
        .state

        .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
        .await.expect("parent Thread");
    let child_thread_id = harness
        .state

        .create_child_session_with_metadata(
            &parent_thread_id,
            &harness.cwd,
            "peer_agent",
            "opencode",
            "acp:fake",
            None,
        )
        .await.expect("child Thread");
    harness
        .state

        .upsert_agent_edge(
            &parent_thread_id,
            &child_thread_id,
            psychevo::__product::persistence::AgentEdgeStatus::Open,
            None,
        )
        .await.expect("open child edge");
    let parent_activity = harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: "turn-parent",
            thread_id: Some(&parent_thread_id),
            source_key: None,
            turn_id: Some("turn-parent"),
            kind: "turn",
            owner_surface: Some("web"),
            queued_turns: 0,
            intent: None,
        })
        .await.expect("parent activity");

    let projected = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let projected_for_stream = Arc::clone(&projected);
    let projector = Arc::new(Mutex::new(GatewayLiveProjector::new(Some(
        parent_thread_id.clone(),
    ))));
    let projector_for_stream = Arc::clone(&projector);
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(event) = projector_for_stream
            .lock()
            .expect("projector")
            .project("turn-parent", &event)
        {
            projected_for_stream.lock().expect("events").push(event);
        }
    });
    let mut options = run_options(&harness, "delegated child");
    options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]));
    let delegate = GatewayExternalAgentDelegate {
        gateway: harness.gateway.clone(),
        base_options: options,
        stream: Some(stream),
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let child_turn_id = "turn-child".to_string();
    let mut running = tokio::spawn(delegate.run_inner(ExternalAgentDelegateRequest {
        run_id: child_turn_id.clone(),
        parent_session_id: parent_thread_id.clone(),
        child_session_id: child_thread_id.clone(),
        agent_name: "opencode".to_string(),
        agent_description: "Delegated ACP child.".to_string(),
        runtime_ref: "acp:fake".to_string(),
        backend_ref: Some("fake".to_string()),
        instructions: Some("Use the captured child session.".to_string()),
        prompt: "list tools".to_string(),
        task_name: "delegated-child".to_string(),
        model: None,
        runtime_options: BTreeMap::new(),
        expected_runtime_profile_revision: None,
        abort: AbortSignal::new(abort_rx),
    }));

    tokio::select! {
        early = &mut running => {
            panic!("delegated child finished before blocking prompt: {early:?}");
        }
        started = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if std::fs::read_to_string(&log)
                    .ok()
                    .is_some_and(|contents| contents.contains("prompt_blocked"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }) => {
            started.expect("child prompt started");
        }
    }

    let parent = harness
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread_id))
        .await;
    let child = harness
        ._application
        .client()
        .resume_thread(&child_thread_id)
        .await
        .expect("child Framework Thread");
    let (child_running, child_active_turn_id, child_queued_turns) = child.__activity();
    assert!(child_running);
    assert_eq!(
        child_active_turn_id.as_deref(),
        Some(child_turn_id.as_str())
    );
    assert_eq!(child_queued_turns, 0);
    let gateway_child = harness
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&child_thread_id))
        .await;
    assert!(parent.running);
    assert!(
        !gateway_child.running,
        "delegated child must not retain a Gateway shadow activity"
    );

    std::fs::write(&release, "release").expect("release child");
    let result = running
        .await
        .expect("delegated task")
        .expect("delegated result");
    assert_eq!(result.child_session_id, child_thread_id);

    let child_entries = projected
        .lock()
        .expect("projected events")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::EntryStarted { turn_id, entry }
            | GatewayEvent::EntryUpdated { turn_id, entry }
            | GatewayEvent::EntryCompleted { turn_id, entry }
                if entry.thread_id == child_thread_id => Some(turn_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!child_entries.is_empty());
    assert!(child_entries.iter().all(|turn_id| turn_id == &child_turn_id));

    let (child_running, child_active_turn_id, child_queued_turns) = child.__activity();
    assert!(!child_running);
    assert_eq!(child_active_turn_id, None);
    assert_eq!(child_queued_turns, 0);
    let terminal_edge = harness
        .state
        .find_agent_edge(&child_thread_id)
        .await
        .expect("edge after terminal")
        .expect("child edge after terminal");
    assert_eq!(
        terminal_edge.status,
        psychevo::__product::persistence::AgentEdgeStatus::Closed
    );
    assert!(
        harness
            .gateway
            .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread_id))
            .await
            .running
    );

    harness
        .gateway
        .finish_durable_gateway_activity(Some(&parent_activity), "completed")
        .await;
    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
}

#[tokio::test]
async fn acp_peer_agent_turn_routes_to_backend_and_persists_native_session() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone()).await;
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp.py");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_acp_session_persistence.py");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::copy(fixture, &script).expect("fake acp script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = {}
args = ["{}"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]

[agents.backends.fake.env]
PSYCHEVO_BINDING_DB = "{}"
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            harness.state.db_path().display(),
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Review with fake ACP.
backend:
  ref: fake
entrypoints: [peer]
tools: [read]
---
Peer instructions.
"#,
    )
    .expect("agent file");

    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let source = GatewaySource::new("web", "peer").persistent();
    let mut first_request = request(&harness, source.clone(), "hello");
    first_request.options.agent = Some("reviewer".to_string());
    first_request.options.runtime_ref = Some("acp:fake".to_string());
    first_request.options.inherited_env = Some(env.clone());
    let first = harness.send(first_request).await.expect("first peer turn");

    assert_eq!(
        first
            .result
            .selected_agent
            .as_ref()
            .map(|agent| agent.name.as_str()),
        Some("reviewer")
    );
    assert!(first.result.final_answer.contains("new:native-1"));
    assert!(first.result.final_answer.contains("Peer instructions."));
    assert!(first.result.final_answer.contains("hello"));

    let binding = harness
        .state

        .gateway_source_binding(&source.source_key().0)
        .await.expect("binding lookup")
        .expect("binding");
    assert_eq!(binding.backend_kind, "unresolved");
    assert_eq!(binding.backend_native_id, None);
    let runtime_binding = harness
        .state

        .gateway_runtime_binding(&first.result.session_id)
        .await.expect("runtime binding lookup")
        .expect("runtime binding");
    assert_eq!(runtime_binding.backend_kind.as_deref(), Some("acp"));
    assert_eq!(
        runtime_binding.native_session_id.as_deref(),
        Some("native-1")
    );
    let delivery = harness
        .state

        .gateway_turn_delivery(&first.turn.id)
        .await.expect("ACP delivery lookup")
        .expect("ACP delivery record");
    assert_eq!(delivery.status, "terminal");
    assert_eq!(delivery.runtime_ref, "acp:fake");
    assert_eq!(delivery.input_json, None);
    assert!(delivery.delivery_confirmed_at_ms.is_some());
    let metadata = harness
        .state

        .session_metadata(&first.result.session_id)
        .await.expect("metadata")
        .expect("metadata value");
    assert_eq!(metadata["peer_agent"]["nativeSessionId"], "native-1");
    let transcript = harness
        .gateway
        .thread_transcript(&first.result.session_id)
        .await.expect("transcript");
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].role, TranscriptEntryRole::User);
    assert_eq!(transcript[1].role, TranscriptEntryRole::Assistant);
    let summary = harness
        .state

        .session_summary(&first.result.session_id)
        .await.expect("session summary")
        .expect("summary");
    assert_eq!(summary.title.as_deref(), Some("hello"));

    let mut second_request = request(&harness, source.clone(), "again");
    second_request.options.agent = Some("reviewer".to_string());
    second_request.options.runtime_ref = Some("acp:fake".to_string());
    second_request.options.inherited_env = Some(env.clone());
    let second = harness.send(second_request).await.expect("second peer turn");
    assert_eq!(second.result.session_id, first.result.session_id);
    assert!(second.result.final_answer.contains("new:native-1"));
    assert!(
        !second
            .result
            .final_answer
            .contains("Peer instructions."),
        "captured Agent instructions are sent once per logical Thread"
    );
    assert!(
        !second
            .result
            .final_answer
            .contains("old answer from loaded history")
    );
    assert_eq!(
        std::fs::read_to_string(script.with_extension("py.processes"))
            .expect("ACP process counter"),
        "1",
        "two turns on one thread must reuse one resident ACP process"
    );

    let child_session = harness
        .state

        .create_child_session_with_metadata(
            &first.result.session_id,
            &harness.cwd,
            "peer_agent",
            "reviewer",
            "acp:fake",
            None,
        )
        .await.expect("child peer session");
    let mut child_request = request(&harness, source, "child prompt");
    child_request.thread_id = Some(child_session.clone());
    child_request.explicit_thread = true;
    child_request.options.agent = Some("reviewer".to_string());
    child_request.options.runtime_ref = Some("acp:fake".to_string());
    child_request.options.inherited_env = Some(env);
    let child = harness.send(child_request).await.expect("child peer turn");
    assert_eq!(child.result.session_id, child_session);
    let child_summary = harness
        .state

        .session_summary(&child.result.session_id)
        .await.expect("child summary")
        .expect("child");
    assert_eq!(child_summary.title, None);
}
#[tokio::test]
async fn non_peer_turn_clears_acp_peer_usage_projection_without_losing_native_session() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone()).await;
    let session_id = harness
        .state

        .create_session_with_metadata(
            &harness.cwd,
            "peer_agent",
            "opencode",
            "acp:opencode",
            Some(json!({
                "peer_agent": {
                    "agentName": "opencode",
                    "backendId": "opencode",
                    "backendKind": "acp",
                    "nativeSessionId": "native-1",
                    "usageUpdate": {
                        "sessionUpdate": "usage_update",
                        "used": 12_400,
                        "size": 200_000
                    }
                }
            })),
        )
        .await.expect("session");
    let mut request = request(
        &harness,
        GatewaySource::new("web", "default-after-peer").persistent(),
        "continue with default",
    );
    request.thread_id = Some(session_id.clone());
    request.explicit_thread = true;

    let result = harness.send(request).await.expect("turn");

    assert_eq!(result.result.session_id, session_id);
    assert_eq!(
        backend.runs()[0].session.as_deref(),
        Some(session_id.as_str())
    );
    let metadata = harness
        .state

        .session_metadata(&session_id)
        .await.expect("metadata")
        .expect("metadata value");
    let peer = metadata
        .get("peer_agent")
        .and_then(Value::as_object)
        .expect("peer metadata");
    assert_eq!(
        peer.get("nativeSessionId").and_then(Value::as_str),
        Some("native-1")
    );
    assert!(!peer.contains_key("usageUpdate"));
}
#[tokio::test]
async fn acp_peer_agent_streams_standard_session_updates_to_gateway_events() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone()).await;
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp_stream.py");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_acp_stream_updates.py");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::copy(fixture, &script).expect("fake acp stream script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = {}
args = ["{}"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display()
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Review with fake ACP.
backend:
  ref: fake
entrypoints: [peer]
tools: [read]
---
"#,
    )
    .expect("agent file");

    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let gateway_events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let gateway_events_for_sink = Arc::clone(&gateway_events);
    let raw_events = Arc::new(Mutex::new(Vec::<RunStreamEvent>::new()));
    let raw_events_for_sink = Arc::clone(&raw_events);
    let source = GatewaySource::new("web", "peer-stream").persistent();
    let mut first_request = request(&harness, source, "hello");
    first_request.options.agent = Some("reviewer".to_string());
    first_request.options.runtime_ref = Some("acp:fake".to_string());
    first_request.options.inherited_env = Some(env.clone());
    first_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        gateway_events_for_sink
            .lock()
            .expect("gateway events lock")
            .push(event);
    }));
    first_request.stream = Some(Arc::new(move |event| {
        raw_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let result = harness
        .send(first_request)
        .await
        .expect("streaming peer turn");

    assert_eq!(result.result.final_answer, "hello world");
    let raw_event_values = raw_events
        .lock()
        .expect("raw events lock")
        .iter()
        .filter_map(|event| event.legacy_value().cloned())
        .collect::<Vec<_>>();
    assert!(
        raw_event_values
            .iter()
            .any(|event| event["update_kind"] == "available_commands_update"),
        "available commands update should be retained as a structured ACP event"
    );
    assert!(
        raw_event_values
            .iter()
            .any(|event| event["update_kind"] == "session_info_update"),
        "session info update should be retained as a structured ACP event"
    );

    {
        let raw_events = raw_events.lock().expect("raw events lock");
        assert!(
            raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_session_update"
                        && value["update_kind"] == "tool_call_update"
            )),
            "raw stream should retain ACP tool updates"
        );
    }

    let live_plans = {
        let gateway_events = gateway_events.lock().expect("gateway events lock");
        let blocks = gateway_events
            .iter()
            .filter_map(|event| match event {
                GatewayEvent::EntryStarted { entry, .. }
                | GatewayEvent::EntryUpdated { entry, .. }
                | GatewayEvent::EntryCompleted { entry, .. } => Some(entry.blocks.as_slice()),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Reasoning
                    && block.body.as_deref() == Some("think first")
            }),
            "thought chunks should render as a live Thinking block"
        );
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Text
                    && block.body.as_deref() == Some("hello world")
            }),
            "message chunks should render as incremental assistant text"
        );
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Shell
                    && block.title.as_deref() == Some("Run echo")
                    && block.status == TranscriptBlockStatus::Completed
                    && block
                        .body
                        .as_deref()
                        .is_some_and(|body| body.contains("done"))
            }),
            "ACP tool updates should render as a completed live tool block"
        );
        blocks
            .iter()
            .filter(|block| {
                block.kind == TranscriptBlockKind::Status
                    && block.title.as_deref() == Some("Plan")
            })
            .map(|block| (*block).clone())
            .collect::<Vec<_>>()
    };
    assert!(live_plans.len() >= 2, "each ACP plan update should be observable");
    let live_plan = live_plans.last().expect("latest live plan");
    assert!(
        live_plans.iter().all(|plan| plan.id == live_plan.id),
        "replacement plan updates must retain one logical block identity"
    );
    assert_eq!(live_plan.id, format!("turn:{}:acp-peer-plan", result.turn.id));
    assert!(
        live_plan
            .body
            .as_deref()
            .is_some_and(|body| body.contains("Verify terminal history") && !body.contains("Inspect repo")),
        "the latest ACP plan must replace the prior value"
    );
    assert_eq!(
        live_plan.metadata.as_ref().unwrap()["plan"]["entries"][0]["content"],
        "Persist replacement plan"
    );
    let committed_plan = result
        .committed_entries
        .iter()
        .flat_map(|entry| entry.blocks.iter())
        .find(|block| block.title.as_deref() == Some("Plan"))
        .expect("terminal committed plan");
    assert_eq!(committed_plan.id, live_plan.id);
    assert_eq!(committed_plan.status, TranscriptBlockStatus::Completed);
    assert_eq!(committed_plan.body, live_plan.body);
    assert_eq!(committed_plan.metadata, live_plan.metadata);

    let summary = harness
        .state

        .session_summary(&result.result.session_id)
        .await.expect("session summary")
        .expect("summary");
    assert_eq!(summary.title.as_deref(), Some("ACP streamed title"));
    let transcript = harness
        .gateway
        .thread_transcript(&result.result.session_id)
        .await.expect("transcript");
    let persisted_blocks = transcript
        .iter()
        .flat_map(|entry| entry.blocks.iter())
        .collect::<Vec<_>>();
    assert!(
        persisted_blocks.iter().any(|block| {
            block.kind == TranscriptBlockKind::Reasoning
                && block.body.as_deref() == Some("think first")
        }),
        "completed ACP reasoning should persist for reload"
    );
    assert!(
        persisted_blocks.iter().any(|block| {
            block.kind == TranscriptBlockKind::Shell
                && block.title.as_deref() == Some("Run echo")
                && block.result.as_ref().is_some_and(|result| {
                    result.status == TranscriptBlockStatus::Completed
                        && result.content.contains("done")
                })
        }),
        "completed ACP tool result should persist for reload"
    );
    let history_plan = persisted_blocks
        .iter()
        .find(|block| block.title.as_deref() == Some("Plan"))
        .expect("durable history plan");
    assert_eq!(history_plan.id, committed_plan.id);
    assert_eq!(history_plan.body, committed_plan.body);
    assert_eq!(history_plan.metadata, committed_plan.metadata);

    let summaries = harness
        .state

        .load_tui_message_summaries(&result.result.session_id)
        .await.expect("stored messages");
    let stored_assistant = summaries
        .iter()
        .find(|summary| matches!(summary.message, psychevo::__agent_core::Message::Assistant { .. }))
        .expect("stored assistant message");
    assert_eq!(
        stored_assistant.usage,
        Some(json!({
            "total_tokens": 144,
            "input_tokens": 100,
            "output_tokens": 44,
            "cached_tokens": 30,
            "reasoning_tokens": 4
        }))
    );
    let usage_summary = psychevo::__product::usage::session_usage_summary(
        psychevo::__product::runtime::SessionUsageOptions {
            state: harness.state.clone(),
            session_id: result.result.session_id.clone(),
        },
    )
    .await.expect("session usage");
    assert_eq!(usage_summary.effective_total_tokens, Some(144));
    assert_eq!(usage_summary.total_status, "reported");
    let psychevo::__agent_core::Message::Assistant { content, .. } = &stored_assistant.message else {
        unreachable!("matched assistant message")
    };
    assert!(
        content.iter().all(|block| !serde_json::to_string(block)
            .expect("assistant block json")
            .contains("Verify terminal history")),
        "display-only ACP plan must not enter provider-visible assistant content"
    );
    assert_eq!(
        stored_assistant.metadata.as_ref().unwrap()["acp"]["plan"]["update"]["entries"][1]["content"],
        "Verify terminal history"
    );
    assert_eq!(
        stored_assistant.metadata.as_ref().unwrap()["acp"]["promptUsageCumulative"],
        json!({
            "total_tokens": 144,
            "input_tokens": 100,
            "output_tokens": 44,
            "cached_tokens": 30,
            "reasoning_tokens": 4
        })
    );
    assert_eq!(
        stored_assistant.metadata.as_ref().unwrap()["acp"]["usageScope"],
        "acp_session_cumulative"
    );

    let mut second_request = request(
        &harness,
        GatewaySource::new("web", "peer-stream").persistent(),
        "continue",
    );
    second_request.options.agent = Some("reviewer".to_string());
    second_request.options.runtime_ref = Some("acp:fake".to_string());
    second_request.options.inherited_env = Some(env);
    let second_result = harness
        .send(second_request)
        .await
        .expect("second streaming peer turn");
    assert_eq!(second_result.result.session_id, result.result.session_id);

    let summaries = harness
        .state

        .load_tui_message_summaries(&result.result.session_id)
        .await.expect("stored messages after second turn");
    let stored_assistants = summaries
        .iter()
        .filter(|summary| {
            matches!(
                summary.message,
                psychevo::__agent_core::Message::Assistant { .. }
            )
        })
        .collect::<Vec<_>>();
    let second_assistant = stored_assistants.last().expect("second assistant message");
    assert_eq!(
        second_assistant.usage,
        Some(json!({
            "total_tokens": 56,
            "input_tokens": 40,
            "output_tokens": 16,
            "cached_tokens": 20,
            "reasoning_tokens": 4
        }))
    );
    assert_eq!(
        second_assistant.metadata.as_ref().unwrap()["acp"]["promptUsageCumulative"],
        json!({
            "total_tokens": 200,
            "input_tokens": 140,
            "output_tokens": 60,
            "cached_tokens": 50,
            "reasoning_tokens": 8
        })
    );
    let usage_summary = psychevo::__product::usage::session_usage_summary(
        psychevo::__product::runtime::SessionUsageOptions {
            state: harness.state.clone(),
            session_id: result.result.session_id.clone(),
        },
    )
    .await.expect("session usage after cumulative ACP update");
    assert_eq!(usage_summary.effective_total_tokens, Some(200));
    assert_eq!(usage_summary.reported_total_tokens, 200);
    assert_eq!(usage_summary.total_status, "reported");
}
