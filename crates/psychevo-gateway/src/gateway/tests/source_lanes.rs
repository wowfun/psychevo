use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psychevo::RunMode;
use psychevo::application::GatewayActivityKind;
use serde_json::json;
use tokio::sync::oneshot;

use super::super::activity::{SendShellRequest, ShellExecutionIntent};
use super::super::durable_activity::DurableGatewayActivityClaim;
use super::support_peer::{
    FrameworkNativeProbe, Harness, compose_test_application, harness, request, send_framework_turn,
};
use crate::{GatewayEventEmitter, gateway_now_ms};
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayEvent, PendingActionView, TranscriptBlockKind, TranscriptBlockStatus,
};
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewaySource, GatewayThreadSelector,
};

async fn thread_count(harness: &Harness, source: &str) -> usize {
    harness
        ._application
        .client()
        .list_threads(psychevo::ThreadListQuery {
            cwd: Some(harness.cwd.clone()),
            archived: false,
            sources: vec![source.to_string()],
            cursor: None,
            limit: 100,
        })
        .await
        .expect("Thread list")
        .threads
        .len()
}

async fn start_thread(harness: &Harness, source: &str) -> String {
    let mut request = psychevo::StartThreadRequest::new(&harness.cwd);
    request.source = source.to_string();
    harness
        ._application
        .client()
        .start_thread(request)
        .await
        .expect("Framework Thread")
        .id()
        .to_string()
}

async fn start_bound_thread(harness: &Harness, source: &str) -> String {
    let mut request = psychevo::StartThreadRequest::new(&harness.cwd);
    request.source = source.to_string();
    let thread = harness
        ._application
        .client()
        .start_thread(request)
        .await
        .expect("Framework Thread");
    let thread_id = thread.id().to_string();
    thread
        .start_turn(psychevo::TurnRequest::new("establish immutable binding"))
        .await
        .expect("bound Turn")
        .wait()
        .await
        .expect("bound Turn completion");
    thread_id
}

#[tokio::test]
async fn invocation_source_does_not_bind_or_reuse() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("cli", "run-1").invocation();

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");
    let second = harness
        .send(request(&harness, source.clone(), "second"))
        .await
        .expect("second turn");

    assert_ne!(first.result.thread_id, second.result.thread_id);
    assert!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("binding lookup")
            .is_none()
    );
    assert_eq!(
        backend.runs()[1].session.as_deref(),
        Some(second.result.thread_id.as_str()),
        "the immutable public thread is materialized before backend dispatch"
    );
}

#[tokio::test]
async fn invocation_source_continue_latest_reuses_matching_public_thread() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;

    let mut initial = request(
        &harness,
        GatewaySource::new("cli", "run-1").invocation(),
        "first",
    );
    initial.cwd = harness.cwd.join("..").join("work");
    let first = harness.send(initial).await.expect("first turn");
    let mut continued = request(
        &harness,
        GatewaySource::new("cli", "run-2").invocation(),
        "second",
    );
    continued.policy.continue_latest = true;
    let second = harness.send(continued).await.expect("continued turn");

    assert_eq!(second.result.thread_id, first.result.thread_id);
    assert_eq!(
        backend.runs()[1].session.as_deref(),
        Some(first.result.thread_id.as_str())
    );
    assert_eq!(thread_count(&harness, "test").await, 1);
}

#[tokio::test]
async fn process_source_reuses_only_within_gateway_instance() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("tui", "cwd").process();

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");
    let second = harness
        .send(request(&harness, source.clone(), "second"))
        .await
        .expect("second turn");
    let (rebuilt_application, rebuilt_gateway) =
        compose_test_application(&harness, backend.executor()).await;
    let rebuilt_gateway_probe = rebuilt_gateway.clone();
    let third = send_framework_turn(
        rebuilt_application,
        rebuilt_gateway,
        request(&harness, source.clone(), "third"),
    )
    .await
    .expect("third turn");

    assert_eq!(first.result.thread_id, second.result.thread_id);
    assert_ne!(first.result.thread_id, third.result.thread_id);
    assert_eq!(
        backend.runs()[1].session.as_deref(),
        Some(first.result.thread_id.as_str())
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("original Gateway binding lookup"),
        Some(first.result.thread_id)
    );
    assert_eq!(
        rebuilt_gateway_probe
            .resolve_source_thread(&source)
            .await
            .expect("rebuilt Gateway binding lookup"),
        Some(third.result.thread_id)
    );
}
#[tokio::test]
async fn persistent_source_round_trips_through_store() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("acp", "client-session").persistent();

    assert_eq!(thread_count(&harness, "test").await, 0);

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");
    let (rebuilt_application, rebuilt_gateway) =
        compose_test_application(&harness, backend.executor()).await;
    let second = send_framework_turn(
        rebuilt_application,
        rebuilt_gateway,
        request(&harness, source.clone(), "second"),
    )
    .await
    .expect("second turn");

    assert_eq!(first.result.thread_id, second.result.thread_id);
    assert_eq!(
        backend.runs()[0].session.as_deref(),
        Some(first.result.thread_id.as_str()),
        "the immutable public thread is materialized before backend dispatch"
    );
    assert_eq!(
        backend.runs()[1].session.as_deref(),
        Some(first.result.thread_id.as_str())
    );
    assert_eq!(thread_count(&harness, "test").await, 1);
    let lane = harness
        .durability
        .gateway_source_lane(&source.source_key().0)
        .await
        .expect("lane lookup")
        .expect("lane");
    assert_eq!(
        lane.thread_id.as_deref(),
        Some(first.result.thread_id.as_str())
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("source binding"),
        Some(first.result.thread_id)
    );
}

#[tokio::test]
async fn bound_thread_uses_stored_cwd_over_request_default() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("im.wechat", "remote-lane").persistent();
    let changed_default = harness
        .cwd
        .parent()
        .expect("temp root")
        .join("changed-default");
    std::fs::create_dir_all(&changed_default).expect("changed default cwd");

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");
    let mut second_request = request(&harness, source, "second");
    second_request.cwd = changed_default.clone();
    let second = harness.send(second_request).await.expect("second turn");

    assert_eq!(first.result.thread_id, second.result.thread_id);
    let runs = backend.runs();
    assert_eq!(
        runs[1].session.as_deref(),
        Some(first.result.thread_id.as_str())
    );
    assert_eq!(runs[0].cwd, harness.cwd);
    assert_eq!(runs[1].cwd, harness.cwd);
    assert_ne!(runs[1].cwd, changed_default);
}

#[tokio::test]
async fn channel_connection_rotation_starts_next_turn_in_changed_default_cwd() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("im.wechat", "remote-lane")
        .persistent()
        .with_raw_identity(json!({
            "connectionId": "wechat",
            "chatId": "remote-lane",
        }));
    let other_source = GatewaySource::new("im.telegram", "remote-lane")
        .persistent()
        .with_raw_identity(json!({
            "connectionId": "telegram",
            "chatId": "remote-lane",
        }));
    let changed_default = harness
        .cwd
        .parent()
        .expect("temp root")
        .join("changed-default");
    std::fs::create_dir_all(&changed_default).expect("changed default cwd");

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");
    let other = harness
        .send(request(&harness, other_source.clone(), "other"))
        .await
        .expect("other turn");

    assert_eq!(
        harness
            .gateway
            .rotate_channel_connection_sources("wechat")
            .await
            .expect("rotate wechat"),
        1
    );
    assert!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("binding lookup")
            .is_none()
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&other_source)
            .await
            .expect("other binding lookup"),
        Some(other.result.thread_id)
    );
    let old_summary = harness
        ._application
        .client()
        .thread_summary(&first.result.thread_id)
        .await
        .expect("old summary")
        .expect("old session");
    assert_eq!(
        old_summary.end_reason.as_deref(),
        Some("channel_workspace_changed")
    );
    assert!(old_summary.archived_at_ms.is_some());

    let mut second_request = request(&harness, source.clone(), "second");
    second_request.cwd = changed_default.clone();
    let second = harness.send(second_request).await.expect("second turn");

    assert_ne!(first.result.thread_id, second.result.thread_id);
    assert_eq!(
        backend.runs().last().expect("last run").cwd,
        changed_default
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("new binding lookup"),
        Some(second.result.thread_id)
    );
}

#[tokio::test]
async fn channel_connection_rotation_waits_for_running_bound_turn() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("im.wechat", "remote-lane")
        .persistent()
        .with_raw_identity(json!({
            "connectionId": "wechat",
            "chatId": "remote-lane",
        }));
    let changed_default = harness
        .cwd
        .parent()
        .expect("temp root")
        .join("changed-default");
    std::fs::create_dir_all(&changed_default).expect("changed default cwd");

    let first = harness
        .send(request(&harness, source.clone(), "first"))
        .await
        .expect("first turn");

    let wait = backend.wait_on_next_run();
    let (second_application, second_gateway) = harness.runner();
    let second_request = request(&harness, source.clone(), "second-running");
    let second = tokio::spawn(async move {
        send_framework_turn(second_application, second_gateway, second_request).await
    });
    wait.started.notified().await;

    let rotating_gateway = harness.gateway.clone();
    let rotation = tokio::spawn(async move {
        rotating_gateway
            .rotate_channel_connection_sources("wechat")
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !rotation.is_finished(),
        "channel rotation must wait for the active bound Turn"
    );

    tokio::task::yield_now().await;
    assert_eq!(
        backend
            .runs()
            .into_iter()
            .map(|run| run.prompt)
            .collect::<Vec<_>>(),
        vec!["first".to_string(), "second-running".to_string()]
    );

    wait.release.notify_one();
    let second = second.await.expect("second task").expect("second turn");
    assert_eq!(
        rotation
            .await
            .expect("rotation task")
            .expect("rotate wechat"),
        1
    );
    let (third_application, third_gateway) = harness.runner();
    let mut third_request = request(&harness, source.clone(), "third-new-cwd");
    third_request.cwd = changed_default.clone();
    let third = tokio::spawn(async move {
        send_framework_turn(third_application, third_gateway, third_request).await
    });
    let third = third.await.expect("third task").expect("third turn");

    assert_eq!(first.result.thread_id, second.result.thread_id);
    assert_ne!(first.result.thread_id, third.result.thread_id);
    let runs = backend.runs();
    assert_eq!(runs[1].cwd, harness.cwd);
    assert_eq!(runs[2].cwd, changed_default);
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("new binding lookup"),
        Some(third.result.thread_id)
    );
}

#[tokio::test]
async fn first_shell_without_bound_source_creates_and_binds_runtime_session() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let source = GatewaySource::new("web", "cwd").persistent();
    let environment = configured_shell_environment(&harness);
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured_events = Arc::clone(&events);

    let result = harness
        .gateway
        .send_shell(SendShellRequest {
            thread_id: None,
            source: Some(source.clone()),
            bind_source: None,
            cwd: harness.cwd.clone(),
            command: "printf shell-ok".to_string(),
            execution: ShellExecutionIntent::new("web").inherited_environment(environment),
            event_sink: Some(GatewayEventEmitter::new(move |event| {
                captured_events.lock().expect("shell events").push(event);
            })),
            lineage: None,
        })
        .await
        .expect("shell");

    let session_id = result.result.thread_id.expect("shell session");
    assert_eq!(result.thread.id, session_id);
    assert_eq!(
        result.result.outcome,
        psychevo::ShellCommandOutcome::Completed
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&source)
            .await
            .expect("binding lookup"),
        Some(session_id.clone())
    );
    assert_eq!(thread_count(&harness, "web").await, 1);
    let events = events.lock().expect("shell events");
    let projected = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                GatewayEvent::EntryStarted { entry, .. }
                    | GatewayEvent::EntryCompleted { entry, .. }
                    if entry
                        .blocks
                        .iter()
                        .any(|block| block.kind == TranscriptBlockKind::Shell)
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        projected.as_slice(),
        [
            GatewayEvent::EntryStarted { entry: started, .. },
            GatewayEvent::EntryCompleted {
                entry: completed,
                ..
            }
        ] if started.thread_id == session_id
            && completed.thread_id == session_id
            && started.blocks[0].status == TranscriptBlockStatus::Running
            && completed.blocks[0].status == TranscriptBlockStatus::Completed
    ));
}

#[tokio::test]
async fn shell_execution_intent_preserves_continuation_model_mode_and_environment() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let environment = configured_shell_environment(&harness);
    let execution = || {
        ShellExecutionIntent::new("shell-policy")
            .continue_latest(["shell-policy".to_string()])
            .model(
                Some("lmstudio/alternate".to_string()),
                Some("high".to_string()),
            )
            .mode(RunMode::Plan)
            .inherited_environment(environment.clone())
    };

    let first = harness
        .gateway
        .send_shell(SendShellRequest {
            thread_id: None,
            source: None,
            bind_source: None,
            cwd: harness.cwd.clone(),
            command: "printf '%s' \"$PSYCHEVO_SHELL_MARKER\"".to_string(),
            execution: execution(),
            event_sink: None,
            lineage: None,
        })
        .await
        .expect("first shell");
    let first_thread_id = first.result.thread_id.clone().expect("first shell thread");
    assert_eq!(first.result.output["output"], "intent-env");

    let second = harness
        .gateway
        .send_shell(SendShellRequest {
            thread_id: None,
            source: None,
            bind_source: None,
            cwd: harness.cwd.clone(),
            command: "printf continued".to_string(),
            execution: execution(),
            event_sink: None,
            lineage: None,
        })
        .await
        .expect("continued shell");
    assert_eq!(
        second.result.thread_id.as_deref(),
        Some(first_thread_id.as_str())
    );

    let summary = harness
        ._application
        .client()
        .resume_thread(first_thread_id.clone())
        .await
        .expect("resume shell thread")
        .summary()
        .await
        .expect("shell summary");
    assert_eq!(summary.source, "shell-policy");
    assert_eq!(summary.provider, "lmstudio");
    assert_eq!(summary.model, "alternate");
    let selection = harness
        ._application
        .client()
        .thread_model_selection(&first_thread_id)
        .await
        .expect("shell model selection")
        .expect("persisted shell model selection");
    assert_eq!(selection.reasoning_effort.as_deref(), Some("high"));
}

fn configured_shell_environment(harness: &Harness) -> BTreeMap<String, String> {
    let root = harness.cwd.parent().expect("temp root");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        r#"
model = "lmstudio/test-model"

[provider.lmstudio.models.test-model]

[provider.lmstudio.models.alternate]
"#,
    )
    .expect("config");
    BTreeMap::from([
        ("HOME".to_string(), root.to_string_lossy().to_string()),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_SHELL_MARKER".to_string(),
            "intent-env".to_string(),
        ),
    ])
}

#[tokio::test]
async fn send_turn_serializes_same_source_fifo() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let wait = backend.wait_on_first_run();
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("tui", "cwd").process();

    let (first_application, first_gateway) = harness.runner();
    let first_request = request(&harness, source.clone(), "first");
    let first = tokio::spawn(async move {
        send_framework_turn(first_application, first_gateway, first_request).await
    });
    wait.started.notified().await;

    let (second_application, second_gateway) = harness.runner();
    let second_request = request(&harness, source.clone(), "second");
    let second = tokio::spawn(async move {
        send_framework_turn(second_application, second_gateway, second_request).await
    });

    tokio::task::yield_now().await;
    assert_eq!(
        backend
            .runs()
            .into_iter()
            .map(|run| run.prompt)
            .collect::<Vec<_>>(),
        vec!["first".to_string()]
    );

    wait.release.notify_one();
    let first = first.await.expect("first task").expect("first turn");
    let second = second.await.expect("second task").expect("second turn");
    assert_eq!(first.result.thread_id, second.result.thread_id);
    assert_eq!(
        backend
            .runs()
            .into_iter()
            .map(|run| run.prompt)
            .collect::<Vec<_>>(),
        vec!["first".to_string(), "second".to_string()]
    );
}

#[tokio::test]
async fn draft_source_lane_runs_while_previous_unbound_source_turn_finishes_later() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let wait = backend.wait_on_first_run();
    let harness = harness(backend.clone()).await;
    let canonical = GatewaySource::new("web", "cwd").persistent();
    let draft = GatewaySource::new("web", "cwd:draft:test").persistent();

    let (first_application, first_gateway) = harness.runner();
    let first_request = request(&harness, canonical.clone(), "first");
    let first = tokio::spawn(async move {
        send_framework_turn(first_application, first_gateway, first_request).await
    });
    wait.started.notified().await;

    harness
        .gateway
        .clear_source_binding(&canonical)
        .await
        .expect("draft open clears canonical binding");

    let mut second_request = request(&harness, draft, "second");
    second_request.bind_source = Some(canonical.clone());
    let second = harness
        .send(second_request)
        .await
        .expect("second draft turn");

    assert_eq!(
        backend
            .runs()
            .into_iter()
            .map(|run| run.prompt)
            .collect::<Vec<_>>(),
        vec!["first".to_string(), "second".to_string()]
    );
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&canonical)
            .await
            .expect("binding lookup"),
        Some(second.result.thread_id.clone())
    );

    wait.release.notify_one();
    let first = first.await.expect("first task").expect("first turn");

    assert_ne!(first.result.thread_id, second.result.thread_id);
    assert_eq!(
        harness
            .gateway
            .resolve_source_thread(&canonical)
            .await
            .expect("binding lookup"),
        Some(second.result.thread_id)
    );
}

#[tokio::test]
async fn explicit_thread_turn_allows_source_rebind_while_running() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let wait = backend.wait_on_first_run();
    let harness = harness(backend).await;
    let source = GatewaySource::new("web", "cwd").persistent();
    let first = start_thread(&harness, "web").await;
    let second = start_thread(&harness, "web").await;
    harness
        .gateway
        .bind_source_thread(
            &source,
            &first,
            &GatewayBackendInfo {
                kind: BackendKind::Native,
                runtime_ref: Some("native".to_string()),
                native_id: Some(first.clone()),
            },
            None,
        )
        .await
        .expect("bind first");

    let mut first_request = request(&harness, source.clone(), "first");
    first_request.thread_id = Some(first.clone());
    let (application, gateway) = harness.runner();
    let running =
        tokio::spawn(async move { send_framework_turn(application, gateway, first_request).await });
    wait.started.notified().await;

    harness
        .gateway
        .bind_source_thread(
            &source,
            &second,
            &GatewayBackendInfo {
                kind: BackendKind::Native,
                runtime_ref: Some("native".to_string()),
                native_id: Some(second.clone()),
            },
            None,
        )
        .await
        .expect("bind second");

    assert!(
        harness
            ._application
            .client()
            .resume_thread(&first)
            .await
            .expect("running Framework Thread")
            .activity()
            .running
    );
    assert!(
        !harness
            .gateway
            .local_activity_for_selector(&GatewayThreadSelector::source(source.source_key()))
            .running
    );

    wait.release.notify_one();
    running
        .await
        .expect("running task")
        .expect("running result");
}

#[tokio::test]
async fn durable_activity_does_not_rebind_parent_turn_to_scoped_child_turn_started() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let turn_id = "turn-parent";
    let parent_thread = start_thread(&harness, "web").await;
    let child_thread = start_thread(&harness, "agent").await;

    let activity = harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: turn_id,
            thread_id: None,
            source_key: Some("web:test"),
            turn_id: Some(turn_id),
            kind: GatewayActivityKind::Turn,
            owner_surface: Some("web"),
            queued_turns: 0,
            intent: Some(json!({
                "kind": "turn",
                "threadId": parent_thread.clone(),
            })),
        })
        .await
        .expect("claim activity");
    assert!(
        harness
            .durability
            .update_gateway_activity_thread(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                &parent_thread,
                gateway_now_ms() + 30_000,
            )
            .await
            .expect("parent turn started")
    );
    assert!(
        !harness
            .durability
            .update_gateway_activity_thread(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                &child_thread,
                gateway_now_ms() + 30_000,
            )
            .await
            .expect("scoped child turn started")
    );

    let record = harness
        .durability
        .gateway_activity(turn_id)
        .await
        .expect("activity lookup")
        .expect("activity");
    assert_eq!(record.thread_id.as_deref(), Some(parent_thread.as_str()));
}

#[tokio::test]
async fn gateway_event_ingress_attributes_child_interaction_before_durable_publish() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let parent_thread = start_bound_thread(&harness, "web").await;
    let child_thread = start_bound_thread(&harness, "agent").await;
    let parent_activity = harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: "turn-parent",
            thread_id: Some(&parent_thread),
            source_key: None,
            turn_id: Some("turn-parent"),
            kind: GatewayActivityKind::Turn,
            owner_surface: Some("web"),
            queued_turns: 0,
            intent: None,
        })
        .await
        .expect("parent activity");
    harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: "turn-child",
            thread_id: Some(&child_thread),
            source_key: None,
            turn_id: Some("turn-child"),
            kind: GatewayActivityKind::Turn,
            owner_surface: Some("agent"),
            queued_turns: 0,
            intent: None,
        })
        .await
        .expect("child activity");
    let observed = Arc::new(Mutex::new(None));
    let observed_for_sink = Arc::clone(&observed);
    let downstream = GatewayEventEmitter::new(move |event| {
        *observed_for_sink.lock().expect("observed event") = Some(event);
    });
    let sink = harness
        .gateway
        .wrap_gateway_event_sink(
            Some(downstream),
            Some(parent_activity),
            Some("thread:parent".to_string()),
            Some("turn-parent".to_string()),
        )
        .expect("wrapped sink");

    sink.emit(GatewayEvent::ActionRequested {
        action: PendingActionView {
            action_id: "permission-child".to_string(),
            kind: GatewayActionKind::Permission,
            title: Some("child permission".to_string()),
            summary: None,
            payload: json!({}),
            thread_id: Some(child_thread),
            turn_id: Some("turn-child".to_string()),
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        },
    })
    .expect("child action event");
    harness.gateway.wait_for_gateway_events().await;

    let event = observed
        .lock()
        .expect("observed event")
        .clone()
        .expect("event");
    let GatewayEvent::ActionRequested { action } = event else {
        panic!("expected child action");
    };
    assert_eq!(
        action.activity_id, None,
        "local-first delivery must not block on durable provenance lookup"
    );
    assert_eq!(action.turn_id.as_deref(), Some("turn-child"));

    let persisted = harness
        .durability
        .list_gateway_live_events_after(0, 10)
        .await
        .expect("persisted gateway events");
    let persisted_event: GatewayEvent = serde_json::from_value(
        persisted
            .last()
            .expect("persisted child action")
            .event
            .clone(),
    )
    .expect("persisted child action event");
    let GatewayEvent::ActionRequested { action } = persisted_event else {
        panic!("expected persisted child action");
    };
    assert_eq!(action.activity_id.as_deref(), Some("turn-child"));
    assert_eq!(action.turn_id.as_deref(), Some("turn-child"));
}

#[tokio::test]
async fn gateway_event_ingress_rejects_action_without_a_resolved_runtime_binding() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let thread_id = start_thread(&harness, "web").await;
    let activity = harness
        .gateway
        .claim_durable_gateway_activity(DurableGatewayActivityClaim {
            activity_id: "turn-unbound-action",
            thread_id: Some(&thread_id),
            source_key: None,
            turn_id: Some("turn-unbound-action"),
            kind: GatewayActivityKind::Turn,
            owner_surface: Some("web"),
            queued_turns: 0,
            intent: None,
        })
        .await
        .expect("unbound activity");
    let sink = harness
        .gateway
        .wrap_gateway_event_sink(
            None,
            Some(activity),
            None,
            Some("turn-unbound-action".to_string()),
        )
        .expect("wrapped sink");

    sink.emit(GatewayEvent::ActionRequested {
        action: PendingActionView {
            action_id: "permission-unbound".to_string(),
            kind: GatewayActionKind::Permission,
            title: None,
            summary: None,
            payload: json!({}),
            thread_id: Some(thread_id),
            turn_id: Some("turn-unbound-action".to_string()),
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        },
    })
    .expect("bounded event admission");
    let error = harness
        .gateway
        .event_ingress
        .fence()
        .await
        .expect_err("missing binding must fail retained-live persistence")
        .to_string();
    assert!(error.contains("no resolved immutable runtime binding"));
    let diagnostics = harness.gateway.event_ingress_diagnostics();
    assert_eq!(diagnostics.committed, 0);
    assert_eq!(diagnostics.failed, 1);
    assert!(
        harness
            .durability
            .list_gateway_live_events_after(0, 10)
            .await
            .expect("retained events")
            .is_empty(),
        "missing binding must not be persisted as native provenance"
    );
}

#[tokio::test]
async fn draft_mutation_guard_serializes_only_the_same_source() {
    let harness = harness(Arc::new(FrameworkNativeProbe::default())).await;
    let source = GatewaySource::new("web", "same-source").persistent();
    let other = GatewaySource::new("web", "other-source").persistent();
    let first_guard = harness
        .gateway
        .lock_source_mutation(&source.source_key())
        .await;

    let other_guard = tokio::time::timeout(
        Duration::from_secs(1),
        harness.gateway.lock_source_mutation(&other.source_key()),
    )
    .await
    .expect("an unrelated source must remain concurrent");
    drop(other_guard);

    let gateway = harness.gateway.clone();
    let (started_tx, started_rx) = oneshot::channel();
    let (acquired_tx, mut acquired_rx) = oneshot::channel();
    let queued_source = source.clone();
    let queued = tokio::spawn(async move {
        let _ = started_tx.send(());
        let _guard = gateway
            .lock_source_mutation(&queued_source.source_key())
            .await;
        let _ = acquired_tx.send(());
    });
    started_rx.await.expect("queued mutation started");
    assert!(matches!(
        acquired_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    drop(first_guard);
    tokio::time::timeout(Duration::from_secs(1), acquired_rx)
        .await
        .expect("same-source mutation must proceed after release")
        .expect("queued mutation acquired guard");
    queued.await.expect("queued mutation task");
}
