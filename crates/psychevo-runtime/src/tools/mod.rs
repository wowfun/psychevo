use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};
use std::{env, thread};

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::TextDiff;
use tokio::time;

use crate::config::LspConfig;
use crate::error::{Error, Result};
use crate::prompt_templates;
use crate::skills::{
    InstallOptions, SkillDiscoveryOptions, SkillTarget, create_skill, discover_skills,
    install_skill, list_skills_value, patch_skill, remove_skill, set_skill_enabled,
    view_skill_value,
};
use crate::types::{RunMode, RunStreamEvent, RunStreamSink};

const READ_MAX_BYTES: usize = 50 * 1024;
const READ_MAX_LINES: usize = 2000;
const EXEC_DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const WRITE_STDIN_DEFAULT_YIELD_TIME_MS: u64 = 250;
const EXEC_MIN_YIELD_TIME_MS: u64 = 250;
const EXEC_MAX_YIELD_TIME_MS: u64 = 30_000;
const EMPTY_POLL_MIN_YIELD_TIME_MS: u64 = 5_000;
const EMPTY_POLL_MAX_YIELD_TIME_MS: u64 = 300_000;
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
const MAX_EXEC_SESSIONS: usize = 64;
#[cfg(test)]
const EXEC_DETACHED_SESSION_TTL: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const EXEC_DETACHED_SESSION_TTL: Duration = Duration::from_secs(10);
const EXEC_STDIN_EVENT_MAX_CHARS: usize = 4096;

pub(crate) fn default_exec_max_output_tokens() -> usize {
    DEFAULT_MAX_OUTPUT_TOKENS
}

#[derive(Clone)]
pub(crate) struct ToolRuntimeContext {
    pub(crate) task_id: String,
    pub(crate) lsp: LspConfig,
    pub(crate) allow_login_shell: bool,
    pub(crate) stream_events: Option<RunStreamSink>,
}

impl Default for ToolRuntimeContext {
    fn default() -> Self {
        Self {
            task_id: "default".to_string(),
            lsp: LspConfig::default(),
            allow_login_shell: false,
            stream_events: None,
        }
    }
}

pub(crate) fn coding_core_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(workdir, RunMode::Build)
}

pub(crate) fn coding_core_tools_for_mode(
    workdir: &Path,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode_with_context(workdir, mode, ToolRuntimeContext::default())
}

pub(crate) fn coding_core_tools_for_mode_with_context(
    workdir: &Path,
    mode: RunMode,
    context: ToolRuntimeContext,
) -> Vec<Arc<dyn ToolBinding>> {
    match mode {
        RunMode::Plan => read_only_plan_tools(workdir, context),
        RunMode::Build => full_build_tools(workdir, context),
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

fn full_build_tools(workdir: &Path, context: ToolRuntimeContext) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf(), context.clone())),
        Arc::new(WriteTool::new(workdir.to_path_buf(), context.clone())),
        Arc::new(EditTool::new(workdir.to_path_buf(), context.clone())),
        Arc::new(ExecCommandTool::new(workdir.to_path_buf(), context.clone())),
        Arc::new(WriteStdinTool::new()),
    ]
}

fn read_only_plan_tools(workdir: &Path, context: ToolRuntimeContext) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf(), context)),
        Arc::new(ListTool::new(workdir.to_path_buf())),
        Arc::new(SearchTool::new(workdir.to_path_buf())),
    ]
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    match mode {
        RunMode::Plan => vec!["read", "list", "search"],
        RunMode::Build => vec!["read", "write", "edit", "exec_command", "write_stdin"],
    }
}

pub(crate) fn mode_instruction(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Build => prompt_templates::base_mode_build(),
        RunMode::Plan => prompt_templates::base_mode_plan(),
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
        RunMode::Build => prompt_templates::base_mode_build_no_tools(),
        RunMode::Plan => prompt_templates::base_mode_plan_no_tools(),
    }
}

// Tool implementations are split by tool family and included in this module.
include!("workdir.rs");
include!("file_state.rs");
include!("write_support.rs");
include!("read.rs");
include!("list.rs");
include!("search.rs");
include!("write.rs");
include!("edit.rs");
include!("exec_command.rs");
include!("clarify.rs");
include!("skills.rs");
include!("args.rs");
include!("truncation.rs");
