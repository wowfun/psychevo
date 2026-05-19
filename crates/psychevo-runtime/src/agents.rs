use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{
    AgentLoopRequest, AssistantBlock, ControlHandle, Message, ToolBinding, ToolExecutionMode,
    ToolOutput, user_text_message,
};
use psychevo_ai::{AbortSignal, GenerationProvider, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::context_usage::ContextRecorder;
use crate::error::{Error, Result};
use crate::events::PersistenceSink;
use crate::messages::assistant_text;
use crate::permissions::PermissionRuntime;
use crate::prompt_assembly::{
    PromptPrefixRecordInput, assemble_child_prompt_prefix, context_evidence_for_request,
    prompt_prefix_record, tool_declarations_hash,
};
use crate::skills::resolve_skills_home;
use crate::store::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMailboxEventInput, AgentMailboxEventRecord, SqliteStore,
};
use crate::tools::coding_core_tools_for_mode;
use crate::types::{
    ApprovalHandler, ApprovalMode, ModelMetadata, PermissionConfig, PermissionMode, RunMode,
    RunStreamEvent, RunStreamSink, SelectedAgent, SessionSummary, SmokeControl,
};

const MAX_AGENT_NAME_LEN: usize = 64;
const SUBAGENT_DEFAULT_MAX_TURNS: usize = 32;
pub const MAX_AGENT_SPAWN_DEPTH_CAP: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    Explicit,
    Project,
    ClaudeProject,
    Global,
    ClaudeGlobal,
    BuiltIn,
}

impl AgentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Project => "project",
            Self::ClaudeProject => "claude_project",
            Self::Global => "global",
            Self::ClaudeGlobal => "claude_global",
            Self::BuiltIn => "built_in",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentDiagnostic {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl AgentDiagnostic {
    fn warning(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "warning".to_string(),
            message: message.into(),
            path,
        }
    }

    fn collision(name: &str, winner: &Path, loser: &Path) -> Self {
        Self {
            kind: "collision".to_string(),
            message: format!(
                "agent name \"{name}\" collision; keeping {} and omitting {}",
                winner.display(),
                loser.display()
            ),
            path: Some(loser.to_path_buf()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AgentToolPolicy {
    pub allowed: Option<BTreeSet<String>>,
    pub denied: BTreeSet<String>,
    pub allowed_agents: Option<BTreeSet<String>>,
    pub denied_agents: BTreeSet<String>,
    pub permissions: Option<Value>,
    pub permission_mode: Option<AgentPermissionMode>,
    pub mcp_servers: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionMode {
    Default,
    AcceptEdits,
    Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub file_path: Option<PathBuf>,
    pub source: AgentSource,
    pub model: Option<String>,
    pub tool_policy: AgentToolPolicy,
    pub skills: Vec<String>,
    pub hooks: Option<Value>,
    pub background: Option<bool>,
    pub initial_prompt: Option<String>,
    pub max_turns: Option<usize>,
    pub max_spawn_depth: u8,
    pub project_instructions: Option<bool>,
    pub effort: Option<String>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentCatalog {
    pub agents: Vec<AgentDefinition>,
    pub shadowed_agents: Vec<AgentDefinition>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInvocationRole {
    Main,
    Subagent,
    Fork,
    System,
}

#[derive(Debug, Clone)]
pub struct AgentDiscoveryOptions {
    pub home: PathBuf,
    pub workdir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub explicit_inputs: Vec<String>,
    pub no_agents: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    PendingInit,
    Running,
    Completed,
    Errored,
    Interrupted,
    Shutdown,
    NotFound,
}

impl AgentRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::PendingInit => "pending_init",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Interrupted => "interrupted",
            Self::Shutdown => "shutdown",
            Self::NotFound => "not_found",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub id: String,
    pub task_name: Option<String>,
    pub agent_name: String,
    pub task: String,
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub role: AgentInvocationRole,
    pub background: bool,
    pub status: AgentRunStatus,
    pub edge_status: Option<AgentEdgeStatus>,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub outcome: Option<String>,
    pub final_answer: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub effective_max_spawn_depth: Option<u8>,
}

pub type AgentRun = AgentRunRecord;

pub struct AgentControl;

impl AgentControl {
    pub fn status_value() -> Value {
        agent_status_value(None, None, false)
    }

    pub async fn wait(id: &str, timeout: Duration) -> Result<Option<AgentRunRecord>> {
        wait_agent_id(id, timeout).await
    }

    pub fn close(id: &str) -> Result<Option<AgentRunRecord>> {
        close_agent_id(id, None)
    }

    pub fn send(id: &str, message: &str) -> Result<Option<AgentRunRecord>> {
        send_agent_message(id, message, None)
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawAgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    tools: Option<Value>,
    #[serde(rename = "disallowedTools")]
    disallowed_tools: Option<Value>,
    permission: Option<Value>,
    permissions: Option<Value>,
    #[serde(rename = "permissionMode")]
    permission_mode: Option<Value>,
    #[serde(rename = "mcpServers")]
    mcp_servers: Option<Value>,
    skills: Option<Value>,
    hooks: Option<Value>,
    background: Option<bool>,
    #[serde(rename = "initialPrompt")]
    initial_prompt: Option<String>,
    #[serde(rename = "maxTurns")]
    max_turns: Option<usize>,
    #[serde(rename = "maxSpawnDepth", alias = "max_spawn_depth")]
    max_spawn_depth: Option<u8>,
    #[serde(rename = "projectInstructions", alias = "project_instructions")]
    project_instructions: Option<Value>,
    effort: Option<String>,
    memory: Option<Value>,
    isolation: Option<Value>,
}

#[derive(Clone)]
pub(crate) struct AgentToolContext {
    pub(crate) provider: Arc<dyn GenerationProvider>,
    pub(crate) model_provider: String,
    pub(crate) model: String,
    pub(crate) provider_label: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) context_limit: Option<u64>,
    pub(crate) generation_metadata: Value,
    pub(crate) workdir: PathBuf,
    pub(crate) mode: RunMode,
    pub(crate) permission_config: PermissionConfig,
    pub(crate) permission_mode: PermissionMode,
    pub(crate) approval_mode: ApprovalMode,
    pub(crate) approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub(crate) store: SqliteStore,
    pub(crate) parent_session_id: String,
    pub(crate) parent_context_snapshot: Vec<Message>,
    pub(crate) catalog: AgentCatalog,
    pub(crate) control_handle: Option<ControlHandle>,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) model_metadata: ModelMetadata,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) allowed_agent_names: Option<BTreeSet<String>>,
    pub(crate) denied_agent_names: BTreeSet<String>,
    pub(crate) required_agent_names: Vec<String>,
    pub(crate) spawn_depth_remaining: Option<u8>,
}

struct AgentRunState {
    record: AgentRunRecord,
    control: Option<ControlHandle>,
}

static AGENT_RUNS: LazyLock<Mutex<HashMap<String, AgentRunState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static AGENT_SPAWN_PAUSED: AtomicBool = AtomicBool::new(false);

pub fn agent_spawn_paused() -> bool {
    AGENT_SPAWN_PAUSED.load(Ordering::SeqCst)
}

pub fn set_agent_spawn_paused(paused: bool) -> bool {
    AGENT_SPAWN_PAUSED.swap(paused, Ordering::SeqCst)
}

pub fn discover_agents(options: &AgentDiscoveryOptions) -> Result<AgentCatalog> {
    if options.no_agents {
        return Ok(AgentCatalog::default());
    }

    let mut catalog = AgentCatalog::default();
    let mut winners: BTreeMap<String, PathBuf> = BTreeMap::new();

    for input in &options.explicit_inputs {
        if input.trim().is_empty() {
            continue;
        }
        if let Some(path) = existing_agent_path(input, &options.workdir, &options.env)? {
            load_agent_file(&mut catalog, &mut winners, &path, AgentSource::Explicit)?;
        }
    }

    load_agent_dir(
        &mut catalog,
        &mut winners,
        &options.workdir.join(".psychevo").join("agents"),
        AgentSource::Project,
    )?;

    for dir in ancestor_claude_agent_dirs(&options.workdir) {
        load_agent_dir(&mut catalog, &mut winners, &dir, AgentSource::ClaudeProject)?;
    }

    load_agent_dir(
        &mut catalog,
        &mut winners,
        &options.home.join("agents"),
        AgentSource::Global,
    )?;

    if let Ok(home) = home_path(&options.env) {
        load_agent_dir(
            &mut catalog,
            &mut winners,
            &home.join(".claude").join("agents"),
            AgentSource::ClaudeGlobal,
        )?;
    }

    for agent in built_in_agents() {
        insert_agent(&mut catalog, &mut winners, agent);
    }

    Ok(catalog)
}

pub fn resolve_agent_definition(
    catalog: &AgentCatalog,
    input: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<AgentDefinition> {
    if let Some(path) = existing_agent_path(input, workdir, env)? {
        return parse_agent_file(&path, AgentSource::Explicit);
    }

    catalog
        .agents
        .iter()
        .find(|agent| agent.name == input)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {input}")))
}

pub fn list_agents_value(catalog: &AgentCatalog) -> Value {
    json!({
        "agents": catalog.agents.iter().map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "source": agent.source.as_str(),
                "path": agent.file_path,
                "model": agent.model,
                "tools": agent.tool_policy.allowed,
                "disallowed_tools": agent.tool_policy.denied,
                "allowed_agents": agent.tool_policy.allowed_agents,
                "disallowed_agents": agent.tool_policy.denied_agents,
                "permission_mode": agent.tool_policy.permission_mode,
                "max_spawn_depth": agent.max_spawn_depth,
                "project_instructions": agent.project_instructions,
                "effective_policy": agent_effective_policy_value(agent, Some(catalog)),
                "diagnostics": agent.diagnostics,
            })
        }).collect::<Vec<_>>(),
        "shadowed_agents": catalog.shadowed_agents.iter().map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "source": agent.source.as_str(),
                "path": agent.file_path,
                "model": agent.model,
                "tools": agent.tool_policy.allowed,
                "disallowed_tools": agent.tool_policy.denied,
                "allowed_agents": agent.tool_policy.allowed_agents,
                "disallowed_agents": agent.tool_policy.denied_agents,
                "permission_mode": agent.tool_policy.permission_mode,
                "max_spawn_depth": agent.max_spawn_depth,
                "project_instructions": agent.project_instructions,
                "effective_policy": agent_effective_policy_value(agent, Some(catalog)),
                "diagnostics": agent.diagnostics,
            })
        }).collect::<Vec<_>>(),
        "diagnostics": catalog.diagnostics,
    })
}

pub fn view_agent_value(agent: &AgentDefinition) -> Value {
    view_agent_value_with_catalog(agent, None)
}

pub fn view_agent_value_with_catalog(
    agent: &AgentDefinition,
    catalog: Option<&AgentCatalog>,
) -> Value {
    json!({
        "name": agent.name,
        "description": agent.description,
        "instructions": agent.instructions,
        "source": agent.source.as_str(),
        "path": agent.file_path,
        "model": agent.model,
        "tools": agent.tool_policy.allowed,
        "disallowed_tools": agent.tool_policy.denied,
        "allowed_agents": agent.tool_policy.allowed_agents,
        "disallowed_agents": agent.tool_policy.denied_agents,
        "permissions": agent.tool_policy.permissions,
        "permission_mode": agent.tool_policy.permission_mode,
        "mcp_servers": agent.tool_policy.mcp_servers,
        "skills": agent.skills,
        "hooks": agent.hooks,
        "background": agent.background,
        "initial_prompt": agent.initial_prompt,
        "max_turns": agent.max_turns,
        "max_spawn_depth": agent.max_spawn_depth,
        "project_instructions": agent.project_instructions,
        "effort": agent.effort,
        "tool_policy": {
            "tools": agent.tool_policy.allowed,
            "disallowed_tools": agent.tool_policy.denied,
            "allowed_agents": agent.tool_policy.allowed_agents,
            "disallowed_agents": agent.tool_policy.denied_agents,
            "permissions": agent.tool_policy.permissions,
            "permission_mode": agent.tool_policy.permission_mode,
            "mcp_servers": agent.tool_policy.mcp_servers,
        },
        "effective_policy": agent_effective_policy_value(agent, catalog),
        "diagnostics": agent.diagnostics,
    })
}

pub fn agent_effective_policy_value(
    agent: &AgentDefinition,
    catalog: Option<&AgentCatalog>,
) -> Value {
    let tools_mode = match &agent.tool_policy.allowed {
        None => "inherit",
        Some(allowed) if allowed.is_empty() => "explicit_empty",
        Some(_) => "explicit_allowlist",
    };
    let agent_catalog_visible = agent_policy_allows_agent_catalog(agent);
    let visible_agents = catalog.filter(|_| agent_catalog_visible).map(|catalog| {
        agent_catalog_for_policy(agent, &catalog.agents)
            .into_iter()
            .map(|agent| agent.name)
            .collect::<Vec<_>>()
    });
    json!({
        "tools": {
            "mode": tools_mode,
            "allowed": agent.tool_policy.allowed,
            "denied": agent.tool_policy.denied,
        },
        "agent_catalog": {
            "visible": agent_catalog_visible,
            "agents": visible_agents,
        },
        "skill_catalog": {
            "visible": agent_policy_allows_skill_catalog(agent),
        },
        "project_instructions": {
            "visible": agent_project_instructions_enabled(Some(agent)),
            "raw": agent.project_instructions,
        },
    })
}

pub(crate) fn agent_project_instructions_enabled(agent: Option<&AgentDefinition>) -> bool {
    agent.is_none_or(|agent| agent.project_instructions != Some(false))
}

pub fn format_agents_for_prompt(catalog: &[AgentDefinition]) -> String {
    if catalog.is_empty() {
        return String::new();
    }
    let mut text = String::from(
        "Available agents. Use the Agent tool for focused subagent work when useful. If the user addresses `@agent-name`, treat that as a request to delegate to that named agent.\n<agents>",
    );
    for agent in catalog {
        text.push_str("\n<agent name=\"");
        text.push_str(&agent.name);
        text.push_str("\" source=\"");
        text.push_str(agent.source.as_str());
        text.push_str("\">");
        text.push_str(&agent.description);
        text.push_str("</agent>");
    }
    text.push_str("\n</agents>");
    text
}

pub(crate) fn format_selected_agent_instruction(
    agent: &AgentDefinition,
    role: AgentInvocationRole,
) -> String {
    let label = match role {
        AgentInvocationRole::Main => "Main session agent",
        AgentInvocationRole::Subagent | AgentInvocationRole::Fork => "Child agent",
        AgentInvocationRole::System => "System agent",
    };
    let mut text = format!(
        "{label}: {}\n\nThe selected-agent purpose and instructions below define how you should handle this invocation. They take precedence over generic coding-agent behavior unless runtime mode, tool policy, safety constraints, resource gates, or direct user constraints are stricter.\n\nPurpose:\n{}",
        agent.name, agent.description
    );
    if !agent.instructions.trim().is_empty() {
        text.push_str("\n\nInstructions:\n");
        text.push_str(agent.instructions.trim());
    }
    text
}

pub(crate) fn apply_agent_tool_policy(
    tools: Vec<Arc<dyn ToolBinding>>,
    agent: Option<&AgentDefinition>,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    tools
        .into_iter()
        .filter(|tool| agent_allows_tool(tool.name(), agent, mode))
        .collect()
}

pub(crate) fn narrow_permission_mode_for_agent(
    parent: PermissionMode,
    agent: Option<&AgentDefinition>,
) -> PermissionMode {
    let Some(agent_mode) = agent.and_then(|agent| agent.tool_policy.permission_mode) else {
        return parent;
    };
    match agent_mode {
        AgentPermissionMode::Plan => parent,
        AgentPermissionMode::Default => match parent {
            PermissionMode::AcceptEdits | PermissionMode::BypassPermissions => {
                PermissionMode::Default
            }
            PermissionMode::Default | PermissionMode::DontAsk => parent,
        },
        AgentPermissionMode::AcceptEdits => match parent {
            PermissionMode::AcceptEdits | PermissionMode::BypassPermissions => {
                PermissionMode::AcceptEdits
            }
            PermissionMode::Default | PermissionMode::DontAsk => parent,
        },
    }
}

pub(crate) fn effective_tool_names(tools: &[Arc<dyn ToolBinding>]) -> Vec<String> {
    tools.iter().map(|tool| tool.name().to_string()).collect()
}

pub(crate) fn agent_catalog_for_prompt(
    catalog: &[AgentDefinition],
    selected_agent: Option<&AgentDefinition>,
    tools: &[Arc<dyn ToolBinding>],
) -> Vec<AgentDefinition> {
    if !tools.iter().any(|tool| tool.name() == "Agent") {
        return Vec::new();
    }
    agent_catalog_for_selected_policy(catalog, selected_agent)
}

pub(crate) fn agent_catalog_for_selected_policy(
    catalog: &[AgentDefinition],
    selected_agent: Option<&AgentDefinition>,
) -> Vec<AgentDefinition> {
    match selected_agent {
        Some(agent) => agent_catalog_for_policy(agent, catalog),
        None => catalog.to_vec(),
    }
}

pub(crate) fn skill_catalog_visible_for_tools(tools: &[Arc<dyn ToolBinding>]) -> bool {
    let has_list = tools.iter().any(|tool| tool.name() == "list_skills");
    let has_view = tools.iter().any(|tool| tool.name() == "view_skill");
    has_list && has_view
}

pub(crate) fn agent_policy_allows_agent_spawn(agent: &AgentDefinition) -> bool {
    agent_policy_allows_agent_catalog(agent)
}

pub(crate) fn apply_agent_hooks(
    tools: Vec<Arc<dyn ToolBinding>>,
    agent: Option<&AgentDefinition>,
    workdir: &Path,
) -> Vec<Arc<dyn ToolBinding>> {
    let Some(agent) = agent.filter(|agent| agent.hooks.is_some()) else {
        return tools;
    };
    tools
        .into_iter()
        .map(|tool| {
            Arc::new(HookedTool {
                inner: tool,
                hooks: agent.hooks.clone(),
                agent_name: agent.name.clone(),
                workdir: workdir.to_path_buf(),
            }) as Arc<dyn ToolBinding>
        })
        .collect()
}

pub(crate) fn run_agent_hook_event(
    agent: Option<&AgentDefinition>,
    event: &str,
    workdir: &Path,
    payload: Value,
) {
    let Some(agent) = agent else {
        return;
    };
    let payload = json!({
        "event": event,
        "agent": agent.name.clone(),
        "payload": payload,
    });
    let _ = run_hook_commands(agent.hooks.as_ref(), event, workdir, &payload);
}

pub(crate) fn agent_tools(context: AgentToolContext) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools = Vec::<Arc<dyn ToolBinding>>::new();
    if context.spawn_depth_remaining != Some(0) {
        tools.push(Arc::new(AgentTool::new(context.clone())));
    }
    tools.push(Arc::new(ListAgentsTool::new(context.clone())));
    tools.push(Arc::new(WaitAgentTool::new(context.clone())));
    tools.push(Arc::new(SendMessageTool::new(context.clone())));
    tools.push(Arc::new(CloseAgentTool::new(context.clone())));
    tools.push(Arc::new(ResumeAgentTool::new(context)));
    tools
}

pub fn agent_status_value(
    store: Option<&SqliteStore>,
    parent_session_id: Option<&str>,
    all: bool,
) -> Value {
    let mut records = Vec::new();
    let mut scope_sessions = BTreeSet::new();
    if let Some(store) = store {
        let edges = if all {
            store.list_agent_edges().unwrap_or_default()
        } else if let Some(parent) = parent_session_id {
            scope_sessions.insert(parent.to_string());
            collect_agent_edge_tree(store, parent).unwrap_or_default()
        } else {
            Vec::new()
        };
        for edge in &edges {
            scope_sessions.insert(edge.child_session_id.clone());
        }
        for edge in edges {
            records.push(agent_record_from_edge(store, edge));
        }
    }
    let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    for state in runs.values() {
        if !all && let Some(parent) = parent_session_id {
            let in_scope = state.record.parent_session_id == parent
                || scope_sessions.contains(&state.record.parent_session_id);
            if !in_scope {
                continue;
            }
        }
        if !records.iter().any(|record| record.id == state.record.id) {
            records.push(state.record.clone());
        }
    }
    records.sort_by(|left, right| right.started_at_ms.cmp(&left.started_at_ms));
    json!({
        "agents": records,
        "control": {
            "spawning_paused": agent_spawn_paused(),
            "max_spawn_depth_cap": MAX_AGENT_SPAWN_DEPTH_CAP,
            "concurrency_cap": Value::Null,
        }
    })
}

pub async fn wait_agent_id(id: &str, timeout: Duration) -> Result<Option<AgentRunRecord>> {
    let started = Instant::now();
    loop {
        if let Some(record) = {
            let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
            find_live_record_locked(&runs, id)
        } && agent_status_is_final(record.status)
        {
            return Ok(Some(record));
        }
        if started.elapsed() >= timeout {
            let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
            return Ok(find_live_record_locked(&runs, id));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn wait_agent_mailbox(
    parent_session_id: &str,
    timeout: Duration,
    store: &SqliteStore,
) -> Result<Value> {
    let started = Instant::now();
    loop {
        if store.has_pending_agent_mailbox_events(parent_session_id)? {
            return Ok(json!({
                "message": "Wait completed.",
                "timed_out": false,
            }));
        }
        if started.elapsed() >= timeout {
            return Ok(json!({
                "message": "Wait timed out.",
                "timed_out": true,
            }));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub fn close_agent_id(id: &str, store: Option<&SqliteStore>) -> Result<Option<AgentRunRecord>> {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    let Some((live_id, previous)) = find_live_key_and_record_locked(&runs, id) else {
        drop(runs);
        if let Some(store) = store
            && let Some(edge) = store.find_agent_edge(id)?
        {
            let previous = agent_record_from_edge(store, edge.clone());
            store.close_agent_edge_subtree(&edge.child_session_id)?;
            return Ok(Some(previous));
        }
        return Ok(None);
    };
    let child_session = {
        let state = runs.get_mut(&live_id).expect("live record exists");
        if let Some(control) = &state.control {
            control.stop();
        }
        state.record.status = AgentRunStatus::Shutdown;
        state.record.edge_status = Some(AgentEdgeStatus::Closed);
        state.record.ended_at_ms = Some(now_ms());
        state.record.outcome = Some("shutdown".to_string());
        state.record.child_session_id.clone()
    };
    if let Some(child_session) = child_session.as_deref() {
        close_live_descendants_locked(&mut runs, child_session);
    }
    drop(runs);
    if let Some(store) = store
        && let Some(child_session) = child_session
    {
        store.close_agent_edge_subtree(&child_session)?;
    }
    Ok(Some(previous))
}

pub fn stop_agent_id_with_grace(
    id: &str,
    store: Option<&SqliteStore>,
    grace: Duration,
) -> Result<Option<AgentRunRecord>> {
    let requested = request_agent_stop_id(id)?;
    if requested.is_none() {
        return close_agent_id(id, store);
    }
    std::thread::sleep(grace);
    force_stop_agent_id(id, store)
}

fn request_agent_stop_id(id: &str) -> Result<Option<AgentRunRecord>> {
    let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    let Some((live_id, record)) = find_live_key_and_record_locked(&runs, id) else {
        return Ok(None);
    };
    if agent_status_is_final(record.status) {
        return Ok(Some(record));
    }
    if let Some(state) = runs.get(&live_id)
        && let Some(control) = &state.control
    {
        control.stop();
    }
    Ok(Some(record))
}

fn force_stop_agent_id(id: &str, store: Option<&SqliteStore>) -> Result<Option<AgentRunRecord>> {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    let Some((live_id, previous)) = find_live_key_and_record_locked(&runs, id) else {
        drop(runs);
        if let Some(store) = store
            && let Some(edge) = store.find_agent_edge(id)?
        {
            let previous = agent_record_from_edge(store, edge.clone());
            store.close_agent_edge_subtree(&edge.child_session_id)?;
            return Ok(Some(previous));
        }
        return Ok(None);
    };
    if agent_status_is_final(previous.status) {
        return Ok(Some(previous));
    }
    let child_session = {
        let state = runs.get_mut(&live_id).expect("live record exists");
        if let Some(control) = &state.control {
            control.stop();
            control.abort();
        }
        state.record.status = AgentRunStatus::Interrupted;
        state.record.edge_status = Some(AgentEdgeStatus::Closed);
        state.record.ended_at_ms = Some(now_ms());
        state.record.outcome = Some("interrupted".to_string());
        state.record.child_session_id.clone()
    };
    if let Some(child_session) = child_session.as_deref() {
        interrupt_live_descendants_locked(&mut runs, child_session);
    }
    drop(runs);
    if let Some(store) = store
        && let Some(child_session) = child_session
    {
        store.close_agent_edge_subtree(&child_session)?;
    }
    Ok(Some(previous))
}

fn collect_agent_edge_tree(
    store: &SqliteStore,
    parent_session_id: &str,
) -> Result<Vec<AgentEdgeRecord>> {
    let mut records = Vec::new();
    let mut queue = vec![parent_session_id.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(parent) = queue.pop() {
        for edge in store.list_agent_edges_for_parent(&parent)? {
            if seen.insert(edge.child_session_id.clone()) {
                queue.push(edge.child_session_id.clone());
            }
            records.push(edge);
        }
    }
    Ok(records)
}

fn close_live_descendants_locked(
    runs: &mut HashMap<String, AgentRunState>,
    root_child_session_id: &str,
) {
    let mut sessions = BTreeSet::from([root_child_session_id.to_string()]);
    let mut changed = true;
    while changed {
        changed = false;
        for state in runs.values() {
            if sessions.contains(&state.record.parent_session_id)
                && let Some(child_session) = &state.record.child_session_id
                && sessions.insert(child_session.clone())
            {
                changed = true;
            }
        }
    }

    for state in runs.values_mut() {
        let child_in_scope = state
            .record
            .child_session_id
            .as_ref()
            .is_some_and(|child| sessions.contains(child));
        if child_in_scope || sessions.contains(&state.record.parent_session_id) {
            if let Some(control) = &state.control {
                control.stop();
            }
            state.record.status = AgentRunStatus::Shutdown;
            state.record.edge_status = Some(AgentEdgeStatus::Closed);
            state.record.ended_at_ms = Some(now_ms());
            state.record.outcome = Some("shutdown".to_string());
        }
    }
}

fn interrupt_live_descendants_locked(
    runs: &mut HashMap<String, AgentRunState>,
    root_child_session_id: &str,
) {
    let mut sessions = BTreeSet::from([root_child_session_id.to_string()]);
    let mut changed = true;
    while changed {
        changed = false;
        for state in runs.values() {
            if sessions.contains(&state.record.parent_session_id)
                && let Some(child_session) = &state.record.child_session_id
                && sessions.insert(child_session.clone())
            {
                changed = true;
            }
        }
    }

    for state in runs.values_mut() {
        let child_in_scope = state
            .record
            .child_session_id
            .as_ref()
            .is_some_and(|child| sessions.contains(child));
        if child_in_scope || sessions.contains(&state.record.parent_session_id) {
            if agent_status_is_final(state.record.status) {
                continue;
            }
            if let Some(control) = &state.control {
                control.stop();
                control.abort();
            }
            state.record.status = AgentRunStatus::Interrupted;
            state.record.edge_status = Some(AgentEdgeStatus::Closed);
            state.record.ended_at_ms = Some(now_ms());
            state.record.outcome = Some("interrupted".to_string());
        }
    }
}

pub fn send_agent_message(
    id: &str,
    message: &str,
    store: Option<&SqliteStore>,
) -> Result<Option<AgentRunRecord>> {
    let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    if let Some((live_id, record)) = find_live_key_and_record_locked(&runs, id) {
        if !agent_status_is_final(record.status) {
            if let Some(state) = runs.get(&live_id)
                && let Some(control) = &state.control
            {
                let _ = control.inject_user_message(user_text_message(message.to_string()));
            }
            return Ok(Some(record));
        }
        if store.is_none() {
            return Ok(Some(record));
        }
    }
    drop(runs);
    if let Some(store) = store
        && let Some(edge) = store.find_agent_edge(id)?
    {
        store.set_agent_edge_status(&edge.child_session_id, AgentEdgeStatus::Open)?;
        let mut record = agent_record_from_edge(store, edge);
        record.status = AgentRunStatus::PendingInit;
        record.edge_status = Some(AgentEdgeStatus::Open);
        return Ok(Some(record));
    }
    Ok(None)
}

async fn send_agent_message_with_context(
    context: AgentToolContext,
    target: &str,
    message: &str,
    abort: AbortSignal,
) -> Result<Option<AgentRunRecord>> {
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    if message.trim().is_empty() {
        return Err(Error::Message("agent message is empty".to_string()));
    }
    {
        let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        if let Some((live_id, record)) = find_live_key_and_record_locked(&runs, target)
            && !agent_status_is_final(record.status)
        {
            if let Some(state) = runs.get(&live_id)
                && let Some(control) = &state.control
            {
                let _ = control.inject_user_message(user_text_message(message.to_string()));
            }
            return Ok(Some(record));
        }
    }

    let Some(edge) = context.store.find_agent_edge(target)? else {
        return Ok(None);
    };
    let base = agent_record_from_edge(&context.store, edge.clone());
    let agent_name = edge_agent_name(&edge).unwrap_or(base.agent_name.as_str());
    let agent = context
        .catalog
        .agents
        .iter()
        .find(|agent| agent.name == agent_name)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {agent_name}")))?;
    let id = base.id;
    let task_name = base
        .task_name
        .clone()
        .unwrap_or_else(|| default_task_name(&agent.name, &id));
    let model_override = context
        .store
        .session_summary(&edge.child_session_id)?
        .map(|summary| summary.model);
    let spawn_depth_remaining = edge_spawn_depth_remaining(&edge);
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: message.to_string(),
        parent_session_id: edge.parent_session_id.clone(),
        child_session_id: Some(edge.child_session_id.clone()),
        role: base.role,
        background: true,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record: record.clone(),
                control: Some(control_handle),
            },
        );
    }
    context
        .store
        .set_agent_edge_status(&edge.child_session_id, AgentEdgeStatus::Open)?;
    let previous_messages = context.store.load_messages(&edge.child_session_id)?;
    let mut child_context = context;
    child_context.parent_session_id = edge.parent_session_id.clone();
    let child = ChildRun {
        id,
        context: child_context,
        agent,
        prompt: message.to_string(),
        task_name,
        model_override,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        spawn_depth_remaining,
        role: base.role,
        background: true,
        parent_tool_call_id: None,
        existing_child_session: Some(edge.child_session_id),
        previous_messages_override: Some(previous_messages),
        control_receivers,
        abort,
    };
    tokio::spawn(async move {
        let _ = run_child_agent(child).await;
    });
    Ok(Some(record))
}

pub fn resume_agent_id(id: &str, store: Option<&SqliteStore>) -> Result<Option<AgentRunRecord>> {
    if let Some(record) = {
        let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        find_live_record_locked(&runs, id)
    } {
        return Ok(Some(record));
    }
    if let Some(store) = store
        && let Some(edge) = store.find_agent_edge(id)?
    {
        store.set_agent_edge_status(&edge.child_session_id, AgentEdgeStatus::Open)?;
        let mut record = agent_record_from_edge(store, edge);
        record.edge_status = Some(AgentEdgeStatus::Open);
        return Ok(Some(record));
    }
    Ok(None)
}

fn edge_agent_name(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| agent.get("name"))
        .and_then(Value::as_str)
}

fn edge_spawn_depth_remaining(edge: &AgentEdgeRecord) -> u8 {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| {
            agent
                .get("effective_max_spawn_depth")
                .or_else(|| agent.get("max_spawn_depth"))
        })
        .and_then(Value::as_u64)
        .map(|value| (value as u8).min(MAX_AGENT_SPAWN_DEPTH_CAP))
        .unwrap_or(0)
}

fn find_live_record_locked(
    runs: &HashMap<String, AgentRunState>,
    target: &str,
) -> Option<AgentRunRecord> {
    find_live_key_and_record_locked(runs, target).map(|(_, record)| record)
}

fn find_live_key_and_record_locked(
    runs: &HashMap<String, AgentRunState>,
    target: &str,
) -> Option<(String, AgentRunRecord)> {
    runs.iter()
        .find(|(id, state)| {
            id.as_str() == target
                || state.record.child_session_id.as_deref() == Some(target)
                || state.record.task_name.as_deref() == Some(target)
        })
        .map(|(id, state)| (id.clone(), state.record.clone()))
}

fn agent_record_from_edge(store: &SqliteStore, edge: AgentEdgeRecord) -> AgentRunRecord {
    if let Some(record) = {
        let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        find_live_record_locked(&runs, &edge.child_session_id)
    } {
        return record;
    }
    let agent = edge
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object);
    let summary = store.session_summary(&edge.child_session_id).ok().flatten();
    let id = agent
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(edge.child_session_id.as_str())
        .to_string();
    let status = if edge.status == AgentEdgeStatus::Closed {
        AgentRunStatus::Shutdown
    } else {
        match summary
            .as_ref()
            .and_then(|summary| summary.end_reason.as_deref())
        {
            Some("normal") => AgentRunStatus::Completed,
            Some("stopped") | Some("aborted") => AgentRunStatus::Interrupted,
            Some(_) => AgentRunStatus::Errored,
            None => AgentRunStatus::Interrupted,
        }
    };
    let effective_max_spawn_depth = edge_spawn_depth_remaining(&edge);
    AgentRunRecord {
        id,
        task_name: agent
            .and_then(|agent| agent.get("task_name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        agent_name: agent
            .and_then(|agent| agent.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("agent")
            .to_string(),
        task: agent
            .and_then(|agent| agent.get("task"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        parent_session_id: edge.parent_session_id,
        child_session_id: Some(edge.child_session_id),
        role: agent
            .and_then(|agent| agent.get("role"))
            .and_then(Value::as_str)
            .and_then(parse_invocation_role)
            .unwrap_or(AgentInvocationRole::Subagent),
        background: agent
            .and_then(|agent| agent.get("background"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        status,
        edge_status: Some(edge.status),
        started_at_ms: summary
            .as_ref()
            .map(|summary| summary.started_at_ms)
            .unwrap_or(edge.created_at_ms),
        ended_at_ms: summary.as_ref().and_then(|summary| summary.ended_at_ms),
        outcome: summary.and_then(|summary| summary.end_reason),
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(effective_max_spawn_depth),
    }
}

fn agent_child_session_summary_value(store: &SqliteStore, summary: &SessionSummary) -> Value {
    let latest_usage = latest_session_assistant_usage(store, &summary.id);
    let latest_total_tokens = latest_usage.as_ref().and_then(usage_total_tokens);
    let mut value = json!({
        "id": summary.id,
        "message_count": summary.message_count,
        "tool_call_count": summary.tool_call_count,
    });
    if let Some(object) = value.as_object_mut() {
        if let Some(usage) = latest_usage {
            object.insert("latest_usage".to_string(), usage);
        }
        if let Some(tokens) = latest_total_tokens {
            object.insert("latest_total_tokens".to_string(), Value::from(tokens));
        }
    }
    value
}

fn latest_session_assistant_usage(store: &SqliteStore, session_id: &str) -> Option<Value> {
    store
        .load_tui_message_summaries(session_id)
        .ok()?
        .into_iter()
        .rev()
        .find_map(|summary| match summary.message {
            Message::Assistant { .. } => summary.usage,
            _ => None,
        })
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            let mut total = 0u64;
            let mut any = false;
            for key in [
                "input_tokens",
                "output_tokens",
                "reasoning_tokens",
                "cached_tokens",
                "cache_write_tokens",
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    total = total.saturating_add(value);
                    any = true;
                }
            }
            any.then_some(total)
        })
}

fn parse_invocation_role(value: &str) -> Option<AgentInvocationRole> {
    match value {
        "main" => Some(AgentInvocationRole::Main),
        "child" | "subagent" => Some(AgentInvocationRole::Subagent),
        "fork" => Some(AgentInvocationRole::Fork),
        "system" => Some(AgentInvocationRole::System),
        _ => None,
    }
}

fn agent_status_is_final(status: AgentRunStatus) -> bool {
    matches!(
        status,
        AgentRunStatus::Completed
            | AgentRunStatus::Errored
            | AgentRunStatus::Interrupted
            | AgentRunStatus::Shutdown
            | AgentRunStatus::NotFound
    )
}

fn load_agent_dir(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    dir: &Path,
    source: AgentSource,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut paths = Vec::new();
    collect_agent_markdown_files(dir, &mut paths)?;
    paths.sort();
    for path in paths {
        load_agent_file(catalog, winners, &path, source)?;
    }
    Ok(())
}

fn collect_agent_markdown_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_agent_markdown_files(&path, paths)?;
        } else if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    Ok(())
}

fn load_agent_file(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    path: &Path,
    source: AgentSource,
) -> Result<()> {
    match parse_agent_file(path, source) {
        Ok(agent) => insert_agent(catalog, winners, agent),
        Err(err) => catalog.diagnostics.push(AgentDiagnostic::warning(
            format!("failed to load agent {}: {err}", path.display()),
            Some(path.to_path_buf()),
        )),
    }
    Ok(())
}

fn insert_agent(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    agent: AgentDefinition,
) {
    let loser_path = agent
        .file_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("<{}>", agent.source.as_str())));
    if let Some(winner) = winners.get(&agent.name) {
        catalog
            .diagnostics
            .push(AgentDiagnostic::collision(&agent.name, winner, &loser_path));
        catalog.shadowed_agents.push(agent);
        return;
    }
    winners.insert(agent.name.clone(), loser_path);
    catalog.agents.push(agent);
}

fn parse_agent_file(path: &Path, source: AgentSource) -> Result<AgentDefinition> {
    let content = fs::read_to_string(path)?;
    let (frontmatter, instructions) = split_frontmatter(&content)?;
    let raw = match frontmatter {
        Some(frontmatter) => serde_yaml::from_str::<RawAgentFrontmatter>(frontmatter)
            .map_err(|err| Error::Config(format!("agent frontmatter failed: {err}")))?,
        None => RawAgentFrontmatter::default(),
    };
    let default_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("agent");
    agent_from_raw(
        raw,
        default_name,
        instructions,
        Some(path.to_path_buf()),
        source,
    )
}

fn agent_from_raw(
    raw: RawAgentFrontmatter,
    default_name: &str,
    instructions: String,
    file_path: Option<PathBuf>,
    source: AgentSource,
) -> Result<AgentDefinition> {
    let path = file_path.clone();
    let name = raw
        .name
        .as_deref()
        .unwrap_or(default_name)
        .trim()
        .to_string();
    let mut diagnostics = Vec::new();
    if !valid_agent_name(&name) {
        diagnostics.push(AgentDiagnostic::warning(
            format!("agent name `{name}` is invalid"),
            path.clone(),
        ));
    }
    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("agent `{name}` must define a description")))?
        .to_string();

    if raw.memory.is_some() {
        diagnostics.push(AgentDiagnostic::warning(
            "agent memory is parsed but not executed in this version",
            path.clone(),
        ));
    }
    if raw
        .isolation
        .as_ref()
        .is_some_and(|value| value.as_str() == Some("worktree"))
    {
        diagnostics.push(AgentDiagnostic::warning(
            "agent isolation: worktree is parsed but not executed in this version",
            path.clone(),
        ));
    }

    let (permission_mode, permission_diagnostic) =
        parse_permission_mode(raw.permission_mode.as_ref());
    if let Some(message) = permission_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }
    let (project_instructions, project_instructions_diagnostic) =
        parse_project_instructions(raw.project_instructions.as_ref());
    if let Some(message) = project_instructions_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }
    let tool_policy = parse_agent_tool_policy(
        raw.tools.as_ref(),
        raw.disallowed_tools.as_ref(),
        raw.permissions.or(raw.permission),
        permission_mode,
        raw.mcp_servers.as_ref(),
    );
    diagnostics.extend(tool_policy_diagnostics(&tool_policy, path.clone()));
    let model = raw.model.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty() && trimmed != "inherit").then(|| trimmed.to_string())
    });

    Ok(AgentDefinition {
        name,
        description,
        instructions: instructions.trim().to_string(),
        file_path,
        source,
        model,
        tool_policy,
        skills: parse_string_vec(raw.skills.as_ref()),
        hooks: raw.hooks,
        background: raw.background,
        initial_prompt: raw
            .initial_prompt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        max_turns: raw.max_turns,
        max_spawn_depth: clamp_agent_spawn_depth(raw.max_spawn_depth),
        project_instructions,
        effort: raw.effort,
        diagnostics,
    })
}

fn clamp_agent_spawn_depth(value: Option<u8>) -> u8 {
    value.unwrap_or(0).min(MAX_AGENT_SPAWN_DEPTH_CAP)
}

fn split_frontmatter(content: &str) -> Result<(Option<&str>, String)> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Ok((None, content.to_string()));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(Error::Config("agent frontmatter is not closed".to_string()));
    };
    let frontmatter = &rest[..end];
    let body = rest[end + "\n---".len()..]
        .strip_prefix('\n')
        .unwrap_or(&rest[end + "\n---".len()..]);
    Ok((Some(frontmatter), body.to_string()))
}

fn parse_agent_tool_policy(
    tools: Option<&Value>,
    disallowed_tools: Option<&Value>,
    permissions: Option<Value>,
    permission_mode: Option<AgentPermissionMode>,
    mcp_servers: Option<&Value>,
) -> AgentToolPolicy {
    let allowed = parse_allowed_tool_entries(tools);
    let denied = parse_tool_entries(disallowed_tools, ToolEntryMode::Deny);
    let (allowed_tools, allowed_agents) = match allowed {
        Some(allowed) => (
            Some(allowed.tools),
            (!allowed.agents.is_empty()).then_some(allowed.agents),
        ),
        None => (None, None),
    };
    AgentToolPolicy {
        allowed: allowed_tools,
        denied: denied.tools,
        allowed_agents,
        denied_agents: denied.agents,
        permissions,
        permission_mode,
        mcp_servers: parse_mcp_server_set(mcp_servers),
    }
}

fn parse_allowed_tool_entries(value: Option<&Value>) -> Option<ParsedToolEntries> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) if raw.trim().is_empty() => None,
        Some(Value::Array(items)) if items.is_empty() => Some(ParsedToolEntries::default()),
        Some(_) => Some(parse_tool_entries(value, ToolEntryMode::Allow)),
    }
}

fn tool_policy_diagnostics(
    policy: &AgentToolPolicy,
    path: Option<PathBuf>,
) -> Vec<AgentDiagnostic> {
    let mut diagnostics = Vec::new();
    for tool in policy
        .allowed
        .iter()
        .flat_map(|tools| tools.iter())
        .chain(policy.denied.iter())
    {
        if !known_tool_policy_name(tool) {
            diagnostics.push(AgentDiagnostic::warning(
                format!(
                    "agent tool `{tool}` is not a known built-in tool; preserving it for compatibility"
                ),
                path.clone(),
            ));
        }
    }
    diagnostics
}

fn known_tool_policy_name(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "search"
            | "list"
            | "bash"
            | "edit"
            | "write"
            | "Agent"
            | "Skill"
            | "list_agents"
            | "wait_agent"
            | "send_message"
            | "close_agent"
            | "resume_agent"
            | "list_skills"
            | "view_skill"
            | "create_skill"
            | "patch_skill"
            | "remove_skill"
            | "enable_skill"
            | "disable_skill"
            | "install_skill"
    ) || mcp_tool_server(name).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolEntryMode {
    Allow,
    Deny,
}

#[derive(Debug, Default)]
struct ParsedToolEntries {
    tools: BTreeSet<String>,
    agents: BTreeSet<String>,
}

fn parse_tool_entries(value: Option<&Value>, mode: ToolEntryMode) -> ParsedToolEntries {
    let mut parsed = ParsedToolEntries::default();
    for item in parse_tool_vec(value) {
        let (tool, agents) = parse_tool_entry(&item);
        let canonical = normalize_tool_name(tool);
        if !(mode == ToolEntryMode::Deny && canonical == "Agent" && !agents.is_empty()) {
            parsed.tools.insert(canonical.clone());
        }
        if canonical == "Agent" {
            parsed.agents.extend(agents);
        }
    }
    parsed
}

fn parse_tool_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(raw)) => split_tool_string(raw),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split_tool_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn split_tool_string(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '(' => {
                depth = depth.saturating_add(1);
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                let item = current.trim();
                if !item.is_empty() {
                    items.push(item.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let item = current.trim();
    if !item.is_empty() {
        items.push(item.to_string());
    }
    items
}

fn parse_tool_entry(raw: &str) -> (String, Vec<String>) {
    let trimmed = raw.trim();
    let Some(open) = trimmed.find('(') else {
        return (trimmed.to_string(), Vec::new());
    };
    if !trimmed.ends_with(')') {
        return (trimmed.to_string(), Vec::new());
    }
    let tool = trimmed[..open].trim().to_string();
    let names = trimmed[open + 1..trimmed.len().saturating_sub(1)]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (tool, names)
}

fn parse_permission_mode(value: Option<&Value>) -> (Option<AgentPermissionMode>, Option<String>) {
    let Some(raw) = value.and_then(Value::as_str).map(str::trim) else {
        return (None, None);
    };
    match raw {
        "" | "default" => (Some(AgentPermissionMode::Default), None),
        "acceptEdits" | "accept_edits" => (Some(AgentPermissionMode::AcceptEdits), None),
        "plan" => (Some(AgentPermissionMode::Plan), None),
        "bypass"
        | "bypassPermissions"
        | "bypass_permissions"
        | "dangerous"
        | "dangerouslySkipPermissions"
        | "dangerously_skip_permissions" => (
            None,
            Some(format!(
                "permissionMode `{raw}` is parsed but unsupported; it does not expand tool access"
            )),
        ),
        other => (
            None,
            Some(format!(
                "permissionMode `{other}` is not recognized and does not change tool access"
            )),
        ),
    }
}

fn parse_project_instructions(value: Option<&Value>) -> (Option<bool>, Option<String>) {
    match value {
        None | Some(Value::Null) => (None, None),
        Some(Value::Bool(enabled)) => (Some(*enabled), None),
        Some(_) => (
            None,
            Some(
                "projectInstructions must be a boolean when set; defaulting to injected project instructions"
                    .to_string(),
            ),
        ),
    }
}

fn parse_string_set(value: Option<&Value>) -> Option<BTreeSet<String>> {
    let items = parse_string_vec(value);
    (!items.is_empty()).then(|| items.into_iter().collect())
}

fn parse_mcp_server_set(value: Option<&Value>) -> BTreeSet<String> {
    parse_string_set(value).unwrap_or_default()
}

fn parse_string_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(raw)) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_tool_name(raw: String) -> String {
    match raw.trim() {
        "Read" | "read" => "read".to_string(),
        "Grep" | "grep" | "Search" | "search" => "search".to_string(),
        "Glob" | "glob" | "List" | "list" => "list".to_string(),
        "Bash" | "bash" => "bash".to_string(),
        "Edit" | "edit" => "edit".to_string(),
        "Write" | "write" => "write".to_string(),
        "Agent" | "agent" | "Task" | "task" => "Agent".to_string(),
        "Skill" | "skill" => "Skill".to_string(),
        other => other.to_string(),
    }
}

fn agent_allows_tool(name: &str, agent: Option<&AgentDefinition>, mode: RunMode) -> bool {
    let Some(agent) = agent else {
        if mode == RunMode::Plan && !plan_mode_tool_allowed(name) {
            return false;
        }
        return true;
    };
    if (mode == RunMode::Plan
        || agent.tool_policy.permission_mode == Some(AgentPermissionMode::Plan))
        && !plan_mode_tool_allowed(name)
    {
        return false;
    }
    let canonical = normalize_tool_name(name.to_string());
    let policy_names = tool_policy_names(name, &canonical);
    if policy_names
        .iter()
        .any(|name| agent.tool_policy.denied.contains(name.as_str()))
    {
        return false;
    }
    if let Some(server) = mcp_tool_server(name)
        && !agent.tool_policy.mcp_servers.is_empty()
        && !agent.tool_policy.mcp_servers.contains(server)
    {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => policy_names
            .iter()
            .any(|name| allowed.contains(name.as_str())),
        None => true,
    }
}

fn tool_policy_names(name: &str, canonical: &str) -> Vec<String> {
    let mut names = Vec::from([canonical.to_string(), name.to_string()]);
    if agent_control_tool_name(name) {
        names.push("Agent".to_string());
    }
    if skill_read_tool_name(name) {
        names.push("Skill".to_string());
    }
    names.sort();
    names.dedup();
    names
}

fn plan_mode_tool_allowed(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "list"
            | "search"
            | "list_skills"
            | "view_skill"
            | "Agent"
            | "list_agents"
            | "wait_agent"
            | "send_message"
            | "close_agent"
            | "resume_agent"
    )
}

fn mcp_tool_server(name: &str) -> Option<&str> {
    if let Some(raw) = name.strip_prefix("mcp:") {
        return raw
            .split([':', '/'])
            .next()
            .filter(|server| !server.is_empty());
    }
    name.strip_prefix("mcp__")
        .and_then(|raw| raw.split("__").next())
        .filter(|server| !server.is_empty())
}

fn agent_control_tool_name(name: &str) -> bool {
    matches!(
        name,
        "Agent" | "list_agents" | "wait_agent" | "send_message" | "close_agent" | "resume_agent"
    )
}

fn skill_read_tool_name(name: &str) -> bool {
    matches!(name, "list_skills" | "view_skill")
}

fn agent_policy_allows_agent_catalog(agent: &AgentDefinition) -> bool {
    if agent.tool_policy.denied.contains("Agent") {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => allowed.contains("Agent"),
        None => true,
    }
}

fn agent_policy_allows_skill_catalog(agent: &AgentDefinition) -> bool {
    if agent.tool_policy.denied.contains("Skill")
        || agent.tool_policy.denied.contains("list_skills")
        || agent.tool_policy.denied.contains("view_skill")
    {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => {
            allowed.contains("Skill")
                || (allowed.contains("list_skills") && allowed.contains("view_skill"))
        }
        None => true,
    }
}

fn agent_catalog_for_policy(
    agent: &AgentDefinition,
    catalog: &[AgentDefinition],
) -> Vec<AgentDefinition> {
    if !agent_policy_allows_agent_catalog(agent) {
        return Vec::new();
    }
    catalog
        .iter()
        .filter(|candidate| {
            agent
                .tool_policy
                .allowed_agents
                .as_ref()
                .is_none_or(|allowed| allowed.contains(&candidate.name))
        })
        .filter(|candidate| !agent.tool_policy.denied_agents.contains(&candidate.name))
        .cloned()
        .collect()
}

fn valid_agent_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_AGENT_NAME_LEN {
        return false;
    }
    let mut prev_dash = false;
    for ch in name.chars() {
        let valid = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
        if !valid {
            return false;
        }
        if ch == '-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
    }
    !name.starts_with('-') && !name.ends_with('-')
}

fn existing_agent_path(
    input: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let path = if raw == "~" {
        home_path(env)?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_path(env)?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let path = if path.is_absolute() {
        path
    } else {
        workdir.join(path)
    };
    Ok((path.is_file()).then_some(path))
}

fn ancestor_claude_agent_dirs(workdir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = workdir.to_path_buf();
    loop {
        result.push(current.join(".claude").join("agents"));
        if current.join(".git").exists() {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    result
}

fn home_path(env: &BTreeMap<String, String>) -> Result<PathBuf> {
    env.get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

fn built_in_agents() -> Vec<AgentDefinition> {
    vec![
        built_in_agent(
            "general",
            "General-purpose subagent for focused coding tasks.",
            "You are a focused general-purpose Psychevo subagent. Work only on the assigned task and return a concise final summary.",
            None,
        ),
        built_in_agent(
            "plan-research",
            "Read-only planning and research subagent.",
            "You are a read-only planning subagent. Inspect context and produce a concrete plan. Do not modify files or run mutating commands.",
            Some(["read", "list", "search"].into_iter().collect()),
        ),
        built_in_agent(
            "explore",
            "Read-only codebase exploration subagent.",
            "You are a read-only explorer. Answer specific codebase questions with file references and avoid broad refactors.",
            Some(["read", "list", "search"].into_iter().collect()),
        ),
    ]
}

fn built_in_agent(
    name: &str,
    description: &str,
    instructions: &str,
    allowed: Option<BTreeSet<&str>>,
) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        description: description.to_string(),
        instructions: instructions.to_string(),
        file_path: None,
        source: AgentSource::BuiltIn,
        model: None,
        tool_policy: AgentToolPolicy {
            allowed: allowed.map(|set| set.into_iter().map(str::to_string).collect()),
            denied: BTreeSet::new(),
            allowed_agents: None,
            denied_agents: BTreeSet::new(),
            permissions: None,
            permission_mode: None,
            mcp_servers: BTreeSet::new(),
        },
        skills: Vec::new(),
        hooks: None,
        background: None,
        initial_prompt: None,
        max_turns: None,
        max_spawn_depth: 0,
        project_instructions: None,
        effort: None,
        diagnostics: Vec::new(),
    }
}

struct AgentTool {
    context: AgentToolContext,
}

impl AgentTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

struct HookedTool {
    inner: Arc<dyn ToolBinding>,
    hooks: Option<Value>,
    agent_name: String,
    workdir: PathBuf,
}

impl ToolBinding for HookedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.inner.execution_mode()
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let inner = Arc::clone(&self.inner);
        let hooks = self.hooks.clone();
        let agent_name = self.agent_name.clone();
        let tool_name = self.inner.name().to_string();
        let workdir = self.workdir.clone();
        Box::pin(async move {
            let pre_payload = json!({
                "event": "PreToolUse",
                "agent": agent_name,
                "tool": tool_name,
                "tool_call_id": tool_call_id.clone(),
                "arguments": args.clone(),
            });
            if let Some(blocked) =
                run_hook_commands(hooks.as_ref(), "PreToolUse", &workdir, &pre_payload)
            {
                return ToolOutput::error(blocked);
            }

            let output = inner
                .execute(tool_call_id.clone(), args.clone(), abort)
                .await;
            let post_payload = json!({
                "event": "PostToolUse",
                "agent": agent_name,
                "tool": tool_name,
                "tool_call_id": tool_call_id,
                "arguments": args.clone(),
                "output": output.json.clone(),
                "is_error": output.is_error,
            });
            let _ = run_hook_commands(hooks.as_ref(), "PostToolUse", &workdir, &post_payload);
            output
        })
    }
}

impl ToolBinding for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Spawn a focused child agent. Named agents start with fresh context by default; set fork_context true to include the parent context snapshot."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_type": {"type": "string", "description": "Agent definition name. Defaults to general."},
                "name": {"type": "string", "description": "Compatibility alias for agent_type."},
                "prompt": {"type": "string", "description": "Task for the subagent."},
                "task_name": {"type": "string", "description": "Optional durable task name for later wait/send/close/resume."},
                "background": {"type": "boolean", "description": "When true, return a handle immediately."},
                "model": {"type": "string", "description": "Optional model override."},
                "fork_context": {"type": "boolean", "description": "Include the parent context snapshot."},
                "fork_turns": {"type": "string", "description": "none, all, or a positive integer count of recent parent messages."},
                "max_turns": {"type": "integer", "minimum": 1},
                "max_spawn_depth": {"type": "integer", "minimum": 0, "maximum": MAX_AGENT_SPAWN_DEPTH_CAP}
            },
            "required": ["prompt"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let context = self.context.clone();
        Box::pin(async move {
            let parsed: AgentToolArgs = match serde_json::from_value(args) {
                Ok(args) => args,
                Err(err) => {
                    return ToolOutput::error(format!("invalid Agent arguments: {err}"));
                }
            };
            match spawn_subagent(context, parsed, tool_call_id, abort).await {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

#[derive(Debug, Deserialize)]
struct AgentToolArgs {
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    name: Option<String>,
    prompt: String,
    #[serde(default)]
    task_name: Option<String>,
    #[serde(default)]
    background: Option<bool>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    fork_context: bool,
    #[serde(default)]
    fork_turns: Option<String>,
    #[serde(default)]
    max_turns: Option<usize>,
    #[serde(default)]
    max_spawn_depth: Option<u8>,
}

async fn spawn_subagent(
    context: AgentToolContext,
    args: AgentToolArgs,
    tool_call_id: String,
    abort: AbortSignal,
) -> Result<Value> {
    if args.prompt.trim().is_empty() {
        return Err(Error::Message("Agent prompt is empty".to_string()));
    }
    if agent_spawn_paused() {
        return Err(Error::Config("agent spawning is paused".to_string()));
    }
    if context.spawn_depth_remaining == Some(0) {
        return Err(Error::Config(
            "agent spawning is disabled for this child agent".to_string(),
        ));
    }
    let agent_name = resolve_agent_tool_name(&args, &context.required_agent_names)?;
    let agent = context
        .catalog
        .agents
        .iter()
        .find(|agent| agent.name == agent_name)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {agent_name}")))?;
    if let Some(allowed) = &context.allowed_agent_names
        && !allowed.contains(&agent.name)
    {
        return Err(Error::Config(format!(
            "agent `{}` is not allowed by selected-agent tool policy",
            agent.name
        )));
    }
    if context.denied_agent_names.contains(&agent.name) {
        return Err(Error::Config(format!(
            "agent `{}` is denied by selected-agent tool policy",
            agent.name
        )));
    }
    let id = Uuid::now_v7().to_string();
    let task_name = args
        .task_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_task_name)
        .unwrap_or_else(|| default_task_name(&agent.name, &id));
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let background =
        args.fork_context || agent.background.unwrap_or(false) || args.background.unwrap_or(false);
    let role = if args.fork_context {
        AgentInvocationRole::Fork
    } else {
        AgentInvocationRole::Subagent
    };
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.prompt.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: None,
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record,
                control: Some(control_handle.clone()),
            },
        );
    }

    let response_agent_name = agent.name.clone();
    let response_agent_description = agent.description.clone();
    let response_task_name = task_name.clone();
    let response_store = context.store.clone();
    let child = ChildRun {
        id: id.clone(),
        context,
        agent,
        prompt: args.prompt,
        task_name,
        model_override: args.model,
        fork_context: args.fork_context,
        fork_turns: args.fork_turns,
        max_turns: args.max_turns,
        spawn_depth_remaining,
        role,
        background,
        parent_tool_call_id: Some(tool_call_id),
        existing_child_session: None,
        previous_messages_override: None,
        control_receivers,
        abort,
    };

    if background {
        tokio::spawn(async move {
            let _ = run_child_agent(child).await;
        });
        Ok(json!({
            "id": id,
            "agent_name": response_agent_name,
            "agent_description": response_agent_description,
            "task_name": response_task_name,
            "status": "running",
            "background": true,
            "effective_max_spawn_depth": spawn_depth_remaining
        }))
    } else {
        let record = run_child_agent(child).await?;
        let response_child_session_id = record.child_session_id.clone();
        let child_summary = record
            .child_session_id
            .as_deref()
            .and_then(|session_id| response_store.session_summary(session_id).ok().flatten())
            .map(|summary| agent_child_session_summary_value(&response_store, &summary));
        Ok(json!({
            "id": record.id,
            "agent_name": record.agent_name,
            "agent_description": response_agent_description,
            "task_name": record.task_name,
            "task": record.task,
            "status": record.status.as_str(),
            "background": false,
            "session_id": response_child_session_id,
            "child_session_id": record.child_session_id,
            "outcome": record.outcome,
            "final_answer": record.final_answer,
            "error": record.error,
            "child_session": child_summary,
            "effective_max_spawn_depth": record.effective_max_spawn_depth,
        }))
    }
}

fn resolve_agent_tool_name(
    args: &AgentToolArgs,
    required_agent_names: &[String],
) -> Result<String> {
    let agent_type = normalized_optional_name(args.agent_type.as_deref());
    let name_alias = normalized_optional_name(args.name.as_deref());
    if let (Some(agent_type), Some(name_alias)) = (&agent_type, &name_alias)
        && agent_type != name_alias
    {
        return Err(Error::Config(format!(
            "Agent arguments agent_type `{agent_type}` and name `{name_alias}` conflict"
        )));
    }
    if let Some(name) = agent_type.or(name_alias) {
        return Ok(name);
    }
    match required_agent_names {
        [single] => Ok(single.clone()),
        [] => Ok("general".to_string()),
        many => Err(Error::Config(format!(
            "Agent call must set agent_type when the user mentioned multiple agents: {}",
            many.join(", ")
        ))),
    }
}

fn normalized_optional_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn child_spawn_depth_remaining(
    context: &AgentToolContext,
    agent: &AgentDefinition,
    requested: Option<u8>,
) -> u8 {
    resolved_child_spawn_depth_remaining(
        context.spawn_depth_remaining,
        agent.max_spawn_depth,
        requested,
    )
}

fn resolved_child_spawn_depth_remaining(
    parent_remaining: Option<u8>,
    definition_depth: u8,
    requested: Option<u8>,
) -> u8 {
    let requested = clamp_agent_spawn_depth(requested.or(Some(definition_depth)));
    match parent_remaining {
        Some(parent_remaining) => requested.min(parent_remaining.saturating_sub(1)),
        None => requested,
    }
}

pub(crate) fn spawn_child_agent_background(
    context: AgentToolContext,
    agent: AgentDefinition,
    prompt: String,
) -> Result<AgentRunRecord> {
    if prompt.trim().is_empty() {
        return Err(Error::Message("Agent prompt is empty".to_string()));
    }
    let id = Uuid::now_v7().to_string();
    let task_name = default_task_name(&agent.name, &id);
    let role = AgentInvocationRole::Subagent;
    let background = true;
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, None);
    let child_model = child_model_from(&context, &agent, None);
    let metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: &id,
        task_name: &task_name,
        agent: &agent,
        parent_session_id: &context.parent_session_id,
        role,
        task: &prompt,
        background,
        fork_context: false,
        spawn_depth_remaining,
        context: Some(&context),
    });
    let child_session = context.store.create_child_session_with_metadata(
        &context.parent_session_id,
        &context.workdir,
        "agent",
        &child_model,
        &context.model_provider,
        Some(metadata.clone()),
    )?;
    context.store.upsert_agent_edge(
        &context.parent_session_id,
        &child_session,
        AgentEdgeStatus::Open,
        Some(metadata),
    )?;
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: prompt.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: Some(child_session.clone()),
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record: record.clone(),
                control: Some(control_handle),
            },
        );
    }
    append_parent_agent_start_notification(&context.store, &context.parent_session_id, &record)?;
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let child = ChildRun {
        id,
        context,
        agent,
        prompt,
        task_name,
        model_override: None,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        spawn_depth_remaining,
        role,
        background,
        parent_tool_call_id: None,
        existing_child_session: Some(child_session),
        previous_messages_override: Some(Vec::new()),
        control_receivers,
        abort: AbortSignal::new(abort_rx),
    };
    tokio::spawn(async move {
        let _ = run_child_agent(child).await;
    });
    Ok(record)
}

struct ChildRun {
    id: String,
    context: AgentToolContext,
    agent: AgentDefinition,
    prompt: String,
    task_name: String,
    model_override: Option<String>,
    fork_context: bool,
    fork_turns: Option<String>,
    max_turns: Option<usize>,
    spawn_depth_remaining: u8,
    role: AgentInvocationRole,
    background: bool,
    parent_tool_call_id: Option<String>,
    existing_child_session: Option<String>,
    previous_messages_override: Option<Vec<Message>>,
    control_receivers: psychevo_agent_core::ControlReceivers,
    abort: AbortSignal,
}

async fn run_child_agent(child: ChildRun) -> Result<AgentRunRecord> {
    if child.abort.aborted() {
        update_run_failed(&child.id, "parent invocation aborted");
        return Err(Error::Message("parent invocation aborted".to_string()));
    }
    let child_model = child_model(&child);
    let child_session = if let Some(child_session) = child.existing_child_session.clone() {
        child.context.store.resume_session(&child_session)?;
        child
            .context
            .store
            .set_agent_edge_status(&child_session, AgentEdgeStatus::Open)?;
        child_session
    } else {
        child.context.store.create_child_session_with_metadata(
            &child.context.parent_session_id,
            &child.context.workdir,
            "agent",
            &child_model,
            &child.context.model_provider,
            Some(child_agent_metadata(ChildAgentMetadataInput {
                id: &child.id,
                task_name: &child.task_name,
                agent: &child.agent,
                parent_session_id: &child.context.parent_session_id,
                role: child.role,
                task: &child.prompt,
                background: child.background,
                fork_context: child.fork_context,
                spawn_depth_remaining: child.spawn_depth_remaining,
                context: Some(&child.context),
            })),
        )?
    };
    update_run_child_session(&child.id, &child_session);
    emit_agent_session_start(&child, &child_session);
    if child.existing_child_session.is_none() {
        child.context.store.upsert_agent_edge(
            &child.context.parent_session_id,
            &child_session,
            AgentEdgeStatus::Open,
            Some(child_agent_metadata(ChildAgentMetadataInput {
                id: &child.id,
                task_name: &child.task_name,
                agent: &child.agent,
                parent_session_id: &child.context.parent_session_id,
                role: child.role,
                task: &child.prompt,
                background: child.background,
                fork_context: child.fork_context,
                spawn_depth_remaining: child.spawn_depth_remaining,
                context: Some(&child.context),
            })),
        )?;
    }
    run_agent_hook_event(
        Some(&child.agent),
        "SubagentStart",
        &child.context.workdir,
        json!({
            "id": child.id.clone(),
            "child_session_id": child_session.clone(),
            "parent_session_id": child.context.parent_session_id.clone(),
        }),
    );

    let previous_messages = child.previous_messages_override.clone().unwrap_or_else(|| {
        fork_messages(
            &child.context.parent_context_snapshot,
            child.fork_context,
            child.fork_turns.as_deref(),
        )
    });
    let mut tools = coding_core_tools_for_mode(&child.context.workdir, child.context.mode);
    let mut child_agent_tool_context = child.context.clone();
    child_agent_tool_context.parent_session_id = child_session.clone();
    child_agent_tool_context.parent_context_snapshot = previous_messages.clone();
    child_agent_tool_context.required_agent_names = Vec::new();
    child_agent_tool_context.spawn_depth_remaining = Some(child.spawn_depth_remaining);
    tools.extend(agent_tools(child_agent_tool_context));
    tools = apply_agent_tool_policy(tools, Some(&child.agent), child.context.mode);
    tools = apply_agent_hooks(tools, Some(&child.agent), &child.context.workdir);
    let permission_mode =
        narrow_permission_mode_for_agent(child.context.permission_mode, Some(&child.agent));
    let permission_runtime = PermissionRuntime::new(
        child.context.workdir.clone(),
        child.context.workdir.join(".psychevo"),
        child.context.permission_config.clone(),
        permission_mode,
        child.context.approval_mode,
        child.context.approval_handler.clone(),
    );
    tools = permission_runtime.wrap_tools(tools);
    let effective_tool_names = effective_tool_names(&tools);
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let prompt_assembly = assemble_child_prompt_prefix(
        child.context.mode,
        &child.agent,
        &child.context.model_metadata.capabilities,
        !tools.is_empty(),
    );
    let selected_agent = SelectedAgent {
        name: child.agent.name.clone(),
        source: child.agent.source.as_str().to_string(),
        path: child.agent.file_path.clone(),
    };
    let prefix_metadata = json!({
        "mode": child.context.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": child.context.approval_mode.as_str(),
        "selected_agent": selected_agent.clone(),
        "agent_role": invocation_role_str(child.role),
        "parent_session_id": child.context.parent_session_id.clone(),
        "effective_tools": effective_tool_names,
        "agent_catalog_visible": false,
        "visible_agents": [],
        "skill_catalog_visible": false,
        "project_instructions_visible": false,
        "project_instructions_role": serde_json::Value::Null,
    });
    let prefix_record = prompt_prefix_record(PromptPrefixRecordInput {
        session_id: &child_session,
        provider: &child.context.model_provider,
        model: &child_model,
        prefix_hash: prompt_assembly.prefix_hash.clone(),
        tool_declarations_hash,
        invalidation_reason: Some(if child.existing_child_session.is_some() {
            "child_session_resumed".to_string()
        } else {
            "new_child_session".to_string()
        }),
        slots: prompt_assembly.prefix_slots.clone(),
        metadata: Some(prefix_metadata.clone()),
    });
    let prefix_record = child
        .context
        .store
        .upsert_session_prompt_prefix(prefix_record)?;
    let prompt_prefix_metadata = json!({
        "hash": prefix_record.prefix_hash,
        "version": prefix_record.version,
        "created_at_ms": prefix_record.created_at_ms,
        "provider": prefix_record.provider,
        "model": prefix_record.model,
        "tool_declarations_hash": prefix_record.tool_declarations_hash,
        "invalidation_reason": prefix_record.invalidation_reason,
        "effective_tools": prefix_metadata.get("effective_tools").cloned().unwrap_or_default(),
        "agent_catalog_visible": prefix_metadata.get("agent_catalog_visible").cloned().unwrap_or_default(),
        "visible_agents": prefix_metadata.get("visible_agents").cloned().unwrap_or_default(),
        "skill_catalog_visible": prefix_metadata.get("skill_catalog_visible").cloned().unwrap_or_default(),
        "project_instructions_visible": prefix_metadata.get("project_instructions_visible").cloned().unwrap_or_default(),
        "project_instructions_role": prefix_metadata.get("project_instructions_role").cloned().unwrap_or_default(),
    });
    let prompt_context_evidence = context_evidence_for_request(
        &prompt_assembly.prompt_instructions,
        &[],
        &prompt_assembly.prefix_contextual_user_messages,
        &[],
    );
    let mut generation_metadata = child.context.generation_metadata.clone();
    if let Some(object) = generation_metadata.as_object_mut() {
        object.insert("prompt_prefix".to_string(), prompt_prefix_metadata.clone());
    }
    let request = AgentLoopRequest {
        model_provider: child.context.model_provider.clone(),
        model: child_model,
        generation_metadata,
        prompt_instructions: prompt_assembly.prompt_instructions,
        turn_prompt_instructions: Vec::new(),
        previous_messages,
        context_messages: Vec::new(),
        prefix_contextual_user_messages: prompt_assembly.prefix_contextual_user_messages,
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message(child.prompt.clone())],
        tools,
        max_turns: child
            .max_turns
            .or(child.agent.max_turns)
            .unwrap_or(SUBAGENT_DEFAULT_MAX_TURNS),
    };

    let child_stream_events = child.context.stream_events.as_ref().map(|stream| {
        let stream = Arc::clone(stream);
        let child_session_id = child_session.clone();
        Arc::new(move |event| {
            stream(RunStreamEvent::scoped(child_session_id.clone(), event));
        }) as RunStreamSink
    });
    let sink = Arc::new(PersistenceSink {
        store: child.context.store.clone(),
        session_id: child_session,
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(prompt_context_evidence),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: child_stream_events,
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: child.context.model_metadata.clone(),
        context_recorder: Option::<ContextRecorder>::None,
        prompt_display: None,
        selected_agent: Some(selected_agent),
        prompt_prefix_metadata: Some(prompt_prefix_metadata),
    });
    let parent_store = child.context.store.clone();
    let parent_session_id = child.context.parent_session_id.clone();
    let completion = match psychevo_agent_core::run_agent_loop(
        Arc::clone(&child.context.provider),
        request,
        sink,
        child.control_receivers,
    )
    .await
    {
        Ok(completion) => completion,
        Err(err) => {
            update_run_failed(&child.id, &err.to_string());
            run_agent_hook_event(
                Some(&child.agent),
                "SubagentStop",
                &child.context.workdir,
                json!({
                    "id": child.id.clone(),
                    "outcome": "failed",
                    "error": err.to_string(),
                }),
            );
            return Err(err.into());
        }
    };
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let record = update_run_completed(&child.id, completion.outcome, final_answer.clone());
    run_agent_hook_event(
        Some(&child.agent),
        "SubagentStop",
        &child.context.workdir,
        json!({
            "id": child.id.clone(),
            "outcome": completion.outcome.as_str(),
            "final_answer": final_answer.clone(),
        }),
    );
    if child.background {
        let _ = append_parent_agent_mailbox_event(
            &parent_store,
            &parent_session_id,
            &record,
            completion.outcome.as_str(),
            &final_answer,
        );
    }
    Ok(record)
}

fn emit_agent_session_start(child: &ChildRun, child_session_id: &str) {
    let Some(stream) = &child.context.stream_events else {
        return;
    };
    stream(RunStreamEvent::Event(json!({
        "type": "agent_session_start",
        "tool_call_id": child.parent_tool_call_id.clone(),
        "agent_id": child.id.clone(),
        "agent_name": child.agent.name.clone(),
        "agent_description": child.agent.description.clone(),
        "task_name": child.task_name.clone(),
        "task": child.prompt.clone(),
        "parent_session_id": child.context.parent_session_id.clone(),
        "child_session_id": child_session_id,
        "background": child.background,
        "role": invocation_role_str(child.role),
        "effective_max_spawn_depth": child.spawn_depth_remaining,
    })));
}

fn child_model(child: &ChildRun) -> String {
    child_model_from(
        &child.context,
        &child.agent,
        child.model_override.as_deref(),
    )
}

fn child_model_from(
    context: &AgentToolContext,
    agent: &AgentDefinition,
    model_override: Option<&str>,
) -> String {
    model_override
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "inherit")
        .map(str::to_string)
        .or_else(|| {
            context
                .env
                .get("PSYCHEVO_SUBAGENT_MODEL")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| agent.model.clone())
        .unwrap_or_else(|| context.model.clone())
}

fn invocation_role_str(role: AgentInvocationRole) -> &'static str {
    match role {
        AgentInvocationRole::Main => "main",
        AgentInvocationRole::Subagent => "child",
        AgentInvocationRole::Fork => "fork",
        AgentInvocationRole::System => "system",
    }
}

fn sanitize_task_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches(['-', '_']).to_string();
    if out.is_empty() {
        "agent-task".to_string()
    } else {
        out
    }
}

fn default_task_name(agent_name: &str, id: &str) -> String {
    let suffix = id.split('-').next().unwrap_or(id);
    sanitize_task_name(&format!("{agent_name}-{suffix}"))
}

const AGENT_NOTIFICATION_METADATA_KEY: &str = "agent_notification";

struct ChildAgentMetadataInput<'a> {
    id: &'a str,
    task_name: &'a str,
    agent: &'a AgentDefinition,
    parent_session_id: &'a str,
    role: AgentInvocationRole,
    task: &'a str,
    background: bool,
    fork_context: bool,
    spawn_depth_remaining: u8,
    context: Option<&'a AgentToolContext>,
}

fn child_agent_metadata(input: ChildAgentMetadataInput<'_>) -> Value {
    let mut object = input
        .context
        .and_then(|context| context.generation_metadata.as_object().cloned())
        .unwrap_or_default();
    if let Some(context) = input.context {
        object.insert(
            "provider_label".to_string(),
            Value::String(context.provider_label.clone()),
        );
        object.insert(
            "base_url".to_string(),
            Value::String(context.base_url.clone()),
        );
        object.insert(
            "api_key_env".to_string(),
            context
                .api_key_env
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "reasoning_effort".to_string(),
            context
                .reasoning_effort
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "mode".to_string(),
            Value::String(context.mode.as_str().to_string()),
        );
        object.insert(
            "permission_mode".to_string(),
            Value::String(context.permission_mode.as_str().to_string()),
        );
        object.insert(
            "approval_mode".to_string(),
            Value::String(context.approval_mode.as_str().to_string()),
        );
        let context_limit = context
            .context_limit
            .or_else(|| context.model_metadata.context_limit());
        if let Some(limit) = context_limit {
            object.insert("context_limit".to_string(), Value::from(limit));
        }
        object
            .entry("model_metadata".to_string())
            .or_insert_with(|| context.model_metadata.public_json());
    }
    object.insert(
        "agent".to_string(),
        json!({
            "id": input.id,
            "task_name": input.task_name,
            "name": input.agent.name.clone(),
            "source": input.agent.source.as_str(),
            "path": input.agent.file_path.clone(),
            "parent_session_id": input.parent_session_id,
            "role": invocation_role_str(input.role),
            "task": input.task,
            "background": input.background,
            "fork_context": input.fork_context,
            "effective_max_spawn_depth": input.spawn_depth_remaining,
            "max_spawn_depth": input.spawn_depth_remaining,
        }),
    );
    Value::Object(object)
}

fn append_parent_agent_start_notification(
    store: &SqliteStore,
    parent_session_id: &str,
    record: &AgentRunRecord,
) -> Result<()> {
    let text = format!(
        "Agent `{}` started in the background.\n\n{}",
        record.agent_name, record.task
    );
    let message = user_text_message(text);
    store.append_message_with_metrics(
        parent_session_id,
        &message,
        None,
        Some(json!({
            AGENT_NOTIFICATION_METADATA_KEY: {
                "type": "agent_started",
                "agent_id": record.id,
                "task_name": record.task_name,
                "agent_name": record.agent_name,
                "child_session_id": record.child_session_id,
                "status": record.status,
                "summary": record.task,
                "effective_max_spawn_depth": record.effective_max_spawn_depth,
                "hidden": false
            }
        })),
    )
}

fn append_parent_agent_mailbox_event(
    store: &SqliteStore,
    parent_session_id: &str,
    record: &AgentRunRecord,
    outcome: &str,
    final_answer: &str,
) -> Result<()> {
    let content = subagent_notification_content(record, outcome, final_answer);
    let payload = inter_agent_communication_payload(record, content.clone());
    let content_text = serde_json::to_string(&payload)?;
    store.append_agent_mailbox_event(AgentMailboxEventInput {
        parent_session_id: parent_session_id.to_string(),
        child_session_id: record.child_session_id.clone(),
        agent_id: record.id.clone(),
        task_name: record.task_name.clone(),
        agent_name: record.agent_name.clone(),
        content_text,
        payload,
        metadata: Some(json!({
            "type": "agent_completed",
            "agent_id": record.id,
            "task_name": record.task_name,
            "agent_name": record.agent_name,
            "child_session_id": record.child_session_id,
            "status": record.status,
            "outcome": outcome,
            "summary": final_answer,
            "background": record.background,
            "effective_max_spawn_depth": record.effective_max_spawn_depth
        })),
    })?;
    Ok(())
}

fn subagent_notification_content(
    record: &AgentRunRecord,
    outcome: &str,
    final_answer: &str,
) -> String {
    format!(
        "<subagent_notification>\n{}\n</subagent_notification>",
        json!({
            "agent_id": record.id,
            "task_name": record.task_name,
            "agent_name": record.agent_name,
            "child_session_id": record.child_session_id,
            "status": record.status,
            "outcome": outcome,
            "final_answer": final_answer,
        })
    )
}

fn inter_agent_communication_payload(record: &AgentRunRecord, content: String) -> Value {
    let author = record
        .task_name
        .as_deref()
        .map(sanitize_task_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| sanitize_task_name(&record.agent_name));
    json!({
        "author": format!("/root/{author}"),
        "recipient": "/root",
        "other_recipients": [],
        "content": content,
        "trigger_turn": false
    })
}

pub(crate) fn agent_mailbox_event_message(record: &AgentMailboxEventRecord) -> Message {
    Message::Assistant {
        content: vec![AssistantBlock::Text {
            text: record.content_text.clone(),
        }],
        timestamp_ms: record.delivered_at_ms.unwrap_or(record.created_at_ms),
        finish_reason: Some("inter_agent_communication".to_string()),
        outcome: Outcome::Normal,
        model: None,
        provider: None,
    }
}

fn fork_messages(
    snapshot: &[Message],
    fork_context: bool,
    fork_turns: Option<&str>,
) -> Vec<Message> {
    if !fork_context && fork_turns.unwrap_or("none") == "none" {
        return Vec::new();
    }
    match fork_turns.unwrap_or("all") {
        "none" => Vec::new(),
        "all" => snapshot.to_vec(),
        raw => match raw.parse::<usize>() {
            Ok(count) => snapshot
                .iter()
                .rev()
                .take(count)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            Err(_) => snapshot.to_vec(),
        },
    }
}

fn update_run_child_session(id: &str, child_session: &str) {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    if let Some(state) = runs.get_mut(id) {
        state.record.child_session_id = Some(child_session.to_string());
    }
}

fn update_run_completed(id: &str, outcome: Outcome, final_answer: String) -> AgentRunRecord {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    let state = runs.get_mut(id).expect("agent run exists");
    if agent_status_is_final(state.record.status) {
        return state.record.clone();
    }
    state.record.status = if outcome == Outcome::Normal {
        AgentRunStatus::Completed
    } else {
        AgentRunStatus::Errored
    };
    state.record.ended_at_ms = Some(now_ms());
    state.record.outcome = Some(outcome.as_str().to_string());
    state.record.final_answer = Some(final_answer);
    state.record.clone()
}

fn update_run_failed(id: &str, error: &str) {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    if let Some(state) = runs.get_mut(id) {
        if agent_status_is_final(state.record.status) {
            return;
        }
        state.record.status = AgentRunStatus::Errored;
        state.record.ended_at_ms = Some(now_ms());
        state.record.outcome = Some("failed".to_string());
        state.record.error = Some(error.to_string());
    }
}

struct ListAgentsTool {
    context: AgentToolContext,
}

impl ListAgentsTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List live and resumable child agents for this session."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.store.clone();
        let parent = self.context.parent_session_id.clone();
        Box::pin(
            async move { ToolOutput::ok(agent_status_value(Some(&store), Some(&parent), false)) },
        )
    }
}

struct WaitAgentTool {
    context: AgentToolContext,
}

impl WaitAgentTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait for a background agent mailbox update. The result only reports wait status; agent output is delivered through mailbox context."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "timeout_ms": {"type": "integer", "minimum": 0}
            },
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.store.clone();
        let parent_session_id = self.context.parent_session_id.clone();
        let control_handle = self.context.control_handle.clone();
        Box::pin(async move {
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(30_000);
            let value = match wait_agent_mailbox(
                &parent_session_id,
                Duration::from_millis(timeout_ms),
                &store,
            )
            .await
            {
                Ok(value) => value,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let timed_out = value
                .get("timed_out")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !timed_out {
                let delivered_after_seq = match store.next_message_seq(&parent_session_id) {
                    Ok(seq) => seq,
                    Err(err) => return ToolOutput::error(err.to_string()),
                };
                let delivered = match store.deliver_pending_agent_mailbox_events_for_tool(
                    &parent_session_id,
                    &tool_call_id,
                    delivered_after_seq,
                ) {
                    Ok(records) => records,
                    Err(err) => return ToolOutput::error(err.to_string()),
                };
                if let Some(handle) = control_handle {
                    for record in delivered.iter().filter(|record| {
                        record.delivered_tool_call_id.as_deref() == Some(tool_call_id.as_str())
                            && record.delivered_after_session_seq == Some(delivered_after_seq)
                    }) {
                        let _ = handle.inject_user_message(agent_mailbox_event_message(record));
                    }
                }
            }
            ToolOutput::ok(value)
        })
    }
}

struct SendMessageTool {
    context: AgentToolContext,
}

impl SendMessageTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to an agent. Closed or completed agents are reopened for continuation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {"type": "string"},
                "message": {"type": "string"}
            },
            "required": ["target", "message"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let context = self.context.clone();
        Box::pin(async move {
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let message = args
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match send_agent_message_with_context(context, target, message, abort).await {
                Ok(Some(record)) => ToolOutput::ok(json!({ "agent": record })),
                Ok(None) => ToolOutput::error(format!("agent not found: {target}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

struct CloseAgentTool {
    context: AgentToolContext,
}

impl CloseAgentTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for CloseAgentTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn description(&self) -> &str {
        "Close an agent and its open descendants, returning the previous status."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"target": {"type": "string"}},
            "required": ["target"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.store.clone();
        Box::pin(async move {
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match close_agent_id(target, Some(&store)) {
                Ok(Some(record)) => ToolOutput::ok(json!({ "previous_status": record })),
                Ok(None) => ToolOutput::error(format!("agent not found: {target}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

struct ResumeAgentTool {
    context: AgentToolContext,
}

impl ResumeAgentTool {
    fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for ResumeAgentTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn description(&self) -> &str {
        "Reopen a previously closed agent so it can be addressed by later control tools."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.store.clone();
        Box::pin(async move {
            let id = args.get("id").and_then(Value::as_str).unwrap_or_default();
            match resume_agent_id(id, Some(&store)) {
                Ok(Some(record)) => ToolOutput::ok(json!({ "agent": record })),
                Ok(None) => ToolOutput::error(format!("agent not found: {id}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn resolve_agents_home(
    env: &BTreeMap<String, String>,
    workdir: &Path,
) -> Result<PathBuf> {
    resolve_skills_home(env, workdir)
}

fn run_hook_commands(
    hooks: Option<&Value>,
    event: &str,
    workdir: &Path,
    payload: &Value,
) -> Option<String> {
    for command in hook_commands(hooks, event) {
        let output = run_hook_command(&command, workdir, payload);
        match output {
            Ok(output) if event == "PreToolUse" && output.status.code() == Some(2) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Some(if stderr.is_empty() {
                    "PreToolUse hook blocked tool execution".to_string()
                } else {
                    stderr
                });
            }
            Ok(_) | Err(_) => {}
        }
    }
    None
}

fn run_hook_command(
    command: &str,
    workdir: &Path,
    payload: &Value,
) -> std::io::Result<std::process::Output> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    child.wait_with_output()
}

fn hook_commands(hooks: Option<&Value>, event: &str) -> Vec<String> {
    let Some(Value::Object(map)) = hooks else {
        return Vec::new();
    };
    let Some(value) = map.get(event).or_else(|| map.get(&event.to_lowercase())) else {
        return Vec::new();
    };
    parse_hook_command_value(value)
}

fn parse_hook_command_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(command) => vec![command.clone()],
        Value::Array(items) => items.iter().flat_map(parse_hook_command_value).collect(),
        Value::Object(map) => map
            .get("command")
            .and_then(Value::as_str)
            .map(|command| vec![command.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use psychevo_agent_core::{AssistantBlock, ToolBinding, ToolExecutionMode, ToolOutput};
    use psychevo_ai::{AbortSignal, FakeProvider};
    use tempfile::TempDir;
    use tokio::sync::watch;

    struct TestTool(&'static str);

    impl ToolBinding for TestTool {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "test tool"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn execution_mode(&self) -> ToolExecutionMode {
            ToolExecutionMode::Parallel
        }

        fn execute(
            &self,
            _tool_call_id: String,
            _args: Value,
            _abort: AbortSignal,
        ) -> BoxFuture<'static, ToolOutput> {
            Box::pin(async { ToolOutput::ok(json!({})) })
        }
    }

    fn test_tool(name: &'static str) -> Arc<dyn ToolBinding> {
        Arc::new(TestTool(name))
    }

    fn test_agent_run_record(
        parent_session_id: String,
        child_session_id: Option<String>,
    ) -> AgentRunRecord {
        AgentRunRecord {
            id: "agent-1".to_string(),
            task_name: Some("worker-task".to_string()),
            agent_name: "worker".to_string(),
            task: "do the work".to_string(),
            parent_session_id,
            child_session_id,
            role: AgentInvocationRole::Subagent,
            background: true,
            status: AgentRunStatus::Completed,
            edge_status: Some(AgentEdgeStatus::Open),
            started_at_ms: 1,
            ended_at_ms: Some(2),
            outcome: Some("normal".to_string()),
            final_answer: Some("mailbox final".to_string()),
            error: None,
            effective_max_spawn_depth: Some(0),
        }
    }

    fn env(home: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_HOME".to_string(),
                home.join(".psychevo").display().to_string(),
            ),
        ])
    }

    #[test]
    fn parses_claude_style_agent_frontmatter() {
        let tmp = TempDir::new().expect("tmp");
        let path = tmp.path().join("reviewer.md");
        fs::write(
            &path,
            r#"---
name: reviewer
description: Review code carefully
tools: Read, Grep, Agent
disallowedTools:
  - Bash
memory: true
maxSpawnDepth: 1
---
Review the code.
"#,
        )
        .expect("write");

        let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
        assert_eq!(agent.name, "reviewer");
        assert_eq!(
            agent
                .tool_policy
                .allowed
                .as_ref()
                .unwrap()
                .get("read")
                .map(String::as_str),
            Some("read")
        );
        assert!(
            agent
                .tool_policy
                .allowed
                .as_ref()
                .unwrap()
                .contains("search")
        );
        assert!(
            agent
                .tool_policy
                .allowed
                .as_ref()
                .unwrap()
                .contains("Agent")
        );
        assert!(agent.tool_policy.denied.contains("bash"));
        assert_eq!(agent.max_spawn_depth, 1);
        assert!(
            agent
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("memory"))
        );
    }

    #[test]
    fn parses_named_agent_tool_restrictions_and_permission_mode() {
        let tmp = TempDir::new().expect("tmp");
        let path = tmp.path().join("planner.md");
        fs::write(
            &path,
            r#"---
name: planner
description: Plan with a specific delegate
model: inherit
tools: "Read, Agent(review, explore)"
disallowedTools: "Agent(explore)"
permissionMode: plan
initialPrompt: "draft a plan"
---
Plan the work.
"#,
        )
        .expect("write");

        let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
        assert_eq!(agent.model, None);
        assert_eq!(agent.initial_prompt.as_deref(), Some("draft a plan"));
        assert_eq!(
            agent.tool_policy.permission_mode,
            Some(AgentPermissionMode::Plan)
        );
        assert!(
            agent
                .tool_policy
                .allowed_agents
                .as_ref()
                .expect("allowed agents")
                .contains("review")
        );
        assert!(agent.tool_policy.denied_agents.contains("explore"));
        assert!(agent_allows_tool("read", Some(&agent), RunMode::Build));
        assert!(agent_allows_tool("Agent", Some(&agent), RunMode::Build));
        assert!(!agent_allows_tool("bash", Some(&agent), RunMode::Build));
    }

    #[tokio::test]
    async fn background_completion_records_mailbox_event_without_parent_user_message() {
        let tmp = TempDir::new().expect("tmp");
        let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
        let parent = store
            .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
            .expect("parent");
        let child = store
            .create_child_session_with_metadata(
                &parent,
                tmp.path(),
                "agent",
                "model",
                "provider",
                None,
            )
            .expect("child");
        let record = test_agent_run_record(parent.clone(), Some(child));

        append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
            .expect("mailbox event");

        assert!(store.load_messages(&parent).expect("messages").is_empty());
        let events = store.load_agent_mailbox_events(&parent).expect("events");
        assert_eq!(events.len(), 1);
        assert!(events[0].content_text.contains("mailbox final"));
        assert!(
            events[0].payload["content"]
                .as_str()
                .expect("content")
                .contains("<subagent_notification>")
        );
    }

    #[tokio::test]
    async fn wait_agent_mailbox_returns_status_without_final_answer() {
        let tmp = TempDir::new().expect("tmp");
        let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
        let parent = store
            .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
            .expect("parent");
        let record = test_agent_run_record(parent.clone(), None);
        append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
            .expect("mailbox event");

        let value = wait_agent_mailbox(&parent, Duration::from_millis(0), &store)
            .await
            .expect("wait");
        assert_eq!(value["timed_out"], false);
        assert_eq!(value["message"], "Wait completed.");
        assert!(value.get("final_answer").is_none());
        assert!(value.get("statuses").is_none());

        let empty_parent = store
            .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
            .expect("empty parent");
        let value = wait_agent_mailbox(&empty_parent, Duration::from_millis(0), &store)
            .await
            .expect("timeout");
        assert_eq!(value["timed_out"], true);
    }

    #[test]
    fn agent_permission_mode_can_only_narrow_parent_mode() {
        let mut agent = AgentDefinition {
            name: "worker".to_string(),
            description: "Worker".to_string(),
            instructions: String::new(),
            file_path: None,
            source: AgentSource::Explicit,
            model: None,
            tool_policy: AgentToolPolicy::default(),
            skills: Vec::new(),
            hooks: None,
            background: None,
            initial_prompt: None,
            max_turns: None,
            max_spawn_depth: 0,
            project_instructions: None,
            effort: None,
            diagnostics: Vec::new(),
        };
        agent.tool_policy.permission_mode = Some(AgentPermissionMode::AcceptEdits);
        assert_eq!(
            narrow_permission_mode_for_agent(PermissionMode::Default, Some(&agent)),
            PermissionMode::Default
        );
        assert_eq!(
            narrow_permission_mode_for_agent(PermissionMode::BypassPermissions, Some(&agent)),
            PermissionMode::AcceptEdits
        );

        agent.tool_policy.permission_mode = Some(AgentPermissionMode::Default);
        assert_eq!(
            narrow_permission_mode_for_agent(PermissionMode::AcceptEdits, Some(&agent)),
            PermissionMode::Default
        );
        assert_eq!(
            narrow_permission_mode_for_agent(PermissionMode::DontAsk, Some(&agent)),
            PermissionMode::DontAsk
        );
    }

    #[test]
    fn empty_tools_array_is_explicit_empty_allowlist() {
        let tmp = TempDir::new().expect("tmp");
        let inherit_path = tmp.path().join("inherit.md");
        fs::write(
            &inherit_path,
            "---\nname: inherit\ndescription: Inherit tools\n---\nBody.\n",
        )
        .expect("write inherit");
        let empty_path = tmp.path().join("translate.md");
        fs::write(
            &empty_path,
            "---\nname: translate\ndescription: Translate only\ntools: []\n---\nTranslate.\n",
        )
        .expect("write empty");
        let empty_string_path = tmp.path().join("empty-string.md");
        fs::write(
            &empty_string_path,
            "---\nname: empty-string\ndescription: Empty string inherits\ntools: ''\n---\nBody.\n",
        )
        .expect("write empty string");

        let inherit = parse_agent_file(&inherit_path, AgentSource::Explicit).expect("inherit");
        let empty = parse_agent_file(&empty_path, AgentSource::Explicit).expect("empty");
        let empty_string =
            parse_agent_file(&empty_string_path, AgentSource::Explicit).expect("empty string");

        assert_eq!(inherit.tool_policy.allowed, None);
        assert!(agent_allows_tool("read", Some(&inherit), RunMode::Build));
        assert_eq!(empty.tool_policy.allowed, Some(BTreeSet::new()));
        for name in [
            "read",
            "write",
            "bash",
            "Agent",
            "list_skills",
            "view_skill",
        ] {
            assert!(
                !agent_allows_tool(name, Some(&empty), RunMode::Build),
                "{name} should be blocked"
            );
        }
        assert_eq!(empty_string.tool_policy.allowed, None);
        assert!(agent_allows_tool(
            "read",
            Some(&empty_string),
            RunMode::Build
        ));
    }

    #[test]
    fn project_instructions_policy_parses_boolean_and_defaults_to_injected() {
        let tmp = TempDir::new().expect("tmp");
        let omitted_path = tmp.path().join("omitted.md");
        fs::write(
            &omitted_path,
            "---\nname: omitted\ndescription: Omitted policy\n---\nBody.\n",
        )
        .expect("write omitted");
        let null_path = tmp.path().join("null.md");
        fs::write(
            &null_path,
            "---\nname: null-agent\ndescription: Null policy\nprojectInstructions: null\n---\nBody.\n",
        )
        .expect("write null");
        let false_path = tmp.path().join("false.md");
        fs::write(
            &false_path,
            "---\nname: no-project\ndescription: No project instructions\nprojectInstructions: false\n---\nBody.\n",
        )
        .expect("write false");
        let invalid_path = tmp.path().join("invalid.md");
        fs::write(
            &invalid_path,
            "---\nname: invalid\ndescription: Invalid policy\nprojectInstructions: off\n---\nBody.\n",
        )
        .expect("write invalid");

        let omitted = parse_agent_file(&omitted_path, AgentSource::Explicit).expect("omitted");
        let null = parse_agent_file(&null_path, AgentSource::Explicit).expect("null");
        let disabled = parse_agent_file(&false_path, AgentSource::Explicit).expect("false");
        let invalid = parse_agent_file(&invalid_path, AgentSource::Explicit).expect("invalid");

        assert_eq!(omitted.project_instructions, None);
        assert!(agent_project_instructions_enabled(Some(&omitted)));
        assert_eq!(null.project_instructions, None);
        assert!(agent_project_instructions_enabled(Some(&null)));
        assert_eq!(disabled.project_instructions, Some(false));
        assert!(!agent_project_instructions_enabled(Some(&disabled)));
        assert_eq!(invalid.project_instructions, None);
        assert!(agent_project_instructions_enabled(Some(&invalid)));
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("projectInstructions"))
        );
        assert_eq!(
            view_agent_value(&disabled)["effective_policy"]["project_instructions"]["visible"],
            false
        );
    }

    #[test]
    fn empty_tools_suppresses_agent_and_skill_prompt_catalogs() {
        let agent = built_in_agent(
            "translate",
            "Translate only",
            "Translate.",
            Some(BTreeSet::new()),
        );
        let worker = built_in_agent("worker", "Worker", "Work.", None);
        let skill = crate::skills::Skill {
            name: "reviewer".to_string(),
            description: "Review code".to_string(),
            file_path: PathBuf::from("/tmp/reviewer/SKILL.md"),
            base_dir: PathBuf::from("/tmp/reviewer"),
            source: crate::skills::SkillSource::Project,
            disable_model_invocation: false,
        };
        let tools = apply_agent_tool_policy(
            vec![
                test_tool("Agent"),
                test_tool("list_skills"),
                test_tool("view_skill"),
            ],
            Some(&agent),
            RunMode::Build,
        );

        let prompt_agents = agent_catalog_for_prompt(&[worker], Some(&agent), &tools);
        let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
            vec![skill]
        } else {
            Vec::new()
        };
        let assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
            RunMode::Build,
            Some(&agent),
            &prompt_agents,
            &prompt_skills,
            &[],
            &Default::default(),
            !tools.is_empty(),
        );

        assert!(tools.is_empty());
        assert!(prompt_agents.is_empty());
        assert!(prompt_skills.is_empty());
        assert!(
            !assembly
                .prefix_slots
                .iter()
                .any(|slot| slot.slot == "agent_catalog")
        );
        assert!(
            !assembly
                .prefix_slots
                .iter()
                .any(|slot| slot.slot == "skill_index")
        );
        assert!(
            assembly.prefix_slots[0]
                .content
                .contains("No callable tools are available")
        );
        assert!(!assembly.prefix_slots[0].content.contains("read, edit"));
    }

    #[test]
    fn project_instructions_are_developer_prompt_slots_with_system_fallback() {
        let fragment = crate::project_instructions::ProjectInstructionFragment {
            source_name: "AGENTS.md".to_string(),
            source_path: PathBuf::from("/tmp/repo/AGENTS.md"),
            directory: PathBuf::from("/tmp/repo"),
            content:
                "# AGENTS.md instructions for /tmp/repo\n\n<INSTRUCTIONS>\nUse Chinese.\n</INSTRUCTIONS>"
                    .to_string(),
            truncated: false,
            original_bytes: 12,
            included_bytes: 12,
        };
        let developer_caps = crate::types::ModelCapabilities {
            developer_role: Some(true),
            ..Default::default()
        };
        let developer_assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
            RunMode::Build,
            None,
            &[],
            &[],
            std::slice::from_ref(&fragment),
            &developer_caps,
            true,
        );
        assert!(
            developer_assembly
                .prefix_contextual_user_messages
                .is_empty()
        );
        let project_slot = developer_assembly
            .prefix_slots
            .iter()
            .find(|slot| slot.source_kind.as_deref() == Some("project_instruction"))
            .expect("project slot");
        assert_eq!(project_slot.provider_role, "developer");
        assert_eq!(project_slot.semantic_role, "developer_prompt");
        assert!(
            project_slot
                .content
                .contains("policy context, not user task content")
        );

        let fallback_assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
            RunMode::Build,
            None,
            &[],
            &[],
            &[fragment],
            &Default::default(),
            true,
        );
        let fallback_slot = fallback_assembly
            .prefix_slots
            .iter()
            .find(|slot| slot.source_kind.as_deref() == Some("project_instruction"))
            .expect("fallback project slot");
        assert_eq!(fallback_slot.provider_role, "system");
    }

    #[tokio::test]
    async fn agent_name_allowlist_filters_prompt_catalog_and_spawn() {
        let tmp = TempDir::new().expect("tmp");
        let path = tmp.path().join("coordinator.md");
        fs::write(
            &path,
            r#"---
name: coordinator
description: Coordinate selected agents
tools: Agent(worker, researcher)
---
Coordinate.
"#,
        )
        .expect("write");
        let coordinator = parse_agent_file(&path, AgentSource::Explicit).expect("coordinator");
        let worker = built_in_agent("worker", "Worker", "Work.", None);
        let researcher = built_in_agent("researcher", "Researcher", "Research.", None);
        let explore = built_in_agent("explore-extra", "Explore", "Explore.", None);
        let catalog = AgentCatalog {
            agents: vec![worker, researcher, explore],
            shadowed_agents: Vec::new(),
            diagnostics: Vec::new(),
        };

        let tools = apply_agent_tool_policy(
            vec![test_tool("Agent"), test_tool("list_agents")],
            Some(&coordinator),
            RunMode::Build,
        );
        let visible = agent_catalog_for_prompt(&catalog.agents, Some(&coordinator), &tools)
            .into_iter()
            .map(|agent| agent.name)
            .collect::<Vec<_>>();
        assert_eq!(visible, vec!["worker", "researcher"]);

        let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
        let parent = store
            .create_session_with_metadata(tmp.path(), "test", "model", "provider", None)
            .expect("parent");
        let (_tx, rx) = watch::channel(false);
        let err = spawn_subagent(
            AgentToolContext {
                provider: Arc::new(FakeProvider::new(Vec::new())),
                model_provider: "provider".to_string(),
                model: "model".to_string(),
                provider_label: "provider".to_string(),
                base_url: "http://127.0.0.1:9/v1".to_string(),
                api_key_env: None,
                reasoning_effort: None,
                context_limit: None,
                generation_metadata: json!({}),
                workdir: tmp.path().to_path_buf(),
                mode: RunMode::Build,
                permission_config: PermissionConfig::default(),
                permission_mode: PermissionMode::Default,
                approval_mode: ApprovalMode::Manual,
                approval_handler: None,
                store,
                parent_session_id: parent,
                parent_context_snapshot: Vec::new(),
                catalog,
                control_handle: None,
                stream_events: None,
                model_metadata: ModelMetadata::default(),
                env: BTreeMap::new(),
                allowed_agent_names: coordinator.tool_policy.allowed_agents.clone(),
                denied_agent_names: coordinator.tool_policy.denied_agents.clone(),
                required_agent_names: Vec::new(),
                spawn_depth_remaining: None,
            },
            AgentToolArgs {
                agent_type: Some("explore-extra".to_string()),
                name: None,
                prompt: "explore".to_string(),
                task_name: None,
                background: None,
                model: None,
                fork_context: false,
                fork_turns: None,
                max_turns: None,
                max_spawn_depth: None,
            },
            "call".to_string(),
            AbortSignal::new(rx),
        )
        .await
        .expect_err("unlisted agent should be rejected");
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn skill_alias_controls_read_only_skill_surface() {
        let tmp = TempDir::new().expect("tmp");
        let allow_path = tmp.path().join("skill-reader.md");
        fs::write(
            &allow_path,
            "---\nname: skill-reader\ndescription: Skill reader\ntools: Skill\n---\nRead skills.\n",
        )
        .expect("write allow");
        let deny_path = tmp.path().join("no-skill-reader.md");
        fs::write(
            &deny_path,
            "---\nname: no-skill-reader\ndescription: No skill read\ndisallowedTools: Skill\n---\nNo skills.\n",
        )
        .expect("write deny");

        let allow = parse_agent_file(&allow_path, AgentSource::Explicit).expect("allow");
        let deny = parse_agent_file(&deny_path, AgentSource::Explicit).expect("deny");
        let skill_tools = vec![
            test_tool("list_skills"),
            test_tool("view_skill"),
            test_tool("create_skill"),
        ];
        let allowed = apply_agent_tool_policy(skill_tools.clone(), Some(&allow), RunMode::Build)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(allowed, vec!["list_skills", "view_skill"]);
        assert!(agent_policy_allows_skill_catalog(&allow));

        let denied = apply_agent_tool_policy(skill_tools, Some(&deny), RunMode::Build)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(denied, vec!["create_skill"]);
        assert!(!skill_catalog_visible_for_tools(&[test_tool(
            "create_skill"
        )]));
        assert!(!agent_policy_allows_skill_catalog(&deny));
    }

    #[test]
    fn unknown_tool_names_are_preserved_with_diagnostics() {
        let tmp = TempDir::new().expect("tmp");
        let path = tmp.path().join("external.md");
        fs::write(
            &path,
            "---\nname: external\ndescription: External tool\ntools: custom_tool\n---\nUse external tool.\n",
        )
        .expect("write");

        let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");

        assert!(
            agent
                .tool_policy
                .allowed
                .as_ref()
                .expect("allowed")
                .contains("custom_tool")
        );
        assert!(
            agent
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("custom_tool"))
        );
    }

    #[test]
    fn recursively_discovers_agent_markdown_files() {
        let tmp = TempDir::new().expect("tmp");
        let home = tmp.path().join("home");
        let workdir = tmp.path().join("repo");
        fs::create_dir_all(workdir.join(".psychevo/agents/nested")).expect("dirs");
        fs::write(
            workdir.join(".psychevo/agents/nested/reviewer.md"),
            "---\ndescription: Nested reviewer\n---\nReview.",
        )
        .expect("write");

        let catalog = discover_agents(&AgentDiscoveryOptions {
            home,
            workdir: workdir.clone(),
            env: env(tmp.path()),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })
        .expect("catalog");
        assert!(catalog.agents.iter().any(|agent| agent.name == "reviewer"));
    }

    #[test]
    fn project_agent_wins_over_built_in() {
        let tmp = TempDir::new().expect("tmp");
        let home = tmp.path().join("home");
        let workdir = tmp.path().join("repo");
        fs::create_dir_all(workdir.join(".psychevo/agents")).expect("dirs");
        fs::write(
            workdir.join(".psychevo/agents/general.md"),
            "---\ndescription: Project general\n---\nProject instructions.",
        )
        .expect("write");
        let catalog = discover_agents(&AgentDiscoveryOptions {
            home,
            workdir: workdir.clone(),
            env: env(tmp.path()),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })
        .expect("catalog");
        let general = catalog
            .agents
            .iter()
            .find(|agent| agent.name == "general")
            .expect("general");
        assert_eq!(general.source, AgentSource::Project);
    }

    #[test]
    fn selected_agent_instruction_includes_description_and_body() {
        let agent = built_in_agent(
            "translate",
            "Detect the source language automatically.",
            "Preserve punctuation and return only the translation.",
            None,
        );

        let main = format_selected_agent_instruction(&agent, AgentInvocationRole::Main);
        assert!(main.contains("Main session agent: translate"));
        assert!(main.contains("Purpose:\nDetect the source language automatically."));
        assert!(main.contains("Instructions:\nPreserve punctuation"));

        let child = format_selected_agent_instruction(&agent, AgentInvocationRole::Subagent);
        assert!(child.contains("Child agent: translate"));
        assert!(child.contains("Purpose:\nDetect the source language automatically."));
    }

    #[test]
    fn duplicate_agents_are_available_as_shadowed_definitions() {
        let tmp = TempDir::new().expect("tmp");
        let home = tmp.path().join("home");
        let workdir = tmp.path().join("repo");
        fs::create_dir_all(workdir.join(".psychevo/agents")).expect("project dirs");
        fs::create_dir_all(home.join("agents")).expect("global dirs");
        fs::write(
            workdir.join(".psychevo/agents/review.md"),
            "---\ndescription: Project review\n---\nProject instructions.",
        )
        .expect("write project");
        fs::write(
            home.join("agents/review.md"),
            "---\ndescription: Global review\n---\nGlobal instructions.",
        )
        .expect("write global");

        let catalog = discover_agents(&AgentDiscoveryOptions {
            home,
            workdir: workdir.clone(),
            env: env(tmp.path()),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })
        .expect("catalog");

        let active = catalog
            .agents
            .iter()
            .find(|agent| agent.name == "review")
            .expect("active review");
        assert_eq!(active.source, AgentSource::Project);
        let shadowed = catalog
            .shadowed_agents
            .iter()
            .find(|agent| agent.name == "review")
            .expect("shadowed review");
        assert_eq!(shadowed.source, AgentSource::Global);
        assert!(
            catalog
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == "collision")
        );
    }

    #[test]
    fn agent_tool_name_alias_and_conflict_resolution() {
        let args = AgentToolArgs {
            agent_type: None,
            name: Some("translate".to_string()),
            prompt: "translate this".to_string(),
            task_name: None,
            background: None,
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        };
        assert_eq!(
            resolve_agent_tool_name(&args, &[]).expect("alias"),
            "translate"
        );

        let args = AgentToolArgs {
            agent_type: Some("general".to_string()),
            name: Some("translate".to_string()),
            prompt: "translate this".to_string(),
            task_name: None,
            background: None,
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        };
        let err = resolve_agent_tool_name(&args, &[]).expect_err("conflict");
        assert!(err.to_string().contains("conflict"));
    }

    #[test]
    fn required_agent_mention_supplies_omitted_agent_type() {
        let args = AgentToolArgs {
            agent_type: None,
            name: None,
            prompt: "translate this".to_string(),
            task_name: None,
            background: None,
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        };

        assert_eq!(
            resolve_agent_tool_name(&args, &["translate".to_string()]).expect("required"),
            "translate"
        );
        assert_eq!(
            resolve_agent_tool_name(&args, &[]).expect("default"),
            "general"
        );
        let err = resolve_agent_tool_name(&args, &["translate".to_string(), "review".to_string()])
            .expect_err("ambiguous");
        assert!(err.to_string().contains("multiple agents"));
    }

    #[test]
    fn max_spawn_depth_defaults_to_leaf_and_decrements() {
        assert_eq!(resolved_child_spawn_depth_remaining(None, 0, None), 0);
        assert_eq!(resolved_child_spawn_depth_remaining(None, 1, None), 1);
        assert_eq!(resolved_child_spawn_depth_remaining(None, 0, Some(1)), 1);
        assert_eq!(resolved_child_spawn_depth_remaining(Some(1), 0, None), 0);
        assert_eq!(resolved_child_spawn_depth_remaining(Some(1), 1, None), 0);
        assert_eq!(
            resolved_child_spawn_depth_remaining(
                None,
                MAX_AGENT_SPAWN_DEPTH_CAP.saturating_add(1),
                None
            ),
            MAX_AGENT_SPAWN_DEPTH_CAP
        );
    }

    #[test]
    fn pause_new_spawns_state_is_explicit() {
        set_agent_spawn_paused(false);
        assert!(!agent_spawn_paused());
        let previous = set_agent_spawn_paused(true);
        assert!(!previous);
        assert!(agent_spawn_paused());
        set_agent_spawn_paused(false);
    }

    #[test]
    fn child_session_summary_uses_latest_assistant_usage_tokens() {
        let tmp = TempDir::new().expect("tmp");
        let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
        let session = store
            .create_session_with_metadata(tmp.path(), "agent", "mock-model", "mock", None)
            .expect("session");
        for (text, total) in [("first", 10), ("latest", 25)] {
            store
                .append_message_with_metrics(
                    &session,
                    &Message::Assistant {
                        content: vec![AssistantBlock::Text {
                            text: text.to_string(),
                        }],
                        timestamp_ms: now_ms(),
                        finish_reason: Some("stop".to_string()),
                        outcome: Outcome::Normal,
                        model: Some("mock-model".to_string()),
                        provider: Some("mock".to_string()),
                    },
                    Some(json!({"total_tokens": total})),
                    None,
                )
                .expect("assistant message");
        }
        let summary = store
            .session_summary(&session)
            .expect("summary")
            .expect("session summary");
        let value = agent_child_session_summary_value(&store, &summary);

        assert_eq!(value["latest_total_tokens"], 25);
        assert_eq!(value["latest_usage"]["total_tokens"], 25);
    }

    #[test]
    fn stop_agent_with_grace_marks_live_run_interrupted() {
        let id = format!("test-stop-{}-{:?}", now_ms(), std::thread::current().id());
        let (control, _receivers) = ControlHandle::new();
        {
            let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
            runs.insert(
                id.clone(),
                AgentRunState {
                    record: AgentRunRecord {
                        id: id.clone(),
                        task_name: Some("test-stop".to_string()),
                        agent_name: "general".to_string(),
                        task: "stop gracefully".to_string(),
                        parent_session_id: "parent".to_string(),
                        child_session_id: Some("child".to_string()),
                        role: AgentInvocationRole::Subagent,
                        background: true,
                        status: AgentRunStatus::Running,
                        edge_status: Some(AgentEdgeStatus::Open),
                        started_at_ms: now_ms(),
                        ended_at_ms: None,
                        outcome: None,
                        final_answer: None,
                        error: None,
                        effective_max_spawn_depth: Some(0),
                    },
                    control: Some(control),
                },
            );
        }

        let previous = stop_agent_id_with_grace(&id, None, Duration::ZERO)
            .expect("stop")
            .expect("previous record");
        assert_eq!(previous.status, AgentRunStatus::Running);

        let record = {
            let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
            runs.remove(&id).expect("run state").record
        };
        assert_eq!(record.status, AgentRunStatus::Interrupted);
        assert_eq!(record.edge_status, Some(AgentEdgeStatus::Closed));
        assert_eq!(record.outcome.as_deref(), Some("interrupted"));
        assert!(record.ended_at_ms.is_some());
    }

    #[test]
    fn pre_tool_hook_exit_two_blocks_with_stderr() {
        let tmp = TempDir::new().expect("tmp");
        let hooks = json!({
            "PreToolUse": ["cat >/dev/null; echo blocked >&2; exit 2"]
        });
        let blocked = run_hook_commands(
            Some(&hooks),
            "PreToolUse",
            tmp.path(),
            &json!({"tool": "read"}),
        );
        assert_eq!(blocked.as_deref(), Some("blocked"));
    }

    #[test]
    fn mcp_server_scope_filters_canonical_mcp_tools() {
        let agent = built_in_agent("mcp-test", "MCP test", "Test", None);
        let mut agent = agent;
        agent.tool_policy.mcp_servers = ["repo".to_string()].into_iter().collect();
        assert!(agent_allows_tool(
            "mcp:repo:read",
            Some(&agent),
            RunMode::Build
        ));
        assert!(!agent_allows_tool(
            "mcp:other:read",
            Some(&agent),
            RunMode::Build
        ));
    }
}
