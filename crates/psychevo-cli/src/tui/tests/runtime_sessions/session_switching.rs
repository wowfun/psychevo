use crate::tui::tests::fixtures::{
    attach_background_agent_running, attach_pending_framework_agent_running,
    drain_starting_turn_cleanups, finished_turn_result, install_accepted_turn_admission,
    install_pending_turn_admission, test_app, test_shell_running_control,
};
use crate::tui::tests::{
    insert_tui_message, insert_tui_message_with_metadata, runtime_turn_event, start_thread_fixture,
};
use crate::tui::{
    AgentMissionRegistration, BottomPanel, BottomSelectionPanel, BottomSelectionValue,
    FullscreenUi, KeyCode, KeyEvent, KeyModifiers, PermissionMode, PresentedShellEvent,
    QueuedInput, RunMode, RunningTask, RunningTurn, RunningTurnControl, RunningTurnEvents,
    SessionListView, ShellCommandEvent, ShellCommandOutcome, ShellCommandResult,
    SideConversationSurface, SlashCommand, StartSideConversationRequest,
    TUI_SIDE_CONVERSATION_SESSION_SOURCE, ThreadModelSelection, TranscriptKind, TuiApprovalHandler,
    TuiLiveEvent, TurnEvent, TurnResult, session_project_label, short_session, textarea_text,
    visible_transcript_message_count, wall_now_ms,
};
use psychevo::{
    Application, ApprovalHandler,
    application::{PermissionApprovalDecision, PermissionApprovalRequest},
};
use std::collections::VecDeque;
use std::fs;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;

#[tokio::test]
pub(crate) async fn remembered_thread_lookup_propagates_non_missing_client_errors() {
    let temp = tempdir().expect("temp");
    let application = Application::builder()
        .home(temp.path())
        .database_path(":memory:")
        .build()
        .await
        .expect("Application");
    let client = application.client();
    application.shutdown().await.expect("shutdown");

    let error = client
        .try_resume_thread("remembered-thread")
        .await
        .expect_err("closed Client error must not become missing");
    assert!(error.to_string().contains("shutting down"), "{error:#}");
}

#[tokio::test]
pub(crate) async fn new_cancels_pending_admission_without_restoring_its_draft() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "old pending prompt");
    ui.set_composer_text("new session draft");

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert!(release.send(()).is_ok(), "cleanup did not retain admission");
    assert!(ui.starting_turn.is_none());
    assert_eq!(ui.starting_turn_cleanups.len(), 1);
    assert!(ui.running.is_none());
    assert_eq!(app.current_session, None);
    assert_eq!(textarea_text(&ui.textarea), "new session draft");
    assert!(ui.transcript.is_empty());

    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    assert!(ui.starting_turn_cleanups.is_empty());
    assert_eq!(app.current_session, None);
    assert_eq!(textarea_text(&ui.textarea), "new session draft");
}

#[tokio::test]
pub(crate) async fn explicit_session_switch_cancels_pending_admission_without_rebinding() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first);
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "old pending prompt");
    ui.set_composer_text("selected session draft");

    app.open_session_direct(&mut ui, &second)
        .await
        .expect("switch session");

    assert!(release.send(()).is_ok(), "cleanup did not retain admission");
    assert!(ui.starting_turn.is_none());
    assert_eq!(ui.starting_turn_cleanups.len(), 1);
    assert!(ui.running.is_none());
    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert_eq!(textarea_text(&ui.textarea), "selected session draft");
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("old pending prompt"))
    );

    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    assert!(ui.starting_turn_cleanups.is_empty());
    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert_eq!(textarea_text(&ui.textarea), "selected session draft");
}

#[tokio::test]
pub(crate) async fn invalid_session_switch_keeps_pending_foreground_unchanged() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let current = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    app.current_session = Some(current.clone());
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "pending prompt");
    let queue_owner = ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .queue_owner_id
        .clone();
    app.queue_fullscreen_prompt(
        &mut ui,
        "queued prompt".to_string(),
        "queued prompt".to_string(),
        Vec::new(),
    );
    let transcript_before = ui
        .transcript
        .iter()
        .map(|row| (row.kind, row.title.clone(), row.text.clone()))
        .collect::<Vec<_>>();

    let error = app
        .open_session_direct(&mut ui, "missing-session")
        .await
        .expect_err("missing destination");
    assert!(error.to_string().contains("missing-session"), "{error:#}");
    assert_eq!(app.current_session.as_deref(), Some(current.as_str()));
    assert_eq!(
        ui.starting_turn
            .as_ref()
            .map(|starting| starting.queue_owner_id.as_str()),
        Some(queue_owner.as_str())
    );
    assert!(ui.starting_turn_cleanups.is_empty());
    assert_eq!(
        ui.transcript
            .iter()
            .map(|row| (row.kind, row.title.clone(), row.text.clone()))
            .collect::<Vec<_>>(),
        transcript_before
    );
    assert_eq!(ui.queued_inputs.len(), 1);

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("cleanup");
    release.send(()).expect("cleanup retained admission");
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
}

#[tokio::test]
pub(crate) async fn queued_mission_owned_by_starting_turn_is_not_retargeted_on_session_switch() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let destination = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "new draft prompt");
    let owner = ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .queue_owner_id
        .clone();
    app.queue_fullscreen_prompt_with_mission(
        &mut ui,
        "mission prompt".to_string(),
        "/mission ship".to_string(),
        Vec::new(),
        Some(AgentMissionRegistration {
            id: "queued-mission".to_string(),
            goal: "ship".to_string(),
            lead_agent_name: "general".to_string(),
            team: None,
            metadata: None,
        }),
    );
    assert_eq!(
        crate::tui::queued_input_session_id(ui.queued_inputs.front().expect("mission")),
        Some(owner.as_str())
    );

    app.open_session_direct(&mut ui, &destination)
        .await
        .expect("switch");
    assert!(ui.queued_inputs.is_empty());
    release.send(()).expect("cleanup retained admission");
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    assert_eq!(app.current_session.as_deref(), Some(destination.as_str()));
}

#[tokio::test]
pub(crate) async fn compact_during_new_turn_admission_uses_the_private_queue_owner() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "new draft prompt");
    let owner = ui
        .starting_turn
        .as_ref()
        .expect("starting Turn")
        .queue_owner_id
        .clone();

    app.submit_fullscreen_compaction(
        &mut ui,
        Some("retain decisions".to_string()),
        "/compact retain decisions".to_string(),
    )
    .expect("queue compaction");

    assert!(matches!(
        ui.queued_inputs.front(),
        Some(QueuedInput::Compact {
            session_id,
            instructions: Some(instructions),
            ..
        }) if session_id.as_deref() == Some(owner.as_str())
            && instructions == "retain decisions"
    ));
    assert!(ui.transcript.iter().all(|row| {
        !(row.kind == TranscriptKind::Error && row.text.contains("no session context"))
    }));

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");
    assert!(ui.queued_inputs.is_empty());
    release.send(()).expect("cleanup retained admission");
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
}

#[tokio::test]
pub(crate) async fn fullscreen_teardown_interrupts_and_joins_auxiliary_agent_owner() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    attach_pending_framework_agent_running(&app, &mut ui).await;
    app.detach_foreground_for_session_switch(&mut ui, None)
        .await;
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);

    app.settle_fullscreen_task_owners(&mut ui).await;

    assert!(ui.auxiliary_agent_tasks.is_empty());
    assert!(ui.auxiliary_shell_tasks.is_empty());
}

#[tokio::test]
pub(crate) async fn pending_contextual_shell_moves_with_its_turn_across_session_switch() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let destination = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    attach_pending_framework_agent_running(&app, &mut ui).await;
    let owner_session = ui
        .running
        .as_ref()
        .and_then(|running| running.session_id.clone())
        .expect("accepted owner session");
    ui.start_assistant();

    app.submit_fullscreen_shell(&mut ui, "printf owner-shell".to_string())
        .expect("queue contextual shell");
    assert_eq!(ui.pending_auxiliary_shell_commands.len(), 1);

    app.open_session_direct(&mut ui, &destination)
        .await
        .expect("switch");
    assert_eq!(ui.pending_auxiliary_shell_commands.len(), 1);

    let (_foreground_tx, foreground_rx) = mpsc::unbounded_channel();
    ui.running = Some(RunningTurn {
        session_id: Some(destination.clone()),
        control: test_shell_running_control(&app),
        selector: None,
        turn_id: Some("destination-turn".to_string()),
        events: RunningTurnEvents::TurnTest(foreground_rx),
        task: RunningTask::Agent(tokio::spawn(async {
            std::future::pending::<psychevo::Result<TurnResult>>().await
        })),
    });
    ui.start_assistant();
    app.apply_fullscreen_turn_event(
        &mut ui,
        runtime_turn_event(serde_json::json!({
            "type": "run_start",
            "session_id": destination,
            "provider": "mock",
            "model": "model-b",
            "mode": "default"
        })),
    );
    assert!(ui.auxiliary_shell_tasks.is_empty());
    assert_eq!(ui.pending_auxiliary_shell_commands.len(), 1);

    let foreground = ui.running.take().expect("destination foreground");
    foreground.task.abort();
    ui.finish_turn();
    let mut owner = ui.auxiliary_agent_tasks.remove(0);
    assert!(app.apply_pending_auxiliary_agent_live_events(
        &mut ui,
        &mut owner,
        VecDeque::from([TuiLiveEvent::Turn(runtime_turn_event(serde_json::json!({
            "type": "run_start",
            "session_id": owner_session,
            "provider": "mock",
            "model": "model-a",
            "mode": "default"
        })))]),
    ));
    assert!(ui.pending_auxiliary_shell_commands.is_empty());
    assert_eq!(ui.auxiliary_shell_tasks.len(), 1);
    assert_eq!(
        ui.auxiliary_shell_tasks[0].session_id.as_deref(),
        owner.session_id.as_deref()
    );

    ui.auxiliary_agent_tasks.push(owner);
    app.settle_fullscreen_task_owners(&mut ui).await;
}

#[tokio::test]
pub(crate) async fn new_cleans_up_turn_accepted_before_admission_result_is_observed() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let mut admission =
        install_accepted_turn_admission(&app, &mut ui, "durably accepted old prompt").await;
    admission.wait_until_accepted().await;
    ui.set_composer_text("new session draft");

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert!(ui.starting_turn.is_none());
    assert_eq!(ui.starting_turn_cleanups.len(), 1);
    assert_eq!(app.current_session, None);
    assert_eq!(textarea_text(&ui.textarea), "new session draft");
    admission.release_to_tui_cleanup();
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    admission.assert_turn_settled().await;
    admission.shutdown().await;
}

#[tokio::test]
pub(crate) async fn escape_restores_input_and_cleans_up_a_raced_accepted_turn() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let mut admission =
        install_accepted_turn_admission(&app, &mut ui, "accepted prompt to restore").await;
    admission.wait_until_accepted().await;
    ui.set_composer_text("newer draft");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("escape");

    assert!(ui.starting_turn.is_none());
    assert_eq!(ui.starting_turn_cleanups.len(), 1);
    assert_eq!(
        textarea_text(&ui.textarea),
        "accepted prompt to restore\nnewer draft"
    );
    admission.release_to_tui_cleanup();
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    admission.assert_turn_settled().await;
    admission.shutdown().await;
}

#[tokio::test]
pub(crate) async fn ctrl_c_cancels_starting_turn_without_requesting_quit() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);
    let release = install_pending_turn_admission(&app, &mut ui, "prompt to restore");

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await
        .expect("Ctrl+C");

    assert!(!should_quit);
    assert!(!ui.quit_requested);
    assert_eq!(textarea_text(&ui.textarea), "prompt to restore");
    release.send(()).expect("cleanup retained admission");
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
}

#[tokio::test]
pub(crate) async fn running_session_switch_buffers_stream_until_return() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &second,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "second prompt"}],
            "timestamp_ms": 1
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async { std::future::pending::<psychevo::Result<TurnResult>>().await });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();
    ui.running_elapsed_override = Some(Duration::from_secs(9));

    tx.send(TurnEvent::ReasoningDelta {
        text: "first session visible before switch".to_string(),
    })
    .expect("send visible stream");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain visible stream");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking
            && row.text.contains("first session visible before switch")
    }));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    for ch in second.chars().take(8) {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .await
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("select");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert_eq!(
        ui.status_running_elapsed(app.current_session.as_deref()),
        None
    );

    tx.send(TurnEvent::ReasoningDelta {
        text: "first session hidden stream".to_string(),
    })
    .expect("send hidden stream");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain hidden stream");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("first session hidden stream"))
    );

    app.open_session_direct(&mut ui, &first)
        .await
        .expect("switch back to first");

    assert_eq!(app.current_session.as_deref(), Some(first.as_str()));
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking && row.text.contains("first session hidden stream")
    }));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking
            && row.text.contains("first session visible before switch")
    }));

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn switched_background_turn_keeps_approval_path_after_second_submission() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first.clone());
    let mut ui = FullscreenUi::new(&app);
    let (_events_tx, events_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async { std::future::pending::<psychevo::Result<TurnResult>>().await });
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control: test_shell_running_control(&app),
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(events_rx),
        task: RunningTask::Agent(task),
    });
    let (approval_tx, approval_rx) = mpsc::unbounded_channel();
    ui.approval_rx = Some(approval_rx);

    app.open_session_direct(&mut ui, &second)
        .await
        .expect("switch");
    assert!(ui.approval_rx.is_none());
    assert!(ui.auxiliary_agent_tasks[0].approval_rx.is_some());
    app.start_fullscreen_turn(
        &mut ui,
        "second foreground".to_string(),
        "second foreground".to_string(),
        Vec::new(),
    )
    .expect("second foreground submission");
    assert!(ui.approval_rx.is_none());

    let handler = TuiApprovalHandler {
        session_id: Some(first.clone()),
        sender: approval_tx,
    };
    let approval = tokio::spawn(handler.request_permission(PermissionApprovalRequest {
        tool_call_id: "background-child-approval".to_string(),
        tool_name: "write".to_string(),
        summary: "background child write".to_string(),
        reason: "test".to_string(),
        matched_rule: None,
        suggested_rule: None,
        allow_always: false,
        filesystem: None,
        mcp_startup: None,
        timeout_secs: 300,
    }));
    tokio::task::yield_now().await;
    assert!(ui.drain_permission_approval_requests());
    let Some(BottomPanel::PermissionApproval(panel)) = ui.bottom_panel.as_ref() else {
        panic!("background approval did not surface");
    };
    assert_eq!(panel.session_id.as_deref(), Some(first.as_str()));

    handler
        .cancel_permission_with_reason("background-child-approval", "aborted")
        .await;
    ui.drain_permission_approval_requests();
    assert_eq!(
        approval.await.expect("approval task"),
        PermissionApprovalDecision::deny()
    );
    assert!(app.request_current_session_interrupt(&mut ui).await);
    drain_starting_turn_cleanups(&mut app, &mut ui).await;
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn approval_queue_preserves_request_order_across_receivers() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let foreground_session =
        start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let background_session =
        start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(foreground_session.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_background_agent_running(&app, &mut ui, &background_session);

    let (foreground_tx, foreground_rx) = mpsc::unbounded_channel();
    let (background_tx, background_rx) = mpsc::unbounded_channel();
    ui.approval_rx = Some(foreground_rx);
    ui.auxiliary_agent_tasks[0].approval_rx = Some(background_rx);
    let foreground_handler = TuiApprovalHandler {
        session_id: Some(foreground_session),
        sender: foreground_tx,
    };
    let background_handler = TuiApprovalHandler {
        session_id: Some(background_session),
        sender: background_tx,
    };

    let foreground_waiter = tokio::spawn(foreground_handler.request_permission(
        PermissionApprovalRequest {
            tool_call_id: "requested-first".to_string(),
            tool_name: "write".to_string(),
            summary: "first request".to_string(),
            reason: "test".to_string(),
            matched_rule: None,
            suggested_rule: None,
            allow_always: false,
            filesystem: None,
            mcp_startup: None,
            timeout_secs: 300,
        },
    ));
    tokio::task::yield_now().await;
    let background_waiter = tokio::spawn(background_handler.request_permission(
        PermissionApprovalRequest {
            tool_call_id: "requested-second".to_string(),
            tool_name: "write".to_string(),
            summary: "second request".to_string(),
            reason: "test".to_string(),
            matched_rule: None,
            suggested_rule: None,
            allow_always: false,
            filesystem: None,
            mcp_startup: None,
            timeout_secs: 300,
        },
    ));
    tokio::task::yield_now().await;

    assert!(ui.drain_permission_approval_requests());
    let Some(BottomPanel::PermissionApproval(panel)) = ui.bottom_panel.as_ref() else {
        panic!("first approval did not surface");
    };
    assert_eq!(panel.request.tool_call_id, "requested-first");
    assert_eq!(
        ui.pending_permission_approvals
            .front()
            .expect("second approval")
            .request
            .tool_call_id,
        "requested-second"
    );

    app.resolve_permission_approval_from_command(
        &mut ui,
        PermissionApprovalDecision::deny(),
        "/deny",
    )
    .expect("resolve first");
    background_handler
        .cancel_permission_with_reason("requested-second", "test cleanup")
        .await;
    ui.drain_permission_approval_requests();
    assert_eq!(
        foreground_waiter.await.expect("foreground waiter"),
        PermissionApprovalDecision::deny()
    );
    assert_eq!(
        background_waiter.await.expect("background waiter"),
        PermissionApprovalDecision::deny()
    );
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn session_switch_preserves_visible_approval_and_next_decision_progresses() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first.clone());
    let mut ui = FullscreenUi::new(&app);
    let (_events_tx, events_rx) = mpsc::unbounded_channel();
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control: test_shell_running_control(&app),
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(events_rx),
        task: RunningTask::Agent(tokio::spawn(async {
            std::future::pending::<psychevo::Result<TurnResult>>().await
        })),
    });
    let (approval_tx, approval_rx) = mpsc::unbounded_channel();
    ui.approval_rx = Some(approval_rx);
    let handler = TuiApprovalHandler {
        session_id: Some(first.clone()),
        sender: approval_tx,
    };
    let first_waiter = tokio::spawn(handler.request_permission(PermissionApprovalRequest {
        tool_call_id: "visible-before-switch".to_string(),
        tool_name: "write".to_string(),
        summary: "visible before switch".to_string(),
        reason: "test".to_string(),
        matched_rule: None,
        suggested_rule: None,
        allow_always: false,
        filesystem: None,
        mcp_startup: None,
        timeout_secs: 300,
    }));
    tokio::task::yield_now().await;
    assert!(ui.drain_permission_approval_requests());

    app.open_session_direct(&mut ui, &second)
        .await
        .expect("switch");
    assert!(matches!(
        ui.bottom_panel,
        Some(BottomPanel::PermissionApproval(_))
    ));
    app.resolve_permission_approval_from_command(
        &mut ui,
        PermissionApprovalDecision::allow_once(),
        "/approve once",
    )
    .expect("resolve first");
    assert_eq!(
        first_waiter.await.expect("first waiter"),
        PermissionApprovalDecision::allow_once()
    );

    let second_waiter = tokio::spawn(handler.request_permission(PermissionApprovalRequest {
        tool_call_id: "after-switch".to_string(),
        tool_name: "write".to_string(),
        summary: "after switch".to_string(),
        reason: "test".to_string(),
        matched_rule: None,
        suggested_rule: None,
        allow_always: false,
        filesystem: None,
        mcp_startup: None,
        timeout_secs: 300,
    }));
    tokio::task::yield_now().await;
    assert!(ui.drain_permission_approval_requests());
    handler
        .cancel_permission_with_reason("after-switch", "aborted")
        .await;
    ui.drain_permission_approval_requests();
    assert_eq!(
        second_waiter.await.expect("second waiter"),
        PermissionApprovalDecision::deny()
    );
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn fullscreen_new_with_unresolved_running_session_hides_unowned_late_output() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    app.force_new_once = true;

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async { std::future::pending::<psychevo::Result<TurnResult>>().await });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");
    assert_eq!(app.current_session, None);
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);

    tx.send(TurnEvent::ReasoningDelta {
        text: "unresolved old session thinking".to_string(),
    })
    .expect("send unresolved output");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain unresolved output");

    assert_eq!(app.current_session, None);
    assert_eq!(
        ui.auxiliary_agent_tasks[0]
            .pending_unowned_live_events
            .len(),
        1
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("unresolved old session thinking"))
    );

    tx.send(runtime_turn_event(serde_json::json!({
        "type": "run_start",
        "session_id": "old-session",
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send old session start");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain old session start");

    assert_eq!(app.current_session, None);
    assert!(
        ui.auxiliary_agent_tasks[0]
            .pending_unowned_live_events
            .is_empty()
    );
    let backlog = ui
        .session_live_event_backlog
        .get("old-session")
        .expect("old session backlog");
    assert!(backlog
        .iter()
        .any(|event| matches!(event, TuiLiveEvent::Turn(TurnEvent::ReasoningDelta { text }) if text == "unresolved old session thinking")));
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("unresolved old session thinking"))
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn background_session_completion_does_not_steal_current_session() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = finished_turn_result(first.clone());
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
    });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.open_session_direct(&mut ui, &second)
        .await
        .expect("switch to second");
    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));

    let _ = done_tx.send(());
    while !ui
        .auxiliary_agent_tasks
        .iter()
        .all(|agent| agent.task.is_finished())
    {
        tokio::task::yield_now().await;
    }
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain completion");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.auxiliary_agent_tasks.is_empty());
}

#[tokio::test]
pub(crate) async fn sessions_panel_lists_global_sessions_and_opening_switches_cwd() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let config_path = app.home.join("config.toml");
    fs::write(&config_path, "\n").expect("config");
    app.config_path = Some(config_path);
    let other_cwd = temp.path().join("other-work");
    fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = psychevo::paths::canonicalize_cwd(&other_cwd).expect("other canonical");
    let session_id =
        start_thread_fixture(&app, &other_cwd, "web", "mock-model", "mock", None).await;
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "global prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "global prompt"}],
            "timestamp_ms": 1
        }),
        None,
    );

    let panel = app
        .session_selection_panel(SessionListView::Active)
        .await
        .expect("session panel");
    let row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &session_id))
        .expect("global session row");
    assert_eq!(row.group.as_deref(), Some("other-work"));
    let expected_description = format!("{}  mock/mock-model  messages=1", other_cwd.display());
    assert_eq!(
        row.description.as_deref(),
        Some(expected_description.as_str())
    );
    assert!(row.search_text.contains("other-work"));

    let mut ui = FullscreenUi::new(&app);
    app.open_session_direct(&mut ui, &session_id)
        .await
        .expect("open global session");

    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    assert_eq!(app.cwd, other_cwd);
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Prompt && row.text == "global prompt" })
    );
}

#[tokio::test]
pub(crate) async fn tui_sessions_exclude_internal_side_and_child_sessions() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let parent = start_thread_fixture(&app, &app.cwd, "tui", "mock-model", "mock", None).await;
    let side = start_thread_fixture(
        &app,
        &app.cwd,
        TUI_SIDE_CONVERSATION_SESSION_SOURCE,
        "mock-model",
        "mock",
        None,
    )
    .await;
    let child = app
        .runtime
        .client()
        .resume_thread(&parent)
        .await
        .expect("parent Thread")
        .start_side_conversation(StartSideConversationRequest {
            surface: SideConversationSurface::Tui,
            model: ThreadModelSelection {
                provider: "mock".to_string(),
                model: "mock-model".to_string(),
                reasoning_effort: None,
            },
            mode: RunMode::Default,
            permission_mode: PermissionMode::Default,
            selected_agent: None,
            agent_binding: None,
        })
        .await
        .expect("child Thread")
        .id()
        .to_string();
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    for (index, session_id) in [&parent, &side].into_iter().enumerate() {
        insert_tui_message(
            &conn,
            session_id,
            1,
            "user",
            index as i64 + 1,
            serde_json::json!({
                "role": "user",
                "content": [{"text": "visible"}],
                "timestamp_ms": index as i64 + 1,
            }),
        );
    }

    let sessions = app
        .tui_sessions(SessionListView::Active)
        .await
        .expect("sessions");
    let ids = sessions
        .iter()
        .map(|session| session.summary.id.as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&parent.as_str()));
    assert!(!ids.contains(&side.as_str()));
    assert!(!ids.contains(&child.as_str()));
}

#[tokio::test]
pub(crate) async fn new_session_does_not_receive_previous_running_output() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async { std::future::pending::<psychevo::Result<TurnResult>>().await });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: Some(first),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");
    assert_eq!(app.current_session, None);
    assert!(ui.running.is_none());

    tx.send(TurnEvent::ReasoningDelta {
        text: "stale running output".to_string(),
    })
    .expect("send stale output");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain stale output");

    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("stale running output"))
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn running_shell_switch_buffers_stream_until_return() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let first = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let second = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let shell = app
        .runtime
        .client()
        .shell_command(app.shell_command_request("printf shell-one".to_string()))
        .expect("typed shell command");
    let control = shell.control();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo::Result<ShellCommandResult>>().await
    });
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control: RunningTurnControl::Shell(control),
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Shell(rx),
        task: RunningTask::UserShell(task),
    });
    ui.start_assistant();

    app.open_session_direct(&mut ui, &second)
        .await
        .expect("switch to second");
    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_shell_tasks.len(), 1);
    assert_eq!(
        ui.status_running_elapsed(app.current_session.as_deref()),
        None
    );

    tx.send(PresentedShellEvent {
        presentation_id: 42,
        event: ShellCommandEvent::Started {
            thread_id: Some(first.clone()),
            command: "printf shell-one".to_string(),
            started_at_ms: wall_now_ms(),
        },
    })
    .expect("send shell start");
    tx.send(PresentedShellEvent {
        presentation_id: 42,
        event: ShellCommandEvent::Completed {
            thread_id: Some(first.clone()),
            output: serde_json::json!({
                "output": "shell-one",
                "exit_code": 0,
                "truncated": false
            }),
            outcome: ShellCommandOutcome::Completed,
            elapsed_ms: 12,
        },
    })
    .expect("send shell end");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain hidden shell");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("shell-one"))
    );

    app.open_session_direct(&mut ui, &first)
        .await
        .expect("switch back to first");

    assert_eq!(app.current_session.as_deref(), Some(first.as_str()));
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Ran
            && row.title == "! printf shell-one"
            && row.text == "shell-one"
    }));

    for shell in &ui.auxiliary_shell_tasks {
        shell.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn sessions_panel_selection_does_not_reorder_by_view_time() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let older = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let newer = start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        "UPDATE sessions SET started_at_ms = 1000, updated_at_ms = 1000 WHERE id = ?1",
        rusqlite::params![&older],
    )
    .expect("older times");
    conn.execute(
        "UPDATE sessions SET started_at_ms = 2000, updated_at_ms = 2000 WHERE id = ?1",
        rusqlite::params![&newer],
    )
    .expect("newer times");
    app.current_session = Some(newer.clone());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(session_panel_ids(panel), vec![newer.clone()]);
    assert!(panel.rows.iter().any(
        |row| matches!(&row.value, BottomSelectionValue::LoadOlderSessions(cwd) if cwd == app.cwd.to_string_lossy().as_ref())
    ));

    for ch in "load older".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .await
        .expect("load older query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("load older");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(session_panel_ids(panel), vec![newer.clone(), older.clone()]);

    for ch in "model-a".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .await
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("select");
    assert_eq!(app.current_session.as_deref(), Some(older.as_str()));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions again");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(session_panel_ids(panel), vec![newer, older.clone()]);
    let current_row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &older))
        .expect("older row");
    assert!(current_row.is_current);
    assert!(matches!(
        panel.selected_value(),
        Some(BottomSelectionValue::Session(id)) if id == older
    ));
}

pub(crate) fn session_panel_ids(panel: &BottomSelectionPanel) -> Vec<String> {
    panel
        .rows
        .iter()
        .filter_map(|row| match &row.value {
            BottomSelectionValue::Session(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
pub(crate) async fn sessions_panel_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    start_thread_fixture(&app, &app.cwd, "tui", "model-b", "mock", None).await;
    app.current_session = None;
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.selected, 0);

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("wrap up");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(
        panel.selected,
        panel.filtered_indices().len().saturating_sub(1)
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("wrap down");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.selected, 0);
}

#[tokio::test]
pub(crate) async fn sessions_panel_action_mode_archives_current_and_restores_from_archived_view() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "restore me"}],
            "timestamp_ms": 1
        }),
    );
    app.current_session = Some(session_id.clone());
    app.current_session_title = Some("Restore Me".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("old visible prompt".to_string());
    ui.replace_session_history_prompts(vec!["old visible prompt".to_string()]);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await
    .expect("archive");

    assert_eq!(app.current_session, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.history.is_empty());
    assert_eq!(
        app.tui_sessions(SessionListView::Active)
            .await
            .expect("active")
            .len(),
        0
    );
    assert_eq!(
        app.tui_sessions(SessionListView::Archived)
            .await
            .expect("archived")
            .len(),
        1
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("archived view");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.session_view, Some(SessionListView::Archived));
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("restore select");

    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "restore me")
    );
    assert_eq!(ui.history.as_slice(), ["restore me"]);
}

#[tokio::test]
pub(crate) async fn sessions_panel_delete_requires_repeat_action_and_can_cancel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_session = None;
    let session_id = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "delete me"}],
            "timestamp_ms": 1
        }),
    );
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .expect("first delete");
    assert!(
        app.runtime
            .client()
            .thread_summary(&session_id)
            .await
            .expect("summary")
            .is_some()
    );
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.delete_confirm.as_deref(), Some(session_id.as_str()));

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .expect("cancel");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.delete_confirm, None);

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .expect("first delete again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm confirm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .expect("confirm delete");

    assert!(
        app.runtime
            .client()
            .thread_summary(&session_id)
            .await
            .expect("summary")
            .is_none()
    );
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.notice.as_deref(), Some("session deleted"));
}

#[tokio::test]
pub(crate) async fn sessions_panel_action_mode_does_not_pollute_search_and_rejects_running_current()
{
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_thread_fixture(&app, &app.cwd, "tui", "model-a", "mock", None).await;
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async { std::future::pending::<psychevo::Result<TurnResult>>().await });
    let control = test_shell_running_control(&app);
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::TurnTest(rx),
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    )
    .await
    .expect("unknown action");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.query, "");
    assert_eq!(
        panel.notice.as_deref(),
        Some("action: F fork  A archive  D delete")
    );

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .await
    .expect("arm archive");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await
    .expect("archive");
    assert!(
        app.runtime
            .client()
            .thread_summary(&session_id)
            .await
            .expect("summary")
            .is_some()
    );
    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(
        panel.notice.as_deref(),
        Some("cannot archive the current session while a turn is running")
    );

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn session_display_messages_count_visible_prompts_and_answers() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let session_id = start_thread_fixture(&app, &app.cwd, "tui", "mock-model", "mock", None).await;
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "visible prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "visible prompt"}],
            "timestamp_ms": 1
        }),
        None,
    );
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "visible answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "visible answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );
    insert_tui_message(
        &conn,
        &session_id,
        3,
        "assistant",
        3,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "reasoning",
                "text": "folded only",
                "provider_evidence": null
            }],
            "timestamp_ms": 3,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        4,
        "assistant",
        4,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_read",
                "name": "read",
                "arguments": {"path": "Cargo.toml"},
                "arguments_json": "{\"path\":\"Cargo.toml\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 4,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        5,
        "tool_result",
        5,
        serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_read",
            "tool_name": "read",
            "content": "{\"path\":\"Cargo.toml\",\"content\":\"ok\"}",
            "is_error": false,
            "timestamp_ms": 5
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .await
        .expect("history");

    assert_eq!(visible_transcript_message_count(&ui.transcript), 2);
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| matches!(row.kind, TranscriptKind::Explored))
            .count(),
        1
    );
    assert_eq!(
        app.session_list_lines().await.expect("session list"),
        [format!(
            "{} {} mock/mock-model messages=2",
            short_session(&session_id),
            session_project_label(&app.cwd.to_string_lossy())
        )]
    );
    let panel = app
        .session_selection_panel(SessionListView::Active)
        .await
        .expect("session panel");
    let row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &session_id))
        .expect("session row");
    let expected_description = format!("{}  mock/mock-model  messages=2", app.cwd.display());
    assert_eq!(
        row.description.as_deref(),
        Some(expected_description.as_str())
    );
}
