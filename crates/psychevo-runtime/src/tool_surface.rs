use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::ToolBinding;
use psychevo_ai::ToolDeclaration;

use crate::agents::{AgentToolContext, agent_tools};
use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig};
use crate::skills::SkillDiscoveryOptions;
use crate::tools::{
    ToolRuntimeContext, clarify_tool, coding_core_tools_for_mode_with_selection,
    skill_tools_for_mode,
};
use crate::types::{ClarifyControl, RunMode, RunStreamSink};

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
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) tool_selection: ToolSelectionConfig,
    pub(crate) custom_toolsets: std::collections::BTreeMap<String, CustomToolsetConfig>,
    pub(crate) clarify: ClarifyToolSurface,
    pub(crate) skills: Option<SkillDiscoveryOptions>,
    pub(crate) agents: Option<AgentToolContext>,
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools = coding_core_tools_for_mode_with_selection(
        &input.workdir,
        input.mode,
        ToolRuntimeContext {
            task_id: input.task_id,
            lsp: input.lsp.clone(),
            allow_login_shell: input.allow_login_shell,
            stream_events: input.stream_events.clone(),
            path_prefixes: input.path_prefixes.clone(),
        },
        &input.tool_selection,
        &input.custom_toolsets,
    );
    if let ClarifyToolSurface::Enabled { control, stream } = input.clarify {
        tools.push(clarify_tool(control, stream));
    }
    if let Some(skill_options) = input.skills {
        tools.extend(skill_tools_for_mode(skill_options, input.mode));
    }
    if let Some(agent_context) = input.agents {
        tools.extend(agent_tools(agent_context));
    }
    tools
}

pub(crate) fn tool_declarations(tools: &[Arc<dyn ToolBinding>]) -> Vec<ToolDeclaration> {
    tools
        .iter()
        .map(|tool| ToolDeclaration {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters(),
        })
        .collect()
}
