use std::collections::BTreeMap;
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::GatewayApplication;
use crate::FrameworkNativeTestExecutor;

const CHILD_HELPER_TEST: &str =
    "composition::crash_restart_tests::adapter_dispatch_crash_process_helper";
const PRE_DISPATCH_CHILD_HELPER_TEST: &str =
    "composition::crash_restart_tests::accepted_before_adapter_dispatch_crash_process_helper";
const CHILD_MODE_ENV: &str = "PSYCHEVO_TEST_ADAPTER_DISPATCH_CRASH";
const CHILD_MODE_VALUE: &str = "dispatch-intent-v1";
const PRE_DISPATCH_CHILD_MODE_VALUE: &str = "accepted-before-dispatch-v1";
const HOME_ENV: &str = "PSYCHEVO_TEST_CRASH_HOME";
const DATABASE_ENV: &str = "PSYCHEVO_TEST_CRASH_DATABASE";
const CWD_ENV: &str = "PSYCHEVO_TEST_CRASH_CWD";
const THREAD_ENV: &str = "PSYCHEVO_TEST_CRASH_THREAD";
const TURN_ENV: &str = "PSYCHEVO_TEST_CRASH_TURN";
const CLIENT_TURN_ENV: &str = "PSYCHEVO_TEST_CRASH_CLIENT_TURN";
const ADAPTER_STARTED_MARKER: &str = "PSYCHEVO_ADAPTER_DISPATCH_INTENT_COMMITTED";
const PRE_DISPATCH_ACCEPTED_MARKER: &str = "PSYCHEVO_TURN_ACCEPTED_BEFORE_ADAPTER_DISPATCH";
const CRASHED_PROMPT: &str = "do not replay this accepted input";
const LANE_BLOCKER_PROMPT: &str = "hold the Thread lane before queued dispatch";
const PRE_DISPATCH_CRASHED_PROMPT: &str = "accepted while queued; never replay this input";
const RECOVERY_PROMPT: &str = "load authoritative Agent history, then continue";
const PROCESS_HELPER_WATCHDOG: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
struct DeliveryEvidence {
    thread_id: String,
    status: String,
    input_json: Option<String>,
    delivery_confirmed_at_ms: Option<i64>,
    terminal_at_ms: Option<i64>,
}

fn delivery_evidence(database: &std::path::Path, turn_id: &str) -> DeliveryEvidence {
    Connection::open(database)
        .expect("open crash/restart database")
        .query_row(
            r#"
            SELECT thread_id, status, input_json,
                   delivery_confirmed_at_ms, terminal_at_ms
            FROM gateway_turn_deliveries
            WHERE turn_id = ?1
            "#,
            params![turn_id],
            |row| {
                Ok(DeliveryEvidence {
                    thread_id: row.get(0)?,
                    status: row.get(1)?,
                    input_json: row.get(2)?,
                    delivery_confirmed_at_ms: row.get(3)?,
                    terminal_at_ms: row.get(4)?,
                })
            },
        )
        .expect("durable delivery evidence")
}

fn terminal_evidence(database: &std::path::Path, turn_id: &str) -> Option<(String, String, Value)> {
    Connection::open(database)
        .expect("open crash/restart database")
        .query_row(
            r#"
            SELECT status, outcome, metadata_json
            FROM gateway_turn_terminals
            WHERE turn_id = ?1
            "#,
            params![turn_id],
            |row| {
                let metadata: String = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    serde_json::from_str(&metadata).expect("terminal metadata JSON"),
                ))
            },
        )
        .optional()
        .expect("durable terminal evidence")
}

fn retained_receipt_count(
    database: &std::path::Path,
    thread_id: &str,
    client_turn_id: &str,
    turn_id: &str,
) -> usize {
    let metadata: String = Connection::open(database)
        .expect("open crash/restart database")
        .query_row(
            "SELECT metadata_json FROM sessions WHERE id = ?1",
            params![thread_id],
            |row| row.get(0),
        )
        .expect("Thread metadata");
    serde_json::from_str::<Value>(&metadata)
        .expect("Thread metadata JSON")
        .get("gatewayTurnStartReceipts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|receipt| {
            receipt.get("clientTurnId").and_then(Value::as_str) == Some(client_turn_id)
                && receipt.get("turnId").and_then(Value::as_str) == Some(turn_id)
        })
        .count()
}

fn completed_result(thread_id: String, final_answer: &str) -> psychevo::TurnResult {
    psychevo::TurnResult {
        thread_id,
        outcome: psychevo::TurnOutcome::Completed,
        final_answer: final_answer.to_string(),
        provider: "crash-restart-fixture".to_string(),
        model: "fixture-model".to_string(),
        reasoning_effort: None,
        tool_failures: 0,
        context_limit: None,
        context_snapshot: None,
        warnings: Vec::new(),
        terminal_reason: None,
        terminal_error: None,
        selected_agent: None,
        selected_skills: Vec::new(),
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing child-process environment `{name}`"))
}

#[tokio::test]
async fn adapter_dispatch_crash_process_helper() {
    if std::env::var(CHILD_MODE_ENV).as_deref() != Ok(CHILD_MODE_VALUE) {
        return;
    }

    let home = std::path::PathBuf::from(required_env(HOME_ENV));
    let database = std::path::PathBuf::from(required_env(DATABASE_ENV));
    let cwd = std::path::PathBuf::from(required_env(CWD_ENV));
    let thread_id = required_env(THREAD_ENV);
    let turn_id = required_env(TURN_ENV);
    let client_turn_id = required_env(CLIENT_TURN_ENV);
    let expected_thread_id = thread_id.clone();
    let expected_turn_id = turn_id.clone();
    let executor: FrameworkNativeTestExecutor = Arc::new(move |invocation| {
        assert_eq!(invocation.receipt.thread_id, expected_thread_id);
        assert_eq!(invocation.receipt.turn_id, expected_turn_id);
        let marker = format!(
            "{ADAPTER_STARTED_MARKER} {} {}",
            invocation.receipt.thread_id, invocation.receipt.turn_id
        );
        Box::pin(async move {
            invocation.persistence.mark_delivery_unknown().await?;
            println!("{marker}");
            std::io::stdout()
                .flush()
                .expect("flush Adapter-start barrier");
            std::future::pending::<psychevo::Result<psychevo::TurnResult>>().await
        })
    });
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        database,
        None,
        BTreeMap::new(),
        executor,
    )
    .await
    .expect("child composition");
    let start = psychevo::StartThreadRequest::new(&cwd).with_initial_context(
        thread_id.clone(),
        None,
        BTreeMap::new(),
    );
    let request = psychevo::TurnRequest::new(CRASHED_PROMPT)
        .with_identity("crash-restart-test", Some(client_turn_id))
        .with_requested_turn_id(turn_id.clone());
    let accepted = runtime
        .client()
        .start_thread_with_turn(start, request)
        .await
        .expect("durably accepted Turn");
    assert_eq!(accepted.receipt().thread_id, thread_id);
    assert_eq!(accepted.receipt().turn_id, turn_id);

    std::future::pending::<()>().await;
}

#[tokio::test]
async fn accepted_before_adapter_dispatch_crash_process_helper() {
    if std::env::var(CHILD_MODE_ENV).as_deref() != Ok(PRE_DISPATCH_CHILD_MODE_VALUE) {
        return;
    }

    let home = std::path::PathBuf::from(required_env(HOME_ENV));
    let database = std::path::PathBuf::from(required_env(DATABASE_ENV));
    let cwd = std::path::PathBuf::from(required_env(CWD_ENV));
    let thread_id = required_env(THREAD_ENV);
    let turn_id = required_env(TURN_ENV);
    let client_turn_id = required_env(CLIENT_TURN_ENV);
    let blocker_started = Arc::new(tokio::sync::Notify::new());
    let executor: FrameworkNativeTestExecutor = {
        let blocker_started = Arc::clone(&blocker_started);
        Arc::new(move |invocation| {
            let blocker_started = Arc::clone(&blocker_started);
            Box::pin(async move {
                if invocation.input.prompt != LANE_BLOCKER_PROMPT {
                    panic!(
                        "queued Turn reached Adapter before the acceptance crash barrier: {}",
                        invocation.input.prompt
                    );
                }
                blocker_started.notify_one();
                std::future::pending::<psychevo::Result<psychevo::TurnResult>>().await
            })
        })
    };
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        database,
        None,
        BTreeMap::new(),
        executor,
    )
    .await
    .expect("child composition");
    let client = runtime.client();
    let start = psychevo::StartThreadRequest::new(&cwd).with_initial_context(
        thread_id.clone(),
        None,
        BTreeMap::new(),
    );
    let blocker = psychevo::TurnRequest::new(LANE_BLOCKER_PROMPT)
        .with_requested_turn_id(format!("{turn_id}-lane-blocker"));
    let blocker = client
        .start_thread_with_turn(start, blocker)
        .await
        .expect("accepted lane blocker");
    assert_eq!(blocker.receipt().thread_id, thread_id);
    blocker_started.notified().await;

    let thread = client
        .resume_thread(&thread_id)
        .await
        .expect("requested crash-test Thread");
    let queued = psychevo::TurnRequest::new(PRE_DISPATCH_CRASHED_PROMPT)
        .with_identity("crash-restart-test", Some(client_turn_id))
        .with_requested_turn_id(turn_id.clone());
    let accepted = thread
        .start_turn(queued)
        .await
        .expect("durably accepted queued Turn");
    assert_eq!(accepted.receipt().thread_id, thread_id);
    assert_eq!(accepted.receipt().turn_id, turn_id);
    println!(
        "{PRE_DISPATCH_ACCEPTED_MARKER} {} {}",
        accepted.receipt().thread_id,
        accepted.receipt().turn_id
    );
    std::io::stdout()
        .flush()
        .expect("flush durable-acceptance barrier");

    std::future::pending::<()>().await;
}

async fn wait_for_child_marker(
    stdout: tokio::process::ChildStdout,
    expected_marker: &str,
) -> Result<(), String> {
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("read child Adapter barrier: {error}"))?
    {
        if line.contains(expected_marker) {
            return Ok(());
        }
    }
    Err("child process exited before Adapter dispatch barrier".to_string())
}

#[tokio::test]
async fn process_death_after_acceptance_before_adapter_dispatch_never_replays() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let database = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let thread_id = "pre-dispatch-crash-thread";
    let crashed_turn_id = "pre-dispatch-crash-turn";
    let crashed_client_turn_id = "pre-dispatch-crash-client-turn";
    let expected_marker = format!("{PRE_DISPATCH_ACCEPTED_MARKER} {thread_id} {crashed_turn_id}");

    let mut child = tokio::process::Command::new(
        std::env::current_exe().expect("current Gateway test executable"),
    )
    .args([
        "--exact",
        PRE_DISPATCH_CHILD_HELPER_TEST,
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD_MODE_ENV, PRE_DISPATCH_CHILD_MODE_VALUE)
    .env(HOME_ENV, &home)
    .env(DATABASE_ENV, &database)
    .env(CWD_ENV, &cwd)
    .env(THREAD_ENV, thread_id)
    .env(TURN_ENV, crashed_turn_id)
    .env(CLIENT_TURN_ENV, crashed_client_turn_id)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .expect("spawn pre-dispatch crash helper process");
    let stdout = child.stdout.take().expect("child stdout barrier");
    match tokio::time::timeout(
        PROCESS_HELPER_WATCHDOG,
        wait_for_child_marker(stdout, &expected_marker),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let status = child.wait().await.expect("wait for failed crash helper");
            panic!("{error}; status={status}");
        }
        Err(_) => {
            child.kill().await.expect("kill wedged crash helper");
            let status = child.wait().await.expect("wait for wedged crash helper");
            panic!("child durable-acceptance barrier exceeded watchdog; status={status}");
        }
    }
    assert_eq!(child.try_wait().expect("child status before kill"), None);
    child
        .kill()
        .await
        .expect("kill process after durable acceptance");
    let killed = child.wait().await.expect("wait for killed process");
    assert!(
        !killed.success(),
        "the crash helper must die without shutdown"
    );

    let before_restart = delivery_evidence(&database, crashed_turn_id);
    assert_eq!(before_restart.thread_id, thread_id);
    assert_eq!(before_restart.status, "not_delivered");
    assert!(
        before_restart
            .input_json
            .as_deref()
            .is_some_and(|input| input.contains(PRE_DISPATCH_CRASHED_PROMPT)),
        "accepted queued delivery retains recovery evidence"
    );
    assert_eq!(before_restart.delivery_confirmed_at_ms, None);
    assert_eq!(before_restart.terminal_at_ms, None);
    assert_eq!(terminal_evidence(&database, crashed_turn_id), None);
    assert_eq!(
        retained_receipt_count(
            &database,
            thread_id,
            crashed_client_turn_id,
            crashed_turn_id,
        ),
        1,
        "durable acceptance commits exactly one client receipt before dispatch"
    );

    let invocations = Arc::new(AtomicUsize::new(0));
    let observed_prompts = Arc::new(Mutex::new(Vec::new()));
    let recovery_executor: FrameworkNativeTestExecutor = {
        let invocations = Arc::clone(&invocations);
        let observed_prompts = Arc::clone(&observed_prompts);
        Arc::new(move |invocation| {
            invocations.fetch_add(1, Ordering::SeqCst);
            observed_prompts
                .lock()
                .expect("observed prompt lock")
                .push(invocation.input.prompt.clone());
            Box::pin(async move {
                invocation.persistence.confirm_delivery().await?;
                Ok(completed_result(
                    invocation.receipt.thread_id,
                    "continued without replaying queued input",
                ))
            })
        })
    };
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        database.clone(),
        None,
        BTreeMap::new(),
        recovery_executor,
    )
    .await
    .expect("reopened Application/Gateway composition");
    let client = runtime.client();
    let thread = client
        .resume_thread(thread_id)
        .await
        .expect("reopen durable Thread");
    assert!(matches!(
        client.resume_turn(crashed_turn_id).await,
        Err(psychevo::Error::OutcomeIndeterminate { turn_id })
            if turn_id == crashed_turn_id
    ));

    let duplicate = psychevo::TurnRequest::new(PRE_DISPATCH_CRASHED_PROMPT)
        .with_identity(
            "crash-restart-test",
            Some(crashed_client_turn_id.to_string()),
        )
        .with_requested_turn_id(crashed_turn_id.to_string());
    thread
        .start_turn(duplicate)
        .await
        .expect_err("duplicate durable Turn delivery must be rejected");
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        0,
        "duplicate acceptance cannot dispatch the queued input"
    );
    assert_eq!(
        retained_receipt_count(
            &database,
            thread_id,
            crashed_client_turn_id,
            crashed_turn_id,
        ),
        1
    );
    assert_eq!(
        delivery_evidence(&database, crashed_turn_id),
        before_restart
    );

    let recovery = psychevo::TurnRequest::new(RECOVERY_PROMPT)
        .with_identity(
            "crash-restart-test",
            Some("pre-dispatch-recovery-client-turn".to_string()),
        )
        .with_requested_turn_id("pre-dispatch-recovery-turn".to_string());
    let result = thread
        .start_turn(recovery)
        .await
        .expect("accept explicit recovery Turn")
        .wait()
        .await
        .expect("complete explicit recovery Turn");
    assert_eq!(
        result.final_answer,
        "continued without replaying queued input"
    );
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert_eq!(
        *observed_prompts.lock().expect("observed prompt lock"),
        vec![RECOVERY_PROMPT.to_string()]
    );

    runtime
        .shutdown()
        .await
        .expect("clean reopened composition shutdown");
}

#[tokio::test]
async fn process_death_after_adapter_dispatch_recovers_without_replay() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let database = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let thread_id = "crash-restart-thread";
    let crashed_turn_id = "crash-restart-turn";
    let crashed_client_turn_id = "crash-restart-client-turn";
    let expected_marker = format!("{ADAPTER_STARTED_MARKER} {thread_id} {crashed_turn_id}");

    let mut child = tokio::process::Command::new(
        std::env::current_exe().expect("current Gateway test executable"),
    )
    .args([
        "--exact",
        CHILD_HELPER_TEST,
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD_MODE_ENV, CHILD_MODE_VALUE)
    .env(HOME_ENV, &home)
    .env(DATABASE_ENV, &database)
    .env(CWD_ENV, &cwd)
    .env(THREAD_ENV, thread_id)
    .env(TURN_ENV, crashed_turn_id)
    .env(CLIENT_TURN_ENV, crashed_client_turn_id)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .expect("spawn crash helper process");
    let stdout = child.stdout.take().expect("child stdout barrier");
    match tokio::time::timeout(
        PROCESS_HELPER_WATCHDOG,
        wait_for_child_marker(stdout, &expected_marker),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let status = child.wait().await.expect("wait for failed crash helper");
            panic!("{error}; status={status}");
        }
        Err(_) => {
            child.kill().await.expect("kill wedged crash helper");
            let status = child.wait().await.expect("wait for wedged crash helper");
            panic!("child Adapter barrier exceeded watchdog; status={status}");
        }
    }
    assert_eq!(child.try_wait().expect("child status before kill"), None);
    child
        .kill()
        .await
        .expect("kill process after Adapter start");
    let killed = child.wait().await.expect("wait for killed process");
    assert!(
        !killed.success(),
        "the crash helper must die without shutdown"
    );

    let before_restart = delivery_evidence(&database, crashed_turn_id);
    assert_eq!(before_restart.thread_id, thread_id);
    assert_eq!(before_restart.status, "unknown");
    assert!(
        before_restart
            .input_json
            .as_deref()
            .is_some_and(|input| input.contains(CRASHED_PROMPT)),
        "unknown delivery retains recovery input"
    );
    assert_eq!(before_restart.delivery_confirmed_at_ms, None);
    assert_eq!(before_restart.terminal_at_ms, None);
    assert_eq!(terminal_evidence(&database, crashed_turn_id), None);
    assert_eq!(
        retained_receipt_count(
            &database,
            thread_id,
            crashed_client_turn_id,
            crashed_turn_id,
        ),
        1,
        "durable acceptance and clientTurnId receipt commit before Adapter invocation"
    );

    let invocations = Arc::new(AtomicUsize::new(0));
    let reconciled = Arc::new(AtomicBool::new(false));
    let observed_prompts = Arc::new(Mutex::new(Vec::new()));
    let expected_unknown = crashed_turn_id.to_string();
    let recovery_executor: FrameworkNativeTestExecutor = {
        let invocations = Arc::clone(&invocations);
        let reconciled = Arc::clone(&reconciled);
        let observed_prompts = Arc::clone(&observed_prompts);
        Arc::new(move |invocation| {
            invocations.fetch_add(1, Ordering::SeqCst);
            observed_prompts
                .lock()
                .expect("observed prompt lock")
                .push(invocation.input.prompt.clone());
            let expected_unknown = expected_unknown.clone();
            let reconciled = Arc::clone(&reconciled);
            Box::pin(async move {
                let unknown = invocation
                    .persistence
                    .prior_unknown_delivery()
                    .await?
                    .ok_or_else(|| psychevo::Error::Message("missing unknown delivery".into()))?;
                if unknown.turn_id != expected_unknown {
                    return Err(psychevo::Error::Message(format!(
                        "unexpected unknown delivery `{}`",
                        unknown.turn_id
                    )));
                }
                let metadata = json!({
                    "reconciledFrom": "agent_history",
                    "messageIds": ["authoritative-assistant-message"],
                });
                if !invocation
                    .persistence
                    .reconcile_unknown_delivery(expected_unknown.clone(), metadata.clone())
                    .await?
                {
                    return Err(psychevo::Error::Message(
                        "first evidence-backed reconciliation was not applied".into(),
                    ));
                }
                if invocation
                    .persistence
                    .reconcile_unknown_delivery(expected_unknown, json!({"overwrite": true}))
                    .await?
                {
                    return Err(psychevo::Error::Message(
                        "repeated reconciliation rewrote a terminal".into(),
                    ));
                }
                reconciled.store(true, Ordering::SeqCst);
                invocation.persistence.confirm_delivery().await?;
                Ok(completed_result(
                    invocation.receipt.thread_id,
                    "continued after authoritative recovery",
                ))
            })
        })
    };
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        database.clone(),
        None,
        BTreeMap::new(),
        recovery_executor,
    )
    .await
    .expect("reopened Application/Gateway composition");
    let client = runtime.client();
    let thread = client
        .resume_thread(thread_id)
        .await
        .expect("reopen durable Thread");
    assert!(
        !client.activity_snapshot().threads.contains_key(thread_id),
        "reopen must not project a zombie active Turn"
    );
    assert_eq!(
        thread
            .snapshot()
            .await
            .expect("reopened Thread snapshot")
            .summary
            .active_turn_id,
        None,
        "the durable Thread view has no resurrected process-local Turn"
    );
    assert!(
        matches!(
            client.resume_turn(crashed_turn_id).await,
            Err(psychevo::Error::OutcomeIndeterminate { turn_id })
                if turn_id == crashed_turn_id
        ),
        "unknown delivery has no invented terminal before Agent history reconciliation"
    );
    assert_eq!(
        client
            .framework_turn_terminal_evidence(crashed_turn_id)
            .await
            .expect("pre-reconcile terminal read"),
        None
    );

    let duplicate = psychevo::TurnRequest::new(CRASHED_PROMPT)
        .with_identity(
            "crash-restart-test",
            Some(crashed_client_turn_id.to_string()),
        )
        .with_requested_turn_id(crashed_turn_id.to_string());
    thread
        .start_turn(duplicate)
        .await
        .expect_err("duplicate durable Turn delivery must be rejected");
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        0,
        "duplicate acceptance cannot reinvoke the Adapter"
    );
    assert_eq!(
        retained_receipt_count(
            &database,
            thread_id,
            crashed_client_turn_id,
            crashed_turn_id,
        ),
        1,
        "duplicate acceptance cannot duplicate its durable receipt"
    );
    assert_eq!(
        delivery_evidence(&database, crashed_turn_id),
        before_restart,
        "duplicate acceptance cannot rewrite the unknown delivery"
    );
    assert_eq!(
        terminal_evidence(&database, crashed_turn_id),
        None,
        "duplicate acceptance cannot invent a terminal"
    );

    let recovery = psychevo::TurnRequest::new(RECOVERY_PROMPT)
        .with_identity(
            "crash-restart-test",
            Some("crash-restart-recovery-client-turn".to_string()),
        )
        .with_requested_turn_id("crash-restart-recovery-turn".to_string());
    let result = thread
        .start_turn(recovery)
        .await
        .expect("accept explicit recovery Turn")
        .wait()
        .await
        .expect("complete explicit recovery Turn");
    assert_eq!(
        result.final_answer,
        "continued after authoritative recovery"
    );
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert!(reconciled.load(Ordering::SeqCst));
    assert_eq!(
        *observed_prompts.lock().expect("observed prompt lock"),
        vec![RECOVERY_PROMPT.to_string()],
        "restart dispatches only the caller's new input"
    );

    let recovered = delivery_evidence(&database, crashed_turn_id);
    assert_eq!(recovered.status, "terminal");
    assert_eq!(recovered.input_json, None);
    assert!(recovered.delivery_confirmed_at_ms.is_some());
    assert!(recovered.terminal_at_ms.is_some());
    let (status, outcome, metadata) =
        terminal_evidence(&database, crashed_turn_id).expect("reconciled terminal");
    assert_eq!(status, "completed");
    assert_eq!(outcome, "normal");
    assert_eq!(metadata["reconciledFrom"], "agent_history");
    assert_eq!(
        metadata["messageIds"],
        json!(["authoritative-assistant-message"]),
        "the second reconciliation cannot overwrite first evidence"
    );
    let framework_terminal = client
        .framework_turn_terminal_evidence(crashed_turn_id)
        .await
        .expect("Framework terminal read")
        .expect("authoritative reconciled terminal");
    assert_eq!(
        framework_terminal.status,
        psychevo::FrameworkTurnTerminalStatus::Completed
    );
    assert_eq!(
        framework_terminal.outcome,
        psychevo::FrameworkTurnTerminalOutcome::Normal
    );

    runtime
        .shutdown()
        .await
        .expect("clean reopened composition shutdown");
}
