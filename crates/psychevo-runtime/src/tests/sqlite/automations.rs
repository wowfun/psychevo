use super::*;
use crate::store::AutomationRunFinishInput;
use psychevo_agent_core::now_ms;

fn automation_input(workdir: &str) -> AutomationTaskInput {
    AutomationTaskInput {
        id: Some("automation-1".to_string()),
        workdir: workdir.to_string(),
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

#[test]
fn automation_task_upsert_lists_due_and_deletes() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let workdir_str = workdir.to_string_lossy().to_string();

    let created = store
        .upsert_automation_task(automation_input(&workdir_str))
        .expect("create automation");
    assert_eq!(created.id, "automation-1");
    assert_eq!(created.schedule["everyMinutes"], 30);
    assert!(created.enabled);

    let mut update = automation_input(&workdir_str);
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

    let tasks = store
        .automation_tasks_for_workdir(&workdir_str)
        .expect("tasks");
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

    let mut enabled = automation_input(&workdir_str);
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
            .automation_tasks_for_workdir(&workdir_str)
            .expect("empty")
            .is_empty()
    );
}

#[test]
fn automation_run_claim_is_single_running_and_finish_updates_task() {
    let temp = tempdir().expect("tempdir");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let workdir_str = workdir.to_string_lossy().to_string();
    let thread_id = store
        .create_session_with_metadata(&workdir, "automation", "model", "provider", None)
        .expect("session");
    store
        .upsert_automation_task(automation_input(&workdir_str))
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
