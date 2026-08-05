use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::Json;
use axum::http::Response;
use axum::routing::post;
use psychevo::application::{GatewayActivityKind, GatewayActivityTerminalStatus};
use psychevo::{AgentRelationshipStatus, ThreadAgentBinding};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Notify;

use super::super::durable_activity::DurableGatewayActivityClaim;
use super::support_peer::{
    FrameworkNativeProbe, copied_acp_fixture, harness, native_provider_harness, request,
    send_framework_turn_with_id, test_acp_command_toml,
};
use crate::GatewayEventEmitter;
use psychevo_gateway_protocol::events_transcript::{
    GatewayEvent, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntryRole,
};
use psychevo_gateway_protocol::source::{GatewaySource, GatewayThreadSelector};

fn delegated_spawn_agent_sse() -> String {
    let arguments = json!({
        "agent_type": "opencode",
        "task_name": "delegated_child",
        "message": "list tools",
    })
    .to_string();
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call-agent\",\"function\":{{\"name\":\"spawn_agent\",\"arguments\":{}}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(&arguments).expect("spawn_agent arguments")
    )
}

fn delegated_parent_terminal_sse() -> String {
    "data: {\"choices\":[{\"delta\":{\"content\":\"parent done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        .to_string()
}

#[tokio::test]
async fn public_turn_terminal_observes_completed_thread_activity() {
    let backend = Arc::new(FrameworkNativeProbe::default());
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
            let activity = gateway_for_event
                .local_activity_for_selector(&GatewayThreadSelector::thread_id(thread_id));
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
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind parent provider");
    let provider_addr = listener.local_addr().expect("parent provider address");
    let provider_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_requests = Arc::clone(&provider_requests);
    let request_index = Arc::new(AtomicUsize::new(0));
    let next_request_index = Arc::clone(&request_index);
    let parent_response_started = Arc::new(Notify::new());
    let parent_response_started_for_provider = Arc::clone(&parent_response_started);
    let release_parent_response = Arc::new(Notify::new());
    let release_parent_response_for_provider = Arc::clone(&release_parent_response);
    let responses = Arc::new([delegated_spawn_agent_sse(), delegated_parent_terminal_sse()]);
    let provider = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            let captured_requests = Arc::clone(&captured_requests);
            let request_index = Arc::clone(&next_request_index);
            let responses = Arc::clone(&responses);
            let parent_response_started = Arc::clone(&parent_response_started_for_provider);
            let release_parent_response = Arc::clone(&release_parent_response_for_provider);
            async move {
                captured_requests
                    .lock()
                    .expect("parent provider requests")
                    .push(request);
                let index = request_index.fetch_add(1, Ordering::SeqCst);
                if index == 1 {
                    parent_response_started.notify_one();
                    release_parent_response.notified().await;
                }
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        responses
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| "data: [DONE]\n\n".to_string()),
                    ))
                    .expect("parent provider response")
            }
        }),
    );
    let provider_task = tokio::spawn(async move {
        axum::serve(listener, provider)
            .await
            .expect("parent provider");
    });

    let harness = native_provider_harness().await;
    let home = harness._temp.path().join("home");
    let fixture = crate::test_support::acp_fixture(&harness.cwd, "fake_acp_lifecycle");
    let log = harness.cwd.join("delegated-child-activity.jsonl");
    let release = harness.cwd.join("delegated-child.release");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "mock/mock-model"

[provider.mock]
api = "http://{provider_addr}/v1"
no_auth = true

[provider.mock.models."mock-model"]

[agents.backends.fake]
kind = "acp"
command = {}
args = [{}]
entrypoints = ["subagent"]

[agents.backends.fake.env]
ACP_LIFECYCLE_LOG = {}
ACP_LIFECYCLE_MODE = "blocking-prompt"
ACP_LIFECYCLE_RELEASE = {}
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&fixture.script),
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

    let client = harness._application.client();
    let mut parent_request = psychevo::StartThreadRequest::new(&harness.cwd);
    parent_request.source = "web".to_string();
    let parent_thread = client
        .start_thread(parent_request)
        .await
        .expect("parent Thread");
    let parent_thread_id = parent_thread.id().to_string();
    parent_thread
        .set_title("Delegated ACP parent")
        .await
        .expect("parent title");
    let parent_activity = harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: "turn-parent",
            thread_id: Some(&parent_thread_id),
            source_key: None,
            turn_id: Some("turn-parent"),
            kind: GatewayActivityKind::Turn,
            owner_surface: Some("web"),
            queued_turns: 0,
            intent: None,
        })
        .await
        .expect("parent activity");

    let projected = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let projected_for_sink = Arc::clone(&projected);
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "delegated-child").persistent(),
        "delegate the ACP child",
    );
    turn_request.thread_id = Some(parent_thread_id.clone());
    turn_request.runtime_source = Some("web".to_string());
    turn_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        projected_for_sink.lock().expect("events").push(event);
    }));
    let (application, gateway) = harness.runner();
    let mut running = tokio::spawn(async move {
        send_framework_turn_with_id(
            application,
            gateway,
            turn_request,
            "turn-parent".to_string(),
        )
        .await
    });

    tokio::select! {
        early = &mut running => {
            panic!("parent Turn finished before delegated child blocked: {early:?}");
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

    {
        let requests = provider_requests.lock().expect("provider requests");
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]["tools"]
                .as_array()
                .expect("parent tools")
                .iter()
                .any(|tool| tool["function"]["name"] == "spawn_agent"),
            "the real parent model Turn must receive the spawn_agent binding"
        );
    }

    let children = parent_thread
        .agent_children()
        .await
        .expect("parent Agent children");
    assert_eq!(children.len(), 1);
    let relationship = &children[0];
    assert_eq!(relationship.parent_thread_id, parent_thread_id);
    assert_eq!(relationship.status, AgentRelationshipStatus::Open);
    let child_thread_id = relationship.child_thread_id.clone();
    let agent = relationship.agent.as_ref().expect("durable Agent metadata");
    assert_eq!(agent.name.as_deref(), Some("opencode"));
    assert_eq!(agent.task_name.as_deref(), Some("delegated_child"));
    assert_eq!(agent.task.as_deref(), Some("list tools"));
    assert_eq!(agent.role.as_deref(), Some("child"));
    assert_eq!(agent.parent_tool_call_id.as_deref(), Some("call-agent"));
    let child_turn_id = agent.id.clone().expect("Agent run Turn identity");

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
    let child_summary = child.summary().await.expect("child summary");
    assert_eq!(child_summary.source, "peer_agent");
    assert_eq!(
        child_summary.parent_thread_id.as_deref(),
        Some(parent_thread_id.as_str())
    );
    assert_eq!(child_summary.title, None);
    let child_binding = client
        .thread_agent_binding(&child_thread_id)
        .await
        .expect("child Agent binding")
        .expect("resolved child Agent binding");
    let ThreadAgentBinding::Resolved {
        binding: child_binding,
        ..
    } = child_binding
    else {
        panic!("delegated child Agent binding remained unresolved");
    };
    assert_eq!(child_binding.runtime_ref, "acp:fake");
    assert_eq!(child_binding.backend_kind, "acp");
    assert_eq!(child_binding.agent_ref.as_deref(), Some("opencode"));
    let child_activity = child.activity();
    assert!(child_activity.running);
    assert_eq!(
        child_activity.active_turn_id.as_deref(),
        Some(child_turn_id.as_str())
    );
    assert_eq!(child_activity.queued_turns, 0);
    let gateway_child = harness
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&child_thread_id))
        .await;
    assert!(parent.running);
    assert!(
        !gateway_child.running,
        "delegated child must not retain a Gateway shadow activity"
    );
    let parent_framework_activity = parent_thread.activity();
    assert!(parent_framework_activity.running);
    assert_eq!(
        parent_framework_activity.active_turn_id.as_deref(),
        Some("turn-parent")
    );

    std::fs::write(&release, "release").expect("release child");
    tokio::time::timeout(Duration::from_secs(3), parent_response_started.notified())
        .await
        .expect("parent continuation started after child terminal");

    let child_activity = child.activity();
    assert!(!child_activity.running);
    assert_eq!(child_activity.active_turn_id, None);
    assert_eq!(child_activity.queued_turns, 0);
    let closed_relationship = child
        .agent_relationship()
        .await
        .expect("child Agent relationship")
        .expect("durable child Agent relationship");
    assert_eq!(closed_relationship.parent_thread_id, parent_thread_id);
    assert_eq!(closed_relationship.child_thread_id, child_thread_id);
    assert_eq!(closed_relationship.status, AgentRelationshipStatus::Closed);
    assert!(
        parent_thread.activity().running,
        "the parent Framework Turn must remain active until its post-child model continuation"
    );
    assert!(
        harness
            .gateway
            .activity_for_selector(GatewayThreadSelector::thread_id(&parent_thread_id))
            .await
            .running,
        "the child terminal must not finish its parent's Gateway activity"
    );
    assert!(
        !projected
            .lock()
            .expect("projected events")
            .iter()
            .any(|event| matches!(
                event,
                GatewayEvent::TurnCompleted {
                    thread_id: Some(thread_id),
                    ..
                } if thread_id == &parent_thread_id
            )),
        "the parent terminal must follow the delegated child terminal"
    );

    release_parent_response.notify_one();
    let result = running.await.expect("parent task").expect("parent result");
    assert_eq!(result.result.thread_id, parent_thread_id);
    assert_eq!(result.result.final_answer, "parent done");
    {
        let requests = provider_requests.lock().expect("provider requests");
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1]["messages"]
                .as_array()
                .expect("parent continuation messages")
                .iter()
                .any(|message| {
                    message["role"] == "tool" && message["tool_call_id"] == "call-agent"
                }),
            "the parent continuation must consume the real spawn_agent result"
        );
    }

    let child_entries = projected
        .lock()
        .expect("projected events")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::EntryStarted { turn_id, entry }
            | GatewayEvent::EntryUpdated { turn_id, entry }
            | GatewayEvent::EntryCompleted { turn_id, entry }
                if entry.thread_id == child_thread_id =>
            {
                Some(turn_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!child_entries.is_empty());
    assert!(
        child_entries
            .iter()
            .all(|turn_id| turn_id == &child_turn_id)
    );
    let terminal_threads = projected
        .lock()
        .expect("projected events")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id),
                ..
            } => Some(thread_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(terminal_threads.last(), Some(&parent_thread_id));

    harness
        .gateway
        .finish_durable_gateway_activity(
            Some(&parent_activity),
            GatewayActivityTerminalStatus::Completed,
        )
        .await
        .expect("finish parent activity");
    harness
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
    provider_task.abort();
}

#[tokio::test]
async fn acp_peer_agent_turn_routes_to_backend_and_persists_native_session() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_session_persistence",
        "fake_acp",
    );
    let script = fixture.script;
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = {}
args = [{}]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]

[agents.backends.fake.env]
PSYCHEVO_BINDING_DB = {}
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script),
            crate::test_support::toml_path(&harness.db_path),
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
    first_request.runtime_source = Some("web".to_string());
    first_request.policy.agent_ref = Some("reviewer".to_string());
    first_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    first_request.policy.inherited_env = Some(env.clone());
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

    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("binding lookup"),
        Some(first.result.thread_id.clone())
    );
    let runtime_binding = harness
        ._application
        .client()
        .thread_agent_binding(&first.result.thread_id)
        .await
        .expect("runtime binding lookup")
        .expect("runtime binding");
    let ThreadAgentBinding::Resolved {
        binding: runtime_binding,
        ..
    } = runtime_binding
    else {
        panic!("runtime binding remained unresolved");
    };
    assert_eq!(runtime_binding.backend_kind, "acp");
    assert_eq!(
        runtime_binding.native_session_id.as_deref(),
        Some("native-1")
    );
    let transcript = harness
        .gateway
        .thread_transcript(&first.result.thread_id)
        .await
        .expect("transcript");
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].role, TranscriptEntryRole::User);
    assert_eq!(transcript[1].role, TranscriptEntryRole::Assistant);
    let summary = harness
        ._application
        .client()
        .thread_summary(&first.result.thread_id)
        .await
        .expect("session summary")
        .expect("summary");
    assert_eq!(summary.title.as_deref(), Some("hello"));

    let mut second_request = request(&harness, source.clone(), "again");
    second_request.policy.agent_ref = Some("reviewer".to_string());
    second_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    second_request.policy.inherited_env = Some(env.clone());
    let second = harness
        .send(second_request)
        .await
        .expect("second peer turn");
    assert_eq!(second.result.thread_id, first.result.thread_id);
    assert!(second.result.final_answer.contains("new:native-1"));
    assert!(
        !second.result.final_answer.contains("Peer instructions."),
        "captured Agent instructions are sent once per logical Thread"
    );
    assert!(
        !second
            .result
            .final_answer
            .contains("old answer from loaded history")
    );
    let mut process_counter_path = script.as_os_str().to_os_string();
    process_counter_path.push(".processes");
    assert_eq!(
        std::fs::read_to_string(PathBuf::from(process_counter_path)).expect("ACP process counter"),
        "1",
        "two turns on one thread must reuse one resident ACP process"
    );

    let mut top_level_start = psychevo::StartThreadRequest::new(&harness.cwd);
    top_level_start.source = "peer_agent".to_string();
    let top_level_thread_id = harness
        ._application
        .client()
        .start_thread(top_level_start)
        .await
        .expect("top-level peer Thread")
        .id()
        .to_string();
    let mut top_level_request = request(&harness, source, "child prompt");
    top_level_request.thread_id = Some(top_level_thread_id.clone());
    top_level_request.policy.agent_ref = Some("reviewer".to_string());
    top_level_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    top_level_request.policy.inherited_env = Some(env);
    let top_level = harness
        .send(top_level_request)
        .await
        .expect("top-level peer turn");
    assert_eq!(top_level.result.thread_id, top_level_thread_id);
    let top_level_summary = harness
        ._application
        .client()
        .thread_summary(&top_level.result.thread_id)
        .await
        .expect("top-level summary")
        .expect("top-level Thread");
    assert_eq!(top_level_summary.title.as_deref(), Some("child prompt"));
}

#[tokio::test]
async fn acp_peer_agent_streams_standard_session_updates_to_gateway_events() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let home = harness._temp.path().join("home");
    let fixture = copied_acp_fixture(
        &harness.cwd,
        harness._temp.path(),
        "fake_acp_stream_updates",
        "fake_acp_stream",
    );
    let script = fixture.script;
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"[agents.backends.fake]
kind = "acp"
description = "Fake ACP agent."
command = {}
args = [{}]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
            test_acp_command_toml(&harness.cwd),
            crate::test_support::toml_path(&script)
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
    let turn_events = Arc::new(Mutex::new(Vec::<psychevo::TurnEvent>::new()));
    let turn_events_for_sink = Arc::clone(&turn_events);
    let source = GatewaySource::new("web", "peer-stream").persistent();
    let mut first_request = request(&harness, source, "hello");
    first_request.runtime_source = Some("web".to_string());
    first_request.policy.agent_ref = Some("reviewer".to_string());
    first_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    first_request.policy.inherited_env = Some(env.clone());
    first_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        gateway_events_for_sink
            .lock()
            .expect("gateway events lock")
            .push(event);
    }));
    first_request.turn_events = Some(Arc::new(move |event| {
        turn_events_for_sink
            .lock()
            .expect("raw events lock")
            .push(event);
    }));

    let result = harness
        .send(first_request)
        .await
        .expect("streaming peer turn");

    assert_eq!(result.result.final_answer, "hello world");
    let runtime_event_values = turn_events
        .lock()
        .expect("Turn events lock")
        .iter()
        .filter_map(|event| match event {
            psychevo::TurnEvent::Runtime { data } => Some(data.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        runtime_event_values
            .iter()
            .any(|event| event["update_kind"] == "available_commands_update"),
        "available commands update should be retained as a structured ACP event"
    );
    assert!(
        runtime_event_values
            .iter()
            .any(|event| event["update_kind"] == "session_info_update"),
        "session info update should be retained as a structured ACP event"
    );

    {
        let turn_events = turn_events.lock().expect("Turn events lock");
        assert!(
            turn_events.iter().any(|event| matches!(
                event,
                psychevo::TurnEvent::Runtime { data }
                    if data["type"] == "acp_peer_session_update"
                        && data["update_kind"] == "tool_call_update"
            )),
            "typed Turn events should retain ACP tool updates"
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
                block.kind == TranscriptBlockKind::Status && block.title.as_deref() == Some("Plan")
            })
            .map(|block| (*block).clone())
            .collect::<Vec<_>>()
    };
    assert!(
        live_plans.len() >= 2,
        "each ACP plan update should be observable"
    );
    let live_plan = live_plans.last().expect("latest live plan");
    assert!(
        live_plans.iter().all(|plan| plan.id == live_plan.id),
        "replacement plan updates must retain one logical block identity"
    );
    assert_eq!(
        live_plan.id,
        format!("turn:{}:acp-peer-plan", result.turn.id)
    );
    assert!(
        live_plan.body.as_deref().is_some_and(
            |body| body.contains("Verify terminal history") && !body.contains("Inspect repo")
        ),
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
        ._application
        .client()
        .thread_summary(&result.result.thread_id)
        .await
        .expect("session summary")
        .expect("summary");
    assert_eq!(summary.title.as_deref(), Some("ACP streamed title"));
    let transcript = harness
        .gateway
        .thread_transcript(&result.result.thread_id)
        .await
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
        ._application
        .client()
        .resume_thread(&result.result.thread_id)
        .await
        .expect("history Thread")
        .history()
        .latest(Some(200))
        .await
        .expect("stored messages")
        .items;
    let stored_assistant = summaries
        .iter()
        .find(|summary| {
            matches!(
                summary.message,
                psychevo::application::Message::Assistant { .. }
            )
        })
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
    let usage_summary = harness
        ._application
        .client()
        .resume_thread(&result.result.thread_id)
        .await
        .expect("usage Thread")
        .usage_summary()
        .await
        .expect("session usage");
    assert_eq!(usage_summary.effective_total_tokens, Some(144));
    assert_eq!(usage_summary.total_status, "reported");
    let psychevo::application::Message::Assistant { content, .. } = &stored_assistant.message
    else {
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
    second_request.policy.agent_ref = Some("reviewer".to_string());
    second_request.policy.runtime_profile_ref = Some("acp:fake".to_string());
    second_request.policy.inherited_env = Some(env);
    let second_result = harness
        .send(second_request)
        .await
        .expect("second streaming peer turn");
    assert_eq!(second_result.result.thread_id, result.result.thread_id);

    let summaries = harness
        ._application
        .client()
        .resume_thread(&result.result.thread_id)
        .await
        .expect("history Thread after second turn")
        .history()
        .latest(Some(200))
        .await
        .expect("stored messages after second turn")
        .items;
    let stored_assistants = summaries
        .iter()
        .filter(|summary| {
            matches!(
                summary.message,
                psychevo::application::Message::Assistant { .. }
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
    let usage_summary = harness
        ._application
        .client()
        .resume_thread(&result.result.thread_id)
        .await
        .expect("usage Thread after cumulative update")
        .usage_summary()
        .await
        .expect("session usage after cumulative ACP update");
    assert_eq!(usage_summary.effective_total_tokens, Some(200));
    assert_eq!(usage_summary.reported_total_tokens, 200);
    assert_eq!(usage_summary.total_status, "reported");
}
