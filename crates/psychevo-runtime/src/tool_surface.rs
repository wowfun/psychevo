use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::{ToolBinding, ToolRouter};
use psychevo_ai::ToolDeclaration;
use serde_json::json;

use crate::agents::{AgentToolContext, agent_tools};
use crate::capabilities::{
    CapabilityCategory, CapabilityContributionRecord, CapabilitySnapshotParts, source_record,
};
use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig};
use crate::mcp::mcp_tool_name_parts;
use crate::skills::SkillDiscoveryOptions;
use crate::tools::{
    ToolRuntimeContext, clarify_tool, coding_core_tools_for_mode_with_selection,
    effective_toolset_names_for_mode_with_config, skill_tools_for_mode,
};
use crate::types::{ClarifyControl, RunMode, RunStreamSink, RunWarning};

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
    pub(crate) workdir: PathBuf,
    pub(crate) task_id: String,
    pub(crate) mode: RunMode,
    pub(crate) lsp: LspConfig,
    pub(crate) allow_login_shell: bool,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) tool_selection: ToolSelectionConfig,
    pub(crate) custom_toolsets: std::collections::BTreeMap<String, CustomToolsetConfig>,
    pub(crate) clarify: ClarifyToolSurface,
    pub(crate) skills: Option<SkillDiscoveryOptions>,
    pub(crate) extension_tools: Vec<Arc<dyn ToolBinding>>,
    pub(crate) agents: Option<AgentToolContext>,
}

pub(crate) struct ToolSurfaceAssemblyResult {
    pub(crate) tools: Vec<Arc<dyn ToolBinding>>,
    pub(crate) capability_parts: CapabilitySnapshotParts,
    pub(crate) warnings: Vec<RunWarning>,
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    assemble_tool_surface_with_capabilities(input).tools
}

pub(crate) fn assemble_tool_surface_with_capabilities(
    input: ToolSurfaceAssembly,
) -> ToolSurfaceAssemblyResult {
    let mut parts = CapabilitySnapshotParts::default();
    let mut warnings = Vec::new();
    record_toolset_contributions(&input, &mut parts);

    let mut tools = coding_core_tools_for_mode_with_selection(
        &input.workdir,
        input.mode,
        ToolRuntimeContext {
            task_id: input.task_id,
            lsp: input.lsp.clone(),
            lsp_manager: crate::tools::write_support::default_lsp_manager(),
            allow_login_shell: input.allow_login_shell,
            stream_events: input.stream_events.clone(),
            env: input.env.clone(),
            path_prefixes: input.path_prefixes.clone(),
        },
        &input.tool_selection,
        &input.custom_toolsets,
    );
    let mut selected_tools = Vec::new();
    let mut seen_tool_names = std::collections::BTreeSet::new();
    register_tool_group(
        &mut selected_tools,
        &mut seen_tool_names,
        &mut parts,
        &mut warnings,
        ToolGroupSource {
            id: "builtin:toolset:coding-core",
            kind: "builtin",
            raw_identity: "coding-core",
            lifetime: "static",
        },
        tools.drain(..),
    );
    if let ClarifyToolSurface::Enabled { control, stream } = input.clarify {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut parts,
            &mut warnings,
            ToolGroupSource {
                id: "builtin:tool:clarify",
                kind: "builtin",
                raw_identity: "clarify",
                lifetime: "static",
            },
            [clarify_tool(control, stream)],
        );
    }
    if let Some(skill_options) = input.skills {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut parts,
            &mut warnings,
            ToolGroupSource {
                id: "runtime:skills",
                kind: "runtime",
                raw_identity: "skill tools",
                lifetime: "session_snapshot",
            },
            skill_tools_for_mode(skill_options, input.mode),
        );
    }
    for tool in input.extension_tools {
        let (source_id, source_kind, raw_identity) = mcp_tool_name_parts(tool.name())
            .map(|(server, _)| {
                (
                    format!("mcp:{server}"),
                    "mcp".to_string(),
                    format!("MCP server `{server}`"),
                )
            })
            .unwrap_or_else(|| {
                (
                    "runtime:extension-tools".to_string(),
                    "runtime".to_string(),
                    "runtime extension tools".to_string(),
                )
            });
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut parts,
            &mut warnings,
            ToolGroupSource {
                id: &source_id,
                kind: &source_kind,
                raw_identity: &raw_identity,
                lifetime: "session_snapshot",
            },
            [tool],
        );
    }
    if let Some(agent_context) = input.agents {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut parts,
            &mut warnings,
            ToolGroupSource {
                id: "runtime:agents",
                kind: "runtime",
                raw_identity: "agent tools",
                lifetime: "session_snapshot",
            },
            agent_tools(agent_context),
        );
    }
    ToolSurfaceAssemblyResult {
        tools: selected_tools,
        capability_parts: parts,
        warnings,
    }
}

pub(crate) fn tool_declarations(tools: &[Arc<dyn ToolBinding>]) -> Vec<ToolDeclaration> {
    ToolRouter::from_tools(tools.iter().cloned()).declarations()
}

fn record_toolset_contributions(input: &ToolSurfaceAssembly, parts: &mut CapabilitySnapshotParts) {
    let source_id = "runtime:toolsets";
    parts.push_source(source_record(
        source_id,
        "runtime",
        "toolset selection",
        None,
        "session_snapshot",
        None,
    ));
    for toolset in effective_toolset_names_for_mode_with_config(input.mode, &input.tool_selection) {
        let is_custom = input.custom_toolsets.contains_key(&toolset);
        let contribution_id = format!("{source_id}:toolset:{toolset}");
        parts.push_selected(CapabilityContributionRecord {
            id: contribution_id,
            source_id: source_id.to_string(),
            category: CapabilityCategory::Toolset,
            raw_name: toolset.clone(),
            visible_name: Some(toolset),
            exposure: None,
            status: "selected".to_string(),
            reason: None,
            metadata: Some(json!({
                "mode": input.mode.as_str(),
                "custom": is_custom,
            })),
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct ToolGroupSource<'a> {
    id: &'a str,
    kind: &'a str,
    raw_identity: &'a str,
    lifetime: &'a str,
}

fn register_tool_group<I>(
    selected_tools: &mut Vec<Arc<dyn ToolBinding>>,
    seen_tool_names: &mut std::collections::BTreeSet<String>,
    parts: &mut CapabilitySnapshotParts,
    warnings: &mut Vec<RunWarning>,
    source: ToolGroupSource<'_>,
    tools: I,
) where
    I: IntoIterator<Item = Arc<dyn ToolBinding>>,
{
    parts.push_source(source_record(
        source.id,
        source.kind,
        source.raw_identity,
        None,
        source.lifetime,
        None,
    ));
    for tool in tools {
        let visible_name = tool.name().to_string();
        let exposure = tool.exposure();
        let conflict = seen_tool_names.contains(&visible_name);
        let contribution = CapabilityContributionRecord {
            id: format!("{}:tool:{visible_name}", source.id),
            source_id: source.id.to_string(),
            category: CapabilityCategory::Tool,
            raw_name: visible_name.clone(),
            visible_name: Some(visible_name.clone()),
            exposure: Some(exposure),
            status: if conflict { "rejected" } else { "selected" }.to_string(),
            reason: if conflict {
                Some("model-visible tool name conflict".to_string())
            } else {
                None
            },
            metadata: Some(json!({
                "execution_mode": format!("{:?}", tool.execution_mode()),
            })),
        };
        if seen_tool_names.insert(visible_name.clone()) {
            parts.push_selected(contribution);
            selected_tools.push(tool);
        } else {
            warnings.push(RunWarning {
                kind: "capability_conflict".to_string(),
                message: format!(
                    "Tool `{visible_name}` from `{}` conflicts with an already selected tool; omitted",
                    source.id
                ),
                source_path: None,
                suggestion: None,
            });
            parts.push_omitted(contribution, "rejected");
        }
    }
}
