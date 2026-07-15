use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::config::{
    PluginPolicyConfig, ToolsetContribution, load_mcp_oauth_access_token, load_run_config,
    resolve_psychevo_home,
};
use crate::contribution_projection::ContributionProjection;
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
    pub location: CapabilityRootLocation,
}

impl SelectedCapabilityRoot {
    pub fn local(id: impl Into<String>, path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            id: id.into(),
            authority: CapabilityRootAuthority::Local,
            location: CapabilityRootLocation::Local { path: path.into() },
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
            location: CapabilityRootLocation::Local { path: path.into() },
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRootLocation {
    Local { path: std::path::PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionDataScope {
    Session,
    Thread,
    Turn,
}

#[derive(Default, Clone)]
pub struct ExtensionDataInit {
    values: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl ExtensionDataInit {
    pub fn insert<T>(&mut self, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.values.insert(TypeId::of::<T>(), Arc::new(value));
    }
}

pub struct ExtensionData {
    scope: ExtensionDataScope,
    values: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl ExtensionData {
    pub fn new(scope: ExtensionDataScope) -> Self {
        Self {
            scope,
            values: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_init(scope: ExtensionDataScope, init: ExtensionDataInit) -> Self {
        Self {
            scope,
            values: Mutex::new(init.values),
        }
    }

    pub fn scope(&self) -> ExtensionDataScope {
        self.scope
    }

    pub fn insert<T>(&self, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.values
            .lock()
            .expect("extension data mutex poisoned")
            .insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub fn get<T>(&self) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .lock()
            .expect("extension data mutex poisoned")
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|value| value.downcast::<T>().ok())
    }
}

pub trait McpServerContributor: Send + Sync {
    fn id(&self) -> &str;
    fn servers(&self) -> Vec<McpServerInput>;
}

pub trait ToolContributor: Send + Sync {
    fn id(&self) -> &str;
    fn tools(&self) -> Vec<RuntimeTool>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionRegistryDiagnostic {
    pub kind: String,
    pub contributor_id: String,
    pub message: String,
}

#[derive(Default)]
pub struct ExtensionRegistryBuilder {
    mcp_server_contributors: Vec<Arc<dyn McpServerContributor>>,
    tool_contributors: Vec<Arc<dyn ToolContributor>>,
    diagnostics: Vec<ExtensionRegistryDiagnostic>,
}

impl ExtensionRegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mcp_server_contributor(&mut self, contributor: Arc<dyn McpServerContributor>) {
        replace_contributor(
            &mut self.mcp_server_contributors,
            contributor,
            &mut self.diagnostics,
            "mcp_server",
        );
    }

    pub fn tool_contributor(&mut self, contributor: Arc<dyn ToolContributor>) {
        replace_contributor(
            &mut self.tool_contributors,
            contributor,
            &mut self.diagnostics,
            "tool",
        );
    }

    pub fn build(self) -> ExtensionRegistry {
        ExtensionRegistry {
            mcp_server_contributors: self.mcp_server_contributors,
            tool_contributors: self.tool_contributors,
            diagnostics: self.diagnostics,
        }
    }
}

#[derive(Clone, Default)]
pub struct ExtensionRegistry {
    mcp_server_contributors: Vec<Arc<dyn McpServerContributor>>,
    tool_contributors: Vec<Arc<dyn ToolContributor>>,
    diagnostics: Vec<ExtensionRegistryDiagnostic>,
}

impl ExtensionRegistry {
    pub fn mcp_server_contributors(&self) -> &[Arc<dyn McpServerContributor>] {
        &self.mcp_server_contributors
    }

    pub fn tool_contributors(&self) -> &[Arc<dyn ToolContributor>] {
        &self.tool_contributors
    }

    pub fn diagnostics(&self) -> &[ExtensionRegistryDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn mcp_servers(&self) -> Vec<McpServerInput> {
        self.mcp_server_contributors
            .iter()
            .flat_map(|contributor| contributor.servers())
            .collect()
    }

    pub(crate) fn runtime_tools(&self) -> Vec<RuntimeTool> {
        self.tool_contributors
            .iter()
            .flat_map(|contributor| contributor.tools())
            .collect()
    }
}

trait IdentifiedContributor {
    fn contributor_id(&self) -> &str;
}

impl IdentifiedContributor for dyn McpServerContributor {
    fn contributor_id(&self) -> &str {
        self.id()
    }
}

impl IdentifiedContributor for dyn ToolContributor {
    fn contributor_id(&self) -> &str {
        self.id()
    }
}

fn replace_contributor<T: ?Sized + IdentifiedContributor>(
    contributors: &mut Vec<Arc<T>>,
    contributor: Arc<T>,
    diagnostics: &mut Vec<ExtensionRegistryDiagnostic>,
    kind: &str,
) {
    if let Some(index) = contributors
        .iter()
        .position(|existing| existing.contributor_id() == contributor.contributor_id())
    {
        let id = contributor.contributor_id().to_string();
        contributors[index] = contributor;
        diagnostics.push(ExtensionRegistryDiagnostic {
            kind: "replacement".to_string(),
            contributor_id: id.clone(),
            message: format!("later {kind} contributor `{id}` replaced the earlier registration"),
        });
    } else {
        contributors.push(contributor);
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
    pub(crate) registry: ExtensionRegistry,
    pub(crate) skill_inputs: Vec<PathBuf>,
    pub(crate) agent_inputs: Vec<String>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
    pub(crate) warnings: Vec<RunWarning>,
    pub(crate) projection: ContributionProjection,
}

#[derive(Clone, Default)]
pub(crate) struct AcceptedExtensionInputs {
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) runtime_tools: Vec<RuntimeTool>,
    pub(crate) hook_sources: Vec<HookSourceDescriptor>,
    pub(crate) toolsets: Vec<ToolsetContribution>,
}

impl ExtensionAssembly {
    pub(crate) fn accepted_inputs(&self) -> AcceptedExtensionInputs {
        AcceptedExtensionInputs {
            mcp_servers: self.registry.mcp_servers(),
            runtime_tools: self.registry.runtime_tools(),
            hook_sources: self.hook_sources.clone(),
            toolsets: self.toolsets.clone(),
        }
    }
}

/// Resolves the named MCP declarations through the same effective config and
/// extension assembly used by the Native runtime, without starting an MCP
/// client. Agent adapters use this to hand an explicitly selected subset to an
/// external Agent.
pub fn resolve_mcp_server_handoffs(
    options: &RunOptions,
    names: &std::collections::BTreeSet<String>,
) -> crate::Result<Vec<ResolvedMcpServerInput>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let home = resolve_psychevo_home(&loaded.env)?;
    let mut mcp_servers = options.mcp_servers.clone();
    mcp_servers.extend(loaded.config.mcp_servers.clone());
    let assembly = assemble_extensions(ExtensionAssemblyInput {
        home: &home,
        cwd: &cwd,
        env: &loaded.env,
        plugin_policy: &loaded.config.plugins,
        selected_capability_roots: &options.selected_capability_roots,
        mcp_servers,
        runtime_tools: Vec::new(),
    });
    let available = assembly.registry.mcp_servers();
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

pub(crate) fn assemble_extensions(input: ExtensionAssemblyInput<'_>) -> ExtensionAssembly {
    let plugin_assembly =
        load_enabled_plugin_contributions(input.home, input.cwd, input.env, input.plugin_policy);
    let selected_root_contributions =
        selected_root_contributions(input.cwd, input.selected_capability_roots);

    let mut mcp_servers = input.mcp_servers;
    mcp_servers.extend(selected_root_contributions.mcp_servers.iter().cloned());
    mcp_servers.extend(plugin_assembly.mcp_servers.iter().cloned());
    let mut runtime_tools = input.runtime_tools;
    runtime_tools.extend(plugin_assembly.runtime_tools.iter().cloned());

    let registry = registry_from_static_inputs(mcp_servers, runtime_tools);
    let mut warnings = plugin_assembly.warnings.clone();
    warnings.extend(selected_root_contributions.warnings.clone());

    ExtensionAssembly {
        registry,
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
        projection: plugin_assembly.projection.clone(),
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
        let CapabilityRootLocation::Local { path } = &root.location;
        let root_path = if path.is_absolute() {
            path.clone()
        } else {
            cwd.join(path)
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

pub(crate) fn registry_from_static_inputs(
    mcp_servers: Vec<McpServerInput>,
    runtime_tools: Vec<RuntimeTool>,
) -> ExtensionRegistry {
    let mut builder = ExtensionRegistryBuilder::new();
    if !mcp_servers.is_empty() {
        builder.mcp_server_contributor(Arc::new(StaticMcpServerContributor { mcp_servers }));
    }
    if !runtime_tools.is_empty() {
        builder.tool_contributor(Arc::new(StaticToolContributor { runtime_tools }));
    }
    builder.build()
}

struct StaticMcpServerContributor {
    mcp_servers: Vec<McpServerInput>,
}

impl McpServerContributor for StaticMcpServerContributor {
    fn id(&self) -> &str {
        "static-mcp"
    }

    fn servers(&self) -> Vec<McpServerInput> {
        self.mcp_servers.clone()
    }
}

struct StaticToolContributor {
    runtime_tools: Vec<RuntimeTool>,
}

impl ToolContributor for StaticToolContributor {
    fn id(&self) -> &str {
        "static-tools"
    }

    fn tools(&self) -> Vec<RuntimeTool> {
        self.runtime_tools.clone()
    }
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
        kind: "extension_registry".to_string(),
        message,
        source_path: None,
        suggestion: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    #[derive(Debug, PartialEq, Eq)]
    struct Marker(&'static str);

    struct TestMcpContributor {
        id: &'static str,
        server: &'static str,
    }

    impl McpServerContributor for TestMcpContributor {
        fn id(&self) -> &str {
            self.id
        }

        fn servers(&self) -> Vec<McpServerInput> {
            vec![McpServerInput::new(
                self.server,
                crate::types::McpTransportInput::Unsupported {
                    kind: "test".to_string(),
                },
            )]
        }
    }

    #[test]
    fn extension_data_stores_values_by_type() {
        let mut init = ExtensionDataInit::default();
        init.insert(Marker("thread"));
        let data = ExtensionData::from_init(ExtensionDataScope::Thread, init);

        assert_eq!(data.scope(), ExtensionDataScope::Thread);
        assert_eq!(data.get::<Marker>().as_deref(), Some(&Marker("thread")));
        assert!(data.get::<String>().is_none());
    }

    #[test]
    fn registry_preserves_contributor_order() {
        let mut builder = ExtensionRegistryBuilder::new();
        builder.mcp_server_contributor(Arc::new(TestMcpContributor {
            id: "a",
            server: "a",
        }));
        builder.mcp_server_contributor(Arc::new(TestMcpContributor {
            id: "b",
            server: "b",
        }));

        let names = builder
            .build()
            .mcp_servers()
            .into_iter()
            .map(|server| server.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn registry_replaces_duplicate_contributor_id_in_place_with_diagnostic() {
        let mut builder = ExtensionRegistryBuilder::new();
        builder.mcp_server_contributor(Arc::new(TestMcpContributor {
            id: "package:review",
            server: "old",
        }));
        builder.mcp_server_contributor(Arc::new(TestMcpContributor {
            id: "package:review",
            server: "new",
        }));

        let registry = builder.build();

        assert_eq!(registry.mcp_servers()[0].name, "new");
        assert_eq!(registry.mcp_server_contributors().len(), 1);
        assert_eq!(
            registry.diagnostics(),
            &[ExtensionRegistryDiagnostic {
                kind: "replacement".to_string(),
                contributor_id: "package:review".to_string(),
                message: "later mcp_server contributor `package:review` replaced the earlier registration"
                    .to_string(),
            }]
        );
    }

    #[test]
    fn mcp_handoff_resolves_named_secret_without_debug_leak_and_rejects_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        let mut options = crate::tests::base_options(&temp);
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

        let resolved = resolve_mcp_server_handoffs(&options, &names).expect("handoff");

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

    #[test]
    fn assembly_freezes_static_inputs_and_selected_root_outputs() {
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
        });
        let accepted = assembly.accepted_inputs();

        assert_eq!(
            assembly.skill_inputs,
            vec![root.join("skills").canonicalize().expect("skills")]
        );
        assert_eq!(
            assembly
                .registry
                .mcp_servers()
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
