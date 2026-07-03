use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use futures::future::BoxFuture;
use http::{HeaderName, HeaderValue};
use psychevo_agent_core::{
    ToolBinding, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolOutput,
};
use psychevo_ai::AbortSignal;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, GetPromptRequestParams, ReadResourceRequestParams,
};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{Peer, RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::permissions::PermissionRuntime;
use crate::types::{McpServerInput, McpTransportInput, RunWarning};

const LIST_MCP_RESOURCES_TOOL: &str = "list_mcp_resources";
const LIST_MCP_RESOURCE_TEMPLATES_TOOL: &str = "list_mcp_resource_templates";
const READ_MCP_RESOURCE_TOOL: &str = "read_mcp_resource";
const LIST_MCP_PROMPTS_TOOL: &str = "list_mcp_prompts";
const GET_MCP_PROMPT_TOOL: &str = "get_mcp_prompt";

const MCP_TOOL_NAME_DELIMITER: &str = "__";
const MAX_TOOL_NAME_LENGTH: usize = 64;
const HASH_SUFFIX_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum McpSourceTier {
    Plugin = 0,
    SelectedCapabilityRoot = 1,
    Profile = 2,
    Session = 3,
    Extension = 4,
}

impl McpSourceTier {
    fn for_input(input: &McpServerInput) -> Self {
        match input.source_kind.as_deref() {
            Some("plugin") => Self::Plugin,
            Some("selected_capability_root" | "capability_root") => Self::SelectedCapabilityRoot,
            Some("profile") => Self::Profile,
            Some("extension") => Self::Extension,
            Some("session" | "acp" | "run_option") | None => Self::Session,
            Some(_) => Self::Session,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct McpCatalogEntry {
    pub(crate) normalized_name: String,
    pub(crate) source_id: String,
    pub(crate) source_kind: String,
    pub(crate) tier: McpSourceTier,
    pub(crate) input: McpServerInput,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct McpSourceCatalog {
    pub(crate) entries: Vec<McpCatalogEntry>,
    pub(crate) warnings: Vec<RunWarning>,
}

impl McpSourceCatalog {
    pub(crate) fn resolve(inputs: &[McpServerInput]) -> Self {
        let mut accepted = BTreeMap::<String, McpCatalogEntry>::new();
        let mut warnings = Vec::new();

        for input in inputs {
            let normalized_name = normalize_mcp_server_name(&input.name);
            let tier = McpSourceTier::for_input(input);
            let source_kind = input
                .source_kind
                .clone()
                .unwrap_or_else(|| "session".to_string());
            let source_id = input
                .source_id
                .clone()
                .unwrap_or_else(|| format!("{source_kind}:mcp:{}", input.name));
            let candidate = McpCatalogEntry {
                normalized_name: normalized_name.clone(),
                source_id,
                source_kind,
                tier,
                input: input.clone(),
            };

            match accepted.get(&normalized_name) {
                Some(existing) if existing.tier > candidate.tier => {
                    warnings.push(mcp_warning(format!(
                        "MCP server `{}` from `{}` omitted because `{}` from `{}` has higher precedence",
                        input.name, candidate.source_id, existing.input.name, existing.source_id
                    )));
                }
                Some(existing) if existing.tier == candidate.tier => {
                    warnings.push(mcp_warning(format!(
                        "MCP server `{}` from `{}` conflicts with `{}` from `{}` at the same precedence; omitted",
                        input.name, candidate.source_id, existing.input.name, existing.source_id
                    )));
                }
                Some(existing) => {
                    warnings.push(mcp_warning(format!(
                        "MCP server `{}` from `{}` replaces lower-precedence `{}` from `{}`",
                        input.name, candidate.source_id, existing.input.name, existing.source_id
                    )));
                    accepted.insert(normalized_name, candidate);
                }
                None => {
                    accepted.insert(normalized_name, candidate);
                }
            }
        }

        Self {
            entries: accepted.into_values().collect(),
            warnings,
        }
    }

    pub(crate) fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        for entry in &self.entries {
            hasher.update(entry.normalized_name.as_bytes());
            hasher.update([0]);
            hasher.update(entry.source_id.as_bytes());
            hasher.update([0]);
            hasher.update(entry.source_kind.as_bytes());
            hasher.update([0]);
            hasher.update(mcp_transport_kind(&entry.input.transport).as_bytes());
            hasher.update([0]);
        }
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct McpSamplingConfig {
    pub(crate) enabled: bool,
    pub(crate) timeout_secs: u64,
    pub(crate) max_tokens: u32,
    pub(crate) max_tool_rounds: u32,
    pub(crate) max_requests_per_minute: u32,
    pub(crate) model: Option<String>,
    pub(crate) allowed_models: Vec<String>,
}

impl McpSamplingConfig {
    pub(crate) fn bounded_default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 60,
            max_tokens: 1024,
            max_tool_rounds: 2,
            max_requests_per_minute: 12,
            model: None,
            allowed_models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct McpElicitationPolicy {
    pub(crate) supports_form: bool,
    pub(crate) supports_url: bool,
    pub(crate) timeout_secs: u64,
    pub(crate) auto_accept_empty_confirmation: bool,
}

impl McpElicitationPolicy {
    pub(crate) fn default_form_and_url() -> Self {
        Self {
            supports_form: true,
            supports_url: true,
            timeout_secs: 300,
            auto_accept_empty_confirmation: true,
        }
    }
}

#[derive(Clone)]
pub(crate) struct McpRuntimeSnapshot {
    pub(crate) tools: Vec<Arc<dyn ToolBinding>>,
    pub(crate) warnings: Vec<RunWarning>,
    pub(crate) snapshot_hash: String,
    pub(crate) catalog_hash: String,
    pub(crate) accepted_servers: Vec<String>,
    pub(crate) resources_available: bool,
    pub(crate) prompts_available: bool,
    pub(crate) sampling_config: McpSamplingConfig,
    pub(crate) elicitation_policy: McpElicitationPolicy,
}

pub(crate) async fn mcp_tool_bindings(
    inputs: &[McpServerInput],
    cwd: &Path,
    permission_runtime: Option<&PermissionRuntime>,
) -> (Vec<Arc<dyn ToolBinding>>, Vec<RunWarning>) {
    let snapshot = mcp_runtime_snapshot(inputs, cwd, permission_runtime).await;
    (snapshot.tools, snapshot.warnings)
}

pub(crate) async fn mcp_runtime_snapshot(
    inputs: &[McpServerInput],
    cwd: &Path,
    permission_runtime: Option<&PermissionRuntime>,
) -> McpRuntimeSnapshot {
    let catalog = McpSourceCatalog::resolve(inputs);
    let mut tools = Vec::<Arc<dyn ToolBinding>>::new();
    let mut warnings = catalog.warnings.clone();
    let mut connections = BTreeMap::<String, Arc<McpConnection>>::new();
    let mut tool_candidates = Vec::<McpToolCandidate>::new();
    let mut accepted_servers = Vec::new();
    let mut resources_available = false;
    let mut prompts_available = false;
    let sampling_config = McpSamplingConfig::bounded_default();
    let elicitation_policy = McpElicitationPolicy::default_form_and_url();

    for entry in &catalog.entries {
        let server_name = entry.normalized_name.clone();
        if let Some(permission_runtime) = permission_runtime
            && let Err(err) = permission_runtime
                .authorize_mcp_startup(&server_name, mcp_transport_kind(&entry.input.transport))
                .await
        {
            warnings.push(mcp_warning(format!(
                "MCP server `{}` startup omitted: {err}",
                entry.input.name
            )));
            continue;
        }

        let service = match connect_mcp_server(&entry.input, cwd).await {
            Ok(service) => service,
            Err(err) => {
                warnings.push(mcp_warning(format!(
                    "MCP server `{}` is unavailable: {err}",
                    entry.input.name
                )));
                continue;
            }
        };
        let peer = service.peer().clone();
        let connection = Arc::new(McpConnection {
            peer,
            _service: Mutex::new(service),
        });
        accepted_servers.push(server_name.clone());
        connections.insert(server_name.clone(), Arc::clone(&connection));

        let listed = match connection.peer.list_all_tools().await {
            Ok(listed) => listed,
            Err(err) => {
                warnings.push(mcp_warning(format!(
                    "MCP server `{}` did not list tools: {err}",
                    entry.input.name
                )));
                Vec::new()
            }
        };
        resources_available |= connection.peer.list_all_resources().await.is_ok()
            || connection.peer.list_all_resource_templates().await.is_ok();
        prompts_available |= connection.peer.list_all_prompts().await.is_ok();

        for tool in listed {
            let raw_tool_name = tool.name.to_string();
            let title = tool
                .title
                .clone()
                .or_else(|| tool.annotations.as_ref().and_then(|a| a.title.clone()));
            let description = mcp_tool_description(
                &server_name,
                &raw_tool_name,
                title.as_deref(),
                tool.description.as_deref(),
            );
            let raw_identity = format!("{}\0{}\0{}", server_name, raw_tool_name, entry.source_id);
            tool_candidates.push(McpToolCandidate {
                namespace: mcp_tool_namespace(&server_name),
                callable_name: sanitize_mcp_identifier(&raw_tool_name, "tool"),
                raw_identity,
                binding: McpToolBinding {
                    visible_name: String::new(),
                    canonical_namespace: mcp_tool_namespace(&server_name),
                    canonical_name: sanitize_mcp_identifier(&raw_tool_name, "tool"),
                    source_id: entry.source_id.clone(),
                    source_kind: entry.source_kind.clone(),
                    raw_server_name: entry.input.name.clone(),
                    normalized_server_name: server_name.clone(),
                    raw_tool_name,
                    description,
                    parameters: Value::Object((*tool.input_schema).clone()),
                    connection: Arc::clone(&connection),
                },
            });
        }
    }

    for mut binding in normalize_mcp_tool_candidates(tool_candidates, &mut warnings) {
        binding.visible_name =
            mcp_flat_tool_name(&binding.canonical_namespace, &binding.canonical_name);
        tools.push(Arc::new(binding));
    }

    let utility_connection = Arc::new(McpUtilityConnectionSet { connections });
    if resources_available {
        tools.push(Arc::new(McpUtilityTool::list_resources(Arc::clone(
            &utility_connection,
        ))));
        tools.push(Arc::new(McpUtilityTool::list_resource_templates(
            Arc::clone(&utility_connection),
        )));
        tools.push(Arc::new(McpUtilityTool::read_resource(Arc::clone(
            &utility_connection,
        ))));
    }
    if prompts_available {
        tools.push(Arc::new(McpUtilityTool::list_prompts(Arc::clone(
            &utility_connection,
        ))));
        tools.push(Arc::new(McpUtilityTool::get_prompt(utility_connection)));
    }

    let catalog_hash = catalog.hash();
    let snapshot_hash = mcp_snapshot_hash(
        &catalog_hash,
        &accepted_servers,
        resources_available,
        prompts_available,
        &tools,
        &sampling_config,
        &elicitation_policy,
    );

    McpRuntimeSnapshot {
        tools,
        warnings,
        snapshot_hash,
        catalog_hash,
        accepted_servers,
        resources_available,
        prompts_available,
        sampling_config,
        elicitation_policy,
    }
}

pub(crate) fn mcp_transport_kind(transport: &McpTransportInput) -> &'static str {
    match transport {
        McpTransportInput::Stdio { .. } => "stdio",
        McpTransportInput::StreamableHttp { .. } => "streamable_http",
        McpTransportInput::Unsupported { .. } => "unsupported",
    }
}

pub(crate) fn mcp_tool_name_parts(tool_name: &str) -> Option<(&str, &str)> {
    let rest = tool_name.strip_prefix("mcp__")?;
    rest.split_once("__")
}

pub(crate) fn mcp_tool_source_kind(tool_name: &str) -> &'static str {
    if is_mcp_utility_tool(tool_name) {
        "mcp_utility"
    } else {
        "mcp"
    }
}

pub(crate) fn mcp_tool_source_id(tool_name: &str) -> String {
    if is_mcp_utility_tool(tool_name) {
        return "mcp:utility".to_string();
    }
    mcp_tool_name_parts(tool_name)
        .map(|(server, _)| format!("mcp:{server}"))
        .unwrap_or_else(|| "mcp:unknown".to_string())
}

pub(crate) fn is_mcp_utility_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        LIST_MCP_RESOURCES_TOOL
            | LIST_MCP_RESOURCE_TEMPLATES_TOOL
            | READ_MCP_RESOURCE_TOOL
            | LIST_MCP_PROMPTS_TOOL
            | GET_MCP_PROMPT_TOOL
    )
}

pub(crate) fn mcp_utility_action(tool_name: &str, args: &Value) -> Option<(String, String)> {
    let action = match tool_name {
        LIST_MCP_RESOURCES_TOOL => "resources/list",
        LIST_MCP_RESOURCE_TEMPLATES_TOOL => "resource_templates/list",
        READ_MCP_RESOURCE_TOOL => "resources/read",
        LIST_MCP_PROMPTS_TOOL => "prompts/list",
        GET_MCP_PROMPT_TOOL => "prompts/get",
        _ => return None,
    };
    let server = args
        .get("server")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("*");
    Some((server.to_string(), action.to_string()))
}

pub(crate) fn normalize_mcp_server_name(name: &str) -> String {
    sanitize_mcp_identifier(name, "server")
}

pub(crate) fn mcp_tool_namespace(server_name: &str) -> String {
    format!("mcp__{}", sanitize_mcp_identifier(server_name, "server"))
}

#[cfg(test)]
pub(crate) fn mcp_tool_visible_name(server_name: &str, tool_name: &str) -> String {
    mcp_flat_tool_name(
        &mcp_tool_namespace(server_name),
        &sanitize_mcp_identifier(tool_name, "tool"),
    )
}

pub(crate) fn mcp_flat_tool_name(namespace: &str, tool_name: &str) -> String {
    format!("{namespace}{MCP_TOOL_NAME_DELIMITER}{tool_name}")
}

pub(crate) fn sanitize_mcp_identifier(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '_'
        };
        if next == '_' {
            if !previous_underscore {
                out.push(next);
            }
            previous_underscore = true;
        } else {
            out.push(next);
            previous_underscore = false;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

struct McpToolCandidate {
    namespace: String,
    callable_name: String,
    raw_identity: String,
    binding: McpToolBinding,
}

fn normalize_mcp_tool_candidates(
    candidates: Vec<McpToolCandidate>,
    warnings: &mut Vec<RunWarning>,
) -> Vec<McpToolBinding> {
    let mut namespace_identities = BTreeMap::<String, HashSet<String>>::new();
    for candidate in &candidates {
        namespace_identities
            .entry(candidate.namespace.clone())
            .or_default()
            .insert(candidate.binding.source_id.clone());
    }
    let colliding_namespaces = namespace_identities
        .into_iter()
        .filter_map(|(namespace, identities)| (identities.len() > 1).then_some(namespace))
        .collect::<HashSet<_>>();

    let mut raw_seen = HashSet::new();
    let mut adjusted = Vec::new();
    for mut candidate in candidates {
        if !raw_seen.insert(candidate.raw_identity.clone()) {
            warnings.push(mcp_warning(format!(
                "MCP tool `{}/{}` duplicates a raw source identity; omitted",
                candidate.binding.normalized_server_name, candidate.binding.raw_tool_name
            )));
            continue;
        }
        if colliding_namespaces.contains(&candidate.namespace) {
            candidate.namespace = append_hash_suffix(&candidate.namespace, &candidate.raw_identity);
        }
        adjusted.push(candidate);
    }

    let mut name_identities = BTreeMap::<(String, String), HashSet<String>>::new();
    for candidate in &adjusted {
        name_identities
            .entry((candidate.namespace.clone(), candidate.callable_name.clone()))
            .or_default()
            .insert(candidate.raw_identity.clone());
    }
    let colliding_names = name_identities
        .into_iter()
        .filter_map(|(key, identities)| (identities.len() > 1).then_some(key))
        .collect::<HashSet<_>>();

    let mut used = HashSet::new();
    let mut out = Vec::new();
    adjusted.sort_by(|left, right| left.raw_identity.cmp(&right.raw_identity));
    for mut candidate in adjusted {
        if colliding_names.contains(&(candidate.namespace.clone(), candidate.callable_name.clone()))
        {
            candidate.callable_name =
                append_hash_suffix(&candidate.callable_name, &candidate.raw_identity);
        }
        let (namespace, callable_name) = unique_callable_parts(
            &candidate.namespace,
            &candidate.callable_name,
            &candidate.raw_identity,
            &mut used,
        );
        candidate.binding.canonical_namespace = namespace;
        candidate.binding.canonical_name = callable_name;
        out.push(candidate.binding);
    }
    out
}

fn mcp_snapshot_hash(
    catalog_hash: &str,
    accepted_servers: &[String],
    resources_available: bool,
    prompts_available: bool,
    tools: &[Arc<dyn ToolBinding>],
    sampling_config: &McpSamplingConfig,
    elicitation_policy: &McpElicitationPolicy,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(catalog_hash.as_bytes());
    hasher.update([0]);
    hasher.update(if resources_available {
        b"resources:1"
    } else {
        b"resources:0"
    });
    hasher.update([0]);
    hasher.update(if prompts_available {
        b"prompts:1"
    } else {
        b"prompts:0"
    });
    hasher.update([0]);
    hasher.update(serde_json::to_string(sampling_config).unwrap_or_default());
    hasher.update([0]);
    hasher.update(serde_json::to_string(elicitation_policy).unwrap_or_default());
    hasher.update([0]);
    for server in accepted_servers {
        hasher.update(server.as_bytes());
        hasher.update([0]);
    }
    for tool in tools {
        hasher.update(tool.name().as_bytes());
        hasher.update([0]);
        hasher.update(tool.canonical_tool_name().name.as_bytes());
        hasher.update([0]);
        if let Some(namespace) = tool.canonical_tool_name().namespace {
            hasher.update(namespace.as_bytes());
        }
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn hash_suffix(raw_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_identity.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("_{}", &digest[..HASH_SUFFIX_LEN])
}

fn append_hash_suffix(value: &str, raw_identity: &str) -> String {
    format!("{value}{}", hash_suffix(raw_identity))
}

fn truncate_name(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect()
}

fn fit_callable_parts_with_hash(
    namespace: &str,
    tool_name: &str,
    raw_identity: &str,
) -> (String, String) {
    let suffix = hash_suffix(raw_identity);
    let reserved_len = MCP_TOOL_NAME_DELIMITER.len();
    let max_tool_len = MAX_TOOL_NAME_LENGTH.saturating_sub(namespace.len() + reserved_len);
    if max_tool_len >= suffix.len() {
        let prefix_len = max_tool_len - suffix.len();
        return (
            namespace.to_string(),
            format!("{}{}", truncate_name(tool_name, prefix_len), suffix),
        );
    }

    let max_namespace_len = MAX_TOOL_NAME_LENGTH.saturating_sub(suffix.len() + reserved_len);
    (truncate_name(namespace, max_namespace_len), suffix)
}

fn unique_callable_parts(
    namespace: &str,
    tool_name: &str,
    raw_identity: &str,
    used_names: &mut HashSet<String>,
) -> (String, String) {
    let fallback = mcp_flat_tool_name(namespace, tool_name);
    if fallback.len() <= MAX_TOOL_NAME_LENGTH && used_names.insert(fallback) {
        return (namespace.to_string(), tool_name.to_string());
    }

    let mut attempt = 0_u32;
    loop {
        let hash_input = if attempt == 0 {
            raw_identity.to_string()
        } else {
            format!("{raw_identity}\0{attempt}")
        };
        let (namespace, tool_name) =
            fit_callable_parts_with_hash(namespace, tool_name, &hash_input);
        let fallback = mcp_flat_tool_name(&namespace, &tool_name);
        if used_names.insert(fallback) {
            return (namespace, tool_name);
        }
        attempt = attempt.saturating_add(1);
    }
}

pub(crate) async fn connect_mcp_server(
    input: &McpServerInput,
    cwd: &Path,
) -> Result<RunningService<RoleClient, ()>, String> {
    match &input.transport {
        McpTransportInput::Stdio {
            command,
            args,
            env,
            cwd: server_cwd,
        } => {
            let mut cmd = Command::new(command);
            cmd.args(args)
                .envs(env)
                .current_dir(server_cwd.as_deref().unwrap_or(cwd));
            let transport = TokioChildProcess::new(cmd).map_err(|err| err.to_string())?;
            ().serve(transport).await.map_err(|err| err.to_string())
        }
        McpTransportInput::StreamableHttp { url, headers } => {
            let mut parsed_headers = HashMap::new();
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|err| format!("invalid HTTP header `{name}`: {err}"))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|err| format!("invalid HTTP header value for `{name}`: {err}"))?;
                parsed_headers.insert(name, value);
            }
            let config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .custom_headers(parsed_headers);
            let transport = StreamableHttpClientTransport::from_config(config);
            ().serve(transport).await.map_err(|err| err.to_string())
        }
        McpTransportInput::Unsupported { kind } => Err(format!("unsupported transport `{kind}`")),
    }
}

pub(crate) fn mcp_tool_description(
    server_name: &str,
    raw_tool_name: &str,
    title: Option<&str>,
    description: Option<&str>,
) -> String {
    let mut out = format!("MCP tool `{server_name}/{raw_tool_name}`.");
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        out.push(' ');
        out.push_str(title.trim());
        out.push('.');
    }
    if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
        out.push(' ');
        out.push_str(description.trim());
    }
    out
}

pub(crate) fn mcp_warning(message: String) -> RunWarning {
    RunWarning {
        kind: "mcp".to_string(),
        message,
        source_path: None,
        suggestion: None,
    }
}

pub(crate) struct McpConnection {
    pub(crate) peer: Peer<RoleClient>,
    pub(crate) _service: Mutex<RunningService<RoleClient, ()>>,
}

pub(crate) struct McpUtilityConnectionSet {
    connections: BTreeMap<String, Arc<McpConnection>>,
}

impl McpUtilityConnectionSet {
    fn peers_for_optional_server(&self, server: Option<&str>) -> Vec<(String, Peer<RoleClient>)> {
        match server.map(normalize_mcp_server_name) {
            Some(server) => self
                .connections
                .get(&server)
                .map(|connection| vec![(server, connection.peer.clone())])
                .unwrap_or_default(),
            None => self
                .connections
                .iter()
                .map(|(server, connection)| (server.clone(), connection.peer.clone()))
                .collect(),
        }
    }

    fn peer_for_required_server(&self, server: &str) -> Option<(String, Peer<RoleClient>)> {
        let normalized = normalize_mcp_server_name(server);
        self.connections
            .get(&normalized)
            .map(|connection| (normalized, connection.peer.clone()))
    }
}

#[derive(Debug, Clone, Copy)]
enum McpUtilityKind {
    ListResources,
    ListResourceTemplates,
    ReadResource,
    ListPrompts,
    GetPrompt,
}

pub(crate) struct McpUtilityTool {
    kind: McpUtilityKind,
    name: &'static str,
    description: &'static str,
    connection_set: Arc<McpUtilityConnectionSet>,
}

impl McpUtilityTool {
    fn list_resources(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::ListResources,
            name: LIST_MCP_RESOURCES_TOOL,
            description: "List MCP resources from one server or all accepted MCP servers.",
            connection_set,
        }
    }

    fn list_resource_templates(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::ListResourceTemplates,
            name: LIST_MCP_RESOURCE_TEMPLATES_TOOL,
            description: "List MCP resource templates from one server or all accepted MCP servers.",
            connection_set,
        }
    }

    fn read_resource(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::ReadResource,
            name: READ_MCP_RESOURCE_TOOL,
            description: "Read one MCP resource by server and URI.",
            connection_set,
        }
    }

    fn list_prompts(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::ListPrompts,
            name: LIST_MCP_PROMPTS_TOOL,
            description: "List MCP prompts from one server or all accepted MCP servers.",
            connection_set,
        }
    }

    fn get_prompt(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::GetPrompt,
            name: GET_MCP_PROMPT_TOOL,
            description: "Get one MCP prompt by server, name, and optional arguments.",
            connection_set,
        }
    }
}

impl ToolBinding for McpUtilityTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters(&self) -> Value {
        mcp_utility_parameters(self.kind)
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec {
            category: ToolDisplayCategory::Explore,
            title_arg_keys: vec!["server".to_string(), "uri".to_string(), "name".to_string()],
            title_result_keys: vec!["server".to_string(), "uri".to_string(), "name".to_string()],
            summary_keys: vec![
                "server".to_string(),
                "uri".to_string(),
                "name".to_string(),
                "is_error".to_string(),
            ],
            body_keys: vec![
                "resources".to_string(),
                "resource_templates".to_string(),
                "prompts".to_string(),
                "contents".to_string(),
                "messages".to_string(),
            ],
            body_policy: ToolDisplayBodyPolicy::Body,
        }
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let kind = self.kind;
        let connection_set = Arc::clone(&self.connection_set);
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("MCP utility call was aborted before dispatch");
            }
            execute_mcp_utility(kind, connection_set, args, abort).await
        })
    }
}

fn mcp_utility_parameters(kind: McpUtilityKind) -> Value {
    let server = json!({
        "type": "string",
        "description": "Normalized or raw MCP server name. Omit only for list operations that should query every accepted server."
    });
    match kind {
        McpUtilityKind::ListResources
        | McpUtilityKind::ListResourceTemplates
        | McpUtilityKind::ListPrompts => json!({
            "type": "object",
            "properties": {
                "server": server,
            },
            "additionalProperties": false
        }),
        McpUtilityKind::ReadResource => json!({
            "type": "object",
            "properties": {
                "server": server,
                "uri": {
                    "type": "string",
                    "description": "MCP resource URI to read."
                }
            },
            "required": ["server", "uri"],
            "additionalProperties": false
        }),
        McpUtilityKind::GetPrompt => json!({
            "type": "object",
            "properties": {
                "server": server,
                "name": {
                    "type": "string",
                    "description": "MCP prompt name to fetch."
                },
                "arguments": {
                    "type": "object",
                    "description": "Optional MCP prompt arguments."
                }
            },
            "required": ["server", "name"],
            "additionalProperties": false
        }),
    }
}

async fn execute_mcp_utility(
    kind: McpUtilityKind,
    connection_set: Arc<McpUtilityConnectionSet>,
    args: Value,
    mut abort: AbortSignal,
) -> ToolOutput {
    let object = match args {
        Value::Object(object) => object,
        Value::Null => serde_json::Map::new(),
        other => {
            return ToolOutput::error(format!("MCP utility expects object arguments, got {other}"));
        }
    };
    let server = object.get("server").and_then(Value::as_str);
    match kind {
        McpUtilityKind::ListResources => {
            let mut resources = Vec::new();
            let mut errors = Vec::new();
            for (server, peer) in connection_set.peers_for_optional_server(server) {
                if abort.aborted() {
                    return ToolOutput::error("MCP resource listing was aborted");
                }
                match peer.list_all_resources().await {
                    Ok(listed) => resources.extend(
                        listed
                            .into_iter()
                            .map(|resource| json!({ "server": server, "resource": resource })),
                    ),
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            utility_list_output("resources", resources, errors)
        }
        McpUtilityKind::ListResourceTemplates => {
            let mut templates = Vec::new();
            let mut errors = Vec::new();
            for (server, peer) in connection_set.peers_for_optional_server(server) {
                if abort.aborted() {
                    return ToolOutput::error("MCP resource template listing was aborted");
                }
                match peer.list_all_resource_templates().await {
                    Ok(listed) => templates.extend(listed.into_iter().map(
                        |template| json!({ "server": server, "resource_template": template }),
                    )),
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            utility_list_output("resource_templates", templates, errors)
        }
        McpUtilityKind::ReadResource => {
            let Some(server) = server else {
                return ToolOutput::error("read_mcp_resource requires server");
            };
            let Some(uri) = object.get("uri").and_then(Value::as_str) else {
                return ToolOutput::error("read_mcp_resource requires uri");
            };
            let Some((server, peer)) = connection_set.peer_for_required_server(server) else {
                return ToolOutput::error(format!("MCP server `{server}` is not available"));
            };
            let request = ReadResourceRequestParams::new(uri.to_string());
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error(format!(
                    "MCP resource `{server}/{uri}` was aborted"
                )),
                result = peer.read_resource(request) => match result {
                    Ok(result) => {
                        let json = json!({
                            "server": server,
                            "uri": uri,
                            "contents": result.contents,
                        });
                        ToolOutput::ok_with_model_content(
                            json.clone(),
                            serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string()),
                        )
                    }
                    Err(err) => ToolOutput::error(format!(
                        "MCP resource `{server}/{uri}` failed: {err}"
                    )),
                },
            }
        }
        McpUtilityKind::ListPrompts => {
            let mut prompts = Vec::new();
            let mut errors = Vec::new();
            for (server, peer) in connection_set.peers_for_optional_server(server) {
                if abort.aborted() {
                    return ToolOutput::error("MCP prompt listing was aborted");
                }
                match peer.list_all_prompts().await {
                    Ok(listed) => prompts.extend(
                        listed
                            .into_iter()
                            .map(|prompt| json!({ "server": server, "prompt": prompt })),
                    ),
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            utility_list_output("prompts", prompts, errors)
        }
        McpUtilityKind::GetPrompt => {
            let Some(server) = server else {
                return ToolOutput::error("get_mcp_prompt requires server");
            };
            let Some(name) = object.get("name").and_then(Value::as_str) else {
                return ToolOutput::error("get_mcp_prompt requires name");
            };
            let Some((server, peer)) = connection_set.peer_for_required_server(server) else {
                return ToolOutput::error(format!("MCP server `{server}` is not available"));
            };
            let mut request = GetPromptRequestParams::new(name.to_string());
            if let Some(arguments) = object.get("arguments").and_then(Value::as_object) {
                request = request.with_arguments(arguments.clone());
            }
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error(format!(
                    "MCP prompt `{server}/{name}` was aborted"
                )),
                result = peer.get_prompt(request) => match result {
                    Ok(result) => {
                        let json = json!({
                            "server": server,
                            "name": name,
                            "description": result.description,
                            "messages": result.messages,
                        });
                        ToolOutput::ok_with_model_content(
                            json.clone(),
                            serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string()),
                        )
                    }
                    Err(err) => ToolOutput::error(format!(
                        "MCP prompt `{server}/{name}` failed: {err}"
                    )),
                },
            }
        }
    }
}

fn utility_list_output(key: &str, items: Vec<Value>, errors: Vec<Value>) -> ToolOutput {
    if items.is_empty() && !errors.is_empty() {
        return ToolOutput::error(json!({ "errors": errors }).to_string());
    }
    let mut object = serde_json::Map::new();
    object.insert(key.to_string(), Value::Array(items));
    object.insert("errors".to_string(), Value::Array(errors));
    ToolOutput::ok(Value::Object(object))
}

pub(crate) struct McpToolBinding {
    pub(crate) visible_name: String,
    pub(crate) canonical_namespace: String,
    pub(crate) canonical_name: String,
    pub(crate) source_id: String,
    pub(crate) source_kind: String,
    pub(crate) raw_server_name: String,
    pub(crate) normalized_server_name: String,
    pub(crate) raw_tool_name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
    pub(crate) connection: Arc<McpConnection>,
}

impl ToolBinding for McpToolBinding {
    fn name(&self) -> &str {
        &self.visible_name
    }

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        psychevo_ai::ToolName::namespaced(
            self.canonical_namespace.clone(),
            self.canonical_name.clone(),
        )
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec {
            category: ToolDisplayCategory::Run,
            title_arg_keys: vec!["name".to_string()],
            title_result_keys: vec!["name".to_string()],
            summary_keys: vec![
                "server".to_string(),
                "tool".to_string(),
                "is_error".to_string(),
            ],
            body_keys: vec!["content".to_string(), "structured_content".to_string()],
            body_policy: ToolDisplayBodyPolicy::Body,
        }
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let server_name = self.normalized_server_name.clone();
        let raw_server_name = self.raw_server_name.clone();
        let raw_tool_name = self.raw_tool_name.clone();
        let provider_name = self.visible_name.clone();
        let canonical_namespace = self.canonical_namespace.clone();
        let canonical_name = self.canonical_name.clone();
        let source_id = self.source_id.clone();
        let source_kind = self.source_kind.clone();
        let peer = self.connection.peer.clone();
        Box::pin(async move {
            let arguments = match args {
                Value::Object(map) => map,
                Value::Null => serde_json::Map::new(),
                other => {
                    return ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` expects object arguments, got {other}"
                    ));
                }
            };
            if abort.aborted() {
                return ToolOutput::error(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` was aborted before dispatch"
                ));
            }
            let request =
                CallToolRequestParams::new(raw_tool_name.clone()).with_arguments(arguments);
            let mut abort = abort;
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` was aborted"
                )),
                result = peer.call_tool(request) => match result {
                    Ok(result) => mcp_tool_output_with_identity(McpToolOutputIdentity {
                        normalized_server_name: server_name.clone(),
                        raw_server_name,
                        raw_tool_name: raw_tool_name.clone(),
                        provider_name,
                        canonical_namespace,
                        canonical_name,
                        source_id,
                        source_kind,
                    }, result),
                    Err(err) => ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` failed: {err}"
                    )),
                },
            }
        })
    }
}

pub(crate) struct McpToolOutputIdentity {
    pub(crate) normalized_server_name: String,
    pub(crate) raw_server_name: String,
    pub(crate) raw_tool_name: String,
    pub(crate) provider_name: String,
    pub(crate) canonical_namespace: String,
    pub(crate) canonical_name: String,
    pub(crate) source_id: String,
    pub(crate) source_kind: String,
}

#[cfg(test)]
pub(crate) fn mcp_tool_output(
    server_name: &str,
    raw_tool_name: &str,
    result: CallToolResult,
) -> ToolOutput {
    mcp_tool_output_with_identity(
        McpToolOutputIdentity {
            normalized_server_name: server_name.to_string(),
            raw_server_name: server_name.to_string(),
            raw_tool_name: raw_tool_name.to_string(),
            provider_name: mcp_tool_visible_name(server_name, raw_tool_name),
            canonical_namespace: mcp_tool_namespace(server_name),
            canonical_name: sanitize_mcp_identifier(raw_tool_name, "tool"),
            source_id: format!("mcp:{server_name}"),
            source_kind: "mcp".to_string(),
        },
        result,
    )
}

pub(crate) fn mcp_tool_output_with_identity(
    identity: McpToolOutputIdentity,
    result: CallToolResult,
) -> ToolOutput {
    let is_error = result.is_error.unwrap_or(false);
    let text_content = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    let model_content = if !text_content.trim().is_empty() {
        text_content
    } else if let Some(structured) = &result.structured_content {
        serde_json::to_string(structured).unwrap_or_else(|_| structured.to_string())
    } else {
        serde_json::to_string(&result.content).unwrap_or_else(|_| String::new())
    };
    ToolOutput {
        json: json!({
            "name": format!("{}/{}", identity.normalized_server_name, identity.raw_tool_name),
            "server": identity.normalized_server_name,
            "raw_server": identity.raw_server_name,
            "tool": identity.raw_tool_name,
            "provider_name": identity.provider_name,
            "canonical": {
                "namespace": identity.canonical_namespace,
                "name": identity.canonical_name,
            },
            "source": {
                "id": identity.source_id,
                "kind": identity.source_kind,
            },
            "content": result.content,
            "structured_content": result.structured_content,
            "is_error": is_error,
        }),
        model_content: Some(model_content),
        attachments: Vec::new(),
        is_error,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn normalizes_mcp_names_for_model_visible_tools() {
        assert_eq!(normalize_mcp_server_name("docs server"), "docs_server");
        assert_eq!(
            mcp_tool_visible_name("docs server", "search/repo"),
            "mcp__docs_server__search_repo"
        );
        assert_eq!(
            mcp_tool_name_parts("mcp__docs_server__search_repo"),
            Some(("docs_server", "search_repo"))
        );
    }

    #[test]
    fn source_catalog_applies_codex_style_precedence() {
        let plugin = McpServerInput::with_source(
            "repo tools",
            McpTransportInput::Unsupported {
                kind: "plugin".to_string(),
            },
            "plugin:repo",
            "plugin",
        );
        let selected = McpServerInput::with_source(
            "repo tools",
            McpTransportInput::Unsupported {
                kind: "selected".to_string(),
            },
            "capability-root:repo",
            "selected_capability_root",
        );
        let profile = McpServerInput::with_source(
            "repo tools",
            McpTransportInput::Unsupported {
                kind: "profile".to_string(),
            },
            "profile:mcp:repo tools",
            "profile",
        );
        let session = McpServerInput::with_source(
            "repo tools",
            McpTransportInput::Unsupported {
                kind: "session".to_string(),
            },
            "session:mcp:repo tools",
            "session",
        );

        let catalog = McpSourceCatalog::resolve(&[plugin, selected, profile, session]);

        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].source_kind, "session");
        assert_eq!(
            catalog.entries[0].normalized_name,
            normalize_mcp_server_name("repo tools")
        );
        assert_eq!(catalog.warnings.len(), 3);
    }

    #[test]
    fn callable_parts_are_hash_suffixed_and_bounded() {
        let namespace = "mcp__very_long_server_name_that_needs_truncation_for_chat_tools";
        let tool = "very_long_tool_name_that_also_needs_truncation";
        let (namespace, tool) =
            unique_callable_parts(namespace, tool, "raw identity", &mut HashSet::new());
        let fallback = mcp_flat_tool_name(&namespace, &tool);

        assert!(fallback.len() <= MAX_TOOL_NAME_LENGTH);
        assert!(fallback.contains('_'));
    }

    #[test]
    fn utility_actions_map_to_mcp_permission_labels() {
        assert_eq!(
            mcp_utility_action(
                READ_MCP_RESOURCE_TOOL,
                &json!({"server": "docs", "uri": "file:///a"})
            ),
            Some(("docs".to_string(), "resources/read".to_string()))
        );
        assert_eq!(
            mcp_utility_action(LIST_MCP_PROMPTS_TOOL, &json!({})),
            Some(("*".to_string(), "prompts/list".to_string()))
        );
        assert!(mcp_utility_action("read", &json!({})).is_none());
    }

    #[test]
    fn sampling_and_elicitation_defaults_are_bounded() {
        let sampling = McpSamplingConfig::bounded_default();
        assert!(sampling.enabled);
        assert!(sampling.timeout_secs <= 60);
        assert!(sampling.max_tokens <= 1024);
        assert!(sampling.max_tool_rounds <= 2);

        let elicitation = McpElicitationPolicy::default_form_and_url();
        assert!(elicitation.supports_form);
        assert!(elicitation.supports_url);
        assert!(elicitation.auto_accept_empty_confirmation);
    }

    #[test]
    fn mcp_output_prefers_text_for_model_content() {
        let mut result = CallToolResult::success(vec![rmcp::model::Content::text("hello")]);
        result.structured_content = Some(json!({"ok": true}));
        let output = mcp_tool_output("server", "tool", result);
        assert_eq!(output.model_content.as_deref(), Some("hello"));
        assert_eq!(output.json["name"], "server/tool");
        assert_eq!(output.json["canonical"]["namespace"], "mcp__server");
        assert!(!output.is_error);
    }
}
