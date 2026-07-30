use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::{self, StreamExt};
use http::{HeaderName, HeaderValue};
use psychevo_agent_core::{
    ToolBinding, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolOutput,
};
use psychevo_ai::{AbortSignal, SecretValue};
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

use crate::config::{
    config_show_value, load_mcp_oauth_access_token, parse_run_config, resolve_psychevo_home,
};
use crate::host_paths::{ExecutableResolveOptions, HostPlatform, resolve_executable_path};
use crate::permissions::PermissionRuntime;
use crate::types::{
    McpServerInput, McpServerPolicy, McpStartupApprovalTarget, McpTransportInput, RunOptions,
    RunWarning,
};

const LIST_MCP_RESOURCES_TOOL: &str = "list_mcp_resources";
const LIST_MCP_RESOURCE_TEMPLATES_TOOL: &str = "list_mcp_resource_templates";
const READ_MCP_RESOURCE_TOOL: &str = "read_mcp_resource";
const LIST_MCP_PROMPTS_TOOL: &str = "list_mcp_prompts";
const GET_MCP_PROMPT_TOOL: &str = "get_mcp_prompt";

const MCP_TOOL_NAME_DELIMITER: &str = "__";
const MAX_TOOL_NAME_LENGTH: usize = 64;
const HASH_SUFFIX_LEN: usize = 12;
const MCP_STARTUP_CONCURRENCY: usize = 8;
const DEFAULT_MCP_STARTUP_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MCP_CALL_TIMEOUT_SECS: u64 = 300;
const MCP_UTILITY_LIST_LIMIT: usize = 1_000;

fn effective_mcp_tool_timeout_secs(policy: &McpServerPolicy) -> u64 {
    policy
        .tool_timeout_secs
        .unwrap_or(DEFAULT_MCP_CALL_TIMEOUT_SECS)
}

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
            update_mcp_transport_hash(&mut hasher, &entry.input.transport);
            update_mcp_policy_hash(&mut hasher, &entry.input.policy);
        }
        format!("{:x}", hasher.finalize())
    }
}

fn update_mcp_transport_hash(hasher: &mut Sha256, transport: &McpTransportInput) {
    match transport {
        McpTransportInput::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            hasher.update(b"stdio");
            update_mcp_hash_value(hasher, &command.to_string_lossy());
            for arg in args {
                update_mcp_hash_value(hasher, arg);
            }
            hasher.update([0xff]);
            for name in env.keys() {
                update_mcp_hash_value(hasher, name);
            }
            hasher.update([0xff]);
            if let Some(cwd) = cwd {
                update_mcp_hash_value(hasher, &cwd.to_string_lossy());
            }
        }
        McpTransportInput::StreamableHttp {
            url,
            headers,
            bearer_token_env_var,
            scopes,
            oauth_resource,
            oauth_client_id,
        } => {
            hasher.update(b"streamable_http");
            update_mcp_hash_value(hasher, &normalize_mcp_http_url(url));
            for name in headers.keys() {
                update_mcp_hash_value(hasher, name);
            }
            hasher.update([0xff]);
            if let Some(env_var) = bearer_token_env_var {
                update_mcp_hash_value(hasher, env_var);
            }
            for scope in scopes {
                update_mcp_hash_value(hasher, scope);
            }
            hasher.update([0xff]);
            if let Some(resource) = oauth_resource {
                update_mcp_hash_value(hasher, resource);
            }
            if let Some(client_id) = oauth_client_id {
                update_mcp_hash_value(hasher, client_id);
            }
        }
        McpTransportInput::Unsupported { kind } => {
            hasher.update(b"unsupported");
            update_mcp_hash_value(hasher, kind);
        }
    }
    hasher.update([0]);
}

fn update_mcp_hash_value(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_le_bytes());
    hasher.update(value.as_bytes());
}

fn update_mcp_policy_hash(hasher: &mut Sha256, policy: &McpServerPolicy) {
    hasher.update(if policy.enabled {
        b"enabled:1"
    } else {
        b"enabled:0"
    });
    hasher.update([0]);
    hasher.update(if policy.required {
        b"required:1"
    } else {
        b"required:0"
    });
    hasher.update([0]);
    hasher.update(if policy.supports_parallel_tool_calls {
        b"parallel:1"
    } else {
        b"parallel:0"
    });
    hasher.update([0]);
    if let Some(startup_timeout_secs) = policy.startup_timeout_secs {
        hasher.update(format!("startup_timeout:{startup_timeout_secs}").as_bytes());
    }
    hasher.update([0]);
    if let Some(tool_timeout_secs) = policy.tool_timeout_secs {
        hasher.update(format!("tool_timeout:{tool_timeout_secs}").as_bytes());
    }
    hasher.update([0]);
    if let Some(enabled_tools) = &policy.enabled_tools {
        for tool in enabled_tools {
            hasher.update(tool.as_bytes());
            hasher.update([0]);
        }
    }
    hasher.update([0]);
    for tool in &policy.disabled_tools {
        hasher.update(tool.as_bytes());
        hasher.update([0]);
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
    pub(crate) required_failures: Vec<String>,
    pub(crate) snapshot_hash: String,
    pub(crate) catalog_hash: String,
    pub(crate) accepted_servers: Vec<String>,
    pub(crate) resources_available: bool,
    pub(crate) prompts_available: bool,
    pub(crate) sampling_config: McpSamplingConfig,
    pub(crate) elicitation_policy: McpElicitationPolicy,
    reusable: bool,
}

#[doc(hidden)]
#[derive(Clone)]
pub struct McpRuntime {
    owner: McpRuntimeOwner,
}

#[derive(Clone)]
enum McpRuntimeOwner {
    Direct(Arc<Mutex<McpConnectionManager>>),
    Thread {
        registry: McpRuntimeRegistry,
        thread_id: Arc<str>,
    },
}

#[derive(Clone, Default)]
pub(crate) struct McpRuntimeRegistry {
    managers: Arc<StdMutex<HashMap<String, Arc<Mutex<McpConnectionManager>>>>>,
}

impl Default for McpRuntime {
    fn default() -> Self {
        Self {
            owner: McpRuntimeOwner::Direct(Arc::new(Mutex::new(
                McpConnectionManager::default(),
            ))),
        }
    }
}

impl std::fmt::Debug for McpRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpRuntime(..)")
    }
}

impl McpRuntime {
    pub(crate) fn for_thread(
        registry: McpRuntimeRegistry,
        thread_id: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            owner: McpRuntimeOwner::Thread {
                registry,
                thread_id: thread_id.into(),
            },
        }
    }

    pub(crate) async fn snapshot(
        &self,
        inputs: &[McpServerInput],
        cwd: &Path,
        permission_runtime: Option<&PermissionRuntime>,
        read_only_tools: bool,
    ) -> (McpRuntimeSnapshot, u64) {
        if inputs.is_empty() && matches!(self.owner, McpRuntimeOwner::Thread { .. }) {
            return (
                mcp_runtime_snapshot(inputs, cwd, permission_runtime, read_only_tools).await,
                0,
            );
        }
        let manager = match &self.owner {
            McpRuntimeOwner::Direct(manager) => Arc::clone(manager),
            McpRuntimeOwner::Thread {
                registry,
                thread_id,
            } => registry.manager(thread_id),
        };
        let mut manager = manager.lock().await;
        let snapshot = manager
            .snapshot(inputs, cwd, permission_runtime, read_only_tools)
            .await;
        (snapshot, manager.generation())
    }

    #[cfg(test)]
    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        match (&self.owner, &other.owner) {
            (McpRuntimeOwner::Direct(left), McpRuntimeOwner::Direct(right)) => {
                Arc::ptr_eq(left, right)
            }
            (
                McpRuntimeOwner::Thread {
                    registry: left_registry,
                    thread_id: left_thread,
                },
                McpRuntimeOwner::Thread {
                    registry: right_registry,
                    thread_id: right_thread,
                },
            ) => {
                left_registry.same_instance(right_registry) && left_thread == right_thread
            }
            _ => false,
        }
    }
}

impl McpRuntimeRegistry {
    fn manager(&self, thread_id: &str) -> Arc<Mutex<McpConnectionManager>> {
        Arc::clone(
            self.managers
                .lock()
                .expect("MCP runtime registry poisoned")
                .entry(thread_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(McpConnectionManager::default()))),
        )
    }

    pub(crate) fn runtime(&self, thread_id: &str) -> McpRuntime {
        McpRuntime::for_thread(self.clone(), Arc::<str>::from(thread_id))
    }

    pub(crate) fn remove(&self, thread_id: &str) {
        self.managers
            .lock()
            .expect("MCP runtime registry poisoned")
            .remove(thread_id);
    }

    pub(crate) fn clear(&self) {
        self.managers
            .lock()
            .expect("MCP runtime registry poisoned")
            .clear();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.managers
            .lock()
            .expect("MCP runtime registry poisoned")
            .len()
    }

    #[cfg(test)]
    fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.managers, &other.managers)
    }
}

#[derive(Default)]
pub(crate) struct McpConnectionManager {
    cached: Option<McpCachedSnapshot>,
    dirty_servers: HashSet<String>,
    generation: u64,
}

struct McpCachedSnapshot {
    connection_identity_hash: String,
    cwd: PathBuf,
    snapshot: McpRuntimeSnapshot,
}

impl McpConnectionManager {
    #[cfg(test)]
    pub(crate) fn mark_tools_changed(&mut self, server_name: &str) {
        self.dirty_servers
            .insert(normalize_mcp_server_name(server_name));
    }

    #[cfg(test)]
    pub(crate) fn mark_all_dirty(&mut self) {
        self.dirty_servers.insert("*".to_string());
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) async fn snapshot(
        &mut self,
        inputs: &[McpServerInput],
        cwd: &Path,
        permission_runtime: Option<&PermissionRuntime>,
        read_only_tools: bool,
    ) -> McpRuntimeSnapshot {
        let connection_identity_hash = mcp_connection_identity_hash(
            inputs,
            cwd,
            permission_runtime,
            read_only_tools,
        );
        let cwd = cwd.to_path_buf();
        if self.dirty_servers.is_empty()
            && let Some(cached) = &self.cached
            && cached.connection_identity_hash == connection_identity_hash
            && cached.cwd == cwd
        {
            return cached.snapshot.clone();
        }

        let snapshot =
            mcp_runtime_snapshot(inputs, &cwd, permission_runtime, read_only_tools).await;
        self.generation = self.generation.saturating_add(1);
        self.dirty_servers.clear();
        self.cached = snapshot.reusable.then(|| McpCachedSnapshot {
            connection_identity_hash,
            cwd,
            snapshot: snapshot.clone(),
        });
        snapshot
    }
}

fn mcp_connection_identity_hash(
    inputs: &[McpServerInput],
    cwd: &Path,
    permission_runtime: Option<&PermissionRuntime>,
    read_only_tools: bool,
) -> String {
    let catalog = McpSourceCatalog::resolve(inputs);
    let mut hasher = Sha256::new();
    hasher.update(catalog.hash().as_bytes());
    hasher.update([u8::from(read_only_tools)]);
    update_mcp_hash_value(&mut hasher, &cwd.to_string_lossy());
    for entry in &catalog.entries {
        match &entry.input.transport {
            McpTransportInput::Stdio { env, .. } => {
                for value in env.values() {
                    hasher.update(Sha256::digest(value.as_bytes()));
                }
                hasher.update([0]);
            }
            McpTransportInput::StreamableHttp {
                url,
                headers,
                bearer_token_env_var,
                ..
            } => {
                for value in headers.values() {
                    hasher.update(Sha256::digest(value.as_bytes()));
                }
                let token = resolve_http_bearer_token(
                    &entry.input,
                    bearer_token_env_var.as_deref(),
                    url,
                );
                if let Some(token) = token {
                    hasher.update(Sha256::digest(token.as_bytes()));
                }
                hasher.update([0]);
            }
            McpTransportInput::Unsupported { .. } => {}
        }
    }
    if let Some(runtime) = permission_runtime {
        let inner = &runtime.inner;
        update_mcp_hash_value(&mut hasher, &inner.cwd.to_string_lossy());
        update_mcp_hash_value(&mut hasher, &inner.project_config_dir.to_string_lossy());
        update_mcp_hash_value(&mut hasher, inner.mode.as_str());
        update_mcp_hash_value(&mut hasher, &format!("{:?}", inner.config));
        update_mcp_hash_value(&mut hasher, &format!("{:?}", inner.sandbox_policy));
        for path in &inner.protected_config_paths {
            update_mcp_hash_value(&mut hasher, &path.to_string_lossy());
        }
        hasher.update([
            u8::from(inner.approval_handler.is_some()),
            u8::from(inner.smart_approval_handler.is_some()),
            u8::from(inner.hook_runtime.is_some()),
        ]);
    }
    format!("{:x}", hasher.finalize())
}

pub(crate) async fn mcp_runtime_snapshot(
    inputs: &[McpServerInput],
    cwd: &Path,
    permission_runtime: Option<&PermissionRuntime>,
    read_only_tools: bool,
) -> McpRuntimeSnapshot {
    let catalog = McpSourceCatalog::resolve(inputs);
    let mut tools = Vec::<Arc<dyn ToolBinding>>::new();
    let mut warnings = catalog.warnings.clone();
    let mut connections = BTreeMap::<String, Arc<McpConnection>>::new();
    let mut tool_candidates = Vec::<McpToolCandidate>::new();
    let mut accepted_servers = Vec::new();
    let mut required_failures = Vec::new();
    let mut resources_available = false;
    let mut prompts_available = false;
    let mut reusable = true;
    let sampling_config = McpSamplingConfig::bounded_default();
    let elicitation_policy = McpElicitationPolicy::default_form_and_url();

    let mut enabled_entries = Vec::new();
    for (index, entry) in catalog.entries.iter().cloned().enumerate() {
        if !entry.input.policy.enabled {
            let message = format!("MCP server `{}` is disabled", entry.input.name);
            warnings.push(mcp_warning(message.clone()));
            if entry.input.policy.required {
                required_failures.push(message);
                reusable = false;
            }
            continue;
        }
        enabled_entries.push((index, entry));
    }
    let mut prepared = stream::iter(enabled_entries)
        .map(|(index, entry)| async move {
            (
                index,
                prepare_mcp_server(entry, cwd, permission_runtime).await,
            )
        })
        .buffer_unordered(MCP_STARTUP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    prepared.sort_by_key(|(index, _)| *index);

    for (_, result) in prepared {
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(failure) => {
                warnings.push(mcp_warning(failure.message.clone()));
                reusable = false;
                if failure.required {
                    required_failures.push(failure.message);
                }
                continue;
            }
        };
        let entry = &prepared.entry;
        let server_name = entry.normalized_name.clone();
        let connection = prepared.connection;
        accepted_servers.push(server_name.clone());
        connections.insert(server_name.clone(), Arc::clone(&connection));
        resources_available |= prepared.resources_available;
        prompts_available |= prepared.prompts_available;
        if let Some(error) = prepared.tool_listing_error {
            warnings.push(mcp_warning(format!(
                "MCP server `{}` did not list tools: {error}",
                entry.input.name
            )));
            reusable = false;
        }

        for tool in prepared.tools {
            let raw_tool_name = tool.name.to_string();
            if !mcp_tool_allowed_by_policy(&entry.input.policy, &raw_tool_name)
                || !mcp_tool_allowed_by_effect_policy(&tool, read_only_tools)
            {
                continue;
            }
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
                    supports_parallel_tool_calls: entry.input.policy.supports_parallel_tool_calls,
                    tool_timeout_secs: effective_mcp_tool_timeout_secs(&entry.input.policy),
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
        required_failures,
        snapshot_hash,
        catalog_hash,
        accepted_servers,
        resources_available,
        prompts_available,
        sampling_config,
        elicitation_policy,
        reusable,
    }
}

struct PreparedMcpServer {
    entry: McpCatalogEntry,
    connection: Arc<McpConnection>,
    tools: Vec<rmcp::model::Tool>,
    tool_listing_error: Option<String>,
    resources_available: bool,
    prompts_available: bool,
}

/// One accepted, fully resolved startup identity. Approval and process/network
/// startup consume this same value so neither side can reinterpret mutable
/// environment or relative paths after the other has acted.
struct ResolvedMcpLaunch {
    entry: McpCatalogEntry,
    input: McpServerInput,
    bearer_token: Option<SecretValue>,
    descriptor_fingerprint: String,
}

impl ResolvedMcpLaunch {
    fn resolve(entry: McpCatalogEntry, cwd: &Path) -> Result<Self, String> {
        let mut input = entry.input.clone();
        let bearer_token = match &mut input.transport {
            McpTransportInput::Stdio {
                command,
                env: server_env,
                cwd: server_cwd,
                ..
            } => {
                let effective_cwd = server_cwd.as_deref().unwrap_or(cwd);
                let effective_cwd = std::fs::canonicalize(effective_cwd).map_err(|error| {
                    format!(
                        "MCP server `{}` cwd `{}` could not be resolved: {error}",
                        input.name,
                        effective_cwd.display()
                    )
                })?;
                let mut executable_env = env::vars().collect::<BTreeMap<_, _>>();
                executable_env.extend(server_env.clone());
                let command_text = command.to_string_lossy().into_owned();
                let executable = resolve_executable_path(
                    &command_text,
                    &effective_cwd,
                    &ExecutableResolveOptions {
                        platform: HostPlatform::current(),
                        env: &executable_env,
                    },
                )
                .ok_or_else(|| {
                    format!(
                        "MCP server `{}` command `{command_text}` was not found",
                        input.name
                    )
                })?;
                *command = std::fs::canonicalize(&executable).unwrap_or(executable);
                *server_cwd = Some(effective_cwd);
                None
            }
            McpTransportInput::StreamableHttp {
                url,
                bearer_token_env_var,
                ..
            } => {
                let configured_url = url.clone();
                let bearer_token = resolve_http_bearer_token(
                    &entry.input,
                    bearer_token_env_var.as_deref(),
                    &configured_url,
                )
                .map(SecretValue::new);
                *url = normalize_mcp_http_url(&configured_url);
                bearer_token
            }
            McpTransportInput::Unsupported { kind } => {
                return Err(format!(
                    "MCP server `{}` uses unsupported transport `{kind}`",
                    input.name
                ));
            }
        };
        let descriptor_fingerprint = mcp_descriptor_fingerprint(&entry, &input);
        Ok(Self {
            entry,
            input,
            bearer_token,
            descriptor_fingerprint,
        })
    }

    fn descriptor_key(&self) -> String {
        format!(
            "{}@{}",
            self.entry.normalized_name, self.descriptor_fingerprint
        )
    }

    fn approval_target(&self) -> McpStartupApprovalTarget {
        match &self.input.transport {
            McpTransportInput::Stdio {
                command,
                args,
                env,
                cwd,
            } => McpStartupApprovalTarget::Stdio {
                command: command.to_string_lossy().into_owned(),
                args: args.clone(),
                cwd: cwd
                    .as_deref()
                    .expect("resolved stdio MCP launch has an effective cwd")
                    .to_string_lossy()
                    .into_owned(),
                env_names: env.keys().cloned().collect(),
            },
            McpTransportInput::StreamableHttp {
                url,
                headers,
                bearer_token_env_var,
                oauth_client_id,
                ..
            } => {
                let mut credential_names = bearer_token_env_var.iter().cloned().collect::<Vec<_>>();
                if oauth_client_id.is_some() {
                    credential_names.push("oauth_client_id".to_string());
                }
                McpStartupApprovalTarget::Http {
                    url: mcp_http_approval_url(url),
                    header_names: headers.keys().cloned().collect(),
                    credential_names,
                }
            }
            McpTransportInput::Unsupported { kind } => {
                unreachable!("unsupported MCP transport `{kind}` cannot resolve")
            }
        }
    }
}

struct McpStartupFailure {
    message: String,
    required: bool,
}

async fn prepare_mcp_server(
    entry: McpCatalogEntry,
    cwd: &Path,
    permission_runtime: Option<&PermissionRuntime>,
) -> Result<PreparedMcpServer, McpStartupFailure> {
    let required = entry.input.policy.required;
    let timeout_secs = entry
        .input
        .policy
        .startup_timeout_secs
        .unwrap_or(DEFAULT_MCP_STARTUP_TIMEOUT_SECS);
    let display_name = entry.input.name.clone();
    let launch =
        ResolvedMcpLaunch::resolve(entry, cwd).map_err(|message| McpStartupFailure {
            message,
            required,
        })?;
    let authorization_id = format!("mcp_startup:{}", launch.descriptor_key());
    match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        prepare_mcp_server_within_deadline(launch, permission_runtime),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            if let Some(permission_runtime) = permission_runtime {
                permission_runtime
                    .cancel_authorization(&authorization_id)
                    .await;
            }
            Err(McpStartupFailure {
                message: format!(
                    "MCP server `{}` startup timed out after {timeout_secs}s",
                    display_name
                ),
                required,
            })
        }
    }
}

async fn prepare_mcp_server_within_deadline(
    launch: ResolvedMcpLaunch,
    permission_runtime: Option<&PermissionRuntime>,
) -> Result<PreparedMcpServer, McpStartupFailure> {
    let server_name = launch.entry.normalized_name.clone();
    if let Some(permission_runtime) = permission_runtime
        && let Err(err) = permission_runtime
            .authorize_mcp_startup(
                &server_name,
                &launch.entry.source_id,
                &launch.approval_target(),
                &launch.descriptor_fingerprint,
            )
            .await
    {
        return Err(McpStartupFailure {
            message: format!(
                "MCP server `{}` startup omitted: {err}",
                launch.entry.input.name
            ),
            required: launch.entry.input.policy.required,
        });
    }
    let service = connect_resolved_mcp_launch(&launch)
        .await
        .map_err(|err| McpStartupFailure {
            message: format!(
                "MCP server `{}` is unavailable: {err}",
                launch.entry.input.name
            ),
            required: launch.entry.input.policy.required,
        })?;
    let peer = service.peer().clone();
    let capabilities = peer
        .peer_info()
        .map(|info| info.capabilities.clone())
        .unwrap_or_default();
    let (tools, tool_listing_error) = if capabilities.tools.is_some() {
        match peer.list_all_tools().await {
            Ok(tools) => (tools, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        }
    } else {
        (Vec::new(), None)
    };
    let connection = Arc::new(McpConnection {
        peer,
        _service: Mutex::new(service),
    });
    Ok(PreparedMcpServer {
        entry: launch.entry,
        connection,
        tools,
        tool_listing_error,
        resources_available: capabilities.resources.is_some(),
        prompts_available: capabilities.prompts.is_some(),
    })
}

fn normalize_mcp_http_url(url: &str) -> String {
    url.parse::<http::Uri>()
        .map(|uri| uri.to_string())
        .unwrap_or_else(|_| url.trim().to_string())
}

fn mcp_http_approval_url(url: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(url) else {
        return "<invalid MCP URL>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

fn mcp_descriptor_fingerprint(entry: &McpCatalogEntry, input: &McpServerInput) -> String {
    let mut hasher = Sha256::new();
    update_mcp_hash_value(&mut hasher, &entry.normalized_name);
    update_mcp_hash_value(&mut hasher, &entry.source_id);
    update_mcp_hash_value(&mut hasher, &entry.source_kind);
    update_mcp_transport_hash(&mut hasher, &input.transport);
    update_mcp_policy_hash(&mut hasher, &input.policy);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn mcp_transport_kind(transport: &McpTransportInput) -> &'static str {
    match transport {
        McpTransportInput::Stdio { .. } => "stdio",
        McpTransportInput::StreamableHttp { .. } => "streamable_http",
        McpTransportInput::Unsupported { .. } => "unsupported",
    }
}

pub async fn mcp_test_server_value(options: &RunOptions, name: &str) -> crate::Result<Value> {
    let document = config_show_value(options, crate::types::ConfigScope::Effective)?;
    let value = document.get("value").cloned().unwrap_or_else(|| json!({}));
    let config = parse_run_config(value)?;
    let server = config
        .mcp_servers
        .into_iter()
        .find(|server| server.name == name)
        .ok_or_else(|| crate::Error::Config(format!("unknown MCP server: {name}")))?;
    match connect_mcp_server_with_policy(&server, &options.cwd).await {
        Ok(service) => {
            let peer = service.peer().clone();
            let tools = peer
                .list_all_tools()
                .await
                .map(|tools| {
                    tools
                        .into_iter()
                        .map(|tool| {
                            json!({
                                "name": tool.name,
                                "title": tool.title,
                                "description": tool.description,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(json!({
                "ok": true,
                "name": name,
                "transport": mcp_transport_kind(&server.transport),
                "tools": tools,
            }))
        }
        Err(err) => Ok(json!({
            "ok": false,
            "name": name,
            "transport": mcp_transport_kind(&server.transport),
            "error": err,
        })),
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
        McpTransportInput::StreamableHttp {
            url,
            headers,
            bearer_token_env_var,
            ..
        } => {
            let mut parsed_headers = HashMap::new();
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|err| format!("invalid HTTP header `{name}`: {err}"))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|err| format!("invalid HTTP header value for `{name}`: {err}"))?;
                parsed_headers.insert(name, value);
            }
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .custom_headers(parsed_headers);
            if let Some(token) =
                resolve_http_bearer_token(input, bearer_token_env_var.as_deref(), url)
            {
                config = config.auth_header(token);
            }
            let transport = StreamableHttpClientTransport::from_config(config);
            ().serve(transport).await.map_err(|err| err.to_string())
        }
        McpTransportInput::Unsupported { kind } => Err(format!("unsupported transport `{kind}`")),
    }
}

async fn connect_resolved_mcp_launch(
    launch: &ResolvedMcpLaunch,
) -> Result<RunningService<RoleClient, ()>, String> {
    match &launch.input.transport {
        McpTransportInput::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut cmd = Command::new(command);
            cmd.args(args).envs(env).current_dir(
                cwd.as_deref()
                    .expect("resolved stdio MCP launch has an effective cwd"),
            );
            let transport = TokioChildProcess::new(cmd).map_err(|error| error.to_string())?;
            ().serve(transport).await.map_err(|error| error.to_string())
        }
        McpTransportInput::StreamableHttp { url, headers, .. } => {
            let mut parsed_headers = HashMap::new();
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|error| format!("invalid HTTP header `{name}`: {error}"))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|error| format!("invalid HTTP header value for `{name}`: {error}"))?;
                parsed_headers.insert(name, value);
            }
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .custom_headers(parsed_headers);
            if let Some(token) = launch.bearer_token.as_ref() {
                config = config.auth_header(token.expose_secret().to_string());
            }
            let transport = StreamableHttpClientTransport::from_config(config);
            ().serve(transport).await.map_err(|error| error.to_string())
        }
        McpTransportInput::Unsupported { kind } => Err(format!("unsupported transport `{kind}`")),
    }
}

fn resolve_http_bearer_token(
    input: &McpServerInput,
    bearer_token_env_var: Option<&str>,
    url: &str,
) -> Option<String> {
    if let Some(env_var) = bearer_token_env_var
        && let Ok(value) = env::var(env_var)
        && !value.trim().is_empty()
    {
        return Some(value);
    }
    let env_map = env::vars().collect::<BTreeMap<_, _>>();
    let home = resolve_psychevo_home(&env_map).ok()?;
    load_mcp_oauth_access_token(&home, &input.name, url)
        .ok()
        .flatten()
}

async fn connect_mcp_server_with_policy(
    input: &McpServerInput,
    cwd: &Path,
) -> Result<RunningService<RoleClient, ()>, String> {
    let timeout_secs = input
        .policy
        .startup_timeout_secs
        .unwrap_or(DEFAULT_MCP_STARTUP_TIMEOUT_SECS);
    tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        connect_mcp_server(input, cwd),
    )
    .await
    .map_err(|_| format!("startup timed out after {timeout_secs}s"))?
}

fn mcp_tool_allowed_by_policy(policy: &McpServerPolicy, raw_tool_name: &str) -> bool {
    if let Some(enabled_tools) = &policy.enabled_tools
        && !enabled_tools.iter().any(|tool| tool == raw_tool_name)
    {
        return false;
    }
    !policy
        .disabled_tools
        .iter()
        .any(|tool| tool == raw_tool_name)
}

fn mcp_tool_allowed_by_effect_policy(tool: &rmcp::model::Tool, read_only_tools: bool) -> bool {
    !read_only_tools
        || tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            == Some(true)
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
            description: "List MCP resources from one server or all available MCP servers.",
            connection_set,
        }
    }

    fn list_resource_templates(connection_set: Arc<McpUtilityConnectionSet>) -> Self {
        Self {
            kind: McpUtilityKind::ListResourceTemplates,
            name: LIST_MCP_RESOURCE_TEMPLATES_TOOL,
            description: "List MCP resource templates from one server or all available MCP servers.",
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
            description: "List MCP prompts from one server or all available MCP servers.",
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
        "description": "MCP server name. Omit for list operations that should query every available server."
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
            let mut truncated = false;
            let requests = connection_set
                .peers_for_optional_server(server)
                .into_iter()
                .map(|(server, peer)| {
                    let mut abort = abort.clone();
                    let request = Box::pin(async move {
                        await_mcp_utility(
                            peer.list_all_resources(),
                            &mut abort,
                            "MCP resource listing",
                        )
                        .await
                    }) as BoxFuture<'static, Result<Vec<_>, String>>;
                    (server, request)
                })
                .collect();
            for (server, result) in collect_mcp_utility_lists(requests).await {
                match result {
                    Ok(listed) => {
                        truncated = append_utility_values(
                            &mut resources,
                            listed
                                .into_iter()
                                .map(|resource| json!({ "server": server, "resource": resource })),
                        );
                        if truncated {
                            break;
                        }
                    }
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            if truncated {
                errors.push(utility_truncation_error());
            }
            utility_list_output("resources", resources, errors)
        }
        McpUtilityKind::ListResourceTemplates => {
            let mut templates = Vec::new();
            let mut errors = Vec::new();
            let mut truncated = false;
            let requests = connection_set
                .peers_for_optional_server(server)
                .into_iter()
                .map(|(server, peer)| {
                    let mut abort = abort.clone();
                    let request = Box::pin(async move {
                        await_mcp_utility(
                            peer.list_all_resource_templates(),
                            &mut abort,
                            "MCP resource template listing",
                        )
                        .await
                    }) as BoxFuture<'static, Result<Vec<_>, String>>;
                    (server, request)
                })
                .collect();
            for (server, result) in collect_mcp_utility_lists(requests).await {
                match result {
                    Ok(listed) => {
                        truncated = append_utility_values(
                            &mut templates,
                            listed.into_iter().map(
                                |template| json!({ "server": server, "resource_template": template }),
                            ),
                        );
                        if truncated {
                            break;
                        }
                    }
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            if truncated {
                errors.push(utility_truncation_error());
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
            match await_mcp_utility(
                peer.read_resource(request),
                &mut abort,
                &format!("MCP resource `{server}/{uri}`"),
            )
            .await
            {
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
                Err(err) => ToolOutput::error(err),
            }
        }
        McpUtilityKind::ListPrompts => {
            let mut prompts = Vec::new();
            let mut errors = Vec::new();
            let mut truncated = false;
            let requests = connection_set
                .peers_for_optional_server(server)
                .into_iter()
                .map(|(server, peer)| {
                    let mut abort = abort.clone();
                    let request = Box::pin(async move {
                        await_mcp_utility(
                            peer.list_all_prompts(),
                            &mut abort,
                            "MCP prompt listing",
                        )
                        .await
                    }) as BoxFuture<'static, Result<Vec<_>, String>>;
                    (server, request)
                })
                .collect();
            for (server, result) in collect_mcp_utility_lists(requests).await {
                match result {
                    Ok(listed) => {
                        truncated = append_utility_values(
                            &mut prompts,
                            listed
                                .into_iter()
                                .map(|prompt| json!({ "server": server, "prompt": prompt })),
                        );
                        if truncated {
                            break;
                        }
                    }
                    Err(err) => errors.push(json!({ "server": server, "error": err.to_string() })),
                }
            }
            if truncated {
                errors.push(utility_truncation_error());
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
            match await_mcp_utility(
                peer.get_prompt(request),
                &mut abort,
                &format!("MCP prompt `{server}/{name}`"),
            )
            .await
            {
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
                Err(err) => ToolOutput::error(err),
            }
        }
    }
}

type McpUtilityListResult<T> = Result<Vec<T>, String>;
type McpUtilityListRequest<T> = (String, BoxFuture<'static, McpUtilityListResult<T>>);
type IndexedMcpUtilityListRequest<T> =
    BoxFuture<'static, (usize, String, McpUtilityListResult<T>)>;

async fn collect_mcp_utility_lists<T: Send + 'static>(
    requests: Vec<McpUtilityListRequest<T>>,
) -> Vec<(String, Result<Vec<T>, String>)> {
    let mut pending: Vec<IndexedMcpUtilityListRequest<T>> =
        Vec::with_capacity(requests.len());
    for (index, (server, request)) in requests.into_iter().enumerate() {
        pending.push(Box::pin(async move {
            (index, server, request.await)
        }));
    }
    let mut results = stream::iter(pending)
        .buffer_unordered(MCP_STARTUP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    results.sort_by_key(|(index, _, _)| *index);
    results
        .into_iter()
        .map(|(_, server, result)| (server, result))
        .collect()
}

async fn await_mcp_utility<T, E>(
    future: impl std::future::Future<Output = Result<T, E>>,
    abort: &mut AbortSignal,
    label: &str,
) -> Result<T, String>
where
    E: std::fmt::Display,
{
    tokio::select! {
        _ = abort.wait_for_abort() => Err(format!("{label} was aborted")),
        result = tokio::time::timeout(
            Duration::from_secs(DEFAULT_MCP_CALL_TIMEOUT_SECS),
            future,
        ) => match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(format!("{label} failed: {error}")),
            Err(_) => Err(format!(
                "{label} timed out after {DEFAULT_MCP_CALL_TIMEOUT_SECS}s"
            )),
        },
    }
}

fn append_utility_values(
    target: &mut Vec<Value>,
    values: impl IntoIterator<Item = Value>,
) -> bool {
    for value in values {
        if target.len() == MCP_UTILITY_LIST_LIMIT {
            return true;
        }
        target.push(value);
    }
    false
}

fn utility_truncation_error() -> Value {
    json!({
        "error": format!(
            "results truncated at {MCP_UTILITY_LIST_LIMIT} entries"
        )
    })
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
    pub(crate) supports_parallel_tool_calls: bool,
    pub(crate) tool_timeout_secs: u64,
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

    fn search_metadata(&self) -> Vec<String> {
        vec![
            self.source_id.clone(),
            self.source_kind.clone(),
            self.raw_server_name.clone(),
            self.normalized_server_name.clone(),
            self.raw_tool_name.clone(),
            format!("{}/{}", self.normalized_server_name, self.raw_tool_name),
            format!("{}/{}", self.raw_server_name, self.raw_tool_name),
        ]
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        if self.supports_parallel_tool_calls {
            ToolExecutionMode::Parallel
        } else {
            ToolExecutionMode::Sequential
        }
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
        let tool_timeout_secs = self.tool_timeout_secs;
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
            let call = peer.call_tool(request);
            let identity = McpToolOutputIdentity {
                normalized_server_name: server_name.clone(),
                raw_server_name,
                raw_tool_name: raw_tool_name.clone(),
                provider_name,
                canonical_namespace,
                canonical_name,
                source_id,
                source_kind,
            };
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` was aborted"
                )),
                result = tokio::time::timeout(Duration::from_secs(tool_timeout_secs), call) => match result {
                    Ok(Ok(result)) => mcp_tool_output_with_identity(identity, result),
                    Ok(Err(err)) => ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` failed: {err}"
                    )),
                    Err(_) => ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` timed out after {tool_timeout_secs}s"
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
    fn http_startup_approval_omits_url_credentials() {
        let input = McpServerInput::with_source(
            "remote",
            McpTransportInput::StreamableHttp {
                url: "https://user:password@example.test:8443/mcp?signature=secret#fragment"
                    .to_string(),
                headers: BTreeMap::from([(
                    "Authorization".to_string(),
                    "secret header".to_string(),
                )]),
                bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                scopes: Vec::new(),
                oauth_resource: None,
                oauth_client_id: None,
            },
            "profile:mcp:remote",
            "profile",
        );
        let entry = McpSourceCatalog::resolve(std::slice::from_ref(&input))
            .entries
            .into_iter()
            .next()
            .expect("catalog entry");
        let launch = ResolvedMcpLaunch::resolve(entry, Path::new(".")).expect("resolved launch");

        assert!(matches!(
            &launch.input.transport,
            McpTransportInput::StreamableHttp { url, .. }
                if url.contains("user:password")
                    && url.contains("signature=secret")
        ));
        assert_eq!(
            launch.approval_target(),
            McpStartupApprovalTarget::Http {
                url: "https://example.test:8443/mcp".to_string(),
                header_names: vec!["Authorization".to_string()],
                credential_names: vec!["MCP_TOKEN".to_string()],
            }
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

    #[tokio::test]
    async fn disabled_required_server_records_required_failure() {
        let input = McpServerInput::with_source(
            "repo tools",
            McpTransportInput::Unsupported {
                kind: "stdio".to_string(),
            },
            "profile:mcp:repo tools",
            "profile",
        )
        .with_policy(McpServerPolicy {
            enabled: false,
            required: true,
            ..McpServerPolicy::default()
        });

        let snapshot = mcp_runtime_snapshot(&[input], Path::new("."), None, false).await;

        assert!(snapshot.tools.is_empty());
        assert_eq!(snapshot.required_failures.len(), 1);
        assert!(
            snapshot.required_failures[0].contains("disabled"),
            "{:?}",
            snapshot.required_failures
        );
    }

    #[test]
    fn source_catalog_hash_includes_policy() {
        let base = McpServerInput::with_source(
            "repo",
            McpTransportInput::Unsupported {
                kind: "stdio".to_string(),
            },
            "profile:mcp:repo",
            "profile",
        );
        let filtered = base.clone().with_policy(McpServerPolicy {
            enabled_tools: Some(vec!["search".to_string()]),
            disabled_tools: vec!["delete".to_string()],
            ..McpServerPolicy::default()
        });

        assert_ne!(
            McpSourceCatalog::resolve(&[base]).hash(),
            McpSourceCatalog::resolve(&[filtered]).hash()
        );
    }

    #[test]
    fn omitted_mcp_tool_timeout_resolves_to_the_bounded_default() {
        assert_eq!(
            effective_mcp_tool_timeout_secs(&McpServerPolicy::default()),
            DEFAULT_MCP_CALL_TIMEOUT_SECS
        );
        assert_eq!(
            effective_mcp_tool_timeout_secs(&McpServerPolicy {
                tool_timeout_secs: Some(7),
                ..McpServerPolicy::default()
            }),
            7
        );
    }

    #[test]
    fn read_only_projection_requires_an_explicit_server_hint() {
        let unannotated = rmcp::model::Tool::default();
        let mut read_only = rmcp::model::Tool::default();
        read_only.annotations = Some(rmcp::model::ToolAnnotations::new().read_only(true));
        let mut effectful = rmcp::model::Tool::default();
        effectful.annotations = Some(rmcp::model::ToolAnnotations::new().read_only(false));

        assert!(mcp_tool_allowed_by_effect_policy(&unannotated, false));
        assert!(mcp_tool_allowed_by_effect_policy(&read_only, false));
        assert!(mcp_tool_allowed_by_effect_policy(&effectful, false));
        assert!(!mcp_tool_allowed_by_effect_policy(&unannotated, true));
        assert!(mcp_tool_allowed_by_effect_policy(&read_only, true));
        assert!(!mcp_tool_allowed_by_effect_policy(&effectful, true));
    }

    #[derive(Debug)]
    struct PendingMcpApproval {
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        request: Arc<StdMutex<Option<crate::types::PermissionApprovalRequest>>>,
        cancelled_id: Arc<StdMutex<Option<String>>>,
    }

    impl crate::types::ApprovalHandler for PendingMcpApproval {
        fn request_permission(
            &self,
            request: crate::types::PermissionApprovalRequest,
        ) -> BoxFuture<'static, crate::types::PermissionApprovalDecision> {
            *self.request.lock().expect("request") = Some(request);
            Box::pin(std::future::pending())
        }

        fn cancel_permission(&self, tool_call_id: &str) -> BoxFuture<'static, ()> {
            let cancelled = Arc::clone(&self.cancelled);
            *self.cancelled_id.lock().expect("cancelled id") = Some(tool_call_id.to_string());
            Box::pin(async move {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            })
        }
    }

    #[tokio::test]
    async fn startup_timeout_cancels_its_permission_request() {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = Arc::new(PendingMcpApproval {
            cancelled: Arc::clone(&cancelled),
            request: Arc::new(StdMutex::new(None)),
            cancelled_id: Arc::new(StdMutex::new(None)),
        });
        let captured_request = Arc::clone(&handler.request);
        let cancelled_id = Arc::clone(&handler.cancelled_id);
        let cwd = std::env::temp_dir();
        let permissions = PermissionRuntime::new(
            cwd.clone(),
            cwd.join(".psychevo"),
            crate::types::PermissionConfig::default(),
            crate::types::PermissionMode::Default,
            Some(handler),
            None,
        );
        let input = McpServerInput::with_source(
            "pending",
            McpTransportInput::Stdio {
                command: std::env::current_exe().expect("current executable"),
                args: Vec::new(),
                env: BTreeMap::from([(
                    "PRIVATE_MCP_TOKEN".to_string(),
                    "never-project-this-value".to_string(),
                )]),
                cwd: Some(cwd.clone()),
            },
            "test:mcp:pending",
            "session",
        )
        .with_policy(McpServerPolicy {
            startup_timeout_secs: Some(1),
            ..McpServerPolicy::default()
        });
        let entry = McpSourceCatalog::resolve(&[input])
            .entries
            .into_iter()
            .next()
            .expect("catalog entry");

        let failure = match prepare_mcp_server(entry, &cwd, Some(&permissions)).await {
            Ok(_) => panic!("expected startup timeout"),
            Err(failure) => failure,
        };

        assert!(failure.message.contains("timed out"));
        assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
        let request = captured_request
            .lock()
            .expect("request")
            .clone()
            .expect("typed startup approval");
        assert!(!request.allow_always);
        let tool_call_id = request.tool_call_id.clone();
        let startup = request.mcp_startup.expect("mcp startup detail");
        assert_eq!(startup.server, "pending");
        assert_eq!(startup.source, "test:mcp:pending");
        let serialized = serde_json::to_string(&startup).expect("serialize startup detail");
        assert!(serialized.contains("PRIVATE_MCP_TOKEN"));
        assert!(!serialized.contains("never-project-this-value"));
        assert!(matches!(
            startup.target,
            McpStartupApprovalTarget::Stdio { .. }
        ));
        assert_eq!(
            cancelled_id.lock().expect("cancelled id").as_deref(),
            Some(tool_call_id.as_str())
        );
    }

    #[tokio::test]
    async fn utility_lists_run_concurrently_and_merge_in_catalog_order() {
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel();
        let (second_finished_tx, second_finished_rx) = tokio::sync::oneshot::channel();
        let first: BoxFuture<'static, Result<Vec<u8>, String>> = Box::pin(async move {
            let _ = first_started_tx.send(());
            let _ = release_first_rx.await;
            Ok(vec![1])
        });
        let second: BoxFuture<'static, Result<Vec<u8>, String>> = Box::pin(async move {
            let _ = first_started_rx.await;
            let _ = second_finished_tx.send(());
            Ok(vec![2])
        });

        let collecting = tokio::spawn(collect_mcp_utility_lists(vec![
            ("first".to_string(), first),
            ("second".to_string(), second),
        ]));
        tokio::time::timeout(Duration::from_millis(100), second_finished_rx)
            .await
            .expect("second utility must not wait for the first")
            .expect("second utility completion");
        let _ = release_first_tx.send(());
        let results = collecting.await.expect("collector");

        assert_eq!(
            results
                .iter()
                .map(|(server, _)| server.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn source_catalog_hash_includes_complete_transport_identity() {
        let stdio = McpServerInput::with_source(
            "repo",
            McpTransportInput::Stdio {
                command: PathBuf::from("/bin/repo-mcp"),
                args: vec!["serve".to_string()],
                env: BTreeMap::from([("MODE".to_string(), "read".to_string())]),
                cwd: Some(PathBuf::from("/repo/a")),
            },
            "profile:mcp:repo",
            "profile",
        );
        let mut changed_stdio = stdio.clone();
        changed_stdio.transport = McpTransportInput::Stdio {
            command: PathBuf::from("/bin/repo-mcp-v2"),
            args: vec!["serve".to_string(), "--fast".to_string()],
            env: BTreeMap::from([("MODE".to_string(), "write".to_string())]),
            cwd: Some(PathBuf::from("/repo/b")),
        };
        assert_ne!(
            McpSourceCatalog::resolve(&[stdio]).hash(),
            McpSourceCatalog::resolve(&[changed_stdio]).hash()
        );

        let http = McpServerInput::with_source(
            "remote",
            McpTransportInput::StreamableHttp {
                url: "https://one.example/mcp".to_string(),
                headers: BTreeMap::from([("X-Tenant".to_string(), "one".to_string())]),
                bearer_token_env_var: Some("MCP_TOKEN_ONE".to_string()),
                scopes: vec!["read".to_string()],
                oauth_resource: Some("resource-one".to_string()),
                oauth_client_id: Some("client-one".to_string()),
            },
            "profile:mcp:remote",
            "profile",
        );
        let mut changed_http = http.clone();
        changed_http.transport = McpTransportInput::StreamableHttp {
            url: "https://two.example/mcp".to_string(),
            headers: BTreeMap::from([("X-Tenant".to_string(), "two".to_string())]),
            bearer_token_env_var: Some("MCP_TOKEN_TWO".to_string()),
            scopes: vec!["write".to_string()],
            oauth_resource: Some("resource-two".to_string()),
            oauth_client_id: Some("client-two".to_string()),
        };
        assert_ne!(
            McpSourceCatalog::resolve(&[http]).hash(),
            McpSourceCatalog::resolve(&[changed_http]).hash()
        );
    }

    #[test]
    fn descriptor_fingerprint_excludes_secret_values_but_includes_bindings() {
        let input = McpServerInput::with_source(
            "remote",
            McpTransportInput::StreamableHttp {
                url: "https://example.test/mcp".to_string(),
                headers: BTreeMap::from([("X-Secret".to_string(), "alpha".to_string())]),
                bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                scopes: Vec::new(),
                oauth_resource: None,
                oauth_client_id: None,
            },
            "profile:mcp:remote@7",
            "profile",
        );
        let entry = McpSourceCatalog::resolve(std::slice::from_ref(&input))
            .entries
            .into_iter()
            .next()
            .expect("entry");
        let first = mcp_descriptor_fingerprint(&entry, &input);

        let mut secret_changed = input.clone();
        if let McpTransportInput::StreamableHttp { headers, .. } =
            &mut secret_changed.transport
        {
            headers.insert("X-Secret".to_string(), "beta".to_string());
        }
        assert_eq!(
            first,
            mcp_descriptor_fingerprint(&entry, &secret_changed)
        );

        let mut binding_changed = input;
        if let McpTransportInput::StreamableHttp {
            bearer_token_env_var,
            ..
        } = &mut binding_changed.transport
        {
            *bearer_token_env_var = Some("OTHER_MCP_TOKEN".to_string());
        }
        assert_ne!(
            first,
            mcp_descriptor_fingerprint(&entry, &binding_changed)
        );
    }

    #[test]
    fn connection_identity_hash_includes_permission_environment() {
        let input = McpServerInput::with_source(
            "repo",
            McpTransportInput::Unsupported {
                kind: "stdio".to_string(),
            },
            "profile:mcp:repo",
            "profile",
        );
        let cwd = PathBuf::from("/repo");
        let default_permissions = PermissionRuntime::new(
            cwd.clone(),
            cwd.join(".psychevo"),
            crate::types::PermissionConfig::default(),
            crate::types::PermissionMode::Default,
            None,
            None,
        );
        let bypass_permissions = PermissionRuntime::new(
            cwd.clone(),
            cwd.join(".psychevo"),
            crate::types::PermissionConfig::default(),
            crate::types::PermissionMode::BypassPermissions,
            None,
            None,
        );

        assert_ne!(
            mcp_connection_identity_hash(
                std::slice::from_ref(&input),
                &cwd,
                Some(&default_permissions),
                false,
            ),
            mcp_connection_identity_hash(&[input], &cwd, Some(&bypass_permissions), false)
        );
    }

    #[test]
    fn connection_identity_hash_separates_read_only_mcp_surface() {
        let input = McpServerInput::new(
            "repo",
            McpTransportInput::Unsupported {
                kind: "stdio".to_string(),
            },
        );
        let cwd = PathBuf::from("/repo");

        assert_ne!(
            mcp_connection_identity_hash(std::slice::from_ref(&input), &cwd, None, false),
            mcp_connection_identity_hash(&[input], &cwd, None, true),
        );
    }

    #[tokio::test]
    async fn connection_manager_does_not_cache_failed_snapshot() {
        let mut manager = McpConnectionManager::default();
        let cwd = std::env::temp_dir();
        let unavailable = McpServerInput::with_source(
            "repo",
            McpTransportInput::Unsupported {
                kind: "temporarily_unavailable".to_string(),
            },
            "profile:mcp:repo",
            "profile",
        );

        manager
            .snapshot(std::slice::from_ref(&unavailable), &cwd, None, false)
            .await;
        let first_generation = manager.generation();
        assert!(
            manager.cached.is_none(),
            "startup failure must remain retryable"
        );

        manager.snapshot(&[unavailable], &cwd, None, false).await;
        assert_eq!(
            manager.generation(),
            first_generation + 1,
            "the next safe boundary must retry startup"
        );
    }

    #[tokio::test]
    async fn connection_manager_refreshes_only_at_snapshot_boundary() {
        let mut manager = McpConnectionManager::default();
        let cwd = std::env::temp_dir();

        let first = manager.snapshot(&[], &cwd, None, false).await;
        let first_generation = manager.generation();
        let second = manager.snapshot(&[], &cwd, None, false).await;

        assert_eq!(first.snapshot_hash, second.snapshot_hash);
        assert_eq!(manager.generation(), first_generation);

        manager.mark_tools_changed("repo");
        let refreshed = manager.snapshot(&[], &cwd, None, false).await;

        assert_eq!(refreshed.snapshot_hash, first.snapshot_hash);
        assert_eq!(manager.generation(), first_generation + 1);

        manager.mark_all_dirty();
        let refreshed_again = manager.snapshot(&[], &cwd, None, false).await;
        assert_eq!(refreshed_again.snapshot_hash, first.snapshot_hash);
        assert_eq!(manager.generation(), first_generation + 2);
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
    fn first_party_mcp_utilities_hide_selection_internals() {
        let connection_set = Arc::new(McpUtilityConnectionSet {
            connections: BTreeMap::new(),
        });
        let tools: Vec<Arc<dyn ToolBinding>> = vec![
            Arc::new(McpUtilityTool::list_resources(Arc::clone(&connection_set))),
            Arc::new(McpUtilityTool::list_resource_templates(Arc::clone(
                &connection_set,
            ))),
            Arc::new(McpUtilityTool::read_resource(Arc::clone(&connection_set))),
            Arc::new(McpUtilityTool::list_prompts(Arc::clone(&connection_set))),
            Arc::new(McpUtilityTool::get_prompt(connection_set)),
        ];

        for tool in tools {
            crate::tests::assert_first_party_tool_declaration_quality(tool.as_ref());
        }
    }

    #[test]
    fn utility_list_is_bounded_with_an_explicit_truncation_diagnostic() {
        let mut values = Vec::new();
        let truncated = append_utility_values(
            &mut values,
            (0..=MCP_UTILITY_LIST_LIMIT).map(|index| json!(index)),
        );

        assert!(truncated);
        assert_eq!(values.len(), MCP_UTILITY_LIST_LIMIT);
        assert_eq!(
            utility_truncation_error(),
            json!({
                "error": format!(
                    "results truncated at {MCP_UTILITY_LIST_LIMIT} entries"
                )
            })
        );
    }

    #[test]
    fn external_mcp_description_keeps_source_owned_wording() {
        let source_description = "Remote harness vocabulary remains source-owned.";
        let description =
            mcp_tool_description("docs", "search", Some("Search"), Some(source_description));

        assert!(description.ends_with(source_description), "{description}");
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
