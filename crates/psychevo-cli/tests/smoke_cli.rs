use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use pretty_assertions::assert_eq;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

fn pevo() -> &'static str {
    env!("CARGO_BIN_EXE_pevo")
}

fn pevo_cmd(home: &Path) -> Command {
    let mut command = Command::new(pevo());
    command.env_clear().env("HOME", home);
    command
}

fn isolated_run_cmd(home: &Path, config: &Path, db: &Path) -> Command {
    let mut command = pevo_cmd(home);
    command
        .env("PSYCHEVO_CONFIG", config)
        .env("PSYCHEVO_DB", db);
    command
}

fn isolated_tui_cmd(home: &Path, psychevo_home: &Path, config: &Path, db: &Path) -> Command {
    let mut command = isolated_run_cmd(home, config, db);
    command.env("PSYCHEVO_HOME", psychevo_home);
    command
}

fn init_tui_home(test_home: &Path) -> PathBuf {
    let psychevo_home = test_home.join("psychevo-home");
    let output = pevo_cmd(test_home)
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    psychevo_home
}

struct MockSseServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockSseServer {
    fn start(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            while let Some(body) = responses.pop_front() {
                let (mut stream, _) = listener.accept().expect("accept");
                let request = read_http_request(&mut stream);
                requests_for_thread.lock().expect("requests").push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).expect("response");
            }
        });
        Self { base_url, requests }
    }

    fn request_json(&self, index: usize) -> Value {
        let requests = self.requests.lock().expect("requests");
        let request = requests.get(index).expect("request");
        let body = request.split("\r\n\r\n").nth(1).expect("body");
        serde_json::from_str(body).expect("request json")
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).expect("request");
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let header_end = data
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap_or(data.len());
    let headers = String::from_utf8_lossy(&data[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    while data.len().saturating_sub(header_end) < content_length {
        let n = stream.read(&mut buf).expect("body");
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&data).to_string()
}

fn sse_text(text: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(text).expect("text")
    )
}

fn sse_reasoning_then_text(reasoning: &str, text: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":{}}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(reasoning).expect("reasoning"),
        serde_json::to_string(text).expect("text")
    )
}

fn sse_metadata_usage_then_text(text: &str) -> String {
    format!(
        "data: {{\"id\":\"resp_1\",\"model\":\"mock-model\",\"choices\":[],\"usage\":{{\"prompt_tokens\":3,\"completion_tokens\":4,\"total_tokens\":7}}}}\n\n\
         data: {{\"id\":\"resp_1\",\"model\":\"mock-model\",\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(text).expect("text")
    )
}

fn sse_tool_read_then_done() -> String {
    concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_read\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"fixture.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn write_run_config(dir: &Path, base_url: &str) -> PathBuf {
    write_run_config_with_reasoning(dir, base_url, None)
}

fn write_run_config_with_reasoning(
    dir: &Path,
    base_url: &str,
    reasoning_effort: Option<&str>,
) -> PathBuf {
    std::fs::create_dir_all(dir).expect("config dir");
    std::fs::write(dir.join(".env"), "TEST_PROVIDER_KEY=test-key\n").expect("env");
    let reasoning = reasoning_effort
        .map(|value| format!(r#""reasoning_effort": "{value}""#))
        .unwrap_or_default();
    let config = format!(
        r#"{{
          "model": "mock/mock-model",
          "provider": {{
            "mock": {{
              "options": {{
                "base_url": "{base_url}",
                "api_key_env": "TEST_PROVIDER_KEY"
              }},
              "models": {{
                "mock-model": {{{reasoning}}}
              }}
            }}
          }}
        }}"#
    );
    let path = dir.join("config.jsonc");
    std::fs::write(&path, config).expect("config");
    path
}

fn write_multi_model_config(dir: &Path, base_url: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("config dir");
    std::fs::write(dir.join(".env"), "TEST_PROVIDER_KEY=test-key\n").expect("env");
    let config = format!(
        r#"{{
          "model": "mock/mock-model",
          "provider": {{
            "mock": {{
              "options": {{
                "base_url": "{base_url}",
                "api_key_env": "TEST_PROVIDER_KEY"
              }},
              "models": {{
                "mock-model": {{}},
                "other-model": {{ "reasoning_effort": "high" }}
              }}
            }}
          }}
        }}"#
    );
    let path = dir.join("config.jsonc");
    std::fs::write(&path, config).expect("config");
    path
}

fn user_contents(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message["content"].as_str().expect("content").to_string())
        .collect()
}

#[test]
fn cli_init_creates_home_tree_and_is_idempotent() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains(&format!("home: {}", home.display())));
    assert!(home.join("config.jsonc").exists());
    assert!(home.join(".env").exists());
    assert!(home.join("state.db").exists());
    assert!(home.join("sessions").is_dir());
    assert!(home.join("logs").is_dir());
    assert!(home.join("cache").is_dir());

    let config = std::fs::read_to_string(home.join("config.jsonc")).expect("config");
    assert!(config.contains("\"model\": \"deepseek/deepseek-chat\""));
    assert!(config.contains("\"api_key_env\": \"DEEPSEEK_API_KEY\""));
    let env_template = std::fs::read_to_string(home.join(".env")).expect("env");
    assert!(env_template.contains("DEEPSEEK_API_KEY=sk-..."));
    assert!(!stdout.contains("sk-"));

    let conn = Connection::open(home.join("state.db")).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 3);

    std::fs::write(home.join("config.jsonc"), "custom config").expect("custom config");
    std::fs::write(home.join(".env"), "CUSTOM=1\n").expect("custom env");
    let rerun = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init rerun");
    assert!(rerun.status.success());
    assert_eq!(
        std::fs::read_to_string(home.join("config.jsonc")).expect("config"),
        "custom config"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".env")).expect("env"),
        "CUSTOM=1\n"
    );
}

#[test]
fn cli_init_reset_state_backs_up_existing_sqlite_files() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(init.status.success());
    std::fs::write(home.join("state.db-wal"), "wal").expect("wal");
    std::fs::write(home.join("state.db-shm"), "shm").expect("shm");

    let reset = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .args(["init", "--reset-state"])
        .output()
        .expect("pevo init reset");
    assert!(
        reset.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reset.stderr)
    );
    let stdout = String::from_utf8(reset.stdout).expect("stdout");
    assert!(stdout.contains("state_backup:"));
    assert!(home.join("state.db").exists());
    assert!(!home.join("state.db-wal").exists());
    assert!(!home.join("state.db-shm").exists());

    let backup_root = home.join("backups");
    let backups = std::fs::read_dir(&backup_root)
        .expect("backups")
        .collect::<std::io::Result<Vec<_>>>()
        .expect("backup entries");
    assert_eq!(backups.len(), 1);
    let backup = backups[0].path();
    assert!(backup.join("state.db").exists());
    assert_eq!(
        std::fs::read_to_string(backup.join("state.db-wal")).expect("wal"),
        "wal"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("state.db-shm")).expect("shm"),
        "shm"
    );

    let conn = Connection::open(home.join("state.db")).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 3);
}

#[test]
fn cli_smoke_preserves_deterministic_harness_flags() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let output = pevo_cmd(temp.path())
        .args([
            "smoke",
            "--db",
            db.to_str().expect("db"),
            "--workdir",
            workdir.to_str().expect("workdir"),
            "--prompt",
            "read write edit bash",
        ])
        .output()
        .expect("pevo smoke");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("outcome: normal"));
    assert_eq!(
        std::fs::read_to_string(workdir.join(".psychevo-smoke/generated.txt")).expect("generated"),
        "written by psychevo smoke\n"
    );
}

#[test]
fn cli_run_positional_prompt_outputs_final_answer_and_persists_metadata() {
    let server = MockSseServer::start(vec![sse_text("mock final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
            "world",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "mock final\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let request = server.request_json(0);
    assert_eq!(request["model"], "mock-model");
    assert_eq!(user_contents(&request), vec!["hello world"]);

    let conn = Connection::open(db).expect("db");
    let (source, provider, model): (String, String, String) = conn
        .query_row("SELECT source, provider, model FROM sessions", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("session");
    assert_eq!(source, "run");
    assert_eq!(provider, "mock");
    assert_eq!(model, "mock-model");
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM messages WHERE role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("metadata");
    let metadata: Value = serde_json::from_str(&metadata_json).expect("metadata json");
    assert!(metadata["elapsed_ms"].as_u64().is_some());
}

#[test]
fn cli_run_dir_controls_tool_workdir() {
    let server = MockSseServer::start(vec![sse_tool_read_then_done(), sse_text("read complete")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(workdir.join("fixture.txt"), "fixture content\n").expect("fixture");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "read",
            "fixture.txt",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "read complete\n"
    );

    let conn = Connection::open(db).expect("db");
    let read_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'read' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("read results");
    assert_eq!(read_results, 1);
}

#[test]
fn cli_run_json_outputs_ndjson_events() {
    let server = MockSseServer::start(vec![sse_text("json final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert_eq!(events.first().expect("first")["type"], "run_start");
    assert!(events.iter().any(|event| event["type"] == "agent_start"));
    assert!(events.iter().any(|event| event["type"] == "message_end"));
    assert!(events.iter().any(|event| event["type"] == "agent_end"));
    assert!(!stdout.contains("json final\njson final"));
}

#[test]
fn cli_run_json_hides_reasoning_by_default_and_debug_flag_emits_it() {
    let server = MockSseServer::start(vec![
        sse_reasoning_then_text("private chain", "visible final"),
        sse_reasoning_then_text("debug chain", "debug final"),
    ]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let hidden = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run hidden");
    assert!(
        hidden.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hidden.stderr)
    );
    let hidden_stdout = String::from_utf8(hidden.stdout).expect("stdout");
    assert!(hidden_stdout.contains("visible final"));
    assert!(!hidden_stdout.contains("private chain"));
    assert!(!hidden_stdout.contains("reasoning_content"));

    let shown = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "--include-reasoning",
            "hello",
        ])
        .output()
        .expect("pevo run shown");
    assert!(
        shown.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let shown_stdout = String::from_utf8(shown.stdout).expect("stdout");
    let events = shown_stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "reasoning_delta" && event["text"] == "debug chain" })
    );
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "reasoning_end" && event["text"] == "debug chain" })
    );
    assert!(shown_stdout.contains("debug final"));
}

#[test]
fn cli_run_include_reasoning_requires_json_format() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--include-reasoning",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--format json"));
}

#[test]
fn cli_run_json_omits_metadata_only_message_updates() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("metadata final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    let empty_updates = events
        .iter()
        .filter(|event| {
            event["type"] == "message_update"
                && event["message"]["role"] == "assistant"
                && event["message"]["content"]
                    .as_array()
                    .is_some_and(|content| content.is_empty())
        })
        .count();
    assert_eq!(empty_updates, 0);
}

#[test]
fn cli_run_reads_stdin_and_appends_to_positional_prompt() {
    let server = MockSseServer::start(vec![sse_text("stdin final"), sse_text("append final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut stdin_only = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    stdin_only
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"from stdin\n")
        .expect("write stdin");
    let output = stdin_only.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut appended = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "fix",
            "this",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    appended
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"details\n")
        .expect("write stdin");
    let output = appended.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(user_contents(&server.request_json(0)), vec!["from stdin\n"]);
    assert_eq!(
        user_contents(&server.request_json(1)),
        vec!["fix this\ndetails\n"]
    );
}

#[test]
fn cli_run_empty_prompt_rejects_before_session_creation() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");
    let mut child = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"   \n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("You must provide a message"));
    assert!(!db.exists());
}

#[test]
fn cli_run_errors_use_selected_output_format() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config_dir = temp.path().join("config");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    let config = config_dir.join("config.jsonc");
    std::fs::write(
        &config,
        r#"{
          "model": "custom/local",
          "provider": {
            "custom": {
              "options": {
                "base_url": "https://example.invalid/v1",
                "api_key_env": "PSYCHEVO_TEST_MISSING_KEY_SHOULD_NOT_EXIST"
              },
              "models": { "local": {} }
            }
          }
        }"#,
    )
    .expect("config");

    let default_output = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo run");
    assert!(!default_output.status.success());
    assert!(String::from_utf8_lossy(&default_output.stderr).contains("requires credentials"));

    let json_output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--format",
            "json",
            "hello",
        ])
        .output()
        .expect("pevo run json");
    assert!(!json_output.status.success());
    assert!(String::from_utf8_lossy(&json_output.stderr).is_empty());
    let stdout = String::from_utf8(json_output.stdout).expect("stdout");
    let error = serde_json::from_str::<Value>(stdout.trim()).expect("error json");
    assert_eq!(error["type"], "error");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("requires credentials")
    );
}

#[test]
fn cli_run_requires_initialized_home_without_config_db_bypass() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .args(["run", "hello"])
        .output()
        .expect("pevo run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("pevo init"));
    assert!(!temp.path().join(".psychevo").exists());
}

#[test]
fn cli_run_rejects_removed_flags() {
    let temp = tempdir().expect("temp");
    let cases: &[&[&str]] = &[
        &["run", "--prompt", "hello"],
        &["run", "--json", "hello"],
        &["run", "--provider", "deepseek", "hello"],
        &["run", "--base-url", "http://127.0.0.1:9", "hello"],
        &["run", "--api-key-env", "KEY", "hello"],
        &["run", "--db", "state.db", "hello"],
        &["run", "--workdir", ".", "hello"],
        &["run", "--max-context-messages", "1", "hello"],
        &["run", "--verbose", "hello"],
        &["run", "--config", "config.jsonc", "hello"],
    ];
    for args in cases {
        let output = pevo_cmd(temp.path())
            .args(*args)
            .output()
            .expect("pevo run");
        assert!(!output.status.success(), "expected failure for {args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(args[1]),
            "stderr did not mention rejected flag {args:?}: {stderr}"
        );
    }
}

#[test]
fn cli_run_model_override_requires_provider_qualified_model() {
    let server = MockSseServer::start(vec![sse_text("model final")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "-m",
            "mock/mock-model",
            "hello",
        ])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.request_json(0)["model"], "mock-model");

    let invalid = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "-m",
            "mock-model",
            "hello",
        ])
        .output()
        .expect("pevo run invalid");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("provider/model"));
}

#[test]
fn cli_run_variant_overrides_reasoning_effort_and_none_suppresses_it() {
    let high_server = MockSseServer::start(vec![sse_text("high")]);
    let temp = tempdir().expect("temp");
    let high_db = temp.path().join("high.db");
    let workdir = temp.path().join("work");
    let high_config = write_run_config(&temp.path().join("high-config"), &high_server.base_url);
    let high = isolated_run_cmd(temp.path(), &high_config, &high_db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--variant",
            "high",
            "hello",
        ])
        .output()
        .expect("pevo run high");
    assert!(high.status.success());
    assert_eq!(high_server.request_json(0)["reasoning_effort"], "high");

    let none_server = MockSseServer::start(vec![sse_text("none")]);
    let none_db = temp.path().join("none.db");
    let none_config = write_run_config_with_reasoning(
        &temp.path().join("none-config"),
        &none_server.base_url,
        Some("high"),
    );
    let none = isolated_run_cmd(temp.path(), &none_config, &none_db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--variant",
            "none",
            "hello",
        ])
        .output()
        .expect("pevo run none");
    assert!(none.status.success());
    assert!(
        none_server
            .request_json(0)
            .get("reasoning_effort")
            .is_none()
    );
}

#[test]
fn cli_run_continue_reuses_latest_matching_run_session() {
    let server = MockSseServer::start(vec![sse_text("first"), sse_text("second")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "first"])
        .output()
        .expect("first run");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "second",
        ])
        .output()
        .expect("second run");
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = 'run'",
            [],
            |row| row.get(0),
        )
        .expect("sessions");
    assert_eq!(sessions, 1);
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("messages");
    assert_eq!(messages, 4);

    let conflict = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "--session",
            "session-id",
            "hello",
        ])
        .output()
        .expect("conflict");
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("--continue"));
}

#[test]
fn cli_run_continue_ignores_smoke_sessions() {
    let server = MockSseServer::start(vec![sse_text("run")]);
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let smoke = pevo_cmd(temp.path())
        .args([
            "smoke",
            "--db",
            db.to_str().expect("db"),
            "--workdir",
            workdir.to_str().expect("workdir"),
        ])
        .output()
        .expect("smoke");
    assert!(smoke.status.success());

    let config = write_run_config(&temp.path().join("config"), &server.base_url);
    let run = isolated_run_cmd(temp.path(), &config, &db)
        .args([
            "run",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--continue",
            "hello",
        ])
        .output()
        .expect("run");
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 2);
}

#[test]
fn cli_tui_initial_prompt_shows_thinking_by_default() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text(
        "private chain",
        "visible tui",
    )]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("Thinking:"));
    assert!(stdout.contains("private chain"));
    assert!(stdout.contains("visible tui"));
}

#[test]
fn cli_tui_debug_shows_usage_metadata_summary() {
    let server = MockSseServer::start(vec![sse_metadata_usage_then_text("debug metrics")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let output = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--debug",
            "--dir",
            workdir.to_str().expect("workdir"),
            "hello",
        ])
        .output()
        .expect("pevo tui");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("Answer:"));
    assert!(stdout.contains("debug metrics"));
    assert!(stdout.contains("Meta:"));
    assert!(stdout.contains("usage 3 input 4 output"));
    assert!(stdout.contains("response resp_1"));
    assert!(!stdout.contains("total_tokens="));
    assert!(!stdout.contains("provider_response_id="));
}

#[test]
fn cli_tui_thinking_toggle_hides_reasoning_and_persists() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("debug chain", "visible tui")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/show-thinking off\nhello\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("thinking: off"));
    assert!(!stdout.contains("Thinking: hidden"));
    assert!(!stdout.contains("debug chain"));
    assert!(stdout.contains("visible tui"));

    let state = std::fs::read_to_string(home.join("tui-state.json")).expect("state");
    assert!(state.contains(r#""thinking_visible": false"#));
}

#[test]
fn cli_tui_status_shows_configured_default_variant() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config_with_reasoning(
        &temp.path().join("config"),
        "http://127.0.0.1:9",
        Some("xhigh"),
    );

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/status\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("model: mock/mock-model"));
    assert!(stdout.contains("variant: xhigh"));
    let expected_status = format!(
        "workdir: {}\nhome: {}\ndb: {}\nsession: (none)\nmodel: mock/mock-model\nvariant: xhigh\nmode: default\nthinking: on\ndebug: off",
        workdir.display(),
        home.display(),
        db.display()
    );
    assert!(
        stdout.contains(&expected_status),
        "stdout did not contain status block:\n{stdout}"
    );
}

#[test]
fn cli_tui_new_is_silent_until_next_prompt() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/new\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("new session will start on next prompt"));
}

#[test]
fn cli_tui_scripted_undo_and_redo_print_deterministic_status() {
    let server = MockSseServer::start(vec![sse_text("visible tui")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello\n/undo\n/redo\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("visible tui"));
    assert!(stdout.contains("undone 2 messages; prompt restored"));
    assert!(stdout.contains("redone 2 messages; complete"));
}

#[test]
fn cli_tui_mode_set_plan_persists_and_uses_read_only_tools() {
    let server = MockSseServer::start(vec![sse_text("planned")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/mode plan\nhello\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mode: plan"));
    assert!(stdout.contains("planned"));

    let request = server.request_json(0);
    let tool_names = request["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["function"]["name"].as_str().expect("tool").to_string())
        .collect::<Vec<_>>();
    assert_eq!(tool_names, vec!["read", "list", "search"]);
    assert_eq!(request["messages"][0]["role"], "system");
    assert!(
        request["messages"][0]["content"]
            .as_str()
            .expect("system")
            .contains("hard read-only")
    );

    let state = std::fs::read_to_string(home.join("tui-state.json")).expect("state");
    assert!(state.contains(r#""mode": "plan""#));

    let conn = Connection::open(&db).expect("db");
    let system_messages: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'system'",
            [],
            |row| row.get(0),
        )
        .expect("system messages");
    assert_eq!(system_messages, 0);
}

#[test]
fn cli_tui_model_lists_configured_entries_without_prompt() {
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_multi_model_config(&temp.path().join("config"), "http://127.0.0.1:9");

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/model\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(stdout.contains("mock/other-model variant=high"));
}

#[test]
fn cli_tui_continues_latest_run_or_tui_session_and_new_creates_tui_session() {
    let server = MockSseServer::start(vec![
        sse_text("first"),
        sse_text("second"),
        sse_text("third"),
    ]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let run = isolated_run_cmd(temp.path(), &config, &db)
        .args(["run", "--dir", workdir.to_str().expect("workdir"), "first"])
        .output()
        .expect("pevo run");
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let continued = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "second"])
        .output()
        .expect("pevo tui continue");
    assert!(
        continued.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&continued.stderr)
    );

    let conn = Connection::open(&db).expect("db");
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 1);
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("messages");
    assert_eq!(messages, 4);

    let new_session = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args([
            "tui",
            "--dir",
            workdir.to_str().expect("workdir"),
            "--new",
            "third",
        ])
        .output()
        .expect("pevo tui new");
    assert!(
        new_session.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&new_session.stderr)
    );

    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 2);
    let tui_sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = 'tui'",
            [],
            |row| row.get(0),
        )
        .expect("tui sessions");
    assert_eq!(tui_sessions, 1);
}

#[test]
fn cli_tui_sessions_lists_sessions_and_session_show_is_removed() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("hidden chain", "visible")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/sessions\n/session show latest\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("usage: /sessions, /resume, or /continue"));
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(!stdout.contains("hidden chain"));
}

#[test]
fn cli_tui_sessions_scripted_fallback_lists_sessions() {
    let server = MockSseServer::start(vec![sse_reasoning_then_text("hidden chain", "visible")]);
    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let config = write_run_config(&temp.path().join("config"), &server.base_url);

    let first = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir"), "hello"])
        .output()
        .expect("pevo tui");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let mut child = isolated_tui_cmd(temp.path(), &home, &config, &db)
        .args(["tui", "--dir", workdir.to_str().expect("workdir")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"/sessions\n/quit\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mock/mock-model"));
    assert!(!stdout.contains("hidden chain"));
}
