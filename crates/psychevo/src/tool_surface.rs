use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    ToolBinding, ToolDisplaySpec, ToolExecutionMode, ToolExposure, ToolOutput, ToolRouter,
};
use psychevo_ai::{AbortSignal, ToolDeclaration};
use serde_json::Value;

use crate::agents::{AgentToolContext, agent_tools};
use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig, ToolsetContribution};
use crate::sandbox::{SandboxPolicy, SandboxWriteGrants};
use crate::skills::SkillRuntime;
use crate::tools::{
    ToolRuntimeContext, builtin_tool_aliases, builtin_toolset_description, builtin_toolset_names,
    builtin_toolset_tools, clarify_tool, default_enabled_toolsets, known_tool_name,
    skill_tools_for_mode_with_runtime, tool_allowed_in_mode, tool_by_name, tool_names_for_mode,
};
use crate::types::{
    ClarifyControl, RunMode, RunStreamSink, RunWarning, RuntimeTool, WorkspaceMutationSink,
};

pub(crate) enum ClarifyToolSurface {
    Disabled,
    Enabled {
        control: Option<Arc<ClarifyControl>>,
        stream: Option<RunStreamSink>,
    },
}

impl ClarifyToolSurface {
    pub(crate) fn enabled(
        control: Option<Arc<ClarifyControl>>,
        stream: Option<RunStreamSink>,
    ) -> Self {
        Self::Enabled { control, stream }
    }

    pub(crate) fn declaration_only() -> Self {
        Self::Enabled {
            control: None,
            stream: None,
        }
    }
}

pub(crate) struct ToolSurfaceAssembly {
    pub(crate) cwd: PathBuf,
    pub(crate) task_id: String,
    pub(crate) mode: RunMode,
    pub(crate) lsp: LspConfig,
    pub(crate) allow_login_shell: bool,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) workspace_mutations: Option<WorkspaceMutationSink>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) sandbox_policy: SandboxPolicy,
    pub(crate) sandbox_grants: SandboxWriteGrants,
    pub(crate) home: Option<PathBuf>,
    pub(crate) image_input_enabled: bool,
    pub(crate) image_generation: Option<crate::config::ResolvedImageGenerationConfig>,
    pub(crate) web_search: crate::config::WebSearchConfig,
    pub(crate) selection: ToolSelectionIntent,
    pub(crate) clarify: ClarifyToolSurface,
    pub(crate) skills: Option<SkillRuntime>,
    pub(crate) extension_tools: Vec<RuntimeTool>,
    pub(crate) agents: Option<AgentToolContext>,
}

pub(crate) struct ToolSelectionPlan {
    pub(crate) tools: Vec<Arc<dyn ToolBinding>>,
    pub(crate) warnings: Vec<RunWarning>,
    pub(crate) accepted_toolset_names: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct ToolSelectionIntent {
    definitions: BTreeMap<String, ToolsetDefinition>,
    selected_toolsets: Vec<String>,
    disabled_toolsets: BTreeSet<String>,
    pub(crate) tool_search_enabled: bool,
}

impl ToolSelectionIntent {
    pub(crate) fn selects_tool(&self, name: &str, mode: RunMode) -> bool {
        let mut visiting = BTreeSet::new();
        self.selected_toolsets
            .iter()
            .any(|toolset| self.toolset_selects_tool(toolset, name, mode, &mut visiting))
    }

    fn toolset_selects_tool(
        &self,
        toolset: &str,
        name: &str,
        mode: RunMode,
        visiting: &mut BTreeSet<String>,
    ) -> bool {
        if self.disabled_toolsets.contains(toolset) || !visiting.insert(toolset.to_string()) {
            return false;
        }
        let selected = self.definitions.get(toolset).is_some_and(|definition| {
            (definition.config.tools.iter().any(|tool| tool == name)
                && (!known_tool_name(name) || tool_allowed_in_mode(name, mode)))
                || definition
                    .config
                    .includes
                    .iter()
                    .any(|include| self.toolset_selects_tool(include, name, mode, visiting))
        });
        visiting.remove(toolset);
        selected
    }
}

pub(crate) fn compile_tool_selection(
    mode: RunMode,
    selection: &ToolSelectionConfig,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
    contributed_toolsets: &[ToolsetContribution],
) -> ToolSelectionIntent {
    ToolSelectionIntent {
        definitions: build_toolset_definitions(custom_toolsets, contributed_toolsets),
        selected_toolsets: selected_toolset_names(mode, selection),
        disabled_toolsets: disabled_toolset_names(mode, selection),
        tool_search_enabled: selection.tool_search.enabled,
    }
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    assemble_tool_surface_with_warnings(input).tools
}

pub(crate) fn assemble_tool_surface_with_warnings(input: ToolSurfaceAssembly) -> ToolSelectionPlan {
    let mut warnings = Vec::new();
    let mut available_tools = ToolRegistry::default();
    let runtime_context = ToolRuntimeContext {
        task_id: input.task_id,
        file_reads: crate::tools::FileReadTracker::default(),
        lsp: input.lsp.clone(),
        lsp_manager: crate::tools::write_support::default_lsp_manager(),
        allow_login_shell: input.allow_login_shell,
        stream_events: input.stream_events.clone(),
        workspace_mutations: input.workspace_mutations.clone(),
        env: input.env.clone(),
        path_prefixes: input.path_prefixes.clone(),
        sandbox_policy: input.sandbox_policy.clone(),
        sandbox_grants: input.sandbox_grants.clone(),
        home: input.home.clone(),
        image_input_enabled: input.image_input_enabled,
        image_generation: input.image_generation.clone(),
        web_search: input.web_search.clone(),
    };

    for name in tool_names_for_mode(input.mode) {
        if let Some(binding) = tool_by_name(name, &input.cwd, runtime_context.clone()) {
            let entry = AvailableToolEntry::new(binding, format!("builtin:tool:{name}"), "builtin");
            available_tools.register(entry.clone());
            for alias in builtin_tool_aliases(name) {
                available_tools.register(entry.clone().with_name(*alias));
            }
        }
    }
    let mut fallback_entries = Vec::new();
    let mut selected_tools = Vec::new();
    let mut selected_tool_sources = BTreeMap::new();

    if let ClarifyToolSurface::Enabled { control, stream } = input.clarify {
        let entry = AvailableToolEntry::new(
            clarify_tool(control, stream),
            "builtin:tool:clarify",
            "builtin",
        );
        available_tools.register(entry.clone());
        fallback_entries.push(entry);
    }
    if let Some(skill_runtime) = input.skills {
        for tool in skill_tools_for_mode_with_runtime(skill_runtime, input.mode) {
            let entry = AvailableToolEntry::new(tool, "runtime:skills", "runtime");
            available_tools.register(entry.clone());
            fallback_entries.push(entry);
        }
    }
    for tool in input.extension_tools {
        let binding = extension_tool_binding(&tool, input.selection.tool_search_enabled);
        let source_id = tool
            .source_id()
            .map(str::to_string)
            .unwrap_or_else(|| "runtime:extension-tools".to_string());
        let source_kind = tool
            .source_kind()
            .map(str::to_string)
            .unwrap_or_else(|| "runtime".to_string());
        let entry = AvailableToolEntry::new(binding, source_id, source_kind);
        available_tools.register(entry.clone());
        fallback_entries.push(entry);
    }
    if let Some(agent_context) = input.agents {
        for tool in agent_tools(agent_context) {
            let entry = AvailableToolEntry::new(tool, "runtime:agents", "runtime");
            available_tools.register(entry.clone());
            fallback_entries.push(entry);
        }
    }

    let mut toolsets = input.selection.definitions;
    insert_runtime_derived_toolsets(&mut toolsets, &available_tools);

    let selected_toolsets = input.selection.selected_toolsets;
    let disabled_toolsets = input.selection.disabled_toolsets;
    let mut accepted_toolset_names = Vec::new();
    let mut accepted_toolsets = BTreeSet::new();
    let mut expansion = ToolsetExpansion {
        mode: input.mode,
        definitions: &toolsets,
        registry: &available_tools,
        disabled_toolsets: &disabled_toolsets,
        selected_tools: &mut selected_tools,
        selected_tool_sources: &mut selected_tool_sources,
        accepted_toolset_names: &mut accepted_toolset_names,
        accepted_toolsets: &mut accepted_toolsets,
        warnings: &mut warnings,
        visiting: Vec::new(),
    };
    for toolset in selected_toolsets {
        expand_toolset(&toolset, &mut expansion);
    }

    for entry in fallback_entries {
        let accepted = select_tool_entry(
            &entry,
            &mut *expansion.selected_tools,
            &mut *expansion.selected_tool_sources,
            &mut *expansion.warnings,
            true,
        );
        if accepted {
            record_runtime_derived_toolset_acceptance(
                &entry,
                &mut *expansion.accepted_toolset_names,
                &mut *expansion.accepted_toolsets,
            );
        }
    }
    drop(expansion);
    ToolSelectionPlan {
        tools: selected_tools,
        warnings,
        accepted_toolset_names,
    }
}

pub(crate) fn tool_declarations(tools: &[Arc<dyn ToolBinding>]) -> Vec<ToolDeclaration> {
    ToolRouter::from_tools(tools.iter().cloned())
        .expect("assembled tool surface must have unique display and canonical identities")
        .declarations()
}

fn extension_tool_binding(tool: &RuntimeTool, tool_search_enabled: bool) -> Arc<dyn ToolBinding> {
    let binding = tool.binding();
    if should_defer_extension_tool(tool, binding.exposure(), tool_search_enabled) {
        Arc::new(DeferredToolBinding { inner: binding }) as Arc<dyn ToolBinding>
    } else {
        binding
    }
}

fn should_defer_extension_tool(
    tool: &RuntimeTool,
    exposure: ToolExposure,
    tool_search_enabled: bool,
) -> bool {
    tool_search_enabled
        && exposure == ToolExposure::Direct
        && matches!(tool.source_kind(), Some("mcp" | "plugin"))
}

struct DeferredToolBinding {
    inner: Arc<dyn ToolBinding>,
}

impl ToolBinding for DeferredToolBinding {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        self.inner.canonical_tool_name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn search_metadata(&self) -> Vec<String> {
        self.inner.search_metadata()
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.inner.execution_mode()
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        self.inner.display_spec()
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        self.inner.execute(tool_call_id, args, abort)
    }
}

#[derive(Default)]
struct ToolRegistry {
    by_name: BTreeMap<String, Vec<AvailableToolEntry>>,
}

impl ToolRegistry {
    fn register(&mut self, entry: AvailableToolEntry) {
        self.by_name
            .entry(entry.name.clone())
            .or_default()
            .push(entry);
    }

    fn entries(&self, name: &str) -> Option<&[AvailableToolEntry]> {
        self.by_name.get(name).map(Vec::as_slice)
    }

    fn all_entries(&self) -> impl Iterator<Item = &AvailableToolEntry> {
        self.by_name.values().flat_map(|entries| entries.iter())
    }
}

#[derive(Clone)]
struct AvailableToolEntry {
    name: String,
    binding: Arc<dyn ToolBinding>,
    source_id: String,
    source_kind: String,
}

impl AvailableToolEntry {
    fn new(
        binding: Arc<dyn ToolBinding>,
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
    ) -> Self {
        let name = binding.name().to_string();
        Self {
            name,
            binding,
            source_id: source_id.into(),
            source_kind: source_kind.into(),
        }
    }

    fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

#[derive(Clone)]
struct ToolsetDefinition {
    name: String,
    config: CustomToolsetConfig,
}

fn build_toolset_definitions(
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
    contributed_toolsets: &[ToolsetContribution],
) -> BTreeMap<String, ToolsetDefinition> {
    let mut definitions = BTreeMap::new();
    for name in builtin_toolset_names() {
        if let Some(tools) = builtin_toolset_tools(name) {
            definitions.insert(
                name.to_string(),
                ToolsetDefinition {
                    name: name.to_string(),
                    config: CustomToolsetConfig {
                        description: builtin_toolset_description(name).map(str::to_string),
                        tools: tools.iter().map(|tool| (*tool).to_string()).collect(),
                        includes: Vec::new(),
                    },
                },
            );
        }
    }
    for (name, config) in custom_toolsets {
        insert_toolset_definition(
            &mut definitions,
            ToolsetDefinition {
                name: name.clone(),
                config: config.clone(),
            },
        );
    }
    for contribution in contributed_toolsets {
        insert_toolset_definition(
            &mut definitions,
            ToolsetDefinition {
                name: contribution.name.clone(),
                config: contribution.config.clone(),
            },
        );
    }
    definitions
}

fn insert_toolset_definition(
    definitions: &mut BTreeMap<String, ToolsetDefinition>,
    definition: ToolsetDefinition,
) {
    if definitions.contains_key(&definition.name) {
        return;
    }
    definitions.insert(definition.name.clone(), definition);
}

fn insert_runtime_derived_toolsets(
    definitions: &mut BTreeMap<String, ToolsetDefinition>,
    registry: &ToolRegistry,
) {
    let mut tools_by_source = BTreeMap::<String, BTreeSet<String>>::new();
    for entry in registry.all_entries() {
        if entry.source_kind != "mcp" {
            continue;
        }
        let Some(server) = entry.source_id.strip_prefix("mcp:") else {
            continue;
        };
        if server == "utility" || server.is_empty() {
            continue;
        }
        tools_by_source
            .entry(entry.source_id.clone())
            .or_default()
            .insert(entry.name.clone());
    }
    for (source_id, tools) in tools_by_source {
        let Some(server) = source_id.strip_prefix("mcp:") else {
            continue;
        };
        let server = server.to_string();
        insert_toolset_definition(
            definitions,
            ToolsetDefinition {
                name: format!("mcp-{server}"),
                config: CustomToolsetConfig {
                    description: Some(format!("MCP tools from `{server}`")),
                    tools: tools.into_iter().collect(),
                    includes: Vec::new(),
                },
            },
        );
    }
}

fn record_runtime_derived_toolset_acceptance(
    entry: &AvailableToolEntry,
    accepted_toolset_names: &mut Vec<String>,
    accepted_toolsets: &mut BTreeSet<String>,
) {
    if entry.source_kind != "mcp" {
        return;
    }
    let Some(server) = entry.source_id.strip_prefix("mcp:") else {
        return;
    };
    if server == "utility" || server.is_empty() {
        return;
    }
    let toolset_name = format!("mcp-{server}");
    if accepted_toolsets.insert(toolset_name.clone()) {
        accepted_toolset_names.push(toolset_name);
    }
}

fn selected_toolset_names(mode: RunMode, selection: &ToolSelectionConfig) -> Vec<String> {
    selection
        .modes
        .get(mode.as_str())
        .and_then(|config| config.enabled_toolsets.clone())
        .unwrap_or_else(|| {
            default_enabled_toolsets()
                .iter()
                .map(|name| (*name).to_string())
                .collect()
        })
}

fn disabled_toolset_names(mode: RunMode, selection: &ToolSelectionConfig) -> BTreeSet<String> {
    selection
        .modes
        .get(mode.as_str())
        .map(|config| config.disabled_toolsets.iter().cloned().collect())
        .unwrap_or_default()
}

struct ToolsetExpansion<'a> {
    mode: RunMode,
    definitions: &'a BTreeMap<String, ToolsetDefinition>,
    registry: &'a ToolRegistry,
    disabled_toolsets: &'a BTreeSet<String>,
    selected_tools: &'a mut Vec<Arc<dyn ToolBinding>>,
    selected_tool_sources: &'a mut BTreeMap<String, String>,
    accepted_toolset_names: &'a mut Vec<String>,
    accepted_toolsets: &'a mut BTreeSet<String>,
    warnings: &'a mut Vec<RunWarning>,
    visiting: Vec<String>,
}

fn expand_toolset(name: &str, state: &mut ToolsetExpansion<'_>) -> bool {
    if state.disabled_toolsets.contains(name) {
        return false;
    }

    let Some(definition) = state.definitions.get(name).cloned() else {
        return false;
    };

    if state.visiting.iter().any(|visiting| visiting == name) {
        return false;
    }

    state.visiting.push(name.to_string());
    let mut accepted_binding = false;
    for include in &definition.config.includes {
        accepted_binding |= expand_toolset(include, state);
    }
    for tool in &definition.config.tools {
        accepted_binding |= select_named_tool(tool, &definition, state);
    }
    state.visiting.pop();

    if accepted_binding && state.accepted_toolsets.insert(name.to_string()) {
        state.accepted_toolset_names.push(name.to_string());
    }
    accepted_binding
}

fn select_named_tool(
    name: &str,
    _toolset: &ToolsetDefinition,
    state: &mut ToolsetExpansion<'_>,
) -> bool {
    if known_tool_name(name) && !tool_allowed_in_mode(name, state.mode) {
        return false;
    }

    let Some(entries) = state.registry.entries(name) else {
        return false;
    };

    entries.first().is_some_and(|entry| {
        select_tool_entry(
            entry,
            state.selected_tools,
            state.selected_tool_sources,
            state.warnings,
            false,
        )
    })
}

fn select_tool_entry(
    entry: &AvailableToolEntry,
    selected_tools: &mut Vec<Arc<dyn ToolBinding>>,
    selected_tool_sources: &mut BTreeMap<String, String>,
    warnings: &mut Vec<RunWarning>,
    warn_on_conflict: bool,
) -> bool {
    if let Some(existing_source) = selected_tool_sources.get(&entry.name) {
        if existing_source == &entry.source_id {
            return true;
        }
        if warn_on_conflict {
            warnings.push(RunWarning {
                kind: "capability_conflict".to_string(),
                message: format!(
                    "Tool `{}` from `{}` conflicts with an already selected tool; omitted",
                    entry.name, entry.source_id
                ),
                source_path: None,
                suggestion: None,
            });
        }
        return false;
    }
    selected_tool_sources.insert(entry.name.clone(), entry.source_id.clone());
    selected_tools.push(Arc::clone(&entry.binding));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use psychevo_agent_core::{ToolExecutionMode, ToolOutput};
    use psychevo_ai::AbortSignal;
    use serde_json::{Value, json};

    use crate::config::ToolModeConfig;

    struct TestTool {
        name: String,
        exposure: ToolExposure,
    }

    impl TestTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                exposure: ToolExposure::Direct,
            }
        }

        fn with_exposure(name: &str, exposure: ToolExposure) -> Self {
            Self {
                name: name.to_string(),
                exposure,
            }
        }
    }

    impl ToolBinding for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "test tool"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn exposure(&self) -> ToolExposure {
            self.exposure
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

    fn base_input(mode: RunMode) -> ToolSurfaceAssembly {
        ToolSurfaceAssembly {
            cwd: std::env::temp_dir(),
            task_id: "test".to_string(),
            mode,
            lsp: Default::default(),
            allow_login_shell: false,
            stream_events: None,
            workspace_mutations: None,
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: SandboxPolicy::disabled(),
            sandbox_grants: SandboxWriteGrants::default(),
            home: None,
            image_input_enabled: true,
            image_generation: None,
            web_search: Default::default(),
            selection: compile_tool_selection(
                mode,
                &ToolSelectionConfig::default(),
                &BTreeMap::new(),
                &[],
            ),
            clarify: ClarifyToolSurface::Disabled,
            skills: None,
            extension_tools: Vec::new(),
            agents: None,
        }
    }

    fn selected_names(plan: &ToolSelectionPlan) -> Vec<&str> {
        plan.tools.iter().map(|tool| tool.name()).collect()
    }

    #[test]
    fn contributed_toolset_selects_hosted_web_search_in_the_canonical_intent() {
        let mut selection = ToolSelectionConfig::default();
        selection.modes.insert(
            "default".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec!["plugin-web".to_string()]),
                disabled_toolsets: Vec::new(),
            },
        );
        let contributed = vec![ToolsetContribution {
            source_id: "plugin:search@local".to_string(),
            source_kind: "plugin".to_string(),
            name: "plugin-web".to_string(),
            config: CustomToolsetConfig {
                description: None,
                tools: vec!["web_search".to_string()],
                includes: Vec::new(),
            },
        }];

        let intent =
            compile_tool_selection(RunMode::Default, &selection, &BTreeMap::new(), &contributed);

        assert!(intent.selects_tool("web_search", RunMode::Default));
    }

    #[test]
    fn builtin_alias_selects_the_canonical_binding_from_the_same_registry() {
        let mut input = base_input(RunMode::Default);
        let mut selection = ToolSelectionConfig::default();
        selection.modes.insert(
            "default".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec!["legacy-image-name".to_string()]),
                disabled_toolsets: Vec::new(),
            },
        );
        let custom = BTreeMap::from([(
            "legacy-image-name".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: vec!["image_generation.generate".to_string()],
                includes: Vec::new(),
            },
        )]);
        input.selection = compile_tool_selection(RunMode::Default, &selection, &custom, &[]);

        let result = assemble_tool_surface_with_warnings(input);

        assert_eq!(selected_names(&result), vec!["image_generate"]);
    }

    #[test]
    fn plugin_toolset_accepts_only_registered_tool_bindings() {
        let mut input = base_input(RunMode::Default);
        let mut selection = ToolSelectionConfig::default();
        selection.modes.insert(
            "default".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec!["plugin-pack".to_string()]),
                disabled_toolsets: Vec::new(),
            },
        );
        let contributed = vec![ToolsetContribution {
            source_id: "plugin:demo@local".to_string(),
            source_kind: "plugin".to_string(),
            name: "plugin-pack".to_string(),
            config: CustomToolsetConfig {
                description: Some("plugin tools".to_string()),
                tools: vec!["plugin_do".to_string(), "missing_plugin_tool".to_string()],
                includes: vec!["coding-core".to_string()],
            },
        }];
        input.selection =
            compile_tool_selection(RunMode::Default, &selection, &BTreeMap::new(), &contributed);
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("plugin_do")),
            "plugin:demo@local",
            "plugin",
        ));

        let result = assemble_tool_surface_with_warnings(input);

        assert!(
            result
                .accepted_toolset_names
                .contains(&"plugin-pack".to_string())
        );
        assert!(selected_names(&result).contains(&"plugin_do"));
        assert_eq!(
            selected_names(&result)
                .iter()
                .filter(|name| **name == "plugin_do")
                .count(),
            1
        );
        assert!(!selected_names(&result).contains(&"missing_plugin_tool"));
    }

    #[test]
    fn default_tool_search_defers_plugin_tools_until_search_activation() {
        let mut input = base_input(RunMode::Default);
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("plugin_lookup")),
            "plugin:demo@local",
            "plugin",
        ));
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("mcp_lookup")),
            "mcp:demo",
            "mcp",
        ));

        let result = assemble_tool_surface_with_warnings(input);
        let declarations = ToolRouter::from_tools(result.tools)
            .expect("unique tools")
            .with_tool_search(psychevo_agent_core::ToolSearchOptions::enabled())
            .declarations();
        let names = declarations
            .into_iter()
            .map(|declaration| declaration.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"tool_search".to_string()));
        assert!(!names.contains(&"plugin_lookup".to_string()));
        assert!(!names.contains(&"mcp_lookup".to_string()));
    }

    #[test]
    fn mcp_runtime_tools_derive_source_toolset_metadata() {
        let mut input = base_input(RunMode::Default);
        let mut selection = ToolSelectionConfig::default();
        selection.modes.insert(
            "default".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec!["mcp-repo".to_string()]),
                disabled_toolsets: Vec::new(),
            },
        );
        input.selection =
            compile_tool_selection(RunMode::Default, &selection, &BTreeMap::new(), &[]);
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("mcp__repo__search")),
            "mcp:repo",
            "mcp",
        ));

        let result = assemble_tool_surface_with_warnings(input);

        assert!(
            result
                .accepted_toolset_names
                .contains(&"mcp-repo".to_string())
        );
        assert!(selected_names(&result).contains(&"mcp__repo__search"));
    }

    #[test]
    fn host_owned_runtime_tools_stay_direct_with_default_tool_search() {
        let mut input = base_input(RunMode::Default);
        input
            .extension_tools
            .push(RuntimeTool::new(Arc::new(TestTool::new("automation"))));

        let result = assemble_tool_surface_with_warnings(input);
        let declarations = ToolRouter::from_tools(result.tools)
            .expect("unique tools")
            .with_tool_search(psychevo_agent_core::ToolSearchOptions::enabled())
            .declarations();
        let names = declarations
            .into_iter()
            .map(|declaration| declaration.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"automation".to_string()));
        assert!(!names.contains(&"tool_search".to_string()));
    }

    #[test]
    fn explicit_tool_search_disable_keeps_plugin_tools_direct() {
        let mut input = base_input(RunMode::Default);
        input.selection.tool_search_enabled = false;
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("plugin_lookup")),
            "plugin:demo@local",
            "plugin",
        ));

        let result = assemble_tool_surface_with_warnings(input);
        let declarations = ToolRouter::from_tools(result.tools)
            .expect("unique tools")
            .with_tool_search(psychevo_agent_core::ToolSearchOptions::disabled())
            .declarations();
        let names = declarations
            .into_iter()
            .map(|declaration| declaration.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"plugin_lookup".to_string()));
        assert!(!names.contains(&"tool_search".to_string()));
    }

    #[test]
    fn extension_tool_policy_preserves_explicit_deferred_and_hidden_exposure() {
        let mut input = base_input(RunMode::Default);
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::with_exposure(
                "plugin_deferred",
                ToolExposure::Deferred,
            )),
            "plugin:demo@local",
            "plugin",
        ));
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::with_exposure(
                "plugin_hidden",
                ToolExposure::Hidden,
            )),
            "plugin:demo@local",
            "plugin",
        ));

        let result = assemble_tool_surface_with_warnings(input);
        let router = ToolRouter::from_tools(result.tools)
            .expect("unique tools")
            .with_tool_search(psychevo_agent_core::ToolSearchOptions::enabled());
        let names = router
            .declarations()
            .into_iter()
            .map(|declaration| declaration.name)
            .collect::<Vec<_>>();

        assert_eq!(
            router.effective_exposure("plugin_deferred"),
            Some(ToolExposure::Deferred)
        );
        assert_eq!(
            router.effective_exposure("plugin_hidden"),
            Some(ToolExposure::Hidden)
        );
        assert!(names.contains(&"tool_search".to_string()));
        assert!(!names.contains(&"plugin_deferred".to_string()));
        assert!(!names.contains(&"plugin_hidden".to_string()));
    }

    #[test]
    fn invalid_toolset_inputs_do_not_expose_tools_or_claim_acceptance() {
        let mut input = base_input(RunMode::Plan);
        let mut selection = ToolSelectionConfig::default();
        selection.modes.insert(
            "plan".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec![
                    "writer".to_string(),
                    "cycle-a".to_string(),
                    "missing".to_string(),
                ]),
                disabled_toolsets: Vec::new(),
            },
        );
        let mut custom_toolsets = BTreeMap::new();
        custom_toolsets.insert(
            "writer".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: vec!["write".to_string(), "ghost".to_string()],
                includes: Vec::new(),
            },
        );
        custom_toolsets.insert(
            "cycle-a".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: Vec::new(),
                includes: vec!["cycle-b".to_string()],
            },
        );
        custom_toolsets.insert(
            "cycle-b".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: Vec::new(),
                includes: vec!["cycle-a".to_string()],
            },
        );
        input.selection = compile_tool_selection(RunMode::Plan, &selection, &custom_toolsets, &[]);

        let result = assemble_tool_surface_with_warnings(input);
        assert!(!selected_names(&result).contains(&"write"));
        assert!(!selected_names(&result).contains(&"ghost"));
        assert!(
            !result
                .accepted_toolset_names
                .contains(&"cycle-a".to_string())
        );
        assert!(
            !result
                .accepted_toolset_names
                .contains(&"missing".to_string())
        );
    }

    #[test]
    fn duplicate_visible_extension_tool_warns_and_keeps_one_binding() {
        let mut input = base_input(RunMode::Default);
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("read")),
            "plugin:shadow@local",
            "plugin",
        ));

        let result = assemble_tool_surface_with_warnings(input);

        assert!(result.warnings.iter().any(|warning| {
            warning.kind == "capability_conflict" && warning.message.contains("plugin:shadow@local")
        }));
        assert_eq!(
            selected_names(&result)
                .iter()
                .filter(|name| **name == "read")
                .count(),
            1
        );
    }
}
