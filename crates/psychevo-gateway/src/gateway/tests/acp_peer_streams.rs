use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psychevo::ThreadAgentBinding;
use psychevo::application::{Message, PermissionApprovalDecision};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::support_peer::{
    FrameworkNativeProbe, copied_acp_fixture, harness, request, send_framework_turn,
    send_framework_turn_with_handle, send_framework_turn_with_id, test_acp_command_toml,
};
use crate::GatewayEventEmitter;
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, GatewayEvent,
};
use psychevo_gateway_protocol::source::{GatewayInputPart, GatewaySource};

#[tokio::test]
async fn acp_peer_rejects_non_v1_protocol_without_fallback() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_wrong_version",
        "fake_acp_wrong_version",
    );
    let script = fixture.script;
    let log = harness._temp.path().join("wrong-version.jsonl");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Wrong-version ACP agent."
command = {}
args = [{}, {}]
entrypoints = ["peer"]
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script),
            crate::test_support::toml_path(&log)
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

    let turn_events = Arc::new(Mutex::new(Vec::<psychevo::TurnEvent>::new()));
    let turn_events_for_sink = Arc::clone(&turn_events);
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
    turn_request.policy.agent_ref = Some("reviewer".to_string());
    turn_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    turn_request.policy.inherited_env = Some(env.clone());
    turn_request.turn_events = Some(Arc::new(move |event| {
        turn_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let error = harness
        .send(turn_request)
        .await
        .expect_err("protocol v2 must not be accepted by the stable outbound adapter");
    assert!(error.to_string().contains("protocol"), "{error}");
    let methods = std::fs::read_to_string(log).expect("protocol log");
    assert!(methods.contains("initialize"), "{methods}");
    assert!(!methods.contains("session/new"), "{methods}");
    let turn_events = turn_events.lock().expect("Turn events lock");
    assert!(!turn_events.iter().any(|event| matches!(
        event,
        psychevo::TurnEvent::Runtime { data }
            if data["type"] == "acp_peer_protocol_negotiated"
                || data["type"] == "acp_peer_protocol_fallback"
    )));
}

#[tokio::test]
async fn acp_peer_v1_applies_controls_before_structured_prompt() {
    use base64::Engine as _;

    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_v1_contract",
        "fake_acp_v1_contract",
    );
    let script = fixture.script;
    let log = harness._temp.path().join("v1-contract.jsonl");
    let image = harness.cwd.join("pixel.png");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &image,
        base64::engine::general_purpose::STANDARD
            .decode(crate::test_support::ONE_PIXEL_PNG_BASE64)
            .expect("PNG fixture"),
    )
    .expect("image");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Stable ACP v1 contract agent."
command = {}
args = [{}, {}]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script),
            crate::test_support::toml_path(&log)
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

    let turn_events = Arc::new(Mutex::new(Vec::<psychevo::TurnEvent>::new()));
    let turn_events_for_sink = Arc::clone(&turn_events);
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
    turn_request.policy.agent_ref = Some("reviewer".to_string());
    turn_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    turn_request.policy.control_values = BTreeMap::from([
        ("model".to_string(), "test/second-model".to_string()),
        ("effort".to_string(), "high".to_string()),
        ("mode".to_string(), "code".to_string()),
        ("fast".to_string(), "true".to_string()),
    ]);
    turn_request.input.push(GatewayInputPart::Image {
        input: psychevo_gateway_protocol::source::GatewayImageInput::LocalPath {
            path: image.display().to_string(),
        },
    });
    turn_request.policy.inherited_env = Some(env.clone());
    turn_request.turn_events = Some(Arc::new(move |event| {
        turn_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let result = harness
        .send(turn_request)
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
    assert!(
        turn_events
            .lock()
            .expect("Turn events lock")
            .iter()
            .filter_map(|event| match event {
                psychevo::TurnEvent::Runtime { data } => Some(data),
                _ => None,
            })
            .any(|event| {
                event["type"] == "acp_peer_unknown_notification"
                    && event["update_kind"] == "_future_status"
                    && event["origin"] == "live"
            })
    );

    {
        let turn_events = turn_events.lock().expect("Turn events lock");
        assert!(turn_events.iter().any(|event| matches!(
            event,
            psychevo::TurnEvent::Runtime { data }
                if data["type"] == "acp_peer_protocol_negotiated"
                    && data["protocol_version"] == "1"
        )));
        assert!(!turn_events.iter().any(|event| matches!(
            event,
            psychevo::TurnEvent::Runtime { data }
                if data["type"] == "acp_peer_protocol_fallback"
        )));
    }

    let records_before_rejection = records.len();
    let mut rejected = request(
        &harness,
        GatewaySource::new("web", "peer-v1-config-rejected").persistent(),
        "must not be delivered",
    );
    rejected.policy.agent_ref = Some("reviewer".to_string());
    rejected.policy.runtime_profile_ref = Some("acp:fake".to_string());
    rejected
        .policy
        .control_values
        .insert("model".to_string(), "test/missing-model".to_string());
    rejected.policy.inherited_env = Some(env);
    let error = harness
        .send(rejected)
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
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_cancel",
        "fake_acp_cancel",
    );
    let script = fixture.script;
    let log = harness._temp.path().join("cancel.jsonl");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Cancellable ACP agent."
command = {}
args = [{}, {}]
entrypoints = ["peer"]
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script),
            crate::test_support::toml_path(&log)
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

    let (handle_tx, handle_rx) = oneshot::channel();
    let mut request = request(
        &harness,
        GatewaySource::new("web", "peer-cancel").persistent(),
        "wait",
    );
    request.policy.agent_ref = Some("reviewer".to_string());
    request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    request.policy.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]));
    let (application, gateway) = harness.runner();
    let turn = tokio::spawn(async move {
        send_framework_turn_with_handle(application, gateway, request, handle_tx).await
    });

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
    handle_rx
        .await
        .expect("accepted Framework Turn handle")
        .interrupt();

    let result = turn
        .await
        .expect("turn task")
        .expect("aborted turn should remain a typed result");
    assert_eq!(result.result.outcome, psychevo::TurnOutcome::Interrupted);
    let methods = std::fs::read_to_string(log).expect("cancel log");
    assert!(methods.contains("session/cancel"), "{methods}");
    let binding = harness
        ._application
        .client()
        .thread_agent_binding(&result.result.thread_id)
        .await
        .expect("runtime binding")
        .expect("binding after abort");
    let ThreadAgentBinding::Resolved { binding, .. } = binding else {
        panic!("binding remained unresolved after abort");
    };
    assert_eq!(binding.native_session_id.as_deref(), Some("native-cancel"));
}

#[tokio::test]
async fn acp_next_turn_load_reconciles_unknown_delivery_before_new_input() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_reconcile",
        "fake_acp_reconcile",
    );
    let script = fixture.script;
    let log = harness._temp.path().join("reconcile.jsonl");
    let state_path = harness._temp.path().join("reconcile-state.json");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Unknown-delivery reconciliation ACP agent."
command = {}
args = [{}, {}, {}]
entrypoints = ["peer"]
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script),
            crate::test_support::toml_path(&log),
            crate::test_support::toml_path(&state_path),
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
        request.policy.agent_ref = Some("reviewer".to_string());
        request.policy.runtime_profile_ref = Some("acp:fake".to_string());
        request.policy.inherited_env = Some(env.clone());
        request
    };

    let first_turn_id = "turn-acp-reconcile-unknown";
    let first_error = send_framework_turn_with_id(
        harness._application.clone(),
        harness.gateway.clone(),
        request_for("old input with unknown delivery"),
        first_turn_id.to_string(),
    )
    .await
    .expect_err("first prompt response is lost after Agent acceptance");
    let thread_id = harness
        .gateway
        .resolve_source_thread(&source)
        .await
        .expect("source lookup after uncertain delivery")
        .expect("published Thread after uncertain delivery");
    let first_terminal = harness
        ._application
        .client()
        .framework_turn_terminal_evidence(first_turn_id)
        .await
        .expect("first terminal")
        .expect("first terminal record");
    assert_eq!(first_terminal.status.as_str(), "failed", "{first_error}");
    assert_eq!(
        harness
            ._application
            .client()
            .resume_thread(&thread_id)
            .await
            .expect("Thread after uncertain delivery")
            .history()
            .latest(Some(200))
            .await
            .expect("messages after unknown delivery")
            .items
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
    let second = send_framework_turn_with_id(
        harness._application.clone(),
        harness.gateway.clone(),
        second_request,
        second_turn_id.to_string(),
    )
    .await
    .expect("next explicit turn loads and continues");
    assert_eq!(second.result.final_answer, "reconciled answer 2");

    let reconciled_terminal = harness
        ._application
        .client()
        .framework_turn_terminal_evidence(first_turn_id)
        .await
        .expect("reconciled terminal")
        .expect("reconciled terminal record");
    assert_eq!(reconciled_terminal.status.as_str(), "completed");
    assert_eq!(reconciled_terminal.outcome.as_str(), "normal");

    let messages = harness
        ._application
        .client()
        .resume_thread(&thread_id)
        .await
        .expect("reconciled Thread")
        .history()
        .latest(Some(200))
        .await
        .expect("reconciled messages")
        .items;
    assert_eq!(
        messages
            .iter()
            .filter(|summary| matches!(summary.message, Message::User { .. }))
            .count(),
        2
    );
    assert!(messages.iter().any(|summary| {
        serde_json::to_string(&summary.message)
            .expect("replayed message json")
            .contains("reconciled answer 1")
    }));
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
    let third = send_framework_turn_with_id(
        harness._application.clone(),
        harness.gateway.clone(),
        third_request,
        "turn-acp-reconcile-third".to_string(),
    )
    .await
    .expect("second load deduplicates replay before third input");
    assert_eq!(third.result.final_answer, "reconciled answer 3");
    let deduplicated = harness
        ._application
        .client()
        .resume_thread(&thread_id)
        .await
        .expect("deduplicated Thread")
        .history()
        .latest(Some(200))
        .await
        .expect("deduplicated messages")
        .items;
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
    let backend = Arc::new(FrameworkNativeProbe::default());
    backend.request_permission();
    let harness = harness(backend).await;
    let source = GatewaySource::new("tui", "cwd").process();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut request = request(&harness, source.clone(), "permission");
    request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        let _ = event_tx.send(event);
    }));

    let (application, gateway) = harness.runner();
    let turn =
        tokio::spawn(async move { send_framework_turn(application, gateway, request).await });

    loop {
        let event = event_rx.recv().await.expect("gateway event");
        if let GatewayEvent::ActionRequested { action } = event
            && action.kind == GatewayActionKind::Permission
        {
            assert_eq!(action.action_id, "permission-1");
            break;
        }
    }

    let thread_id = harness
        .gateway
        .resolve_source_thread(&source)
        .await
        .expect("permission source lookup")
        .expect("permission source binding");
    let thread = harness
        ._application
        .client()
        .resume_thread(&thread_id)
        .await
        .expect("permission Framework Thread");
    assert!(
        thread
            .respond(
                "permission-1",
                psychevo::InteractionResponse::Permission(
                    PermissionApprovalDecision::allow_once(),
                ),
            )
            .await
            .expect("permission response")
            .accepted
    );
    turn.await.expect("turn task").expect("turn");

    let resolved = event_rx.recv().await.expect("permission resolved event");
    assert!(matches!(
        resolved,
        GatewayEvent::ActionResolved {
            kind: GatewayActionKind::Permission,
            outcome: GatewayActionOutcome::Accepted,
            payload,
            ..
        } if payload["reason"] == "allow_once"
    ));
}

#[tokio::test]
async fn framework_permission_accepts_the_materialized_source_thread() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    backend.request_permission();
    let harness = harness(backend).await;
    let source = GatewaySource::new("tui", "cwd").process();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut request = request(&harness, source.clone(), "permission");
    request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        let _ = event_tx.send(event);
    }));

    let (application, gateway) = harness.runner();
    let turn =
        tokio::spawn(async move { send_framework_turn(application, gateway, request).await });

    loop {
        let event = event_rx.recv().await.expect("gateway event");
        if let GatewayEvent::ActionRequested { action } = event
            && action.kind == GatewayActionKind::Permission
        {
            assert_eq!(action.action_id, "permission-1");
            break;
        }
    }

    let thread_id = harness
        .gateway
        .resolve_source_thread(&source)
        .await
        .expect("permission source lookup")
        .expect("permission materialized Thread");
    let thread = harness
        ._application
        .client()
        .resume_thread(&thread_id)
        .await
        .expect("permission Framework Thread");
    assert!(
        thread
            .respond(
                "permission-1",
                psychevo::InteractionResponse::Permission(
                    PermissionApprovalDecision::allow_once(),
                ),
            )
            .await
            .expect("permission response")
            .accepted
    );
    turn.await.expect("turn task").expect("turn");
}
