use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{
    ResolvedRunProvider, create_global_custom_provider, custom_provider_api_key_env,
    fetch_model_catalog_with_client, load_run_config, model_catalog_endpoint,
    model_catalog_providers, resolve_run_provider,
};
use crate::events::{PersistenceSink, project_agent_event, project_run_stream_event};
use crate::paths::canonical_workdir;
use crate::run::{SESSION_TITLE_MAX_CHARS, ensure_new_tui_session_title};
use crate::snapshot::SnapshotStore;
use crate::tools::{WorkdirTool, list_tool_impl, search_tool_impl};
use crate::types::{MessageAccounting, ModelCost, ModelCostTier, ModelMetadata, SelectedAgent};
use pretty_assertions::assert_eq;
use psychevo_agent_core::{AgentEvent, AssistantBlock, EventSink, Message};
use psychevo_ai::{FakeProvider, Outcome, RawStreamEvent};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::tempdir;

fn base_options(temp: &tempfile::TempDir) -> RunOptions {
    RunOptions {
        db_path: temp.path().join("state.db"),
        workdir: temp.path().join("work"),
        snapshot_root: Some(temp.path().join("snapshots")),
        session: None,
        continue_latest: false,
        prompt: "hello".to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: None,
        model: None,
        reasoning_effort: None,
        include_reasoning: false,
        mode: RunMode::Build,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )])),
        agent: None,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
    }
}

fn home_dir(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join(".psychevo")
}

fn write_config(path: impl AsRef<std::path::Path>, content: &str) -> std::io::Result<()> {
    let mut text = content.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    fs::write(path, text)
}

struct CatalogServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl CatalogServer {
    fn new(body: &'static str) -> Self {
        Self::with_delay(body, Duration::ZERO)
    }

    fn with_delay(body: &'static str, delay: Duration) -> Self {
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

    fn request(&self) -> String {
        self.requests
            .lock()
            .expect("requests")
            .first()
            .cloned()
            .expect("request")
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

fn assert_schema_property_descriptions(tool_name: &str, schema: &Value) {
    let mut missing = Vec::new();
    collect_missing_schema_descriptions(schema, tool_name.to_string(), &mut missing);
    assert!(
        missing.is_empty(),
        "{tool_name} has schema properties without descriptions: {missing:?}"
    );
}

fn collect_missing_schema_descriptions(value: &Value, path: String, missing: &mut Vec<String>) {
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
include!("modes_shell_tools.rs");
include!("model_catalog.rs");
include!("config.rs");
include!("sessions_titles.rs");
include!("skills.rs");
include!("undo.rs");
include!("sqlite.rs");
include!("persistence_projection.rs");
