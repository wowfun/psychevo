use std::sync::Arc;
use std::time::Duration;

use psychevo::PermissionMode;
use psychevo::application::{AutomationRunStatus, AutomationTaskInput, AutomationTaskKind};
use psychevo_gateway_protocol as wire;
use serde_json::json;
use tokio::sync::mpsc;

use super::helpers::{
    AutomationTurnProbe, wait_for_automation_status, web_state_with_automation_turn_probe,
};
use crate::gateway_now_ms;
use crate::server::automations;
use crate::server::binding::AuthContext;
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;

#[tokio::test]
async fn automation_manual_run_uses_auto_sandbox_and_updates_status() {
    let backend = Arc::new(AutomationTurnProbe::default());
    let (_temp, state) = web_state_with_automation_turn_probe(backend.clone()).await;
    let (tx, mut rx) = mpsc::unbounded_channel();

    let created = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "automation/write".to_string(),
            params: Some(json!({
                "target": { "kind": "project" },
                "title": "Repo check",
                "prompt": "Check the repo.",
                "schedule": { "kind": "interval", "everyMinutes": 30 }
            })),
        },
    )
    .await
    .expect("automation/write");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();

    let run = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!(2)),
            method: "automation/run".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/run");
    assert_eq!(run["accepted"], true);
    assert!(run["run"]["id"].is_string());

    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("fake backend run");
    let task =
        wait_for_automation_status(&state, &automation_id, AutomationRunStatus::Completed).await;
    assert!(task.next_run_at_ms.is_some());
    let runs = state
        .inner
        .durability
        .automation_runs_for_task(&automation_id, 10)
        .await
        .expect("automation runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, AutomationRunStatus::Completed);
    assert!(runs[0].thread_id.is_some());

    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert_eq!(backend_runs[0].prompt, "Check the repo.");
    assert_eq!(
        backend_runs[0].permission_mode,
        Some(PermissionMode::BypassPermissions)
    );
    let sandbox = backend_runs[0]
        .sandbox_override
        .as_ref()
        .expect("sandbox override");
    assert!(sandbox.enabled);
    assert_eq!(
        sandbox.mode,
        psychevo::application::RunSandboxMode::WorkspaceWrite
    );

    let mut saw_terminal = false;
    while let Ok(message) = rx.try_recv() {
        saw_terminal |= message.contains("\"type\":\"turnCompleted\"");
    }
    assert!(saw_terminal);
}

#[tokio::test]
async fn automation_run_and_task_status_follow_every_turn_outcome() {
    let backend = Arc::new(AutomationTurnProbe::default());
    backend
        .outcomes
        .lock()
        .expect("automation outcomes")
        .extend([
            psychevo::TurnOutcome::Completed,
            psychevo::TurnOutcome::Failed,
            psychevo::TurnOutcome::Stopped,
            psychevo::TurnOutcome::Interrupted,
        ]);
    let (_temp, state) = web_state_with_automation_turn_probe(backend.clone()).await;
    let (tx, _rx) = mpsc::unbounded_channel();

    for (index, (expected, expected_outcome, expected_error)) in [
        (AutomationRunStatus::Completed, "normal", None),
        (
            AutomationRunStatus::Failed,
            "failed",
            Some("fake automation terminal failure"),
        ),
        (AutomationRunStatus::Interrupted, "stopped", None),
        (AutomationRunStatus::Interrupted, "aborted", None),
    ]
    .into_iter()
    .enumerate()
    {
        let created = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
                id: Some(json!(["create", index])),
                method: "automation/write".to_string(),
                params: Some(json!({
                    "target": { "kind": "project" },
                    "title": format!("Outcome {index}"),
                    "prompt": "Exercise the terminal mapping.",
                    "schedule": { "kind": "interval", "everyMinutes": 30 }
                })),
            },
        )
        .await
        .expect("automation/write");
        let automation_id = created["automation"]["id"]
            .as_str()
            .expect("automation id")
            .to_string();

        let accepted = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
                id: Some(json!(["run", index])),
                method: "automation/run".to_string(),
                params: Some(json!({ "automationId": automation_id })),
            },
        )
        .await
        .expect("automation/run");
        assert_eq!(accepted["accepted"], true);
        tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
            .await
            .expect("fake backend run");

        let task = wait_for_automation_status(&state, &automation_id, expected).await;
        assert_eq!(task.last_status, Some(expected));
        let runs = state
            .inner
            .durability
            .automation_runs_for_task(&automation_id, 1)
            .await
            .expect("automation runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, expected);
        assert_eq!(runs[0].error.as_deref(), expected_error);
        assert_eq!(
            runs[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata["outcome"].as_str()),
            Some(expected_outcome)
        );
    }
}

#[tokio::test]
async fn automation_stale_running_run_recovers_and_preserves_history_thread() {
    let backend = Arc::new(AutomationTurnProbe::default());
    let (temp, state) = web_state_with_automation_turn_probe(backend.clone()).await;
    let cwd = state.inner.cwd.to_string_lossy().to_string();
    let mut start = psychevo::StartThreadRequest::new(&state.inner.cwd);
    start.source = "automation".to_string();
    let historical_thread = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("historical Thread")
        .id()
        .to_string();
    let automation_id = "stale-gateway-run";
    let source_key = "automation:stale-gateway-run";
    state
        .inner
        .durability
        .upsert_automation_task(AutomationTaskInput {
            id: Some(automation_id.to_string()),
            cwd,
            kind: AutomationTaskKind::Project,
            target_thread_id: None,
            title: "Recover stale run".to_string(),
            prompt: "Recover this stale run.".to_string(),
            schedule: json!({"kind": "interval", "everyMinutes": 30}),
            enabled: true,
            execution: json!({"policy": "autoSandbox"}),
            model: None,
            reasoning_effort: None,
            source_key: Some(source_key.to_string()),
            next_run_at_ms: Some(gateway_now_ms().saturating_sub(1_000)),
        })
        .await
        .expect("automation task");
    let stale_run = state
        .inner
        .durability
        .claim_automation_run(automation_id, "scheduler")
        .await
        .expect("claim")
        .expect("running claim");
    let stale_started_at = gateway_now_ms().saturating_sub(10 * 60 * 1_000);
    let conn = rusqlite::Connection::open(temp.path().join("state.db")).expect("db");
    conn.execute(
        "UPDATE automation_runs SET started_at_ms = ?2, thread_id = ?3, source_key = ?4 WHERE id = ?1",
        rusqlite::params![stale_run.id, stale_started_at, historical_thread, source_key],
    )
    .expect("age stale run");

    let accepted = automations::run_due_automations_once(state.clone())
        .await
        .expect("scheduler pass");
    assert_eq!(accepted, 0);
    let recovered = state
        .inner
        .durability
        .automation_task(automation_id)
        .await
        .expect("automation task")
        .expect("task");
    assert_eq!(recovered.last_status, Some(AutomationRunStatus::Failed));
    assert!(
        recovered
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("stale running claim expired"))
    );
    assert!(
        recovered
            .next_run_at_ms
            .is_some_and(|next| next > gateway_now_ms())
    );

    let (tx, _rx) = mpsc::unbounded_channel();
    let manual = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("manual")),
            method: "automation/run".to_string(),
            params: Some(json!({ "automationId": automation_id })),
        },
    )
    .await
    .expect("automation/run");
    assert_eq!(manual["accepted"], true);
    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("manual backend run");
    wait_for_automation_status(&state, automation_id, AutomationRunStatus::Completed).await;

    let listed = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("list")),
            method: "automation/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("automation/list");
    let runs = listed["automations"][0]["runs"].as_array().expect("runs");
    assert!(runs.iter().any(|run| {
        run["status"].as_str() == Some("failed")
            && run["threadId"].as_str() == Some(historical_thread.as_str())
    }));
}
