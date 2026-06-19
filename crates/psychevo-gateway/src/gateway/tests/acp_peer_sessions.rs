    #[tokio::test]
    async fn acp_peer_agent_turn_routes_to_backend_and_persists_native_session() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let home = harness._temp.path().join("home");
        let script = harness._temp.path().join("fake_acp.py");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

loaded_session = None

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update
    }})

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-1"}})
    elif method == "session/load":
        loaded_session = params.get("sessionId")
        update(loaded_session, {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "old answer from loaded history"}
        })
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-1"
        chunks = []
        for block in params.get("prompt") or []:
            if block.get("type") == "text":
                chunks.append(block.get("text") or "")
        prefix = "loaded:" + loaded_session if loaded_session else "new:" + session_id
        text = prefix + ":" + "\n".join(chunks)
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }
        }})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
"#,
        )
        .expect("fake acp script");
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
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
        first_request.options.inherited_env = Some(env.clone());
        let first = harness
            .gateway
            .send_turn(first_request)
            .await
            .expect("first peer turn");

        assert_eq!(first.thread.backend.kind, BackendKind::PeerAgent);
        assert_eq!(first.thread.backend.native_id.as_deref(), Some("native-1"));
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
            .store()
            .gateway_source_binding(&source.source_key().0)
            .expect("binding lookup")
            .expect("binding");
        assert_eq!(binding.backend_kind, "peer_agent");
        assert_eq!(binding.backend_native_id.as_deref(), Some("native-1"));
        let metadata = harness
            .state
            .store()
            .session_metadata(&first.result.session_id)
            .expect("metadata")
            .expect("metadata value");
        assert_eq!(metadata["peer_agent"]["nativeSessionId"], "native-1");
        let transcript = harness
            .gateway
            .thread_transcript(&first.result.session_id)
            .expect("transcript");
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].role, TranscriptEntryRole::User);
        assert_eq!(transcript[1].role, TranscriptEntryRole::Assistant);

        let mut second_request = request(&harness, source.clone(), "again");
        second_request.options.agent = Some("reviewer".to_string());
        second_request.options.inherited_env = Some(env);
        let second = harness
            .gateway
            .send_turn(second_request)
            .await
            .expect("second peer turn");
        assert_eq!(second.result.session_id, first.result.session_id);
        assert!(second.result.final_answer.contains("loaded:native-1"));
        assert!(!second.result.final_answer.contains("old answer from loaded history"));
    }
    #[tokio::test]
    async fn non_peer_turn_clears_acp_peer_usage_projection_without_losing_native_session() {
        let backend = Arc::new(FakeBackend::default());
        let harness = harness(backend.clone());
        let session_id = harness
            .state
            .store()
            .create_session_with_metadata(
                &harness.workdir,
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
            .expect("session");
        let mut request = request(
            &harness,
            GatewaySource::new("web", "default-after-peer").persistent(),
            "continue with default",
        );
        request.thread_id = Some(session_id.clone());

        let result = harness.gateway.send_turn(request).await.expect("turn");

        assert_eq!(result.result.session_id, session_id);
        assert_eq!(
            backend.runs()[0].session.as_deref(),
            Some(session_id.as_str())
        );
        let metadata = harness
            .state
            .store()
            .session_metadata(&session_id)
            .expect("metadata")
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
        let harness = harness(backend.clone());
        let home = harness._temp.path().join("home");
        let script = harness._temp.path().join("fake_acp_stream.py");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update
    }})

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-stream"}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-stream"
        update(session_id, {"sessionUpdate": "session_info_update", "title": "ACP streamed title"})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "research", "description": "Run peer research"}
        ]})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "think "}})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "first"}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hello "}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "world"}})
        update(session_id, {"sessionUpdate": "tool_call", "toolCallId": "call-echo", "title": "Run echo", "kind": "execute", "status": "pending", "rawInput": {"cmd": "echo done"}})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-echo", "status": "in_progress", "content": [
            {"type": "content", "content": {"type": "text", "text": "running\n"}}
        ]})
        update(session_id, {"sessionUpdate": "plan", "entries": [
            {"content": "Inspect repo", "priority": "high", "status": "completed"},
            {"content": "Patch bridge", "priority": "high", "status": "in_progress"}
        ]})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-echo", "status": "completed", "content": [
            {"type": "content", "content": {"type": "text", "text": "done\n"}}
        ], "rawOutput": {"output": "done\n"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
"#,
        )
        .expect("fake acp stream script");
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
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
        let mut request = request(&harness, source, "hello");
        request.options.agent = Some("reviewer".to_string());
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
            .expect("streaming peer turn");

        assert_eq!(result.result.final_answer, "hello world");
        assert!(
            result
                .result
                .events
                .iter()
                .any(|event| event["update_kind"] == "available_commands_update"),
            "available commands update should be retained as a structured ACP event"
        );
        assert!(
            result
                .result
                .events
                .iter()
                .any(|event| event["update_kind"] == "session_info_update"),
            "session info update should be retained as a structured ACP event"
        );

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
        assert!(
            blocks.iter().any(|block| {
                block.kind == TranscriptBlockKind::Status
                    && block.title.as_deref() == Some("Plan")
                    && block
                        .body
                        .as_deref()
                        .is_some_and(|body| body.contains("Inspect repo"))
            }),
            "ACP plan updates should render as a live plan block"
        );
        drop(gateway_events);

        let summary = harness
            .state
            .store()
            .session_summary(&result.result.session_id)
            .expect("session summary")
            .expect("summary");
        assert_eq!(summary.title.as_deref(), Some("ACP streamed title"));
        let transcript = harness
            .gateway
            .thread_transcript(&result.result.session_id)
            .expect("transcript");
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
    }
