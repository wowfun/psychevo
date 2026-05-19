use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::TextDiff;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time;

use crate::error::{Error, Result};
use crate::skills::{
    InstallOptions, SkillDiscoveryOptions, SkillTarget, create_skill, discover_skills,
    install_skill, list_skills_value, patch_skill, remove_skill, set_skill_enabled,
    view_skill_value,
};
use crate::types::RunMode;

const READ_MAX_BYTES: usize = 50 * 1024;
const READ_MAX_LINES: usize = 2000;
const BASH_DEFAULT_TIMEOUT_SECS: u64 = 120;
const BASH_MAX_TIMEOUT_SECS: u64 = 300;

pub(crate) fn default_bash_timeout_secs() -> u64 {
    BASH_DEFAULT_TIMEOUT_SECS
}

pub(crate) fn coding_core_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(workdir, RunMode::Build)
}

pub(crate) fn coding_core_tools_for_mode(
    workdir: &Path,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    match mode {
        RunMode::Plan => read_only_plan_tools(workdir),
        RunMode::Build => full_build_tools(workdir),
    }
}

pub(crate) fn clarify_tool(
    control: Option<Arc<crate::types::ClarifyControl>>,
    stream: Option<crate::types::RunStreamSink>,
) -> Arc<dyn ToolBinding> {
    Arc::new(ClarifyTool::new(control, stream))
}

pub(crate) fn skill_tools_for_mode(
    options: SkillDiscoveryOptions,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools: Vec<Arc<dyn ToolBinding>> = vec![
        Arc::new(ListSkillsTool::new(options.clone())),
        Arc::new(ViewSkillTool::new(options.clone())),
    ];
    if mode == RunMode::Build {
        tools.push(Arc::new(CreateSkillTool::new(options.clone())));
        tools.push(Arc::new(PatchSkillTool::new(options.clone())));
        tools.push(Arc::new(RemoveSkillTool::new(options.clone())));
        tools.push(Arc::new(EnableSkillTool::new(options.clone())));
        tools.push(Arc::new(DisableSkillTool::new(options.clone())));
        tools.push(Arc::new(InstallSkillTool::new(options)));
    }
    tools
}

fn full_build_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(WriteTool::new(workdir.to_path_buf())),
        Arc::new(EditTool::new(workdir.to_path_buf())),
        Arc::new(BashTool::new(workdir.to_path_buf())),
    ]
}

fn read_only_plan_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(ListTool::new(workdir.to_path_buf())),
        Arc::new(SearchTool::new(workdir.to_path_buf())),
    ]
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    match mode {
        RunMode::Plan => vec!["read", "list", "search"],
        RunMode::Build => vec!["read", "write", "edit", "bash"],
    }
}

pub(crate) fn mode_instruction(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Build => {
            "Runtime mode: default. You may use the available coding tools to read, edit, write, and run commands under the selected workdir. When available, use clarify for meaningful user decisions or feedback, not permission approval."
        }
        RunMode::Plan => {
            "Runtime mode: plan. This turn is hard read-only. Use only the available read, list, search, and clarify tools to inspect the workdir and ask meaningful user questions. Do not write files, edit files, run shell commands, or claim to have modified the workspace."
        }
    }
}

pub(crate) fn mode_instruction_for_tool_availability(
    mode: RunMode,
    tools_available: bool,
) -> &'static str {
    if tools_available {
        return mode_instruction(mode);
    }
    match mode {
        RunMode::Build => {
            "Runtime mode: default. No callable tools are available for this invocation. Answer from the provided instructions, context, and user prompt only; do not claim to read, write, edit, run shell commands, call agents, or access skills."
        }
        RunMode::Plan => {
            "Runtime mode: plan. No callable tools are available for this invocation. Answer from the provided instructions, context, and user prompt only; do not claim to inspect files, run searches, call agents, or access skills."
        }
    }
}

// Tool implementations are split by tool family and included in this module.
include!("workdir.rs");
include!("read.rs");
include!("list.rs");
include!("search.rs");
include!("write.rs");
include!("edit.rs");
include!("patch.rs");
include!("bash.rs");
include!("clarify.rs");
include!("skills.rs");
include!("args.rs");
include!("truncation.rs");
