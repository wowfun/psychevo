use crate::tui::app_loop::{FullscreenFrameBackend, StableCursorBackend, draw_fullscreen_frame};
use crate::tui::tests::fixtures::{
    attach_no_steer_running, buffer_text, draw_fullscreen_for_test, test_app,
};
use crate::tui::{
    ClipboardCommand, ClipboardEnvironment, DiffOverlay, FULLSCREEN_EVENT_POLL_INTERVAL,
    FULLSCREEN_PASSIVE_REDRAW_INTERVAL, FullscreenUi, KeyCode, KeyEvent, KeyModifiers, Line,
    ManagedTerminalTitle, Modifier, MouseButton, MouseEvent, MouseEventKind, NO_ARGS, Paragraph,
    Rect, RunMode, ScreenLine, SelectableRegion, SelectionState, TUI_MOUSE_CAPTURE_DISABLE_ANSI,
    TUI_MOUSE_CAPTURE_ENABLE_ANSI, TUI_ROLE_SELECTION_BG, Terminal, TranscriptKind, TranscriptRow,
    TuiApp, base64_encode, copy_text_to_clipboard_with, is_probably_wsl_from,
    local_clipboard_commands_for, mouse_event_needs_redraw, osc52_sequence_with_passthrough,
    passive_redraw_due, schedule_next_passive_redraw, screen_cells_from_text,
    selected_text_from_lines, textarea_with_text, tmux_clipboard_copy_ready,
    write_fullscreen_enter_commands, write_fullscreen_exit_commands,
};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, TestBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullscreenBackendEvent {
    BeginSynchronizedUpdate,
    HideCursor,
    Clear,
    Paint,
    SetCursor(Position),
    ShowCursor,
    EndSynchronizedUpdate,
    Flush,
}

struct RecordingBackend {
    inner: TestBackend,
    events: Vec<FullscreenBackendEvent>,
    fail_next_end: bool,
}

#[derive(Default)]
struct FlushCountingWriter {
    bytes: Vec<u8>,
    flushes: usize,
}

impl io::Write for FlushCountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

impl RecordingBackend {
    fn new(width: u16, height: u16) -> Self {
        Self {
            inner: TestBackend::new(width, height),
            events: Vec::new(),
            fail_next_end: false,
        }
    }

    fn take_semantic_events(&mut self) -> Vec<FullscreenBackendEvent> {
        std::mem::take(&mut self.events)
            .into_iter()
            .filter(|event| *event != FullscreenBackendEvent::Flush)
            .collect()
    }
}

fn assert_flicker_free_cursor_frame(
    events: &[FullscreenBackendEvent],
    expected_position: Position,
) {
    assert_eq!(
        events.first(),
        Some(&FullscreenBackendEvent::BeginSynchronizedUpdate)
    );
    assert!(events.contains(&FullscreenBackendEvent::Paint));
    assert_eq!(
        events.last(),
        Some(&FullscreenBackendEvent::EndSynchronizedUpdate)
    );
    let cursor_index = events
        .iter()
        .rposition(|event| *event == FullscreenBackendEvent::SetCursor(expected_position))
        .expect("final cursor anchor");
    assert!(cursor_index < events.len() - 1);
    assert!(!events[..cursor_index].contains(&FullscreenBackendEvent::ShowCursor));
    assert!(
        events
            .iter()
            .filter(|event| **event == FullscreenBackendEvent::ShowCursor)
            .count()
            <= 1
    );
}

impl Backend for RecordingBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.events.push(FullscreenBackendEvent::Paint);
        self.inner.draw(content).map_err(|never| match never {})
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.events.push(FullscreenBackendEvent::HideCursor);
        self.inner.hide_cursor().map_err(|never| match never {})
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.events.push(FullscreenBackendEvent::ShowCursor);
        self.inner.show_cursor().map_err(|never| match never {})
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner
            .get_cursor_position()
            .map_err(|never| match never {})
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.events
            .push(FullscreenBackendEvent::SetCursor(position));
        self.inner
            .set_cursor_position(position)
            .map_err(|never| match never {})
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.events.push(FullscreenBackendEvent::Clear);
        self.inner.clear().map_err(|never| match never {})
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner
            .clear_region(clear_type)
            .map_err(|never| match never {})
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size().map_err(|never| match never {})
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size().map_err(|never| match never {})
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.events.push(FullscreenBackendEvent::Flush);
        self.inner.flush().map_err(|never| match never {})
    }
}

impl FullscreenFrameBackend for RecordingBackend {
    fn begin_synchronized_update(&mut self) -> Result<(), Self::Error> {
        self.events
            .push(FullscreenBackendEvent::BeginSynchronizedUpdate);
        Ok(())
    }

    fn end_synchronized_update(&mut self) -> Result<(), Self::Error> {
        self.events
            .push(FullscreenBackendEvent::EndSynchronizedUpdate);
        if std::mem::take(&mut self.fail_next_end) {
            return Err(io::Error::other("injected synchronized commit failure"));
        }
        Ok(())
    }
}

#[test]
fn crossterm_backend_emits_synchronized_output_markers() {
    let mut output = FlushCountingWriter::default();
    {
        let mut backend = CrosstermBackend::new(&mut output);
        backend
            .begin_synchronized_update()
            .expect("begin synchronized update");
        backend
            .queue_cursor_visibility(true)
            .expect("queue cursor visibility");
        backend
            .end_synchronized_update()
            .expect("end synchronized update");
    }

    assert_eq!(output.bytes, b"\x1b[?2026h\x1b[?25h\x1b[?2026l");
    assert_eq!(output.flushes, 1, "one frame commit must flush once");
}

#[test]
fn fullscreen_frame_commit_paints_before_anchoring_and_showing_cursor() {
    let backend = StableCursorBackend::hidden(RecordingBackend::new(12, 4));
    let mut terminal = Terminal::new(backend).expect("terminal");

    draw_fullscreen_frame(&mut terminal, true, |frame| {
        frame.render_widget(Paragraph::new("changed"), frame.area());
        frame.set_cursor_position(Position::new(7, 3));
    })
    .expect("draw fullscreen frame");

    let events = terminal.backend_mut().inner_mut().take_semantic_events();
    assert_flicker_free_cursor_frame(&events, Position::new(7, 3));
    assert!(events.ends_with(&[
        FullscreenBackendEvent::SetCursor(Position::new(7, 3)),
        FullscreenBackendEvent::ShowCursor,
        FullscreenBackendEvent::EndSynchronizedUpdate,
    ]));
}

#[test]
fn fullscreen_frame_without_editor_keeps_cursor_hidden() {
    let backend = StableCursorBackend::hidden(RecordingBackend::new(12, 4));
    let mut terminal = Terminal::new(backend).expect("terminal");

    draw_fullscreen_frame(&mut terminal, false, |frame| {
        frame.render_widget(Paragraph::new("read only"), frame.area());
    })
    .expect("draw fullscreen frame");

    let events = terminal.backend_mut().inner_mut().take_semantic_events();
    assert_eq!(
        events.first(),
        Some(&FullscreenBackendEvent::BeginSynchronizedUpdate)
    );
    assert!(events.contains(&FullscreenBackendEvent::Paint));
    assert!(!events.contains(&FullscreenBackendEvent::HideCursor));
    assert!(!events.contains(&FullscreenBackendEvent::ShowCursor));
    assert_eq!(
        events.last(),
        Some(&FullscreenBackendEvent::EndSynchronizedUpdate)
    );
}

#[test]
fn consecutive_editable_frames_do_not_restart_cursor_visibility() {
    let backend = StableCursorBackend::hidden(RecordingBackend::new(12, 4));
    let mut terminal = Terminal::new(backend).expect("terminal");

    for label in ["first", "second"] {
        draw_fullscreen_frame(&mut terminal, false, |frame| {
            frame.render_widget(Paragraph::new(label), frame.area());
            frame.set_cursor_position(Position::new(7, 3));
        })
        .expect("draw fullscreen frame");
        if label == "first" {
            terminal.backend_mut().inner_mut().take_semantic_events();
        }
    }

    let events = terminal.backend_mut().inner_mut().take_semantic_events();
    assert!(!events.contains(&FullscreenBackendEvent::HideCursor));
    assert!(!events.contains(&FullscreenBackendEvent::ShowCursor));
    assert!(events.ends_with(&[
        FullscreenBackendEvent::SetCursor(Position::new(7, 3)),
        FullscreenBackendEvent::EndSynchronizedUpdate,
    ]));
}

#[test]
fn failed_frame_commit_retries_unconfirmed_cursor_visibility() {
    let backend = StableCursorBackend::hidden(RecordingBackend::new(12, 4));
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.backend_mut().inner_mut().fail_next_end = true;

    draw_fullscreen_frame(&mut terminal, false, |frame| {
        frame.set_cursor_position(Position::new(7, 3));
    })
    .expect_err("injected commit failure");
    terminal.backend_mut().inner_mut().take_semantic_events();

    draw_fullscreen_frame(&mut terminal, false, |frame| {
        frame.set_cursor_position(Position::new(7, 3));
    })
    .expect("retry frame");
    let events = terminal.backend_mut().inner_mut().take_semantic_events();
    assert!(
        events.contains(&FullscreenBackendEvent::ShowCursor),
        "a failed synchronized commit must not suppress the next visibility command"
    );
}

#[tokio::test]
async fn composer_and_spinner_redraws_keep_cursor_hidden_until_anchored() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let backend = StableCursorBackend::hidden(RecordingBackend::new(80, 12));
    let mut terminal = Terminal::new(backend).expect("terminal");

    for text in ["", "x", ""] {
        ui.textarea = textarea_with_text(text);
        draw_fullscreen_frame(&mut terminal, false, |frame| {
            app.render_fullscreen(frame, &mut ui)
        })
        .expect("draw composer transition");
        let input = ui.last_composer_input_area.expect("composer input area");
        let expected_x = input.x.saturating_add(text.len() as u16);
        let events = terminal.backend_mut().inner_mut().take_semantic_events();
        assert_flicker_free_cursor_frame(&events, Position::new(expected_x, input.y));
    }

    attach_no_steer_running(&app, &mut ui);
    let mut spinner_frames = Vec::new();
    for elapsed in [Duration::ZERO, Duration::from_millis(120)] {
        ui.running_elapsed_override = Some(elapsed);
        draw_fullscreen_frame(&mut terminal, false, |frame| {
            app.render_fullscreen(frame, &mut ui)
        })
        .expect("draw spinner frame");
        let input = ui.last_composer_input_area.expect("composer input area");
        let events = terminal.backend_mut().inner_mut().take_semantic_events();
        assert_flicker_free_cursor_frame(&events, Position::new(input.x, input.y));
        spinner_frames.push(buffer_text(terminal.backend().inner().inner.buffer()));
    }
    assert!(spinner_frames[0].contains('⠋'));
    assert!(spinner_frames[1].contains('⠙'));

    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[tokio::test]
async fn diff_overlay_suppresses_the_composer_cursor_anchor() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    let backend = StableCursorBackend::hidden(RecordingBackend::new(80, 12));
    let mut terminal = Terminal::new(backend).expect("terminal");
    draw_fullscreen_frame(&mut terminal, false, |frame| {
        app.render_fullscreen(frame, &mut ui)
    })
    .expect("initial composer frame");
    terminal.backend_mut().inner_mut().take_semantic_events();

    ui.diff_overlay = Some(DiffOverlay::from_lines(vec![Line::from("diff")]));
    draw_fullscreen_frame(&mut terminal, false, |frame| {
        app.render_fullscreen(frame, &mut ui)
    })
    .expect("diff overlay frame");

    let events = terminal.backend_mut().inner_mut().take_semantic_events();
    assert!(events.contains(&FullscreenBackendEvent::HideCursor));
    assert!(!events.contains(&FullscreenBackendEvent::ShowCursor));
}
#[tokio::test]
pub(crate) async fn tui_mouse_capture_avoids_any_motion_tracking() {
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1000h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1002h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1006h"));
    assert!(TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1007h"));
    assert!(!TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1007l"));
    assert!(!TUI_MOUSE_CAPTURE_ENABLE_ANSI.contains("?1003h"));
}

#[tokio::test]
pub(crate) async fn tui_mouse_capture_disable_restores_alternate_scroll() {
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1007l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1006l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1002l"));
    assert!(TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1000l"));
    assert!(!TUI_MOUSE_CAPTURE_DISABLE_ANSI.contains("?1003l"));
}

#[tokio::test]
pub(crate) async fn fullscreen_enter_commands_enable_clean_alternate_screen() {
    let mut output = Vec::new();
    write_fullscreen_enter_commands(&mut output).expect("enter commands");
    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("?1049h"));
    assert!(output.contains("?1000h"));
    assert!(output.contains("?1002h"));
    assert!(output.contains("?1006h"));
    assert!(output.contains("?1007h"));
    assert!(!output.contains("?1007l"));
    assert!(output.contains("\x1b[2J"));
    assert!(output.contains("\x1b[1;1H"));
    assert!(output.contains("\x1b[6 q"));
    assert!(output.ends_with("\x1b[?25l"));
}

#[tokio::test]
pub(crate) async fn fullscreen_exit_commands_restore_terminal_modes() {
    let mut output = Vec::new();
    write_fullscreen_exit_commands(&mut output).expect("exit commands");
    let output = String::from_utf8(output).expect("utf8");
    assert!(output.starts_with("\x1b[?2026l"));
    assert!(output.contains("?1007l"));
    assert!(output.contains("?1006l"));
    assert!(output.contains("?1002l"));
    assert!(output.contains("?1000l"));
    assert!(output.contains("?1049l"));
    let default_cursor = output.rfind("\x1b[0 q").expect("default cursor shape");
    let show_cursor = output.rfind("?25h").expect("show cursor");
    assert!(default_cursor < show_cursor);
    assert!(output.contains("?25h"));
}

#[tokio::test]
pub(crate) async fn terminal_tab_title_uses_session_title_short_id_and_new_session_fallback() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;

    app.current_session = Some("019f691d-3360-75b0-80d0-2678847af384".to_string());
    app.current_session_title = None;
    assert_eq!(app.terminal_tab_title(), "Pevo | 019f691d");

    app.current_session_title = Some("Better Session Title".to_string());
    assert_eq!(app.terminal_tab_title(), "Pevo | Better Session Title");

    app.current_session = None;
    app.current_session_title = None;
    assert_eq!(app.terminal_tab_title(), "Pevo | New session");
}

#[tokio::test]
pub(crate) async fn managed_terminal_title_sanitizes_deduplicates_and_clears_osc_output() {
    let mut title = ManagedTerminalTitle::default();
    let mut output = Vec::new();

    title
        .sync(
            &mut output,
            "Pevo | Review\u{1b}]0;injected\u{7}\u{202e} title",
        )
        .expect("first sync");
    title
        .sync(
            &mut output,
            "Pevo | Review\u{1b}]0;injected\u{7}\u{202e} title",
        )
        .expect("deduplicated sync");
    title.clear(&mut output).expect("clear");

    assert_eq!(
        String::from_utf8(output).expect("utf8"),
        "\u{1b}]0;Pevo | Review]0;injected title\u{7}\u{1b}]0;\u{7}"
    );
}

#[tokio::test]
pub(crate) async fn passive_mouse_motion_does_not_request_redraw() {
    assert!(!mouse_event_needs_redraw(MouseEventKind::Moved));
    assert!(mouse_event_needs_redraw(MouseEventKind::Drag(
        MouseButton::Left
    )));
    assert!(mouse_event_needs_redraw(MouseEventKind::ScrollUp));
}

#[tokio::test]
pub(crate) async fn passive_redraw_due_throttles_timeout_only_motion() {
    let start = Instant::now();
    let mut next_due = schedule_next_passive_redraw(start);

    assert!(!passive_redraw_due(
        start + FULLSCREEN_EVENT_POLL_INTERVAL,
        &mut next_due
    ));
    assert!(passive_redraw_due(
        start + FULLSCREEN_PASSIVE_REDRAW_INTERVAL,
        &mut next_due
    ));
    assert!(!passive_redraw_due(
        start + FULLSCREEN_PASSIVE_REDRAW_INTERVAL + FULLSCREEN_EVENT_POLL_INTERVAL,
        &mut next_due
    ));
}

#[tokio::test]
pub(crate) async fn selection_extracts_text_from_registered_screen_lines() {
    let lines = vec![
        ScreenLine {
            region: SelectableRegion::Transcript,
            y: 1,
            cells: screen_cells_from_text(2, "hello world"),
        },
        ScreenLine {
            region: SelectableRegion::Transcript,
            y: 2,
            cells: screen_cells_from_text(2, "second line"),
        },
    ];
    let selection = SelectionState {
        anchor: Some((8, 1)),
        focus: Some((8, 2)),
        region: Some(SelectableRegion::Transcript),
    };

    assert_eq!(
        selected_text_from_lines(&lines, &selection).as_deref(),
        Some("world\nsecond")
    );
}

#[tokio::test]
pub(crate) async fn selection_uses_rendered_wrapped_transcript_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "alpha beta gamma delta epsilon zeta".to_string(),
    ));

    draw_fullscreen_for_test(&app, &mut ui, 18, 8);

    let first = ui.screen_lines[0].text();
    let second = ui.screen_lines[1].text();
    ui.start_selection(0, ui.screen_lines[0].y);
    ui.update_selection(18, ui.screen_lines[1].y);

    assert_eq!(first, "alpha beta gamma");
    assert_eq!(second, "delta epsilon zeta");
    assert_eq!(ui.selected_text(), Some(format!("{first}\n{second}")));
}

#[tokio::test]
pub(crate) async fn selection_preserves_wide_characters_from_rendered_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("中文测试abc".to_string());

    draw_fullscreen_for_test(&app, &mut ui, 24, 8);
    ui.start_selection(2, 0);
    ui.update_selection(10, 0);

    assert_eq!(ui.screen_lines[0].text(), "› 中文测试abc");
    assert_eq!(ui.selected_text().as_deref(), Some("中文测试"));
}

#[tokio::test]
pub(crate) async fn selection_can_copy_sidebar_rendered_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    draw_fullscreen_for_test(&app, &mut ui, 120, 10);

    let line = ui
        .screen_lines
        .iter()
        .find(|line| line.text() == "Modified Files")
        .expect("sidebar modified files line");
    let (x, y) = (line.first_x(), line.y);
    ui.start_selection(x, y);
    ui.update_selection(x + 14, y);

    assert_eq!(ui.selected_text().as_deref(), Some("Modified Files"));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let cell = buffer.cell((x, y)).expect("sidebar selected cell");
    assert!(cell.modifier.contains(Modifier::REVERSED));
    assert!(cell.modifier.contains(Modifier::BOLD));
}

#[tokio::test]
pub(crate) async fn sidebar_omits_context_section_and_footer_chrome() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.current_mode = RunMode::Plan;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 18);
    let text = buffer_text(&buffer);

    assert!(text.contains("Review sidebar polish"));
    assert!(text.contains("Modified Files"));
    for omitted in [
        "Context",
        "cwd:",
        "branch:",
        "messages:",
        "tool calls:",
        "tokens:",
        "context:",
        "cost:",
        "source: tui",
        "mode: plan",
        "Footer",
        "local facts only",
    ] {
        assert!(
            !text.contains(omitted),
            "sidebar should omit {omitted:?}:\n{text}"
        );
    }
}

#[tokio::test]
pub(crate) async fn sidebar_render_clears_stale_terminal_cells() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.sidebar_tokens = Some(36_019);
    ui.refresh_sidebar(&app);
    ui.sidebar.changed_files = vec![
        "?? .gitignore".to_string(),
        "?? .opencode/".to_string(),
        "?? .psychevo/".to_string(),
    ];

    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            let lines = (0..24)
                .map(|_| Line::from("g".repeat(120)))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines),
                Rect {
                    x: 0,
                    y: 0,
                    width: 120,
                    height: 24,
                },
            );
        })
        .expect("pollute frame");

    draw_fullscreen_frame(&mut terminal, false, |frame| {
        app.render_fullscreen(frame, &mut ui)
    })
    .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let text = buffer_text(&buffer);
    let sidebar_x = 120 - 42;

    assert!(text.contains("Modified Files"), "{text}");
    assert!(!text.contains("tokens:"), "{text}");
    assert!(!text.contains("gokens"), "{text}");
    assert_eq!(
        buffer
            .cell((sidebar_x + 4, 21))
            .expect("blank sidebar cell")
            .symbol(),
        " "
    );
}

#[tokio::test]
pub(crate) async fn multiline_transcript_selection_ignores_same_row_sidebar_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau"
                .to_string(),
        ));
    ui.refresh_sidebar(&app);

    draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let transcript_rows = ui
        .screen_lines
        .iter()
        .filter(|line| line.region == SelectableRegion::Transcript)
        .take(2)
        .map(|line| (line.first_x(), line.y, line.text()))
        .collect::<Vec<_>>();
    assert_eq!(transcript_rows.len(), 2);
    let sidebar_row = ui
        .screen_lines
        .iter()
        .find(|line| line.region == SelectableRegion::Sidebar && line.y == transcript_rows[0].1)
        .map(|line| (line.first_x(), line.y, line.text()))
        .expect("same-row sidebar text");

    ui.start_selection(transcript_rows[0].0, transcript_rows[0].1);
    ui.update_selection(78, transcript_rows[1].1);
    let selected = ui.selected_text().expect("selected text");

    assert!(selected.contains("alpha beta gamma"));
    assert!(selected.contains("lambda"));
    assert!(
        !selected.contains(&sidebar_row.2),
        "selected text should not include same-row sidebar text: {selected:?}"
    );
    assert!(!selected.contains("Context"));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 10);
    let sidebar_cell = buffer
        .cell((sidebar_row.0, sidebar_row.1))
        .expect("sidebar cell");
    assert!(!sidebar_cell.modifier.contains(Modifier::REVERSED));
    assert_ne!(sidebar_cell.bg, TUI_ROLE_SELECTION_BG);
}

#[tokio::test]
pub(crate) async fn active_selection_highlights_rendered_buffer_and_esc_clears() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy me".to_string());
    ui.start_selection(2, 0);
    ui.update_selection(6, 0);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    let start = buffer.cell((2, 0)).expect("highlight start");
    assert!(start.modifier.contains(Modifier::REVERSED));
    assert!(start.modifier.contains(Modifier::BOLD));
    assert_ne!(start.bg, TUI_ROLE_SELECTION_BG);
    let end = buffer.cell((5, 0)).expect("highlight end");
    assert!(end.modifier.contains(Modifier::REVERSED));
    assert!(end.modifier.contains(Modifier::BOLD));
    assert_ne!(end.bg, TUI_ROLE_SELECTION_BG);
    let outside = buffer.cell((6, 0)).expect("outside highlight");
    assert!(!outside.modifier.contains(Modifier::REVERSED));

    let should_quit = app
        .handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(!should_quit);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    assert!(
        !buffer
            .cell((2, 0))
            .expect("cleared")
            .modifier
            .contains(Modifier::REVERSED)
    );
}

#[tokio::test]
pub(crate) async fn osc52_sequence_encodes_clipboard_text() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(
        osc52_sequence_with_passthrough("hello", false).expect("osc52"),
        "\x1b]52;c;aGVsbG8=\x07"
    );
}

#[tokio::test]
pub(crate) async fn osc52_sequence_encodes_cjk_clipboard_text_as_utf8() {
    assert_eq!(base64_encode("中文测试".as_bytes()), "5Lit5paH5rWL6K+V");
    assert_eq!(
        osc52_sequence_with_passthrough("中文测试", false).expect("osc52"),
        "\x1b]52;c;5Lit5paH5rWL6K+V\x07"
    );
}

#[tokio::test]
pub(crate) async fn osc52_sequence_rejects_oversized_clipboard_payload() {
    let text = "x".repeat(100_001);

    assert!(osc52_sequence_with_passthrough(&text, false).is_err());
}

#[tokio::test]
pub(crate) async fn wsl_clipboard_detection_uses_kernel_markers_without_env() {
    assert!(is_probably_wsl_from(
        Some("Linux version 6.6.87.2-microsoft-standard-WSL2"),
        None,
        false,
        false,
    ));
    assert!(is_probably_wsl_from(
        None,
        Some("6.6.87.2-microsoft-standard-WSL2"),
        false,
        false,
    ));
    assert!(!is_probably_wsl_from(
        Some("Linux version 6.6.87-generic"),
        Some("6.6.87-generic"),
        false,
        false,
    ));
}

#[tokio::test]
pub(crate) async fn wsl_clipboard_candidates_try_powershell_then_clip_exe() {
    let candidates = local_clipboard_commands_for(false, false, true, true);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("powershell.exe")
    );
    assert_eq!(
        candidates.get(1).map(|candidate| candidate.command),
        Some("clip.exe")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xclip")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[tokio::test]
pub(crate) async fn linux_wayland_clipboard_candidates_try_wl_copy_before_x11() {
    let candidates = local_clipboard_commands_for(false, false, false, true);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xclip")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[tokio::test]
pub(crate) async fn linux_x11_clipboard_candidates_fall_back_to_xclip_and_xsel() {
    let candidates = local_clipboard_commands_for(false, false, false, false);

    assert_eq!(
        candidates.first().map(|candidate| candidate.command),
        Some("xclip")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.command == "wl-copy")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.command == "xsel")
    );
}

#[tokio::test]
pub(crate) async fn clipboard_backend_reports_failure_when_all_backends_fail() {
    let candidates = local_clipboard_commands_for(false, false, true, false);
    let mut tried = Vec::new();

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: false,
            tmux_session: false,
        },
        candidates,
        |candidate, _| {
            tried.push(candidate.command);
            Ok(false)
        },
        |_| panic!("local clipboard fallback should not use tmux"),
        |_| Err(io::Error::other("osc blocked")),
    );

    let err = result.expect_err("clipboard failure");
    let message = err.to_string();
    assert_eq!(tried.first().copied(), Some("powershell.exe"));
    assert_eq!(tried.get(1).copied(), Some("clip.exe"));
    assert!(message.contains("powershell.exe unavailable"));
    assert!(message.contains("clip.exe unavailable"));
    assert!(message.contains("OSC52: osc blocked"));
}

#[tokio::test]
pub(crate) async fn local_clipboard_emits_osc52_before_native_commands() {
    let calls = std::cell::RefCell::new(Vec::new());

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: false,
            tmux_session: false,
        },
        vec![ClipboardCommand {
            command: "remote-copy",
            args: NO_ARGS,
        }],
        |candidate, _| {
            calls.borrow_mut().push(candidate.command);
            Ok(true)
        },
        |_| panic!("local clipboard should not use tmux"),
        |text| {
            calls.borrow_mut().push("OSC52");
            assert_eq!(text, "hello");
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(calls.into_inner(), ["OSC52", "remote-copy"]);
}

#[tokio::test]
pub(crate) async fn ssh_clipboard_skips_remote_native_commands_and_uses_osc52() {
    let mut local_calls = 0;
    let mut tmux_calls = 0;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: false,
        },
        local_clipboard_commands_for(false, false, true, true),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |_| {
            tmux_calls += 1;
            Ok(())
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(tmux_calls, 0);
    assert_eq!(osc_text.as_deref(), Some("hello"));
}

#[tokio::test]
pub(crate) async fn ssh_tmux_clipboard_emits_osc52_and_tmux_load_buffer() {
    let mut local_calls = 0;
    let mut tmux_text = None;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |text| {
            tmux_text = Some(text.to_string());
            Ok(())
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(osc_text.as_deref(), Some("hello"));
    assert_eq!(tmux_text.as_deref(), Some("hello"));
}

#[tokio::test]
pub(crate) async fn ssh_tmux_clipboard_succeeds_when_tmux_fails_after_osc52() {
    let mut local_calls = 0;
    let mut tmux_calls = 0;
    let mut osc_text = None;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |_| {
            tmux_calls += 1;
            Err(io::Error::other("tmux unavailable"))
        },
        |text| {
            osc_text = Some(text.to_string());
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(tmux_calls, 1);
    assert_eq!(osc_text.as_deref(), Some("hello"));
}

#[tokio::test]
pub(crate) async fn ssh_tmux_clipboard_succeeds_when_osc52_fails_but_tmux_succeeds() {
    let mut local_calls = 0;
    let mut tmux_text = None;
    let mut osc_calls = 0;

    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| {
            local_calls += 1;
            Ok(true)
        },
        |text| {
            tmux_text = Some(text.to_string());
            Ok(())
        },
        |_| {
            osc_calls += 1;
            Err(io::Error::other("osc blocked"))
        },
    );

    assert!(result.is_ok());
    assert_eq!(local_calls, 0);
    assert_eq!(osc_calls, 1);
    assert_eq!(tmux_text.as_deref(), Some("hello"));
}

#[tokio::test]
pub(crate) async fn ssh_tmux_clipboard_reports_osc52_and_tmux_failures() {
    let result = copy_text_to_clipboard_with(
        "hello",
        ClipboardEnvironment {
            ssh_session: true,
            tmux_session: true,
        },
        local_clipboard_commands_for(false, false, false, false),
        |_, _| panic!("ssh clipboard should not use remote native commands"),
        |_| Err(io::Error::other("tmux unavailable")),
        |_| Err(io::Error::other("osc blocked")),
    );

    let message = result.expect_err("clipboard failure").to_string();
    assert!(message.contains("OSC52: osc blocked"));
    assert!(message.contains("tmux: tmux unavailable"));
}

#[tokio::test]
pub(crate) async fn tmux_clipboard_ready_rejects_disabled_or_missing_forwarding() {
    assert!(
        tmux_clipboard_copy_ready(
            || Ok("external\n".to_string()),
            || Ok("193: Ms: (string) \\033]52;%p1%s;%p2%s\\a\n".to_string()),
        )
        .is_ok()
    );
    assert_eq!(
        tmux_clipboard_copy_ready(
            || Ok("off\n".to_string()),
            || panic!("tmux info should not be queried when forwarding is disabled"),
        )
        .expect_err("disabled forwarding")
        .to_string(),
        "tmux clipboard forwarding is disabled"
    );
    assert_eq!(
        tmux_clipboard_copy_ready(
            || Ok("external\n".to_string()),
            || Ok("193: Ms: [missing]\n".to_string()),
        )
        .expect_err("missing Ms")
        .to_string(),
        "tmux clipboard forwarding is unavailable: missing Ms capability"
    );
}

#[tokio::test]
pub(crate) async fn mouse_drag_copies_selected_text_through_clipboard_sink() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let copied = Arc::new(Mutex::new(Vec::new()));
    let copied_for_sink = Arc::clone(&copied);
    app.clipboard = Arc::new(move |text| {
        copied_for_sink
            .lock()
            .expect("clipboard lock")
            .push(text.to_string());
        Ok(())
    });
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy this line".to_string());
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse drag");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse up");

    assert_eq!(app.clipboard_copies_in_flight, 1);
    wait_for_clipboard_task(&mut app, &mut ui).await;
    assert_eq!(copied.lock().expect("clipboard lock").as_slice(), ["copy"]);
    assert_eq!(ui.selection, SelectionState::default());
}

#[tokio::test]
pub(crate) async fn mouse_up_clipboard_failure_clears_selection_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.clipboard = Arc::new(|_| Err(io::Error::other("blocked")));
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy this line".to_string());
    draw_fullscreen_for_test(&app, &mut ui, 48, 10);

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 6,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse drag");
    let should_quit = app
        .handle_fullscreen_mouse(
            &mut ui,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 6,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
        )
        .await
        .expect("mouse up");

    assert!(!should_quit);
    assert_eq!(ui.selection, SelectionState::default());
    assert_eq!(app.clipboard_copies_in_flight, 1);
    wait_for_clipboard_task(&mut app, &mut ui).await;
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("copy failed: blocked")
    }));
}

pub(crate) async fn wait_for_clipboard_task(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if app.drain_finished_clipboard_copies(ui) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("clipboard task should finish");
}

#[tokio::test]
pub(crate) async fn ctrl_c_copies_active_selection_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    let copied = Arc::new(Mutex::new(Vec::new()));
    let copied_for_sink = Arc::clone(&copied);
    app.clipboard = Arc::new(move |text| {
        copied_for_sink
            .lock()
            .expect("clipboard lock")
            .push(text.to_string());
        Ok(())
    });
    let mut ui = FullscreenUi::new(&app);
    ui.push_screen_line(0, 0, "selected text");
    ui.start_selection(0, 0);
    ui.update_selection(8, 0);

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await
        .expect("ctrl-c");

    assert!(!should_quit);
    assert!(!ui.quit_requested);
    assert_eq!(
        copied.lock().expect("clipboard lock").as_slice(),
        ["selected"]
    );
    assert_eq!(ui.selection, SelectionState::default());
}

#[tokio::test]
pub(crate) async fn clipboard_failure_during_ctrl_c_is_consumed_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp).await;
    app.clipboard = Arc::new(|_| Err(io::Error::other("blocked")));
    let mut ui = FullscreenUi::new(&app);
    ui.push_screen_line(0, 0, "selected text");
    ui.start_selection(0, 0);
    ui.update_selection(8, 0);

    let should_quit = app
        .handle_fullscreen_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await
        .expect("ctrl-c");

    assert!(!should_quit);
    assert!(!ui.quit_requested);
    assert_eq!(ui.selection, SelectionState::default());
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("copy failed: blocked")
    }));
}
