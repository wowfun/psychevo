#[tokio::test]
async fn native_turn_delivery_ledger_scrubs_confirmed_input_at_terminal() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let input = vec![GatewayInputPart::Text {
        text: "private native prompt".to_string(),
    }];
    let input_json = serde_json::to_string(&input).expect("delivery input json");
    let input_hash = format!("{:x}", Sha256::digest(input_json.as_bytes()));
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "native-delivery-ledger").persistent(),
        "legacy prompt is replaced by structured input",
    );
    turn_request.input = input;

    let result = harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect("native turn");
    let delivery = harness
        .state
        .store()
        .gateway_turn_delivery(&result.turn.id)
        .expect("delivery lookup")
        .expect("delivery record");
    assert_eq!(delivery.status, "terminal");
    assert_eq!(delivery.runtime_ref, "native");
    assert_eq!(delivery.input_hash, input_hash);
    assert_eq!(delivery.input_json, None);
    assert!(delivery.delivery_confirmed_at_ms.is_some());
    assert!(delivery.terminal_at_ms.is_some());
    let activity = harness
        .state
        .store()
        .gateway_activity(&result.turn.id)
        .expect("activity lookup")
        .expect("activity");
    assert!(
        activity
            .intent
            .as_ref()
            .is_some_and(|intent| intent.get("input").is_none()),
        "confirmed delivery must scrub the duplicate durable activity input"
    );
}

#[tokio::test]
async fn public_turn_terminal_observes_completed_thread_activity() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let observed_status = Arc::new(Mutex::new(None));
    let status_for_event = Arc::clone(&observed_status);
    let state_for_event = harness.state.clone();
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "terminal-activity-order").persistent(),
        "finish activity before terminal",
    );
    turn_request.event_sink = Some(Arc::new(move |event| {
        if let GatewayEvent::TurnCompleted { turn_id, .. } = event {
            let status = state_for_event
                .store()
                .gateway_activity(&turn_id)
                .expect("activity read at terminal")
                .expect("activity at terminal")
                .status;
            *status_for_event.lock().expect("terminal status lock") = Some(status);
        }
    }));

    harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect("native turn");

    assert_eq!(
        observed_status.lock().expect("observed status").as_deref(),
        Some("completed")
    );
}

#[tokio::test]
async fn delegated_acp_child_owns_activity_turn_identity_and_terminal_order() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
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
        .store()
        .create_session_with_metadata(&harness.cwd, "web", "model", "provider", None)
        .expect("parent Thread");
    let child_thread_id = harness
        .state
        .store()
        .create_child_session_with_metadata(
            &parent_thread_id,
            &harness.cwd,
            "peer_agent",
            "opencode",
            "acp:fake",
            None,
        )
        .expect("child Thread");
    harness
        .state
        .store()
        .upsert_agent_edge(
            &parent_thread_id,
            &child_thread_id,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("open child edge");
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
        .expect("parent activity");

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
    let terminal_observation = Arc::new(Mutex::new(None));
    let terminal_for_sink = Arc::clone(&terminal_observation);
    let gateway_for_sink = harness.gateway.clone();
    let state_for_sink = harness.state.clone();
    let child_for_sink = child_thread_id.clone();
    let event_sink: GatewayEventSink = Arc::new(move |event| {
        if matches!(event, GatewayEvent::TurnCompleted { .. }) {
            let activity = gateway_for_sink
                .activity_for_selector(GatewayThreadSelector::thread_id(&child_for_sink));
            let edge = state_for_sink
                .store()
                .find_agent_edge(&child_for_sink)
                .expect("edge at terminal")
                .expect("child edge at terminal");
            *terminal_for_sink.lock().expect("terminal observation") =
                Some((activity, edge.status));
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
        event_sink: Some(event_sink),
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let child_turn_id = "turn-child".to_string();
    let running = tokio::spawn(delegate.run_inner(ExternalAgentDelegateRequest {
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

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if std::fs::read_to_string(&log)
                .ok()
                .is_some_and(|contents| contents.contains("prompt_blocked"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child prompt started");

    let parent = harness
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread_id));
    let child = harness
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&child_thread_id));
    assert!(parent.running);
    assert!(child.running);
    assert_eq!(child.active_turn_id.as_deref(), Some(child_turn_id.as_str()));

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

    let (terminal_activity, terminal_edge) = terminal_observation
        .lock()
        .expect("terminal observation")
        .clone()
        .expect("terminal observed");
    assert!(!terminal_activity.running);
    assert_eq!(terminal_activity.active_turn_id, None);
    assert_eq!(terminal_edge, psychevo_runtime::AgentEdgeStatus::Closed);
    assert!(
        harness
            .gateway
            .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread_id))
            .running
    );

    harness.gateway.finish_durable_gateway_activity(Some(&parent_activity), "completed");
    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
}

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
import os
import sqlite3
import sys

loaded_session = None
counter_path = sys.argv[0] + ".counter"
process_counter_path = sys.argv[0] + ".processes"
try:
    with open(process_counter_path, "r", encoding="utf-8") as process_counter_file:
        process_counter = int(process_counter_file.read())
except (FileNotFoundError, ValueError):
    process_counter = 0
with open(process_counter_path, "w", encoding="utf-8") as process_counter_file:
    process_counter_file.write(str(process_counter + 1))

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
        try:
            with open(counter_path, "r", encoding="utf-8") as counter_file:
                counter = int(counter_file.read())
        except (FileNotFoundError, ValueError):
            counter = 0
        counter += 1
        with open(counter_path, "w", encoding="utf-8") as counter_file:
            counter_file.write(str(counter))
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-" + str(counter)}})
    elif method == "session/load":
        loaded_session = params.get("sessionId")
        update(loaded_session, {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "old answer from loaded history"}
        })
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-1"
        with sqlite3.connect(os.environ["PSYCHEVO_BINDING_DB"]) as connection:
            persisted = connection.execute(
                "SELECT native_session_id FROM gateway_runtime_bindings WHERE native_session_id = ?",
                (session_id,),
            ).fetchone()
        if persisted != (session_id,):
            raise RuntimeError("native session binding was not persisted before prompt")
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
    let first = harness
        .gateway
        .send_turn(first_request)
        .await
        .expect("first peer turn");

    assert_eq!(first.thread.backend.kind, BackendKind::Acp);
    assert!(
        first
            .thread
            .backend
            .native_id
            .as_deref()
            .is_some_and(|handle| handle.starts_with("ags_") && !handle.contains("native-1"))
    );
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
    assert_eq!(binding.backend_kind, "unresolved");
    assert_eq!(binding.backend_native_id, None);
    let runtime_binding = harness
        .state
        .store()
        .gateway_runtime_binding(&first.result.session_id)
        .expect("runtime binding lookup")
        .expect("runtime binding");
    assert_eq!(runtime_binding.backend_kind.as_deref(), Some("acp"));
    assert_eq!(
        runtime_binding.native_session_id.as_deref(),
        Some("native-1")
    );
    let delivery = harness
        .state
        .store()
        .gateway_turn_delivery(&first.turn.id)
        .expect("ACP delivery lookup")
        .expect("ACP delivery record");
    assert_eq!(delivery.status, "terminal");
    assert_eq!(delivery.runtime_ref, "acp:fake");
    assert_eq!(delivery.input_json, None);
    assert!(delivery.delivery_confirmed_at_ms.is_some());
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
    let summary = harness
        .state
        .store()
        .session_summary(&first.result.session_id)
        .expect("session summary")
        .expect("summary");
    assert_eq!(summary.title.as_deref(), Some("hello"));

    let mut second_request = request(&harness, source.clone(), "again");
    second_request.options.agent = Some("reviewer".to_string());
    second_request.options.runtime_ref = Some("acp:fake".to_string());
    second_request.options.inherited_env = Some(env.clone());
    let second = harness
        .gateway
        .send_turn(second_request)
        .await
        .expect("second peer turn");
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
        .store()
        .create_child_session_with_metadata(
            &first.result.session_id,
            &harness.cwd,
            "peer_agent",
            "reviewer",
            "acp:fake",
            None,
        )
        .expect("child peer session");
    let mut child_request = request(&harness, source, "child prompt");
    child_request.thread_id = Some(child_session.clone());
    child_request.options.agent = Some("reviewer".to_string());
    child_request.options.runtime_ref = Some("acp:fake".to_string());
    child_request.options.inherited_env = Some(env);
    let child = harness
        .gateway
        .send_turn(child_request)
        .await
        .expect("child peer turn");
    assert_eq!(child.result.session_id, child_session);
    let child_summary = harness
        .state
        .store()
        .session_summary(&child.result.session_id)
        .expect("child summary")
        .expect("child");
    assert_eq!(child_summary.title, None);
}
#[tokio::test]
async fn non_peer_turn_clears_acp_peer_usage_projection_without_losing_native_session() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend.clone());
    let session_id = harness
        .state
        .store()
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

prompt_count = 0
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
        prompt_count += 1
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
        update(session_id, {"sessionUpdate": "plan", "entries": [
            {"content": "Persist replacement plan", "priority": "high", "status": "completed"},
            {"content": "Verify terminal history", "priority": "high", "status": "in_progress"}
        ]})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-echo", "status": "completed", "content": [
            {"type": "content", "content": {"type": "text", "text": "done\n"}}
        ], "rawOutput": {"output": "done\n"}})
        usage = {
            "totalTokens": 144 if prompt_count == 1 else 200,
            "inputTokens": 100 if prompt_count == 1 else 140,
            "outputTokens": 44 if prompt_count == 1 else 60,
            "cachedReadTokens": 30 if prompt_count == 1 else 50,
            "thoughtTokens": 4 if prompt_count == 1 else 8
        }
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "stopReason": "end_turn",
            "usage": usage
        }})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {
            "type": "text", "text": "must remain after the response fence"
        }})
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
    first_request.event_sink = Some(Arc::new(move |event| {
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
        .gateway
        .send_turn(first_request)
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
    let history_plan = persisted_blocks
        .iter()
        .find(|block| block.title.as_deref() == Some("Plan"))
        .expect("durable history plan");
    assert_eq!(history_plan.id, committed_plan.id);
    assert_eq!(history_plan.body, committed_plan.body);
    assert_eq!(history_plan.metadata, committed_plan.metadata);

    let summaries = harness
        .state
        .store()
        .load_tui_message_summaries(&result.result.session_id)
        .expect("stored messages");
    let stored_assistant = summaries
        .iter()
        .find(|summary| matches!(summary.message, psychevo_runtime::Message::Assistant { .. }))
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
    let usage_summary = psychevo_runtime::session_usage_summary(
        psychevo_runtime::SessionUsageOptions {
            state: harness.state.clone(),
            session_id: result.result.session_id.clone(),
        },
    )
    .expect("session usage");
    assert_eq!(usage_summary.effective_total_tokens, Some(144));
    assert_eq!(usage_summary.total_status, "reported");
    let psychevo_runtime::Message::Assistant { content, .. } = &stored_assistant.message else {
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
        .gateway
        .send_turn(second_request)
        .await
        .expect("second streaming peer turn");
    assert_eq!(second_result.result.session_id, result.result.session_id);

    let summaries = harness
        .state
        .store()
        .load_tui_message_summaries(&result.result.session_id)
        .expect("stored messages after second turn");
    let stored_assistants = summaries
        .iter()
        .filter(|summary| {
            matches!(
                summary.message,
                psychevo_runtime::Message::Assistant { .. }
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
    let usage_summary = psychevo_runtime::session_usage_summary(
        psychevo_runtime::SessionUsageOptions {
            state: harness.state.clone(),
            session_id: result.result.session_id.clone(),
        },
    )
    .expect("session usage after cumulative ACP update");
    assert_eq!(usage_summary.effective_total_tokens, Some(200));
    assert_eq!(usage_summary.reported_total_tokens, 200);
    assert_eq!(usage_summary.total_status, "reported");
}
