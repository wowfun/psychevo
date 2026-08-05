use crate::{
    state::StateRuntime,
    types::{RunMode, RunOptions},
};
use psychevo_agent_core::ToolBinding;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub(crate) async fn base_options(temp: &tempfile::TempDir) -> RunOptions {
    seed_managed_rg(&home_dir(temp));
    RunOptions {
        state: StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state runtime"),
        cwd: temp.path().join("work"),
        snapshot_root: Some(temp.path().join("snapshots")),
        session: None,
        continue_latest: false,
        prompt: "hello".to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: None,
        project_context_override: None,
        sandbox_override: None,
        model: None,
        reasoning_effort: None,
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        workspace_mutations: None,
        runtime_tools: Vec::new(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home_dir(temp).to_string_lossy().to_string(),
            ),
        ])),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_runtime: None,
    }
}

pub(crate) fn home_dir(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join(".psychevo")
}

pub(crate) fn seed_managed_rg(psychevo_home: &std::path::Path) {
    let tools = psychevo_home.join("tools");
    fs::create_dir_all(&tools).expect("tools");
    let rg = tools.join(if cfg!(windows) { "rg.exe" } else { "rg" });
    fs::write(&rg, "#!/bin/sh\nprintf 'test rg\\n'\n").expect("rg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&rg).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&rg, permissions).expect("chmod");
    }
}

pub(crate) fn write_config(
    path: impl AsRef<std::path::Path>,
    content: &str,
) -> std::io::Result<()> {
    let mut text = content.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    fs::write(path, text)
}

pub(crate) struct CatalogServer {
    pub(crate) base_url: String,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
}

impl CatalogServer {
    pub(crate) fn new(body: &'static str) -> Self {
        Self::with_delay(body, Duration::ZERO)
    }

    pub(crate) fn with_delay(body: &'static str, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let request = read_http_request(&mut stream);
                requests_for_thread.lock().expect("requests").push(request);
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
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

    pub(crate) fn request(&self) -> String {
        self.requests
            .lock()
            .expect("requests")
            .first()
            .cloned()
            .expect("request")
    }
}

pub(crate) fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    let mut expected_len = None;
    loop {
        let n = stream.read(&mut buf).expect("request");
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_len);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            break;
        }
    }
    String::from_utf8_lossy(&request).to_string()
}

pub(crate) fn assert_schema_property_descriptions(tool_name: &str, schema: &Value) {
    let mut missing = Vec::new();
    collect_missing_schema_descriptions(schema, tool_name.to_string(), &mut missing);
    assert!(
        missing.is_empty(),
        "{tool_name} has schema properties without descriptions: {missing:?}"
    );
}

pub(crate) fn assert_first_party_tool_declaration_quality(tool: &dyn ToolBinding) {
    assert!(
        !tool.description().trim().is_empty(),
        "{} has an empty tool description",
        tool.name()
    );
    let parameters = tool.parameters();
    assert_schema_property_descriptions(tool.name(), &parameters);
    let declaration_text = format!(
        "{}\n{}",
        tool.description(),
        serde_json::to_string(&parameters).expect("tool parameters serialize")
    )
    .to_ascii_lowercase();
    for implementation_term in [
        "harness",
        "authorized",
        "scoped grant",
        "permission and resource",
        "permissions.allow_login_shell",
        "accepted cwd",
        "model-visible",
        "active model",
        "runtime cap",
        "sqlite counter",
        "psychevo_home",
        "accepted mcp",
        "normalized or raw",
        "psychevo media artifact",
        "mailbox",
        "adapter",
        "ui projection",
        "persistence",
    ] {
        assert!(
            !declaration_text.contains(implementation_term),
            "{} exposes implementation term {implementation_term:?}: {declaration_text}",
            tool.name()
        );
    }
}

pub(crate) fn collect_missing_schema_descriptions(
    value: &Value,
    path: String,
    missing: &mut Vec<String>,
) {
    if let Some(properties) = value.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            let property_path = format!("{path}.{name}");
            let described = property
                .get("description")
                .and_then(Value::as_str)
                .is_some_and(|description| !description.trim().is_empty());
            if !described {
                missing.push(property_path.clone());
            }
            collect_missing_schema_descriptions(property, property_path, missing);
        }
    }
    if let Some(items) = value.get("items") {
        collect_missing_schema_descriptions(items, format!("{path}[]"), missing);
    }
    for keyword in ["oneOf", "anyOf", "allOf"] {
        if let Some(alternatives) = value.get(keyword).and_then(Value::as_array) {
            for (index, alternative) in alternatives.iter().enumerate() {
                collect_missing_schema_descriptions(
                    alternative,
                    format!("{path}.{keyword}[{index}]"),
                    missing,
                );
            }
        }
    }
}

// Runtime tests are split by subsystem while sharing this module's fixtures.
#[path = "config.rs"]
pub(crate) mod config;
#[path = "event_stream.rs"]
pub(crate) mod event_stream;
#[path = "media.rs"]
pub(crate) mod media;
#[path = "model_catalog.rs"]
pub(crate) mod model_catalog;
#[path = "modes_shell_tools.rs"]
pub(crate) mod modes_shell_tools;
#[path = "persistence_projection.rs"]
pub(crate) mod persistence_projection;
#[path = "sessions_titles.rs"]
pub(crate) mod sessions_titles;
#[path = "skills.rs"]
pub(crate) mod skills;
#[path = "sqlite.rs"]
pub(crate) mod sqlite;
#[path = "undo.rs"]
pub(crate) mod undo;
