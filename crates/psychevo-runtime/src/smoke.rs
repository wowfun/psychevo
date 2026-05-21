use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use psychevo_agent_core::{
    AgentLoopRequest, ControlHandle, Message, run_agent_loop, user_text_message,
};
use psychevo_ai::{FakeProvider, Outcome, RawStreamEvent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::context::prune_context;
use crate::error::Result;
use crate::events::PersistenceSink;
use crate::messages::assistant_text;
use crate::paths::canonical_workdir;
use crate::store::SqliteStore;
use crate::tools::coding_core_tools;
use crate::types::{ModelMetadata, SmokeControl, SmokeOptions, SmokeResult};

const SMOKE_DIR: &str = ".psychevo-smoke";
const SMOKE_SUBJECT: &str = ".psychevo-smoke/subject.txt";
const SMOKE_GENERATED: &str = ".psychevo-smoke/generated.txt";
const SMOKE_MANIFEST: &str = ".psychevo-smoke/manifest.json";

pub async fn run_smoke(options: SmokeOptions) -> Result<SmokeResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.reset {
        reset_smoke(&workdir)?;
    }
    prepare_smoke_files(&workdir)?;

    let store = SqliteStore::open(&options.db_path)?;
    let session_id = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        session_id
    } else {
        store.create_session(&workdir)?
    };

    let prompt = options
        .prompt
        .clone()
        .unwrap_or_else(|| "smoke".to_string());
    let previous_messages = prune_context(
        store.load_messages(&session_id)?,
        options.max_context_messages,
    );
    let scripts = fake_scripts_for_prompt(&prompt);
    let provider = Arc::new(FakeProvider::new(scripts));
    let tools = coding_core_tools(&workdir);

    let (control_handle, control_receivers) = ControlHandle::new();
    let expected_control_outcome = match options.control {
        SmokeControl::None => None,
        SmokeControl::StopAfterTurn => Some(Outcome::Stopped),
        SmokeControl::AbortOnAgentStart => Some(Outcome::Aborted),
    };
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: options.control,
        control_handle: Some(control_handle),
        events: None,
        stream_events: None,
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: ModelMetadata::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    });
    let request = AgentLoopRequest {
        model_provider: "fake".to_string(),
        model: "fake-coding-model".to_string(),
        generation_metadata: json!({}),
        prompt_instructions: Vec::new(),
        turn_prompt_instructions: Vec::new(),
        previous_messages,
        context_messages: Vec::new(),
        prefix_contextual_user_messages: Vec::new(),
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message(prompt)],
        tools,
        max_turns: 8,
    };
    let completion = run_agent_loop(provider, request, sink, control_receivers).await?;
    write_smoke_manifest(&workdir)?;

    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();

    Ok(SmokeResult {
        session_id,
        outcome: completion.outcome,
        final_answer,
        db_path: options.db_path,
        workdir,
        tool_failures,
        expected_control_outcome,
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct SmokeManifest {
    files: Vec<String>,
}

fn reset_smoke(workdir: &Path) -> Result<()> {
    let manifest_path = workdir.join(SMOKE_MANIFEST);
    if !manifest_path.exists() {
        return Ok(());
    }
    let manifest: SmokeManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    for file in manifest.files {
        let path = workdir.join(file);
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    if manifest_path.is_file() {
        fs::remove_file(manifest_path)?;
    }
    Ok(())
}

fn prepare_smoke_files(workdir: &Path) -> Result<()> {
    let dir = workdir.join(SMOKE_DIR);
    fs::create_dir_all(&dir)?;
    fs::write(
        workdir.join(SMOKE_SUBJECT),
        "original psychevo smoke\nsecond line\n",
    )?;
    Ok(())
}

fn write_smoke_manifest(workdir: &Path) -> Result<()> {
    let manifest = SmokeManifest {
        files: vec![
            SMOKE_SUBJECT.to_string(),
            SMOKE_GENERATED.to_string(),
            SMOKE_MANIFEST.to_string(),
        ],
    };
    fs::write(
        workdir.join(SMOKE_MANIFEST),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(())
}

fn fake_scripts_for_prompt(prompt: &str) -> Vec<Vec<RawStreamEvent>> {
    let tools = selected_tools(prompt);
    if tools.is_empty() {
        return vec![vec![
            RawStreamEvent::Text(format!("smoke text: {prompt}")),
            RawStreamEvent::Done(Outcome::Normal),
        ]];
    }

    let mut first = Vec::new();
    let mut call_index = 0usize;
    for tool in tools {
        if tool == "read" {
            push_tool_call(
                &mut first,
                call_index,
                "read-1",
                "read",
                json!({ "path": SMOKE_SUBJECT, "offset": 1, "limit": 20 }),
            );
            call_index += 1;
            push_tool_call(
                &mut first,
                call_index,
                "read-2",
                "read",
                json!({ "path": SMOKE_SUBJECT, "offset": 2, "limit": 20 }),
            );
            call_index += 1;
        } else if tool == "write" {
            push_tool_call(
                &mut first,
                call_index,
                "write-1",
                "write",
                json!({ "path": SMOKE_GENERATED, "content": "written by psychevo smoke\n" }),
            );
            call_index += 1;
        } else if tool == "edit" {
            push_tool_call(
                &mut first,
                call_index,
                "edit-1",
                "edit",
                json!({
                    "mode": "replace",
                    "path": SMOKE_SUBJECT,
                    "old_string": "original",
                    "new_string": "edited"
                }),
            );
            call_index += 1;
        } else if tool == "exec_command" {
            push_tool_call(
                &mut first,
                call_index,
                "exec-1",
                "exec_command",
                json!({ "cmd": "printf 'exec smoke\\n'", "yield_time_ms": 250 }),
            );
            call_index += 1;
        }
    }
    first.push(RawStreamEvent::Done(Outcome::Normal));
    vec![
        first,
        vec![
            RawStreamEvent::Text("smoke tools complete".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ],
    ]
}

fn push_tool_call(
    events: &mut Vec<RawStreamEvent>,
    call_index: usize,
    id: &str,
    name: &str,
    args: Value,
) {
    events.push(RawStreamEvent::ToolStart {
        content_index: call_index,
        call_index,
        id: id.to_string(),
        name: name.to_string(),
    });
    let args = serde_json::to_string(&args).expect("smoke tool args serializable");
    let split = args.len() / 2;
    events.push(RawStreamEvent::ToolArgs {
        content_index: call_index,
        call_index,
        delta: args[..split].to_string(),
    });
    events.push(RawStreamEvent::ToolArgs {
        content_index: call_index,
        call_index,
        delta: args[split..].to_string(),
    });
    events.push(RawStreamEvent::ToolEnd {
        content_index: call_index,
        call_index,
    });
}

fn selected_tools(prompt: &str) -> Vec<&'static str> {
    let lower = prompt.to_lowercase();
    let mut found = ["read", "write", "edit", "exec_command", "write_stdin"]
        .into_iter()
        .filter_map(|name| lower.find(name).map(|idx| (idx, name)))
        .collect::<Vec<_>>();
    found.sort_by_key(|(idx, _)| *idx);
    found.into_iter().map(|(_, name)| name).collect()
}
