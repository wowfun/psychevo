#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn running_session_switch_buffers_stream_until_return() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.cwd, "tui", "model-b", "mock", None)
        .expect("second");
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
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();
    ui.running_elapsed_override = Some(Duration::from_secs(9));

    tx.send(RunStreamEvent::ReasoningDelta {
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
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select");

    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert_eq!(
        ui.status_running_elapsed(app.current_session.as_deref()),
        None
    );

    tx.send(RunStreamEvent::ReasoningDelta {
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
pub(crate) async fn fullscreen_new_with_unresolved_running_session_hides_unowned_late_output() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = None;
    app.force_new_once = true;

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");
    assert_eq!(app.current_session, None);
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);

    tx.send(RunStreamEvent::ReasoningDelta {
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

    tx.send(RunStreamEvent::value(serde_json::json!({
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
        .any(|event| matches!(event, TuiLiveEvent::Runtime(RunStreamEvent::ReasoningDelta { text }) if text == "unresolved old session thinking")));
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.cwd, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: first.clone(),
        ..finished_run_result(&app)
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.open_session_direct(&mut ui, &second)
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

#[test]
pub(crate) fn sessions_panel_lists_global_sessions_and_opening_switches_cwd() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = app.home.join("config.toml");
    fs::write(&config_path, "\n").expect("config");
    app.config_path = Some(config_path);
    let other_cwd = temp.path().join("other-work");
    fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_cwd = other_cwd.canonicalize().expect("other canonical");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&other_cwd, "web", "mock-model", "mock", None)
        .expect("session");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "global prompt"}],
            "timestamp_ms": 1
        }),
    );

    let panel = app
        .session_selection_panel(SessionListView::Active)
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
        .expect("open global session");

    assert_eq!(app.current_session.as_deref(), Some(session_id.as_str()));
    assert_eq!(app.cwd, other_cwd);
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Prompt && row.text == "global prompt" })
    );
}

#[test]
pub(crate) fn tui_sessions_exclude_internal_side_and_child_sessions() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent");
    let side = store
        .create_session_with_metadata(
            &app.cwd,
            TUI_SIDE_CONVERSATION_SESSION_SOURCE,
            "mock-model",
            "mock",
            None,
        )
        .expect("side");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "tui", "mock-model", "mock", None)
        .expect("child");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    for (index, session_id) in [&parent, &side, &child].into_iter().enumerate() {
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

    let sessions = app.tui_sessions(SessionListView::Active).expect("sessions");
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("first");
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(first),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");
    assert_eq!(app.current_session, None);
    assert!(ui.running.is_none());

    tx.send(RunStreamEvent::ReasoningDelta {
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.cwd, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first.clone());

    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::UserShellResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(first.clone()),
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::UserShell(task),
    });
    ui.start_assistant();

    app.open_session_direct(&mut ui, &second)
        .expect("switch to second");
    assert_eq!(app.current_session.as_deref(), Some(second.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_shell_tasks.len(), 1);
    assert_eq!(
        ui.status_running_elapsed(app.current_session.as_deref()),
        None
    );

    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_start",
        "session_id": first,
        "tool_call_id": "user_shell",
        "tool_name": "exec_command",
        "args": {"cmd": "printf shell-one"},
        "source": "user_shell",
    })))
    .expect("send shell start");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_end",
        "session_id": first,
        "tool_call_id": "user_shell",
        "tool_name": "exec_command",
        "result": {"output": "shell-one", "exit_code": 0, "truncated": false},
        "outcome": "normal",
        "source": "user_shell",
    })))
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let older = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("older");
    let newer = store
        .create_session_with_metadata(&app.cwd, "tui", "model-b", "mock", None)
        .expect("newer");
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
        .expect("load older query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
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
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("first");
    store
        .create_session_with_metadata(&app.cwd, "tui", "model-b", "mock", None)
        .expect("second");
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
        .expect("wrap up");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(
        panel.selected,
        panel.filtered_indices().len().saturating_sub(1)
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.selected, 0);
}

#[tokio::test]
pub(crate) async fn sessions_panel_action_mode_archives_current_and_restores_from_archived_view() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("session");
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
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .expect("archive");

    assert_eq!(app.current_session, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.history.is_empty());
    assert_eq!(
        app.tui_sessions(SessionListView::Active)
            .expect("active")
            .len(),
        0
    );
    assert_eq!(
        app.tui_sessions(SessionListView::Archived)
            .expect("archived")
            .len(),
        1
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("archived view");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.session_view, Some(SessionListView::Archived));
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
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
    let mut app = test_app(&temp);
    app.current_session = None;
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("session");
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
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("first delete");
    assert!(
        store
            .session_summary(&session_id)
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
    .expect("cancel");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.delete_confirm, None);

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("first delete again");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm confirm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .expect("confirm delete");

    assert!(
        store
            .session_summary(&session_id)
            .expect("summary")
            .is_none()
    );
    assert!(
        store
            .load_messages(&session_id)
            .expect("messages")
            .is_empty()
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
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.cwd, "tui", "model-a", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    )
    .expect("unknown action");
    let Some(BottomPanel::Sessions(panel)) = &ui.bottom_panel else {
        panic!("expected sessions panel");
    };
    assert_eq!(panel.query, "");
    assert_eq!(panel.notice.as_deref(), Some("action: A archive  D delete"));

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .expect("arm archive");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .expect("archive");
    assert!(
        store
            .session_summary(&session_id)
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

#[test]
pub(crate) fn session_display_messages_count_visible_prompts_and_answers() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "visible prompt"}],
            "timestamp_ms": 1
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "visible answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
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
    app.load_current_session_history(&mut ui).expect("history");

    assert_eq!(visible_transcript_message_count(&ui.transcript), 2);
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| matches!(row.kind, TranscriptKind::Explored))
            .count(),
        1
    );
    assert_eq!(
        app.session_list_lines().expect("session list"),
        [format!(
            "{} {} mock/mock-model messages=2",
            short_session(&session_id),
            session_project_label(&app.cwd.to_string_lossy())
        )]
    );
    let panel = app
        .session_selection_panel(SessionListView::Active)
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
