#[tokio::test]
async fn acp_peer_rejects_non_v1_protocol_without_fallback() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp_wrong_version.py");
    let log = harness._temp.path().join("wrong-version.jsonl");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps({"method": message.get("method")}) + "\n")
    if message.get("method") == "initialize":
        send({"jsonrpc": "2.0", "id": message.get("id"), "result": {
            "protocolVersion": 2,
            "agentCapabilities": {}
        }})
"#,
    )
    .expect("fake ACP script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Wrong-version ACP agent."
command = {}
args = ["{}", "{}"]
entrypoints = ["peer"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            log.display()
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Wrong-version ACP agent.
backend:
  ref: fake
entrypoints: [peer]
---
"#,
    )
    .expect("agent file");

    let raw_events = Arc::new(Mutex::new(Vec::<RunStreamEvent>::new()));
    let raw_events_for_sink = Arc::clone(&raw_events);
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "peer-wrong-version").persistent(),
        "hello",
    );
    turn_request.options.agent = Some("reviewer".to_string());
    turn_request.options.runtime_ref = Some("acp:fake".to_string());
    turn_request.options.inherited_env = Some(env.clone());
    turn_request.stream = Some(Arc::new(move |event| {
        raw_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let error = harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect_err("protocol v2 must not be accepted by the stable outbound adapter");
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["delivery"].as_str()),
        Some("not_delivered"),
        "{error}"
    );
    let methods = std::fs::read_to_string(log).expect("protocol log");
    assert!(methods.contains("initialize"), "{methods}");
    assert!(!methods.contains("session/new"), "{methods}");
    let raw_events = raw_events.lock().expect("raw events lock");
    assert!(!raw_events.iter().any(|event| matches!(
        event,
        RunStreamEvent::Event(value)
            if value["type"] == "acp_peer_protocol_negotiated"
                || value["type"] == "acp_peer_protocol_fallback"
    )));
}

#[tokio::test]
async fn acp_peer_v1_applies_controls_before_structured_prompt() {
    use base64::Engine as _;

    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp_v1_contract.py");
    let log = harness._temp.path().join("v1-contract.jsonl");
    let image = harness.cwd.join("pixel.png");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &image,
        base64::engine::general_purpose::STANDARD
            .decode(psychevo_ai::DEFAULT_FAKE_IMAGE_BASE64)
            .expect("PNG fixture"),
    )
    .expect("image");
    std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]
values = {"model": "test/default-model", "effort": "low", "mode": "ask", "fast": False}
next_session_id = 0

def send(value):
    print(json.dumps(value), flush=True)

def record(value):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps(value) + "\n")

def update(session_id, update_value):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update_value
    }})

def config_options():
    return [
        {"id": "model", "name": "Model", "category": "model", "type": "select",
         "currentValue": values["model"], "options": [
             {"value": "test/default-model", "name": "Default"},
             {"value": "test/second-model", "name": "Second"}]},
        {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select",
         "currentValue": values["effort"], "options": [
             {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
         "currentValue": values["mode"], "options": [
             {"value": "ask", "name": "Ask"}, {"value": "code", "name": "Code"}]},
        {"id": "fast", "name": "Fast", "type": "boolean", "currentValue": values["fast"]}
    ]

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        record({"event": "initialize", "version": params.get("protocolVersion")})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": True,
                "promptCapabilities": {"image": True, "embeddedContext": True},
                "sessionCapabilities": {"close": {}}
            }
        }})
    elif method == "session/new":
        next_session_id += 1
        record({"event": "new"})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "sessionId": "native-v1-contract-" + str(next_session_id), "configOptions": config_options()
        }})
    elif method == "session/set_config_option":
        config_id = params.get("configId")
        value = params.get("value")
        if isinstance(value, dict):
            value = value.get("value", value.get("boolean"))
        values[config_id] = value
        record({"event": "set", "id": config_id, "value": value})
        send({"jsonrpc": "2.0", "id": mid, "result": {"configOptions": config_options()}})
    elif method == "session/prompt":
        blocks = params.get("prompt") or []
        types = [block.get("type") for block in blocks]
        resource = next((block.get("resource") or {} for block in blocks if block.get("type") == "resource"), {})
        image = next((block for block in blocks if block.get("type") == "image"), {})
        record({
            "event": "prompt",
            "types": types,
            "resourceText": resource.get("text"),
            "resourceMime": resource.get("mimeType"),
            "imageMime": image.get("mimeType"),
            "imageDataLength": len(image.get("data") or ""),
            "values": values
        })
        update(params.get("sessionId"), {
            "sessionUpdate": "_future_status",
            "label": "forward compatible"
        })
        text = "structured:" + ",".join(types) + ":" + values["model"] + ":" + values["effort"] + ":" + values["mode"] + ":" + str(values["fast"]).lower()
        update(params.get("sessionId"), {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text}
        })
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    elif method == "session/close":
        record({"event": "close", "sessionId": params.get("sessionId")})
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
"#,
        )
        .expect("fake ACP script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Stable ACP v1 contract agent."
command = {}
args = ["{}", "{}"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            log.display()
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Stable ACP v1 contract agent.
backend:
  ref: fake
entrypoints: [peer]
tools: [read]
---
Peer instructions from markdown.
"#,
    )
    .expect("agent file");

    let raw_events = Arc::new(Mutex::new(Vec::<RunStreamEvent>::new()));
    let raw_events_for_sink = Arc::clone(&raw_events);
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "peer-v1-contract").persistent(),
        "inspect the image",
    );
    turn_request.options.agent = Some("reviewer".to_string());
    turn_request.options.runtime_ref = Some("acp:fake".to_string());
    turn_request.options.runtime_options = BTreeMap::from([
        ("model".to_string(), "test/second-model".to_string()),
        ("effort".to_string(), "high".to_string()),
        ("mode".to_string(), "code".to_string()),
        ("fast".to_string(), "true".to_string()),
    ]);
    turn_request.options.image_inputs = vec![ImageInput::LocalPath(image)];
    turn_request.options.inherited_env = Some(env.clone());
    turn_request.stream = Some(Arc::new(move |event| {
        raw_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let result = harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect("stable v1 contract turn");
    assert_eq!(
        result.result.final_answer,
        "structured:resource,text,image:test/second-model:high:code:true"
    );

    let records = std::fs::read_to_string(&log)
        .expect("contract log")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("contract record"))
        .collect::<Vec<_>>();
    let event_names = records
        .iter()
        .filter_map(|record| record["event"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        event_names,
        vec!["initialize", "new", "set", "set", "set", "set", "prompt"]
    );
    let config_ids = records
        .iter()
        .filter(|record| record["event"] == "set")
        .map(|record| record["id"].as_str().expect("config id"))
        .collect::<Vec<_>>();
    assert_eq!(config_ids, vec!["model", "effort", "mode", "fast"]);
    let prompt = records.last().expect("prompt record");
    assert_eq!(prompt["resourceText"], "Peer instructions from markdown.");
    assert_eq!(prompt["resourceMime"], "text/markdown");
    assert_eq!(prompt["imageMime"], "image/png");
    assert!(prompt["imageDataLength"].as_u64().unwrap_or_default() > 0);
    assert!(result.result.events.iter().any(|event| {
        event["type"] == "acp_peer_unknown_notification"
            && event["update_kind"] == "_future_status"
            && event["origin"] == "live"
    }));

    {
        let raw_events_guard = raw_events.lock().expect("raw events lock");
        assert!(raw_events_guard.iter().any(|event| matches!(
            event,
            RunStreamEvent::Event(value)
                if value["type"] == "acp_peer_protocol_negotiated"
                    && value["protocol_version"] == "1"
        )));
        assert!(!raw_events_guard.iter().any(|event| matches!(
            event,
            RunStreamEvent::Event(value) if value["type"] == "acp_peer_protocol_fallback"
        )));
    }

    let records_before_rejection = records.len();
    let mut rejected = request(
        &harness,
        GatewaySource::new("web", "peer-v1-config-rejected").persistent(),
        "must not be delivered",
    );
    rejected.options.agent = Some("reviewer".to_string());
    rejected.options.runtime_ref = Some("acp:fake".to_string());
    rejected
        .options
        .runtime_options
        .insert("model".to_string(), "test/missing-model".to_string());
    rejected.options.inherited_env = Some(env);
    let error = harness
        .gateway
        .send_turn(rejected)
        .await
        .expect_err("invalid ACP config must reject before prompt delivery");
    assert!(error.to_string().contains("test/missing-model"), "{error}");
    let rejected_records = std::fs::read_to_string(&log)
        .expect("contract log after rejection")
        .lines()
        .skip(records_before_rejection)
        .map(|line| serde_json::from_str::<Value>(line).expect("contract record"))
        .collect::<Vec<_>>();
    assert_eq!(
        rejected_records
            .iter()
            .filter_map(|record| record["event"].as_str())
            .collect::<Vec<_>>(),
        vec!["new"]
    );

    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("graceful ACP process shutdown");
    let shutdown_records = std::fs::read_to_string(&log).expect("shutdown contract log");
    assert!(
        shutdown_records.lines().any(|line| {
            serde_json::from_str::<Value>(line).is_ok_and(|record| record["event"] == "close")
        }),
        "graceful shutdown must close resident ACP sessions before process termination: {shutdown_records}"
    );
}

#[tokio::test]
async fn acp_peer_abort_sends_session_cancel_before_process_cleanup() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp_cancel.py");
    let log = harness._temp.path().join("cancel.jsonl");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]
prompt_id = None

def send(value):
    print(json.dumps(value), flush=True)

def record(method):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(method + "\n")

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    record(method or "response")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1, "agentCapabilities": {}
        }})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-cancel"}})
    elif method == "session/prompt":
        prompt_id = mid
    elif method == "session/cancel" and prompt_id is not None:
        send({"jsonrpc": "2.0", "id": prompt_id, "result": {"stopReason": "end_turn"}})
        prompt_id = None
"#,
    )
    .expect("fake ACP script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Cancellable ACP agent."
command = {}
args = ["{}", "{}"]
entrypoints = ["peer"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            log.display()
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Cancellable ACP agent.
backend:
  ref: fake
entrypoints: [peer]
---
"#,
    )
    .expect("agent file");

    let (handle, control) = run_control();
    let mut request = request(
        &harness,
        GatewaySource::new("web", "peer-cancel").persistent(),
        "wait",
    );
    request.options.agent = Some("reviewer".to_string());
    request.options.runtime_ref = Some("acp:fake".to_string());
    request.options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]));
    request.control_handle = Some(handle.clone());
    request.control = Some(control);
    let gateway = harness.gateway.clone();
    let turn = tokio::spawn(async move { gateway.send_turn(request).await });

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(&log)
                .ok()
                .is_some_and(|contents| contents.contains("session/prompt"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("prompt should start");
    handle.abort();

    let result = turn
        .await
        .expect("turn task")
        .expect("aborted turn should remain a typed result");
    assert_eq!(result.result.outcome, Outcome::Aborted);
    let methods = std::fs::read_to_string(log).expect("cancel log");
    assert!(methods.contains("session/cancel"), "{methods}");
    let binding = harness
        .state
        .store()
        .gateway_runtime_binding(&result.result.session_id)
        .expect("runtime binding")
        .expect("binding after abort");
    assert_eq!(binding.native_session_id.as_deref(), Some("native-cancel"));
}

#[tokio::test]
async fn acp_unknown_delivery_retains_input_without_automatic_retry() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let home = harness._temp.path().join("home");
    let script = harness._temp.path().join("fake_acp_unknown.py");
    let log = harness._temp.path().join("unknown-delivery.log");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]

def send(value):
    print(json.dumps(value), flush=True)

def record(method):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(method + "\n")

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    record(method or "response")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1, "agentCapabilities": {"loadSession": True}
        }})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-unknown"}})
    elif method == "session/prompt":
        sys.exit(0)
"#,
    )
    .expect("fake ACP script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Unknown-delivery ACP agent."
command = {}
args = ["{}", "{}"]
entrypoints = ["peer"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            log.display()
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Unknown-delivery ACP agent.
backend:
  ref: fake
entrypoints: [peer]
---
"#,
    )
    .expect("agent file");

    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "peer-unknown-delivery").persistent(),
        "legacy prompt",
    );
    turn_request.input = vec![GatewayInputPart::Text {
        text: "recover this exact input".to_string(),
    }];
    turn_request.options.agent = Some("reviewer".to_string());
    turn_request.options.runtime_ref = Some("acp:fake".to_string());
    turn_request.options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]));
    let turn_id = "turn-acp-unknown-delivery";
    let error = harness
        .gateway
        .run_turn_now("thread:acp-unknown", turn_request, turn_id.to_string())
        .await
        .expect_err("connection loss after prompt dispatch must stay unknown");
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
        .state
        .store()
        .gateway_turn_delivery(turn_id)
        .expect("delivery lookup")
        .expect("delivery record");
    assert_eq!(delivery.status, "unknown");
    assert!(
        delivery
            .input_json
            .as_deref()
            .is_some_and(|input| input.contains("recover this exact input"))
    );
    assert_eq!(delivery.delivery_confirmed_at_ms, None);
    assert_eq!(delivery.terminal_at_ms, None);
    let activity = harness
        .state
        .store()
        .gateway_activity(turn_id)
        .expect("activity lookup")
        .expect("activity");
    assert!(
        activity
            .intent
            .as_ref()
            .is_some_and(|intent| intent.get("input").is_some()),
        "unknown delivery must retain the duplicate durable activity input"
    );
    let methods = std::fs::read_to_string(log).expect("unknown delivery log");
    assert_eq!(methods.matches("initialize").count(), 1, "{methods}");
    assert_eq!(methods.matches("session/prompt").count(), 1, "{methods}");
}

#[tokio::test]
async fn acp_next_turn_load_reconciles_unknown_delivery_before_new_input() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let home = harness._temp.path().join("home-reconcile");
    let script = harness._temp.path().join("fake_acp_reconcile.py");
    let log = harness._temp.path().join("reconcile.jsonl");
    let state_path = harness._temp.path().join("reconcile-state.json");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import os
import sys

log_path = sys.argv[1]
state_path = sys.argv[2]

def load_state():
    try:
        with open(state_path, "r", encoding="utf-8") as state_file:
            return json.load(state_file)
    except (FileNotFoundError, json.JSONDecodeError):
        return {"promptCount": 0, "messages": []}

def save_state(state):
    with open(state_path, "w", encoding="utf-8") as state_file:
        json.dump(state, state_file)

def record(method, **fields):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps({"method": method, **fields}) + "\n")

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, message_id, text):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "messageId": message_id,
            "content": {"type": "text", "text": text}
        }
    }})

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        record(method)
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {"loadSession": True}
        }})
    elif method == "session/new":
        record(method)
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-reconcile"}})
    elif method == "session/load":
        state = load_state()
        record(method, sessionId=params.get("sessionId"))
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": params.get("sessionId"),
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "replayed-tool-only",
                "title": "Replay tool-only fact",
                "kind": "execute",
                "status": "completed"
            }
        }})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": params.get("sessionId"),
            "update": {
                "sessionUpdate": "plan",
                "entries": [{
                    "content": "Replay replacement plan",
                    "priority": "high",
                    "status": "completed"
                }]
            }
        }})
        for replay in state["messages"]:
            update(params.get("sessionId"), replay["id"], replay["text"])
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        state = load_state()
        state["promptCount"] += 1
        turn = state["promptCount"]
        prompt = "\n".join(
            block.get("text") or ""
            for block in params.get("prompt") or []
            if block.get("type") == "text"
        )
        answer = "reconciled answer " + str(turn)
        replay = {"id": "assistant-" + str(turn), "text": answer}
        state["messages"].append(replay)
        save_state(state)
        record(method, turn=turn, prompt=prompt)
        if turn == 1:
            os._exit(17)
        update(params.get("sessionId"), replay["id"], answer)
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
"#,
    )
    .expect("fake ACP reconciliation script");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Unknown-delivery reconciliation ACP agent."
command = {}
args = ["{}", "{}", "{}"]
entrypoints = ["peer"]
"#,
            test_python_command_toml(&harness.cwd),
            script.display(),
            log.display(),
            state_path.display(),
        ),
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("reviewer.md"),
        r#"---
name: reviewer
description: Reconcile unknown delivery through Agent history.
backend:
  ref: fake
entrypoints: [peer]
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
    let source = GatewaySource::new("web", "peer-reconcile").persistent();
    let request_for = |prompt: &str| {
        let mut request = request(&harness, source.clone(), prompt);
        request.input = vec![GatewayInputPart::Text {
            text: prompt.to_string(),
        }];
        request.options.agent = Some("reviewer".to_string());
        request.options.runtime_ref = Some("acp:fake".to_string());
        request.options.inherited_env = Some(env.clone());
        request
    };

    let first_turn_id = "turn-acp-reconcile-unknown";
    harness
        .gateway
        .send_turn_with_id(
            request_for("old input with unknown delivery"),
            first_turn_id.to_string(),
        )
        .await
        .expect_err("first prompt response is lost after Agent acceptance");
    let first_delivery = harness
        .state
        .store()
        .gateway_turn_delivery(first_turn_id)
        .expect("first delivery")
        .expect("first delivery record");
    assert_eq!(first_delivery.status, "unknown");
    assert!(first_delivery.input_json.is_some());
    let thread_id = first_delivery.thread_id.clone();
    let first_terminal = harness
        .state
        .store()
        .gateway_turn_terminal(first_turn_id)
        .expect("first terminal")
        .expect("first terminal record");
    assert_eq!(first_terminal.status, "failed");
    assert_eq!(
        harness
            .state
            .store()
            .load_tui_message_summaries(&thread_id)
            .expect("messages after unknown delivery")
            .len(),
        1,
        "transport failure belongs to the terminal fact, not an assistant message"
    );
    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("settle first ACP generation");

    let second_turn_id = "turn-acp-reconcile-second";
    let mut second_request = request_for("new input after reconciliation");
    second_request.thread_id = Some(thread_id.clone());
    let second = harness
        .gateway
        .send_turn_with_id(second_request, second_turn_id.to_string())
        .await
        .expect("next explicit turn loads and continues");
    assert_eq!(second.result.final_answer, "reconciled answer 2");

    let reconciled_delivery = harness
        .state
        .store()
        .gateway_turn_delivery(first_turn_id)
        .expect("reconciled delivery")
        .expect("reconciled delivery record");
    assert_eq!(reconciled_delivery.status, "terminal");
    assert_eq!(reconciled_delivery.input_json, None);
    assert!(reconciled_delivery.delivery_confirmed_at_ms.is_some());
    assert!(reconciled_delivery.terminal_at_ms.is_some());
    let reconciled_terminal = harness
        .state
        .store()
        .gateway_turn_terminal(first_turn_id)
        .expect("reconciled terminal")
        .expect("reconciled terminal record");
    assert_eq!(reconciled_terminal.status, "completed");
    assert_eq!(reconciled_terminal.outcome.as_deref(), Some("normal"));
    assert_eq!(reconciled_terminal.error_message, None);
    assert_eq!(
        reconciled_terminal
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("replayMessageIds")),
        Some(&json!(["assistant-1"])),
        "tool and Plan replay ids are not delivery evidence"
    );

    let messages = harness
        .state
        .store()
        .load_tui_message_summaries(&thread_id)
        .expect("reconciled messages");
    assert_eq!(
        messages
            .iter()
            .filter(|summary| matches!(summary.message, Message::User { .. }))
            .count(),
        2
    );
    assert!(
        messages.iter().any(|summary| {
            serde_json::to_string(&summary.message)
                .expect("replayed message json")
                .contains("reconciled answer 1")
        })
    );
    assert!(
        messages.iter().any(|summary| {
            summary
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.pointer("/acp/turnId"))
                .and_then(Value::as_str)
                == Some(first_turn_id)
                && summary
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.pointer("/acp/messageIds"))
                    == Some(&json!(["assistant-1"]))
        }),
        "the reconciled assistant carries only its real ACP message id"
    );
    assert!(messages.iter().any(|summary| {
        matches!(summary.message, Message::User { .. })
            && serde_json::to_string(&summary.message)
                .expect("new input json")
                .contains("new input after reconciliation")
    }));

    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("settle second ACP generation");
    let mut third_request = request_for("third input after a second load");
    third_request.thread_id = Some(thread_id.clone());
    let third = harness
        .gateway
        .send_turn_with_id(third_request, "turn-acp-reconcile-third".to_string())
        .await
        .expect("second load deduplicates replay before third input");
    assert_eq!(third.result.final_answer, "reconciled answer 3");
    let deduplicated = harness
        .state
        .store()
        .load_tui_message_summaries(&thread_id)
        .expect("deduplicated messages");
    assert_eq!(deduplicated.len(), 8);
    let encoded_messages = deduplicated
        .iter()
        .map(|summary| serde_json::to_string(&summary.message).expect("message json"))
        .collect::<Vec<_>>();
    assert_eq!(
        encoded_messages
            .iter()
            .filter(|message| message.contains("reconciled answer 1"))
            .count(),
        1
    );
    assert_eq!(
        encoded_messages
            .iter()
            .filter(|message| message.contains("reconciled answer 2"))
            .count(),
        1
    );
    assert_eq!(
        deduplicated
            .iter()
            .filter(|summary| {
                summary
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.pointer("/acp/replayId"))
                    .and_then(Value::as_str)
                    == Some("tool:replayed-tool-only")
            })
            .count(),
        1
    );
    assert_eq!(
        deduplicated
            .iter()
            .filter(|summary| {
                summary
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.pointer("/acp/replayId"))
                    .and_then(Value::as_str)
                    == Some("plan:legacy-v1")
            })
            .count(),
        1
    );

    let events = std::fs::read_to_string(&log)
        .expect("reconciliation log")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("reconciliation event"))
        .collect::<Vec<_>>();
    let methods = events
        .iter()
        .filter_map(|event| event["method"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "initialize",
            "session/new",
            "session/prompt",
            "initialize",
            "session/load",
            "session/prompt",
            "initialize",
            "session/load",
            "session/prompt",
        ]
    );
    let prompts = events
        .iter()
        .filter(|event| event["method"] == "session/prompt")
        .filter_map(|event| event["prompt"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        prompts,
        vec![
            "old input with unknown delivery",
            "new input after reconciliation",
            "third input after a second load",
        ],
        "the unknown input is not replayed during session/load"
    );
}

#[tokio::test]
async fn submit_permission_resolves_gateway_permission_request() {
    let backend = Arc::new(FakeBackend::default());
    backend.request_permission();
    let harness = harness(backend);
    let source = GatewaySource::new("tui", "cwd").process();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut request = request(&harness, source.clone(), "permission");
    request.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));

    let gateway = harness.gateway.clone();
    let turn = tokio::spawn(async move { gateway.send_turn(request).await });

    loop {
        let event = event_rx.recv().await.expect("gateway event");
        if let GatewayEvent::ActionRequested { action } = event
            && action.kind == GatewayActionKind::Permission
        {
            assert_eq!(action.action_id, "permission-1");
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
        GatewayEvent::ActionResolved {
            kind: GatewayActionKind::Permission,
            outcome: GatewayActionOutcome::Accepted,
            payload,
            ..
        } if payload["decision"] == "allowOnce"
    ));
}

#[tokio::test]
async fn submit_permission_accepts_thread_alias_for_source_started_request() {
    let backend = Arc::new(FakeBackend::default());
    backend.request_permission();
    let harness = harness(backend);
    let source = GatewaySource::new("tui", "cwd").process();
    let source_queue_key = source_key_key(&source.source_key());
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut request = request(&harness, source.clone(), "permission");
    request.event_sink = Some(Arc::new(move |event| {
        let _ = event_tx.send(event);
    }));

    let gateway = harness.gateway.clone();
    let turn = tokio::spawn(async move { gateway.send_turn(request).await });

    loop {
        let event = event_rx.recv().await.expect("gateway event");
        if let GatewayEvent::ActionRequested { action } = event
            && action.kind == GatewayActionKind::Permission
        {
            assert_eq!(action.action_id, "permission-1");
            break;
        }
    }

    harness
        .gateway
        .register_active_thread_alias(&source_queue_key, "thread-materialized");
    assert!(harness.gateway.submit_permission(
        GatewayThreadSelector::thread_id("thread-materialized"),
        "permission-1",
        PermissionApprovalDecision::allow_once(),
    ));
    turn.await.expect("turn task").expect("turn");
}
