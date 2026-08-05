use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use psychevo_agent_core::ToolBinding;

use crate::config::{
    CustomToolsetConfig, LspConfig, ResolvedImageGenerationConfig, ToolSelectionConfig,
    WebSearchConfig,
};
use crate::prompt_templates;
use crate::sandbox::{SandboxPolicy, SandboxWriteGrants};
#[cfg(test)]
use crate::skills::SkillDiscoveryOptions;
use crate::skills::SkillRuntime;
use crate::types::{RunMode, RunStreamSink, WorkspaceMutationSink};

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
    pub(crate) file_reads: FileReadTracker,
    pub(crate) lsp: LspConfig,
    pub(crate) lsp_manager: Arc<crate::tools::write_support::patch_lsp::LspManager>,
    pub(crate) allow_login_shell: bool,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) workspace_mutations: Option<WorkspaceMutationSink>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) sandbox_policy: SandboxPolicy,
    pub(crate) sandbox_grants: SandboxWriteGrants,
    pub(crate) home: Option<PathBuf>,
    pub(crate) image_input_enabled: bool,
    pub(crate) image_generation: Option<ResolvedImageGenerationConfig>,
    pub(crate) web_search: WebSearchConfig,
}

impl Default for ToolRuntimeContext {
    fn default() -> Self {
        Self {
            task_id: "default".to_string(),
            file_reads: FileReadTracker::default(),
            lsp: LspConfig::default(),
            lsp_manager: crate::tools::write_support::patch_lsp::default_lsp_manager(),
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
            web_search: WebSearchConfig::default(),
        }
    }
}

#[cfg(test)]
pub(crate) fn coding_core_tools(cwd: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(cwd, RunMode::Default)
}

pub(crate) fn coding_core_tools_for_mode(cwd: &Path, mode: RunMode) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode_with_context(cwd, mode, ToolRuntimeContext::default())
}

pub(crate) fn coding_core_tools_for_mode_with_context(
    cwd: &Path,
    mode: RunMode,
    context: ToolRuntimeContext,
) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode_with_selection(
        cwd,
        mode,
        context,
        &ToolSelectionConfig::default(),
        &BTreeMap::new(),
    )
}

pub(crate) fn coding_core_tools_for_mode_with_selection(
    cwd: &Path,
    mode: RunMode,
    context: ToolRuntimeContext,
    selection: &ToolSelectionConfig,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
) -> Vec<Arc<dyn ToolBinding>> {
    effective_tool_names_for_mode_with_config(mode, selection, custom_toolsets)
        .into_iter()
        .filter_map(|name| tool_by_name(&name, cwd, context.clone()))
        .collect()
}

pub(crate) fn clarify_tool(
    control: Option<Arc<crate::types::ClarifyControl>>,
    stream: Option<crate::types::RunStreamSink>,
) -> Arc<dyn ToolBinding> {
    Arc::new(ClarifyTool::new(control, stream))
}

#[cfg(test)]
pub(crate) fn skill_tools_for_mode(
    options: SkillDiscoveryOptions,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    skill_tools_for_mode_with_runtime(SkillRuntime::new(options), mode)
}

pub(crate) fn skill_tools_for_mode_with_runtime(
    runtime: SkillRuntime,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools: Vec<Arc<dyn ToolBinding>> = vec![
        Arc::new(ListSkillsTool::new(runtime.clone())),
        Arc::new(ViewSkillTool::new(runtime.clone())),
    ];
    if mode == RunMode::Default {
        tools.push(Arc::new(SkillManageTool::new(runtime.clone())));
        tools.push(Arc::new(SkillHubTool::new(runtime.clone(), mode)));
        tools.push(Arc::new(SkillConfigTool::new(runtime, mode)));
    } else {
        tools.push(Arc::new(SkillHubTool::new(runtime.clone(), mode)));
        tools.push(Arc::new(SkillConfigTool::new(runtime, mode)));
    }
    tools
}

type BuiltinToolFactory = fn(&Path, ToolRuntimeContext) -> Option<Arc<dyn ToolBinding>>;

const MODE_DEFAULT: u8 = 1 << 0;
const MODE_PLAN: u8 = 1 << 1;

struct BuiltinToolSpec {
    canonical_name: &'static str,
    aliases: &'static [&'static str],
    modes: u8,
    toolset: &'static str,
    factory: BuiltinToolFactory,
}

impl BuiltinToolSpec {
    fn allows(&self, mode: RunMode) -> bool {
        let flag = match mode {
            RunMode::Default => MODE_DEFAULT,
            RunMode::Plan => MODE_PLAN,
        };
        self.modes & flag != 0
    }

    fn matches(&self, name: &str) -> bool {
        self.canonical_name == name || self.aliases.contains(&name)
    }
}

static BUILTIN_TOOL_SPECS: &[BuiltinToolSpec] = &[
    BuiltinToolSpec {
        canonical_name: "read",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "coding-core",
        factory: |cwd, context| Some(Arc::new(ReadTool::new(cwd.to_path_buf(), context))),
    },
    BuiltinToolSpec {
        canonical_name: "write",
        aliases: &[],
        modes: MODE_DEFAULT,
        toolset: "coding-core",
        factory: |cwd, context| Some(Arc::new(WriteTool::new(cwd.to_path_buf(), context))),
    },
    BuiltinToolSpec {
        canonical_name: "edit",
        aliases: &[],
        modes: MODE_DEFAULT,
        toolset: "coding-core",
        factory: |cwd, context| Some(Arc::new(EditTool::new(cwd.to_path_buf(), context))),
    },
    BuiltinToolSpec {
        canonical_name: "exec_command",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "coding-core",
        factory: |cwd, context| Some(Arc::new(ExecCommandTool::new(cwd.to_path_buf(), context))),
    },
    BuiltinToolSpec {
        canonical_name: "write_stdin",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "coding-core",
        factory: |_cwd, context| {
            Some(Arc::new(WriteStdinTool::new(
                context.task_id,
                context.sandbox_policy,
            )))
        },
    },
    BuiltinToolSpec {
        canonical_name: "web_fetch",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "web",
        factory: |_cwd, _context| Some(Arc::new(WebFetchTool::new())),
    },
    BuiltinToolSpec {
        canonical_name: "web_search",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "web",
        factory: |_cwd, context| {
            (context.web_search.execution != crate::config::WebSearchExecution::Hosted).then(|| {
                Arc::new(WebSearchTool::new(
                    context.web_search,
                    context.env,
                    context.task_id,
                )) as Arc<dyn ToolBinding>
            })
        },
    },
    BuiltinToolSpec {
        canonical_name: "view_image",
        aliases: &[],
        modes: MODE_DEFAULT | MODE_PLAN,
        toolset: "vision",
        factory: |cwd, context| Some(Arc::new(ViewImageTool::new(cwd.to_path_buf(), context))),
    },
    BuiltinToolSpec {
        canonical_name: "image_generate",
        aliases: &["image_generation.generate"],
        modes: MODE_DEFAULT,
        toolset: "vision",
        factory: |cwd, context| Some(Arc::new(ImageGenerateTool::new(cwd.to_path_buf(), context))),
    },
];

fn builtin_tool_spec(name: &str) -> Option<&'static BuiltinToolSpec> {
    BUILTIN_TOOL_SPECS.iter().find(|spec| spec.matches(name))
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    BUILTIN_TOOL_SPECS
        .iter()
        .filter(|spec| spec.allows(mode))
        .map(|spec| spec.canonical_name)
        .collect()
}

pub(crate) fn effective_tool_names_for_mode_with_config(
    mode: RunMode,
    selection: &ToolSelectionConfig,
    custom_toolsets: &BTreeMap<String, CustomToolsetConfig>,
) -> Vec<String> {
    let toolsets = effective_toolset_names_for_mode_with_config(mode, selection);
    let disabled = disabled_toolset_names_for_mode(mode, selection);

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

pub(crate) fn effective_toolset_names_for_mode_with_config(
    mode: RunMode,
    selection: &ToolSelectionConfig,
) -> Vec<String> {
    let mode_config = selection.modes.get(mode.as_str());
    let mut toolsets = mode_config
        .and_then(|config| config.enabled_toolsets.clone())
        .unwrap_or_else(|| {
            default_enabled_toolsets()
                .into_iter()
                .map(|name| name.to_string())
                .collect()
        });
    let disabled = disabled_toolset_names_for_mode(mode, selection);
    toolsets.retain(|toolset| !disabled.contains(toolset.as_str()));
    toolsets
}

fn disabled_toolset_names_for_mode(
    mode: RunMode,
    selection: &ToolSelectionConfig,
) -> BTreeSet<&str> {
    selection
        .modes
        .get(mode.as_str())
        .map(|config| {
            config
                .disabled_toolsets
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn builtin_toolset_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    for spec in BUILTIN_TOOL_SPECS {
        if !names.contains(&spec.toolset) {
            names.push(spec.toolset);
        }
    }
    names
}

pub(crate) fn default_enabled_toolsets() -> Vec<&'static str> {
    builtin_toolset_names()
}

pub(crate) fn builtin_toolset_description(name: &str) -> Option<&'static str> {
    match name {
        "coding-core" => {
            Some("Local coding tools for reading files, editing files, and running shell commands.")
        }
        "web" => Some("Read-only tools for searching the web and fetching known URLs."),
        "vision" => Some("Image inspection and deterministic image generation tools."),
        _ => None,
    }
}

pub(crate) fn builtin_toolset_tools(name: &str) -> Option<Vec<&'static str>> {
    let tools = BUILTIN_TOOL_SPECS
        .iter()
        .filter(|spec| spec.toolset == name)
        .map(|spec| spec.canonical_name)
        .collect::<Vec<_>>();
    (!tools.is_empty()).then_some(tools)
}

pub(crate) fn tool_allowed_in_mode(name: &str, mode: RunMode) -> bool {
    builtin_tool_spec(name).is_some_and(|spec| spec.allows(mode))
}

pub(crate) fn known_tool_name(name: &str) -> bool {
    builtin_tool_spec(name).is_some()
}

pub(crate) fn builtin_tool_aliases(name: &str) -> &'static [&'static str] {
    builtin_tool_spec(name).map_or(&[], |spec| spec.aliases)
}

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
    cwd: &Path,
    context: ToolRuntimeContext,
) -> Option<Arc<dyn ToolBinding>> {
    let spec = builtin_tool_spec(name)?;
    (spec.factory)(cwd, context)
}

#[cfg(test)]
mod builtin_tool_spec_tests {
    use super::*;

    #[test]
    fn builtin_spec_identities_are_unique_and_factories_are_canonical() {
        let mut identities = HashSet::new();
        for spec in BUILTIN_TOOL_SPECS {
            assert!(
                identities.insert(spec.canonical_name),
                "duplicate canonical tool {}",
                spec.canonical_name
            );
            let binding = (spec.factory)(Path::new("/tmp"), ToolRuntimeContext::default())
                .expect("default builtin factory");
            assert_eq!(binding.name(), spec.canonical_name);
            for alias in spec.aliases {
                assert!(identities.insert(*alias), "duplicate alias {alias}");
                let aliased = tool_by_name(alias, Path::new("/tmp"), ToolRuntimeContext::default())
                    .expect("alias factory");
                assert_eq!(aliased.name(), spec.canonical_name);
            }
        }
    }

    #[test]
    fn builtin_mode_and_toolset_projections_come_from_the_specs() {
        for mode in [RunMode::Plan, RunMode::Default] {
            let names = tool_names_for_mode(mode);
            for spec in BUILTIN_TOOL_SPECS {
                assert_eq!(
                    names.contains(&spec.canonical_name),
                    spec.allows(mode),
                    "{} in {}",
                    spec.canonical_name,
                    mode.as_str()
                );
            }
        }
        for toolset in builtin_toolset_names() {
            let projected = builtin_toolset_tools(toolset).expect("toolset projection");
            assert_eq!(
                projected,
                BUILTIN_TOOL_SPECS
                    .iter()
                    .filter(|spec| spec.toolset == toolset)
                    .map(|spec| spec.canonical_name)
                    .collect::<Vec<_>>()
            );
        }
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

// Tool implementations are split by tool family.
pub(crate) mod args;
pub(crate) mod clarify;
pub(crate) mod cwd;
pub(crate) mod edit;
pub(crate) mod exec_command;
pub(crate) mod file_mutation;
pub(crate) mod image_tools;
pub(crate) mod read;
pub(crate) mod skills;
pub(crate) mod truncation;
pub(crate) mod web_fetch;
pub(crate) mod web_search;
pub(crate) mod web_url_policy;
pub(crate) mod write;
pub(crate) mod write_support;

use clarify::ClarifyTool;
use edit::EditTool;
use exec_command::sessions::session_manager::{ExecCommandTool, WriteStdinTool};
use file_mutation::FileReadTracker;
use image_tools::{ImageGenerateTool, ViewImageTool};
use read::ReadTool;
use skills::{ListSkillsTool, SkillConfigTool, SkillHubTool, SkillManageTool, ViewSkillTool};
use web_fetch::WebFetchTool;
use web_search::WebSearchTool;
use write::WriteTool;
