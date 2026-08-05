use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::future::BoxFuture;
use psychevo::application::{GatewayControlCommandInput, GatewayControlCommandKind};
use psychevo::{Application, PermissionMode, RunMode, ShellCommandRequest, ThreadAgentBinding};
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::super::activity::{ActiveActivityControl, ActiveActivityKind};
use super::super::peer_runtime::{PeerResolutionContext, resolve_peer_turn};
use super::support_peer::{FrameworkNativeProbe, harness, request};
use psychevo_gateway_protocol::source::GatewaySource;

#[derive(Debug)]
struct BlockingFrameworkAdapter {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[derive(Debug)]
struct PreparedBlockingFrameworkTurn {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

impl psychevo::AgentSessionAdapter for BlockingFrameworkAdapter {
    fn prepare_turn(
        self: Arc<Self>,
        _request: psychevo::AgentTurnPreparation,
    ) -> BoxFuture<'static, psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>> {
        Box::pin(async move {
            Ok(Box::new(PreparedBlockingFrameworkTurn {
                started: self.started.clone(),
                release: self.release.clone(),
            }) as Box<dyn psychevo::PreparedAgentTurn>)
        })
    }
}

impl psychevo::PreparedAgentTurn for PreparedBlockingFrameworkTurn {
    fn invoke(
        self: Box<Self>,
        invocation: psychevo::AgentTurnInvocation,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
        Box::pin(async move {
            self.started.notify_one();
            self.release.notified().await;
            let receipt = invocation.receipt.clone();
            drop(invocation);
            Ok(psychevo::TurnResult {
                thread_id: receipt.thread_id,
                outcome: psychevo::TurnOutcome::Completed,
                final_answer: "done".to_string(),
                provider: "fake".to_string(),
                model: "fake".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    }
}

async fn resolved_binding(
    harness: &super::support_peer::Harness,
    thread_id: &str,
) -> psychevo::AgentBindingSnapshot {
    match harness
        ._application
        .client()
        .thread_agent_binding(thread_id)
        .await
        .expect("binding read")
        .expect("binding")
    {
        ThreadAgentBinding::Resolved { binding, .. } => *binding,
        ThreadAgentBinding::Unresolved { reason, .. } => {
            panic!("binding remained unresolved: {reason:?}")
        }
    }
}

#[tokio::test]
async fn typed_steer_requires_expected_turn_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let application = Application::builder()
        .home(temp.path())
        .database_path(":memory:")
        .agent_session_adapter(Arc::new(BlockingFrameworkAdapter {
            started: started.clone(),
            release: release.clone(),
        }))
        .build()
        .await
        .expect("Application");
    let thread = application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(temp.path()))
        .await
        .expect("Framework Thread");
    let handle = thread
        .start_turn(psychevo::TurnRequest::new("first"))
        .await
        .expect("accepted Turn");
    started.notified().await;
    let active_turn_id = handle.receipt().turn_id.clone();
    assert!(!thread.steer("stale-turn", "steer").expect("stale result"));
    assert!(
        thread
            .steer(&active_turn_id, "steer")
            .expect("active steer")
    );

    release.notify_one();
    handle.wait().await.expect("first Turn");
    application.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn post_side_effect_control_write_failure_is_diagnostic_and_never_replayed() {
    let harness = harness(Arc::new(FrameworkNativeProbe::default())).await;
    let activity_id = "control-write-failure";
    let shell = harness
        ._application
        .client()
        .shell_command(ShellCommandRequest::new(&harness.cwd, "printf unused"))
        .expect("Shell command");
    let control = shell.control();
    harness.gateway.register_active(
        "control-write-failure",
        activity_id.to_string(),
        Some(ActiveActivityControl::Shell(control.clone())),
        ActiveActivityKind::Shell,
    );
    harness
        .durability
        .enqueue_gateway_control_command(GatewayControlCommandInput {
            activity_id,
            owner_id: harness.gateway.owner_id(),
            command_kind: GatewayControlCommandKind::Interrupt,
            payload: json!({}),
        })
        .await
        .expect("enqueue interrupt");
    let fault_connection = rusqlite::Connection::open(&harness.db_path).expect("fault connection");
    fault_connection
        .execute_batch(
            r#"
            CREATE TRIGGER fail_gateway_control_terminal_update
            BEFORE UPDATE OF status ON gateway_control_commands
            WHEN NEW.status IN ('applied', 'failed')
            BEGIN
                SELECT RAISE(FAIL, 'injected control terminal failure');
            END
            "#,
        )
        .expect("install fault trigger");

    harness
        .gateway
        .apply_pending_gateway_control_commands()
        .await;

    assert!(control.is_interrupted());
    let status = fault_connection
        .query_row(
            "SELECT status FROM gateway_control_commands WHERE activity_id = ?1",
            [activity_id],
            |row| row.get::<_, String>(0),
        )
        .expect("command status");
    assert_eq!(status, "applying");
    assert_eq!(
        harness
            .gateway
            .shell_activity_diagnostics()
            .failed_operations,
        1
    );

    harness
        .gateway
        .apply_pending_gateway_control_commands()
        .await;
    let diagnostics = harness.gateway.shell_activity_diagnostics();
    assert_eq!(diagnostics.failed_operations, 2);
    assert_eq!(diagnostics.control_commands_claimed, 1);
    assert_eq!(diagnostics.control_commands_indeterminate, 1);
    assert_eq!(diagnostics.control_dispatch_latency_samples, 1);
    let p50 = diagnostics
        .control_dispatch_latency_p50_ms
        .expect("control dispatch p50");
    let p95 = diagnostics
        .control_dispatch_latency_p95_ms
        .expect("control dispatch p95");
    let p99 = diagnostics
        .control_dispatch_latency_p99_ms
        .expect("control dispatch p99");
    assert!(p50 <= p95 && p95 <= p99);
    let status = fault_connection
        .query_row(
            "SELECT status FROM gateway_control_commands WHERE activity_id = ?1",
            [activity_id],
            |row| row.get::<_, String>(0),
        )
        .expect("indeterminate command status");
    assert_eq!(status, "outcome_indeterminate");
    drop(fault_connection);
}

#[tokio::test]
async fn gateway_shutdown_drains_every_scope_before_reporting_a_task_panic() {
    let harness = harness(Arc::new(FrameworkNativeProbe::default())).await;
    let producer_started = Arc::new(tokio::sync::Notify::new());
    let producer_started_for_task = Arc::clone(&producer_started);
    let turn_completed = Arc::new(AtomicBool::new(false));
    let turn_completed_for_task = turn_completed.clone();
    let infrastructure_completed = Arc::new(AtomicBool::new(false));
    let infrastructure_completed_for_task = infrastructure_completed.clone();
    harness
        .gateway
        .supervisor
        .spawn_producer("panic-producer", async move {
            producer_started_for_task.notify_one();
            panic!("injected Gateway producer panic");
        });
    producer_started.notified().await;
    harness
        .gateway
        .supervisor
        .spawn_activity("activity", async move {
            turn_completed_for_task.store(true, Ordering::Release);
        });
    harness
        .gateway
        .supervisor
        .spawn_infrastructure("infrastructure", async move {
            infrastructure_completed_for_task.store(true, Ordering::Release);
        });

    let error = harness
        .gateway
        .shutdown_activity_runtime(false)
        .await
        .expect_err("panic must make shutdown non-clean");

    assert!(turn_completed.load(Ordering::Acquire));
    assert!(infrastructure_completed.load(Ordering::Acquire));
    assert!(error.to_string().contains("panic-producer"));
    assert!(error.to_string().contains("Producer"));
}

#[tokio::test]
async fn framework_shutdown_does_not_recursively_shut_down_its_gateway_owner() {
    let harness = harness(Arc::new(FrameworkNativeProbe::default())).await;

    harness
        ._application
        .shutdown()
        .await
        .expect("Framework shutdown");

    let permit = harness
        .gateway
        .acquire_activity_permit()
        .expect("Gateway remains owned by the composition root");
    drop(permit);
    harness
        .gateway
        .shutdown_activity_runtime(false)
        .await
        .expect("Gateway shutdown");
}

#[tokio::test]
async fn native_agent_adapter_lowers_runtime_control_map_without_dispatch_name_branch() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let mut request = request(
        &harness,
        GatewaySource::new("web", "native-controls").process(),
        "control lowering",
    );
    request.policy.control_values = BTreeMap::from([
        ("model".to_string(), "model-a".to_string()),
        ("reasoning".to_string(), "high".to_string()),
        ("mode".to_string(), "plan".to_string()),
        ("permissionMode".to_string(), "dontAsk".to_string()),
    ]);

    let result = harness.send(request).await.expect("Native turn");

    let runs = backend.runs();
    let run = runs.first().expect("captured Native request");
    assert_eq!(run.model.as_deref(), Some("model-a"));
    assert_eq!(run.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(run.mode, RunMode::Plan);
    assert_eq!(run.permission_mode, Some(PermissionMode::DontAsk));
    assert!(run.runtime_options.is_empty());
    let binding = resolved_binding(&harness, &result.thread.id).await;
    assert_eq!(binding.agent_ref, None);
    assert!(!binding.agent_fingerprint.is_empty());
    assert!(
        binding
            .agent_definition_json
            .contains("psychevo.default-agent")
    );
}

#[tokio::test]
async fn bound_named_agent_ignores_current_definition_drift() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    let agents = harness.cwd.join(".psychevo/agents");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&agents).expect("agents");
    let definition = agents.join("reviewer.md");
    std::fs::write(
        &definition,
        "---\ndescription: Reviewer\n---\nReview version one.\n",
    )
    .expect("Agent Definition");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let source = GatewaySource::new("web", "agent-fingerprint").process();
    let mut first = request(&harness, source.clone(), "first");
    first.policy.agent_ref = Some("reviewer".to_string());
    first.policy.inherited_env = Some(env.clone());
    let first = harness.send(first).await.expect("first turn");
    let binding = resolved_binding(&harness, &first.thread.id).await;
    assert_eq!(binding.agent_ref.as_deref(), Some("reviewer"));
    assert!(
        binding
            .agent_definition_json
            .contains("Review version one.")
    );

    std::fs::write(
        &definition,
        "---\ndescription: Reviewer\n---\nReview version two.\n",
    )
    .expect("changed Agent Definition");
    let mut second = request(&harness, source, "second");
    second.thread_id = Some(first.thread.id);
    second.policy.inherited_env = Some(env);
    let second = harness
        .send(second)
        .await
        .expect("captured Agent Definition remains authoritative");
    let binding = resolved_binding(&harness, &second.thread.id).await;
    assert!(
        binding
            .agent_definition_json
            .contains("Review version one.")
            && !binding
                .agent_definition_json
                .contains("Review version two.")
    );
}

#[tokio::test]
async fn runtime_ref_resolves_generated_peer_backend_without_agent_selection() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP runtime."
command = "opencode"
args = ["acp"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
    )
    .expect("config");

    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            harness._temp.path().display().to_string(),
        ),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]);
    let peer = resolve_peer_turn(PeerResolutionContext {
        cwd: &harness.cwd,
        base_env: &env,
        runtime_ref: Some("opencode"),
        agent_ref: None,
        no_agents: false,
    })
    .expect("resolve peer")
    .expect("peer runtime");

    assert_eq!(peer.agent.name, "opencode");
    assert_eq!(peer.backend.id, "opencode");
}

#[tokio::test]
async fn runtime_ref_rejects_local_agent_definitions() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend).await;
    let home = harness._temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP runtime."
command = "opencode"
args = ["acp"]
entrypoints = ["peer"]
client_capabilities = ["fs.read"]
"#,
    )
    .expect("config");
    let agents_dir = harness.cwd.join(".psychevo").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(
        agents_dir.join("translate.md"),
        r#"---
name: translate
description: Translate messages.
entrypoints: [subagent]
---
Translate the prompt.
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
    let error = resolve_peer_turn(PeerResolutionContext {
        cwd: &harness.cwd,
        base_env: &env,
        runtime_ref: Some("opencode"),
        agent_ref: Some("translate"),
        no_agents: false,
    })
    .expect_err("incompatible runtime");

    assert!(
        error
            .to_string()
            .contains("ACP peer runtimes run their own modes"),
        "{error}"
    );
}

async fn collect_transcript_pages(
    harness: &super::support_peer::Harness,
    thread_id: &str,
    limit: usize,
) -> Vec<psychevo_gateway_protocol::events_transcript::TranscriptEntry> {
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = harness
            .gateway
            .thread_transcript_page(thread_id, cursor.as_deref(), limit)
            .await
            .expect("transcript page");
        assert!(page.entries.len() <= limit, "page exceeded hard limit");
        let next = page.next_cursor.clone();
        pages.push(page.entries);
        let Some(next) = next else {
            break;
        };
        cursor = Some(next);
    }
    pages.into_iter().rev().flatten().collect()
}

fn forge_same_thread_cursor(cursor: &str, mutate: impl FnOnce(&mut Value)) -> String {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).expect("decode real cursor");
    let mut value = serde_json::from_slice::<Value>(&bytes).expect("cursor JSON");
    mutate(&mut value);
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).expect("forged cursor JSON"))
}

fn write_staged_revert_for_test(
    db_path: &std::path::Path,
    thread_id: &str,
    boundary_session_seq: Option<i64>,
) {
    let connection = rusqlite::Connection::open(db_path).expect("revert connection");
    let raw_metadata = connection
        .query_row(
            "SELECT metadata_json FROM sessions WHERE id = ?1",
            [thread_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("session metadata");
    let mut metadata = raw_metadata
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .expect("valid session metadata")
        .unwrap_or_else(|| json!({}));
    let metadata = metadata.as_object_mut().expect("object session metadata");
    match boundary_session_seq {
        Some(boundary_session_seq) => {
            metadata.insert(
                "revert".to_string(),
                json!({
                    "kind": "workspaceUndo",
                    "start_seq": boundary_session_seq,
                    "original_snapshot": "test-snapshot",
                }),
            );
        }
        None => {
            metadata.remove("revert");
        }
    }
    connection
        .execute(
            "UPDATE sessions SET metadata_json = ?1 WHERE id = ?2",
            rusqlite::params![
                serde_json::to_string(&*metadata).expect("encode session metadata"),
                thread_id
            ],
        )
        .expect("write staged revert");
}

#[tokio::test]
async fn failed_terminal_keeps_its_committed_boundary_between_later_turns() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    backend.persist_history();
    let harness = harness(backend.clone()).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");

    thread
        .start_turn(psychevo::TurnRequest::new("first"))
        .await
        .expect("first accepted")
        .wait()
        .await
        .expect("first completed");
    backend.fail_next();
    let failed = thread
        .start_turn(psychevo::TurnRequest::new("failed"))
        .await
        .expect("failed accepted");
    let failed_turn_id = failed.receipt().turn_id.clone();
    failed.wait().await.expect_err("failed terminal");
    thread
        .start_turn(psychevo::TurnRequest::new("later"))
        .await
        .expect("later accepted")
        .wait()
        .await
        .expect("later completed");

    let entries = collect_transcript_pages(&harness, thread.id(), 2).await;
    let ids = entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    let terminal_id = format!("turn:{failed_turn_id}:terminal");
    assert_eq!(
        ids,
        [
            "message:1",
            "message:2",
            terminal_id.as_str(),
            "message:3",
            "message:4",
        ]
    );
}

#[tokio::test]
async fn structural_only_same_boundary_burst_pages_exactly_once() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");

    for index in 0..7 {
        backend.fail_next();
        let handle = thread
            .start_turn(psychevo::TurnRequest::new(format!("fail {index}")))
            .await
            .expect("failed Turn accepted");
        handle.wait().await.expect_err("failed Turn terminal");
    }

    let entries = collect_transcript_pages(&harness, thread.id(), 2).await;
    assert_eq!(entries.len(), 7);
    assert!(entries.iter().all(|entry| entry.message_seq.is_none()));
    assert!(entries.iter().all(|entry| entry.id.ends_with(":terminal")));
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        7
    );
}

#[tokio::test]
async fn same_thread_forged_message_cursor_is_rejected() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    backend.persist_history();
    let harness = harness(backend).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");
    thread
        .start_turn(psychevo::TurnRequest::new("visible"))
        .await
        .expect("Turn accepted")
        .wait()
        .await
        .expect("Turn completed");
    let page = harness
        .gateway
        .thread_transcript_page(thread.id(), None, 1)
        .await
        .expect("latest page");
    let cursor = page.next_cursor.expect("message cursor");
    let forged = forge_same_thread_cursor(&cursor, |value| {
        let created_at_ms = value["position"]["createdAtMs"]
            .as_i64()
            .expect("message ordering timestamp");
        value["position"]["createdAtMs"] = json!(created_at_ms.saturating_add(1));
    });

    let error = harness
        .gateway
        .thread_transcript_page(thread.id(), Some(&forged), 1)
        .await
        .err()
        .expect("forged message cursor");
    assert!(error.to_string().contains("cursor"), "{error}");
}

#[tokio::test]
async fn same_thread_forged_structural_cursor_is_rejected() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");
    for prompt in ["first failure", "second failure"] {
        backend.fail_next();
        thread
            .start_turn(psychevo::TurnRequest::new(prompt))
            .await
            .expect("Turn accepted")
            .wait()
            .await
            .expect_err("failed terminal");
    }
    let page = harness
        .gateway
        .thread_transcript_page(thread.id(), None, 1)
        .await
        .expect("latest structural page");
    let cursor = page.next_cursor.expect("structural cursor");
    let forged = forge_same_thread_cursor(&cursor, |value| {
        let boundary = value["position"]["boundarySessionSeq"]
            .as_i64()
            .expect("structural boundary");
        value["position"]["boundarySessionSeq"] = json!(boundary.saturating_add(1));
    });

    let error = harness
        .gateway
        .thread_transcript_page(thread.id(), Some(&forged), 1)
        .await
        .err()
        .expect("forged structural cursor");
    assert!(error.to_string().contains("cursor"), "{error}");
}

#[tokio::test]
async fn reverted_terminal_cursor_is_rejected_until_the_projection_is_restored() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");
    for prompt in ["first failure", "second failure"] {
        backend.fail_next();
        thread
            .start_turn(psychevo::TurnRequest::new(prompt))
            .await
            .expect("Turn accepted")
            .wait()
            .await
            .expect_err("failed terminal");
    }
    let page = harness
        .gateway
        .thread_transcript_page(thread.id(), None, 1)
        .await
        .expect("latest terminal page");
    let cursor = page.next_cursor.expect("terminal cursor");

    write_staged_revert_for_test(&harness.db_path, thread.id(), Some(0));
    let reverted = harness
        .gateway
        .thread_transcript_page(thread.id(), None, 10)
        .await
        .expect("reverted projection");
    assert!(reverted.entries.is_empty());
    let error = harness
        .gateway
        .thread_transcript_page(thread.id(), Some(&cursor), 1)
        .await
        .err()
        .expect("reverted terminal cursor");
    assert!(error.to_string().contains("cursor"), "{error}");

    write_staged_revert_for_test(&harness.db_path, thread.id(), None);
    let restored = harness
        .gateway
        .thread_transcript_page(thread.id(), Some(&cursor), 1)
        .await
        .expect("restored terminal cursor");
    assert_eq!(restored.entries.len(), 1);
    assert!(restored.entries[0].id.ends_with(":terminal"));
}

#[tokio::test]
async fn visible_history_paging_skips_an_unbounded_hidden_run_before_limit() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    backend.persist_history();
    let harness = harness(backend.clone()).await;
    let thread = harness
        ._application
        .client()
        .start_thread(psychevo::StartThreadRequest::new(&harness.cwd))
        .await
        .expect("Thread");
    thread
        .start_turn(psychevo::TurnRequest::new("older visible"))
        .await
        .expect("older Turn accepted")
        .wait()
        .await
        .expect("older Turn completed");
    backend.append_hidden_messages_on_next(20);
    thread
        .start_turn(psychevo::TurnRequest::new("hidden context"))
        .await
        .expect("hidden Turn accepted")
        .wait()
        .await
        .expect("hidden Turn completed");
    thread
        .start_turn(psychevo::TurnRequest::new("latest visible"))
        .await
        .expect("latest Turn accepted")
        .wait()
        .await
        .expect("latest Turn completed");

    assert_eq!(
        thread
            .history()
            .latest(Some(200))
            .await
            .expect("execution history")
            .items
            .len(),
        24,
        "execution history must retain inherited context"
    );
    let entries = collect_transcript_pages(&harness, thread.id(), 1).await;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["message:1", "message:2", "message:23", "message:24"]
    );
}
