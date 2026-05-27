pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::io::{Read, Write};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Stdio;
pub(crate) use std::sync::{Arc, Condvar, LazyLock, Mutex};
pub(crate) use std::time::{Duration, Instant, SystemTime};
pub(crate) use std::{env, thread};

pub(crate) use futures::future::BoxFuture;
pub(crate) use psychevo_agent_core::{ToolAttachment, ToolBinding, ToolExecutionMode, ToolOutput};
pub(crate) use psychevo_ai::AbortSignal;
pub(crate) use serde::Deserialize;
pub(crate) use serde_json::{Value, json};
pub(crate) use similar::TextDiff;
pub(crate) use tokio::time;

pub(crate) use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::prompt_templates;
pub(crate) use crate::skills::{
    InstallOptions, ListSkillsOptions, SkillDiscoveryOptions, SkillTarget, create_skill,
    discover_skills, install_skill, list_skills_value_with_options, patch_skill, remove_skill,
    set_skill_config_value, set_skill_enabled, view_skill_value,
};
pub(crate) use crate::types::{RunMode, RunStreamEvent, RunStreamSink};

pub(crate) const READ_MAX_BYTES: usize = 50 * 1024;
pub(crate) const READ_MAX_LINES: usize = 2000;
pub(crate) const EXEC_DEFAULT_YIELD_TIME_MS: u64 = 10_000;
pub(crate) const WRITE_STDIN_DEFAULT_YIELD_TIME_MS: u64 = 250;
pub(crate) const EXEC_MIN_YIELD_TIME_MS: u64 = 250;
pub(crate) const EXEC_MAX_YIELD_TIME_MS: u64 = 30_000;
pub(crate) const EMPTY_POLL_MIN_YIELD_TIME_MS: u64 = 5_000;
pub(crate) const EMPTY_POLL_MAX_YIELD_TIME_MS: u64 = 300_000;
pub(crate) const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
pub(crate) const MAX_EXEC_SESSIONS: usize = 64;
#[cfg(test)]
pub(crate) const EXEC_DETACHED_SESSION_TTL: Duration = Duration::from_millis(50);
#[cfg(not(test))]
pub(crate) const EXEC_DETACHED_SESSION_TTL: Duration = Duration::from_secs(10);
pub(crate) const EXEC_STDIN_EVENT_MAX_CHARS: usize = 4096;

pub(crate) fn default_exec_max_output_tokens() -> usize {
    DEFAULT_MAX_OUTPUT_TOKENS
}

#[derive(Clone)]
pub(crate) struct ToolRuntimeContext {
    pub(crate) task_id: String,
    pub(crate) lsp: LspConfig,
    pub(crate) allow_login_shell: bool,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) path_prefixes: Vec<PathBuf>,
}

impl Default for ToolRuntimeContext {
    fn default() -> Self {
        Self {
            task_id: "default".to_string(),
            lsp: LspConfig::default(),
            allow_login_shell: false,
            stream_events: None,
            path_prefixes: Vec::new(),
        }
    }
}

#[cfg(test)]
pub(crate) fn coding_core_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(workdir, RunMode::Default)
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
    coding_core_tools_for_mode_with_selection(
        workdir,
        mode,
        context,
        &ToolSelectionConfig::default(),
        &BTreeMap::new(),
    )
}

pub(crate) fn coding_core_tools_for_mode_with_selection(
    workdir: &Path,
    mode: RunMode,
    context: ToolRuntimeContext,
    selection: &ToolSelectionConfig,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
) -> Vec<Arc<dyn ToolBinding>> {
    effective_tool_names_for_mode_with_config(mode, selection, custom_toolsets)
        .into_iter()
        .filter_map(|name| tool_by_name(&name, workdir, context.clone()))
        .collect()
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
    if mode == RunMode::Default {
        tools.push(Arc::new(SkillManageTool::new(options.clone())));
        tools.push(Arc::new(SkillHubTool::new(options.clone(), mode)));
        tools.push(Arc::new(SkillConfigTool::new(options, mode)));
    } else {
        tools.push(Arc::new(SkillHubTool::new(options.clone(), mode)));
        tools.push(Arc::new(SkillConfigTool::new(options, mode)));
    }
    tools
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    match mode {
        RunMode::Plan => vec!["read", "exec_command", "write_stdin", "web_fetch"],
        RunMode::Default => vec![
            "read",
            "write",
            "edit",
            "exec_command",
            "write_stdin",
            "web_fetch",
        ],
    }
}

pub(crate) fn effective_tool_names_for_mode_with_config(
    mode: RunMode,
    selection: &ToolSelectionConfig,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
) -> Vec<String> {
    let mode_config = selection.modes.get(mode.as_str());
    let mut toolsets = mode_config
        .and_then(|config| config.enabled_toolsets.clone())
        .unwrap_or_else(|| {
            DEFAULT_ENABLED_TOOLSETS
                .iter()
                .map(|name| name.to_string())
                .collect()
        });
    let disabled = mode_config
        .map(|config| {
            config
                .disabled_toolsets
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    toolsets.retain(|toolset| !disabled.contains(toolset.as_str()));

    let mut out = Vec::new();
    let mut seen_tools = HashSet::new();
    let mut visiting = BTreeSet::new();
    for toolset in toolsets {
        collect_toolset_tools(
            &toolset,
            mode,
            custom_toolsets,
            &disabled,
            &mut out,
            &mut seen_tools,
            &mut visiting,
        );
    }
    out
}

pub(crate) fn builtin_toolset_names() -> &'static [&'static str] {
    &["coding-core", "web"]
}

pub(crate) fn default_enabled_toolsets() -> &'static [&'static str] {
    &DEFAULT_ENABLED_TOOLSETS
}

pub(crate) fn builtin_toolset_description(name: &str) -> Option<&'static str> {
    match name {
        "coding-core" => {
            Some("Local coding tools for reading files, editing files, and running shell commands.")
        }
        "web" => Some("Read-only URL fetch tools for known web resources."),
        _ => None,
    }
}

pub(crate) fn builtin_toolset_tools(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "coding-core" => Some(&["read", "write", "edit", "exec_command", "write_stdin"]),
        "web" => Some(&["web_fetch"]),
        _ => None,
    }
}

pub(crate) fn tool_allowed_in_mode(name: &str, mode: RunMode) -> bool {
    match mode {
        RunMode::Plan => matches!(name, "read" | "exec_command" | "write_stdin" | "web_fetch"),
        RunMode::Default => matches!(
            name,
            "read" | "write" | "edit" | "exec_command" | "write_stdin" | "web_fetch"
        ),
    }
}

pub(crate) fn known_tool_name(name: &str) -> bool {
    matches!(
        name,
        "read" | "write" | "edit" | "exec_command" | "write_stdin" | "web_fetch"
    )
}

pub(crate) const DEFAULT_ENABLED_TOOLSETS: [&str; 2] = ["coding-core", "web"];

pub(crate) fn collect_toolset_tools(
    name: &str,
    mode: RunMode,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
    disabled_toolsets: &BTreeSet<&str>,
    out: &mut Vec<String>,
    seen_tools: &mut HashSet<String>,
    visiting: &mut BTreeSet<String>,
) {
    if disabled_toolsets.contains(name) {
        return;
    }
    if !visiting.insert(name.to_string()) {
        return;
    }

    if let Some(tools) = builtin_toolset_tools(name) {
        for tool in tools {
            push_tool_name(tool, mode, out, seen_tools);
        }
    } else if let Some(toolset) = custom_toolsets.get(name) {
        for include in &toolset.includes {
            collect_toolset_tools(
                include,
                mode,
                custom_toolsets,
                disabled_toolsets,
                out,
                seen_tools,
                visiting,
            );
        }
        for tool in &toolset.tools {
            push_tool_name(tool, mode, out, seen_tools);
        }
    }

    visiting.remove(name);
}

pub(crate) fn push_tool_name(
    name: &str,
    mode: RunMode,
    out: &mut Vec<String>,
    seen_tools: &mut HashSet<String>,
) {
    if known_tool_name(name)
        && tool_allowed_in_mode(name, mode)
        && seen_tools.insert(name.to_string())
    {
        out.push(name.to_string());
    }
}

pub(crate) fn tool_by_name(
    name: &str,
    workdir: &Path,
    context: ToolRuntimeContext,
) -> Option<Arc<dyn ToolBinding>> {
    match name {
        "read" => Some(Arc::new(ReadTool::new(workdir.to_path_buf(), context))),
        "write" => Some(Arc::new(WriteTool::new(workdir.to_path_buf(), context))),
        "edit" => Some(Arc::new(EditTool::new(workdir.to_path_buf(), context))),
        "exec_command" => Some(Arc::new(ExecCommandTool::new(
            workdir.to_path_buf(),
            context,
        ))),
        "write_stdin" => Some(Arc::new(WriteStdinTool::new())),
        "web_fetch" => Some(Arc::new(WebFetchTool::new())),
        _ => None,
    }
}

pub(crate) fn mode_instruction(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Default => prompt_templates::base_mode_default(),
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
        RunMode::Default => prompt_templates::base_mode_default_no_tools(),
        RunMode::Plan => prompt_templates::base_mode_plan_no_tools(),
    }
}

// Tool implementations are split by tool family and included in this module.
#[path = "workdir.rs"]
pub(crate) mod workdir;
#[allow(unused_imports)]
pub(crate) use workdir::*;
#[path = "file_state.rs"]
pub(crate) mod file_state;
#[allow(unused_imports)]
pub(crate) use file_state::*;
#[path = "write_support.rs"]
pub(crate) mod write_support;
#[allow(unused_imports)]
pub(crate) use write_support::*;
#[path = "read.rs"]
pub(crate) mod read;
#[allow(unused_imports)]
pub(crate) use read::*;
#[path = "write.rs"]
pub(crate) mod write;
#[allow(unused_imports)]
pub(crate) use write::*;
#[path = "edit.rs"]
pub(crate) mod edit;
#[allow(unused_imports)]
pub(crate) use edit::*;
#[path = "exec_command.rs"]
pub(crate) mod exec_command;
#[allow(unused_imports)]
pub(crate) use exec_command::*;
#[path = "clarify.rs"]
pub(crate) mod clarify;
#[allow(unused_imports)]
pub(crate) use clarify::*;
#[path = "skills.rs"]
pub(crate) mod skills;
#[allow(unused_imports)]
pub(crate) use skills::*;
#[path = "args.rs"]
pub(crate) mod args;
#[allow(unused_imports)]
pub(crate) use args::*;
#[path = "truncation.rs"]
pub(crate) mod truncation;
#[allow(unused_imports)]
pub(crate) use truncation::*;
#[path = "web_fetch.rs"]
pub(crate) mod web_fetch;
#[allow(unused_imports)]
pub(crate) use web_fetch::*;
