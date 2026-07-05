#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const MAX_NAME_LENGTH: usize = 64;
pub(crate) const MAX_DESCRIPTION_LENGTH: usize = 1024;
pub(crate) const BUNDLES_DIR: &str = "skill-bundles";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub source: SkillSource,
    pub enabled: bool,
    pub disable_model_invocation: bool,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub related: Vec<String>,
    pub platforms: Vec<String>,
    pub required_environment_variables: Vec<RequiredEnvironmentVariable>,
    pub required_credential_files: Vec<String>,
    pub setup_help: Option<String>,
    pub compatibility: Option<String>,
    pub license: Option<String>,
    pub allowed_tools: Vec<String>,
    pub required_tools: Vec<String>,
    pub fallback_for_tools: Vec<String>,
    pub required_toolsets: Vec<String>,
    pub fallback_for_toolsets: Vec<String>,
    pub supported_on_current_platform: bool,
    pub collision_group: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Explicit,
    Project,
    Agents,
    AgentsGlobal,
    Global,
    Config,
    Plugin,
    InstallSource,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Project => "project",
            Self::Agents => "agents",
            Self::AgentsGlobal => "agents_global",
            Self::Global => "global",
            Self::Config => "config",
            Self::Plugin => "plugin",
            Self::InstallSource => "install_source",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Project | Self::Agents => "Project",
            Self::Explicit
            | Self::AgentsGlobal
            | Self::Global
            | Self::Config
            | Self::InstallSource => "User",
            Self::Plugin => "System",
        }
    }
}

pub fn skill_source_display_label(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim).filter(|value| !value.is_empty())? {
        "project" | "agents" | "Project" => Some("Project"),
        "explicit" | "global" | "agents_global" | "config" | "install_source" | "User" => {
            Some("User")
        }
        "plugin" | "system" | "builtin" | "built_in" | "core" | "System" => Some("System"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillDiagnostic {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl SkillDiagnostic {
    pub(crate) fn warning(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "warning".to_string(),
            message: message.into(),
            path,
        }
    }

    pub(crate) fn collision(name: &str, paths: &[PathBuf]) -> Self {
        Self {
            kind: "collision".to_string(),
            message: format!(
                "skill name \"{name}\" collision; name-based activation disabled for {} matching skills ({})",
                paths.len(),
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            path: paths.first().cloned(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillCatalog {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<SkillDiagnostic>,
    pub collisions: BTreeMap<String, Vec<PathBuf>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectedSkill {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillContextFragment {
    pub name: String,
    pub path: PathBuf,
    pub base_dir: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SkillDiscoveryOptions {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub config_path: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub explicit_inputs: Vec<String>,
    pub additional_roots: Vec<PathBuf>,
    pub no_skills: bool,
}

#[derive(Debug, Clone)]
pub struct SkillSettings {
    pub disabled: BTreeSet<String>,
    pub platform_disabled: BTreeMap<String, BTreeSet<String>>,
    pub paths: Vec<PathBuf>,
    pub enable_commands: Option<bool>,
    pub template_vars: bool,
    pub inline_shell: bool,
    pub inline_shell_timeout_secs: u64,
}

impl Default for SkillSettings {
    fn default() -> Self {
        Self {
            disabled: BTreeSet::new(),
            platform_disabled: BTreeMap::new(),
            paths: Vec::new(),
            enable_commands: None,
            template_vars: true,
            inline_shell: false,
            inline_shell_timeout_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    Global,
    Project,
}

impl SkillTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanVerdict {
    Safe,
    Caution,
    Dangerous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanFinding {
    pub category: String,
    pub severity: String,
    pub file: PathBuf,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanResult {
    pub verdict: ScanVerdict,
    pub findings: Vec<ScanFinding>,
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub source: String,
    pub target: SkillTarget,
    pub name: Option<String>,
    pub all: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct SaveSkillBundleOptions {
    pub target: SkillTarget,
    pub name: String,
    pub skills: Vec<String>,
    pub description: Option<String>,
    pub instruction: Option<String>,
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct SkillFrontmatter {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) compatibility: Option<String>,
    pub(crate) metadata: Option<Value>,
    pub(crate) tags: Option<Value>,
    pub(crate) related: Option<Value>,
    #[serde(rename = "related_skills")]
    pub(crate) related_skills: Option<Value>,
    pub(crate) platforms: Option<Value>,
    pub(crate) prerequisites: Option<Value>,
    pub(crate) setup: Option<Value>,
    #[serde(rename = "required_environment_variables")]
    pub(crate) required_environment_variables: Option<Value>,
    #[serde(rename = "required_credential_files")]
    pub(crate) required_credential_files: Option<Value>,
    #[serde(rename = "allowed-tools")]
    pub(crate) allowed_tools_hyphen: Option<Value>,
    #[serde(rename = "allowed_tools")]
    pub(crate) allowed_tools_underscore: Option<Value>,
    #[serde(rename = "disable-model-invocation")]
    pub(crate) disable_model_invocation: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct RawSkillSettings {
    pub(crate) disabled: Option<Vec<String>>,
    pub(crate) platform_disabled: Option<BTreeMap<String, Vec<String>>>,
    pub(crate) paths: Option<Vec<String>>,
    pub(crate) enable_commands: Option<bool>,
    pub(crate) template_vars: Option<bool>,
    pub(crate) inline_shell: Option<bool>,
    pub(crate) inline_shell_timeout: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequiredEnvironmentVariable {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_for: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillBundle {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub skills: Vec<String>,
    pub instruction: Option<String>,
    pub path: PathBuf,
    pub scope: SkillTarget,
}

pub fn load_skill_settings(
    home: &Path,
    cwd: &Path,
    config_path: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> Result<SkillSettings> {
    let mut merged = json!({});
    let env_config_path = env_path("PSYCHEVO_CONFIG", env);
    if let Some(config_path) = config_path.or(env_config_path.as_deref()) {
        deep_merge(&mut merged, load_toml_config_file(config_path, true)?);
    } else {
        deep_merge(
            &mut merged,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
        deep_merge(
            &mut merged,
            load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
        );
    }
    parse_skill_settings(&merged, home, cwd, env)
}

pub fn resolve_skills_home(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf> {
    if let Some(value) = env
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        resolve_configured_path(value, cwd, env)
    } else {
        resolve_configured_path("~/.psychevo", cwd, env)
    }
}

pub(crate) fn parse_skill_settings(
    value: &Value,
    _home: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<SkillSettings> {
    let mut settings = SkillSettings::default();
    if let Some(skills) = value.get("skills") {
        let raw: RawSkillSettings = serde_json::from_value(skills.clone())
            .map_err(|err| Error::Config(format!("skills config failed: {err}")))?;
        for name in raw.disabled.unwrap_or_default() {
            let name = name.trim();
            if !name.is_empty() {
                settings.disabled.insert(name.to_string());
            }
        }
        for (platform, names) in raw.platform_disabled.unwrap_or_default() {
            let disabled = names
                .into_iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect::<BTreeSet<_>>();
            if !disabled.is_empty() {
                settings.platform_disabled.insert(platform, disabled);
            }
        }
        let platform = active_skill_platform(env);
        if let Some(disabled) = settings.platform_disabled.get(platform) {
            settings.disabled.extend(disabled.iter().cloned());
        }
        for raw_path in raw.paths.unwrap_or_default() {
            settings
                .paths
                .push(resolve_configured_path(&raw_path, cwd, env)?);
        }
        settings.enable_commands = raw.enable_commands;
        if let Some(value) = raw.template_vars {
            settings.template_vars = value;
        }
        if let Some(value) = raw.inline_shell {
            settings.inline_shell = value;
        }
        if let Some(value) = raw.inline_shell_timeout {
            settings.inline_shell_timeout_secs = value.max(1);
        }
    }
    Ok(settings)
}

pub(crate) fn active_skill_platform(env: &BTreeMap<String, String>) -> &str {
    env.get("PSYCHEVO_PLATFORM")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("run")
}

pub fn discover_skills(options: &SkillDiscoveryOptions) -> Result<SkillCatalog> {
    let settings = load_skill_settings(
        &options.home,
        &options.cwd,
        options.config_path.as_deref(),
        &options.env,
    )?;
    discover_skills_with_settings(options, &settings)
}

pub fn discover_skills_with_settings(
    options: &SkillDiscoveryOptions,
    settings: &SkillSettings,
) -> Result<SkillCatalog> {
    let explicit_names = options
        .explicit_inputs
        .iter()
        .filter(|input| !looks_like_existing_path(input, &options.cwd, &options.env))
        .map(|input| input.trim().to_string())
        .filter(|input| !input.is_empty())
        .collect::<BTreeSet<_>>();
    let mut catalog = SkillCatalog::default();
    let mut seen = BTreeMap::<String, PathBuf>::new();

    for input in &options.explicit_inputs {
        let Some(path) = existing_input_path(input, &options.cwd, &options.env)? else {
            continue;
        };
        add_path_skills(
            &path,
            SkillSource::Explicit,
            true,
            None,
            &BTreeSet::new(),
            &mut catalog,
            &mut seen,
        )?;
    }

    let should_load_defaults = !options.no_skills || !explicit_names.is_empty();
    if should_load_defaults {
        let only_names = options.no_skills.then_some(&explicit_names);
        let mut sources = Vec::new();
        sources.push((
            options.cwd.join(".psychevo").join("skills"),
            SkillSource::Project,
            true,
        ));
        for dir in ancestor_agents_skill_dirs(&options.cwd) {
            sources.push((dir, SkillSource::Agents, false));
        }
        sources.push((options.home.join("skills"), SkillSource::Global, true));
        if let Some(user_home) = env_path("HOME", &options.env) {
            sources.push((
                user_home.join(".agents").join("skills"),
                SkillSource::AgentsGlobal,
                false,
            ));
        }
        for path in &settings.paths {
            sources.push((path.clone(), SkillSource::Config, true));
        }
        for path in &options.additional_roots {
            sources.push((path.clone(), SkillSource::Plugin, true));
        }
        for (path, source, include_root_files) in dedupe_skill_source_roots(sources) {
            add_path_skills(
                &path,
                source,
                include_root_files,
                only_names,
                &settings.disabled,
                &mut catalog,
                &mut seen,
            )?;
        }
    }

    finalize_skill_catalog(&mut catalog);

    let loaded_names = catalog
        .skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<HashSet<_>>();
    for name in explicit_names {
        if !loaded_names.contains(name.as_str()) {
            catalog.diagnostics.push(SkillDiagnostic::warning(
                format!("explicit skill not found: {name}"),
                None,
            ));
        }
    }

    Ok(catalog)
}

fn dedupe_skill_source_roots(
    sources: Vec<(PathBuf, SkillSource, bool)>,
) -> Vec<(PathBuf, SkillSource, bool)> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::with_capacity(sources.len());
    for (path, source, include_root_files) in sources {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            deduped.push((path, source, include_root_files));
        }
    }
    deduped
}

pub fn format_skills_for_prompt(skills: &[Skill]) -> String {
    let visible = skills
        .iter()
        .filter(|skill| skill_prompt_visible_for_activation(skill))
        .collect::<Vec<_>>();
    format_skill_prompt_entries(&visible)
}

pub fn skills_visible_for_prompt_with_tools<T, I>(
    skills: &[Skill],
    available_tools: I,
) -> Vec<Skill>
where
    T: AsRef<str>,
    I: IntoIterator<Item = T>,
{
    skills_visible_for_prompt_with_tools_and_toolsets(
        skills,
        available_tools,
        std::iter::empty::<&str>(),
    )
}

pub fn skills_visible_for_prompt_with_tools_and_toolsets<T, I, U, J>(
    skills: &[Skill],
    available_tools: I,
    accepted_toolsets: J,
) -> Vec<Skill>
where
    T: AsRef<str>,
    I: IntoIterator<Item = T>,
    U: AsRef<str>,
    J: IntoIterator<Item = U>,
{
    let available_tools = available_tools
        .into_iter()
        .map(|tool| tool.as_ref().to_string())
        .collect::<HashSet<_>>();
    let accepted_toolsets = accepted_toolsets
        .into_iter()
        .map(|toolset| toolset.as_ref().to_string())
        .collect::<HashSet<_>>();
    skills
        .iter()
        .filter(|skill| skill_prompt_visible_for_activation(skill))
        .filter(|skill| skill_prompt_visible_for_tools(skill, &available_tools, &accepted_toolsets))
        .cloned()
        .collect()
}

fn skill_prompt_visible_for_tools(
    skill: &Skill,
    available_tools: &HashSet<String>,
    accepted_toolsets: &HashSet<String>,
) -> bool {
    skill
        .required_tools
        .iter()
        .all(|tool| available_tools.contains(tool))
        && !skill
            .fallback_for_tools
            .iter()
            .any(|tool| available_tools.contains(tool))
        && skill
            .required_toolsets
            .iter()
            .all(|toolset| accepted_toolsets.contains(toolset))
        && !skill
            .fallback_for_toolsets
            .iter()
            .any(|toolset| accepted_toolsets.contains(toolset))
}

fn format_skill_prompt_entries(visible: &[&Skill]) -> String {
    if visible.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        prompt_templates::skill_index_intro().to_string(),
        "<available_skills>".to_string(),
    ];
    for skill in visible {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&truncate_description(&skill.description))
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.file_path.display().to_string())
        ));
        lines.push("  </skill>".to_string());
    }
    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

pub fn list_skills_value(catalog: &SkillCatalog, include_hidden: bool) -> Value {
    list_skills_value_with_options(
        catalog,
        &ListSkillsOptions {
            include_hidden,
            ..ListSkillsOptions::default()
        },
    )
}

#[derive(Debug, Clone, Default)]
pub struct ListSkillsOptions {
    pub include_hidden: bool,
    pub detail: bool,
    pub category: Option<String>,
    pub source: Option<String>,
    pub enabled_only: bool,
    pub platform: Option<String>,
    pub tag: Option<String>,
    pub readiness: Option<String>,
    pub sort: Option<String>,
}

pub fn list_skills_value_with_options(
    catalog: &SkillCatalog,
    options: &ListSkillsOptions,
) -> Value {
    let skills = catalog
        .skills
        .iter()
        .filter(|skill| options.include_hidden || skill_prompt_visible_for_activation(skill))
        .filter(|skill| {
            options
                .category
                .as_ref()
                .is_none_or(|category| skill.category.as_deref() == Some(category.as_str()))
        })
        .filter(|skill| {
            options
                .source
                .as_ref()
                .is_none_or(|source| skill.source.as_str() == source)
        })
        .filter(|skill| !options.enabled_only || skill_prompt_visible_for_activation(skill))
        .filter(|skill| {
            options.platform.as_ref().is_none_or(|platform| {
                platform.eq_ignore_ascii_case("all")
                    || skill
                        .platforms
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(platform))
                    || skill.platforms.is_empty() && platform.eq_ignore_ascii_case("current")
            })
        })
        .filter(|skill| {
            options
                .tag
                .as_ref()
                .is_none_or(|tag| skill.tags.iter().any(|candidate| candidate == tag))
        })
        .filter(|skill| {
            options
                .readiness
                .as_ref()
                .is_none_or(|readiness| skill_readiness_status(skill) == readiness.as_str())
        })
        .map(|skill| {
            let mut value = json!({
                "id": skill_catalog_id(skill),
                "name": skill.name,
                "description": skill.description,
                "location": skill.file_path,
                "source": skill.source.as_str(),
                "source_label": skill.source.display_label(),
                "category": skill.category,
                "enabled": skill.enabled,
                "prompt_visible": skill_prompt_visible_for_activation(skill),
                "readiness_status": skill_readiness_status(skill),
                "supported_on_current_platform": skill.supported_on_current_platform,
                "disable_model_invocation": skill.disable_model_invocation,
                "issues": skill_issues(skill),
            });
            if !skill.collision_group.is_empty()
                && let Some(object) = value.as_object_mut()
            {
                object.insert("collision_group".to_string(), json!(skill.collision_group));
            }
            if options.detail
                && let Some(object) = value.as_object_mut()
            {
                object.insert("skill_dir".to_string(), json!(skill.base_dir));
                object.insert("tags".to_string(), json!(skill.tags));
                object.insert("related_skills".to_string(), json!(skill.related));
                object.insert("platforms".to_string(), json!(skill.platforms));
                object.insert(
                    "required_environment_variables".to_string(),
                    json!(skill.required_environment_variables),
                );
                object.insert(
                    "missing_required_environment_variables".to_string(),
                    json!(missing_required_env_names(skill, None)),
                );
                object.insert(
                    "missing_credential_files".to_string(),
                    json!(missing_credential_files(skill)),
                );
                object.insert("compatibility".to_string(), json!(skill.compatibility));
                object.insert("license".to_string(), json!(skill.license));
                object.insert("allowed_tools".to_string(), json!(skill.allowed_tools));
                object.insert("required_tools".to_string(), json!(skill.required_tools));
                object.insert(
                    "fallback_for_tools".to_string(),
                    json!(skill.fallback_for_tools),
                );
                object.insert(
                    "required_toolsets".to_string(),
                    json!(skill.required_toolsets),
                );
                object.insert(
                    "fallback_for_toolsets".to_string(),
                    json!(skill.fallback_for_toolsets),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    json!({
        "success": true,
        "skills": skills,
        "diagnostics": catalog.diagnostics,
        "collisions": catalog.collisions,
        "count": skills.len(),
    })
}

pub fn view_skill_value(
    catalog: &SkillCatalog,
    name: &str,
    file_path: Option<&str>,
) -> Result<Value> {
    view_skill_value_selected(catalog, name, None, file_path)
}

pub fn view_skill_value_selected(
    catalog: &SkillCatalog,
    name: &str,
    selector_path: Option<&Path>,
    file_path: Option<&str>,
) -> Result<Value> {
    let skill = match selector_path {
        Some(path) => find_skill_by_path(catalog, name, path)?,
        None => find_skill(catalog, name)?,
    };
    if !skill.supported_on_current_platform && file_path.is_none() {
        return Ok(json!({
            "success": false,
            "name": skill.name,
            "error": format!("skill '{}' is not supported on this platform", skill.name),
            "source": skill.source.as_str(),
            "source_label": skill.source.display_label(),
            "readiness_status": "unsupported",
            "platforms": skill.platforms,
        }));
    }
    let target = match file_path {
        Some(file_path) => match resolve_skill_relative_path(skill, file_path) {
            Ok(path) => path,
            Err(err) => {
                if err.to_string().contains("No such file") || err.to_string().contains("not found")
                {
                    return Ok(json!({
                        "success": false,
                        "name": skill.name,
                        "file": file_path,
                        "error": format!("skill file not found: {file_path}"),
                        "available_files": available_files(&skill.base_dir),
                    }));
                }
                return Err(err);
            }
        },
        None => skill.file_path.clone(),
    };
    let bytes = fs::read(&target)?;
    let is_binary = bytes.contains(&0);
    if is_binary {
        let content = format!(
            "[Binary file: {}, size: {} bytes]",
            target.display(),
            bytes.len()
        );
        return Ok(json!({
            "success": true,
            "name": skill.name,
            "file": file_path,
            "is_binary": true,
            "size": bytes.len(),
            "content": content.clone(),
            "preview_content": content,
        }));
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let content = format!(
                "[Non-UTF-8 file: {}, size: {} bytes]",
                target.display(),
                err.as_bytes().len()
            );
            return Ok(json!({
                "success": true,
                "name": skill.name,
                "file": file_path,
                "path": target,
                "is_binary": true,
                "size": err.as_bytes().len(),
                "content": content.clone(),
                "preview_content": content,
            }));
        }
    };
    let preview_content = text.clone();
    let content = if file_path.is_none() {
        preprocess_skill_content(strip_frontmatter(&text).trim(), &skill.base_dir, None, None)
    } else {
        text
    };
    Ok(json!({
        "success": true,
        "name": skill.name,
        "id": skill_catalog_id(skill),
        "enabled": skill.enabled,
        "prompt_visible": skill_prompt_visible_for_activation(skill),
        "file": file_path,
        "path": target,
        "skill_dir": skill.base_dir,
        "description": skill.description,
        "source": skill.source.as_str(),
        "source_label": skill.source.display_label(),
        "category": skill.category,
        "tags": skill.tags,
        "related_skills": skill.related,
        "platforms": skill.platforms,
        "platform_status": if skill.supported_on_current_platform { "supported" } else { "unsupported" },
        "issues": skill_issues(skill),
        "collision_group": skill.collision_group,
        "required_environment_variables": skill.required_environment_variables,
        "missing_required_environment_variables": missing_required_env_names(skill, None),
        "missing_credential_files": missing_credential_files(skill),
        "setup_needed": skill_readiness_status(skill) == "setup_needed",
        "readiness_status": skill_readiness_status(skill),
        "setup_help": skill.setup_help,
        "compatibility": skill.compatibility,
        "license": skill.license,
        "allowed_tools": skill.allowed_tools,
        "required_tools": skill.required_tools,
        "fallback_for_tools": skill.fallback_for_tools,
        "required_toolsets": skill.required_toolsets,
        "fallback_for_toolsets": skill.fallback_for_toolsets,
        "content": content,
        "preview_content": preview_content,
        "linked_files": linked_files(&skill.base_dir),
    }))
}

pub(crate) fn skill_catalog_id(skill: &Skill) -> String {
    skill
        .file_path
        .canonicalize()
        .unwrap_or_else(|_| skill.file_path.clone())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn skill_issues(skill: &Skill) -> Vec<String> {
    let mut issues = Vec::new();
    if !skill.enabled {
        issues.push("disabled".to_string());
    }
    if skill.disable_model_invocation {
        issues.push("hidden_from_model".to_string());
    }
    if !skill.supported_on_current_platform {
        issues.push("unsupported_on_current_platform".to_string());
    }
    let missing_env = missing_required_env_names(skill, None);
    if !missing_env.is_empty() {
        issues.push(format!(
            "missing environment variables: {}",
            missing_env.join(", ")
        ));
    }
    let missing_credentials = missing_credential_files(skill);
    if !missing_credentials.is_empty() {
        issues.push(format!(
            "missing credential files: {}",
            missing_credentials.join(", ")
        ));
    }
    if !skill.collision_group.is_empty() {
        issues.push(format!(
            "ambiguous skill name across {} paths",
            skill.collision_group.len()
        ));
    }
    issues
}

pub(crate) fn skill_readiness_status(skill: &Skill) -> &'static str {
    if !skill.supported_on_current_platform {
        "unsupported"
    } else if !missing_required_env_names(skill, None).is_empty()
        || !missing_credential_files(skill).is_empty()
    {
        "setup_needed"
    } else {
        "available"
    }
}

pub(crate) fn missing_required_env_names(
    skill: &Skill,
    env_snapshot: Option<&BTreeMap<String, String>>,
) -> Vec<String> {
    let process_env;
    let env = if let Some(env) = env_snapshot {
        env
    } else {
        process_env = load_home_env_from_skill(skill);
        &process_env
    };
    skill
        .required_environment_variables
        .iter()
        .filter(|entry| !entry.optional)
        .filter(|entry| {
            env.get(&entry.name)
                .map(|value| value.trim().is_empty())
                .unwrap_or_else(|| {
                    std::env::var(&entry.name).map_or(true, |value| value.is_empty())
                })
        })
        .map(|entry| entry.name.clone())
        .collect()
}

pub(crate) fn load_home_env_from_skill(skill: &Skill) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current = Some(skill.base_dir.as_path());
    while let Some(path) = current {
        let env_path = path.join(".env");
        if env_path.exists() {
            load_env_file(&env_path, &mut out);
            break;
        }
        current = path.parent();
    }
    out
}

pub(crate) fn load_env_file(path: &Path, out: &mut BTreeMap<String, String>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if valid_env_name(key) {
                out.insert(
                    key.to_string(),
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string(),
                );
            }
        }
    }
}

pub(crate) fn missing_credential_files(skill: &Skill) -> Vec<String> {
    skill
        .required_credential_files
        .iter()
        .filter(|path| !Path::new(path).exists())
        .cloned()
        .collect()
}

pub fn expand_skill_prompt(catalog: &SkillCatalog, name: &str, args: &str) -> Result<String> {
    let skill = find_skill(catalog, name)?;
    let text = fs::read_to_string(&skill.file_path)?;
    let body = strip_frontmatter(&text).trim();
    let mut block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        escape_xml(&skill.name),
        escape_xml(&skill.file_path.display().to_string()),
        skill.base_dir.display(),
        body
    );
    if !args.trim().is_empty() {
        block.push_str("\n\n");
        block.push_str(args.trim());
    }
    Ok(block)
}

pub fn select_skills_for_prompt(catalog: &SkillCatalog, prompt: &str) -> Vec<SelectedSkill> {
    let mentions = skill_mentions(prompt);
    select_skills(catalog, mentions.iter().map(String::as_str), false)
}

pub fn select_explicit_skills(
    catalog: &SkillCatalog,
    explicit_inputs: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Vec<SelectedSkill> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for input in explicit_inputs {
        if let Ok(Some(path)) = existing_input_path(input, cwd, env) {
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            for skill in &catalog.skills {
                let Ok(skill_path) = skill.file_path.canonicalize() else {
                    continue;
                };
                let Ok(base_dir) = skill.base_dir.canonicalize() else {
                    continue;
                };
                if explicit_path_selects_skill(&path, &skill_path, &base_dir)
                    && seen.insert(skill.file_path.clone())
                {
                    selected.push(selected_skill(skill));
                }
            }
        } else {
            selected.extend(select_skills(
                catalog,
                std::iter::once(input.trim()).filter(|value| !value.is_empty()),
                true,
            ));
        }
    }
    selected
}

pub fn skill_context_messages(
    skills: &[SelectedSkill],
    catalog: &SkillCatalog,
) -> Result<Vec<String>> {
    Ok(skill_context_fragments(skills, catalog)?
        .into_iter()
        .map(|fragment| fragment.content)
        .collect())
}
