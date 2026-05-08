use super::*;
use ratatui::backend::TestBackend;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn summary(id: &str) -> SessionSummary {
    SessionSummary {
        id: id.to_string(),
        source: "tui".to_string(),
        workdir: "/repo".to_string(),
        model: "model".to_string(),
        provider: "provider".to_string(),
        started_at_ms: 1,
        updated_at_ms: 1,
        ended_at_ms: None,
        end_reason: None,
        message_count: 0,
        tool_call_count: 0,
        title: None,
    }
}

struct TuiCatalogServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl TuiCatalogServer {
    fn new(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let request = read_http_request(&mut stream);
                requests_for_thread.lock().expect("requests").push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base_url: format!("http://{addr}/v1"),
            requests,
        }
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    loop {
        let n = stream.read(&mut buf).expect("request");
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&request).to_string()
}

fn insert_tui_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    seq: i64,
    role: &str,
    timestamp_ms: i64,
    message: Value,
) {
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        rusqlite::params![session_id, seq, role, timestamp_ms, message.to_string()],
    )
    .expect("insert tui message");
}

fn insert_tui_message_with_metadata(
    db_path: &PathBuf,
    session_id: &str,
    seq: i64,
    role: &str,
    content_text: &str,
    message: Value,
    metadata: Option<Value>,
) {
    let conn = rusqlite::Connection::open(db_path).expect("conn");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json,
                content_text, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        rusqlite::params![
            session_id,
            seq,
            role,
            seq,
            message.to_string(),
            content_text,
            metadata.map(|value| value.to_string())
        ],
    )
    .expect("insert tui message");
}

fn test_track_snapshot(app: &TuiApp, session_id: &str) -> String {
    let git_dir = app.home.join("snapshots").join("sessions").join(session_id);
    fs::create_dir_all(&git_dir).expect("snapshot dir");
    if !git_dir.join("HEAD").exists() {
        assert!(
            std::process::Command::new("git")
                .env("GIT_DIR", &git_dir)
                .env("GIT_WORK_TREE", &app.workdir)
                .arg("init")
                .output()
                .expect("snapshot init")
                .status
                .success()
        );
    }
    assert!(
        std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(&app.workdir)
            .args(["add", "--all", "--", "."])
            .output()
            .expect("snapshot add")
            .status
            .success()
    );
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(&git_dir)
        .arg("--work-tree")
        .arg(&app.workdir)
        .arg("write-tree")
        .output()
        .expect("snapshot tree");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write_tui_model_config(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("model-config.jsonc");
    fs::write(
        &path,
        r#"{
              "model": "mock/mock-model",
              "provider": {
                "mock": {
                  "options": {
                    "base_url": "http://127.0.0.1:9",
                    "api_key_env": "TEST_PROVIDER_KEY"
                  },
                  "models": {
                    "mock-model": {},
                    "other-model": { "reasoning_effort": "high" }
                  }
                }
              }
            }"#,
    )
    .expect("config");
    path
}

fn test_app_with_models(temp: &tempfile::TempDir) -> TuiApp {
    let mut app = test_app(temp);
    app.env_map
        .insert("TEST_PROVIDER_KEY".to_string(), "test-key".to_string());
    app.config_path = Some(write_tui_model_config(temp));
    app.current_model = Some("mock/mock-model".to_string());
    app.current_variant = None;
    app.refresh_selected_model();
    app
}

#[test]
fn resolves_unique_and_ambiguous_session_prefixes() {
    let sessions = vec![summary("abcdef"), summary("abc999"), summary("def000")];
    assert_eq!(
        resolve_session_ref_from_summaries(&sessions, "def").unwrap(),
        "def000"
    );
    assert!(resolve_session_ref_from_summaries(&sessions, "abc").is_err());
    assert_eq!(
        resolve_session_ref_from_summaries(&sessions, "latest").unwrap(),
        "abcdef"
    );
}

#[test]
fn turn_printer_hides_reasoning_by_default() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::ReasoningDelta {
                text: "private".to_string(),
            },
            &mut output,
        )
        .expect("delta");
    printer
        .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.is_empty());
    assert!(!output.contains("private"));
}

#[test]
fn turn_printer_shows_reasoning_when_enabled() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), true, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::ReasoningDelta {
                text: "visible thinking".to_string(),
            },
            &mut output,
        )
        .expect("delta");
    printer
        .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("Thinking:"));
    assert!(output.contains("visible thinking"));
}

#[test]
fn turn_printer_preserves_bash_command_title_until_tool_end() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_start",
                "tool_call_id": "call_bash",
                "tool_name": "bash",
                "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
            })),
            &mut output,
        )
        .expect("start");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_end",
                "tool_call_id": "call_bash",
                "tool_name": "bash",
                "result": {"output": "ok", "exit_code": 0},
                "outcome": "normal"
            })),
            &mut output,
        )
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("Ran cargo test -p psychevo-cli: running"));
    assert!(output.contains("Ran cargo test -p psychevo-cli:"));
    assert!(!output.contains("Ran command"));
}

#[test]
fn tui_snapshot_wide_idle_minimal_chrome() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("wide_idle_minimal_chrome", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_wide_optional_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    assert_tui_snapshot("wide_optional_sidebar", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_narrow_idle_composer_without_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("narrow_idle_composer_without_sidebar", 80, 20, &app, ui);
}

#[test]
fn tui_snapshot_slash_menu_prefix_filtering() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.textarea = textarea_with_text("/mo");
    assert_tui_snapshot("slash_menu_prefix_filtering", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_model_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("model panel"),
    ));
    assert_tui_snapshot("model_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_variant_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let models = app.model_selection_panel().expect("model panel");
    let (other, source) = models
        .rows
        .iter()
        .find_map(|row| match &row.value {
            BottomSelectionValue::Model { model, source } if model.model == "other-model" => {
                Some((model.clone(), *source))
            }
            _ => None,
        })
        .expect("other model");
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(app.variant_panel(other, source, models));
    assert_tui_snapshot("variant_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_session_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Sessions(stable_session_bottom_panel()));
    assert_tui_snapshot("session_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_running_turn_with_visible_thinking() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::RunningThinking);
    assert_tui_snapshot("running_turn_with_visible_thinking", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_completed_ledger_collapsed_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::CollapsedTool);
    assert_tui_snapshot("completed_ledger_collapsed_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_expanded_long_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ExpandedTool);
    assert_tui_snapshot("expanded_long_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_debug_meta_with_usage_metadata() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.debug = true;
    let ui = fixture_ui(&app, FixtureKind::DebugMeta);
    assert_tui_snapshot("debug_meta_with_usage_metadata", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_failure_tool_error_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::FailureMeta);
    assert_tui_snapshot("failure_tool_error_turn_meta", 120, 24, &app, ui);
}

#[test]
fn transcript_selection_toggles_expandable_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
    row.full_text = Some("a\nb\nc".to_string());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();
    ui.toggle_selected();
    assert!(ui.transcript[0].expanded);
    ui.toggle_selected();
    assert!(!ui.transcript[0].expanded);
}

#[test]
fn turn_meta_omits_tokens_and_uses_prefixless_debug_parts() {
    let usage = serde_json::json!({
        "input_tokens": 2,
        "output_tokens": 3,
        "total_tokens": 5
    });
    let default = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: None,
        failures: 0,
        debug: false,
    });
    assert_eq!(default, "provider/model");
    let metadata = serde_json::json!({"provider_response_id":"resp"});
    let debug = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        failures: 0,
        debug: true,
    });
    assert!(debug.contains("usage 2 input"));
    assert!(debug.contains("3 output"));
    assert!(debug.contains("metadata response resp"));
    assert!(debug.ends_with("plan"));
    assert!(!debug.contains('='));
}

#[test]
fn turn_meta_prefers_completed_elapsed_metadata() {
    let metadata = serde_json::json!({"elapsed_ms": 120});
    let stale_started = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .expect("instant");

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: Some(stale_started),
        usage: None,
        metadata: Some(&metadata),
        failures: 0,
        debug: true,
    });

    assert!(meta.contains("0.1s"));
    assert!(!meta.contains("5."));
    assert!(!meta.contains("metadata elapsed"));
}

#[test]
fn turn_meta_places_variant_after_model_and_filters_debug_duplicate() {
    let metadata = serde_json::json!({
        "elapsed_ms": 120,
        "reasoning_effort": "high",
        "provider_response_id": "resp"
    });
    let usage = serde_json::json!({"input_tokens": 2});

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        failures: 1,
        debug: true,
    });

    assert_eq!(
        meta,
        "provider/model high  0.1s  1 failure  usage 2 input  metadata response resp  plan"
    );
}

#[test]
fn slash_completion_completes_command_prefixes() {
    assert_eq!(slash_completion("/he"), None);
    assert_eq!(slash_completion("/ren").as_deref(), Some("/rename"));
    assert_eq!(slash_completion("/mo").as_deref(), Some("/mode"));
    assert_eq!(slash_completion("/model"), None);
    assert_eq!(slash_completion("hello"), None);
    assert_eq!(slash_completion("/he\nthere"), None);
}

#[test]
fn bottom_panel_row_right_aligns_detail_with_wide_title() {
    let row = BottomSelectionRow {
        label: "当前模式询问".to_string(),
        description: Some("deepseek/deepseek-v4-pro  messages=2".to_string()),
        detail: Some("08:50".to_string()),
        group: None,
        search_text: String::new(),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::Session("session-a".to_string()),
    };

    let width = 54;
    let text = line_text(&bottom_panel_row(&row, false, width));

    assert!(text.ends_with("08:50"));
    assert_eq!(UnicodeWidthStr::width(text.as_str()), usize::from(width));
}

#[tokio::test]
async fn enter_executes_first_slash_menu_suggestion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/sess");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/sessions"));
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Sessions(_))));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Error)
    );
}

#[tokio::test]
async fn slash_menu_selection_can_choose_mode_over_model() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("usage: /mode <plan|default>"))
    );
}

#[tokio::test]
async fn slash_menu_up_down_wrap_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .expect("up");
    assert_eq!(ui.slash_menu_selected, 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 0);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .expect("down");
    assert_eq!(ui.slash_menu_selected, 0);
}

#[tokio::test]
async fn mode_slash_command_requires_value() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mode");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Error
                && row.text.contains("usage: /mode <plan|default>"))
    );
}

#[tokio::test]
async fn mode_slash_command_sets_mode_with_direct_value() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mode plan");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode plan"));
    assert_eq!(app.current_mode, RunMode::Plan);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Error)
    );
}

#[tokio::test]
async fn fullscreen_status_uses_single_multiline_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Status)
        .await
        .expect("status");

    let status_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Status)
        .collect::<Vec<_>>();
    assert_eq!(status_rows.len(), 1);
    assert!(status_rows[0].text.contains("workdir:"));
    assert!(status_rows[0].text.contains("\nmodel: mock/model\n"));
    assert!(status_rows[0].text.contains("\nvariant: high\n"));
    assert!(status_rows[0].text.contains("\ndebug: off"));
}

#[tokio::test]
async fn fullscreen_undo_restores_prompt_and_redo_restores_transcript() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = app.workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());

    let before_first = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "first prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_first}})),
    );
    fs::write(&file, "after first\n").expect("after first");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "first answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "first answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );
    let before_second = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        3,
        "user",
        "second prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "second prompt"}],
            "timestamp_ms": 3
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_second}})),
    );
    fs::write(&file, "after second\n").expect("after second");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        4,
        "assistant",
        "second answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "second answer"}],
            "timestamp_ms": 4,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("load history");
    ui.textarea = textarea_with_text("/undo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("undo");

    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "first answer")
    );
    assert!(ui.transcript.iter().all(|row| row.text != "second answer"));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status && row.text.contains("prompt restored"))
    );

    ui.textarea = textarea_with_text("/redo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("redo");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "second answer")
    );
}

#[tokio::test]
async fn fullscreen_new_command_resets_session_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("previous prompt".to_string());

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(app.current_session, None);
    assert_eq!(app.current_session_title, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.terminal_clear_requested);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
async fn mouse_click_can_execute_slash_menu_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/mo");
    ui.last_slash_menu_areas = vec![(
        1,
        Rect {
            x: 0,
            y: 2,
            width: 16,
            height: 1,
        },
    )];

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse");

    assert_eq!(ui.history.last().map(String::as_str), Some("/mode"));
}

#[tokio::test]
async fn mouse_wheel_scrolls_transcript_inside_tui() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_transcript_width = 80;
    ui.last_transcript_height = 4;
    for index in 0..12 {
        ui.transcript.push(TranscriptRow::simple(
            TranscriptKind::Answer,
            format!("line {index}"),
        ));
    }
    ui.scroll_to_bottom();
    let bottom = ui.scroll;

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("scroll");

    assert!(ui.scroll < bottom);
    assert!(!ui.auto_follow_transcript);
}

#[tokio::test]
async fn model_command_opens_searchable_bottom_picker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");

    let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.rows.len(), 4);
    assert_eq!(panel.rows[0].label, "All providers");
    assert_eq!(panel.rows[1].label, "mock");
    panel.set_query_char('o');
    panel.set_query_char('t');
    panel.set_query_char('h');
    let filtered = panel.filtered_indices();
    assert_eq!(
        filtered
            .iter()
            .map(|index| panel.rows[*index].label.as_str())
            .collect::<Vec<_>>(),
        vec!["All providers", "mock", "mock/other-model"]
    );
}

#[tokio::test]
async fn model_fetch_all_adds_fetched_rows_and_preserves_query() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"},{"id":"mock-model"}]}"#);
    let config_path = temp.path().join("fetch-config.jsonc");
    fs::write(
        &config_path,
        format!(
            r#"{{
              "model": "mock/mock-model",
              "provider": {{
                "mock": {{
                  "options": {{
                    "base_url": "{}",
                    "api_key_env": "TEST_PROVIDER_KEY"
                  }},
                  "models": {{
                    "mock-model": {{}}
                  }}
                }}
              }}
            }}"#,
            server.base_url
        ),
    )
    .expect("config");
    let mut app = test_app(&temp);
    app.env_map
        .insert("TEST_PROVIDER_KEY".to_string(), "test-key".to_string());
    app.config_path = Some(config_path);
    app.current_model = Some("mock/mock-model".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        for ch in "remote".chars() {
            panel.set_query_char(ch);
        }
        panel.select_value_key("fetch:all");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("fetch");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.query, "remote");
    assert_eq!(
        panel
            .filtered_indices()
            .iter()
            .map(|index| panel.rows[*index].label.as_str())
            .collect::<Vec<_>>(),
        vec!["All providers", "mock", "mock/remote-model"]
    );
    let request = server
        .requests
        .lock()
        .expect("requests")
        .first()
        .cloned()
        .expect("request");
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer test-key")
    );
}

#[tokio::test]
async fn model_fetch_missing_credentials_stays_in_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("missing-config.jsonc");
    fs::write(
        &config_path,
        r#"{
              "model": "mock/mock-model",
              "provider": {
                "mock": {
                  "options": {
                    "base_url": "http://api.example/v1",
                    "api_key_env": "TEST_PROVIDER_KEY"
                  },
                  "models": {
                    "mock-model": {}
                  }
                }
              }
            }"#,
    )
    .expect("config");
    app.config_path = Some(config_path);
    app.current_model = Some("mock/mock-model".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    let provider = panel
        .rows
        .iter()
        .find(|row| row.label == "mock")
        .expect("provider");
    assert_eq!(
        provider.description.as_deref(),
        Some("missing TEST_PROVIDER_KEY")
    );
    assert!(matches!(
        provider.value,
        BottomSelectionValue::ProviderInfo(ref provider) if provider == "mock"
    ));
}

#[tokio::test]
async fn fetched_model_selection_uses_provider_default_and_only_persists_tui_state() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetched;
    state.fetched = vec![ModelCatalogEntry {
        id: "remote-model".to_string(),
        context_limit: None,
    }];
    let config_before =
        fs::read_to_string(app.config_path.as_ref().expect("config")).expect("config before");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.select_value_key("model:mock/remote-model");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select fetched");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]]
            .description
            .as_deref(),
        Some("use provider default")
    );

    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select variant");

    assert_eq!(app.current_model.as_deref(), Some("mock/remote-model"));
    assert_eq!(
        app.state.model_for(&app.workdir_key).as_deref(),
        Some("mock/remote-model")
    );
    assert!(
        app.state
            .recent_models
            .contains(&"mock/remote-model".to_string())
    );
    assert_eq!(
        fs::read_to_string(app.config_path.as_ref().expect("config")).expect("config after"),
        config_before
    );
}

#[test]
fn model_picker_initial_focus_prefers_model_rows_before_fetch_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/mock-model"
    );

    app.current_model = None;
    app.selected_model = None;
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/mock-model"
    );

    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("empty-model-config.jsonc");
    fs::write(
        &config_path,
        r#"{
              "provider": {
                "mock": {
                  "options": { "base_url": "http://127.0.0.1:9" },
                  "models": {}
                }
              }
            }"#,
    )
    .expect("config");
    app.config_path = Some(config_path);
    app.current_model = None;
    app.selected_model = None;
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "All providers"
    );
}

#[tokio::test]
async fn model_picker_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .expect("first row");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect("wrap up");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/other-model"
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "All providers"
    );
}

#[tokio::test]
async fn model_fetch_failure_preserves_old_fetched_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetching;
    state.fetched = vec![ModelCatalogEntry {
        id: "old-remote".to_string(),
        context_limit: None,
    }];
    app.model_catalog.tasks.insert(
        "mock".to_string(),
        tokio::spawn(async {
            ModelCatalogFetchResult {
                provider: "mock".to_string(),
                result: Err("network down".to_string()),
            }
        }),
    );
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("panel"),
    ));

    drain_catalog_until_idle(&mut app, &mut ui).await;

    let state = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider");
    assert_eq!(
        state.status,
        ModelCatalogStatus::Failed("network down".to_string())
    );
    assert_eq!(state.fetched[0].id, "old-remote");
}

#[tokio::test]
async fn model_fetch_cancel_preserves_old_fetched_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetching;
    state.fetched = vec![ModelCatalogEntry {
        id: "old-remote".to_string(),
        context_limit: None,
    }];
    app.model_catalog.tasks.insert(
        "mock".to_string(),
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            ModelCatalogFetchResult {
                provider: "mock".to_string(),
                result: Ok(Vec::new()),
            }
        }),
    );

    app.model_catalog.abort_unfinished();

    let state = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider");
    assert!(app.model_catalog.tasks.is_empty());
    assert_eq!(state.status, ModelCatalogStatus::Fetched);
    assert_eq!(state.fetched[0].id, "old-remote");
}

async fn drain_catalog_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..50 {
        app.drain_model_catalog_fetches(ui)
            .await
            .expect("drain catalog");
        if app.model_catalog.tasks.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("catalog fetch did not finish");
}

#[tokio::test]
async fn model_selection_opens_variant_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");

    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert!(panel.title.contains("mock/other-model"));
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "Config default"
    );
}

#[tokio::test]
async fn model_variant_panel_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down to other model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect("wrap up");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "max"
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "Config default"
    );
}

#[tokio::test]
async fn model_config_default_clears_variant_override() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_variant = Some("xhigh".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select config default");

    assert_eq!(app.current_model.as_deref(), Some("mock/other-model"));
    assert_eq!(app.current_variant, None);
    assert_eq!(
        app.state.model_for(&app.workdir_key).as_deref(),
        Some("mock/other-model")
    );
    assert_eq!(app.state.variant_for(&app.workdir_key), None);
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
async fn model_explicit_variant_persists_override() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    for ch in "xhigh".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select variant");

    assert_eq!(app.current_model.as_deref(), Some("mock/other-model"));
    assert_eq!(app.current_variant.as_deref(), Some("xhigh"));
    assert_eq!(
        app.state.variant_for(&app.workdir_key).as_deref(),
        Some("xhigh")
    );
}

#[tokio::test]
async fn model_variant_escape_returns_to_model_then_closes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("back");
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Models(_))));

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("close");
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
async fn fullscreen_thinking_toggle_hides_existing_blocks_without_status() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "private reasoning",
    ));

    app.handle_fullscreen_command(&mut ui, SlashCommand::ThinkingSet(false))
        .await
        .expect("thinking off");

    assert!(!ui.thinking_visible);
    assert_eq!(
        transcript_line_count(&ui.transcript, 80, ui.thinking_visible),
        0
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );

    app.handle_fullscreen_command(&mut ui, SlashCommand::ThinkingSet(true))
        .await
        .expect("thinking on");
    assert!(ui.thinking_visible);
    assert!(transcript_line_count(&ui.transcript, 80, ui.thinking_visible) > 0);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
async fn tab_completes_slash_command_without_switching_mode() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/ren");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("tab");

    assert_eq!(textarea_text(&ui.textarea), "/rename");
    assert_eq!(app.current_mode, RunMode::Build);
}

#[tokio::test]
async fn shift_tab_cycles_mode_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    )
    .await
    .expect("shift tab");

    assert_eq!(app.current_mode, RunMode::Plan);
    assert!(
        !ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Status && row.text.contains("mode:"))
    );
}

#[tokio::test]
async fn fullscreen_drain_keeps_queued_events_after_task_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "final answer"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal"
        }
    })))
    .expect("send answer");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"}
    })))
    .expect("send start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_read_fixture",
        "tool_name": "read",
        "args": {"path": "fixture.txt"},
        "result": {"path": "fixture.txt", "content": "fixture content"},
        "outcome": "normal"
    })))
    .expect("send end");
    drop(tx);

    let result = psychevo_runtime::RunResult {
        session_id: "finished-session".to_string(),
        outcome: Outcome::Normal,
        final_answer: "done".to_string(),
        db_path: app.db_path.clone(),
        workdir: app.workdir.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 0,
        events: Vec::new(),
    };
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn { control, rx, task });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui).await.expect("drain");

    let tool_row = ui
        .transcript
        .iter()
        .find(|row| row.title == "Explored fixture.txt")
        .expect("tool evidence row");
    assert_eq!(tool_row.kind, TranscriptKind::Explored);
    assert_eq!(tool_row.text, "fixture content");
    let tool_index = ui
        .transcript
        .iter()
        .position(|row| row.title == "Explored fixture.txt")
        .expect("tool index");
    let answer_index = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer)
        .expect("answer index");
    assert!(tool_index < answer_index);
    assert!(ui.running.is_none());
}

#[tokio::test]
async fn fullscreen_agent_end_releases_turn_before_auxiliary_task_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "provider": "mock",
            "model": "mock-model"
        }
    })))
    .expect("send answer");
    tx.send(RunStreamEvent::Event(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");

    let result = psychevo_runtime::RunResult {
        session_id: "streamed-session".to_string(),
        outcome: Outcome::Normal,
        final_answer: "hi".to_string(),
        db_path: app.db_path.clone(),
        workdir: app.workdir.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 0,
        events: Vec::new(),
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn { control, rx, task });

    app.drain_fullscreen_events(&mut ui).await.expect("drain");
    let _ = done_tx.send(());

    assert!(ui.running.is_none());
    assert_eq!(app.current_session.as_deref(), Some("streamed-session"));
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "hi")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "a turn is already running")
    );
}

#[test]
fn fullscreen_loads_current_session_history() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text
            ) VALUES (?1, 1, 'user', 1000, ?2, 'hello')
            "#,
        rusqlite::params![
            &session_id,
            serde_json::json!({
                "role": "user",
                "content": [{"text": "hello"}],
                "timestamp_ms": 1000
            })
            .to_string()
        ],
    )
    .expect("insert user");
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text,
                usage_json, metadata_json
            ) VALUES (?1, 2, 'assistant', 2500, ?2, 'hi', ?3, ?4)
            "#,
        rusqlite::params![
            &session_id,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "reasoning",
                        "text": "folded thought",
                        "provider_evidence": {
                            "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                        }
                    },
                    {"type": "text", "text": "hi"}
                ],
                "timestamp_ms": 2500,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mock-model",
                "provider": "mock"
            })
            .to_string(),
            serde_json::json!({"total_tokens": 12}).to_string(),
            serde_json::json!({"provider_response_id": "resp_1"}).to_string()
        ],
    )
    .expect("insert assistant");
    insert_tui_message(
        &conn,
        &session_id,
        3,
        "user",
        3000,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "follow-up"}],
            "timestamp_ms": 3000
        }),
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui).expect("history");

    assert_eq!(ui.transcript[0].kind, TranscriptKind::Prompt);
    assert_eq!(ui.transcript[0].text, "hello");
    assert_eq!(ui.transcript[1].kind, TranscriptKind::Thinking);
    assert_eq!(ui.transcript[1].text, "folded thought");
    assert_eq!(ui.transcript[2].kind, TranscriptKind::Answer);
    assert_eq!(ui.transcript[2].text, "hi");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Meta
                && row.text.contains("1.5s")
                && !row.text.contains("response resp_1"))
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("tokens="))
    );
    assert_eq!(ui.sidebar_tokens, Some(12));
    assert_eq!(ui.history, ["hello", "follow-up"]);
    ui.textarea = textarea_with_text("draft");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "follow-up");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}

#[tokio::test]
async fn sessions_panel_switches_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.workdir, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &first,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
    );
    conn.execute(
        r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text
            ) VALUES (?1, 1, 'user', 1, ?2, 'second prompt')
            "#,
        rusqlite::params![
            &second,
            serde_json::json!({
                "role": "user",
                "content": [{"text": "second prompt"}],
                "timestamp_ms": 1
            })
            .to_string()
        ],
    )
    .expect("insert second prompt");

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("first history");
    assert_eq!(ui.history.as_slice(), ["first prompt"]);
    ui.push_submitted_history("/sessions".to_string());
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
    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Prompt && row.text == "second prompt")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
    assert_eq!(ui.history.as_slice(), ["second prompt", "/sessions"]);
    ui.textarea = textarea_with_text("draft");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "/sessions");
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "/sessions");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
}

#[tokio::test]
async fn sessions_panel_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.workdir, "tui", "model-a", "mock", None)
        .expect("first");
    store
        .create_session_with_metadata(&app.workdir, "tui", "model-b", "mock", None)
        .expect("second");
    app.current_session = Some(first);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Sessions)
        .await
        .expect("sessions");
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

#[test]
fn session_display_messages_count_visible_prompts_and_answers() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
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

    assert_eq!(ui.sidebar.message_count, 2);
    assert_eq!(ui.sidebar.tool_count, 1);
    assert_eq!(
        app.session_list_lines().expect("session list"),
        [format!(
            "{} tui mock/mock-model messages=2",
            short_session(&session_id)
        )]
    );
    let panel = app.session_selection_panel().expect("session panel");
    let row = panel
        .rows
        .iter()
        .find(|row| matches!(&row.value, BottomSelectionValue::Session(id) if id == &session_id))
        .expect("session row");
    assert_eq!(
        row.description.as_deref(),
        Some("mock/mock-model  messages=2")
    );
}

#[test]
fn transcript_auto_follow_tracks_wrapped_streaming_content() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_transcript_width = 32;
    ui.last_transcript_height = 4;
    for index in 0..6 {
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            format!("prior answer {index}"),
        ));
    }
    ui.scroll_to_bottom();
    let initial_bottom = ui.scroll;

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(80)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();

    assert!(ui.scroll > initial_bottom);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());

    ui.scroll_transcript(-2);
    assert!(!ui.auto_follow_transcript);
    let manual_scroll = ui.scroll;
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(120)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();
    assert_eq!(ui.scroll, manual_scroll);

    ui.scroll_transcript(10_000);
    assert!(ui.auto_follow_transcript);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
}

#[test]
fn long_read_tool_output_collapses_and_preserves_full_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let content = (1..=64)
        .map(|line| format!("{line:02}: fn rendered_fixture() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_read_long",
            "tool_name": "read",
            "args": {"path": "src/long.rs"},
            "result": {"path": "src/long.rs", "content": content},
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Explored)
        .expect("read evidence row");
    assert_eq!(row.title, "Explored src/long.rs");
    assert_eq!(row.text.lines().count(), 21);
    assert!(row.text.contains("... 44 more lines"));
    assert_eq!(row.full_text.as_deref(), Some(content.as_str()));
    assert!(row.is_expandable());
}

#[test]
fn running_tool_title_right_aligns_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test --workspace --all-targets",
        "running",
    );
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(120))
            .expect("instant"),
    );

    let title = line_text(&tool_lines(&row, false, true, 36)[0]);

    assert!(title.starts_with("• Ran cargo"));
    assert!(title.ends_with("0.1s"));
    assert_eq!(UnicodeWidthStr::width(title.as_str()), 36);
}

#[test]
fn completed_tool_title_uses_fixed_elapsed_duration() {
    let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored src/lib.rs", "");
    row.tool_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(5))
            .expect("instant"),
    );
    row.tool_elapsed = Some(Duration::from_millis(120));

    let title = line_text(&tool_lines(&row, false, true, 32)[0]);

    assert!(title.ends_with("0.1s"));
    assert!(!title.contains("5."));
}

#[test]
fn narrow_tool_title_preserves_elapsed_duration() {
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Ran,
        "Ran cargo test --workspace --all-targets",
        "",
    );
    row.tool_elapsed = Some(Duration::from_millis(12_340));

    let title = line_text(&tool_lines(&row, false, true, 18)[0]);

    assert!(title.ends_with("12.3s"));
    assert!(title.contains('…'));
}

#[test]
fn history_tool_result_restores_elapsed_duration() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "tool_result",
            "tool_call_id": "call_read",
            "tool_name": "read",
            "content": "{\"path\":\"src/lib.rs\",\"content\":\"done\"}",
            "is_error": false,
            "timestamp_ms": 2
        }),
        None,
        Some(&serde_json::json!({"elapsed_ms": 230})),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Explored)
        .expect("tool row");
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(230)));
    assert!(line_text(&tool_lines(row, false, true, 32)[0]).ends_with("0.2s"));
}

#[test]
fn history_meta_uses_persisted_variant_not_current_variant() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_variant = Some("xhigh".to_string());
    let mut ui = FullscreenUi::new(&app);

    ui.push_history_message(
        &serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
        Some(&serde_json::json!({
            "elapsed_ms": 230,
            "reasoning_effort": "high"
        })),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Meta)
        .expect("meta row");
    assert_eq!(row.text, "mock/mock-model high  0.2s");
    assert!(!row.text.contains("xhigh"));
}

#[test]
fn prompt_block_uses_full_width_background_without_left_rail() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("inspect prompt styling".to_string());
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "›");
    assert_eq!(buffer.cell((0, 0)).expect("cell").bg, TUI_SURFACE_BG);
    assert_eq!(buffer.cell((47, 0)).expect("cell").bg, TUI_SURFACE_BG);
    assert_ne!(buffer.cell((0, 0)).expect("cell").symbol(), "▌");
}

#[test]
fn composer_and_prompt_share_full_width_surface() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("match the composer surface".to_string());
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let composer_y = 7;

    assert_eq!(buffer.cell((0, 0)).expect("prompt marker").symbol(), "›");
    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("composer marker")
            .symbol(),
        "›"
    );
    assert_eq!(buffer.cell((0, 0)).expect("prompt bg").bg, TUI_SURFACE_BG);
    assert_eq!(
        buffer.cell((47, 0)).expect("prompt trailing bg").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer.cell((0, composer_y)).expect("composer bg").bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer
            .cell((47, composer_y))
            .expect("composer trailing bg")
            .bg,
        TUI_SURFACE_BG
    );
    assert_ne!(
        buffer
            .cell((0, composer_y))
            .expect("composer rail")
            .symbol(),
        "│"
    );
    assert_eq!(
        buffer
            .cell((0, composer_y + 1))
            .expect("composer row bg")
            .bg,
        TUI_SURFACE_BG
    );
    assert_eq!(
        buffer
            .cell((47, composer_y + 1))
            .expect("composer row trailing bg")
            .bg,
        TUI_SURFACE_BG
    );
}

#[test]
fn wrapped_prompt_rows_keep_full_width_background_for_wide_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("中文测试中文测试中文测试中文测试".to_string());
    let backend = TestBackend::new(24, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.cell((0, 0)).expect("first marker").symbol(), "›");
    assert_eq!(
        buffer.cell((0, 1)).expect("continuation marker").symbol(),
        " "
    );
    for y in 0..=1 {
        assert_eq!(
            buffer.cell((0, y)).expect("row start").bg,
            TUI_SURFACE_BG,
            "row {y} start"
        );
        assert_eq!(
            buffer.cell((23, y)).expect("row end").bg,
            TUI_SURFACE_BG,
            "row {y} end"
        );
    }
}

#[test]
fn empty_composer_uses_two_surface_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render_fullscreen(frame, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let composer_y = 7;

    assert_eq!(
        buffer
            .cell((0, composer_y))
            .expect("composer marker")
            .symbol(),
        "›"
    );
    for y in composer_y..=composer_y + 1 {
        assert_eq!(
            buffer.cell((0, y)).expect("composer row start").bg,
            TUI_SURFACE_BG
        );
        assert_eq!(
            buffer.cell((47, y)).expect("composer row end").bg,
            TUI_SURFACE_BG
        );
    }
}

#[test]
fn thinking_new_paragraphs_do_not_use_label_width_indent() {
    let row = TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "First paragraph.\n\nSecond paragraph.",
    );
    let lines = thinking_lines(&row, false, true);

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].content.as_ref(), "▌ ");
    assert_eq!(lines[0].spans[1].content.as_ref(), "Thinking: ");
    assert_eq!(lines[0].spans[2].content.as_ref(), "First paragraph.");
    assert_eq!(lines[2].spans[0].content.as_ref(), "▌ ");
    assert_eq!(lines[2].spans[1].content.as_ref(), "Second paragraph.");
}

#[test]
fn bash_tool_title_uses_actual_first_command_line() {
    let title = tool_title(
        "bash",
        &serde_json::json!({
            "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
    );
    assert_eq!(title, "Ran cargo test -p psychevo-cli");
}

#[test]
fn fullscreen_bash_title_survives_tool_end_without_args() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_bash",
            "tool_name": "bash",
            "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_bash",
            "tool_name": "bash",
            "result": {"output": "ok", "exit_code": 0},
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("bash row");
    assert_eq!(row.title, "Ran cargo test -p psychevo-cli");
    assert_ne!(row.title, "Ran command");
}

#[test]
fn history_tool_result_reuses_persisted_bash_command_title() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "tool_call",
            "id": "call_bash",
            "name": "bash",
            "arguments": {
                "command": "find . -maxdepth 2\nprintf done"
            },
            "arguments_json": "{\"command\":\"find . -maxdepth 2\\nprintf done\"}",
            "arguments_error": null,
            "content_index": 0,
            "call_index": 0
        }],
        "timestamp_ms": 1,
        "finish_reason": "tool_calls",
        "outcome": "normal"
    });
    let tool_result = serde_json::json!({
        "role": "tool_result",
        "tool_call_id": "call_bash",
        "tool_name": "bash",
        "content": "{\"output\":\"ok\"}",
        "is_error": false,
        "timestamp_ms": 2
    });

    ui.push_history_message(&assistant, None, None);
    ui.push_history_message(&tool_result, None, None);

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Ran)
        .expect("history bash row");
    assert_eq!(row.title, "Ran find . -maxdepth 2");
}

#[tokio::test]
async fn sidebar_toggle_persists_visibility() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    assert!(!ui.sidebar_enabled());

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
    )
    .await
    .expect("show sidebar");
    assert!(ui.sidebar_enabled());
    let loaded = TuiState::load(&app.state_path).expect("load visible state");
    assert!(loaded.sidebar_visible);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
    )
    .await
    .expect("hide sidebar");
    assert!(!ui.sidebar_enabled());
    let loaded = TuiState::load(&app.state_path).expect("load hidden state");
    assert!(!loaded.sidebar_visible);
}

#[tokio::test]
async fn fullscreen_rename_updates_session_title_and_sidebar() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "model", "provider", None)
        .expect("session");
    app.current_session = Some(session_id.clone());
    app.current_session_title = None;
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Rename("  Better\nSession   Title  ".to_string()),
    )
    .await
    .expect("rename");

    assert_eq!(
        app.current_session_title.as_deref(),
        Some("Better Session Title")
    );
    assert_eq!(ui.sidebar.title, "Better Session Title");
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Better Session Title"));
}

#[tokio::test]
async fn removed_thinking_command_renders_bounded_error_in_fullscreen() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/thinking");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert!(
        ui.transcript.iter().any(|row| {
            row.kind == TranscriptKind::Error && row.text.contains("/show-thinking")
        })
    );
}

#[test]
fn composer_history_recall_preserves_draft() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["first".to_string(), "second".to_string()];
    ui.textarea = textarea_with_text("draft");

    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "second");
    assert!(
        !ui.textarea
            .cursor_line_style()
            .has_modifier(Modifier::UNDERLINED)
    );
    ui.recall_history(-1);
    assert_eq!(textarea_text(&ui.textarea), "first");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "second");
    ui.recall_history(1);
    assert_eq!(textarea_text(&ui.textarea), "draft");
    assert_eq!(ui.history_index, None);
}

#[test]
fn composer_history_recall_respects_multiline_boundaries() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.history = vec!["older".to_string()];
    ui.textarea = textarea_with_text("line one\nline two");

    assert!(!ui.can_recall_history_previous());
    assert!(!ui.can_recall_history_next());
    ui.textarea.move_cursor(CursorMove::Top);
    assert!(ui.can_recall_history_previous());
}

#[test]
fn tool_only_thinking_message_does_not_create_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default"
        }),
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: "thinking only".to_string(),
        },
        true,
        false,
    );
    ui.apply_value_event(
            &serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [
                        { "type": "tool_call", "id": "call_1", "name": "read", "arguments": { "path": "file.txt" } }
                    ]
                }
            }),
            false,
        );

    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta)
    );
}

#[test]
fn tool_failure_without_answer_keeps_failure_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "bash",
            "tool_call_id": "call_1",
            "outcome": "failed",
            "result": { "error": "boom" }
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Meta && row.text.contains("1 failure") })
    );
}

#[test]
fn selection_extracts_text_from_registered_screen_lines() {
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

#[test]
fn selection_uses_rendered_wrapped_transcript_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
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

#[test]
fn selection_preserves_wide_characters_from_rendered_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("中文测试abc".to_string());

    draw_fullscreen_for_test(&app, &mut ui, 24, 8);
    ui.start_selection(2, 0);
    ui.update_selection(10, 0);

    assert_eq!(ui.screen_lines[0].text(), "› 中文测试abc");
    assert_eq!(ui.selected_text().as_deref(), Some("中文测试"));
}

#[test]
fn selection_can_copy_sidebar_rendered_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    draw_fullscreen_for_test(&app, &mut ui, 120, 10);

    let line = ui
        .screen_lines
        .iter()
        .find(|line| line.text() == "Context")
        .expect("sidebar context line");
    let (x, y) = (line.first_x(), line.y);
    ui.start_selection(x, y);
    ui.update_selection(x + 7, y);

    assert_eq!(ui.selected_text().as_deref(), Some("Context"));
}

#[test]
fn sidebar_omits_source_mode_and_footer_chrome() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_mode = RunMode::Plan;
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    ui.refresh_sidebar(&app);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 18);
    let text = buffer_text(&buffer);

    assert!(text.contains("Review sidebar polish"));
    assert!(text.contains("Context"));
    assert!(text.contains("Modified Files"));
    for omitted in ["source: tui", "mode: plan", "Footer", "local facts only"] {
        assert!(
            !text.contains(omitted),
            "sidebar should omit {omitted:?}:\n{text}"
        );
    }
}

#[test]
fn multiline_transcript_selection_ignores_same_row_sidebar_text() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
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
    assert_ne!(
        buffer
            .cell((sidebar_row.0, sidebar_row.1))
            .expect("sidebar cell")
            .bg,
        TUI_SELECTION_BG
    );
}

#[tokio::test]
async fn active_selection_highlights_rendered_buffer_and_esc_clears() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("copy me".to_string());
    ui.start_selection(2, 0);
    ui.update_selection(6, 0);

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    assert_eq!(
        buffer.cell((2, 0)).expect("highlight start").bg,
        TUI_SELECTION_BG
    );
    assert_eq!(
        buffer.cell((5, 0)).expect("highlight end").bg,
        TUI_SELECTION_BG
    );
    assert_ne!(
        buffer.cell((6, 0)).expect("outside highlight").bg,
        TUI_SELECTION_BG
    );

    let should_quit = app
        .handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc");

    assert!(!should_quit);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 32, 8);
    assert_ne!(buffer.cell((2, 0)).expect("cleared").bg, TUI_SELECTION_BG);
}

#[test]
fn osc52_sequence_encodes_clipboard_text() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(
        osc52_sequence_with_passthrough("hello", false).expect("osc52"),
        "\x1b]52;c;aGVsbG8=\x07"
    );
}

#[test]
fn osc52_sequence_rejects_oversized_clipboard_payload() {
    let text = "x".repeat(100_001);

    assert!(osc52_sequence_with_passthrough(&text, false).is_err());
}

#[test]
fn wsl_clipboard_detection_uses_kernel_markers_without_env() {
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

#[test]
fn wsl_clipboard_candidates_try_powershell_then_clip_exe() {
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

#[test]
fn linux_wayland_clipboard_candidates_try_wl_copy_before_x11() {
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

#[test]
fn linux_x11_clipboard_candidates_fall_back_to_xclip_and_xsel() {
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

#[test]
fn clipboard_backend_reports_failure_when_all_backends_fail() {
    let candidates = local_clipboard_commands_for(false, false, true, false);
    let mut tried = Vec::new();

    let result = copy_text_to_clipboard_with(
        "hello",
        candidates,
        |candidate, _| {
            tried.push(candidate.command);
            Ok(false)
        },
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
async fn mouse_drag_copies_selected_text_through_clipboard_sink() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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

    assert_eq!(copied.lock().expect("clipboard lock").as_slice(), ["copy"]);
    assert_eq!(ui.selection, SelectionState::default());
}

#[tokio::test]
async fn mouse_up_clipboard_failure_clears_selection_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Error && row.text.contains("copy failed: blocked")
    }));
}

#[tokio::test]
async fn ctrl_c_copies_active_selection_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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
async fn clipboard_failure_during_ctrl_c_is_consumed_without_quitting() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
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

fn draw_fullscreen_for_test(
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

fn test_app(temp: &tempfile::TempDir) -> TuiApp {
    let home = temp.path().join("home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workdir).expect("workdir");
    let workdir = workdir.canonicalize().expect("canonical");
    TuiApp {
        env_map: BTreeMap::new(),
        home: home.clone(),
        state_path: home.join("tui-state.json"),
        state: TuiState::default(),
        db_path: home.join("state.db"),
        config_path: None,
        workdir: workdir.clone(),
        workdir_key: workdir.display().to_string(),
        current_session: Some("1234567890abcdef".to_string()),
        current_session_title: Some("Review sidebar polish".to_string()),
        force_new_once: false,
        current_model: Some("mock/model".to_string()),
        current_variant: Some("high".to_string()),
        selected_model: None,
        current_mode: RunMode::Build,
        thinking_visible: true,
        clipboard: Arc::new(|_| Ok(())),
        renderer: TuiRenderer::new(false),
        debug: false,
        had_error: false,
        model_catalog: ModelCatalogCache::default(),
    }
}

#[derive(Debug, Clone, Copy)]
enum FixtureKind {
    Idle,
    RunningThinking,
    CollapsedTool,
    ExpandedTool,
    DebugMeta,
    FailureMeta,
}

fn fixture_ui<'a>(app: &TuiApp, kind: FixtureKind) -> FullscreenUi<'a> {
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
            ui.transcript.push(TranscriptRow::with_title(
                TranscriptKind::Explored,
                "Explored crates/psychevo-cli/src/tui.rs",
                "running",
            ));
        }
        FixtureKind::CollapsedTool | FixtureKind::ExpandedTool => {
            ui.transcript.clear();
            push_completed_turn(&mut ui, kind);
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

fn stable_sidebar() -> SidebarSnapshot {
    SidebarSnapshot {
        title: "Review sidebar polish".to_string(),
        session: "12345678".to_string(),
        workdir: "/repo/psychevo".to_string(),
        branch: "main".to_string(),
        tokens: Some(12_000),
        context_percent: Some(18.8),
        message_count: 2,
        tool_count: 1,
        changed_files: vec![
            "M crates/psychevo-cli/src/tui.rs".to_string(),
            "?? specs/210-pevo-tui/testing.md".to_string(),
        ],
    }
}

fn stable_session_bottom_panel() -> BottomSelectionPanel {
    BottomSelectionPanel::new(
        "Sessions",
        "Search local run and TUI sessions.",
        "No sessions",
        vec![
            BottomSelectionRow {
                label: "Implement model picker".to_string(),
                description: Some("mock/mock-model  messages=5".to_string()),
                detail: Some("12:10".to_string()),
                group: Some("2026-05-06".to_string()),
                search_text: "session-a Implement model picker mock mock-model tui".to_string(),
                is_current: true,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-a".to_string()),
            },
            BottomSelectionRow {
                label: "Review session pane".to_string(),
                description: Some("mock/other-model  messages=3".to_string()),
                detail: Some("09:44".to_string()),
                group: Some("2026-05-05".to_string()),
                search_text: "session-b Review session pane mock other-model run".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: None,
                value: BottomSelectionValue::Session("session-b".to_string()),
            },
        ],
    )
}

fn push_completed_turn(ui: &mut FullscreenUi<'_>, kind: FixtureKind) {
    ui.push_user("Summarize the TUI snapshot harness.".to_string());
    ui.transcript.push(TranscriptRow::with_title(
        TranscriptKind::Thinking,
        "Thinking",
        "Check layout boundaries, style roles, and expandable evidence.",
    ));
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Explored,
        "Explored crates/psychevo-cli/src/tui.rs",
        long_tool_output()
            .lines()
            .take(collapsed_fixture_lines(kind))
            .collect::<Vec<_>>()
            .join("\n")
            + &format!("\n... {} more lines", 24 - collapsed_fixture_lines(kind)),
    );
    row.full_text = Some(long_tool_output());
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
            failures: 0,
            debug,
        }),
    ));
}

fn push_failure_turn(ui: &mut FullscreenUi<'_>) {
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

fn long_tool_output() -> String {
    (1..=24)
        .map(|line| format!("{line:02}: crates/psychevo-cli/src/tui.rs evidence row"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapsed_fixture_lines(kind: FixtureKind) -> usize {
    match kind {
        FixtureKind::ExpandedTool => 20,
        _ => 4,
    }
}

fn assert_tui_snapshot(
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

fn write_snapshot_diagnostics(name: &str, text: &str, styles: &str, combined: &str) {
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

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
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

fn buffer_style_text(buffer: &ratatui::buffer::Buffer) -> String {
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

fn style_marker(color: Color) -> &'static str {
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
