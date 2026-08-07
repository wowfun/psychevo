use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

pub(crate) fn pevo() -> &'static str {
    env!("CARGO_BIN_EXE_pevo")
}

pub(crate) fn pevo_cmd(home: &Path) -> Command {
    let home = psychevo::host_paths::normalized_native_path(home);
    let runtime_tmp = home.join("tmp");
    std::fs::create_dir_all(&runtime_tmp).expect("runtime temp");
    let mut command = Command::new(pevo());
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("TEMP", &runtime_tmp)
        .env("TMP", &runtime_tmp)
        .env("TMPDIR", &runtime_tmp);
    for key in [
        "PATH",
        "PATHEXT",
        "COMSPEC",
        "SystemRoot",
        "WINDIR",
        "PSYCHEVO_GIT_BASH_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
}

pub(crate) fn isolated_run_cmd(home: &Path, config: &Path, db: &Path) -> Command {
    seed_managed_rg(&home.join(".psychevo"));
    let mut command = pevo_cmd(home);
    command
        .env("PSYCHEVO_CONFIG", config)
        .env("PSYCHEVO_DB", db);
    command
}

pub(crate) fn isolated_tui_cmd(
    home: &Path,
    psychevo_home: &Path,
    config: &Path,
    db: &Path,
) -> Command {
    let mut command = isolated_run_cmd(home, config, db);
    command.env("PSYCHEVO_HOME", psychevo_home);
    command
}

pub(crate) fn init_tui_home(test_home: &Path) -> PathBuf {
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
    seed_managed_rg(&psychevo_home);
    psychevo_home
}

pub(crate) fn seed_managed_rg(psychevo_home: &Path) {
    let tools = psychevo_home.join("tools");
    std::fs::create_dir_all(&tools).expect("tools");
    let rg = tools.join(if cfg!(windows) { "rg.exe" } else { "rg" });
    std::fs::write(&rg, "#!/bin/sh\nprintf 'test rg\\n'\n").expect("rg");
    #[cfg(unix)]
    {
        pub(crate) use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&rg).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&rg, permissions).expect("chmod");
    }
}

pub(crate) struct MockSseServer {
    pub(crate) base_url: String,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
}

impl MockSseServer {
    pub(crate) fn start(responses: Vec<String>) -> Self {
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

    pub(crate) fn request_json(&self, index: usize) -> Value {
        let requests = self.requests.lock().expect("requests");
        let request = requests.get(index).expect("request");
        let body = request.split("\r\n\r\n").nth(1).expect("body");
        serde_json::from_str(body).expect("request json")
    }
}

pub(crate) fn long_tool_turn_smoke_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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

pub(crate) fn sse_text(text: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(text).expect("text")
    )
}

pub(crate) fn sse_reasoning_then_text(reasoning: &str, text: &str) -> String {
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

fn sse_tool_agent_call(call_id: &str, agent: &str, prompt: &str) -> String {
    let args = serde_json::json!({
        "agent_type": agent,
        "task_name": "translate_greeting",
        "message": prompt,
    })
    .to_string();
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":{},\"function\":{{\"name\":\"spawn_agent\",\"arguments\":{}}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(call_id).expect("call id"),
        serde_json::to_string(&args).expect("args")
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
    std::fs::write(dir.join(".env"), "MOCK_API_KEY=test-key\n").expect("env");
    let reasoning = reasoning_effort
        .map(|value| format!("reasoning_effort = \"{value}\"\n"))
        .unwrap_or_default();
    let config = format!(
        r#"model = "mock/mock-model"

[provider.mock]
api = "{base_url}"

[provider.mock.models."mock-model"]
{reasoning}"#
    );
    let path = dir.join("config.toml");
    std::fs::write(&path, config).expect("config");
    path
}

fn write_multi_model_config(dir: &Path, base_url: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("config dir");
    std::fs::write(dir.join(".env"), "MOCK_API_KEY=test-key\n").expect("env");
    let config = format!(
        r#"model = "mock/mock-model"

[provider.mock]
api = "{base_url}"

[provider.mock.models."mock-model"]

[provider.mock.models."other-model"]
reasoning_effort = "high"
"#
    );
    let path = dir.join("config.toml");
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

fn system_contents(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "system")
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

pub(crate) fn assert_starter_config_template(config: &str) {
    let parsed: toml::Value = toml::from_str(config).expect("starter config toml");
    assert_eq!(
        parsed.get("model").and_then(toml::Value::as_str),
        Some("deepseek/deepseek-chat")
    );
    assert_eq!(
        parsed
            .get("provider")
            .and_then(|provider| provider.get("deepseek"))
            .and_then(|deepseek| deepseek.get("api"))
            .and_then(toml::Value::as_str),
        Some("https://api.deepseek.com/v1")
    );
    assert_eq!(
        parsed
            .get("provider")
            .and_then(|provider| provider.get("deepseek"))
            .and_then(|deepseek| deepseek.get("models"))
            .and_then(|models| models.get("deepseek-chat"))
            .and_then(|model| model.get("reasoning_effort"))
            .and_then(toml::Value::as_str),
        Some("medium")
    );
    let alias = parsed
        .get("tui")
        .and_then(|tui| tui.get("slash_aliases"))
        .and_then(|aliases| aliases.get("/export -i lpr,last-provider-response -f json"))
        .and_then(toml::Value::as_array)
        .expect("default /expr alias");
    assert_eq!(
        alias
            .iter()
            .map(|value| value.as_str().expect("alias string"))
            .collect::<Vec<_>>(),
        vec!["/expr"]
    );
}

// Scenario modules import only the harness seams they exercise.
#[path = "smoke_cli/admin.rs"]
mod smoke_cli_admin;
#[path = "smoke_cli/agent.rs"]
mod smoke_cli_agent;
#[path = "smoke_cli/extensions.rs"]
mod smoke_cli_extensions;
#[path = "smoke_cli/hooks.rs"]
mod smoke_cli_hooks;
#[path = "smoke_cli/init.rs"]
mod smoke_cli_init;
#[path = "smoke_cli/install.rs"]
mod smoke_cli_install;
#[path = "smoke_cli/plugins.rs"]
mod smoke_cli_plugins;
#[path = "smoke_cli/profile.rs"]
mod smoke_cli_profile;
#[path = "smoke_cli/run.rs"]
mod smoke_cli_run;
#[path = "smoke_cli/skills.rs"]
mod smoke_cli_skills;
#[path = "smoke_cli/tui.rs"]
mod smoke_cli_tui;
