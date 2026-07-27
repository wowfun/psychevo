use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::config::{
    PluginPolicyConfig, ToolsetContribution, load_mcp_oauth_access_token, load_run_config,
    resolve_psychevo_home,
};
use crate::hooks::HookSourceDescriptor;
use crate::host_paths::{ExecutableResolveOptions, HostPlatform, resolve_executable_path};
use crate::paths::canonical_cwd;
use crate::plugins::{load_enabled_plugin_contributions, load_plugin_manifest};
use crate::types::{
    McpServerInput, McpTransportInput, ResolvedMcpServerInput, RunOptions, RunWarning, RuntimeTool,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedCapabilityRoot {
    pub id: String,
    #[serde(default, skip_serializing_if = "CapabilityRootAuthority::is_local")]
    pub authority: CapabilityRootAuthority,
    pub path: PathBuf,
}

impl SelectedCapabilityRoot {
    pub fn local(id: impl Into<String>, path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            id: id.into(),
            authority: CapabilityRootAuthority::Local,
            path: path.into(),
        }
    }

    pub fn codex_local(
        id: impl Into<String>,
        plugin: impl Into<String>,
        marketplace: impl Into<String>,
        path: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            authority: CapabilityRootAuthority::Codex {
                plugin: plugin.into(),
                marketplace: marketplace.into(),
            },
            path: path.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityRootAuthority {
    #[default]
    Local,
    Codex {
        plugin: String,
        marketplace: String,
    },
}

impl CapabilityRootAuthority {
    fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

pub(crate) struct ExtensionAssemblyInput<'a> {
    pub(crate) home: &'a Path,
    pub(crate) cwd: &'a Path,
    pub(crate) env: &'a BTreeMap<String, String>,
    pub(crate) plugin_policy: &'a PluginPolicyConfig,
    pub(crate) selected_capability_roots: &'a [SelectedCapabilityRoot],
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
}

#[derive(Default)]
pub(crate) struct ExtensionAssembly {
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
    pub(crate) skill_inputs: Vec<PathBuf>,
    pub(crate) agent_inputs: Vec<String>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
    pub(crate) warnings: Vec<RunWarning>,
    worker_owner: Option<PluginWorkerOwner>,
    worker_lease: Option<PluginWorkerLease>,
}

#[derive(Clone, Default)]
pub(crate) struct AcceptedExtensionInputs {
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
    worker_owner: Option<PluginWorkerOwner>,
}

impl ExtensionAssembly {
    pub(crate) fn accepted_inputs(&self) -> AcceptedExtensionInputs {
        AcceptedExtensionInputs {
            mcp_servers: self.mcp_servers.clone(),
            runtime_tools: self.runtime_tools.clone(),
            hook_sources: self.hook_sources.clone(),
            toolsets: self.toolsets.clone(),
            worker_owner: self.worker_owner.clone(),
        }
    }

    pub(crate) async fn shutdown_workers(&mut self) {
        if let Some(lease) = self.worker_lease.take() {
            lease.shutdown().await;
        }
    }
}

impl AcceptedExtensionInputs {
    pub(crate) fn acquire_worker_lease(&self) -> Option<PluginWorkerLease> {
        self.worker_owner.as_ref().map(PluginWorkerOwner::acquire)
    }
}

#[derive(Clone)]
struct PluginWorkerOwner {
    inner: Arc<PluginWorkerOwnerInner>,
}

struct PluginWorkerOwnerInner {
    sessions: Vec<Arc<crate::plugins::PluginWorkerSession>>,
    leases: AtomicUsize,
}

impl PluginWorkerOwner {
    fn new(sessions: Vec<Arc<crate::plugins::PluginWorkerSession>>) -> Option<Self> {
        (!sessions.is_empty()).then(|| Self {
            inner: Arc::new(PluginWorkerOwnerInner {
                sessions,
                leases: AtomicUsize::new(0),
            }),
        })
    }

    fn acquire(&self) -> PluginWorkerLease {
        self.inner.leases.fetch_add(1, Ordering::AcqRel);
        PluginWorkerLease {
            owner: self.clone(),
            active: true,
        }
    }
}

pub(crate) struct PluginWorkerLease {
    owner: PluginWorkerOwner,
    active: bool,
}

impl PluginWorkerLease {
    pub(crate) async fn shutdown(mut self) {
        if self.release() {
            futures::future::join_all(
                self.owner
                    .inner
                    .sessions
                    .iter()
                    .map(|session| session.shutdown()),
            )
            .await;
        }
    }

    fn release(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.owner.inner.leases.fetch_sub(1, Ordering::AcqRel) == 1
    }
}

impl Drop for PluginWorkerLease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

/// Resolves the named MCP declarations through the same effective config and
/// extension assembly used by the Native runtime, without starting an MCP
/// client. Agent adapters use this to hand an explicitly selected subset to an
/// external Agent.
pub fn resolve_mcp_server_handoffs<'a>(
    options: &'a RunOptions,
    names: &'a std::collections::BTreeSet<String>,
) -> futures::future::BoxFuture<'a, crate::Result<Vec<ResolvedMcpServerInput>>> {
    Box::pin(async move {
        if names.is_empty() {
            return Ok(Vec::new());
        }

        let cwd = canonical_cwd(&options.cwd)?;
        let loaded = load_run_config(options, &cwd)?;
        let home = resolve_psychevo_home(&loaded.env)?;
        let mut mcp_servers = options.mcp_servers.clone();
        mcp_servers.extend(loaded.config.mcp_servers.clone());
        let mut assembly = assemble_extensions(ExtensionAssemblyInput {
            home: &home,
            cwd: &cwd,
            env: &loaded.env,
            plugin_policy: &loaded.config.plugins,
            selected_capability_roots: &options.selected_capability_roots,
            mcp_servers,
            runtime_tools: Vec::new(),
        })
        .await;
        let result = async {
            let available = &assembly.mcp_servers;
            let mut resolved = Vec::with_capacity(names.len());
            for name in names {
                let mut matches = available.iter().filter(|server| server.name == *name);
            let mut server = matches.next().cloned().ok_or_else(|| {
                crate::Error::Config(format!(
                    "Agent MCP server `{name}` is not declared in the effective configuration"
                ))
            })?;
            if matches.next().is_some() {
                return Err(crate::Error::Config(format!(
                    "Agent MCP server `{name}` has multiple effective declarations"
                )));
            }
            if !server.policy.enabled {
                return Err(crate::Error::Config(format!(
                    "Agent MCP server `{name}` is disabled in the effective configuration"
                )));
            }
            if let McpTransportInput::Stdio {
                command,
                env: server_env,
                ..
            } = &mut server.transport
            {
                let mut executable_env = loaded.env.clone();
                executable_env.extend(server_env.clone());
                let command_text = command.to_string_lossy().into_owned();
                let resolved_command = resolve_executable_path(
                    &command_text,
                    &cwd,
                    &ExecutableResolveOptions {
                        platform: HostPlatform::current(),
                        env: &executable_env,
                    },
                )
                .ok_or_else(|| {
                    crate::Error::Config(format!(
                        "Agent MCP server `{name}` command `{command_text}` was not found"
                    ))
                })?;
                *command = resolved_command;
            }
            let bearer_token = match &server.transport {
                McpTransportInput::StreamableHttp {
                    url,
                    bearer_token_env_var,
                    ..
                } => bearer_token_env_var
                    .as_ref()
                    .and_then(|env_var| loaded.env.get(env_var))
                    .map(String::as_str)
                    .map(str::trim)
                    .filter(|token| !token.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        load_mcp_oauth_access_token(&home, &server.name, url)
                            .ok()
                            .flatten()
                    }),
                _ => None,
            };
                resolved.push(ResolvedMcpServerInput {
                    server,
                    bearer_token,
                });
            }
            Ok(resolved)
        }
        .await;
        assembly.shutdown_workers().await;
        result
    })
}

pub(crate) async fn assemble_extensions(input: ExtensionAssemblyInput<'_>) -> ExtensionAssembly {
    let plugin_assembly =
        load_enabled_plugin_contributions(input.home, input.cwd, input.env, input.plugin_policy)
            .await;
    let worker_owner = PluginWorkerOwner::new(plugin_assembly.worker_sessions.clone());
    let worker_lease = worker_owner.as_ref().map(PluginWorkerOwner::acquire);
    let selected_root_contributions =
        selected_root_contributions(input.cwd, input.selected_capability_roots);

    let mut mcp_servers = input.mcp_servers;
    mcp_servers.extend(selected_root_contributions.mcp_servers.iter().cloned());
    mcp_servers.extend(plugin_assembly.mcp_servers.iter().cloned());
    let mut runtime_tools = input.runtime_tools;
    runtime_tools.extend(plugin_assembly.runtime_tools.iter().cloned());

    let mut warnings = plugin_assembly.warnings.clone();
    warnings.extend(selected_root_contributions.warnings.clone());

    ExtensionAssembly {
        mcp_servers,
        runtime_tools,
        skill_inputs: plugin_assembly
            .skill_inputs
            .iter()
            .cloned()
            .chain(selected_root_contributions.skill_inputs)
            .collect(),
        agent_inputs: plugin_assembly
            .agent_inputs
            .iter()
            .cloned()
            .chain(selected_root_contributions.agent_inputs)
            .collect(),
        hook_sources: plugin_assembly
            .hook_sources
            .iter()
            .cloned()
            .chain(selected_root_contributions.hook_sources)
            .collect(),
        toolsets: plugin_assembly
            .toolsets
            .iter()
            .cloned()
            .chain(selected_root_contributions.toolsets)
            .collect(),
        warnings,
        worker_owner,
        worker_lease,
    }
}

#[derive(Default)]
pub(crate) struct SelectedRootContributions {
    pub(crate) skill_inputs: Vec<PathBuf>,
    pub(crate) agent_inputs: Vec<String>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
    pub(crate) warnings: Vec<RunWarning>,
}

pub(crate) fn selected_root_contributions(
    cwd: &Path,
    roots: &[SelectedCapabilityRoot],
) -> SelectedRootContributions {
    let mut out = SelectedRootContributions::default();
    for root in roots {
        let root_path = if root.path.is_absolute() {
            root.path.clone()
        } else {
            cwd.join(&root.path)
        };
        let has_manifest = has_recognized_manifest(&root_path);
        match load_plugin_manifest(&root_path, true) {
            Ok(manifest) => {
                out.skill_inputs
                    .extend(manifest.skill_roots.iter().cloned());
                out.agent_inputs
                    .extend(agent_files_from_roots(&manifest.agent_roots));
                if let Some(hooks) = manifest.hooks.clone() {
                    out.hook_sources.push(HookSourceDescriptor {
                        source_id: format!("capability-root:{}", root.id),
                        source_kind: "capability_root".to_string(),
                        display_name: Some(manifest.name.clone()),
                        path: Some(manifest.manifest_path.clone()),
                        hooks,
                        worker: None,
                    });
                }
                let source_id = format!("capability-root:{}", root.id);
                for server in &manifest.mcp_servers {
                    out.mcp_servers.push(
                        McpServerInput::with_source(
                            server.name.clone(),
                            server.transport.clone(),
                            source_id.clone(),
                            "selected_capability_root",
                        )
                        .with_policy(server.policy.clone()),
                    );
                }
                for (name, config) in &manifest.toolsets {
                    out.toolsets.push(ToolsetContribution {
                        source_id: source_id.clone(),
                        source_kind: "selected_capability_root".to_string(),
                        name: name.clone(),
                        config: config.clone(),
                    });
                }
                if manifest.worker.is_some() {
                    out.warnings.push(extension_warning(format!(
                        "selected capability root `{}` declares a Psychevo worker; install and enable the plugin package to use worker tools or worker hooks",
                        root.id
                    )));
                }
            }
            Err(err) if has_manifest => out.warnings.push(extension_warning(format!(
                "selected capability root `{}` omitted: {err}",
                root.id
            ))),
            Err(_) if root_path.is_dir() => out.skill_inputs.push(root_path),
            Err(err) => out.warnings.push(extension_warning(format!(
                "selected capability root `{}` omitted: {err}",
                root.id
            ))),
        }
    }
    out
}

fn has_recognized_manifest(root: &Path) -> bool {
    [".codex-plugin/plugin.json", ".claude-plugin/plugin.json"]
        .iter()
        .any(|path| root.join(path).is_file())
}

fn agent_files_from_roots(roots: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for root in roots {
        collect_agent_files(root, &mut out);
    }
    out
}

fn collect_agent_files(path: &Path, out: &mut Vec<String>) {
    if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
        out.push(path.display().to_string());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_agent_files(&entry.path(), out);
    }
}

fn extension_warning(message: String) -> RunWarning {
    RunWarning {
        kind: "extension_assembly".to_string(),
        message,
        source_path: None,
        suggestion: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    #[tokio::test]
    async fn mcp_handoff_resolves_named_secret_without_debug_leak_and_rejects_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        let mut options = crate::tests::base_options(&temp).await;
        fs::create_dir_all(&options.cwd).expect("workspace");
        fs::write(
            crate::tests::home_dir(&temp).join("config.toml"),
            "# initialized\n",
        )
        .expect("initialized config");
        options
            .inherited_env
            .as_mut()
            .expect("isolated env")
            .insert(
                "REPO_MCP_TOKEN".to_string(),
                "bearer-test-secret".to_string(),
            );
        let server = McpServerInput::new(
            "repo",
            McpTransportInput::StreamableHttp {
                url: "https://example.test/mcp".to_string(),
                headers: BTreeMap::from([(
                    "X-Test-Secret".to_string(),
                    "header-test-secret".to_string(),
                )]),
                bearer_token_env_var: Some("REPO_MCP_TOKEN".to_string()),
                scopes: Vec::new(),
                oauth_resource: None,
                oauth_client_id: None,
            },
        );
        options.mcp_servers.push(server.clone());
        let names = std::collections::BTreeSet::from(["repo".to_string()]);

        let resolved = resolve_mcp_server_handoffs(&options, &names)
            .await
            .expect("handoff");

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].bearer_token.as_deref(),
            Some("bearer-test-secret")
        );
        let debug = format!("{:?}", resolved[0]);
        assert!(!debug.contains("bearer-test-secret"));
        assert!(!debug.contains("header-test-secret"));

        options.mcp_servers.push(server);
        let error = resolve_mcp_server_handoffs(&options, &names)
            .await
            .expect_err("ambiguous declaration must fail closed");
        assert!(
            error
                .to_string()
                .contains("multiple effective declarations")
        );
    }

    #[test]
    fn selected_root_manifest_contributes_declarative_resources_only() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("plugin");
        fs::create_dir_all(root.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(root.join("skills/cleanup")).expect("skill dir");
        fs::create_dir_all(root.join("agents")).expect("agent dir");
        fs::write(
            root.join("agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Review work.\n---\n",
        )
        .expect("agent");
        fs::write(
            root.join(".codex-plugin/plugin.json"),
            r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "skills": ["./skills"],
              "mcpServers": {
                "repo": { "command": "./mcp-server" }
              },
              "hooks": {
                "PostToolUse": [{"hooks": [{"type": "command", "command": "echo ok"}]}]
              }
            }"#,
        )
        .expect("manifest");
        fs::write(
            root.join("psychevo.plugin.json"),
            r#"{
              "agents": ["./agents"],
              "toolsets": {
                "repo-tools": { "tools": ["mcp__repo__search"] }
              },
              "runtime": {"worker": {"command": "./worker.py"}}
            }"#,
        )
        .expect("overlay");

        let contributions = selected_root_contributions(
            temp.path(),
            &[SelectedCapabilityRoot::local("cleanup", "plugin")],
        );

        assert_eq!(
            contributions.skill_inputs,
            vec![root.join("skills").canonicalize().expect("skills")]
        );
        assert_eq!(
            contributions.agent_inputs,
            vec![root.join("agents/reviewer.md").display().to_string()]
        );
        assert_eq!(contributions.hook_sources.len(), 1);
        assert!(contributions.hook_sources[0].worker.is_none());
        assert_eq!(contributions.mcp_servers.len(), 1);
        assert_eq!(
            contributions.mcp_servers[0].source_kind.as_deref(),
            Some("selected_capability_root")
        );
        assert_eq!(contributions.toolsets.len(), 1);
        assert_eq!(contributions.toolsets[0].name, "repo-tools");
        assert_eq!(contributions.warnings.len(), 1);
    }

    #[tokio::test]
    async fn assembly_freezes_static_inputs_and_selected_root_outputs() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let root = temp.path().join("plugin");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(root.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(root.join("skills/cleanup")).expect("skill dir");
        fs::write(
            root.join(".codex-plugin/plugin.json"),
            r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "skills": ["./skills"],
              "hooks": {
                "SessionStart": [{"hooks": [{"type": "prompt", "prompt": "context"}]}]
              }
            }"#,
        )
        .expect("manifest");

        let assembly = assemble_extensions(ExtensionAssemblyInput {
            home: &home,
            cwd: temp.path(),
            env: &BTreeMap::new(),
            plugin_policy: &PluginPolicyConfig::default(),
            selected_capability_roots: &[SelectedCapabilityRoot::local("cleanup", "plugin")],
            mcp_servers: vec![McpServerInput::new(
                "static",
                crate::types::McpTransportInput::Unsupported {
                    kind: "test".to_string(),
                },
            )],
            runtime_tools: Vec::new(),
        })
        .await;
        let accepted = assembly.accepted_inputs();

        assert_eq!(
            assembly.skill_inputs,
            vec![root.join("skills").canonicalize().expect("skills")]
        );
        assert_eq!(
            assembly
                .mcp_servers
                .into_iter()
                .map(|server| server.name)
                .collect::<Vec<_>>(),
            vec!["static".to_string()]
        );
        assert_eq!(accepted.mcp_servers.len(), 1);
        assert_eq!(accepted.hook_sources.len(), 1);
        assert_eq!(accepted.hook_sources[0].source_kind, "capability_root");
        assert!(accepted.runtime_tools.is_empty());
        assert!(accepted.toolsets.is_empty());
    }

    #[test]
    fn selected_root_directory_without_manifest_is_skill_root() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("skills");
        fs::create_dir_all(&root).expect("skill root");

        let contributions = selected_root_contributions(
            temp.path(),
            &[SelectedCapabilityRoot::local("skills", "skills")],
        );

        assert_eq!(contributions.skill_inputs, vec![root]);
        assert!(contributions.agent_inputs.is_empty());
        assert!(contributions.hook_sources.is_empty());
        assert!(contributions.warnings.is_empty());
    }

    #[test]
    fn selected_root_with_malformed_manifest_is_omitted() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("plugin");
        fs::create_dir_all(root.join(".codex-plugin")).expect("codex manifest dir");
        fs::create_dir_all(root.join(".claude-plugin")).expect("claude manifest dir");
        fs::create_dir_all(root.join("skills")).expect("skill dir");
        fs::write(root.join(".codex-plugin/plugin.json"), "{").expect("codex manifest");
        fs::write(
            root.join(".claude-plugin/plugin.json"),
            r#"{"name":"fallback","version":"1.0.0","description":"fallback","skills":["./skills"]}"#,
        )
        .expect("claude manifest");

        let contributions = selected_root_contributions(
            temp.path(),
            &[SelectedCapabilityRoot::local("broken", "plugin")],
        );

        assert!(contributions.skill_inputs.is_empty());
        assert!(contributions.agent_inputs.is_empty());
        assert!(contributions.hook_sources.is_empty());
        assert_eq!(contributions.warnings.len(), 1);
        assert!(
            contributions.warnings[0]
                .message
                .contains(".codex-plugin/plugin.json")
        );
    }
}
