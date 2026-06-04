use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::{ToolBinding, ToolRouter};
use psychevo_ai::ToolDeclaration;

use crate::agents::{AgentToolContext, agent_tools};
use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig};
use crate::mcp::mcp_tool_name_parts;
use crate::skills::SkillDiscoveryOptions;
use crate::tools::{
    ToolRuntimeContext, clarify_tool, coding_core_tools_for_mode_with_selection,
    skill_tools_for_mode,
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
    pub(crate) warnings: Vec<RunWarning>,
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    assemble_tool_surface_with_warnings(input).tools
}

pub(crate) fn assemble_tool_surface_with_warnings(
    input: ToolSurfaceAssembly,
) -> ToolSurfaceAssemblyResult {
    let mut warnings = Vec::new();

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
        &mut warnings,
        ToolGroupSource {
            id: "builtin:toolset:coding-core",
        },
        tools.drain(..),
    );
    if let ClarifyToolSurface::Enabled { control, stream } = input.clarify {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut warnings,
            ToolGroupSource {
                id: "builtin:tool:clarify",
            },
            [clarify_tool(control, stream)],
        );
    }
    if let Some(skill_options) = input.skills {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut warnings,
            ToolGroupSource {
                id: "runtime:skills",
            },
            skill_tools_for_mode(skill_options, input.mode),
        );
    }
    for tool in input.extension_tools {
        let source_id = mcp_tool_name_parts(tool.name())
            .map(|(server, _)| format!("mcp:{server}"))
            .unwrap_or_else(|| "runtime:extension-tools".to_string());
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut warnings,
            ToolGroupSource { id: &source_id },
            [tool],
        );
    }
    if let Some(agent_context) = input.agents {
        register_tool_group(
            &mut selected_tools,
            &mut seen_tool_names,
            &mut warnings,
            ToolGroupSource {
                id: "runtime:agents",
            },
            agent_tools(agent_context),
        );
    }
    ToolSurfaceAssemblyResult {
        tools: selected_tools,
        warnings,
    }
}

pub(crate) fn tool_declarations(tools: &[Arc<dyn ToolBinding>]) -> Vec<ToolDeclaration> {
    ToolRouter::from_tools(tools.iter().cloned()).declarations()
}

#[derive(Debug, Clone, Copy)]
struct ToolGroupSource<'a> {
    id: &'a str,
}

fn register_tool_group<I>(
    selected_tools: &mut Vec<Arc<dyn ToolBinding>>,
    seen_tool_names: &mut std::collections::BTreeSet<String>,
    warnings: &mut Vec<RunWarning>,
    source: ToolGroupSource<'_>,
    tools: I,
) where
    I: IntoIterator<Item = Arc<dyn ToolBinding>>,
{
    for tool in tools {
        let visible_name = tool.name().to_string();
        if seen_tool_names.insert(visible_name.clone()) {
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
        }
    }
}
