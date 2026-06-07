pub(crate) use super::*;
pub(crate) use psychevo_runtime::{ContextCategory, ContextScope, ContextTokenizer, ContextTotal};
pub(crate) use ratatui::backend::{Backend, TestBackend};
pub(crate) use ratatui::layout::Position;
pub(crate) use std::fs;
pub(crate) use std::io::{Read, Write};
pub(crate) use std::net::TcpListener;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};
pub(crate) use tempfile::tempdir;

pub(crate) fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

pub(crate) fn summary(id: &str) -> SessionSummary {
    SessionSummary {
        id: id.to_string(),
        source: "tui".to_string(),
        parent_session_id: None,
        workdir: "/repo".to_string(),
        model: "model".to_string(),
        provider: "provider".to_string(),
        started_at_ms: 1,
        updated_at_ms: 1,
        ended_at_ms: None,
        end_reason: None,
        archived_at_ms: None,
        message_count: 0,
        tool_call_count: 0,
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

pub(crate) fn test_track_snapshot(app: &TuiApp, session_id: &str) -> String {
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

pub(crate) fn write_tui_model_config(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("model-config.toml");
    fs::write(
        &path,
        r#"model = "mock/mock-model"

[provider.mock.options]
base_url = "http://127.0.0.1:9"
api_key_env = "TEST_PROVIDER_KEY"

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

pub(crate) fn test_app_with_models(temp: &tempfile::TempDir) -> TuiApp {
    let mut app = test_app(temp);
    app.env_map
        .insert("TEST_PROVIDER_KEY".to_string(), "test-key".to_string());
    let config_path = write_tui_model_config(temp);
    std::fs::create_dir_all(app.workdir.join(".psychevo")).expect("local config dir");
    std::fs::copy(&config_path, app.workdir.join(".psychevo/config.toml")).expect("local config");
    app.config_path = Some(config_path);
    app.current_model = Some("mock/mock-model".to_string());
    app.current_variant = None;
    app.refresh_selected_model();
    app
}

// Test chunks stay in this module so existing helpers remain shared.
#[path = "core.rs"]
pub(crate) mod core;
#[allow(unused_imports)]
use core::*;
#[path = "clarify.rs"]
pub(crate) mod clarify;
#[allow(unused_imports)]
use clarify::*;
#[path = "snapshots.rs"]
pub(crate) mod snapshots;
#[allow(unused_imports)]
use snapshots::*;
#[path = "transcript_files.rs"]
pub(crate) mod transcript_files;
#[allow(unused_imports)]
use transcript_files::*;
#[path = "input_popups.rs"]
pub(crate) mod input_popups;
#[allow(unused_imports)]
use input_popups::*;
#[path = "agents_panel.rs"]
pub(crate) mod agents_panel;
#[allow(unused_imports)]
use agents_panel::*;
#[path = "commands.rs"]
pub(crate) mod commands;
#[allow(unused_imports)]
use commands::*;
#[path = "models.rs"]
pub(crate) mod models;
#[allow(unused_imports)]
use models::*;
#[path = "runtime_sessions.rs"]
pub(crate) mod runtime_sessions;
#[allow(unused_imports)]
use runtime_sessions::*;
#[path = "rendering_history.rs"]
pub(crate) mod rendering_history;
#[allow(unused_imports)]
use rendering_history::*;
#[path = "shell_history.rs"]
pub(crate) mod shell_history;
#[allow(unused_imports)]
use shell_history::*;
#[path = "selection_clipboard.rs"]
pub(crate) mod selection_clipboard;
#[allow(unused_imports)]
use selection_clipboard::*;
#[path = "adaptive_rendering.rs"]
pub(crate) mod adaptive_rendering;
#[allow(unused_imports)]
use adaptive_rendering::*;
#[path = "fixtures.rs"]
pub(crate) mod fixtures;
#[allow(unused_imports)]
use fixtures::*;
