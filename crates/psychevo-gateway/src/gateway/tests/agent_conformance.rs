use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psychevo::application::PermissionApprovalDecision;
use psychevo::{Application, ConfigurationQuery, PermissionMode, RunMode, ThreadAgentBinding};
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::super::Gateway;
use super::super::activity::{
    SendTurnRequest, ThreadCallerContext, ThreadSurface, ThreadTurnIntent,
};
use super::super::agent_session::{
    AgentSessionRef, AgentSessionSnapshot, CapturedAgentSessionTarget,
};
use super::super::peer_runtime::{PeerResolutionContext, resolve_peer_turn};
use super::super::results::GatewayTurnResult;
use super::support_peer::{
    FrameworkNativeProbe, Harness, WaitFirst, harness, request, send_framework_turn,
    send_framework_turn_with_handle, test_acp_command_toml,
};
use crate::GatewayEventEmitter;
use crate::acp_peer;
use crate::composition::GatewayApplication;
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, GatewayEvent, TranscriptBlockKind, TranscriptEntry,
    TranscriptEntryRole,
};
use psychevo_gateway_protocol::source::{
    GatewayInputPart, GatewaySource, GatewayThreadSelector, GatewayTurnStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentConformanceRuntime {
    Native,
    Acp,
}

impl AgentConformanceRuntime {
    fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Acp => "acp",
        }
    }
}

struct AgentConformanceHarness {
    runtime: AgentConformanceRuntime,
    inner: Harness,
    backend: Arc<FrameworkNativeProbe>,
    home: PathBuf,
    log: PathBuf,
    release_dir: PathBuf,
}

enum AgentConformanceHold {
    Native(WaitFirst),
    Acp(PathBuf),
}

impl AgentConformanceHarness {
    async fn new(runtime: AgentConformanceRuntime) -> Self {
        let backend = Arc::new(FrameworkNativeProbe::default());
        let inner = harness(backend.clone()).await;
        let home = inner._temp.path().join("home");
        let log = inner._temp.path().join("agent-conformance.jsonl");
        let release_dir = inner._temp.path().join("agent-conformance-releases");
        std::fs::create_dir_all(&home).expect("conformance home");
        std::fs::create_dir_all(&release_dir).expect("conformance release directory");

        if runtime == AgentConformanceRuntime::Native {
            backend.emit_stream_terminal();
            backend.persist_history();
        }

        if runtime == AgentConformanceRuntime::Acp {
            let fixture = crate::test_support::acp_fixture(&inner.cwd, "fake_agent_conformance");
            std::fs::write(
                home.join("config.toml"),
                format!(
                    r#"[agents.backends.conformance]
    kind = "acp"
    description = "Deterministic ACP Agent conformance fixture."
    command = {}
    args = [{}, {}, {}, {}]
    entrypoints = ["peer"]
    "#,
                    test_acp_command_toml(&inner.cwd),
                    crate::test_support::toml_path(&fixture.script),
                    crate::test_support::toml_path(&log),
                    crate::test_support::toml_path(&inner.db_path),
                    crate::test_support::toml_path(&release_dir),
                ),
            )
            .expect("ACP conformance config");
            let agents_dir = inner.cwd.join(".psychevo").join("agents");
            std::fs::create_dir_all(&agents_dir).expect("ACP conformance agents directory");
            std::fs::write(
                agents_dir.join("conformance.md"),
                r#"---
    name: conformance
    description: Deterministic ACP Agent conformance fixture.
    backend:
      ref: conformance
    entrypoints: [peer]
    ---
    Conformance fixture instructions.
    "#,
            )
            .expect("ACP conformance Agent Definition");
        }

        Self {
            runtime,
            inner,
            backend,
            home,
            log,
            release_dir,
        }
    }

    fn source(&self, case: &str, lane: &str) -> GatewaySource {
        GatewaySource::new(
            "agent-conformance",
            format!("{}:{case}:{lane}", self.runtime.label()),
        )
        .persistent()
    }

    async fn send(&self, request: SendTurnRequest) -> psychevo::Result<GatewayTurnResult> {
        send_framework_turn(
            self.inner._application.clone(),
            self.inner.gateway.clone(),
            request,
        )
        .await
    }

    fn runner(&self) -> (Application, Gateway) {
        (self.inner._application.clone(), self.inner.gateway.clone())
    }

    fn request(&self, source: GatewaySource, prompt: &str) -> SendTurnRequest {
        let mut request = request(&self.inner, source, prompt);
        if self.runtime == AgentConformanceRuntime::Acp {
            request.policy.agent_ref = Some("conformance".to_string());
            request.policy.runtime_profile_ref = Some("acp:conformance".to_string());
            request.policy.inherited_env = Some(BTreeMap::from([
                (
                    "HOME".to_string(),
                    self.inner._temp.path().display().to_string(),
                ),
                ("PSYCHEVO_HOME".to_string(), self.home.display().to_string()),
            ]));
        }
        request
    }

    fn records(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("ACP conformance record"))
            .collect()
    }

    fn observed_prompts(&self) -> Vec<String> {
        match self.runtime {
            AgentConformanceRuntime::Native => self
                .backend
                .runs()
                .into_iter()
                .map(|run| run.prompt)
                .collect(),
            AgentConformanceRuntime::Acp => self
                .records()
                .into_iter()
                .filter(|record| record["event"] == "prompt")
                .filter_map(|record| record["prompt"].as_str().map(str::to_string))
                .collect(),
        }
    }

    fn prepare_hold(&self, token: &str) -> AgentConformanceHold {
        match self.runtime {
            AgentConformanceRuntime::Native => {
                AgentConformanceHold::Native(self.backend.wait_on_next_run())
            }
            AgentConformanceRuntime::Acp => {
                AgentConformanceHold::Acp(self.release_dir.join(format!("release-{token}")))
            }
        }
    }

    async fn wait_for_hold(&self, hold: &AgentConformanceHold, prompt: &str) {
        match hold {
            AgentConformanceHold::Native(wait) => {
                tokio::time::timeout(Duration::from_secs(5), wait.started.notified())
                    .await
                    .unwrap_or_else(|_| {
                        panic!(
                            "{} Agent did not enter held prompt `{prompt}`",
                            self.runtime.label()
                        )
                    });
            }
            AgentConformanceHold::Acp(_) => {
                wait_for_agent_conformance_condition(
                    format!("ACP Agent prompt `{prompt}`"),
                    || self.observed_prompts().iter().any(|value| value == prompt),
                )
                .await;
            }
        }
    }

    fn release(&self, hold: AgentConformanceHold) {
        match hold {
            AgentConformanceHold::Native(wait) => wait.release.notify_one(),
            AgentConformanceHold::Acp(path) => {
                std::fs::write(path, "release").expect("release ACP conformance prompt");
            }
        }
    }

    fn assert_binding_was_visible_before_prompt(&self) {
        match self.runtime {
            AgentConformanceRuntime::Native => assert_eq!(
                self.backend.binding_before_run(),
                vec![true],
                "Native Adapter must observe its immutable binding before execution"
            ),
            AgentConformanceRuntime::Acp => {
                let prompt = self
                    .records()
                    .into_iter()
                    .find(|record| record["event"] == "prompt")
                    .expect("ACP prompt record");
                assert_eq!(
                    prompt["bindingBeforePrompt"], true,
                    "ACP native session id must be durably attached before prompt"
                );
            }
        }
    }

    async fn assert_exactly_one_terminal(
        &self,
        thread_id: &str,
        turn_id: &str,
        expected_status: &str,
        expected_outcome: Option<&str>,
    ) {
        let terminal = self
            .inner
            ._application
            .client()
            .framework_turn_terminal_evidence(turn_id)
            .await
            .expect("conformance terminal")
            .expect("durable conformance terminal");
        assert_eq!(terminal.thread_id, thread_id);
        assert_eq!(terminal.turn_id, turn_id);
        assert_eq!(terminal.status.as_str(), expected_status);
        assert_eq!(Some(terminal.outcome.as_str()), expected_outcome);
    }
}

async fn wait_for_agent_conformance_condition(
    label: impl Into<String>,
    mut condition: impl FnMut() -> bool,
) {
    let label = label.into();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
}

fn completion_turn_ids(events: &Arc<Mutex<Vec<GatewayEvent>>>) -> Vec<String> {
    events
        .lock()
        .expect("conformance events lock")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::TurnCompleted { turn_id, .. } => Some(turn_id.clone()),
            _ => None,
        })
        .collect()
}

fn started_turn_ids(events: &Arc<Mutex<Vec<GatewayEvent>>>) -> Vec<String> {
    events
        .lock()
        .expect("conformance events lock")
        .iter()
        .filter_map(|event| match event {
            GatewayEvent::TurnStarted { turn_id, .. } => Some(turn_id.clone()),
            _ => None,
        })
        .collect()
}

type ConformanceBlockSignature = (TranscriptBlockKind, Option<String>, Option<String>);
type ConformanceMessageSignature = (TranscriptEntryRole, Vec<ConformanceBlockSignature>);

fn conformance_message_history_signature(
    entries: &[TranscriptEntry],
) -> Vec<ConformanceMessageSignature> {
    entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.role,
                TranscriptEntryRole::User | TranscriptEntryRole::Assistant
            )
        })
        .map(|entry| {
            (
                entry.role,
                entry
                    .blocks
                    .iter()
                    .map(|block| (block.kind, block.title.clone(), block.body.clone()))
                    .collect(),
            )
        })
        .collect()
}

fn assert_visible_conformance_history(
    runtime: AgentConformanceRuntime,
    entries: &[TranscriptEntry],
    prompt: &str,
    answer: &str,
) {
    let history = conformance_message_history_signature(entries);
    assert_eq!(
        history.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![TranscriptEntryRole::User, TranscriptEntryRole::Assistant],
        "{} Agent history must expose one user/assistant pair",
        runtime.label()
    );
    assert!(
        history[0]
            .1
            .iter()
            .any(|(_, _, body)| body.as_deref() == Some(prompt)),
        "{} Agent history must expose the submitted prompt",
        runtime.label()
    );
    assert!(
        history[1]
            .1
            .iter()
            .any(|(_, _, body)| body.as_deref() == Some(answer)),
        "{} Agent history must expose the terminal answer",
        runtime.label()
    );
}

async fn conformance_success_persists_visible_history_single_terminal_and_certain_delivery(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let events_for_sink = events.clone();
    let mut request = harness.request(harness.source("terminal", "one"), "legacy prompt");
    request.input = vec![GatewayInputPart::Text {
        text: "binding-terminal".to_string(),
    }];
    request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        events_for_sink
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));

    let result = harness
        .send(request)
        .await
        .unwrap_or_else(|error| panic!("{} conformance turn: {error}", runtime.label()));

    harness.assert_binding_was_visible_before_prompt();
    assert_eq!(result.turn.status, GatewayTurnStatus::Completed);
    assert_eq!(started_turn_ids(&events), vec![result.turn.id.clone()]);
    assert_eq!(completion_turn_ids(&events), vec![result.turn.id.clone()]);
    harness
        .assert_exactly_one_terminal(
            &result.thread.id,
            &result.turn.id,
            "completed",
            Some("normal"),
        )
        .await;
    assert_visible_conformance_history(
        runtime,
        &result.committed_entries,
        "binding-terminal",
        &result.result.final_answer,
    );
    let committed_signature = conformance_message_history_signature(&result.committed_entries);

    let visible_history = harness
        .inner
        .gateway
        .thread_transcript(&result.thread.id)
        .await
        .expect("visible conformance history");
    assert_visible_conformance_history(
        runtime,
        &visible_history,
        "binding-terminal",
        &result.result.final_answer,
    );
    let visible_signature = conformance_message_history_signature(&visible_history);
    assert_eq!(visible_signature, committed_signature);
    let db_path = harness.inner.db_path.clone();

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown binding conformance harness");

    let reopened = GatewayApplication::open(harness.home.clone(), db_path, None, BTreeMap::new())
        .await
        .expect("reopen conformance composition");
    let persisted_history = reopened
        .gateway()
        .thread_transcript(&result.thread.id)
        .await
        .expect("persisted conformance history");
    assert_eq!(
        conformance_message_history_signature(&persisted_history),
        visible_signature,
        "{} history must survive a fresh Application/Gateway reader",
        runtime.label()
    );
    reopened
        .shutdown()
        .await
        .expect("shutdown reopened Framework reader");
}

async fn conformance_per_thread_ordering_and_cross_thread_concurrency(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let same_source = harness.source("ordering", "same");
    let same_hold = harness.prepare_hold("same");
    let (first_application, first_gateway) = harness.runner();
    let first_request = harness.request(same_source.clone(), "hold:same");
    let first = tokio::spawn(async move {
        send_framework_turn(first_application, first_gateway, first_request).await
    });
    harness.wait_for_hold(&same_hold, "hold:same").await;

    let (second_application, second_gateway) = harness.runner();
    let second_request = harness.request(same_source.clone(), "second:same");
    let second = tokio::spawn(async move {
        send_framework_turn(second_application, second_gateway, second_request).await
    });
    let same_thread_id = harness
        .inner
        .gateway
        .resolve_source_thread(&same_source)
        .await
        .expect("same-thread source lookup")
        .expect("same-thread source binding");
    let same_thread = harness
        .inner
        ._application
        .client()
        .resume_thread(&same_thread_id)
        .await
        .expect("same Framework Thread");
    wait_for_agent_conformance_condition("same-thread turn to enter Framework queue", || {
        same_thread.activity().queued_turns == 1
    })
    .await;
    assert_eq!(
        harness.observed_prompts(),
        vec!["hold:same".to_string()],
        "{} Agent must not deliver the queued same-thread prompt early",
        runtime.label()
    );
    harness.release(same_hold);
    let first = first
        .await
        .expect("first same-thread task")
        .expect("first same-thread turn");
    let second = second
        .await
        .expect("second same-thread task")
        .expect("second same-thread turn");
    assert_eq!(first.thread.id, second.thread.id);
    assert_eq!(
        harness.observed_prompts(),
        vec!["hold:same".to_string(), "second:same".to_string()]
    );
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown ordering harness");

    let harness = AgentConformanceHarness::new(runtime).await;
    let concurrent_hold = harness.prepare_hold("concurrent");
    let (first_application, first_gateway) = harness.runner();
    let first_request = harness.request(harness.source("concurrent", "one"), "hold:concurrent");
    let first = tokio::spawn(async move {
        send_framework_turn(first_application, first_gateway, first_request).await
    });
    harness
        .wait_for_hold(&concurrent_hold, "hold:concurrent")
        .await;

    let (second_application, second_gateway) = harness.runner();
    let second_request = harness.request(harness.source("concurrent", "two"), "second:concurrent");
    let mut second = tokio::spawn(async move {
        send_framework_turn(second_application, second_gateway, second_request).await
    });
    let second = tokio::time::timeout(Duration::from_secs(5), &mut second)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{} Agent serialized a different thread behind the held thread",
                runtime.label()
            )
        })
        .expect("concurrent second task")
        .expect("concurrent second turn");
    assert!(
        harness
            .observed_prompts()
            .iter()
            .any(|prompt| prompt == "second:concurrent")
    );
    harness.release(concurrent_hold);
    let first = first
        .await
        .expect("concurrent first task")
        .expect("concurrent first turn");
    assert_ne!(first.thread.id, second.thread.id);
    if runtime == AgentConformanceRuntime::Acp {
        assert_eq!(
            harness
                .records()
                .iter()
                .filter(|record| record["event"] == "initialize")
                .count(),
            1,
            "different ACP sessions must multiplex on one resident generation"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown concurrency harness");
}

async fn conformance_rejects_unsupported_control_and_structured_input(
    runtime: AgentConformanceRuntime,
) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));

    let mut control_request = harness.request(
        harness.source("reject", "control"),
        "must-not-reach-agent-control",
    );
    control_request
        .policy
        .control_values
        .insert("conformance-unsupported".to_string(), "value".to_string());
    let control_events = events.clone();
    control_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        control_events
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let control_error = harness
        .send(control_request)
        .await
        .expect_err("unsupported control must be rejected");
    assert!(
        control_error
            .to_string()
            .contains("conformance-unsupported"),
        "{control_error}"
    );
    assert!(harness.observed_prompts().is_empty());
    wait_for_agent_conformance_condition("rejected control terminal event", || {
        !completion_turn_ids(&events).is_empty()
    })
    .await;
    assert_eq!(completion_turn_ids(&events).len(), 1);

    let mut resource_request = harness.request(
        harness.source("reject", "resource"),
        "must-not-reach-agent-resource",
    );
    resource_request.input = vec![GatewayInputPart::Resource {
        uri: "psychevo://conformance/resource".to_string(),
        mime_type: Some("text/plain".to_string()),
        text: Some("structured resource".to_string()),
        blob: None,
    }];
    let resource_events = events.clone();
    resource_request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        resource_events
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let resource_error = harness
        .send(resource_request)
        .await
        .expect_err("unsupported structured input must be rejected");
    assert!(
        resource_error.to_string().contains("resource")
            || resource_error.to_string().contains("embedded-context"),
        "{resource_error}"
    );
    assert!(harness.observed_prompts().is_empty());
    wait_for_agent_conformance_condition("rejected resource terminal event", || {
        completion_turn_ids(&events).len() >= 2
    })
    .await;
    assert_eq!(completion_turn_ids(&events).len(), 2);

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown rejection harness");
}

async fn conformance_cancel_produces_one_interrupted_terminal(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let source = harness.source("cancel", "one");
    let hold = harness.prepare_hold("cancel");
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let events_for_sink = events.clone();
    let (handle_tx, handle_rx) = oneshot::channel();
    let mut request = harness.request(source, "hold:cancel");
    request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        events_for_sink
            .lock()
            .expect("conformance events lock")
            .push(event);
    }));
    let (application, gateway) = harness.runner();
    let turn = tokio::spawn(async move {
        send_framework_turn_with_handle(application, gateway, request, handle_tx).await
    });
    harness.wait_for_hold(&hold, "hold:cancel").await;
    handle_rx
        .await
        .expect("accepted Framework Turn handle")
        .interrupt();

    let result = tokio::time::timeout(Duration::from_secs(5), turn)
        .await
        .unwrap_or_else(|_| panic!("{} Agent ignored cancel", runtime.label()))
        .expect("cancel task")
        .expect("cancel result");
    assert_eq!(result.result.outcome, psychevo::TurnOutcome::Interrupted);
    assert_eq!(result.turn.status, GatewayTurnStatus::Interrupted);
    assert_eq!(completion_turn_ids(&events), vec![result.turn.id.clone()]);
    harness
        .assert_exactly_one_terminal(
            &result.thread.id,
            &result.turn.id,
            "interrupted",
            Some("aborted"),
        )
        .await;
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "cancel"),
            "ACP cancellation must send session/cancel"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown cancel harness");
}

async fn conformance_permission_interaction_is_accepted_once(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime).await;
    if runtime == AgentConformanceRuntime::Native {
        harness.backend.request_permission();
    }
    let source = harness.source("interaction", "one");
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut request = harness.request(source.clone(), "permission");
    request.event_sink = Some(GatewayEventEmitter::new(move |event| {
        let _ = event_tx.send(event);
    }));
    let (application, gateway) = harness.runner();
    let turn =
        tokio::spawn(async move { send_framework_turn(application, gateway, request).await });

    let action_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(GatewayEvent::ActionRequested { action }) = event_rx.recv().await
                && action.kind == GatewayActionKind::Permission
            {
                break action.action_id;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{} Agent did not request permission", runtime.label()));
    assert_eq!(action_id, "permission-1");

    let concurrent_source = harness.source("interaction", "concurrent");
    let (concurrent_application, concurrent_gateway) = harness.runner();
    let concurrent_request = harness.request(concurrent_source, "interaction-concurrent");
    let mut concurrent = tokio::spawn(async move {
        send_framework_turn(
            concurrent_application,
            concurrent_gateway,
            concurrent_request,
        )
        .await
    });
    let concurrent_result = tokio::time::timeout(Duration::from_secs(5), &mut concurrent).await;
    if concurrent_result.is_err() {
        let _ = harness
            .inner
            .gateway
            .submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                &action_id,
                PermissionApprovalDecision::deny(),
            )
            .await;
        let _ = turn.await;
        panic!(
            "{} pending interaction blocked an independent thread",
            runtime.label()
        );
    }
    concurrent_result
        .expect("checked interaction concurrency timeout")
        .expect("interaction concurrency task")
        .expect("interaction concurrency turn");

    let interaction_thread_id = harness
        .inner
        .gateway
        .resolve_source_thread(&source)
        .await
        .expect("interaction source lookup")
        .expect("interaction source binding");
    let interaction_thread = harness
        .inner
        ._application
        .client()
        .resume_thread(&interaction_thread_id)
        .await
        .expect("interaction Framework Thread");
    assert!(
        interaction_thread
            .respond(
                &action_id,
                psychevo::InteractionResponse::Permission(
                    PermissionApprovalDecision::allow_once(),
                ),
            )
            .await
            .expect("first permission response")
            .accepted
    );
    assert!(
        !interaction_thread
            .respond(
                &action_id,
                psychevo::InteractionResponse::Permission(
                    PermissionApprovalDecision::allow_once(),
                ),
            )
            .await
            .expect("duplicate permission response")
            .accepted,
        "{} interaction token must be consumed exactly once",
        runtime.label()
    );
    turn.await
        .expect("permission turn task")
        .expect("permission turn");

    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(GatewayEvent::ActionResolved {
                kind: GatewayActionKind::Permission,
                outcome,
                ..
            }) = event_rx.recv().await
            {
                break outcome;
            }
        }
    })
    .await
    .expect("permission resolved event");
    assert_eq!(outcome, GatewayActionOutcome::Accepted);
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "permission_response"),
            "ACP Agent must receive the accepted interaction response"
        );
    }
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown interaction harness");
}

async fn conformance_shutdown_is_graceful(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let source = harness.source("shutdown", "one");
    harness
        .send(harness.request(source.clone(), "shutdown"))
        .await
        .expect("turn before shutdown");
    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .unwrap_or_else(|error| panic!("{} graceful shutdown: {error}", runtime.label()));
    assert!(
        !harness
            .inner
            .gateway
            .local_activity_for_selector(&GatewayThreadSelector::source(source.source_key()))
            .running
    );
    if runtime == AgentConformanceRuntime::Acp {
        assert!(
            harness
                .records()
                .iter()
                .any(|record| record["event"] == "close"),
            "graceful ACP shutdown must close resident sessions"
        );
    }
}

async fn conformance_shared_agent_session_transact_seam(runtime: AgentConformanceRuntime) {
    let harness = AgentConformanceHarness::new(runtime).await;
    let source = harness.source("transact", "one");
    let turn = harness
        .send(harness.request(source.clone(), "transact-run"))
        .await
        .unwrap_or_else(|error| panic!("{} transact run: {error}", runtime.label()));
    assert_eq!(harness.observed_prompts(), vec!["transact-run".to_string()]);

    let binding = match harness
        .inner
        ._application
        .client()
        .thread_agent_binding(&turn.thread.id)
        .await
        .expect("transact binding lookup")
        .expect("transact binding")
    {
        ThreadAgentBinding::Resolved { binding, .. } => binding,
        ThreadAgentBinding::Unresolved { .. } => panic!("transact binding is unresolved"),
    };
    let request = harness.request(source, "unused");
    let cwd = request.cwd;
    let policy = request.policy;
    let profile: psychevo::config::RuntimeProfileConfig =
        serde_json::from_str(&binding.profile_config_json).expect("captured profile");
    let peer = if runtime == AgentConformanceRuntime::Acp {
        Some(
            resolve_peer_turn(PeerResolutionContext {
                cwd: &cwd,
                base_env: policy
                    .inherited_env
                    .as_ref()
                    .expect("conformance inherited environment"),
                runtime_ref: profile.backend_ref.as_deref(),
                agent_ref: policy.agent_ref.as_deref(),
                no_agents: policy.no_agents,
            })
            .expect("transact ACP peer resolution")
            .expect("transact ACP peer is available"),
        )
    } else {
        None
    };
    let mcp_servers = if let Some(peer) = peer.as_ref() {
        let mut query = ConfigurationQuery::new(&cwd);
        query.inherited_env = policy.inherited_env.clone();
        let configuration = harness
            .inner
            ._application
            .client()
            .configuration(query)
            .expect("transact configuration");
        acp_peer::stdio_turn::resolve_peer_mcp_server_handoffs(peer, &configuration)
            .await
            .expect("transact MCP handoff")
    } else {
        Vec::new()
    };
    let target = CapturedAgentSessionTarget::application_bound(&binding, profile, peer)
        .expect("capture bound Agent session target");
    let session = AgentSessionRef {
        cwd,
        local_session_id: turn.thread.id.clone(),
        native_session_id: binding
            .native_session_id
            .clone()
            .expect("transact native session id"),
        mcp_servers,
    };

    let attached = harness
        .inner
        .gateway
        .agent_sessions
        .attach(target)
        .expect("attach shared Agent session target");
    let inspected = attached
        .inspect(session.clone())
        .await
        .expect("typed inspection response");
    match (runtime, inspected) {
        (AgentConformanceRuntime::Native, AgentSessionSnapshot::Native { profile_id }) => {
            assert_eq!(profile_id, "native");
            let error = attached
                .set_control(session, "fast".to_string(), Value::Bool(true))
                .await
                .expect_err("Native live session control must fail closed");
            assert_eq!(
                error
                    .structured_data()
                    .and_then(|data| data["code"].as_str()),
                Some("unsupported_control")
            );
        }
        (AgentConformanceRuntime::Acp, AgentSessionSnapshot::Acp(snapshot)) => {
            assert!(snapshot.options.iter().any(|option| {
                option.id == "fast" && option.current_value.as_deref() == Some("false")
            }));
            let controlled = attached
                .set_control(session, "fast".to_string(), Value::Bool(true))
                .await
                .expect("typed ACP control response");
            let AgentSessionSnapshot::Acp(snapshot) = controlled else {
                panic!("ACP control returned a non-ACP snapshot");
            };
            assert!(snapshot.options.iter().any(|option| {
                option.id == "fast" && option.current_value.as_deref() == Some("true")
            }));
            assert!(harness.records().iter().any(|record| {
                record["event"] == "set_control"
                    && record["controlId"] == "fast"
                    && record["value"] == true
            }));
        }
        (runtime, _) => panic!(
            "{} inspection returned the wrong snapshot kind",
            runtime.label()
        ),
    }

    harness
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown transact conformance harness");
}

async fn run_agent_conformance(runtime: AgentConformanceRuntime) {
    conformance_success_persists_visible_history_single_terminal_and_certain_delivery(runtime)
        .await;
    conformance_per_thread_ordering_and_cross_thread_concurrency(runtime).await;
    conformance_rejects_unsupported_control_and_structured_input(runtime).await;
    conformance_cancel_produces_one_interrupted_terminal(runtime).await;
    conformance_permission_interaction_is_accepted_once(runtime).await;
    conformance_shutdown_is_graceful(runtime).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_conformance_native() {
    run_agent_conformance(AgentConformanceRuntime::Native).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_conformance_acp() {
    run_agent_conformance(AgentConformanceRuntime::Acp).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_session_transact_conformance_native() {
    conformance_shared_agent_session_transact_seam(AgentConformanceRuntime::Native).await;
}

#[tokio::test]
async fn agent_session_attach_is_idempotent_for_captured_binding_and_rejects_conflict() {
    let harness = AgentConformanceHarness::new(AgentConformanceRuntime::Native).await;
    let source = harness.source("attachment-identity", "one");
    let turn = harness
        .send(harness.request(source.clone(), "capture binding"))
        .await
        .expect("initial Native turn");
    let binding = match harness
        .inner
        ._application
        .client()
        .thread_agent_binding(&turn.thread.id)
        .await
        .expect("binding lookup")
        .expect("captured binding")
    {
        ThreadAgentBinding::Resolved { binding, .. } => binding,
        ThreadAgentBinding::Unresolved { .. } => panic!("captured binding is unresolved"),
    };
    let profile: psychevo::config::RuntimeProfileConfig =
        serde_json::from_str(&binding.profile_config_json).expect("Native profile");

    harness
        .inner
        .gateway
        .agent_sessions
        .attach(
            CapturedAgentSessionTarget::application_bound(&binding, profile.clone(), None)
                .expect("same captured target"),
        )
        .expect("initial captured target attachment");
    let attachment_count = harness
        .inner
        .gateway
        .agent_sessions
        .bound_attachment_count();
    harness
        .inner
        .gateway
        .agent_sessions
        .attach(
            CapturedAgentSessionTarget::application_bound(&binding, profile.clone(), None)
                .expect("same captured target"),
        )
        .expect("same captured target reattaches idempotently");
    assert_eq!(
        harness
            .inner
            .gateway
            .agent_sessions
            .bound_attachment_count(),
        attachment_count,
        "idempotent attach must not create another attachment identity"
    );

    let mut conflicting_binding = binding;
    conflicting_binding.agent_fingerprint = "different-agent-fingerprint".to_string();
    let error = harness
        .inner
        .gateway
        .agent_sessions
        .attach(
            CapturedAgentSessionTarget::application_bound(&conflicting_binding, profile, None)
                .expect("construct conflicting captured target"),
        )
        .err()
        .expect("same thread/revision with another capture must fail");
    assert_eq!(
        error
            .structured_data()
            .and_then(|data| data["code"].as_str()),
        Some("agent_session_attachment_conflict")
    );
}

#[tokio::test]
async fn thread_application_run_turn_lowers_typed_caller_intent() {
    let backend = Arc::new(FrameworkNativeProbe::default());
    let harness = harness(backend.clone()).await;
    let source = GatewaySource::new("application-conformance", "typed-turn").invocation();
    let mut caller = ThreadCallerContext::new(
        ThreadSurface::Other("conformance".to_string()),
        harness.cwd.clone(),
    );
    caller.runtime_source = "application-conformance".to_string();
    let mut intent = ThreadTurnIntent::new(vec![GatewayInputPart::Text {
        text: "typed application input".to_string(),
    }]);
    let mut start = psychevo::StartThreadRequest::new(&harness.cwd);
    start.source = "application-conformance".to_string();
    let thread = harness
        ._application
        .client()
        .start_thread(start)
        .await
        .expect("materialize typed Thread");
    intent.thread_id = Some(thread.id().to_string());
    intent.source = Some(source.clone());
    intent.turn_id = Some(Uuid::now_v7().to_string());
    intent.policy.runtime_profile_ref = Some("native".to_string());
    intent.policy.model = Some("surface-model".to_string());
    intent.policy.reasoning_effort = Some("high".to_string());
    intent.policy.mode = RunMode::Plan;
    intent.policy.permission_mode = Some(PermissionMode::DontAsk);

    let submission = intent
        .into_framework_request(caller)
        .expect("lower typed Thread Application turn");
    thread
        .start_turn(submission.request)
        .await
        .expect("accept typed Thread Application turn")
        .wait()
        .await
        .expect("complete typed Thread Application turn");
    let runs = backend.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].prompt, "typed application input");
    assert_eq!(runs[0].model.as_deref(), Some("surface-model"));
    assert_eq!(runs[0].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(runs[0].mode, RunMode::Plan);
    assert_eq!(runs[0].permission_mode, Some(PermissionMode::DontAsk));
    assert!(
        harness
            ._application
            .client()
            .thread_agent_binding(runs[0].session.as_deref().expect("public Thread session"))
            .await
            .expect("binding lookup")
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_session_transact_conformance_acp() {
    conformance_shared_agent_session_transact_seam(AgentConformanceRuntime::Acp).await;
}
