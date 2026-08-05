use self::fixtures::test_app;
use super::{
    Line, StartThreadRequest, ThreadModelSelection, ThreadSummary, TuiApp, TurnEvent, Value,
};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) fn runtime_turn_event(data: Value) -> TurnEvent {
    TurnEvent::Runtime { data }
}

pub(crate) fn reasoning_completed_turn_event() -> TurnEvent {
    TurnEvent::ReasoningCompleted { text: None }
}

pub(crate) fn scoped_turn_event(thread_id: impl Into<String>, event: TurnEvent) -> TurnEvent {
    TurnEvent::Scoped {
        thread_id: thread_id.into(),
        turn_id: "test-turn".to_string(),
        event: Box::new(event),
    }
}

pub(crate) fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

pub(crate) fn summary(id: &str) -> ThreadSummary {
    ThreadSummary {
        id: id.to_string(),
        source: "tui".to_string(),
        parent_thread_id: None,
        cwd: "/repo".to_string(),
        model: "model".to_string(),
        provider: "provider".to_string(),
        started_at_ms: 1,
        updated_at_ms: 1,
        ended_at_ms: None,
        end_reason: None,
        archived_at_ms: None,
        forked_from_thread_id: None,
        archived: false,
        message_count: 0,
        tool_call_count: 0,
        active_turn_id: None,
        title: None,
    }
}

pub(crate) struct TuiCatalogServer {
    pub(crate) base_url: String,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
}

impl TuiCatalogServer {
    pub(crate) fn new(body: &'static str) -> Self {
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

pub(crate) fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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

pub(crate) fn insert_tui_message(
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

pub(crate) fn insert_tui_message_with_metadata(
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

pub(crate) async fn start_thread_fixture(
    app: &TuiApp,
    cwd: &Path,
    source: &str,
    model: &str,
    provider: &str,
    metadata: Option<Value>,
) -> String {
    let mut request = StartThreadRequest::new(cwd);
    request.source = source.to_string();
    request.metadata = metadata;
    let thread = app
        .runtime
        .client()
        .start_thread(request)
        .await
        .expect("test Thread");
    thread
        .set_model_selection(ThreadModelSelection {
            provider: provider.to_string(),
            model: model.to_string(),
            reasoning_effort: None,
        })
        .await
        .expect("test Thread model selection");
    thread.id().to_string()
}

pub(crate) async fn materialize_current_thread_fixture(app: &mut TuiApp) -> String {
    let mut request = StartThreadRequest::new(app.cwd.clone());
    request.source = "tui".to_string();
    let thread = app
        .runtime
        .client()
        .start_thread(request)
        .await
        .expect("current test Thread");
    let thread_id = thread.id().to_string();
    app.current_session = Some(thread_id.clone());
    thread_id
}

pub(crate) fn test_track_snapshot(app: &TuiApp, _session_id: &str) -> String {
    let workspace_id = psychevo::paths::workspace_snapshot_id(&app.cwd).expect("workspace id");
    let git_dir = app
        .home
        .join("snapshots")
        .join("workspaces")
        .join(workspace_id);
    fs::create_dir_all(&git_dir).expect("snapshot dir");
    if !git_dir.join("HEAD").exists() {
        assert!(
            std::process::Command::new("git")
                .env("GIT_DIR", &git_dir)
                .env("GIT_WORK_TREE", &app.cwd)
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
            .arg(&app.cwd)
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
        .arg(&app.cwd)
        .arg("write-tree")
        .output()
        .expect("snapshot tree");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(crate) fn write_tui_model_config(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("model-config.toml");
    fs::write(
        &path,
        r#"model = "mock/mock-model"

[provider.mock]
api = "http://127.0.0.1:9"

[provider.mock.models."mock-model"]
reasoning = true
tool_call = true
structured_output = true

[provider.mock.models."mock-model".limit]
context = 128000
input = 120000
output = 16000

[provider.mock.models."mock-model".modalities]
input = ["text", "image"]
output = ["text"]

[provider.mock.models."mock-model".cost]
input = 1.5
output = 2.5
cache_read = 0.15
cache_write = 0.75

[provider.mock.models."mock-model".cost.context_over_200k]
input = 3.0
output = 5.0

[provider.mock.models."other-model"]
reasoning_effort = "high"
"#,
    )
    .expect("config");
    path
}

pub(crate) fn install_tui_test_config(app: &mut TuiApp, config_path: &Path) {
    let local_config_dir = app.cwd.join(".psychevo");
    std::fs::create_dir_all(&local_config_dir).expect("local config dir");
    std::fs::copy(config_path, local_config_dir.join("config.toml")).expect("local config");
    app.config_path = Some(config_path.to_path_buf());
}

pub(crate) async fn test_app_with_models(temp: &tempfile::TempDir) -> TuiApp {
    let mut app = test_app(temp).await;
    app.env_map
        .insert("MOCK_API_KEY".to_string(), "test-key".to_string());
    let config_path = write_tui_model_config(temp);
    install_tui_test_config(&mut app, &config_path);
    app.current_model = Some("mock/mock-model".to_string());
    app.current_variant = None;
    app.refresh_selected_model();
    app
}

// Test scenarios are ordinary modules; shared fixtures stay in their owning module.
pub(crate) mod adaptive_rendering;
pub(crate) mod agents_panel;
pub(crate) mod clarify;
pub(crate) mod commands;
pub(crate) mod core;
pub(crate) mod fixtures;
pub(crate) mod input_popups;
pub(crate) mod models;
pub(crate) mod rendering_history;
pub(crate) mod runtime_sessions;
pub(crate) mod selection_clipboard;
pub(crate) mod shell_history;
pub(crate) mod snapshots;
pub(crate) mod transcript_files;
