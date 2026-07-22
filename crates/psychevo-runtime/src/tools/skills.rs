#[allow(unused_imports)]
pub(crate) use super::*;
use crate::skills::{find_skill, skill_prompt_visible_for_activation};
#[derive(Clone)]
pub(crate) struct ListSkillsTool {
    pub(crate) options: SkillDiscoveryOptions,
}

impl ListSkillsTool {
    pub(crate) fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for ListSkillsTool {
    fn name(&self) -> &str {
        "list_skills"
    }

    fn description(&self) -> &str {
        "List available skills."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_hidden": {
                    "type": "boolean",
                    "description": "When true, include hidden, disabled, unsupported-platform, and diagnostics-only skills."
                },
                "detail": {
                    "type": "boolean",
                    "description": "When true, include richer metadata such as paths, tags, readiness, setup, linked files, and diagnostics."
                },
                "category": {
                    "type": "string",
                    "description": "Optional category or bundle namespace filter for the skill listing."
                },
                "source": {
                    "type": "string",
                    "description": "Optional discovery-source filter such as explicit, project, ancestor, global, or configured external paths."
                },
                "enabled_only": {
                    "type": "boolean",
                    "description": "When true, omit disabled, hidden, unsupported, or otherwise inactive skills."
                },
                "platform": {
                    "type": "string",
                    "description": "Optional platform filter, usually linux, macos, or windows."
                },
                "tag": {
                    "type": "string",
                    "description": "Optional skill tag filter."
                },
                "readiness": {
                    "type": "string",
                    "description": "Optional readiness filter, for example ready, needs_env, needs_credentials, unsupported, or invalid."
                },
                "sort": {
                    "type": "string",
                    "description": "Optional sort order for the returned skills."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match discover_skills(&options) {
                Ok(catalog) => ToolOutput::ok(list_skills_value_with_options(
                    &catalog,
                    &ListSkillsOptions {
                        include_hidden: args
                            .get("include_hidden")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        detail: args.get("detail").and_then(Value::as_bool).unwrap_or(false),
                        category: optional_owned(&args, "category"),
                        source: optional_owned(&args, "source"),
                        enabled_only: args
                            .get("enabled_only")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        platform: optional_owned(&args, "platform"),
                        tag: optional_owned(&args, "tag"),
                        readiness: optional_owned(&args, "readiness"),
                        sort: optional_owned(&args, "sort"),
                    },
                )),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct ViewSkillTool {
    pub(crate) options: SkillDiscoveryOptions,
}

impl ViewSkillTool {
    pub(crate) fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for ViewSkillTool {
    fn name(&self) -> &str {
        "view_skill"
    }

    fn description(&self) -> &str {
        "Read a skill's instructions or a supporting file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name or unambiguous skill path/category reference to load."
                },
                "file_path": {
                    "type": "string",
                    "description": "Optional supporting file path inside the skill directory, such as references/note.md or scripts/setup.sh. Omit to load SKILL.md."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match view_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn view_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let catalog = discover_skills(options)?;
    let name = required_string(&args, "name")?;
    let file_path = optional_string(&args, "file_path")?;
    let skill = find_skill(&catalog, name)?;
    if !skill_prompt_visible_for_activation(skill) {
        return Err(Error::Message(format!(
            "skill is not available for model invocation: {name}"
        )));
    }
    view_skill_value(&catalog, name, file_path)
}

#[derive(Clone)]
pub(crate) struct SkillManageTool {
    pub(crate) options: SkillDiscoveryOptions,
}

impl SkillManageTool {
    pub(crate) fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for SkillManageTool {
    fn name(&self) -> &str {
        "skill_manage"
    }

    fn description(&self) -> &str {
        "Create, edit, or delete locally authored skills and their supporting files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "edit", "patch", "delete", "write_file", "remove_file"],
                    "description": "Skill mutation to perform: create a package, replace SKILL.md, patch SKILL.md text, delete a skill, write a supporting file, or remove a supporting file."
                },
                "name": {
                    "type": "string",
                    "description": "Target skill name."
                },
                "description": {
                    "type": "string",
                    "description": "Required short description of when the skill applies when action is create."
                },
                "content": {
                    "type": "string",
                    "description": "Complete replacement content for SKILL.md or for the supporting file named by file_path."
                },
                "old": {
                    "type": "string",
                    "description": "Existing SKILL.md text to replace when action is patch; must identify the intended text unambiguously."
                },
                "new": {
                    "type": "string",
                    "description": "Replacement text for old when action is patch."
                },
                "file_path": {
                    "type": "string",
                    "description": "Supporting file path inside the skill directory for write_file or remove_file."
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Where to create or update the skill: the global profile or the current project."
                },
                "intent": {
                    "type": "string",
                    "description": "Required reason or intent for deleting a skill."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match skill_manage_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn skill_manage_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let action = required_string(&args, "action")?;
    match action {
        "create" => {
            let name = required_string(&args, "name")?;
            let description = required_string(&args, "description")?;
            let value = create_skill(
                &options.home,
                &options.cwd,
                target_from_args(&args)?,
                name,
                description,
            )?;
            if let Some(content) = optional_string(&args, "content")? {
                let catalog = discover_skills(options)?;
                crate::skills::edit_skill(&catalog, &options.home, &options.cwd, name, content)?;
            }
            Ok(value)
        }
        "edit" => {
            let catalog = discover_skills(options)?;
            crate::skills::edit_skill(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
                required_string(&args, "content")?,
            )
        }
        "patch" => {
            let catalog = discover_skills(options)?;
            patch_skill(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
                required_string(&args, "old")?,
                required_string(&args, "new")?,
            )
        }
        "delete" => {
            let intent = optional_string(&args, "intent")?.unwrap_or_default();
            if intent.trim().is_empty() {
                return Err(Error::Message(
                    "delete requires an intent or reason".to_string(),
                ));
            }
            let catalog = discover_skills(options)?;
            remove_skill(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
            )
        }
        "write_file" => {
            let catalog = discover_skills(options)?;
            crate::skills::write_skill_file(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
                required_string(&args, "file_path")?,
                required_string(&args, "content")?,
            )
        }
        "remove_file" => {
            let catalog = discover_skills(options)?;
            crate::skills::remove_skill_file(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
                required_string(&args, "file_path")?,
            )
        }
        other => Err(Error::Message(format!(
            "unknown skill_manage action: {other}"
        ))),
    }
}

#[derive(Clone)]
pub(crate) struct SkillHubTool {
    pub(crate) options: SkillDiscoveryOptions,
    pub(crate) mode: RunMode,
}

impl SkillHubTool {
    pub(crate) fn new(options: SkillDiscoveryOptions, mode: RunMode) -> Self {
        Self { options, mode }
    }
}

impl ToolBinding for SkillHubTool {
    fn name(&self) -> &str {
        "skill_hub"
    }

    fn description(&self) -> &str {
        "Discover and manage skills through the skills hub."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["browse", "search", "inspect", "list", "check", "audit", "install", "update", "uninstall", "publish"],
                    "description": "Hub operation: browse/search local index, inspect one skill, list installed skills, check updates, audit skill files, install, update, uninstall, or prepare publishing."
                },
                "identifier": {
                    "type": "string",
                    "description": "Skill identifier, source path, URL, or registry id used by inspect or install."
                },
                "query": {
                    "type": "string",
                    "description": "Search text for browse/search actions; matches skill names and descriptions."
                },
                "name": {
                    "type": "string",
                    "description": "Skill name used by inspect, install-as-new-name, uninstall, or other name-targeted actions."
                },
                "source": {
                    "type": "string",
                    "description": "Optional source selector for hub operations when multiple sources are configured."
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Install or uninstall target scope: the global profile or the current project."
                },
                "path": {
                    "type": "string",
                    "description": "Filesystem path to audit or publish."
                },
                "repo": {
                    "type": "string",
                    "description": "Optional GitHub repository target for publish preparation."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of skills to return for browse or search; defaults to 20."
                },
                "all": {
                    "type": "boolean",
                    "description": "When true for install, include all files from the source package instead of only the main skill package."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        let mode = self.mode;
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match skill_hub_impl(&options, mode, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn skill_hub_impl(
    options: &SkillDiscoveryOptions,
    mode: RunMode,
    args: Value,
) -> Result<Value> {
    let action = required_string(&args, "action")?;
    let readonly = matches!(
        action,
        "browse" | "search" | "inspect" | "list" | "check" | "audit"
    );
    if mode == RunMode::Plan && !readonly {
        return Err(Error::Message(format!(
            "skill_hub action {action} is not available in plan mode"
        )));
    }
    match action {
        "browse" | "search" => {
            let catalog = discover_skills(options)?;
            let query = optional_string(&args, "query")?
                .unwrap_or_default()
                .to_lowercase();
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let skills = catalog
                .skills
                .into_iter()
                .filter(|skill| {
                    query.is_empty()
                        || skill.name.contains(&query)
                        || skill.description.to_lowercase().contains(&query)
                })
                .take(limit)
                .map(|skill| json!({"name": skill.name, "description": skill.description, "source": skill.source.as_str()}))
                .collect::<Vec<_>>();
            Ok(json!({"success": true, "action": action, "skills": skills}))
        }
        "inspect" => {
            let catalog = discover_skills(options)?;
            let name = optional_string(&args, "identifier")?
                .or_else(|| optional_string(&args, "name").ok().flatten())
                .ok_or_else(|| Error::Message("identifier is required".to_string()))?;
            view_skill_value(&catalog, name, None)
        }
        "list" => {
            let catalog = discover_skills(options)?;
            Ok(list_skills_value_with_options(
                &catalog,
                &ListSkillsOptions {
                    include_hidden: true,
                    detail: true,
                    ..ListSkillsOptions::default()
                },
            ))
        }
        "check" => Ok(
            json!({"success": true, "updates": [], "message": "no hub update source configured"}),
        ),
        "audit" => {
            let path = optional_string(&args, "path")?;
            if let Some(path) = path {
                let scan = crate::skills::scan_skill_path(Path::new(path))?;
                Ok(json!({"success": true, "scan": scan}))
            } else {
                let catalog = discover_skills(options)?;
                let scans = catalog
                    .skills
                    .iter()
                    .map(|skill| {
                        crate::skills::scan_skill_path(&skill.base_dir)
                            .map(|scan| json!({"name": skill.name, "path": skill.base_dir, "scan": scan}))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(json!({"success": true, "scans": scans}))
            }
        }
        "install" => {
            let identifier = required_string(&args, "identifier")?.to_string();
            install_skill(
                &options.home,
                &options.cwd,
                InstallOptions {
                    source: identifier,
                    target: target_from_args(&args)?,
                    name: optional_owned(&args, "name"),
                    all: args.get("all").and_then(Value::as_bool).unwrap_or(false),
                    force: false,
                },
            )
        }
        "update" => Ok(
            json!({"success": true, "updated": [], "message": "hub update is not configured for this source"}),
        ),
        "uninstall" => {
            let catalog = discover_skills(options)?;
            remove_skill(
                &catalog,
                &options.home,
                &options.cwd,
                required_string(&args, "name")?,
            )
        }
        "publish" => {
            let path = required_string(&args, "path")?;
            let scan = crate::skills::scan_skill_path(Path::new(path))?;
            if scan.verdict == crate::skills::ScanVerdict::Dangerous {
                return Err(Error::Message(
                    "cannot publish a skill with dangerous scanner verdict".to_string(),
                ));
            }
            Ok(json!({
                "success": false,
                "error": "GitHub PR publish requires CLI authentication flow",
                "scan": scan,
                "repo": optional_string(&args, "repo")?,
            }))
        }
        other => Err(Error::Message(format!("unknown skill_hub action: {other}"))),
    }
}

#[derive(Clone)]
pub(crate) struct SkillConfigTool {
    pub(crate) options: SkillDiscoveryOptions,
    pub(crate) mode: RunMode,
}

impl SkillConfigTool {
    pub(crate) fn new(options: SkillDiscoveryOptions, mode: RunMode) -> Self {
        Self { options, mode }
    }
}

impl ToolBinding for SkillConfigTool {
    fn name(&self) -> &str {
        "skill_config"
    }

    fn description(&self) -> &str {
        "Inspect or update skill enablement and skill-declared configuration."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "enable", "disable", "set"],
                    "description": "Configuration operation: read skill status, enable a skill, disable a skill, or set a skills.config.* value."
                },
                "name": {
                    "type": "string",
                    "description": "Skill name to enable or disable."
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Configuration scope: the global profile or the current project."
                },
                "key": {
                    "type": "string",
                    "description": "Configuration key under skills.config.* when action is set."
                },
                "value": {
                    "description": "JSON value to store for key when action is set."
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        let mode = self.mode;
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match skill_config_impl(&options, mode, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn skill_config_impl(
    options: &SkillDiscoveryOptions,
    mode: RunMode,
    args: Value,
) -> Result<Value> {
    let action = required_string(&args, "action")?;
    if mode == RunMode::Plan && action != "status" {
        return Err(Error::Message(format!(
            "skill_config action {action} is not available in plan mode"
        )));
    }
    match action {
        "status" => {
            let catalog = discover_skills(options)?;
            Ok(list_skills_value_with_options(
                &catalog,
                &ListSkillsOptions {
                    include_hidden: true,
                    detail: true,
                    ..ListSkillsOptions::default()
                },
            ))
        }
        "enable" => set_skill_enabled(
            &options.home,
            &options.cwd,
            target_from_args(&args)?,
            required_string(&args, "name")?,
            true,
        ),
        "disable" => set_skill_enabled(
            &options.home,
            &options.cwd,
            target_from_args(&args)?,
            required_string(&args, "name")?,
            false,
        ),
        "set" => {
            let value = args.get("value").cloned().ok_or_else(|| {
                Error::Message("value is required for skill_config set".to_string())
            })?;
            set_skill_config_value(
                &options.home,
                &options.cwd,
                target_from_args(&args)?,
                required_string(&args, "key")?,
                value,
            )
        }
        other => Err(Error::Message(format!(
            "unknown skill_config action: {other}"
        ))),
    }
}

pub(crate) fn target_from_args(args: &Value) -> Result<SkillTarget> {
    match optional_string(args, "scope")?.unwrap_or("global") {
        "global" => Ok(SkillTarget::Global),
        "project" => Ok(SkillTarget::Project),
        other => Err(Error::Message(format!(
            "scope must be global or project, got {other}"
        ))),
    }
}

pub(crate) fn optional_owned(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
