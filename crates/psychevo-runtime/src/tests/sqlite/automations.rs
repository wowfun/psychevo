use super::*;
use crate::store::{AutomationRunFinishInput, GatewayActivityClaimInput};
use psychevo_agent_core::now_ms;

fn automation_input(cwd: &str) -> AutomationTaskInput {
    AutomationTaskInput {
        id: Some("automation-1".to_string()),
        cwd: cwd.to_string(),
        kind: "project".to_string(),
        target_thread_id: None,
        title: "Morning check".to_string(),
        prompt: "Summarize repo status".to_string(),
        schedule: json!({"kind": "interval", "everyMinutes": 30}),
        enabled: true,
        execution: json!({"policy": "autoSandbox"}),
        model: Some("test/model".to_string()),
        reasoning_effort: Some("low".to_string()),
        source_key: None,
        next_run_at_ms: Some(1_000),
    }
}

fn automation_activity_claim<'a>(
    activity_id: &'a str,
    source_key: &'a str,
    lease_expires_at_ms: i64,
) -> GatewayActivityClaimInput<'a> {
    GatewayActivityClaimInput {
        activity_id,
        thread_id: None,
        source_key: Some(source_key),
        turn_id: Some(activity_id),
        kind: "turn",
        owner_id: "automation-owner",
        owner_surface: Some("automation"),
        lease_expires_at_ms,
        queued_turns: 0,
        superseded_activity_id: None,
        intent: Some(json!({"kind": "turn"})),
    }
}

#[test]
fn automation_task_upsert_lists_due_and_deletes() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db")).expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_str = cwd.to_string_lossy().to_string();

    let created = store
        .upsert_automation_task(automation_input(&cwd_str))
        .expect("create automation");
    assert_eq!(created.id, "automation-1");
    assert_eq!(created.schedule["everyMinutes"], 30);
    assert!(created.enabled);

    let mut update = automation_input(&cwd_str);
    update.title = "Updated check".to_string();
    update.enabled = false;
    update.next_run_at_ms = Some(2_000);
    let updated = store
        .upsert_automation_task(update)
        .expect("update automation");
    assert_eq!(updated.created_at_ms, created.created_at_ms);
    assert!(updated.updated_at_ms >= created.updated_at_ms);
    assert_eq!(updated.title, "Updated check");
    assert!(!updated.enabled);

    let tasks = store.automation_tasks_for_cwd(&cwd_str).expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["automation-1",]
    );
    assert!(
        store
            .due_automation_tasks(3_000, 10)
            .expect("disabled tasks are not due")
            .is_empty()
    );

    let mut enabled = automation_input(&cwd_str);
    enabled.next_run_at_ms = Some(2_000);
    store
        .upsert_automation_task(enabled)
        .expect("enable automation");
    let due = store.due_automation_tasks(3_000, 10).expect("due tasks");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, "automation-1");

    assert!(
        store
            .delete_automation_task("automation-1")
            .expect("delete automation")
    );
    assert!(
        store
            .automation_tasks_for_cwd(&cwd_str)
            .expect("empty")
            .is_empty()
    );
}

#[test]
fn automation_run_claim_is_single_running_and_finish_updates_task() {
    let temp = tempdir().expect("tempdir");
    let store = StateRuntime::open(temp.path().join("state.db")).expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_str = cwd.to_string_lossy().to_string();
    let thread_id = store
        .create_session_with_metadata(&cwd, "automation", "model", "provider", None)
        .expect("session");
    store
        .upsert_automation_task(automation_input(&cwd_str))
        .expect("create automation");

    let first = store
        .claim_automation_run("automation-1", "scheduler")
        .expect("first claim")
        .expect("running record");
    assert_eq!(first.status, "running");
    assert_eq!(first.trigger, "scheduler");
    assert!(
        store
            .claim_automation_run("automation-1", "scheduler")
            .expect("second claim")
            .is_none()
    );
    let task = store
        .automation_task("automation-1")
        .expect("task")
        .expect("task record");
    assert_eq!(task.last_status.as_deref(), Some("running"));
    assert!(task.last_run_at_ms.unwrap_or_default() <= now_ms());

    let finished = store
        .finish_automation_run(AutomationRunFinishInput {
            run_id: &first.id,
            status: "completed",
            thread_id: Some(&thread_id),
            source_key: Some("automation:automation-1"),
            error: None,
            metadata: Some(json!({"turnId": "turn-1"})),
            next_run_at_ms: Some(99_000),
        })
        .expect("finish")
        .expect("finished run");
    assert_eq!(finished.status, "completed");
    assert_eq!(finished.thread_id.as_deref(), Some(thread_id.as_str()));
    assert_eq!(
        finished
            .metadata
            .as_ref()
            .and_then(|value| value["turnId"].as_str()),
        Some("turn-1")
    );

    let task = store
        .automation_task("automation-1")
        .expect("task")
        .expect("task record");
    assert_eq!(task.last_status.as_deref(), Some("completed"));
    assert_eq!(task.last_error, None);
    assert_eq!(task.source_key.as_deref(), Some("automation:automation-1"));
    assert_eq!(task.next_run_at_ms, Some(99_000));

    let second = store
        .claim_automation_run("automation-1", "manual")
        .expect("claim after finish")
        .expect("new run");
    assert_ne!(second.id, first.id);
    assert_eq!(
        store
            .automation_runs_for_task("automation-1", 10)
            .expect("runs")
            .len(),
        2
    );
}

#[test]
fn stale_automation_run_recovery_candidates_ignore_active_gateway_activity() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("state.db");
    let store = StateRuntime::open(&db_path).expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_str = cwd.to_string_lossy().to_string();
    let source_key = "automation:automation-1";
    let mut input = automation_input(&cwd_str);
    input.source_key = Some(source_key.to_string());
    store
        .upsert_automation_task(input)
        .expect("create automation");
    let run = store
        .claim_automation_run("automation-1", "scheduler")
        .expect("claim")
        .expect("run");
    let old_started_at = now_ms() - 10 * 60 * 1000;
    rusqlite::Connection::open(&db_path)
        .expect("raw connection")
        .execute(
            "UPDATE automation_runs SET started_at_ms = ?2 WHERE id = ?1",
            rusqlite::params![run.id, old_started_at],
        )
        .expect("age run");

    let now = now_ms();
    let stale_without_activity = store
        .stale_automation_runs_for_recovery(now, 5 * 60 * 1000, 10)
        .expect("stale candidates");
    assert_eq!(stale_without_activity.len(), 1);
    assert_eq!(stale_without_activity[0].run.id, run.id);

    let activity = store
        .claim_gateway_activity(automation_activity_claim(
            "activity-1",
            source_key,
            now_ms() + 60_000,
        ))
        .expect("active activity");
    assert!(
        store
            .stale_automation_runs_for_recovery(now_ms(), 5 * 60 * 1000, 10)
            .expect("active protected candidates")
            .is_empty()
    );
    assert!(
        store
            .heartbeat_gateway_activity(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                now_ms() - 1,
            )
            .expect("expire activity")
    );
    let stale_after_expiry = store
        .stale_automation_runs_for_recovery(now_ms(), 5 * 60 * 1000, 10)
        .expect("expired candidates");
    assert_eq!(stale_after_expiry.len(), 1);
}

#[test]
fn recovered_stale_automation_run_allows_new_claim() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("state.db");
    let store = StateRuntime::open(&db_path).expect("store");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let cwd_str = cwd.to_string_lossy().to_string();
    store
        .upsert_automation_task(automation_input(&cwd_str))
        .expect("create automation");
    let run = store
        .claim_automation_run("automation-1", "scheduler")
        .expect("claim")
        .expect("run");
    rusqlite::Connection::open(&db_path)
        .expect("raw connection")
        .execute(
            "UPDATE automation_runs SET started_at_ms = ?2 WHERE id = ?1",
            rusqlite::params![run.id, now_ms() - 10 * 60 * 1000],
        )
        .expect("age run");
    let candidate = store
        .stale_automation_runs_for_recovery(now_ms(), 5 * 60 * 1000, 10)
        .expect("stale candidates")
        .pop()
        .expect("candidate");

    store
        .finish_automation_run(AutomationRunFinishInput {
            run_id: &candidate.run.id,
            status: "failed",
            thread_id: candidate.run.thread_id.as_deref(),
            source_key: candidate.run.source_key.as_deref(),
            error: Some("automation run recovery: stale running claim expired"),
            metadata: Some(json!({"trigger": candidate.run.trigger})),
            next_run_at_ms: Some(99_000),
        })
        .expect("finish stale run");
    let next = store
        .claim_automation_run("automation-1", "manual")
        .expect("new claim")
        .expect("new run");
    assert_ne!(next.id, run.id);
}
