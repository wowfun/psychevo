    #[tokio::test]
    async fn acp_peer_agent_streams_v2_session_updates_to_gateway_events() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let home = harness._temp.path().join("home");
        let script = harness._temp.path().join("fake_acp_v2_stream.py");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

model_value = "test/default-model"
effort_value = "low"

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update
    }})

def config_options():
    return [
        {
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": model_value,
            "options": [
                {"value": "test/default-model", "name": "Default"},
                {"value": "test/second-model", "name": "Second"}
            ]
        },
        {
            "id": "effort",
            "name": "Effort",
            "category": "thought_level",
            "type": "select",
            "currentValue": effort_value,
            "options": [
                {"value": "low", "name": "Low"},
                {"value": "high", "name": "High"}
            ]
        }
    ]

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 2, "capabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-v2", "configOptions": config_options()}})
    elif method == "session/set_config_option":
        config_id = params.get("configId") or params.get("config_id")
        value = params.get("value")
        if isinstance(value, dict):
            value = value.get("value")
        if config_id == "model":
            model_value = value
        elif config_id == "effort":
            effort_value = value
        send({"jsonrpc": "2.0", "id": mid, "result": {"configOptions": config_options()}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-v2"
        update(session_id, {"sessionUpdate": "session_info_update", "title": "ACP v2 streamed title"})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "inspect", "description": "Inspect through v2"}
        ]})
        update(session_id, {"sessionUpdate": "config_option_update", "configOptions": [{
            "id": "mode",
            "name": "Mode",
            "category": "mode",
            "type": "select",
            "currentValue": "code",
            "options": [
                {"value": "code", "name": "Code"},
                {"value": "ask", "name": "Ask"}
            ]
        }]})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "messageId": "thought-1", "content": {"type": "text", "text": "v2 think "}})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "messageId": "thought-1", "content": {"type": "text", "text": "stream"}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "messageId": "message-1", "content": {"type": "text", "text": "v2 hello "}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "messageId": "message-1", "content": {"type": "text", "text": model_value + " " + effort_value + " world"}})
        update(session_id, {"sessionUpdate": "tool_call", "toolCallId": "call-v2", "title": "Run v2 echo", "kind": "execute", "status": "pending", "rawInput": {"cmd": "echo v2"}})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-v2", "status": "in_progress", "content": [
            {"type": "content", "content": {"type": "text", "text": "running v2\n"}}
        ]})
        update(session_id, {"sessionUpdate": "plan_update", "plan": {"type": "items", "id": "plan-v2", "entries": [
            {"content": "Inspect v2 schema", "priority": "high", "status": "completed"},
            {"content": "Project v2 events", "priority": "high", "status": "in_progress"}
        ]}})
        update(session_id, {"sessionUpdate": "usage_update", "used": 42, "size": 1000, "cost": {"amount": 0.012, "currency": "USD"}})
        update(session_id, {"sessionUpdate": "_status_badge", "label": "custom"})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-v2", "status": "completed", "content": [
            {"type": "content", "content": {"type": "text", "text": "v2 done\n"}}
        ], "rawOutput": {"output": "v2 done\n"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
"#,
        )
        .expect("fake acp v2 stream script");
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP v2 agent."
command = "python3"
args = ["{}"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
                script.display()
            ),
        )
        .expect("config");
        let agents_dir = harness.workdir.join(".psychevo").join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir");
        std::fs::write(
            agents_dir.join("reviewer.md"),
            r#"---
name: reviewer
description: Review with fake ACP v2.
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
        let source = GatewaySource::new("web", "peer-v2-stream").persistent();
        let mut request = request(&harness, source, "hello");
        request.options.agent = Some("reviewer".to_string());
        request.options.model = Some("test/second-model".to_string());
        request.options.reasoning_effort = Some("high".to_string());
        request.options.inherited_env = Some(env);
        request.event_sink = Some(Arc::new(move |event| {
            gateway_events_for_sink
                .lock()
                .expect("gateway events lock")
                .push(event);
        }));
        request.stream = Some(Arc::new(move |event| {
            raw_events_for_sink
                .lock()
                .expect("raw events lock")
                .push(event);
        }));

        let result = harness
            .gateway
            .send_turn(request)
            .await
            .expect("v2 streaming peer turn");

        assert_eq!(
            result.result.final_answer,
            "v2 hello test/second-model high world"
        );
        assert!(
            result.result.events.iter().all(|event| {
                event["type"] != "acp_peer_session_update"
                    || event["protocol_version"].as_str() == Some("2")
            }),
            "v2 session updates should be tagged with protocol version 2"
        );
        assert!(
            result
                .result
                .events
                .iter()
                .any(|event| event["update_kind"] == "config_option_update"),
            "v2 config option updates should be retained"
        );
        assert!(
            result
                .result
                .events
                .iter()
                .any(|event| event["update_kind"] == "usage_update"),
            "v2 usage updates should be retained"
        );
        assert!(
            result
                .result
                .events
                .iter()
                .any(|event| event["update_kind"] == "_status_badge"),
            "future ACP session updates should be retained raw"
        );

        let raw_events = raw_events.lock().expect("raw events lock");
        assert!(
            raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_protocol_negotiated"
                        && value["protocol_version"] == "2"
            )),
            "v2-capable peers should negotiate protocol version 2"
        );
        assert!(
            !raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_protocol_fallback"
            )),
            "v2-capable peers should not fall back to v1"
        );
        assert!(
            raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_usage_update"
                        && value["usage"]["used"] == 42
            )),
            "v2 usage updates should be forwarded to the live stream"
        );
        assert!(
            raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_config_option_set"
                        && value["config_id"] == "model"
                        && value["value"] == "test/second-model"
            )),
            "Gateway should set the peer model config option before prompting"
        );
        assert!(
            raw_events.iter().any(|event| matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "acp_peer_config_option_set"
                        && value["config_id"] == "effort"
                        && value["value"] == "high"
            )),
            "Gateway should set the peer reasoning effort config option before prompting"
        );
        drop(raw_events);

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
                    && block.body.as_deref() == Some("v2 think stream")
            }),
            "v2 thought chunks should render as a live Thinking block"
        );
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Text
                    && block.body.as_deref() == Some("v2 hello test/second-model high world")
            }),
            "v2 message chunks should render as incremental assistant text"
        );
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Shell
                    && block.title.as_deref() == Some("Run v2 echo")
                    && block.status == TranscriptBlockStatus::Completed
                    && block
                        .body
                        .as_deref()
                        .is_some_and(|body| body.contains("v2 done"))
            }),
            "v2 tool updates should render as a completed live tool block"
        );
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Status
                    && block.title.as_deref() == Some("Plan")
                    && block
                        .body
                        .as_deref()
                        .is_some_and(|body| body.contains("Inspect v2 schema"))
            }),
            "v2 plan updates should render as a live plan block"
        );
        drop(gateway_events);

        let summary = harness
            .state
            .store()
            .session_summary(&result.result.session_id)
            .expect("session summary")
            .expect("summary");
        assert_eq!(summary.title.as_deref(), Some("ACP v2 streamed title"));
        let metadata = harness
            .state
            .store()
            .session_metadata(&result.result.session_id)
            .expect("session metadata")
            .expect("metadata");
        assert_eq!(metadata["peer_agent"]["usageUpdate"]["used"], 42);
    }

    #[tokio::test]
    async fn submit_permission_resolves_gateway_permission_request() {
        let backend = Arc::new(FakeBackend::default());
        backend.request_permission();
        let harness = harness(backend);
        let source = GatewaySource::new("tui", "workdir").process();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut request = request(&harness, source.clone(), "permission");
        request.event_sink = Some(Arc::new(move |event| {
            let _ = event_tx.send(event);
        }));

        let gateway = harness.gateway.clone();
        let turn = tokio::spawn(async move { gateway.send_turn(request).await });

        loop {
            let event = event_rx.recv().await.expect("gateway event");
            if let GatewayEvent::PermissionRequested { request_id, .. } = event {
                assert_eq!(request_id, "permission-1");
                break;
            }
        }

        assert!(harness.gateway.submit_permission(
            GatewayThreadSelector::source(source.source_key()),
            "permission-1",
            PermissionApprovalDecision::allow_once(),
        ));
        turn.await.expect("turn task").expect("turn");

        let resolved = event_rx.recv().await.expect("permission resolved event");
        assert!(matches!(
            resolved,
            GatewayEvent::PermissionResolved {
                decision: PermissionDecision::AllowOnce,
                ..
            }
        ));
    }
