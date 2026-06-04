#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn transcript_selection_toggles_expandable_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.selected_target = Some(TranscriptHitTarget::Row(ui.transcript[0].id));
    ui.toggle_selected();
    assert!(ui.transcript[0].expanded);
    ui.toggle_selected();
    assert!(!ui.transcript[0].expanded);
}

#[test]
pub(crate) fn transcript_render_blocks_keep_consecutive_tools_flat() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    for index in 0..4 {
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Ran,
            format!("Ran command {index}"),
            "ok",
        ));
    }

    let blocks = transcript_render_blocks(&ui);
    assert_eq!(blocks.len(), 4);
    for (index, block) in blocks.iter().enumerate() {
        assert_eq!(block.index, index);
        assert_eq!(
            block.target,
            TranscriptHitTarget::Row(ui.transcript[index].id)
        );
    }
}

#[test]
pub(crate) fn transcript_render_blocks_keep_thinking_and_tools_flat() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "inspect context",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran ls",
        "ok",
    ));
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Updated,
        "Updated report.md",
        "write normal",
    ));
    ui.transcript
        .push(TranscriptRow::simple(TranscriptKind::Answer, "finished"));

    let blocks = transcript_render_blocks(&ui);
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0].index, 0);
    assert_eq!(blocks[1].index, 1);
    assert_eq!(blocks[2].index, 2);
    assert_eq!(blocks[3].index, 3);
}

#[test]
pub(crate) fn bash_timeout_failure_shows_timeout_before_partial_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_fetch",
            "tool_name": "exec_command",
            "args": {"cmd": "python scripts/fetch.py"},
            "result": {
                "output": "[fetch] 29 rows done\n[fetch] 1 failed",
                "exit_code": null,
                "error": "command timed out after 120 seconds",
                "truncated": false
            },
            "outcome": "failed"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("exec_command row");
    assert!(row.failed);
    assert_eq!(row.title, "exec_command python scripts/fetch.py");
    assert!(
        row.text
            .starts_with("timeout: command timed out after 120 seconds; partial output follows\n")
    );
    assert!(row.text.contains("[fetch] 29 rows done"));
}

#[test]
pub(crate) fn bash_timeout_without_output_omits_no_output_placeholder() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_sleep",
            "tool_name": "exec_command",
            "args": {"cmd": "sleep 60"},
            "result": {
                "output": "(no output)",
                "exit_code": null,
                "error": "command timed out after 1 seconds",
                "truncated": false
            },
            "outcome": "failed"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("exec_command row");
    assert_eq!(row.text, "timeout: command timed out after 1 seconds");
    assert!(!row.text.contains("(no output)"));
}

#[test]
pub(crate) fn command_row_renders_claude_style_prefixes_and_colored_context_bar() {
    let row = TranscriptRow::with_title(
        TranscriptKind::Command,
        "/context",
        "Context Usage\n[HHHHH.....]\nB base  D developer  P project  H history  C turn  U prompt  T tools  . free\ntokens: 5/10 (50.0%)",
    );

    let lines = transcript_lines(&row, false, true, 80, Path::new("/repo"), false);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
    assert_eq!(rendered[0], "> /context");
    assert_eq!(rendered[1], "  └  Context Usage");
    assert_eq!(rendered[2], "     [HHHHH.....]");
    assert_eq!(
        rendered[3],
        "     B base  D developer  P project  H history  C turn  U prompt  T tools  . free"
    );
    assert_eq!(rendered[4], "     tokens: 5/10 (50.0%)");

    let bar_h_style = lines[2].spans[6].style;
    let legend_h_style = lines[3].spans[10].style;
    let legend_label_style = lines[3].spans[11].style;
    assert_eq!(bar_h_style, legend_h_style);
    assert_ne!(legend_h_style, legend_label_style);
    assert_eq!(legend_label_style, tui_theme().dim_style());
    assert_eq!(lines[3].spans[11].content.as_ref(), " history");
}

#[test]
pub(crate) fn command_row_defaults_open_and_toggles_details() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Command,
        "/status",
        "workdir: /repo\nmodel: mock/model",
    );

    assert!(row.is_expandable());
    let open = transcript_lines(&row, true, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(open[0], "> /status ▾ collapse");
    assert!(open.iter().any(|line| line.contains("workdir: /repo")));

    toggle_transcript_row_details(&mut row);
    assert!(row.details_collapsed);
    let collapsed = transcript_lines(&row, false, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(collapsed, ["> /status ▸ details"]);

    toggle_transcript_row_details(&mut row);
    assert!(!row.details_collapsed);
    let reopened = transcript_lines(&row, true, true, 80, Path::new("/repo"), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(
        reopened
            .iter()
            .any(|line| line.contains("model: mock/model"))
    );
}

#[test]
pub(crate) fn command_rows_do_not_count_as_visible_messages() {
    let rows = vec![
        TranscriptRow::with_title(TranscriptKind::Command, "/status", "workdir: /repo"),
        TranscriptRow::with_title(TranscriptKind::Prompt, "", "hello"),
        TranscriptRow::with_title(TranscriptKind::Answer, "", "hi"),
    ];

    assert_eq!(visible_transcript_message_count(&rows), 2);
}

#[test]
pub(crate) fn long_thinking_defaults_to_row_level_collapse_without_left_rail() {
    let long = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", long);
    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(row.text.contains("... 6 more lines"));

    let lines = thinking_lines(&row, false, true, 80);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rendered.contains("• Thinking"), "{rendered}");
    assert!(rendered.contains("▸ 6 more lines"), "{rendered}");
    assert!(rendered.contains("  └ line 1"), "{rendered}");
    assert!(rendered.contains("line 2"), "{rendered}");
    assert!(rendered.contains("... 6 more lines"), "{rendered}");
    assert!(rendered.contains("line 9"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
    assert!(!rendered.contains("line 8"), "{rendered}");
    assert!(!rendered.contains("▌"), "{rendered}");
    assert!(!rendered.contains("Thinking:"), "{rendered}");
}

#[test]
pub(crate) fn long_thinking_cycles_preview_full_title_preview() {
    let long = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", long);

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    assert!(!row.details_collapsed);
    let expanded = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(expanded.contains("line 3"), "{expanded}");
    assert!(!expanded.contains("... 6 more lines"), "{expanded}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(row.details_collapsed);
    let collapsed = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("line 1"), "{collapsed}");
    assert!(!collapsed.contains("... 6 more lines"), "{collapsed}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    let preview = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(preview.contains("▸ 6 more lines"), "{preview}");
    assert!(preview.contains("line 1"), "{preview}");
    assert!(preview.contains("line 12"), "{preview}");
    assert!(!preview.contains("line 8"), "{preview}");
}

#[test]
pub(crate) fn active_thinking_row_uses_activity_marker_and_elapsed() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "working");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(2_500))
            .expect("instant"),
    );

    let line = thinking_lines(&row, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let marker = line.spans[0].content.as_ref();
    assert!(marker.ends_with(' '), "{marker}");
    assert_ne!(marker, "◦ ");
    let rendered = line_text(&line);
    assert!(rendered.contains("Thinking"), "{rendered}");
    assert!(rendered.contains("2s"), "{rendered}");
}

#[test]
pub(crate) fn completed_thinking_row_does_not_show_elapsed() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "done");
    row.tool_elapsed = Some(Duration::from_secs(4));

    let line = thinking_lines(&row, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let rendered = line_text(&line);
    assert!(rendered.contains("Thinking"), "{rendered}");
    assert!(!rendered.contains("4s"), "{rendered}");
}

#[test]
pub(crate) fn completed_thinking_row_uses_shared_evidence_title_styles() {
    let thinking = TranscriptRow::with_title(TranscriptKind::Thinking, "Thinking", "");
    let tool = TranscriptRow::with_title(TranscriptKind::Ran, "Ran ls", "");
    let thinking = thinking_lines(&thinking, false, true, 80)
        .into_iter()
        .next()
        .expect("thinking row");
    let tool = tool_lines(&tool, false, true, 80)
        .into_iter()
        .next()
        .expect("tool row");

    assert_eq!(thinking.spans[0].style, tool.spans[0].style);
    assert_eq!(thinking.spans[1].style, tool.spans[1].style);
    assert_eq!(
        thinking.spans[1].style,
        Style::default().add_modifier(Modifier::BOLD)
    );
}

#[test]
pub(crate) fn short_thinking_row_can_collapse_details() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "first thought\nsecond thought",
    );
    assert!(row.is_expandable());

    let open = thinking_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(open.contains("▾ collapse"), "{open}");
    assert!(open.contains("first thought"), "{open}");

    toggle_transcript_row_details(&mut row);
    assert!(row.details_collapsed);
    let collapsed = thinking_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("first thought"), "{collapsed}");
}

#[test]
pub(crate) fn completed_tool_row_can_collapse_details() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cargo test", "ok\nmore");
    assert!(row.is_expandable());

    let open = tool_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(open.contains("▾ collapse"), "{open}");
    assert!(open.contains("ok"), "{open}");

    toggle_transcript_row_details(&mut row);
    let collapsed = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ details"), "{collapsed}");
    assert!(!collapsed.contains("ok"), "{collapsed}");
}

#[test]
pub(crate) fn long_tool_output_uses_shared_default_collapse() {
    let output = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cat output.txt", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.text.lines().count(), 7);
    assert!(row.text.contains("... 6 more lines"));
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));

    let rendered = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ 6 more lines"), "{rendered}");
    assert!(rendered.contains("  └ line 01"), "{rendered}");
    assert!(rendered.contains("line 02"), "{rendered}");
    assert!(rendered.contains("line 09"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
    assert!(rendered.contains("... 6 more lines"), "{rendered}");
    assert!(!rendered.contains("line 08"), "{rendered}");
}

#[test]
pub(crate) fn long_single_line_tool_output_collapses_by_display_width() {
    let output = format!("{{\"items\":\"{}\"}}", "x".repeat(1400));
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 export", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(row.full_text.is_some());
    assert!(row.text.contains('…'), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 140)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
    assert!(
        rendered.len() < output.len(),
        "rendered output was not bounded"
    );
}

#[test]
pub(crate) fn long_tool_output_collapses_by_display_tokens() {
    let output = (0..LEDGER_BODY_COLLAPSE_TOKENS + 12)
        .map(|_| "x")
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        UnicodeWidthStr::width(output.as_str()) < LEDGER_BODY_COLLAPSE_WIDTH,
        "test fixture should exercise token collapse before width collapse"
    );
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran tokens", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));
    assert!(display_token_count(&row.text) <= LEDGER_BODY_COLLAPSE_TOKENS + 1);
    assert!(row.text.contains('…'), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
}

#[test]
pub(crate) fn line_count_collapse_preview_is_bounded_by_display_tokens() {
    let line = (0..40).map(|_| "field").collect::<Vec<_>>().join(" ");
    let output = (1..=12)
        .map(|index| format!("{index}|{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 -separator '|'", output);

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert!(!row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(
        row.text.contains('…') || row.text.contains("... omitted middle"),
        "{}",
        row.text
    );
    assert!(
        display_token_count(&row.text) <= LEDGER_BODY_COLLAPSE_TOKENS,
        "{}",
        row.text
    );

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
    assert!(!rendered.contains("▸ 6 more lines"), "{rendered}");
}

#[test]
pub(crate) fn unbroken_table_output_collapses_by_display_tokens() {
    let output = "-".repeat(900);
    assert!(
        output.lines().count() <= LEDGER_BODY_COLLAPSE_HEAD_LINES + LEDGER_BODY_COLLAPSE_TAIL_LINES,
        "test fixture should exercise token collapse before line collapse"
    );
    assert!(
        UnicodeWidthStr::width(output.as_str()) < LEDGER_BODY_COLLAPSE_WIDTH,
        "test fixture should exercise token collapse before width collapse"
    );
    let row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran sqlite3 -column", output.clone());

    assert!(row.is_expandable());
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(output.as_str()));
    assert!(row.text.contains('…'), "{}", row.text);
    assert!(row.text.len() < output.len(), "{}", row.text);

    let rendered = tool_lines(&row, false, true, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▸ more output"), "{rendered}");
}

#[test]
pub(crate) fn expanded_tool_output_restores_full_text() {
    let output = (1..=12)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "Ran cat output.txt", output);

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    let rendered = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▾ collapse"), "{rendered}");
    assert!(rendered.contains("line 03"), "{rendered}");
    assert!(rendered.contains("line 12"), "{rendered}");
    assert!(!rendered.contains("... 6 more lines"), "{rendered}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(row.details_collapsed);
    let title_only = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(title_only.contains("▸ details"), "{title_only}");
    assert!(!title_only.contains("line 01"), "{title_only}");

    toggle_transcript_row_details(&mut row);
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    let preview = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(preview.contains("▸ 6 more lines"), "{preview}");
    assert!(preview.contains("line 01"), "{preview}");
    assert!(preview.contains("line 12"), "{preview}");
    assert!(!preview.contains("line 08"), "{preview}");
}

#[test]
pub(crate) fn long_command_and_long_output_expand_together() {
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48073680 ORDER BY id\"";
    let output = (1..=12)
        .map(|line| format!("json row {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, format!("Ran {command}"), output);

    let collapsed = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ 6 more lines"), "{collapsed}");
    assert!(
        !collapsed.contains("command: cd /home/kevin"),
        "{collapsed}"
    );
    assert!(collapsed.contains("json row 12"), "{collapsed}");

    toggle_transcript_row_details(&mut row);
    let expanded = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(
        expanded.contains("command: cd /home/kevin/Projects/feedgarden"),
        "{expanded}"
    );
    assert!(expanded.contains("json row 12"), "{expanded}");
}

#[test]
pub(crate) fn edit_tool_row_renders_inline_codex_style_diff() {
    let diff = "diff --git a/primes.py b/primes.py\n--- a/primes.py\n+++ b/primes.py\n@@ -1,2 +1,2 @@\n def main():\n-    limit = 1000\n+    limit = 2000\n";
    let mut row = TranscriptRow::with_title(TranscriptKind::Updated, "edit primes.py", diff);
    row.tool_name = Some("edit".to_string());

    let lines = tool_lines(&row, false, true, 80);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(rendered.contains("Edited primes.py (+1 -1)"), "{rendered}");
    assert!(rendered.contains("1  def main():"), "{rendered}");
    assert!(rendered.contains("2 -    limit = 1000"), "{rendered}");
    assert!(rendered.contains("2 +    limit = 2000"), "{rendered}");
    assert!(!rendered.contains("1     1 |"), "{rendered}");
}

#[test]
pub(crate) fn malformed_edit_tool_diff_falls_back_to_plain_body() {
    let mut row =
        TranscriptRow::with_title(TranscriptKind::Updated, "edit primes.py", "not a patch");
    row.tool_name = Some("edit".to_string());

    let rendered = tool_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("edit primes.py"), "{rendered}");
    assert!(rendered.contains("not a patch"), "{rendered}");
}

#[test]
pub(crate) fn long_running_command_row_expands_full_command() {
    let command = "cd /home/kevin/Projects/feedgarden && sqlite3 feeds/.cache/hn.db \"SELECT id || '|' || by || '|' || text FROM comments WHERE story_id = 48073680 ORDER BY id\"";
    let mut row =
        TranscriptRow::with_title(TranscriptKind::Ran, format!("Ran {command}"), "running");
    row.tool_started = Some(Instant::now());
    assert!(row.is_expandable());

    let collapsed = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(collapsed.contains("▸ command"), "{collapsed}");
    assert!(
        !collapsed.contains("command: cd /home/kevin"),
        "{collapsed}"
    );

    toggle_transcript_row_details(&mut row);
    assert!(row.expanded);
    let expanded = tool_lines(&row, false, true, 72)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("▾ collapse"), "{expanded}");
    assert!(
        expanded.contains("command: cd /home/kevin/Projects/feedgarden"),
        "{expanded}"
    );
    assert!(!expanded.contains("  └ running"), "{expanded}");
}

#[tokio::test]
pub(crate) async fn ctrl_t_enters_transcript_focus_without_toggling_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    ui.transcript.push(row);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    )
    .await
    .expect("ctrl-t");

    assert_eq!(ui.focus, FocusMode::Transcript);
    assert!(ui.selected_target.is_some());
    assert!(!ui.transcript[0].expanded);
}

#[tokio::test]
pub(crate) async fn mouse_click_toggles_expandable_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 80, 12);

    click_transcript_target(&mut app, &mut ui, TranscriptHitTarget::Row(row_id)).await;

    assert!(ui.transcript[0].expanded);
}

#[tokio::test]
pub(crate) async fn mouse_drag_selection_does_not_toggle_transcript_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.clipboard = Arc::new(|_| Ok(()));
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "abcdef");
    row.full_text = Some("abcdef\nmore".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 80, 12);
    let area = target_area(&ui, TranscriptHitTarget::Row(row_id));

    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), area.x + 2, area.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Drag(MouseButton::Left), area.x + 7, area.y),
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        mouse_event(MouseEventKind::Up(MouseButton::Left), area.x + 7, area.y),
    )
    .await
    .expect("mouse up");
    if app.clipboard_copies_in_flight > 0 {
        wait_for_clipboard_task(&mut app, &mut ui).await;
    }

    assert!(!ui.transcript[0].expanded);
}

#[test]
pub(crate) fn selected_answer_uses_single_line_focus_marker() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let row = TranscriptRow::simple(
        TranscriptKind::Answer,
        "first line\nsecond line\nthird line",
    );

    let rendered = answer_lines(&row, true, true, 80, &app.workdir, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(
        rendered.first().is_some_and(|line| line.starts_with("› ")),
        "{rendered:?}"
    );
    assert!(
        !rendered.iter().skip(1).any(|line| line.starts_with("> ")),
        "{rendered:?}"
    );
    assert!(
        !rendered.iter().skip(1).any(|line| line.starts_with("› ")),
        "{rendered:?}"
    );
}

#[test]
pub(crate) fn selected_thinking_and_tool_rows_use_single_line_focus_marker() {
    let thinking = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "first thought\nsecond thought",
    );
    let thinking = thinking_lines(&thinking, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(
        thinking
            .first()
            .is_some_and(|line| line.starts_with("› Thinking")),
        "{thinking:?}"
    );
    assert!(
        thinking.get(1).is_some_and(|line| line.starts_with("  └ ")),
        "{thinking:?}"
    );
    assert!(
        !thinking.iter().skip(1).any(|line| line.starts_with("> ")),
        "{thinking:?}"
    );

    let tool = TranscriptRow::with_title(TranscriptKind::Ran, "Ran ls", "ok\nmore");
    let tool = tool_lines(&tool, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert!(
        tool.first()
            .is_some_and(|line| line.starts_with("› exec_command ls")),
        "{tool:?}"
    );
    assert!(
        tool.get(1).is_some_and(|line| line.starts_with("  └ ")),
        "{tool:?}"
    );
    assert!(
        !tool.iter().skip(1).any(|line| line.starts_with("> ")),
        "{tool:?}"
    );
}

#[test]
pub(crate) fn status_rows_use_quiet_notice_marker_and_hide_default_title() {
    let row = TranscriptRow::simple(TranscriptKind::Status, "mode: plan\nskill loaded: reviewer");
    let lines = status_lines(&row, false, true, 80);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>();

    assert_eq!(rendered, vec!["· mode: plan", "  skill loaded: reviewer"]);
    assert_eq!(lines[0].spans[0].style, tui_theme().dim_style());
    assert!(
        !rendered.iter().any(|line| line.contains("Status")),
        "{rendered:?}"
    );
    assert!(
        !rendered.iter().any(|line| line.contains("▌")),
        "{rendered:?}"
    );

    let selected = status_lines(&row, true, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(selected, vec!["› mode: plan", "  skill loaded: reviewer"]);
    assert!(
        !selected.iter().skip(1).any(|line| line.starts_with("› ")),
        "{selected:?}"
    );
}

#[test]
pub(crate) fn status_tool_rows_keep_title_hint_and_tree_detail() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "Tool view_skill",
        "# X Daily\n... 174 more lines",
    );
    row.full_text = Some("# X Daily\nfull skill content".to_string());

    let rendered = status_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(rendered[0].starts_with("· Tool view_skill"), "{rendered:?}");
    assert!(rendered[0].contains("▸ 174 more lines"), "{rendered:?}");
    assert_eq!(rendered[1], "  └ # X Daily");
    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("... 174 more lines")),
        "{rendered:?}"
    );
    assert!(
        !rendered.iter().any(|line| line.contains("▌")),
        "{rendered:?}"
    );
}

#[test]
pub(crate) fn clarify_status_results_use_shared_status_notice_renderer() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "Questions 1/1 answered",
        "Which mode should we use?\nanswer: Fast\nnote: include tests",
    );
    row.tool_name = Some("clarify".to_string());

    let rendered = status_lines(&row, false, true, 80)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert_eq!(rendered[0], "· Questions 1/1 answered");
    assert_eq!(rendered[1], "  └ Which mode should we use?");
    assert_eq!(rendered[2], "    answer: Fast");
    assert_eq!(rendered[3], "    note: include tests");
    assert!(
        !rendered.iter().any(|line| line.starts_with("• Questions")),
        "{rendered:?}"
    );
    assert!(
        !rendered.iter().any(|line| line.starts_with("  • ")),
        "{rendered:?}"
    );
}

pub(crate) async fn click_transcript_target(
    app: &mut TuiApp,
    ui: &mut FullscreenUi<'_>,
    target: TranscriptHitTarget,
) {
    let area = target_area(ui, target);
    app.handle_fullscreen_mouse(
        ui,
        mouse_event(MouseEventKind::Down(MouseButton::Left), area.x + 1, area.y),
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        ui,
        mouse_event(MouseEventKind::Up(MouseButton::Left), area.x + 1, area.y),
    )
    .await
    .expect("mouse up");
}

pub(crate) fn target_area(ui: &FullscreenUi<'_>, target: TranscriptHitTarget) -> Rect {
    ui.last_entry_areas
        .iter()
        .find_map(|(entry_target, area)| (*entry_target == target).then_some(*area))
        .expect("target area")
}

pub(crate) fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
pub(crate) fn compact_duration_formatter_uses_whole_seconds_and_minutes() {
    assert_eq!(format_duration_compact(Duration::from_millis(0)), "0s");
    assert_eq!(format_duration_compact(Duration::from_millis(999)), "0s");
    assert_eq!(format_duration_compact(Duration::from_millis(1_000)), "1s");
    assert_eq!(
        format_duration_compact(Duration::from_millis(59_999)),
        "59s"
    );
    assert_eq!(
        format_duration_compact(Duration::from_millis(60_000)),
        "1m00s"
    );
    assert_eq!(
        format_duration_compact(Duration::from_millis(65_000)),
        "1m05s"
    );
    assert_eq!(
        format_duration_compact(Duration::from_millis(140_000)),
        "2m20s"
    );
}

#[test]
pub(crate) fn streaming_tool_calls_parse_id_position_and_complete_arguments() {
    let event = serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_write",
                "name": "write",
                "arguments": {"path": "report.md", "content": "body"},
                "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                "arguments_error": null,
                "content_index": 3,
                "call_index": 2
            }]
        }
    });

    let calls = streaming_tool_calls_from_event(&event);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id.as_deref(), Some("call_write"));
    assert_eq!(calls[0].position_key, "pos:3:2");
    assert_eq!(calls[0].tool_name, "write");
    assert_eq!(calls[0].args["path"], "report.md");
}
