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
    sse_tool_read_call("call_read")
}

fn sse_tool_read_call(call_id: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":{},\"function\":{{\"name\":\"read\",\"arguments\":\"{{\\\"path\\\":\\\"fixture.txt\\\"}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(call_id).expect("call id")
    )
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

fn write_home_skill(home: &Path, name: &str, description: &str, body: &str) {
    let dir = home.join(".psychevo").join("skills").join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description:?}\n---\n\n{body}\n"),
    )
    .expect("skill");
}

// Scenario chunks share this integration-test harness.
include!("smoke_cli/init.rs");
include!("smoke_cli/run.rs");
include!("smoke_cli/tui.rs");
include!("smoke_cli/skills.rs");
include!("smoke_cli/install.rs");
include!("smoke_cli/admin.rs");
