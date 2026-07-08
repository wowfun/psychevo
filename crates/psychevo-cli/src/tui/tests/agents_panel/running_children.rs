#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn loading_parent_history_links_orphan_agent_row_without_marking_running() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(
        &app,
        "general",
        "General-purpose subagent for focused coding tasks.",
    );
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "agent_type": "general",
                    "task_name": "fetch_failed_articles",
                    "message": "Fetch the failed articles and comments."
                }
            })),
        )
        .expect("agent edge");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &parent,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "Fetch failed articles"}],
            "timestamp_ms": 1
        }),
    );
    insert_tui_message(
        &conn,
        &parent,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "agent-1",
                "name": "spawn_agent",
                "arguments": {
                    "agent_type": "general",
                    "task_name": "fetch_failed_articles",
                    "message": "Fetch the failed articles and comments."
                },
                "arguments_json": "{\"agent_type\":\"general\",\"task_name\":\"fetch_failed_articles\",\"message\":\"Fetch the failed articles and comments.\"}",
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    app.current_session = Some(parent);
    let mut ui = FullscreenUi::new(&app);

    app.load_current_session_history(&mut ui)
        .expect("load parent history");

    let agent_rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_name.as_deref() == Some("spawn_agent"))
        .collect::<Vec<_>>();
    assert_eq!(agent_rows.len(), 1);
    let row = agent_rows[0];
    assert_eq!(row.title, "general(fetch_failed_articles)");
    assert_eq!(row.agent_target.as_deref(), Some(child.as_str()));
    assert!(row.interrupted);
    assert_eq!(row.text, "interrupted");
    assert!(row.tool_started.is_none());
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("Open"), "{text}");
}

#[test]
pub(crate) fn agents_status_text_includes_team_mission_member_and_cap_labels() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(
        &app,
        "general",
        "General-purpose subagent for focused coding tasks.",
    );
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "teamRunId": "team-run-1",
                "missionRunId": "mission-run-1",
                "teamName": "release",
                "teamMemberId": "reviewer",
                "agent": {
                    "id": "agent-run-1",
                    "task_name": "review_patch",
                    "name": "general",
                    "task": "Review patch"
                }
            })),
        )
        .expect("agent edge");
    app.current_session = Some(parent);

    let text = app.agents_status_text();

    assert!(
        text.contains("Running/Completed (spawning active, cap 4)"),
        "{text}"
    );
    assert!(text.contains("team:release"), "{text}");
    assert!(text.contains("mission:mission-run-1"), "{text}");
    assert!(text.contains("member:reviewer"), "{text}");
}

#[test]
pub(crate) fn mission_metadata_creates_pending_tui_session_for_first_turn() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(
        &app,
        "general",
        "General-purpose subagent for focused coding tasks.",
    );
    let teams_dir = app.cwd.join(".psychevo/teams");
    std::fs::create_dir_all(&teams_dir).expect("teams");
    std::fs::write(
        teams_dir.join("release.md"),
        concat!(
            "---\n",
            "name: release\n",
            "description: Release team\n",
            "leader: general\n",
            "members:\n",
            "  - id: reviewer\n",
            "    agent: general\n",
            "    role: review\n",
            "maxParallelAgents: 2\n",
            "---\n",
            "Coordinate the release.\n"
        ),
    )
    .expect("team");

    app.record_mission_metadata(Some("release"), "Ship it")
        .expect("metadata");

    let parent = app.current_session.clone().expect("pending session");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let team = store
        .find_active_agent_team_run(&parent)
        .expect("team lookup")
        .expect("team run");
    let mission = store
        .find_active_agent_mission_run(&parent)
        .expect("mission lookup")
        .expect("mission run");
    assert_eq!(team.team_name, "release");
    assert_eq!(team.max_parallel_agents, 2);
    assert_eq!(mission.goal, "Ship it");
}

#[tokio::test]
pub(crate) async fn running_agent_row_enter_opens_child_session_before_parent_turn_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "task_name": "translate-1",
                    "name": "translate",
                    "task": "translate text"
                }
            })),
        )
        .expect("agent edge");
    let child_prompt_ms = wall_now_ms() - 12_500;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &child,
        1,
        "user",
        child_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "translate text"}],
            "timestamp_ms": child_prompt_ms
        }),
    );
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(result)
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
    ui.visible_turn_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(140))
            .expect("instant"),
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("s · Esc"), "{text}");
    assert!(!text.contains("2m20s · Esc"), "{text}");
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "finish the current turn before opening an agent session")
    );
}

#[tokio::test]
pub(crate) async fn esc_interrupts_running_child_session_after_open() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(result)
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

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");
    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("interrupt child session");

    assert!(ui.interrupt_requested);
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn esc_interrupts_running_child_from_parent_session_after_return() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    let parent_prompt_ms = wall_now_ms() - 22_500;
    insert_tui_message(
        &conn,
        &parent,
        1,
        "user",
        parent_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "delegate this"}],
            "timestamp_ms": parent_prompt_ms
        }),
    );
    insert_tui_message(
        &conn,
        &parent,
        2,
        "assistant",
        parent_prompt_ms + 1_000,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_agent",
                "name": "spawn_agent",
                "arguments": {
                    "agent": "translate",
                    "task": "translate text"
                },
                "arguments_json": "{\"agent\":\"translate\",\"task\":\"translate text\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": parent_prompt_ms + 1_000,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let child_prompt_ms = wall_now_ms() - 6_500;
    insert_tui_message(
        &conn,
        &child,
        1,
        "user",
        child_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "translate text"}],
            "timestamp_ms": child_prompt_ms
        }),
    );
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(result)
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

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT),
    )
    .await
    .expect("return to parent");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    let before_draw_ms = wall_now_ms();
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let after_draw_ms = wall_now_ms();
    let text = buffer_text(&buffer);
    let elapsed = rendered_running_elapsed_seconds(&text).expect("running elapsed in status line");
    let parent_range =
        rendered_elapsed_seconds_range(parent_prompt_ms, before_draw_ms, after_draw_ms);
    let child_range =
        rendered_elapsed_seconds_range(child_prompt_ms, before_draw_ms, after_draw_ms);
    assert!(
        (parent_range.0..=parent_range.1).contains(&elapsed),
        "expected parent elapsed in range {:?}, got {elapsed}s\n{text}",
        parent_range
    );
    assert!(
        !(child_range.0..=child_range.1).contains(&elapsed),
        "expected parent elapsed instead of child elapsed range {:?}, got {elapsed}s\n{text}",
        child_range
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("interrupt child from parent session");

    assert!(ui.interrupt_requested);
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

pub(crate) fn rendered_elapsed_seconds_range(
    start_ms: i64,
    before_ms: i64,
    after_ms: i64,
) -> (u64, u64) {
    (
        before_ms.saturating_sub(start_ms).max(0) as u64 / 1_000,
        after_ms.saturating_sub(start_ms).max(0) as u64 / 1_000,
    )
}

pub(crate) fn rendered_running_elapsed_seconds(text: &str) -> Option<u64> {
    let mut elapsed = None;
    for line in text.lines() {
        let mut start = 0;
        while let Some(offset) = line[start..].find(" · Esc") {
            let marker = start + offset;
            if let Some(token) = line[..marker].split_whitespace().last()
                && let Some(seconds) = compact_duration_seconds(token)
            {
                elapsed = Some(seconds);
            }
            start = marker + " · Esc".len();
        }
    }
    elapsed
}

pub(crate) fn compact_duration_seconds(token: &str) -> Option<u64> {
    let token = token.strip_suffix('s')?;
    if let Some((minutes, seconds)) = token.split_once('m') {
        Some(minutes.parse::<u64>().ok()? * 60 + seconds.parse::<u64>().ok()?)
    } else {
        token.parse::<u64>().ok()
    }
}

#[tokio::test]
pub(crate) async fn agent_row_click_toggles_and_open_action_enters_child_session() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.full_text = Some("Running (0 tool uses)\nPrompt:\ntranslate text".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 100, 14);
    let open_area = transcript_test_target_area(&ui, TranscriptHitTarget::AgentOpen(row_id));
    let row_area = transcript_test_target_area(&ui, TranscriptHitTarget::Row(row_id));
    assert_eq!(
        ui.transcript_hit(open_area.x + 1, open_area.y),
        Some(TranscriptHitTarget::AgentOpen(row_id))
    );
    assert_eq!(
        ui.transcript_hit(row_area.x + 1, row_area.y),
        Some(TranscriptHitTarget::Row(row_id))
    );
    let after_open = open_area
        .x
        .saturating_add(open_area.width)
        .saturating_add(1);
    if after_open < row_area.x.saturating_add(row_area.width) {
        assert_eq!(
            ui.transcript_hit(after_open, open_area.y),
            Some(TranscriptHitTarget::Row(row_id))
        );
    }

    click_transcript_test_target(&mut app, &mut ui, TranscriptHitTarget::Row(row_id)).await;
    assert!(ui.transcript[0].expanded);
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));

    draw_fullscreen_for_test(&app, &mut ui, 100, 14);
    click_transcript_test_target(&mut app, &mut ui, TranscriptHitTarget::AgentOpen(row_id)).await;
    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('P'), KeyModifiers::ALT),
    )
    .await
    .expect("return to parent");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
}

#[tokio::test]
pub(crate) async fn transcript_open_shortcut_opens_visible_agent_row_after_focus() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.full_text = Some("Prompt:\ntranslate text".to_string());
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 100, 14);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    )
    .await
    .expect("focus transcript");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
    )
    .await
    .expect("open selected agent");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
}

#[tokio::test]
pub(crate) async fn running_child_session_receives_scoped_stream_after_open() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
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

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open child");
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::ReasoningDelta {
            text: "child is working".to_string(),
        },
    ))
    .expect("send scoped child event");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain child stream");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(
        ui.transcript.iter().any(
            |row| row.kind == TranscriptKind::Thinking && row.text.contains("child is working")
        )
    );
}

#[tokio::test]
pub(crate) async fn opening_running_agent_child_replays_scoped_live_backlog() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
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
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::ReasoningDelta {
            text: "child started before inspection".to_string(),
        },
    ))
    .expect("send scoped child event");

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain parent preview");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(
        ui.transcript[0]
            .text
            .contains("child started before inspection")
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open child");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking && row.text.contains("child started before inspection")
    }));
}

#[tokio::test]
pub(crate) async fn scoped_child_stream_updates_parent_agent_tail_without_child_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let parent = "parent-session".to_string();
    let child = "child-session".to_string();
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
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
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::value(serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "read",
            "tool_call_id": "read-1",
            "args": {"path": "src/lib.rs"},
            "result": {"path": "src/lib.rs", "content": "ok"},
            "outcome": "normal"
        })),
    ))
    .expect("send child tool event");
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::value(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "translated"}]
            },
            "usage": {"total_tokens": 15}
        })),
    ))
    .expect("send child message event");

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain parent preview");

    assert_eq!(ui.transcript.len(), 1);
    assert!(
        ui.transcript[0]
            .text
            .contains("Running (1 tool use · 15 tokens)")
    );
    assert!(ui.transcript[0].text.contains("read src/lib.rs"));
    assert!(ui.transcript[0].text.contains("Response: translated"));
}

#[test]
pub(crate) fn parent_agent_preview_coalesces_streamed_reasoning_chunks() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let child = "child-session".to_string();
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("spawn_agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);

    for text in [
        "Hmm, this",
        " looks",
        " like",
        " a",
        " streamed",
        " reasoning",
        " preview",
        " chunk",
    ] {
        assert!(ui.apply_agent_child_preview_event(
            &child,
            &RunStreamEvent::ReasoningDelta {
                text: text.to_string(),
            },
        ));
    }

    let text = &ui.transcript[0].text;
    assert!(
        text.contains("Thinking: Hmm, this looks like a streamed reasoning preview chunk"),
        "{text}"
    );
    assert_eq!(text.matches("Thinking:").count(), 1, "{text}");
    assert!(!text.contains("more lines"), "{text}");
}
