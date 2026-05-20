use super::*;
use psychevo_runtime::{ContextCategory, ContextScope, ContextTokenizer, ContextTotal};
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Position;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
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
        archived_at_ms: None,
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
                    "mock-model": {
                      "limit": { "context": 128000, "input": 120000, "output": 16000 },
                      "reasoning": true,
                      "tool_call": true,
                      "structured_output": true,
                      "modalities": { "input": ["text", "image"], "output": ["text"] },
                      "cost": {
                        "input": 1.5,
                        "output": 2.5,
                        "cache_read": 0.15,
                        "cache_write": 0.75,
                        "context_over_200k": { "input": 3.0, "output": 5.0 },
                        "source": "config"
                      }
                    },
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

// Test chunks stay in this module so existing helpers remain shared.
include!("core.rs");
include!("clarify.rs");
include!("snapshots.rs");
include!("transcript_files.rs");
include!("input_popups.rs");
include!("agents_panel.rs");
include!("commands.rs");
include!("models.rs");
include!("runtime_sessions.rs");
include!("rendering_history.rs");
include!("shell_history.rs");
include!("selection_clipboard.rs");
include!("adaptive_rendering.rs");
include!("fixtures.rs");
