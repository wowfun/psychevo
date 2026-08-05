use std::collections::BTreeSet;
use std::path::Path;

use psychevo::application::{
    GatewayActivityClaimInput, GatewayActivityKind, Message as RuntimeMessage, UserContentBlock,
};
use psychevo::paths::canonicalize_cwd;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::gateway::activity::GatewayActivity;
use crate::gateway_now_ms;
use crate::server::binding::{AuthContext, WebState, merge_framework_activity};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::tests::helpers::{
    framework_message_fixture_executor, web_state, web_state_with_native_test_executor,
};

async fn start_thread(
    state: &WebState,
    cwd: &Path,
    source: &str,
    metadata: Option<Value>,
) -> psychevo::Thread {
    let mut request = psychevo::StartThreadRequest::new(cwd);
    request.source = source.to_string();
    request.metadata = metadata;
    state
        .inner
        .framework
        .start_thread(request)
        .await
        .expect("thread")
}

#[test]
fn framework_activity_merge_projects_application_owned_turn_state() {
    let mut activity = GatewayActivity {
        active_turn_id: Some("foreign-turn".to_string()),
        queued_turns: 7,
        owner_id: Some("foreign-gateway".to_string()),
        owner_surface: Some("tui".to_string()),
        ..GatewayActivity::default()
    };

    merge_framework_activity(
        &mut activity,
        true,
        Some("framework-turn".to_string()),
        2,
        wire::events_transcript::FrameworkTurnKind::Root,
    );

    assert!(activity.running);
    assert_eq!(activity.active_turn_id.as_deref(), Some("framework-turn"));
    assert_eq!(activity.queued_turns, 2);
    assert_eq!(activity.owner_id.as_deref(), Some("foreign-gateway"));
    assert_eq!(activity.owner_surface.as_deref(), Some("tui"));
    assert!(matches!(
        activity.activities.first(),
        Some(wire::events_transcript::ThreadActivityView::FrameworkTurn {
            activity_id,
            turn_id,
            kind: wire::events_transcript::FrameworkTurnKind::Root,
            queued_turns: 2,
        }) if activity_id == "framework-turn" && turn_id == "framework-turn"
    ));
}

#[tokio::test]
async fn thread_trace_reads_through_the_framework_thread_owner() {
    let (_temp, state) = web_state().await;
    let thread_id = state
        .inner
        .framework
        .start_thread(psychevo::StartThreadRequest::new(&state.inner.cwd))
        .await
        .expect("thread")
        .id()
        .to_string();
    let (out_tx, _out_rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        out_tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/trace".to_string(),
            params: Some(json!({"threadId": thread_id, "limit": 1})),
        },
    )
    .await
    .expect("thread/trace");

    assert_eq!(value["threadId"], thread_id);
    assert_eq!(value["available"], false);
    assert_eq!(value["events"], json!([]));
}

#[tokio::test]
async fn thread_list_returns_global_top_level_sessions_without_source_partition() {
    let fallback_title = format!("{}   {}", "fallback ".repeat(14), "title");
    let (temp, state) =
        web_state_with_native_test_executor(framework_message_fixture_executor(vec![
            RuntimeMessage::User {
                content: vec![UserContentBlock::text(fallback_title)],
                timestamp_ms: gateway_now_ms(),
            },
        ]))
        .await;
    let other_cwd = temp.path().join("other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    let top_level_thread = start_thread(&state, &other_cwd, "web", None).await;
    let top_level = top_level_thread.id().to_string();
    top_level_thread
        .start_turn(psychevo::TurnRequest::new("seed fallback title"))
        .await
        .expect("fallback title turn")
        .wait()
        .await
        .expect("fallback title completion");
    let internal = start_thread(&state, &state.inner.cwd, "tui-side-conversation", None)
        .await
        .id()
        .to_string();
    let (out_tx, _out_rx) = mpsc::unbounded_channel();

    let value = handle_rpc(
        state,
        AuthContext::Bearer,
        out_tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("thread list");
    let sessions = value["sessions"].as_array().expect("sessions");
    let ids = sessions
        .iter()
        .filter_map(|session| session["id"].as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&top_level.as_str()));
    assert!(!ids.contains(&internal.as_str()));
    let listed = sessions
        .iter()
        .find(|session| session["id"].as_str() == Some(top_level.as_str()))
        .expect("top level listed");
    assert_eq!(
        listed["project"]["cwd"].as_str(),
        Some(other_cwd.display().to_string().as_str())
    );
    assert_eq!(listed["project"]["label"], "other-work");
    let display_title = listed["displayTitle"].as_str().expect("display title");
    assert_eq!(display_title.chars().count(), 120);
    assert!(display_title.ends_with('…'));
    assert!(listed.get("visibleEntryCount").is_none());
    assert!(listed.get("preview").is_none());
    assert!(listed.get("source").is_none());
    assert!(
        listed["activity"]["frameworkRevision"].is_string(),
        "idle rows carry the Framework activity replay barrier"
    );
}

#[tokio::test]
async fn thread_list_uses_stable_keyset_pages_and_filter_scoped_cursors() {
    let (_temp, state) = web_state().await;
    let cwd = state.inner.cwd.display().to_string();
    let mut expected = BTreeSet::new();
    for _ in 0..3 {
        expected.insert(
            start_thread(&state, &state.inner.cwd, "web", None)
                .await
                .id()
                .to_string(),
        );
    }
    let (tx, _rx) = mpsc::unbounded_channel();

    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/list".to_string(),
            params: Some(json!({ "cwd": cwd.clone(), "limit": 2 })),
        },
    )
    .await
    .expect("first page");
    assert_eq!(first["sessions"].as_array().expect("sessions").len(), 2);
    let cursor = first["nextCursor"]
        .as_str()
        .expect("next cursor")
        .to_string();

    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "thread/list".to_string(),
            params: Some(json!({ "cwd": cwd, "limit": 2, "cursor": cursor.clone() })),
        },
    )
    .await
    .expect("second page");
    assert_eq!(second["sessions"].as_array().expect("sessions").len(), 1);
    assert!(second["nextCursor"].is_null());
    let actual = first["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(second["sessions"].as_array().into_iter().flatten())
        .filter_map(|session| session["id"].as_str())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);

    let mismatch = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "thread/list".to_string(),
            params: Some(json!({ "archived": true, "cursor": cursor })),
        },
    )
    .await
    .expect_err("cursor scope mismatch");
    assert!(
        mismatch
            .to_string()
            .contains("does not match the current filters")
    );
}

#[tokio::test]
async fn thread_browser_bounds_projection_to_returned_pages_at_large_candidate_counts() {
    let (temp, state) = web_state().await;
    let other_cwd = temp.path().join("large-other-work");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = canonicalize_cwd(&other_cwd).expect("other canonical");
    for index in 0..2_000 {
        let cwd = if index % 2 == 0 {
            &state.inner.cwd
        } else {
            &other_cwd
        };
        start_thread(&state, cwd, "web", None).await;
    }
    let internal = start_thread(&state, &state.inner.cwd, "tui-side-conversation", None)
        .await
        .id()
        .to_string();
    let reserved = start_thread(
        &state,
        &state.inner.cwd,
        "web",
        Some(json!({ "agentSessionImportState": { "phase": "reserved" } })),
    )
    .await
    .id()
    .to_string();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut samples = Vec::new();
    let mut value = None;
    for request_id in 1..=20 {
        let started = std::time::Instant::now();
        value = Some(
            handle_rpc(
                state.clone(),
                AuthContext::Bearer,
                tx.clone(),
                RpcRequest {
                    jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
                    id: Some(json!(request_id)),
                    method: "thread/browser".to_string(),
                    params: Some(json!({ "limit": 20, "recentDays": 7 })),
                },
            )
            .await
            .expect("large thread/browser page"),
        );
        samples.push(started.elapsed());
    }
    samples.sort();
    eprintln!(
        "large thread/browser projection: p50={:?}, p95={:?}",
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1]
    );
    let value = value.expect("large browser result");

    let workspaces = value["workspaces"].as_array().expect("workspaces");
    assert_eq!(workspaces.len(), 2);
    assert!(workspaces.iter().all(|workspace| {
        workspace["sessions"]
            .as_array()
            .is_some_and(|sessions| sessions.len() == 20)
            && workspace["hiddenCount"] == 980
            && workspace["nextCursor"]["offset"] == 20
    }));
    let ids = workspaces
        .iter()
        .flat_map(|workspace| workspace["sessions"].as_array().into_iter().flatten())
        .filter_map(|session| session["id"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(!ids.contains(internal.as_str()));
    assert!(!ids.contains(reserved.as_str()));
}

#[tokio::test]
async fn thread_browser_pages_workspace_sessions_and_keeps_include_exceptions() {
    let (_temp, state) = web_state().await;
    let cwd_string = state.inner.cwd.display().to_string();
    let mut ids = Vec::new();
    for _ in 0..25 {
        let id = start_thread(&state, &state.inner.cwd, "web", None)
            .await
            .id()
            .to_string();
        ids.push(id);
    }
    let (tx, _rx) = mpsc::unbounded_channel();

    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/browser".to_string(),
            params: Some(json!({ "cwd": cwd_string.clone(), "limit": 20 })),
        },
    )
    .await
    .expect("thread/browser first page");
    let workspace = &first["workspaces"][0];
    assert_eq!(
        workspace["sessions"].as_array().expect("sessions").len(),
        20
    );
    assert_eq!(workspace["hiddenCount"], 5);
    assert_eq!(workspace["nextCursor"]["offset"], 20);

    let second = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "thread/browser".to_string(),
            params: Some(json!({ "cursor": workspace["nextCursor"].clone(), "limit": 20 })),
        },
    )
    .await
    .expect("thread/browser second page");
    let second_workspace = &second["workspaces"][0];
    assert_eq!(
        second_workspace["sessions"]
            .as_array()
            .expect("second sessions")
            .len(),
        5
    );
    assert_eq!(second_workspace["hiddenCount"], 0);
    assert_eq!(
        second_workspace
            .as_object()
            .and_then(|workspace| workspace.get("nextCursor")),
        Some(&Value::Null)
    );
    let included_id = second_workspace["sessions"][0]["id"]
        .as_str()
        .expect("included candidate")
        .to_string();

    let included = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(3)),
            method: "thread/browser".to_string(),
            params: Some(json!({
                "cwd": cwd_string.clone(),
                "limit": 20,
                "includeSessionIds": [included_id.clone()],
            })),
        },
    )
    .await
    .expect("thread/browser included session");
    let sessions = included["workspaces"][0]["sessions"]
        .as_array()
        .expect("included sessions");
    assert_eq!(sessions.len(), 21);
    assert!(sessions.iter().any(|session| session["id"] == included_id));
    assert_eq!(included["workspaces"][0]["hiddenCount"], 4);

    let running_id = ids[0].clone();
    state
        .inner
        .durability
        .claim_gateway_activity(GatewayActivityClaimInput {
            activity_id: "browser-running-activity",
            thread_id: Some(&running_id),
            source_key: None,
            turn_id: Some("browser-running-turn"),
            kind: GatewayActivityKind::Turn,
            owner_id: "other-gateway",
            owner_surface: Some("test"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .await
        .expect("running browser activity");
    let running = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(4)),
            method: "thread/browser".to_string(),
            params: Some(json!({ "cwd": cwd_string, "limit": 1 })),
        },
    )
    .await
    .expect("thread/browser running exception");
    let running_sessions = running["workspaces"][0]["sessions"]
        .as_array()
        .expect("running sessions");
    assert_eq!(running_sessions.len(), 2);
    assert!(
        running_sessions.iter().any(|session| {
            session["id"] == running_id && session["activity"]["running"] == true
        })
    );
}
