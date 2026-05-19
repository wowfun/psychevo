use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::ToolBinding;
use psychevo_ai::ToolDeclaration;

use crate::agents::{AgentToolContext, agent_tools};
use crate::skills::SkillDiscoveryOptions;
use crate::tools::{clarify_tool, coding_core_tools_for_mode, skill_tools_for_mode};
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
    pub(crate) mode: RunMode,
    pub(crate) clarify: ClarifyToolSurface,
    pub(crate) skills: Option<SkillDiscoveryOptions>,
    pub(crate) agents: Option<AgentToolContext>,
}

pub(crate) fn assemble_tool_surface(input: ToolSurfaceAssembly) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools = coding_core_tools_for_mode(&input.workdir, input.mode);
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
