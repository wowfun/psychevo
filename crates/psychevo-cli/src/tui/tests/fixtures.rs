#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn draw_fullscreen_for_test(
    app: &TuiApp,
    ui: &mut FullscreenUi<'_>,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, ui))
        .expect("draw");
    terminal.backend().buffer().clone()
}

pub(crate) fn draw_fullscreen_with_cursor_for_test(
    app: &TuiApp,
    ui: &mut FullscreenUi<'_>,
    width: u16,
    height: u16,
) -> (ratatui::buffer::Buffer, (u16, u16)) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, ui))
        .expect("draw");
    let cursor = {
        let Position { x, y } = terminal
            .backend_mut()
            .get_cursor_position()
            .expect("cursor");
        (x, y)
    };
    (terminal.backend().buffer().clone(), cursor)
}

pub(crate) async fn drain_fullscreen_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..200 {
        app.drain_fullscreen_events(ui).await.expect("drain events");
        if ui.running.is_none()
            && ui.queued_inputs.is_empty()
            && ui.auxiliary_shell_tasks.is_empty()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fullscreen work did not become idle");
}

pub(crate) fn test_app(temp: &tempfile::TempDir) -> TuiApp {
    let home = temp.path().join("home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let workdir = workdir.canonicalize().expect("canonical");
    let mut env_map = BTreeMap::new();
    env_map.insert(
        "HOME".to_string(),
        temp.path()
            .canonicalize()
            .expect("temp canonical")
            .display()
            .to_string(),
    );
    let (clipboard_result_tx, clipboard_result_rx) = std::sync::mpsc::channel();
    let db_path = home.join("state.db");
    let state_runtime = StateRuntime::open(&db_path).expect("state runtime");
    let gateway = Gateway::new(state_runtime.clone());
    TuiApp {
        env_map,
        home: home.clone(),
        state_path: home.join("tui-state.json"),
        state: TuiState::default(),
        state_runtime,
        gateway,
        db_path,
        config_path: None,
        workdir: workdir.clone(),
        workdir_key: workdir.display().to_string(),
        current_session: Some("1234567890abcdef".to_string()),
        current_session_title: Some("Review sidebar polish".to_string()),
        force_new_once: false,
        draft_source_raw_id: None,
        current_model: Some("mock/model".to_string()),
        current_variant: Some("high".to_string()),
        selected_model: None,
        current_mode: RunMode::Default,
        current_permission_mode: PermissionMode::Default,
        startup_agent: None,
        current_agent: None,
        current_agent_explicit_default: false,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
        thinking_visible: true,
        raw_visible: false,
        clipboard: Arc::new(|_| Ok(())),
        renderer: TuiRenderer::new(false),
        debug: false,
        had_error: false,
        last_context_snapshot: None,
        model_catalog: ModelCatalogCache::default(),
        clipboard_result_tx,
        clipboard_result_rx,
        clipboard_copies_in_flight: 0,
        slash_config: EffectiveSlashConfig::default(),
        side_conversation: None,
        last_live_agent_reload_check: None,
        last_gateway_live_event_seq: 0,
        session_browser_limits: BTreeMap::new(),
        side_cleanup_task: None,
        compaction_task: None,
        diff_task: None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FixtureKind {
    Idle,
    RunningThinking,
    CollapsedTool,
    ExpandedTool,
    ExpandedLongCommand,
    CollapsedJsonTool,
    LongCommandFoldedOutput,
    ConsecutiveToolRows,
    RichMarkdown,
    ActiveWriteAfterAnswer,
    ActiveVisibleWritePreamble,
    ActiveReasoningWrite,
    LongMarkdownBottom,
    LongThinkingMarkdownBottom,
    LongThinkingMarkdownExpandedBottom,
    DebugMeta,
    FailureMeta,
}

pub(crate) fn fixture_ui<'a>(app: &TuiApp, kind: FixtureKind) -> FullscreenUi<'a> {
    let mut ui = FullscreenUi::new(app);
    ui.sidebar = stable_sidebar();
    match kind {
        FixtureKind::Idle => {}
        FixtureKind::RunningThinking => {
            ui.transcript.clear();
            ui.push_user("Inspect the CLI rendering path.".to_string());
            ui.start_assistant();
            ui.apply_value_event(
                &serde_json::json!({
                    "type": "run_start",
                    "provider": "mock",
                    "model": "mock-model",
                    "mode": "default",
                    "context_limit": 64000
                }),
                false,
            );
            ui.turn_started = None;
            ui.apply_stream_event(
                RunStreamEvent::ReasoningDelta {
                    text: "Read the TUI renderer and identify stable evidence blocks.".to_string(),
                },
                true,
                false,
            );
            ui.running_elapsed_override = Some(Duration::from_secs(12));
            let mut row = TranscriptRow::with_title(
                TranscriptKind::Explored,
                "Explored crates/psychevo-cli/src/tui.rs",
                "running",
            );
            row.tool_started = Some(
                Instant::now()
                    .checked_sub(Duration::from_millis(12_500))
                    .expect("instant"),
            );
            ui.transcript.push(row);
        }
        FixtureKind::CollapsedTool | FixtureKind::ExpandedTool => {
            ui.transcript.clear();
            push_completed_turn(&mut ui, kind);
        }
        FixtureKind::ExpandedLongCommand => {
            ui.transcript.clear();
            push_expanded_long_command_turn(&mut ui);
        }
        FixtureKind::CollapsedJsonTool => {
            ui.transcript.clear();
            push_collapsed_json_tool_turn(&mut ui);
        }
        FixtureKind::LongCommandFoldedOutput => {
            ui.transcript.clear();
            push_long_command_folded_output_turn(&mut ui);
        }
        FixtureKind::ConsecutiveToolRows => {
            ui.transcript.clear();
            push_consecutive_tool_rows_turn(&mut ui);
        }
        FixtureKind::RichMarkdown => {
            ui.transcript.clear();
            push_rich_markdown_turn(&mut ui, &app.workdir);
        }
        FixtureKind::ActiveWriteAfterAnswer => {
            ui.transcript.clear();
            push_active_write_after_answer_turn(&mut ui);
        }
        FixtureKind::ActiveVisibleWritePreamble => {
            ui.transcript.clear();
            push_active_visible_write_preamble_turn(&mut ui);
        }
        FixtureKind::ActiveReasoningWrite => {
            ui.transcript.clear();
            push_active_reasoning_write_turn(&mut ui);
        }
        FixtureKind::LongMarkdownBottom => {
            ui.transcript.clear();
            push_long_markdown_bottom_turn(&mut ui);
        }
        FixtureKind::LongThinkingMarkdownBottom => {
            ui.transcript.clear();
            push_long_thinking_markdown_bottom_turn(&mut ui);
        }
        FixtureKind::LongThinkingMarkdownExpandedBottom => {
            ui.transcript.clear();
            push_long_thinking_markdown_bottom_turn(&mut ui);
            if let Some(row) = ui
                .transcript
                .iter_mut()
                .find(|row| row.kind == TranscriptKind::Thinking)
            {
                row.expanded = true;
            }
            ui.scroll_to_bottom();
        }
        FixtureKind::DebugMeta => {
            ui.transcript.clear();
            push_completed_turn(&mut ui, kind);
            ui.sidebar_hidden = true;
        }
        FixtureKind::FailureMeta => {
            ui.transcript.clear();
            push_failure_turn(&mut ui);
        }
    }
    ui.sidebar = stable_sidebar();
    ui
}

pub(crate) fn stable_sidebar() -> SidebarSnapshot {
    SidebarSnapshot {
        title: "Review sidebar polish".to_string(),
        session: "12345678".to_string(),
        branch: "main".to_string(),
        changed_files: vec![
            "M crates/psychevo-cli/src/tui.rs".to_string(),
            "?? specs/210-pevo-tui/testing.md".to_string(),
        ],
    }
}

pub(crate) fn stable_session_bottom_panel() -> BottomSelectionPanel {
    BottomSelectionPanel::new_sessions(
        SessionListView::Active,
        vec![
            BottomSelectionRow {
                label: "Implement model picker".to_string(),
                description: Some("/repo  mock/mock-model  messages=5".to_string()),
                detail: Some("2026-05-06 12:10".to_string()),
                group: Some("repo".to_string()),
                search_text: "session-a Implement model picker repo /repo mock mock-model"
                    .to_string(),
                is_current: true,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-a".to_string()),
            },
            BottomSelectionRow {
                label: "Review session pane".to_string(),
                description: Some("/repo  mock/other-model  messages=3".to_string()),
                detail: Some("2026-05-05 09:44".to_string()),
                group: Some("repo".to_string()),
                search_text: "session-b Review session pane repo /repo mock other-model"
                    .to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-b".to_string()),
            },
        ],
    )
}

pub(crate) fn stable_archived_session_bottom_panel() -> BottomSelectionPanel {
    BottomSelectionPanel::new_sessions(
        SessionListView::Archived,
        vec![BottomSelectionRow {
            label: "Archived refactor branch".to_string(),
            description: Some("/repo  mock/mock-model  messages=7".to_string()),
            detail: Some("2026-05-01 18:22".to_string()),
            group: Some("repo".to_string()),
            search_text: "session-archived Archived refactor branch repo /repo mock mock-model"
                .to_string(),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Session("session-archived".to_string()),
        }],
    )
}

pub(crate) fn push_completed_turn(ui: &mut FullscreenUi<'_>, kind: FixtureKind) {
    ui.push_user("Summarize the TUI snapshot harness.".to_string());
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "Check layout boundaries, style roles, and expandable evidence.",
    ));
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Explored,
        "Explored crates/psychevo-cli/src/tui.rs",
        long_tool_output(),
    );
    if matches!(kind, FixtureKind::ExpandedTool) {
        row.expanded = true;
        ui.focus = FocusMode::Transcript;
        ui.selected_row = Some(2);
        ui.auto_follow_transcript = false;
    }
    ui.transcript.push(row);
    ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "The harness snapshots stable buffer text and style roles, then leaves real terminal screenshots as diagnostics.",
        ));
    let debug = matches!(kind, FixtureKind::DebugMeta);
    let usage = if debug {
        serde_json::json!({
            "input_tokens": 120,
            "total_tokens": 177
        })
    } else {
        serde_json::json!({
        "input_tokens": 120,
        "output_tokens": 45,
        "reasoning_tokens": 12,
        "total_tokens": 177
        })
    };
    let metadata = if debug {
        serde_json::json!({
            "elapsed_ms": 2500,
            "provider_response_id": "resp_snapshot",
            "reasoning_effort": "high"
        })
    } else {
        serde_json::json!({
            "elapsed_ms": 2500,
            "provider_response_id": "resp_snapshot",
            "reasoning_effort": "high",
            "system_fingerprint": "fp_mock"
        })
    };
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        turn_meta_text(TurnMetaProjection {
            mode: "default",
            provider: "mock",
            model: "mock-model",
            started: None,
            usage: Some(&usage),
            metadata: Some(&metadata),
            accounting: None,
            failures: 0,
            interrupted: false,
            debug,
        }),
    ));
}

pub(crate) fn push_consecutive_tool_rows_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Summarize several tool calls.".to_string());
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "I need to inspect files and run checks before answering.",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Explored,
        "Explored crates/psychevo-cli/src/tui/render/transcript.rs",
        "read 240 lines",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test -p psychevo-cli tui::tests",
        "ok",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Updated,
        "Updated crates/psychevo-cli/src/tui/render/transcript.rs",
        "write normal",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo fmt",
        "ok",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "The tool evidence stays flat even when several tool rows arrive consecutively.",
    ));
    ui.scroll_to_bottom();
}

pub(crate) fn push_expanded_long_command_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Inspect HN comments with sqlite.".to_string());
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48073680 ORDER BY id\" 2>&1 | head -c 3000";
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        format!("Ran {command}"),
        "48073976||aurareturn||6 days of work to do this.\n48074445||aniclecha||I think the industry is moving to English.",
    );
    row.expanded = true;
    let row_id = row.id;
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.selected_target = Some(TranscriptHitTarget::Row(row_id));
    ui.selected_row = Some(1);
    ui.scroll_to_bottom();
}

pub(crate) fn push_collapsed_json_tool_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Inspect cached HN comments.".to_string());
    let json = format!(
        "{{\"comments\":[{}]}}",
        (1..=32)
            .map(|index| format!(
                "{{\"id\":4807{index:04},\"by\":\"user{index}\",\"text\":\"{}\"}}",
                "identity verification discussion ".repeat(2)
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran sqlite3 feeds/.cache/hn.db",
        json,
    ));
    ui.scroll_to_bottom();
}

pub(crate) fn push_long_command_folded_output_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Fetch comments from sqlite.".to_string());
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48072190 ORDER BY id\"";
    let output = (1..=12)
        .map(|index| format!("4807{index:04}||user{index}||cached comment text"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Ran, format!("Ran {command}"), output);
    ui.transcript.push(row);
    ui.scroll_to_bottom();
}

pub(crate) fn push_failure_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Run a command that fails.".to_string());
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test -p psychevo-cli",
        "exit_code=101\ncompile error: fixture failure",
    );
    row.failed = true;
    ui.transcript.push(row);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "The run failed before producing a clean validation result.",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "mock/mock-model  1 failure",
    ));
}

pub(crate) fn long_tool_output() -> String {
    (1..=24)
        .map(|line| format!("{line:02}: crates/psychevo-cli/src/tui.rs evidence row"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn push_rich_markdown_turn(ui: &mut FullscreenUi<'_>, workdir: &Path) {
    ui.push_user("Render a markdown answer.".to_string());
    let path = workdir.join("crates/psychevo-cli/src/tui/render/transcript.rs");
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        format!(
            "# Rendering pass\n\n- Keep **ledger** rhythm\n- Style `inline code`\n\n```rust\nfn render() {{}}\n```\n\nSee [transcript]({}:42).",
            path.display()
        ),
    ));
}

pub(crate) fn push_active_write_after_answer_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Write the report after collecting data.".to_string());
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Now I have all the data I need. Let me write the full report:"
                    },
                    {
                        "type": "tool_call",
                        "id": "call_write_report",
                        "name": "write",
                        "arguments": {
                            "path": "/tmp/hackernews-hot-05-15.md",
                            "content": "report body"
                        },
                        "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-15.md\",\"content\":\"report body\"}",
                        "arguments_error": null,
                        "content_index": 1,
                        "call_index": 0
                    }
                ],
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 186_260,
                "reasoning_effort": "low"
            }
        }),
        false,
    );
    ui.scroll_to_bottom();
}

pub(crate) fn push_active_visible_write_preamble_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Write the complete report.".to_string());
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "Now I have all the data needed. Let me write the complete report."
                }],
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call_write_report",
            "tool_name": "write",
            "arguments_json": "",
            "content_index": 1,
            "call_index": 0
        }),
        false,
    );
    ui.scroll_to_bottom();
}

pub(crate) fn push_active_reasoning_write_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Write the report after reasoning.".to_string());
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2.5-pro",
            "mode": "default"
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "Let me compose the full report now. I have all the data. Let me write it out."
                .to_string(),
        },
        true,
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_write_report",
                    "name": "write",
                    "arguments": {
                        "path": "/tmp/hackernews-hot-05-39.md",
                        "content": "report body"
                    },
                    "arguments_json": "{\"path\":\"/tmp/hackernews-hot-05-39.md\",\"content\":\"report body\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }],
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "mimo-v2.5-pro",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 190_546,
                "reasoning_effort": "low"
            }
        }),
        false,
    );
    ui.scroll_to_bottom();
}

pub(crate) fn push_long_markdown_bottom_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Render long markdown table and scroll to bottom.".to_string());
    let rows = (1..=32)
        .map(|index| {
            format!(
                "| {index} | **Topic {index}** - mixed Markdown/CJK width validation row | {} |",
                100 + index
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        format!(
            "# Long Markdown Scroll Fixture\n\n| # | Topic | Score |\n|---|---|---|\n{rows}\n\nLONG_MARKDOWN_BOTTOM_MARKER"
        ),
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "mock/mock-model low 7m05s 1 failure",
    ));
    ui.scroll_to_bottom();
}

pub(crate) fn push_long_thinking_markdown_bottom_turn(ui: &mut FullscreenUi<'_>) {
    ui.push_user("Restore a reasoning-only daily report.".to_string());
    let rows = (1..=12)
        .map(|index| {
            format!(
                "| {index} | **热门话题 {index}** - 混合 Markdown/CJK 宽度校验行 | {} |",
                180 + index
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        format!(
            "Hacker News 日报已生成完成！✅\n\n**文件：** `feeds/2026-05-10/hackernews-hot-04-03.md`（25KB）\n\n### 今日 12 条热门话题速览：\n\n| # | 话题 | 💬 |\n|---|------|-----|\n{rows}"
        ),
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Meta,
        "",
        "xiaomi-token-plan/mimo-v2.5-pro low 7m05s 1 failure",
    ));
    ui.scroll_to_bottom();
}

pub(crate) fn assert_tui_snapshot(
    name: &str,
    width: u16,
    height: u16,
    app: &TuiApp,
    mut ui: FullscreenUi<'_>,
) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let text = buffer_text(buffer);
    let styles = buffer_style_text(buffer);
    let combined = format!(
        "fixture={name}\nsize={width}x{height}\n\n--- text ---\n{text}\n--- styles ---\n{styles}"
    );
    write_snapshot_diagnostics(name, &text, &styles, &combined);
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/snapshots");
    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => snapshot_path }, {
        insta::assert_snapshot!(name, combined);
    });
}

pub(crate) fn write_snapshot_diagnostics(name: &str, text: &str, styles: &str, combined: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pevo-tui-snapshots")
        .join(name);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::write(dir.join("text.txt"), text);
    let _ = fs::write(dir.join("styles.txt"), styles);
    let _ = fs::write(dir.join("combined.txt"), combined);
    let _ = fs::write(
        dir.join("metadata.json"),
        serde_json::json!({
            "fixture": name,
            "source": "ratatui TestBackend",
            "golden": "insta snapshot"
        })
        .to_string(),
    );
}

pub(crate) fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = *buffer.area();
    let mut text = String::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

pub(crate) fn attach_pending_agent_running(ui: &mut FullscreenUi<'_>) {
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
}

pub(crate) fn attach_background_agent_running(ui: &mut FullscreenUi<'_>, session_id: &str) {
    let (_tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async {
        std::future::pending::<psychevo_runtime::Result<psychevo_runtime::RunResult>>().await
    });
    let (control, _) = run_control();
    ui.auxiliary_agent_tasks.push(AuxiliaryAgentTask {
        session_id: Some(session_id.to_string()),
        child_session_id: None,
        visible_live: true,
        pending_unowned_live_events: Vec::new(),
        control,
        events: RunningTurnEvents::Runtime(rx),
        task,
    });
}

pub(crate) fn buffer_style_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = *buffer.area();
    let mut text = String::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        let mut last = None;
        for x in area.x..area.x + area.width {
            let cell = buffer.cell((x, y)).expect("cell");
            if last != Some(cell.fg) {
                last = Some(cell.fg);
                line.push_str(style_marker(cell.fg));
            }
            line.push_str(cell.symbol());
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

pub(crate) fn style_marker(color: Color) -> &'static str {
    if color == TUI_MAGENTA || color == Color::Magenta {
        "[magenta]"
    } else if color == TUI_CYAN || color == Color::Cyan {
        "[cyan]"
    } else if color == Color::Green {
        "[green]"
    } else if color == TUI_RED || color == Color::Red {
        "[red]"
    } else if color == TUI_DIM || color == Color::DarkGray {
        "[dim]"
    } else if color == TUI_PAPER {
        "[paper]"
    } else {
        "[default]"
    }
}
