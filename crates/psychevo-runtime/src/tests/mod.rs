pub(crate) use super::*;
pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::io::{Read, Write};
pub(crate) use std::net::TcpListener;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use crate::config::{
    ResolvedRunProvider, create_global_custom_provider, custom_provider_api_key_env,
    fetch_and_cache_model_catalog, fetch_model_catalog_with_client, load_agent_backend_configs,
    load_project_context_instruction_mode, load_run_config, model_catalog_endpoint,
    model_catalog_entry_is_free, model_catalog_provider, model_catalog_providers,
    provider_models_cache_path_for_home, read_cached_model_catalog, resolve_compression_config,
    resolve_default_workspace_cwd, resolve_run_provider, resolve_workspace_root,
    set_auxiliary_model_with_reasoning, set_default_model, set_default_model_with_reasoning,
    write_cached_model_catalog,
};
pub(crate) use crate::events::{PersistenceSink, project_agent_event, project_run_stream_event};
pub(crate) use crate::paths::canonical_cwd;
pub(crate) use crate::run::{
    SESSION_TITLE_MAX_CHARS, ensure_new_visible_session_title,
    visible_session_source_allows_auto_title,
};
pub(crate) use crate::snapshot::SnapshotStore;
pub(crate) use crate::types::{
    MessageAccounting, ModelCatalogEntry, ModelCost, ModelCostTier, ModelMetadata,
    ProjectContextInstructionMode, SelectedAgent,
};
pub(crate) use psychevo_agent_core::{
    AgentEvent, AssistantBlock, EventSink, Message, ToolDisplaySpec,
};
pub(crate) use psychevo_ai::{FakeProvider, Outcome, RawStreamEvent};
pub(crate) use rusqlite::Connection;
pub(crate) use serde_json::{Value, json};
pub(crate) use tempfile::tempdir;

pub(crate) fn base_options(temp: &tempfile::TempDir) -> RunOptions {
    seed_managed_rg(&home_dir(temp));
    RunOptions {
        state: StateRuntime::open(temp.path().join("state.db")).expect("state runtime"),
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
        runtime_tools: Vec::new(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
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
        pub(crate) use std::os::unix::fs::PermissionsExt;

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

pub(crate) fn assert_schema_property_descriptions(tool_name: &str, schema: &Value) {
    let mut missing = Vec::new();
    collect_missing_schema_descriptions(schema, tool_name.to_string(), &mut missing);
    assert!(
        missing.is_empty(),
        "{tool_name} has schema properties without descriptions: {missing:?}"
    );
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
}

// Runtime tests are split by subsystem while sharing this module's fixtures.
#[path = "config.rs"]
pub(crate) mod config;
#[path = "model_catalog.rs"]
pub(crate) mod model_catalog;
#[path = "modes_shell_tools.rs"]
pub(crate) mod modes_shell_tools;
#[path = "sessions_titles.rs"]
pub(crate) mod sessions_titles;
pub(crate) use sessions_titles::{assistant_message, user_message};
#[path = "persistence_projection.rs"]
pub(crate) mod persistence_projection;
#[path = "skills.rs"]
pub(crate) mod skills;
#[path = "sqlite.rs"]
pub(crate) mod sqlite;
#[path = "undo.rs"]
pub(crate) mod undo;
