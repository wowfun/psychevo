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
use crate::contribution_projection::{
    ContributionFact, ContributionProjection, ContributionStatus,
};
use crate::sandbox::{SandboxPolicy, SandboxWriteGrants};
use crate::skills::SkillDiscoveryOptions;
use crate::tools::{
    ToolRuntimeContext, builtin_toolset_description, builtin_toolset_names, builtin_toolset_tools,
    clarify_tool, default_enabled_toolsets, known_tool_name, skill_tools_for_mode,
    tool_allowed_in_mode, tool_by_name, tool_names_for_mode,
};
use crate::types::{ClarifyControl, RunMode, RunStreamSink, RunWarning, RuntimeTool};

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
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) sandbox_policy: SandboxPolicy,
    pub(crate) sandbox_grants: SandboxWriteGrants,
    pub(crate) tool_selection: ToolSelectionConfig,
    pub(crate) custom_toolsets: BTreeMap<String, CustomToolsetConfig>,
    pub(crate) contributed_toolsets: Vec<ToolsetContribution>,
    pub(crate) clarify: ClarifyToolSurface,
    pub(crate) skills: Option<SkillDiscoveryOptions>,
    pub(crate) extension_tools: Vec<RuntimeTool>,
    pub(crate) agents: Option<AgentToolContext>,
}

pub(crate) struct ToolSurfaceAssemblyResult {
    pub(crate) tools: Vec<Arc<dyn ToolBinding>>,
    pub(crate) warnings: Vec<RunWarning>,
    pub(crate) accepted_tool_names: Vec<String>,
    pub(crate) accepted_toolset_names: Vec<String>,
    pub(crate) projection: ContributionProjection,
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    assemble_tool_surface_with_warnings(input).tools
}

pub(crate) fn assemble_tool_surface_with_warnings(
    input: ToolSurfaceAssembly,
) -> ToolSurfaceAssemblyResult {
    let mut warnings = Vec::new();
    let mut projection = ContributionProjection::new();
    let mut available_tools = ToolRegistry::default();
    let runtime_context = ToolRuntimeContext {
        task_id: input.task_id,
        lsp: input.lsp.clone(),
        lsp_manager: crate::tools::write_support::default_lsp_manager(),
        allow_login_shell: input.allow_login_shell,
        stream_events: input.stream_events.clone(),
        env: input.env.clone(),
        path_prefixes: input.path_prefixes.clone(),
        sandbox_policy: input.sandbox_policy.clone(),
        sandbox_grants: input.sandbox_grants.clone(),
    };

    for name in tool_names_for_mode(input.mode) {
        if let Some(binding) = tool_by_name(name, &input.cwd, runtime_context.clone()) {
            available_tools.register(AvailableToolEntry::new(
                binding,
                format!("builtin:tool:{name}"),
                "builtin",
                "tool",
                "tool_surface",
            ));
        }
    }
    let mut fallback_entries = Vec::new();
    let mut selected_tools = Vec::new();
    let mut accepted_tool_names = Vec::new();
    let mut selected_tool_sources = BTreeMap::new();
    let toolsets = build_toolset_definitions(
        &input.custom_toolsets,
        &input.contributed_toolsets,
        &mut projection,
    );

    if let ClarifyToolSurface::Enabled { control, stream } = input.clarify {
        let entry = AvailableToolEntry::new(
            clarify_tool(control, stream),
            "builtin:tool:clarify",
            "builtin",
            "clarify_tool",
            "tool_surface",
        );
        available_tools.register(entry.clone());
        fallback_entries.push(entry);
    }
    if let Some(skill_options) = input.skills {
        for tool in skill_tools_for_mode(skill_options, input.mode) {
            let entry =
                AvailableToolEntry::new(tool, "runtime:skills", "runtime", "skill_tool", "skills");
            available_tools.register(entry.clone());
            fallback_entries.push(entry);
        }
    }
    for tool in input.extension_tools {
        let binding = extension_tool_binding(&tool, input.tool_selection.tool_search.enabled);
        let source_id = tool
            .source_id()
            .map(str::to_string)
            .unwrap_or_else(|| "runtime:extension-tools".to_string());
        let source_kind = tool
            .source_kind()
            .map(str::to_string)
            .unwrap_or_else(|| "runtime".to_string());
        let entry = AvailableToolEntry::new(
            binding,
            source_id,
            source_kind,
            "extension_tool",
            "tool_surface",
        );
        available_tools.register(entry.clone());
        fallback_entries.push(entry);
    }
    if let Some(agent_context) = input.agents {
        for tool in agent_tools(agent_context) {
            let entry =
                AvailableToolEntry::new(tool, "runtime:agents", "runtime", "agent_tool", "agents");
            available_tools.register(entry.clone());
            fallback_entries.push(entry);
        }
    }

    let selected_toolsets = selected_toolset_names(input.mode, &input.tool_selection);
    let disabled_toolsets = disabled_toolset_names(input.mode, &input.tool_selection);
    let mut accepted_toolset_names = Vec::new();
    let mut accepted_toolsets = BTreeSet::new();
    let mut expansion = ToolsetExpansion {
        mode: input.mode,
        definitions: &toolsets,
        registry: &available_tools,
        disabled_toolsets: &disabled_toolsets,
        selected_tools: &mut selected_tools,
        selected_tool_sources: &mut selected_tool_sources,
        accepted_tool_names: &mut accepted_tool_names,
        accepted_toolset_names: &mut accepted_toolset_names,
        accepted_toolsets: &mut accepted_toolsets,
        projection: &mut projection,
        warnings: &mut warnings,
        visiting: Vec::new(),
    };
    for toolset in selected_toolsets {
        expand_toolset(&toolset, &mut expansion);
    }

    for entry in fallback_entries {
        select_tool_entry(
            &entry,
            &mut *expansion.selected_tools,
            &mut *expansion.selected_tool_sources,
            &mut *expansion.accepted_tool_names,
            &mut *expansion.projection,
            &mut *expansion.warnings,
            true,
        );
    }
    drop(expansion);
    ToolSurfaceAssemblyResult {
        tools: selected_tools,
        warnings,
        accepted_tool_names,
        accepted_toolset_names,
        projection,
    }
}

pub(crate) fn tool_declarations(tools: &[Arc<dyn ToolBinding>]) -> Vec<ToolDeclaration> {
    ToolRouter::from_tools(tools.iter().cloned()).declarations()
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

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
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
}

#[derive(Clone)]
struct AvailableToolEntry {
    name: String,
    binding: Arc<dyn ToolBinding>,
    source_id: String,
    source_kind: String,
    declaration_family: &'static str,
    owner_module: &'static str,
}

impl AvailableToolEntry {
    fn new(
        binding: Arc<dyn ToolBinding>,
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
        declaration_family: &'static str,
        owner_module: &'static str,
    ) -> Self {
        let name = binding.name().to_string();
        Self {
            name,
            binding,
            source_id: source_id.into(),
            source_kind: source_kind.into(),
            declaration_family,
            owner_module,
        }
    }
}

#[derive(Clone)]
struct ToolsetDefinition {
    name: String,
    source_id: String,
    source_kind: String,
    config: CustomToolsetConfig,
}

fn build_toolset_definitions(
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
    contributed_toolsets: &[ToolsetContribution],
    projection: &mut ContributionProjection,
) -> BTreeMap<String, ToolsetDefinition> {
    let mut definitions = BTreeMap::new();
    for name in builtin_toolset_names() {
        if let Some(tools) = builtin_toolset_tools(name) {
            definitions.insert(
                (*name).to_string(),
                ToolsetDefinition {
                    name: (*name).to_string(),
                    source_id: format!("builtin:toolset:{name}"),
                    source_kind: "builtin".to_string(),
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
                source_id: format!("config:toolset:{name}"),
                source_kind: "config".to_string(),
                config: config.clone(),
            },
            projection,
        );
    }
    for contribution in contributed_toolsets {
        insert_toolset_definition(
            &mut definitions,
            ToolsetDefinition {
                name: contribution.name.clone(),
                source_id: contribution.source_id.clone(),
                source_kind: contribution.source_kind.clone(),
                config: contribution.config.clone(),
            },
            projection,
        );
    }
    definitions
}

fn insert_toolset_definition(
    definitions: &mut BTreeMap<String, ToolsetDefinition>,
    definition: ToolsetDefinition,
    projection: &mut ContributionProjection,
) {
    if let Some(existing) = definitions.get(&definition.name) {
        projection.record(
            ContributionFact::new(
                definition.source_id,
                definition.source_kind,
                "toolset",
                "tool_surface",
                format!("toolset:{}", definition.name),
                ContributionStatus::Conflict,
            )
            .with_reason(format!(
                "toolset name conflicts with declaration from `{}`",
                existing.source_id
            )),
        );
        return;
    }
    definitions.insert(definition.name.clone(), definition);
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
    accepted_tool_names: &'a mut Vec<String>,
    accepted_toolset_names: &'a mut Vec<String>,
    accepted_toolsets: &'a mut BTreeSet<String>,
    projection: &'a mut ContributionProjection,
    warnings: &'a mut Vec<RunWarning>,
    visiting: Vec<String>,
}

fn expand_toolset(name: &str, state: &mut ToolsetExpansion<'_>) -> bool {
    if state.disabled_toolsets.contains(name) {
        record_unknown_toolset_fact(
            name,
            ContributionStatus::Omitted,
            "toolset disabled for mode",
            state.projection,
        );
        return false;
    }

    let Some(definition) = state.definitions.get(name).cloned() else {
        record_unknown_toolset_fact(
            name,
            ContributionStatus::Omitted,
            "unknown toolset",
            state.projection,
        );
        return false;
    };

    if state.visiting.iter().any(|visiting| visiting == name) {
        state.projection.record(
            ContributionFact::new(
                definition.source_id,
                definition.source_kind,
                "toolset",
                "tool_surface",
                format!("toolset:{name}"),
                ContributionStatus::Omitted,
            )
            .with_reason("toolset include cycle detected"),
        );
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

    if accepted_binding {
        if state.accepted_toolsets.insert(name.to_string()) {
            state.accepted_toolset_names.push(name.to_string());
            state.projection.record(ContributionFact::new(
                definition.source_id,
                definition.source_kind,
                "toolset",
                "tool_surface",
                format!("toolset:{name}"),
                ContributionStatus::Accepted,
            ));
        }
    } else {
        state.projection.record(
            ContributionFact::new(
                definition.source_id,
                definition.source_kind,
                "toolset",
                "tool_surface",
                format!("toolset:{name}"),
                ContributionStatus::Omitted,
            )
            .with_reason("toolset did not resolve any executable tool bindings"),
        );
    }
    accepted_binding
}

fn select_named_tool(
    name: &str,
    toolset: &ToolsetDefinition,
    state: &mut ToolsetExpansion<'_>,
) -> bool {
    if known_tool_name(name) && !tool_allowed_in_mode(name, state.mode) {
        state.projection.record(
            ContributionFact::new(
                toolset.source_id.clone(),
                toolset.source_kind.clone(),
                "toolset_tool",
                "tool_surface",
                format!("tool:{name}"),
                ContributionStatus::Omitted,
            )
            .with_reason("tool is not available in the selected run mode"),
        );
        return false;
    }

    let Some(entries) = state.registry.entries(name) else {
        state.projection.record(
            ContributionFact::new(
                toolset.source_id.clone(),
                toolset.source_kind.clone(),
                "toolset_tool",
                "tool_surface",
                format!("tool:{name}"),
                ContributionStatus::Unavailable,
            )
            .with_reason("no execution binding is registered for this tool name"),
        );
        return false;
    };

    let mut accepted = false;
    for (index, entry) in entries.iter().enumerate() {
        if index == 0 {
            accepted |= select_tool_entry(
                entry,
                state.selected_tools,
                state.selected_tool_sources,
                state.accepted_tool_names,
                state.projection,
                state.warnings,
                false,
            );
        } else {
            state.projection.record(
                ContributionFact::new(
                    entry.source_id.clone(),
                    entry.source_kind.clone(),
                    entry.declaration_family,
                    entry.owner_module,
                    format!("tool:{}", entry.name),
                    ContributionStatus::Conflict,
                )
                .with_reason("visible tool name has another registered execution binding"),
            );
        }
    }
    accepted
}

fn select_tool_entry(
    entry: &AvailableToolEntry,
    selected_tools: &mut Vec<Arc<dyn ToolBinding>>,
    selected_tool_sources: &mut BTreeMap<String, String>,
    accepted_tool_names: &mut Vec<String>,
    projection: &mut ContributionProjection,
    warnings: &mut Vec<RunWarning>,
    warn_on_conflict: bool,
) -> bool {
    if let Some(existing_source) = selected_tool_sources.get(&entry.name) {
        if existing_source == &entry.source_id {
            return true;
        }
        projection.record(
            ContributionFact::new(
                entry.source_id.clone(),
                entry.source_kind.clone(),
                entry.declaration_family,
                entry.owner_module,
                format!("tool:{}", entry.name),
                ContributionStatus::Conflict,
            )
            .with_reason(format!(
                "visible tool name already accepted from `{existing_source}`"
            )),
        );
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
    accepted_tool_names.push(entry.name.clone());
    projection.record(ContributionFact::new(
        entry.source_id.clone(),
        entry.source_kind.clone(),
        entry.declaration_family,
        entry.owner_module,
        format!("tool:{}", entry.name),
        ContributionStatus::Accepted,
    ));
    true
}

fn record_unknown_toolset_fact(
    name: &str,
    status: ContributionStatus,
    reason: &str,
    projection: &mut ContributionProjection,
) {
    projection.record(
        ContributionFact::new(
            format!("toolset:{name}"),
            "unknown",
            "toolset",
            "tool_surface",
            format!("toolset:{name}"),
            status,
        )
        .with_reason(reason),
    );
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
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: SandboxPolicy::disabled(),
            sandbox_grants: SandboxWriteGrants::default(),
            tool_selection: ToolSelectionConfig::default(),
            custom_toolsets: BTreeMap::new(),
            contributed_toolsets: Vec::new(),
            clarify: ClarifyToolSurface::Disabled,
            skills: None,
            extension_tools: Vec::new(),
            agents: None,
        }
    }

    #[test]
    fn plugin_toolset_accepts_only_registered_tool_bindings() {
        let mut input = base_input(RunMode::Default);
        input.tool_selection.modes.insert(
            "default".to_string(),
            ToolModeConfig {
                enabled_toolsets: Some(vec!["plugin-pack".to_string()]),
                disabled_toolsets: Vec::new(),
            },
        );
        input.contributed_toolsets.push(ToolsetContribution {
            source_id: "plugin:demo@local".to_string(),
            source_kind: "plugin".to_string(),
            name: "plugin-pack".to_string(),
            config: CustomToolsetConfig {
                description: Some("plugin tools".to_string()),
                tools: vec!["plugin_do".to_string(), "missing_plugin_tool".to_string()],
                includes: vec!["coding-core".to_string()],
            },
        });
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
        assert!(
            result
                .accepted_tool_names
                .contains(&"plugin_do".to_string())
        );
        assert_eq!(
            result
                .accepted_tool_names
                .iter()
                .filter(|name| name.as_str() == "plugin_do")
                .count(),
            1
        );
        assert!(result.projection.facts().iter().any(|fact| {
            fact.status == ContributionStatus::Unavailable
                && fact.effect_target == "tool:missing_plugin_tool"
        }));
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
    fn host_owned_runtime_tools_stay_direct_with_default_tool_search() {
        let mut input = base_input(RunMode::Default);
        input
            .extension_tools
            .push(RuntimeTool::new(Arc::new(TestTool::new("automation"))));

        let result = assemble_tool_surface_with_warnings(input);
        let declarations = ToolRouter::from_tools(result.tools)
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
        input.tool_selection.tool_search.enabled = false;
        input.extension_tools.push(RuntimeTool::with_source(
            Arc::new(TestTool::new("plugin_lookup")),
            "plugin:demo@local",
            "plugin",
        ));

        let result = assemble_tool_surface_with_warnings(input);
        let declarations = ToolRouter::from_tools(result.tools)
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
    fn toolset_projection_records_unknown_cycles_and_plan_omissions() {
        let mut input = base_input(RunMode::Plan);
        input.tool_selection.modes.insert(
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
        input.custom_toolsets.insert(
            "writer".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: vec!["write".to_string(), "ghost".to_string()],
                includes: Vec::new(),
            },
        );
        input.custom_toolsets.insert(
            "cycle-a".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: Vec::new(),
                includes: vec!["cycle-b".to_string()],
            },
        );
        input.custom_toolsets.insert(
            "cycle-b".to_string(),
            CustomToolsetConfig {
                description: None,
                tools: Vec::new(),
                includes: vec!["cycle-a".to_string()],
            },
        );

        let result = assemble_tool_surface_with_warnings(input);
        let facts = result.projection.facts();

        assert!(facts.iter().any(|fact| {
            fact.status == ContributionStatus::Omitted
                && fact.effect_target == "tool:write"
                && fact
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("selected run mode"))
        }));
        assert!(facts.iter().any(|fact| {
            fact.status == ContributionStatus::Unavailable && fact.effect_target == "tool:ghost"
        }));
        assert!(facts.iter().any(|fact| {
            fact.status == ContributionStatus::Omitted
                && fact.effect_target == "toolset:cycle-a"
                && fact
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("cycle"))
        }));
        assert!(facts.iter().any(|fact| {
            fact.status == ContributionStatus::Omitted
                && fact.effect_target == "toolset:missing"
                && fact
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("unknown"))
        }));
    }

    #[test]
    fn duplicate_visible_extension_tool_warns_and_projects_conflict() {
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
        assert!(result.projection.facts().iter().any(|fact| {
            fact.status == ContributionStatus::Conflict
                && fact.source_id == "plugin:shadow@local"
                && fact.effect_target == "tool:read"
        }));
    }
}
