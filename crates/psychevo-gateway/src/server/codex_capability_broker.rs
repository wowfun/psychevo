use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use psychevo_runtime::{Error, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::time::timeout;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const ELICITATION_TIMEOUT: Duration = Duration::from_secs(120);
const ELICITATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(125);

#[derive(Debug, Clone)]
struct BrokerCommand {
    program: PathBuf,
    args: Vec<String>,
}

impl BrokerCommand {
    fn from_env(env: &BTreeMap<String, String>) -> Self {
        let program = env
            .get("PSYCHEVO_CODEX_BIN")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        Self {
            program,
            args: vec![
                "app-server".to_string(),
                "--listen".to_string(),
                "stdio://".to_string(),
            ],
        }
    }
}

pub(super) struct CodexCapabilityBroker {
    command: BrokerCommand,
    env: BTreeMap<String, String>,
    request_timeout: Duration,
    process: Mutex<Option<BrokerProcess>>,
    thread_ids: Mutex<BTreeMap<String, String>>,
    runtime_profiles: Mutex<BTreeMap<String, CodexRuntimeProfile>>,
}

pub(super) struct CodexRuntimeContributions {
    pub(super) capability_roots: Vec<psychevo_runtime::SelectedCapabilityRoot>,
    pub(super) runtime_tools: Vec<psychevo_runtime::RuntimeTool>,
    pub(super) warnings: Vec<String>,
}

struct RuntimePluginDetail {
    identity: CodexPluginIdentity,
    plugin_id: String,
    plugin: Value,
    package_root: Option<PathBuf>,
}

#[derive(Clone)]
struct CodexRuntimeProfile {
    capability_roots: Vec<psychevo_runtime::SelectedCapabilityRoot>,
    delegated_servers: BTreeSet<String>,
    warnings: Vec<String>,
}

impl CodexCapabilityBroker {
    pub(super) fn new(env: &BTreeMap<String, String>) -> Self {
        Self {
            command: BrokerCommand::from_env(env),
            env: env.clone(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            process: Mutex::new(None),
            thread_ids: Mutex::new(BTreeMap::new()),
            runtime_profiles: Mutex::new(BTreeMap::new()),
        }
    }

    #[cfg(test)]
    fn with_command(
        command: BrokerCommand,
        env: BTreeMap<String, String>,
        request_timeout: Duration,
    ) -> Self {
        Self {
            command,
            env,
            request_timeout,
            process: Mutex::new(None),
            thread_ids: Mutex::new(BTreeMap::new()),
            runtime_profiles: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_with_context(method, params, None).await
    }

    async fn request_with_context(
        &self,
        method: &str,
        params: Value,
        context: Option<&CodexElicitationContext>,
    ) -> Result<Value> {
        let mut slot = self.process.lock().await;
        if slot.is_none() {
            *slot =
                Some(BrokerProcess::spawn(&self.command, &self.env, self.request_timeout).await?);
        }
        let result = slot
            .as_mut()
            .expect("broker process initialized")
            .request_with_context(
                method,
                params,
                if context.is_some() {
                    ELICITATION_REQUEST_TIMEOUT
                } else {
                    self.request_timeout
                },
                context,
            )
            .await;
        if result.is_err()
            && let Some(mut process) = slot.take()
        {
            let _ = process.child.kill().await;
        }
        result
    }

    pub(super) async fn plugin_list(&self, cwd: &std::path::Path) -> Result<Value> {
        self.request(
            "plugin/list",
            json!({
                "cwds": [cwd],
                "marketplaceKinds": null,
            }),
        )
        .await
    }

    pub(super) async fn plugin_read(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        self.plugin_read_target(identity, &target).await
    }

    pub(super) async fn plugin_install(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        self.request(
            "plugin/install",
            json!({
                "pluginName": identity.plugin,
                "marketplacePath": target.marketplace_path,
                "remoteMarketplaceName": target.remote_marketplace_name,
            }),
        )
        .await
    }

    pub(super) async fn plugin_uninstall(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        self.request("plugin/uninstall", json!({"pluginId": target.plugin_id}))
            .await
    }

    #[cfg(test)]
    pub(super) async fn stop(&self) {
        if let Some(mut process) = self.process.lock().await.take() {
            let _ = process.child.kill().await;
        }
    }

    async fn ensure_ephemeral_thread(
        &self,
        psychevo_thread_id: &str,
        cwd: &std::path::Path,
    ) -> Result<String> {
        let mut thread_ids = self.thread_ids.lock().await;
        if let Some(thread_id) = thread_ids.get(psychevo_thread_id) {
            return Ok(thread_id.clone());
        }
        let response = self
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "ephemeral": true,
                }),
            )
            .await?;
        let thread_id = response
            .pointer("/thread/id")
            .or_else(|| response.get("threadId"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Message("Codex broker thread/start response has no thread id".to_string())
            })?
            .to_string();
        thread_ids.insert(psychevo_thread_id.to_string(), thread_id.clone());
        Ok(thread_id)
    }

    pub(super) async fn archive_ephemeral_thread(&self, psychevo_thread_id: &str) {
        self.runtime_profiles
            .lock()
            .await
            .remove(psychevo_thread_id);
        let thread_id = self.thread_ids.lock().await.remove(psychevo_thread_id);
        if let Some(thread_id) = thread_id {
            let _ = self
                .request("thread/archive", json!({"threadId":thread_id}))
                .await;
        }
    }

    pub(super) async fn runtime_contributions(
        &self,
        state: super::WebState,
        cwd: &std::path::Path,
        psychevo_thread_id: &str,
        turn_id: Option<String>,
        event_sink: Option<super::GatewayEventSink>,
    ) -> Result<CodexRuntimeContributions> {
        let profile = self.runtime_profile(cwd, psychevo_thread_id).await?;
        let CodexRuntimeProfile {
            capability_roots,
            delegated_servers,
            warnings,
        } = profile;
        if delegated_servers.is_empty() {
            return Ok(CodexRuntimeContributions {
                capability_roots,
                runtime_tools: Vec::new(),
                warnings,
            });
        }
        let codex_thread_id = self
            .ensure_ephemeral_thread(psychevo_thread_id, cwd)
            .await?;
        let inventory = self
            .request(
                "mcpServerStatus/list",
                json!({
                    "threadId": codex_thread_id,
                    "detail": "toolsAndAuthOnly",
                }),
            )
            .await?;
        let mut tools = Vec::new();
        for server in inventory
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(server_name) = server.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !delegated_servers.contains(server_name) {
                continue;
            }
            let Some(server_tools) = server.get("tools").and_then(Value::as_object) else {
                continue;
            };
            for (tool_name, descriptor) in server_tools {
                let model_name = format!("mcp__{server_name}__{tool_name}");
                let description = descriptor
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex App tool")
                    .to_string();
                let parameters = descriptor
                    .get("inputSchema")
                    .or_else(|| descriptor.get("input_schema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                tools.push(psychevo_runtime::RuntimeTool::with_source(
                    std::sync::Arc::new(CodexMcpTool {
                        state: state.clone(),
                        psychevo_thread_id: psychevo_thread_id.to_string(),
                        codex_thread_id: codex_thread_id.clone(),
                        turn_id: turn_id.clone(),
                        event_sink: event_sink.clone(),
                        name: model_name,
                        server_name: server_name.to_string(),
                        remote_name: tool_name.clone(),
                        description,
                        parameters,
                    }),
                    format!("codex:mcp:{server_name}"),
                    "codex_capability_broker",
                ));
            }
        }
        Ok(CodexRuntimeContributions {
            capability_roots,
            runtime_tools: tools,
            warnings,
        })
    }

    async fn runtime_profile(
        &self,
        cwd: &Path,
        psychevo_thread_id: &str,
    ) -> Result<CodexRuntimeProfile> {
        if let Some(profile) = self
            .runtime_profiles
            .lock()
            .await
            .get(psychevo_thread_id)
            .cloned()
        {
            return Ok(profile);
        }
        let (mut plugins, mut warnings) = self.enabled_plugin_details(cwd).await?;
        self.resolve_hook_package_roots(cwd, &mut plugins, &mut warnings)
            .await;

        let mut capability_roots = Vec::new();
        let mut delegated_servers = BTreeSet::new();
        for plugin in &plugins {
            if let Some(root) = &plugin.package_root {
                capability_roots.push(psychevo_runtime::SelectedCapabilityRoot::codex_local(
                    plugin.identity.selector(),
                    plugin.identity.plugin.clone(),
                    plugin.identity.marketplace.clone(),
                    root,
                ));
            } else {
                delegated_servers.extend(
                    plugin
                        .plugin
                        .get("mcpServers")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
                if plugin
                    .plugin
                    .get("skills")
                    .and_then(Value::as_array)
                    .is_some_and(|skills| !skills.is_empty())
                    || plugin
                        .plugin
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|hooks| !hooks.is_empty())
                {
                    warnings.push(format!(
                        "Codex plugin `{}` exposes portable components but no installed package root",
                        plugin.identity.canonical_id()
                    ));
                }
            }
            if plugin
                .plugin
                .get("apps")
                .and_then(Value::as_array)
                .is_some_and(|apps| !apps.is_empty())
            {
                delegated_servers.insert("codex_apps".to_string());
            }
        }
        capability_roots.sort_by(|left, right| left.id.cmp(&right.id));
        capability_roots.dedup_by(|left, right| left.id == right.id);

        let profile = CodexRuntimeProfile {
            capability_roots,
            delegated_servers,
            warnings,
        };
        let mut profiles = self.runtime_profiles.lock().await;
        Ok(profiles
            .entry(psychevo_thread_id.to_string())
            .or_insert(profile)
            .clone())
    }

    async fn enabled_plugin_details(
        &self,
        cwd: &Path,
    ) -> Result<(Vec<RuntimePluginDetail>, Vec<String>)> {
        let catalog = self.plugin_list(cwd).await?;
        let mut details = Vec::new();
        let mut warnings = Vec::new();
        for marketplace in catalog
            .get("marketplaces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(marketplace_name) = marketplace.get("name").and_then(Value::as_str) else {
                continue;
            };
            for summary in marketplace
                .get("plugins")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if summary.get("installed").and_then(Value::as_bool) != Some(true)
                    || summary.get("enabled").and_then(Value::as_bool) != Some(true)
                {
                    continue;
                }
                let Some(plugin_name) = summary.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let identity = CodexPluginIdentity {
                    plugin: plugin_name.to_string(),
                    marketplace: marketplace_name.to_string(),
                };
                let target = match find_catalog_plugin(&catalog, &identity) {
                    Ok(target) => target,
                    Err(err) => {
                        warnings.push(err.to_string());
                        continue;
                    }
                };
                let plugin_id = target.plugin_id.clone();
                match self.plugin_read_target(&identity, &target).await {
                    Ok(detail) => {
                        let plugin = detail.get("plugin").cloned().unwrap_or(detail);
                        let package_root = codex_package_root(&plugin);
                        details.push(RuntimePluginDetail {
                            identity,
                            plugin_id,
                            plugin,
                            package_root,
                        });
                    }
                    Err(err) => warnings.push(format!(
                        "Codex plugin `{}` could not be projected: {err}",
                        identity.canonical_id()
                    )),
                }
            }
        }
        Ok((details, warnings))
    }

    async fn plugin_read_target(
        &self,
        identity: &CodexPluginIdentity,
        target: &CatalogPluginTarget,
    ) -> Result<Value> {
        self.request(
            "plugin/read",
            json!({
                "pluginName": identity.plugin,
                "marketplacePath": target.marketplace_path,
                "remoteMarketplaceName": target.remote_marketplace_name,
            }),
        )
        .await
    }

    async fn resolve_hook_package_roots(
        &self,
        cwd: &Path,
        plugins: &mut [RuntimePluginDetail],
        warnings: &mut Vec<String>,
    ) {
        let unresolved_ids = plugins
            .iter()
            .filter(|plugin| {
                plugin.package_root.is_none()
                    && plugin
                        .plugin
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|hooks| !hooks.is_empty())
            })
            .map(|plugin| plugin.plugin_id.clone())
            .collect::<BTreeSet<_>>();
        if unresolved_ids.is_empty() {
            return;
        }
        let hooks = match self.request("hooks/list", json!({"cwds":[cwd]})).await {
            Ok(hooks) => hooks,
            Err(err) => {
                warnings.push(format!("Codex hook roots could not be resolved: {err}"));
                return;
            }
        };
        for hook in hooks
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
        {
            let Some(plugin_id) = hook.get("pluginId").and_then(Value::as_str) else {
                continue;
            };
            if !unresolved_ids.contains(plugin_id) {
                continue;
            }
            let Some(source_path) = hook.get("sourcePath").and_then(Value::as_str) else {
                continue;
            };
            if let Some(root) = find_codex_package_root(Path::new(source_path))
                && let Some(plugin) = plugins
                    .iter_mut()
                    .find(|plugin| plugin.plugin_id == plugin_id)
            {
                plugin.package_root = Some(root);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexPluginIdentity {
    pub(super) plugin: String,
    pub(super) marketplace: String,
}

impl CodexPluginIdentity {
    pub(super) fn parse_selector(selector: &str) -> Result<Option<Self>> {
        let Some(value) = selector.strip_prefix("codex:") else {
            return Ok(None);
        };
        let Some((plugin, marketplace)) = value.rsplit_once('@') else {
            return Err(Error::Message(
                "Codex plugin selector must be `codex:<plugin>@<marketplace>`".to_string(),
            ));
        };
        if plugin.trim().is_empty() || marketplace.trim().is_empty() {
            return Err(Error::Message(
                "Codex plugin selector must include plugin and marketplace".to_string(),
            ));
        }
        Ok(Some(Self {
            plugin: plugin.to_string(),
            marketplace: marketplace.to_string(),
        }))
    }

    fn canonical_id(&self) -> String {
        format!("{}@{}", self.plugin, self.marketplace)
    }

    fn selector(&self) -> String {
        format!("codex:{}", self.canonical_id())
    }
}

struct CatalogPluginTarget {
    marketplace_path: Option<String>,
    remote_marketplace_name: Option<String>,
    plugin_id: String,
}

fn codex_package_root(plugin: &Value) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if plugin
        .pointer("/summary/source/type")
        .and_then(Value::as_str)
        == Some("local")
        && let Some(path) = plugin
            .pointer("/summary/source/path")
            .and_then(Value::as_str)
    {
        candidates.push(PathBuf::from(path));
    }
    candidates.extend(
        plugin
            .get("skills")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|skill| skill.get("path").and_then(Value::as_str))
            .map(PathBuf::from),
    );
    for key in ["composerIcon", "logo", "logoDark"] {
        if let Some(path) = plugin
            .pointer(&format!("/summary/interface/{key}"))
            .and_then(Value::as_str)
        {
            candidates.push(PathBuf::from(path));
        }
    }
    candidates.extend(
        plugin
            .pointer("/summary/interface/screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from),
    );
    candidates
        .into_iter()
        .find_map(|path| find_codex_package_root(&path))
}

fn find_codex_package_root(path: &Path) -> Option<PathBuf> {
    let mut cursor = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    loop {
        if cursor.join(".codex-plugin").join("plugin.json").is_file() {
            return Some(cursor);
        }
        if !cursor.pop() {
            return None;
        }
    }
}

fn find_catalog_plugin(
    catalog: &Value,
    identity: &CodexPluginIdentity,
) -> Result<CatalogPluginTarget> {
    let marketplaces = catalog
        .get("marketplaces")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Message("Codex plugin catalog response is malformed".to_string()))?;
    let marketplace = marketplaces
        .iter()
        .find(|marketplace| {
            marketplace.get("name").and_then(Value::as_str) == Some(&identity.marketplace)
        })
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex marketplace not found: {}",
                identity.marketplace
            ))
        })?;
    let plugin = marketplace
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some(&identity.plugin))
        })
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex plugin not found: {}",
                identity.canonical_id()
            ))
        })?;
    let marketplace_path = marketplace
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(CatalogPluginTarget {
        remote_marketplace_name: marketplace_path
            .is_none()
            .then(|| identity.marketplace.clone()),
        marketplace_path,
        plugin_id: plugin
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Message("Codex plugin catalog item has no id".to_string()))?
            .to_string(),
    })
}

pub(super) fn merge_plugin_list(mut native: Value, codex: Result<Value>) -> Value {
    let Some(native_object) = native.as_object_mut() else {
        return native;
    };
    let plugins = native_object
        .entry("plugins")
        .or_insert_with(|| json!([]))
        .as_array_mut();
    let Some(plugins) = plugins else {
        return native;
    };
    let codex_authority = match codex {
        Ok(catalog) => {
            for marketplace in catalog
                .get("marketplaces")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let marketplace_name = marketplace
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                for plugin in marketplace
                    .get("plugins")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = plugin.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let identity = CodexPluginIdentity {
                        plugin: name.to_string(),
                        marketplace: marketplace_name.to_string(),
                    };
                    let installed = plugin
                        .get("installed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let enabled = plugin
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let description = plugin
                        .pointer("/interface/shortDescription")
                        .cloned()
                        .unwrap_or(Value::Null);
                    plugins.push(json!({
                        "name": name,
                        "selector": identity.selector(),
                        "canonical_id": identity.canonical_id(),
                        "authority": {
                            "kind": "codex",
                            "plugin": name,
                            "marketplace": marketplace_name,
                        },
                        "scope_name": "codex_home",
                        "enablement_scope_name": "codex_home",
                        "removable": installed,
                        "package_mutable": true,
                        "enablement_mutable": false,
                        "version": plugin.get("localVersion").or_else(|| plugin.get("version")).cloned(),
                        "description": description,
                        "source_id": format!("codex:{marketplace_name}"),
                        "source": marketplace_name,
                        "source_kind": "codex_marketplace",
                        "scope": "codex_home",
                        "manifest_kind": "codex",
                        "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
                        "component_statuses": [],
                        "enabled": enabled,
                        "installed": installed,
                        "readiness": if installed && enabled { "Ready" } else if installed { "Disabled" } else { "Available" },
                        "status": if installed { "Installed" } else { "Available" },
                        "interface": plugin.get("interface").cloned(),
                        "keywords": plugin.get("keywords").cloned().unwrap_or_else(|| json!([])),
                    }));
                }
            }
            json!({"readiness":"ready","owner":"codex"})
        }
        Err(err) => json!({
            "readiness":"unavailable",
            "owner":"codex",
            "reason":err.to_string(),
        }),
    };
    let count = plugins.len();
    let _ = plugins;
    native_object.insert("codex_authority".to_string(), codex_authority);
    native_object.insert("count".to_string(), json!(count));
    native
}

pub(super) fn codex_plugin_read_value(identity: &CodexPluginIdentity, detail: Value) -> Value {
    let plugin = detail.get("plugin").cloned().unwrap_or(detail);
    let native_package_root = codex_package_root(&plugin).is_some();
    let summary = plugin.get("summary").cloned().unwrap_or_else(|| json!({}));
    let installed = summary
        .get("installed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let enabled = summary
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ready = installed && enabled;
    let mut statuses = Vec::new();
    let component = |component: &str, level: &str, owner: &str, readiness: &str, reason: &str| {
        json!({
            "component": component,
            "compatibilityProfile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "highestLevel": level,
            "executionOwner": owner,
            "readiness": readiness,
            "reason": reason,
        })
    };
    if plugin
        .get("skills")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "skills",
            if native_package_root {
                "execute"
            } else {
                "inspect"
            },
            if native_package_root {
                "psychevo_native"
            } else {
                "metadata_only"
            },
            if !native_package_root {
                "unavailable"
            } else if ready {
                "ready"
            } else {
                "disabled"
            },
            if native_package_root {
                "portable skill content is projected from the Codex-owned installed package"
            } else {
                "Codex did not expose an installed package root for native skill projection"
            },
        ));
    }
    if plugin
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "hooks",
            if native_package_root {
                "execute"
            } else {
                "inspect"
            },
            if native_package_root {
                "psychevo_native"
            } else {
                "metadata_only"
            },
            if native_package_root {
                "needs_trust"
            } else {
                "unavailable"
            },
            if native_package_root {
                "hook declarations require Psychevo normalized-hash trust"
            } else {
                "Codex did not expose an installed package root for native hook projection"
            },
        ));
    }
    if plugin
        .get("mcpServers")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "mcp_servers",
            if native_package_root {
                "execute"
            } else {
                "delegate"
            },
            if native_package_root {
                "psychevo_native"
            } else {
                "codex_broker"
            },
            if ready { "ready" } else { "disabled" },
            if native_package_root {
                "ordinary MCP declarations use Psychevo MCP policy"
            } else {
                "Codex retains MCP configuration and execution authority"
            },
        ));
    }
    if plugin
        .get("apps")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "apps",
            "delegate",
            "codex_broker",
            if ready { "ready" } else { "needs_setup" },
            "Apps inventory, authentication, and tool calls remain Codex-owned",
        ));
    }
    json!({
        "plugin": {
            "name": identity.plugin,
            "selector": identity.selector(),
            "canonical_id": identity.canonical_id(),
            "authority": {
                "kind": "codex",
                "plugin": identity.plugin,
                "marketplace": identity.marketplace,
            },
            "version": summary.get("localVersion").or_else(|| summary.get("version")).cloned(),
            "description": plugin.get("description").cloned(),
            "source_id": format!("codex:{}", identity.marketplace),
            "scope_name": "codex_home",
            "manifest_kind": "codex",
            "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "component_statuses": statuses,
            "installed": installed,
            "enabled": enabled,
            "readiness": if ready { "Ready" } else if installed { "Disabled" } else { "Available" },
            "status": if installed { "Installed" } else { "Available" },
            "interface": summary.get("interface").cloned(),
        },
        "manifest": plugin,
        "inspection": {
            "authority": "codex",
            "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "component_statuses": statuses,
        }
    })
}

#[derive(Clone)]
struct CodexMcpTool {
    state: super::WebState,
    psychevo_thread_id: String,
    codex_thread_id: String,
    turn_id: Option<String>,
    event_sink: Option<super::GatewayEventSink>,
    name: String,
    server_name: String,
    remote_name: String,
    description: String,
    parameters: Value,
}

impl ToolBinding for CodexMcpTool {
    fn name(&self) -> &str {
        &self.name
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

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            let context = CodexElicitationContext {
                state: tool.state.clone(),
                psychevo_thread_id: tool.psychevo_thread_id,
                turn_id: tool.turn_id,
                event_sink: tool.event_sink,
            };
            match tool
                .state
                .inner
                .codex_capability_broker
                .request_with_context(
                    "mcpServer/tool/call",
                    json!({
                        "threadId": tool.codex_thread_id,
                        "server": tool.server_name,
                        "tool": tool.remote_name,
                        "arguments": args,
                    }),
                    Some(&context),
                )
                .await
            {
                Ok(value) if value.get("isError").and_then(Value::as_bool) == Some(true) => {
                    ToolOutput::error(value.to_string())
                }
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

struct CodexElicitationContext {
    state: super::WebState,
    psychevo_thread_id: String,
    turn_id: Option<String>,
    event_sink: Option<super::GatewayEventSink>,
}

impl CodexElicitationContext {
    async fn route(&self, message: &Value) -> Value {
        let Some(event_sink) = self.event_sink.as_ref() else {
            return declined_elicitation();
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let projection = project_elicitation(&params);
        let action_id = format!("codex-elicitation:{}", uuid::Uuid::now_v7());
        let (responder, receiver) = oneshot::channel();
        self.state
            .inner
            .codex_elicitations
            .lock()
            .expect("Codex elicitation registry poisoned")
            .insert(
                action_id.clone(),
                PendingCodexElicitation {
                    fields: projection.fields,
                    mode: projection.mode,
                    meta: params.get("_meta").cloned(),
                    event_sink: event_sink.clone(),
                    responder,
                },
            );
        event_sink(super::GatewayEvent::ActionRequested {
            action: super::PendingActionView {
                action_id: action_id.clone(),
                kind: super::GatewayActionKind::Clarify,
                title: Some("Codex App request".to_string()),
                summary: params
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                payload: json!({
                    "owner": "codex_capability_broker",
                    "raw": projection.raw,
                }),
                thread_id: Some(self.psychevo_thread_id.clone()),
                turn_id: self.turn_id.clone(),
                activity_id: None,
                source_key: None,
                owner_id: None,
                lease_expires_at_ms: None,
            },
        });
        let decision = tokio::time::timeout(ELICITATION_TIMEOUT, receiver).await;
        self.state
            .inner
            .codex_elicitations
            .lock()
            .expect("Codex elicitation registry poisoned")
            .remove(&action_id);
        match decision {
            Ok(Ok(result)) => result,
            _ => {
                event_sink(super::GatewayEvent::ActionResolved {
                    action_id,
                    kind: super::GatewayActionKind::Clarify,
                    outcome: super::GatewayActionOutcome::TimedOut,
                    payload: json!({"owner":"codex_capability_broker"}),
                });
                json!({"action":"cancel","content":null,"_meta":null})
            }
        }
    }
}

fn declined_elicitation() -> Value {
    json!({"action":"decline","content":null,"_meta":null})
}

#[derive(Debug, Clone)]
enum ElicitationFieldKind {
    String,
    Boolean,
    Number,
    StringArray,
    UrlAcceptance,
}

#[derive(Debug, Clone)]
struct ElicitationField {
    name: String,
    kind: ElicitationFieldKind,
    required: bool,
}

pub(super) struct PendingCodexElicitation {
    fields: Vec<ElicitationField>,
    mode: String,
    meta: Option<Value>,
    event_sink: super::GatewayEventSink,
    responder: oneshot::Sender<Value>,
}

struct ElicitationProjection {
    fields: Vec<ElicitationField>,
    mode: String,
    raw: Value,
}

fn project_elicitation(params: &Value) -> ElicitationProjection {
    let mode = params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("form")
        .to_string();
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("The Codex App needs more information.");
    if mode == "url" {
        let url = params
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return ElicitationProjection {
            fields: vec![ElicitationField {
                name: "url".to_string(),
                kind: ElicitationFieldKind::UrlAcceptance,
                required: true,
            }],
            mode,
            raw: json!({
                "url": url,
                "questions": [{
                    "header": "Open link",
                    "question": message,
                    "options": [{"label":"Open","description":url}],
                    "multiple": false,
                    "custom": false,
                    "secret": false,
                }]
            }),
        };
    }
    let schema = params.get("requestedSchema").and_then(Value::as_object);
    let properties = schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object);
    let required = schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut fields = Vec::new();
    let mut questions = Vec::new();
    for (name, property) in properties.into_iter().flatten() {
        let property_type = property
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string");
        let kind = match property_type {
            "boolean" => ElicitationFieldKind::Boolean,
            "number" | "integer" => ElicitationFieldKind::Number,
            "array" => ElicitationFieldKind::StringArray,
            _ => ElicitationFieldKind::String,
        };
        let options = elicitation_options(property, property_type);
        let has_options = !options.is_empty();
        let question = property
            .get("description")
            .or_else(|| property.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(message);
        questions.push(json!({
            "header": name,
            "question": question,
            "options": options,
            "multiple": property_type == "array",
            "custom": property_type != "boolean" && !has_options,
            "secret": property.get("format").and_then(Value::as_str) == Some("password"),
            "required": required.contains(name.as_str()),
        }));
        fields.push(ElicitationField {
            name: name.clone(),
            kind,
            required: required.contains(name.as_str()),
        });
    }
    ElicitationProjection {
        fields,
        mode,
        raw: json!({"questions":questions}),
    }
}

fn elicitation_options(property: &Value, property_type: &str) -> Vec<Value> {
    if property_type == "openai/imagePicker" {
        return property
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?;
                let title = item.get("title").and_then(Value::as_str).unwrap_or("");
                let image = item.get("image").and_then(Value::as_str);
                Some(json!({"label":id,"description":title,"image":image}))
            })
            .collect();
    }
    let values = property
        .get("enum")
        .and_then(Value::as_array)
        .map(|values| {
            let names = property.get("enumNames").and_then(Value::as_array);
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let label = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    let description = names
                        .and_then(|names| names.get(index))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    json!({"label":label,"description":description})
                })
                .collect::<Vec<_>>()
        })
        .or_else(|| titled_elicitation_options(property.get("oneOf")))
        .or_else(|| {
            let items = property.get("items")?;
            items
                .get("enum")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| {
                            let label = value
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| value.to_string());
                            json!({"label":label,"description":""})
                        })
                        .collect::<Vec<_>>()
                })
                .or_else(|| {
                    titled_elicitation_options(items.get("anyOf").or_else(|| items.get("oneOf")))
                })
        });
    values.unwrap_or_else(|| {
        if property_type == "boolean" {
            vec![
                json!({"label":"Yes","description":""}),
                json!({"label":"No","description":""}),
            ]
        } else {
            Vec::new()
        }
    })
}

fn titled_elicitation_options(options: Option<&Value>) -> Option<Vec<Value>> {
    options.and_then(Value::as_array).map(|options| {
        options
            .iter()
            .filter_map(|option| {
                let value = option.get("const")?.as_str()?;
                let title = option.get("title").and_then(Value::as_str).unwrap_or("");
                Some(json!({"label":value,"description":title}))
            })
            .collect()
    })
}

pub(super) fn respond_to_elicitation(
    state: &super::WebState,
    interaction_id: &str,
    response: psychevo_gateway_protocol::ThreadInteractionResponse,
) -> Result<Option<psychevo_gateway_protocol::ThreadInteractionRespondResult>> {
    if !matches!(
        response,
        psychevo_gateway_protocol::ThreadInteractionResponse::Clarify { .. }
            | psychevo_gateway_protocol::ThreadInteractionResponse::CancelClarify
    ) {
        return Err(Error::Message(
            "Codex App elicitation requires a clarify response".to_string(),
        ));
    }
    let Some(pending) = state
        .inner
        .codex_elicitations
        .lock()
        .expect("Codex elicitation registry poisoned")
        .remove(interaction_id)
    else {
        return Ok(None);
    };
    let (result, outcome) = match response {
        psychevo_gateway_protocol::ThreadInteractionResponse::Clarify { answers } => {
            let result = accepted_elicitation(&pending, answers);
            (result, super::GatewayActionOutcome::Accepted)
        }
        psychevo_gateway_protocol::ThreadInteractionResponse::CancelClarify => (
            json!({"action":"cancel","content":null,"_meta":pending.meta}),
            super::GatewayActionOutcome::Cancelled,
        ),
        _ => unreachable!("response kind checked before taking the pending elicitation"),
    };
    (pending.event_sink)(super::GatewayEvent::ActionResolved {
        action_id: interaction_id.to_string(),
        kind: super::GatewayActionKind::Clarify,
        outcome,
        payload: json!({"owner":"codex_capability_broker"}),
    });
    let _ = pending.responder.send(result);
    Ok(Some(
        psychevo_gateway_protocol::ThreadInteractionRespondResult {
            accepted: true,
            interaction_id: interaction_id.to_string(),
            outcome,
        },
    ))
}

fn accepted_elicitation(pending: &PendingCodexElicitation, answers: Vec<Vec<String>>) -> Value {
    if pending.mode == "url" {
        let action = if answers
            .first()
            .and_then(|answers| answers.first())
            .is_some_and(|answer| answer == "Open")
        {
            "accept"
        } else {
            "decline"
        };
        return json!({"action":action,"content":null,"_meta":pending.meta});
    }
    let mut content = serde_json::Map::new();
    for (index, field) in pending.fields.iter().enumerate() {
        let values = answers.get(index).cloned().unwrap_or_default();
        if values.is_empty() && !field.required {
            continue;
        }
        let value = match field.kind {
            ElicitationFieldKind::Boolean => values
                .first()
                .map(|value| Value::Bool(value.eq_ignore_ascii_case("yes") || value == "true"))
                .unwrap_or(Value::Null),
            ElicitationFieldKind::Number => values
                .first()
                .and_then(|value| value.parse::<serde_json::Number>().ok())
                .map(Value::Number)
                .unwrap_or(Value::Null),
            ElicitationFieldKind::StringArray => {
                Value::Array(values.into_iter().map(Value::String).collect())
            }
            ElicitationFieldKind::String | ElicitationFieldKind::UrlAcceptance => values
                .into_iter()
                .next()
                .map(Value::String)
                .unwrap_or(Value::Null),
        };
        content.insert(field.name.clone(), value);
    }
    json!({"action":"accept","content":content,"_meta":pending.meta})
}

struct BrokerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl BrokerProcess {
    async fn spawn(
        broker_command: &BrokerCommand,
        env: &BTreeMap<String, String>,
        request_timeout: Duration,
    ) -> Result<Self> {
        let mut command = Command::new(&broker_command.program);
        command
            .args(&broker_command.args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|err| {
            Error::Message(format!(
                "Codex capability broker could not start `{}`: {err}",
                broker_command.program.display()
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Message("Codex broker stdin unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Message("Codex broker stdout unavailable".to_string()))?;
        let mut process = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        };
        process
            .request_with_context(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "psychevo-capability-broker",
                        "title": "Psychevo",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "experimentalApi": true,
                        "mcpServerOpenaiFormElicitation": true,
                    }
                }),
                request_timeout,
                None,
            )
            .await?;
        process
            .write_message(json!({"jsonrpc":"2.0","method":"initialized"}))
            .await?;
        Ok(process)
    }

    async fn request_with_context(
        &mut self,
        method: &str,
        params: Value,
        request_timeout: Duration,
        context: Option<&CodexElicitationContext>,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        timeout(request_timeout, self.read_response(id, context))
            .await
            .map_err(|_| {
                Error::Message(format!(
                    "Codex capability broker request `{method}` timed out"
                ))
            })?
    }

    async fn read_response(
        &mut self,
        expected_id: u64,
        context: Option<&CodexElicitationContext>,
    ) -> Result<Value> {
        loop {
            let line = self
                .stdout
                .next_line()
                .await
                .map_err(|err| Error::Message(format!("Codex broker read failed: {err}")))?
                .ok_or_else(|| {
                    Error::Message("Codex capability broker exited unexpectedly".to_string())
                })?;
            let message: Value = serde_json::from_str(&line).map_err(|err| {
                Error::Message(format!("Codex broker returned invalid JSON: {err}"))
            })?;
            if message.get("method").is_some() && message.get("id").is_some() {
                self.respond_to_server_request(&message, context).await?;
                continue;
            }
            let Some(id) = message.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if id != expected_id {
                return Err(Error::Message(format!(
                    "Codex broker response id mismatch: expected {expected_id}, got {id}"
                )));
            }
            if let Some(error) = message.get("error") {
                return Err(Error::Message(format!(
                    "Codex broker request failed: {}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown JSON-RPC error")
                )));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn respond_to_server_request(
        &mut self,
        message: &Value,
        context: Option<&CodexElicitationContext>,
    ) -> Result<()> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let response = if method == "mcpServer/elicitation/request" {
            let result = if let Some(context) = context {
                context.route(message).await
            } else {
                json!({"action":"decline","content":null,"_meta":null})
            };
            json!({"jsonrpc":"2.0","id":id,"result":result})
        } else {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code":-32601,"message":format!("unsupported broker callback: {method}")},
            })
        };
        self.write_message(response).await
    }

    async fn write_message(&mut self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message)?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|err| Error::Message(format!("Codex broker write failed: {err}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|err| Error::Message(format!("Codex broker flush failed: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[tokio::test]
    async fn broker_handshakes_reuses_process_and_declines_unrouted_elicitation() {
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("fake-codex.py");
        fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json, sys
initialized = False
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{"codexHome":"/fake","platformFamily":"unix","platformOs":"linux","userAgent":"fake"}}), flush=True)
    elif method == "initialized":
        initialized = True
    elif method == "plugin/list":
        assert initialized
        print(json.dumps({"jsonrpc":"2.0","id":900,"method":"mcpServer/elicitation/request","params":{"request":{"mode":"form"}}}), flush=True)
        response = json.loads(sys.stdin.readline())
        assert response["result"]["action"] == "decline"
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{"marketplaces":[{"name":"openai","path":None,"plugins":[{"id":"review@openai","name":"review","installed":False,"enabled":False}]}],"marketplaceLoadErrors":[],"featuredPluginIds":[]}}), flush=True)
    elif method == "plugin/read":
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{"plugin":{"marketplaceName":"openai","summary":{"name":"review","installed":False,"enabled":False},"description":"Review","skills":[{"name":"review"}],"hooks":[],"apps":[{"id":"review-app","name":"Review"}],"mcpServers":[]}}}), flush=True)
    elif method == "plugin/install":
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{"authPolicy":"ON_USE","appsNeedingAuth":[]}}), flush=True)
    elif method == "plugin/uninstall":
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{}}), flush=True)
    elif method == "app/list":
        print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":{"data":[],"nextCursor":None}}), flush=True)
"#,
        )
        .expect("script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("chmod");
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(3),
        );

        let plugins = broker
            .request("plugin/list", json!({"cwds":[]}))
            .await
            .expect("plugin list");
        let identity = CodexPluginIdentity::parse_selector("codex:review@openai")
            .expect("selector")
            .expect("codex identity");
        let detail = broker
            .plugin_read(temp.path(), &identity)
            .await
            .expect("plugin read");
        let install = broker
            .plugin_install(temp.path(), &identity)
            .await
            .expect("plugin install");
        let uninstall = broker
            .plugin_uninstall(temp.path(), &identity)
            .await
            .expect("plugin uninstall");
        let apps = broker
            .request("app/list", json!({"forceRefetch":false}))
            .await
            .expect("app list");

        assert_eq!(plugins["marketplaces"][0]["name"], "openai");
        assert_eq!(detail["plugin"]["description"], "Review");
        assert_eq!(install["authPolicy"], "ON_USE");
        assert_eq!(uninstall, json!({}));
        assert_eq!(apps["data"], json!([]));
        broker.stop().await;
    }

    #[test]
    fn list_merge_preserves_authority_qualified_identity() {
        let native = json!({
            "plugins": [{
                "name":"review",
                "selector":"profile:review@local",
                "authority":{"kind":"psychevo","selector":"profile:review@local"}
            }],
            "count":1
        });
        let codex = json!({
            "marketplaces":[{
                "name":"openai",
                "path":null,
                "plugins":[{
                    "id":"review@openai",
                    "name":"review",
                    "installed":false,
                    "enabled":false,
                    "interface":{"shortDescription":"Review"}
                }]
            }]
        });

        let merged = merge_plugin_list(native, Ok(codex));

        assert_eq!(merged["count"], 2);
        assert_eq!(merged["plugins"][0]["selector"], "profile:review@local");
        assert_eq!(merged["plugins"][1]["selector"], "codex:review@openai");
        assert_eq!(merged["plugins"][1]["canonical_id"], "review@openai");
        assert_eq!(merged["plugins"][1]["authority"]["kind"], "codex");
    }

    #[test]
    fn codex_read_projects_component_owners_without_claiming_native_apps() {
        let temp = tempfile::tempdir().expect("temp");
        let package = temp.path().join("review");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(package.join("skills/review")).expect("skill dir");
        fs::write(package.join(".codex-plugin/plugin.json"), "{}").expect("manifest");
        fs::write(package.join("skills/review/SKILL.md"), "# Review").expect("skill");
        let identity = CodexPluginIdentity {
            plugin: "review".to_string(),
            marketplace: "openai".to_string(),
        };
        let value = codex_plugin_read_value(
            &identity,
            json!({"plugin":{
                "summary":{"installed":true,"enabled":true,"source":{"type":"local","path":package}},
                "skills":[{"name":"review","path":package.join("skills/review/SKILL.md")}],
                "hooks":[],
                "mcpServers":[],
                "apps":[{"id":"review-app"}]
            }}),
        );

        assert_eq!(
            value["plugin"]["component_statuses"][0]["executionOwner"],
            "psychevo_native"
        );
        assert_eq!(
            value["plugin"]["component_statuses"][1]["highestLevel"],
            "delegate"
        );
        assert_eq!(
            value["plugin"]["component_statuses"][1]["executionOwner"],
            "codex_broker"
        );
    }

    #[test]
    fn standard_elicitation_projection_preserves_optional_and_titled_choices() {
        let projection = project_elicitation(&json!({
            "mode":"form",
            "message":"Configure",
            "requestedSchema":{
                "type":"object",
                "properties":{
                    "mode":{"type":"string","oneOf":[
                        {"const":"safe","title":"Safe mode"},
                        {"const":"fast","title":"Fast mode"}
                    ]},
                    "tags":{"type":"array","items":{"anyOf":[
                        {"const":"a","title":"Alpha"},
                        {"const":"b","title":"Beta"}
                    ]}}
                },
                "required":["mode"]
            }
        }));

        assert_eq!(
            projection.raw["questions"][0]["options"][0]["label"],
            "safe"
        );
        assert_eq!(
            projection.raw["questions"][0]["options"][0]["description"],
            "Safe mode"
        );
        assert_eq!(projection.raw["questions"][0]["required"], true);
        assert_eq!(projection.raw["questions"][1]["multiple"], true);
        assert_eq!(projection.raw["questions"][1]["required"], false);

        let image_picker = project_elicitation(&json!({
            "mode":"openai/form",
            "message":"Choose a report",
            "requestedSchema":{
                "type":"object",
                "properties":{
                    "template":{
                        "type":"openai/imagePicker",
                        "items":[{
                            "id":"monthly-review",
                            "title":"Monthly review",
                            "image":"data:image/png;base64,AA=="
                        }]
                    }
                },
                "required":["template"]
            }
        }));
        assert_eq!(
            image_picker.raw["questions"][0]["options"][0],
            json!({
                "label":"monthly-review",
                "description":"Monthly review",
                "image":"data:image/png;base64,AA=="
            })
        );
        assert_eq!(image_picker.raw["questions"][0]["custom"], false);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_package_projects_native_root_and_delegated_app_tool_with_elicitation() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let package = temp.path().join("review-plugin");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(package.join("skills/review")).expect("skill dir");
        fs::write(
            package.join(".codex-plugin/plugin.json"),
            r#"{"name":"review","skills":"./skills","apps":"./.app.json"}"#,
        )
        .expect("manifest");
        fs::write(package.join("skills/review/SKILL.md"), "# Review").expect("skill");
        fs::write(package.join(".app.json"), "{}").expect("app");
        let log = temp.path().join("broker.log");
        let script = temp.path().join("fake-codex.py");
        let script_text = format!(
            r#"#!/usr/bin/env python3
import json, sys
PACKAGE = {package}
LOG = {log}
initialized = False
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        assert msg["params"]["capabilities"]["mcpServerOpenaiFormElicitation"] is True
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"codexHome":"/fake","platformFamily":"unix","platformOs":"linux","userAgent":"fake"}}}}), flush=True)
    elif method == "initialized":
        initialized = True
    elif method == "plugin/list":
        assert initialized
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("plugin-list\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"marketplaces":[{{"name":"openai","path":None,"plugins":[{{"id":"review@openai","name":"review","installed":True,"enabled":True}}]}}],"marketplaceLoadErrors":[],"featuredPluginIds":[]}}}}), flush=True)
    elif method == "plugin/read":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"plugin":{{"marketplaceName":"openai","summary":{{"id":"review@openai","name":"review","installed":True,"enabled":True,"source":{{"type":"local","path":PACKAGE}}}},"skills":[{{"name":"review","path":PACKAGE + "/skills/review/SKILL.md","enabled":True}}],"hooks":[],"apps":[{{"id":"review-app"}}],"mcpServers":[]}}}}}}), flush=True)
    elif method == "thread/start":
        assert msg["params"]["ephemeral"] is True
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"thread":{{"id":"codex-thread-1"}}}}}}), flush=True)
    elif method == "mcpServerStatus/list":
        assert msg["params"]["threadId"] == "codex-thread-1"
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"data":[{{"name":"codex_apps","tools":{{"review":{{"description":"Review app","inputSchema":{{"type":"object","properties":{{}}}}}}}}}}],"nextCursor":None}}}}), flush=True)
    elif method == "mcpServer/tool/call":
        print(json.dumps({{"jsonrpc":"2.0","id":9001,"method":"mcpServer/elicitation/request","params":{{"threadId":"codex-thread-1","turnId":"turn-1","serverName":"codex_apps","mode":"form","_meta":{{"source":"test"}},"message":"Continue?","requestedSchema":{{"type":"object","properties":{{"confirmed":{{"type":"boolean"}}}},"required":["confirmed"]}}}}}}), flush=True)
        answer = json.loads(sys.stdin.readline())
        assert answer["result"]["action"] == "accept"
        assert answer["result"]["content"]["confirmed"] is True
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"content":[{{"type":"text","text":"done"}}],"structuredContent":{{"ok":True}},"isError":False}}}}), flush=True)
    elif method == "thread/archive":
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write(msg["params"]["threadId"] + "\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{}}}}), flush=True)
"#,
            package = serde_json::to_string(&package).expect("package json"),
            log = serde_json::to_string(&log).expect("log json"),
        );
        fs::write(&script, script_text).expect("script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("chmod");

        let env = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]);
        let runtime =
            psychevo_runtime::StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = crate::Gateway::new(runtime);
        let config = super::super::GatewayWebServerConfig::new(
            gateway,
            home,
            cwd.clone(),
            None,
            env,
            temp.path().join("static"),
        );
        let state = super::super::WebState::new(config);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_sink = events.clone();
        let event_sink: crate::GatewayEventSink = std::sync::Arc::new(move |event| {
            events_for_sink.lock().expect("events").push(event);
        });

        let contributions = state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &cwd,
                "psychevo-thread-1",
                Some("turn-1".to_string()),
                Some(event_sink.clone()),
            )
            .await
            .expect("runtime contributions");
        assert_eq!(contributions.capability_roots.len(), 1);
        assert!(matches!(
            contributions.capability_roots[0].authority,
            psychevo_runtime::CapabilityRootAuthority::Codex { .. }
        ));
        assert_eq!(contributions.runtime_tools.len(), 1);
        assert_eq!(
            contributions.runtime_tools[0].name(),
            "mcp__codex_apps__review"
        );
        let second = state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &cwd,
                "psychevo-thread-1",
                Some("turn-2".to_string()),
                None,
            )
            .await
            .expect("frozen runtime contributions");
        assert_eq!(second.capability_roots, contributions.capability_roots);

        let tool = CodexMcpTool {
            state: state.clone(),
            psychevo_thread_id: "psychevo-thread-1".to_string(),
            codex_thread_id: "codex-thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            event_sink: Some(event_sink),
            name: "mcp__codex_apps__review".to_string(),
            server_name: "codex_apps".to_string(),
            remote_name: "review".to_string(),
            description: "Review app".to_string(),
            parameters: json!({"type":"object"}),
        };
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let run =
            tokio::spawn(tool.execute("call-1".to_string(), json!({}), AbortSignal::new(abort_rx)));
        let action_id = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(action_id) =
                    events
                        .lock()
                        .expect("events")
                        .iter()
                        .find_map(|event| match event {
                            crate::GatewayEvent::ActionRequested { action } => {
                                Some(action.action_id.clone())
                            }
                            _ => None,
                        })
                {
                    break action_id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("elicitation action");
        let response = respond_to_elicitation(
            &state,
            &action_id,
            psychevo_gateway_protocol::ThreadInteractionResponse::Clarify {
                answers: vec![vec!["Yes".to_string()]],
            },
        )
        .expect("elicitation response")
        .expect("pending elicitation");
        assert!(response.accepted);
        let output = run.await.expect("tool task");
        assert!(!output.is_error);
        assert_eq!(output.json["structuredContent"]["ok"], true);

        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread("psychevo-thread-1")
            .await;
        assert_eq!(
            fs::read_to_string(&log).expect("archive log"),
            "plugin-list\ncodex-thread-1\n"
        );
        state.inner.codex_capability_broker.stop().await;
    }

    #[tokio::test]
    #[ignore = "live Codex app-server opt-in"]
    async fn live_codex_plugin_broker_lists_current_catalog() {
        let env = std::env::vars().collect::<BTreeMap<_, _>>();
        let broker = CodexCapabilityBroker::new(&env);
        let cwd = std::env::current_dir().expect("current directory");

        let catalog = broker
            .plugin_list(&cwd)
            .await
            .expect("current Codex plugin catalog");
        assert!(
            catalog
                .get("marketplaces")
                .and_then(Value::as_array)
                .is_some(),
            "plugin/list must return the pinned marketplaces array: {catalog}"
        );
        let merged = merge_plugin_list(json!({"plugins":[],"count":0}), Ok(catalog));
        assert_eq!(merged["codex_authority"]["readiness"], "ready");
        assert!(merged["plugins"].is_array());
        broker.stop().await;
    }
}
