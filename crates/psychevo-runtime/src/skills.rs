use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{CONFIG_FILE_NAME, load_toml_config_file, write_toml_config_file};
use crate::error::{Error, Result};
use crate::prompt_templates;

const MAX_NAME_LENGTH: usize = 64;
const MAX_DESCRIPTION_LENGTH: usize = 1024;
const BUNDLES_DIR: &str = "skill-bundles";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub source: SkillSource,
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
    pub supported_on_current_platform: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Explicit,
    Project,
    Agents,
    Global,
    Config,
    InstallSource,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Project => "project",
            Self::Agents => "agents",
            Self::Global => "global",
            Self::Config => "config",
            Self::InstallSource => "install_source",
        }
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
    fn warning(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "warning".to_string(),
            message: message.into(),
            path,
        }
    }

    fn collision(name: &str, winner: &Path, loser: &Path) -> Self {
        Self {
            kind: "collision".to_string(),
            message: format!(
                "skill name \"{name}\" collision; keeping {} and omitting {}",
                winner.display(),
                loser.display()
            ),
            path: Some(loser.to_path_buf()),
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
    pub workdir: PathBuf,
    pub config_path: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub explicit_inputs: Vec<String>,
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
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    metadata: Option<Value>,
    tags: Option<Value>,
    related: Option<Value>,
    #[serde(rename = "related_skills")]
    related_skills: Option<Value>,
    platforms: Option<Value>,
    prerequisites: Option<Value>,
    setup: Option<Value>,
    #[serde(rename = "required_environment_variables")]
    required_environment_variables: Option<Value>,
    #[serde(rename = "required_credential_files")]
    required_credential_files: Option<Value>,
    #[serde(rename = "allowed-tools")]
    allowed_tools_hyphen: Option<Value>,
    #[serde(rename = "allowed_tools")]
    allowed_tools_underscore: Option<Value>,
    #[serde(rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSkillSettings {
    disabled: Option<Vec<String>>,
    platform_disabled: Option<BTreeMap<String, Vec<String>>>,
    paths: Option<Vec<String>>,
    enable_commands: Option<bool>,
    template_vars: Option<bool>,
    inline_shell: Option<bool>,
    inline_shell_timeout: Option<u64>,
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
    workdir: &Path,
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
            load_toml_config_file(&workdir.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
        );
    }
    parse_skill_settings(&merged, home, workdir, env)
}

pub fn resolve_skills_home(env: &BTreeMap<String, String>, workdir: &Path) -> Result<PathBuf> {
    if let Some(value) = env
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        resolve_configured_path(value, workdir, env)
    } else {
        resolve_configured_path("~/.psychevo", workdir, env)
    }
}

fn parse_skill_settings(
    value: &Value,
    _home: &Path,
    workdir: &Path,
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
                .push(resolve_configured_path(&raw_path, workdir, env)?);
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

fn active_skill_platform(env: &BTreeMap<String, String>) -> &str {
    env.get("PSYCHEVO_PLATFORM")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("run")
}

pub fn discover_skills(options: &SkillDiscoveryOptions) -> Result<SkillCatalog> {
    let settings = load_skill_settings(
        &options.home,
        &options.workdir,
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
        .filter(|input| !looks_like_existing_path(input, &options.workdir, &options.env))
        .map(|input| input.trim().to_string())
        .filter(|input| !input.is_empty())
        .collect::<BTreeSet<_>>();
    let mut catalog = SkillCatalog::default();
    let mut seen = BTreeMap::<String, PathBuf>::new();

    for input in &options.explicit_inputs {
        let Some(path) = existing_input_path(input, &options.workdir, &options.env)? else {
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
            options.workdir.join(".psychevo").join("skills"),
            SkillSource::Project,
            true,
        ));
        for dir in ancestor_agents_skill_dirs(&options.workdir) {
            sources.push((dir, SkillSource::Agents, false));
        }
        sources.push((options.home.join("skills"), SkillSource::Global, true));
        for path in &settings.paths {
            sources.push((path.clone(), SkillSource::Config, true));
        }
        for (path, source, include_root_files) in sources {
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

pub fn format_skills_for_prompt(skills: &[Skill]) -> String {
    let visible = skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation && skill.supported_on_current_platform)
        .collect::<Vec<_>>();
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
        .filter(|skill| options.include_hidden || !skill.disable_model_invocation)
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
        .filter(|skill| {
            !options.enabled_only
                || (!skill.disable_model_invocation && skill.supported_on_current_platform)
        })
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
                "name": skill.name,
                "description": skill.description,
                "location": skill.file_path,
                "source": skill.source.as_str(),
                "category": skill.category,
                "readiness_status": skill_readiness_status(skill),
                "disable_model_invocation": skill.disable_model_invocation,
            });
            if options.detail
                && let Some(object) = value.as_object_mut()
            {
                object.insert("skill_dir".to_string(), json!(skill.base_dir));
                object.insert("tags".to_string(), json!(skill.tags));
                object.insert("related_skills".to_string(), json!(skill.related));
                object.insert("platforms".to_string(), json!(skill.platforms));
                object.insert(
                    "supported_on_current_platform".to_string(),
                    json!(skill.supported_on_current_platform),
                );
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
    let skill = find_skill(catalog, name)?;
    if !skill.supported_on_current_platform && file_path.is_none() {
        return Ok(json!({
            "success": false,
            "name": skill.name,
            "error": format!("skill '{}' is not supported on this platform", skill.name),
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
        return Ok(json!({
            "success": true,
            "name": skill.name,
            "file": file_path,
            "is_binary": true,
            "size": bytes.len(),
            "content": format!("[Binary file: {}, size: {} bytes]", target.display(), bytes.len()),
        }));
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            return Ok(json!({
                "success": true,
                "name": skill.name,
                "file": file_path,
                "path": target,
                "is_binary": true,
                "size": err.as_bytes().len(),
                "content": format!("[Non-UTF-8 file: {}, size: {} bytes]", target.display(), err.as_bytes().len()),
            }));
        }
    };
    let content = if file_path.is_none() {
        preprocess_skill_content(strip_frontmatter(&text).trim(), &skill.base_dir, None, None)
    } else {
        text
    };
    Ok(json!({
        "success": true,
        "name": skill.name,
        "file": file_path,
        "path": target,
        "skill_dir": skill.base_dir,
        "description": skill.description,
        "source": skill.source.as_str(),
        "category": skill.category,
        "tags": skill.tags,
        "related_skills": skill.related,
        "platforms": skill.platforms,
        "platform_status": if skill.supported_on_current_platform { "supported" } else { "unsupported" },
        "required_environment_variables": skill.required_environment_variables,
        "missing_required_environment_variables": missing_required_env_names(skill, None),
        "missing_credential_files": missing_credential_files(skill),
        "setup_needed": skill_readiness_status(skill) == "setup_needed",
        "readiness_status": skill_readiness_status(skill),
        "setup_help": skill.setup_help,
        "compatibility": skill.compatibility,
        "license": skill.license,
        "allowed_tools": skill.allowed_tools,
        "content": content,
        "linked_files": linked_files(&skill.base_dir),
    }))
}

fn skill_readiness_status(skill: &Skill) -> &'static str {
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

fn missing_required_env_names(
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

fn load_home_env_from_skill(skill: &Skill) -> BTreeMap<String, String> {
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

fn load_env_file(path: &Path, out: &mut BTreeMap<String, String>) {
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

fn missing_credential_files(skill: &Skill) -> Vec<String> {
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
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Vec<SelectedSkill> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for input in explicit_inputs {
        if let Ok(Some(path)) = existing_input_path(input, workdir, env) {
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

pub fn skill_context_fragments(
    skills: &[SelectedSkill],
    catalog: &SkillCatalog,
) -> Result<Vec<SkillContextFragment>> {
    let mut fragments = Vec::new();
    for selected in skills {
        let Some(skill) = catalog
            .skills
            .iter()
            .find(|skill| skill.name == selected.name && skill.file_path == selected.path)
        else {
            continue;
        };
        let raw = fs::read_to_string(&skill.file_path)?;
        let body = strip_frontmatter(&raw).trim();
        let content = format!(
            "<skill>\n<name>{}</name>\n<path>{}</path>\n<skill_dir>{}</skill_dir>\n{}\n</skill>",
            escape_xml(&skill.name),
            escape_xml(&skill.file_path.display().to_string()),
            escape_xml(&skill.base_dir.display().to_string()),
            body
        );
        fragments.push(SkillContextFragment {
            name: skill.name.clone(),
            path: skill.file_path.clone(),
            base_dir: skill.base_dir.clone(),
            content,
        });
    }
    Ok(fragments)
}

pub fn create_skill(
    home: &Path,
    workdir: &Path,
    target: SkillTarget,
    name: &str,
    description: &str,
) -> Result<Value> {
    if let Some(err) = validate_name(name, name).into_iter().next() {
        return Err(Error::Message(err));
    }
    if description.trim().is_empty() {
        return Err(Error::Message("description is required".to_string()));
    }
    let root = target_skills_dir(home, workdir, target);
    let dir = root.join(name);
    let file = dir.join("SKILL.md");
    if file.exists() {
        return Err(Error::Message(format!("skill already exists: {name}")));
    }
    fs::create_dir_all(&dir)?;
    fs::write(
        &file,
        format!(
            "---\nname: {name}\ndescription: {}\n---\n\n# {name}\n\n",
            yaml_scalar(description)
        ),
    )?;
    Ok(json!({"success": true, "name": name, "path": file}))
}

pub fn patch_skill(
    catalog: &SkillCatalog,
    home: &Path,
    workdir: &Path,
    name: &str,
    old: &str,
    new: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, workdir)?;
    if old == new {
        return Err(Error::Message("old and new text are identical".to_string()));
    }
    let text = fs::read_to_string(&skill.file_path)?;
    let matches = text.match_indices(old).collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(Error::Message("old text not found".to_string()));
    }
    if matches.len() > 1 {
        return Err(Error::Message("old text is ambiguous".to_string()));
    }
    let updated = text.replacen(old, new, 1);
    fs::write(&skill.file_path, updated)?;
    Ok(json!({"success": true, "name": skill.name, "path": skill.file_path}))
}

pub fn edit_skill(
    catalog: &SkillCatalog,
    home: &Path,
    workdir: &Path,
    name: &str,
    content: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, workdir)?;
    fs::write(&skill.file_path, content)?;
    Ok(json!({"success": true, "name": skill.name, "path": skill.file_path}))
}

pub fn write_skill_file(
    catalog: &SkillCatalog,
    home: &Path,
    workdir: &Path,
    name: &str,
    file_path: &str,
    content: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, workdir)?;
    let target = resolve_skill_write_path(skill, file_path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, content)?;
    Ok(json!({"success": true, "name": skill.name, "path": target}))
}

pub fn remove_skill_file(
    catalog: &SkillCatalog,
    home: &Path,
    workdir: &Path,
    name: &str,
    file_path: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, workdir)?;
    let target = resolve_skill_relative_path(skill, file_path)?;
    if target == skill.file_path {
        return Err(Error::Message(
            "remove_file cannot remove SKILL.md; use delete".to_string(),
        ));
    }
    fs::remove_file(&target)?;
    Ok(json!({"success": true, "name": skill.name, "path": target}))
}

pub fn remove_skill(
    catalog: &SkillCatalog,
    home: &Path,
    workdir: &Path,
    name: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, workdir)?;
    if skill.file_path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
        fs::remove_dir_all(&skill.base_dir)?;
    } else {
        fs::remove_file(&skill.file_path)?;
    }
    Ok(json!({"success": true, "name": skill.name}))
}

pub fn set_skill_enabled(
    home: &Path,
    workdir: &Path,
    target: SkillTarget,
    name: &str,
    enabled: bool,
) -> Result<Value> {
    let config_path = match target {
        SkillTarget::Global => home.join(CONFIG_FILE_NAME),
        SkillTarget::Project => workdir.join(".psychevo").join(CONFIG_FILE_NAME),
    };
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut value = load_toml_config_file(&config_path, false)?;
    ensure_object(&mut value);
    let disabled = skills_disabled_array_mut(&mut value)?;
    if enabled {
        disabled.retain(|value| value.as_str() != Some(name));
    } else if !disabled.iter().any(|value| value.as_str() == Some(name)) {
        disabled.push(Value::String(name.to_string()));
    }
    disabled.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    write_toml_config_file(&config_path, &value)?;
    Ok(
        json!({"success": true, "name": name, "enabled": enabled, "scope": target.as_str(), "path": config_path}),
    )
}

pub fn set_skill_config_value(
    home: &Path,
    workdir: &Path,
    target: SkillTarget,
    key: &str,
    new_value: Value,
) -> Result<Value> {
    let suffix = key.strip_prefix("skills.config.").ok_or_else(|| {
        Error::Message("skill config writes must use a skills.config.* key".to_string())
    })?;
    let segments = suffix
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| !valid_config_key_segment(segment))
    {
        return Err(Error::Message(
            "skills.config.* key contains an invalid segment".to_string(),
        ));
    }

    let config_path = match target {
        SkillTarget::Global => home.join(CONFIG_FILE_NAME),
        SkillTarget::Project => workdir.join(".psychevo").join(CONFIG_FILE_NAME),
    };
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut value = load_toml_config_file(&config_path, false)?;
    ensure_object(&mut value);
    set_nested_skill_config_value(&mut value, &segments, new_value)?;
    write_toml_config_file(&config_path, &value)?;
    Ok(json!({"success": true, "key": key, "scope": target.as_str(), "path": config_path}))
}

fn ensure_object(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
}

fn valid_config_key_segment(segment: &str) -> bool {
    segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn set_nested_skill_config_value(
    value: &mut Value,
    segments: &[&str],
    new_value: Value,
) -> Result<()> {
    ensure_object(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Message("config root is not an object".to_string()))?;
    let skills = root
        .entry("skills".to_string())
        .or_insert_with(|| json!({}));
    ensure_object(skills);
    let skills = skills
        .as_object_mut()
        .ok_or_else(|| Error::Message("skills config is not an object".to_string()))?;
    let config = skills
        .entry("config".to_string())
        .or_insert_with(|| json!({}));
    ensure_object(config);

    let mut current = config;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        ensure_object(current);
        let object = current
            .as_object_mut()
            .ok_or_else(|| Error::Message("skills.config path is not an object".to_string()))?;
        current = object
            .entry((*segment).to_string())
            .or_insert_with(|| json!({}));
    }
    ensure_object(current);
    let object = current
        .as_object_mut()
        .ok_or_else(|| Error::Message("skills.config path is not an object".to_string()))?;
    object.insert(segments[segments.len() - 1].to_string(), new_value);
    Ok(())
}

fn skills_disabled_array_mut(value: &mut Value) -> Result<&mut Vec<Value>> {
    ensure_object(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Message("config root is not an object".to_string()))?;
    let skills = root
        .entry("skills".to_string())
        .or_insert_with(|| json!({}));
    ensure_object(skills);
    let skills = skills
        .as_object_mut()
        .ok_or_else(|| Error::Message("skills config is not an object".to_string()))?;
    let disabled = skills
        .entry("disabled".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !disabled.is_array() {
        *disabled = Value::Array(Vec::new());
    }
    disabled
        .as_array_mut()
        .ok_or_else(|| Error::Message("skills.disabled is not an array".to_string()))
}

pub fn scan_skill_path(path: &Path) -> Result<ScanResult> {
    let mut findings = Vec::new();
    for file in text_files_under(path)? {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        scan_text(&text, &file, &mut findings);
    }
    let verdict = if findings
        .iter()
        .any(|finding| finding.severity == "critical" || finding.severity == "high")
    {
        ScanVerdict::Dangerous
    } else if findings.is_empty() {
        ScanVerdict::Safe
    } else {
        ScanVerdict::Caution
    };
    Ok(ScanResult { verdict, findings })
}

pub fn install_skill(home: &Path, workdir: &Path, options: InstallOptions) -> Result<Value> {
    let source_path = match existing_input_path(&options.source, workdir, &BTreeMap::new())? {
        Some(path) => path,
        None => clone_git_source(&options.source)?,
    };
    let mut catalog = SkillCatalog::default();
    let mut seen = BTreeMap::new();
    add_path_skills(
        &source_path,
        SkillSource::InstallSource,
        true,
        None,
        &BTreeSet::new(),
        &mut catalog,
        &mut seen,
    )?;
    if catalog.skills.is_empty() {
        return Err(Error::Message("no installable skills found".to_string()));
    }
    let selected = if let Some(name) = &options.name {
        catalog
            .skills
            .iter()
            .filter(|skill| skill.name == *name)
            .cloned()
            .collect::<Vec<_>>()
    } else if options.all {
        catalog.skills.clone()
    } else if catalog.skills.len() == 1 {
        vec![catalog.skills[0].clone()]
    } else {
        return Err(Error::Message(
            "multiple skills found; pass --all or --name <name>".to_string(),
        ));
    };
    if selected.is_empty() {
        return Err(Error::Message("requested skill not found".to_string()));
    }

    let target_root = target_skills_dir(home, workdir, options.target);
    fs::create_dir_all(&target_root)?;
    let mut installed = Vec::new();
    for skill in selected {
        let scan = scan_skill_path(&skill.base_dir)?;
        if scan.verdict == ScanVerdict::Dangerous && !options.force {
            return Err(Error::Message(format!(
                "skill {} blocked by dangerous scanner verdict",
                skill.name
            )));
        }
        let target =
            if skill.file_path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
                let target = target_root.join(&skill.name);
                if target.exists() {
                    return Err(Error::Message(format!(
                        "skill already exists: {}",
                        skill.name
                    )));
                }
                copy_dir_recursive(&skill.base_dir, &target)?;
                target.join("SKILL.md")
            } else {
                let target = target_root.join(format!("{}.md", skill.name));
                if target.exists() {
                    return Err(Error::Message(format!(
                        "skill already exists: {}",
                        skill.name
                    )));
                }
                fs::copy(&skill.file_path, &target)?;
                target
            };
        installed.push(json!({
            "name": skill.name,
            "path": target,
            "scan": scan,
        }));
    }
    Ok(json!({"success": true, "installed": installed}))
}

pub fn target_skills_dir(home: &Path, workdir: &Path, target: SkillTarget) -> PathBuf {
    match target {
        SkillTarget::Global => home.join("skills"),
        SkillTarget::Project => workdir.join(".psychevo").join("skills"),
    }
}

pub fn list_skill_bundles(home: &Path, workdir: &Path) -> Result<Vec<SkillBundle>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (scope, root) in [
        (SkillTarget::Project, project_bundles_dir(workdir)),
        (SkillTarget::Global, home.join(BUNDLES_DIR)),
    ] {
        if !root.is_dir() {
            continue;
        }
        let mut entries = fs::read_dir(&root)?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let ext = path.extension().and_then(|value| value.to_str());
            if !matches!(ext, Some("toml")) {
                continue;
            }
            if let Some(bundle) = load_bundle_file(&path, scope)?
                && seen.insert(bundle.slug.clone())
            {
                out.push(bundle);
            }
        }
    }
    out.sort_by(|left, right| left.slug.cmp(&right.slug));
    Ok(out)
}

pub fn save_skill_bundle(
    home: &Path,
    workdir: &Path,
    options: SaveSkillBundleOptions,
) -> Result<Value> {
    let slug = slugify(&options.name);
    if slug.is_empty() {
        return Err(Error::Message(
            "bundle name normalizes to empty".to_string(),
        ));
    }
    let skills = options
        .skills
        .into_iter()
        .map(|skill| skill.trim().to_string())
        .filter(|skill| !skill.is_empty())
        .collect::<Vec<_>>();
    if skills.is_empty() {
        return Err(Error::Message(
            "bundle must reference at least one skill".to_string(),
        ));
    }
    let root = bundles_dir(home, workdir, options.target);
    fs::create_dir_all(&root)?;
    let path = root.join(format!("{slug}.toml"));
    if path.exists() && !options.overwrite {
        return Err(Error::Message(format!(
            "bundle already exists: {}",
            path.display()
        )));
    }
    let mut object = serde_json::Map::new();
    object.insert("name".to_string(), Value::String(options.name.clone()));
    if let Some(description) = options.description.filter(|value| !value.trim().is_empty()) {
        object.insert("description".to_string(), Value::String(description));
    }
    object.insert(
        "skills".to_string(),
        Value::Array(
            skills
                .iter()
                .map(|skill| Value::String(skill.clone()))
                .collect(),
        ),
    );
    if let Some(instruction) = options.instruction.filter(|value| !value.trim().is_empty()) {
        object.insert("instruction".to_string(), Value::String(instruction));
    }
    write_toml_config_file(&path, &Value::Object(object))?;
    Ok(
        json!({"success": true, "name": options.name, "slug": slug, "scope": options.target.as_str(), "path": path}),
    )
}

pub fn delete_skill_bundle(
    home: &Path,
    workdir: &Path,
    target: SkillTarget,
    name: &str,
) -> Result<Value> {
    let slug = slugify(name);
    let path = bundles_dir(home, workdir, target).join(format!("{slug}.toml"));
    if !path.exists() {
        return Err(Error::Message(format!("bundle not found: {name}")));
    }
    fs::remove_file(&path)?;
    Ok(json!({"success": true, "name": name, "scope": target.as_str(), "path": path}))
}

fn project_bundles_dir(workdir: &Path) -> PathBuf {
    workdir.join(".psychevo").join(BUNDLES_DIR)
}

fn bundles_dir(home: &Path, workdir: &Path, target: SkillTarget) -> PathBuf {
    match target {
        SkillTarget::Global => home.join(BUNDLES_DIR),
        SkillTarget::Project => project_bundles_dir(workdir),
    }
}

fn load_bundle_file(path: &Path, scope: SkillTarget) -> Result<Option<SkillBundle>> {
    let raw = fs::read_to_string(path)?;
    let parsed: toml::Value =
        toml::from_str(&raw).map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    let value = serde_json::to_value(parsed)?;
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "bundle".to_string());
    let skills = object
        .get("skills")
        .map(string_values_from_value)
        .unwrap_or_default();
    if skills.is_empty() {
        return Ok(None);
    }
    let slug = slugify(&name);
    if slug.is_empty() {
        return Ok(None);
    }
    Ok(Some(SkillBundle {
        description: object
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("Load {} skills as a bundle", skills.len())),
        instruction: object
            .get("instruction")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        name,
        slug,
        skills,
        path: path.to_path_buf(),
        scope,
    }))
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch)
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            Some('-')
        } else {
            None
        };
        let Some(ch) = next else {
            continue;
        };
        if ch == '-' {
            if !last_dash && !out.is_empty() {
                out.push(ch);
                last_dash = true;
            }
        } else {
            out.push(ch);
            last_dash = false;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn add_path_skills(
    path: &Path,
    source: SkillSource,
    include_root_files: bool,
    only_names: Option<&BTreeSet<String>>,
    disabled: &BTreeSet<String>,
    catalog: &mut SkillCatalog,
    seen: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("md")
            && let Some(skill) = load_skill_file(path, source, catalog)?
        {
            add_skill(skill, only_names, disabled, catalog, seen);
        }
        return Ok(());
    }
    load_skills_from_dir(
        path,
        source,
        include_root_files,
        only_names,
        disabled,
        catalog,
        seen,
    )
}

fn load_skills_from_dir(
    dir: &Path,
    source: SkillSource,
    include_root_files: bool,
    only_names: Option<&BTreeSet<String>>,
    disabled: &BTreeSet<String>,
    catalog: &mut SkillCatalog,
    seen: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        if let Some(skill) = load_skill_file(&skill_md, source, catalog)? {
            add_skill(skill, only_names, disabled, catalog, seen);
        }
        return Ok(());
    }
    let mut entries = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            load_skills_from_dir(&path, source, false, only_names, disabled, catalog, seen)?;
        } else if include_root_files
            && path.extension().and_then(|value| value.to_str()) == Some("md")
            && let Some(skill) = load_skill_file(&path, source, catalog)?
        {
            add_skill(skill, only_names, disabled, catalog, seen);
        }
    }
    Ok(())
}

fn load_skill_file(
    file: &Path,
    source: SkillSource,
    catalog: &mut SkillCatalog,
) -> Result<Option<Skill>> {
    let raw = match fs::read_to_string(file) {
        Ok(raw) => raw,
        Err(err) => {
            catalog.diagnostics.push(SkillDiagnostic::warning(
                format!("failed to read skill: {err}"),
                Some(file.to_path_buf()),
            ));
            return Ok(None);
        }
    };
    let (frontmatter, _body) = match parse_frontmatter(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            catalog.diagnostics.push(SkillDiagnostic::warning(
                format!("failed to parse skill frontmatter: {err}"),
                Some(file.to_path_buf()),
            ));
            return Ok(None);
        }
    };
    let base_dir = file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let fallback = if file.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
        base_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string()
    } else {
        file.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string()
    };
    let name = frontmatter.name.unwrap_or(fallback.clone());
    for warning in validate_name(&name, &fallback) {
        catalog
            .diagnostics
            .push(SkillDiagnostic::warning(warning, Some(file.to_path_buf())));
    }
    let Some(description) = frontmatter
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        catalog.diagnostics.push(SkillDiagnostic::warning(
            "description is required",
            Some(file.to_path_buf()),
        ));
        return Ok(None);
    };
    if description.len() > MAX_DESCRIPTION_LENGTH {
        catalog.diagnostics.push(SkillDiagnostic::warning(
            format!(
                "description exceeds {MAX_DESCRIPTION_LENGTH} characters ({})",
                description.len()
            ),
            Some(file.to_path_buf()),
        ));
    }
    let metadata = frontmatter.metadata.clone().unwrap_or(Value::Null);
    let tags = merge_string_values(
        frontmatter.tags.as_ref(),
        metadata_pointer(&metadata, "/hermes/tags"),
    );
    let related = merge_string_values(
        frontmatter
            .related
            .as_ref()
            .or(frontmatter.related_skills.as_ref()),
        metadata_pointer(&metadata, "/hermes/related_skills"),
    );
    let platforms = string_values(frontmatter.platforms.as_ref());
    let supported_on_current_platform = skill_matches_current_os(&platforms);
    let legacy_env_vars = frontmatter
        .prerequisites
        .as_ref()
        .and_then(|value| value.get("env_vars"))
        .map(string_values_from_value)
        .unwrap_or_default();
    let setup_help = setup_help(frontmatter.setup.as_ref());
    let required_environment_variables = required_environment_variables(
        frontmatter.required_environment_variables.as_ref(),
        frontmatter.setup.as_ref(),
        &legacy_env_vars,
    );
    let required_credential_files = string_values(frontmatter.required_credential_files.as_ref());
    let allowed_tools = merge_string_values(
        frontmatter
            .allowed_tools_hyphen
            .as_ref()
            .or(frontmatter.allowed_tools_underscore.as_ref()),
        None,
    );
    Ok(Some(Skill {
        name,
        description,
        file_path: file.to_path_buf(),
        base_dir,
        source,
        disable_model_invocation: frontmatter.disable_model_invocation.unwrap_or(false),
        category: None,
        tags,
        related,
        platforms,
        required_environment_variables,
        required_credential_files,
        setup_help,
        compatibility: frontmatter.compatibility,
        license: frontmatter.license,
        allowed_tools,
        supported_on_current_platform,
    }))
}

fn add_skill(
    skill: Skill,
    only_names: Option<&BTreeSet<String>>,
    disabled: &BTreeSet<String>,
    catalog: &mut SkillCatalog,
    seen: &mut BTreeMap<String, PathBuf>,
) {
    let explicit = skill.source == SkillSource::Explicit;
    if let Some(only_names) = only_names
        && !only_names.contains(&skill.name)
    {
        return;
    }
    if disabled.contains(&skill.name) && !explicit {
        catalog.diagnostics.push(SkillDiagnostic::warning(
            format!("skill disabled: {}", skill.name),
            Some(skill.file_path.clone()),
        ));
        return;
    }
    if let Some(winner) = seen.get(&skill.name) {
        catalog.diagnostics.push(SkillDiagnostic::collision(
            &skill.name,
            winner,
            &skill.file_path,
        ));
        let entry = catalog
            .collisions
            .entry(skill.name.clone())
            .or_insert_with(|| vec![winner.clone()]);
        entry.push(skill.file_path.clone());
        return;
    }
    seen.insert(skill.name.clone(), skill.file_path.clone());
    catalog.skills.push(skill);
}

fn skill_mentions(prompt: &str) -> Vec<String> {
    let bytes = prompt.as_bytes();
    let mut mentions = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let Some(first) = bytes.get(start) else {
            index += 1;
            continue;
        };
        if !is_skill_name_start(*first) {
            index += 1;
            continue;
        }
        let mut end = start + 1;
        while let Some(byte) = bytes.get(end)
            && is_skill_name_char(*byte)
        {
            end += 1;
        }
        mentions.push(prompt[start..end].to_string());
        index = end;
    }
    mentions
}

fn select_skills<'a>(
    catalog: &'a SkillCatalog,
    names: impl Iterator<Item = &'a str>,
    exact_input: bool,
) -> Vec<SelectedSkill> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for name in names {
        if exact_input && !validate_name(name, name).is_empty() {
            continue;
        }
        if catalog.collisions.contains_key(name) {
            continue;
        }
        if let Some(skill) = catalog
            .skills
            .iter()
            .find(|skill| skill.name == name && skill.supported_on_current_platform)
            && seen.insert(skill.file_path.clone())
        {
            selected.push(selected_skill(skill));
        }
    }
    selected
}

fn selected_skill(skill: &Skill) -> SelectedSkill {
    SelectedSkill {
        name: skill.name.clone(),
        path: skill.file_path.clone(),
    }
}

fn explicit_path_selects_skill(path: &Path, skill_path: &Path, base_dir: &Path) -> bool {
    if skill_path == path {
        return true;
    }
    if path.is_dir() && skill_path.starts_with(path) {
        return true;
    }
    let package_skill = skill_path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md");
    package_skill && path.starts_with(base_dir)
}

fn is_skill_name_start(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

fn is_skill_name_char(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

fn parse_frontmatter(
    content: &str,
) -> std::result::Result<(SkillFrontmatter, String), serde_yaml::Error> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---") {
        return Ok((SkillFrontmatter::default(), normalized));
    }
    let Some(end) = normalized[3..].find("\n---") else {
        return Ok((SkillFrontmatter::default(), normalized));
    };
    let yaml_end = 3 + end;
    let yaml = normalized[3..yaml_end].trim();
    let body_start = yaml_end + "\n---".len();
    let body = normalized[body_start..]
        .trim_start_matches('\n')
        .to_string();
    let frontmatter = serde_yaml::from_str::<SkillFrontmatter>(yaml)?;
    Ok((frontmatter, body))
}

fn metadata_pointer<'a>(metadata: &'a Value, pointer: &str) -> Option<&'a Value> {
    metadata.pointer(pointer)
}

fn merge_string_values(primary: Option<&Value>, secondary: Option<&Value>) -> Vec<String> {
    let mut out = string_values(primary);
    let mut seen = out.iter().cloned().collect::<BTreeSet<_>>();
    for value in string_values(secondary) {
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    out
}

fn string_values(value: Option<&Value>) -> Vec<String> {
    value.map(string_values_from_value).unwrap_or_default()
}

fn string_values_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn setup_help(setup: Option<&Value>) -> Option<String> {
    setup
        .and_then(Value::as_object)
        .and_then(|object| object.get("help"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn required_environment_variables(
    raw: Option<&Value>,
    setup: Option<&Value>,
    legacy_env_vars: &[String],
) -> Vec<RequiredEnvironmentVariable> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let setup_help_text = setup_help(setup);
    append_required_env_values(raw, &mut out, &mut seen, setup_help_text.as_deref());
    if let Some(setup) = setup.and_then(Value::as_object)
        && let Some(collect) = setup.get("collect_secrets")
    {
        let values = match collect {
            Value::Array(values) => values.clone(),
            Value::Object(_) => vec![collect.clone()],
            _ => Vec::new(),
        };
        for value in values {
            if let Some(object) = value.as_object() {
                let name = object
                    .get("env_var")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(name) = name {
                    let prompt = object
                        .get("prompt")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned);
                    let help = object
                        .get("provider_url")
                        .or_else(|| object.get("url"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                        .or_else(|| setup_help_text.clone());
                    append_required_env(
                        RequiredEnvironmentVariable {
                            name: name.to_string(),
                            prompt,
                            help,
                            required_for: None,
                            optional: false,
                        },
                        &mut out,
                        &mut seen,
                    );
                }
            }
        }
    }
    for name in legacy_env_vars {
        append_required_env(
            RequiredEnvironmentVariable {
                name: name.clone(),
                prompt: None,
                help: setup_help_text.clone(),
                required_for: None,
                optional: false,
            },
            &mut out,
            &mut seen,
        );
    }
    out
}

fn append_required_env_values(
    raw: Option<&Value>,
    out: &mut Vec<RequiredEnvironmentVariable>,
    seen: &mut BTreeSet<String>,
    setup_help: Option<&str>,
) {
    let Some(raw) = raw else {
        return;
    };
    let values = match raw {
        Value::Array(values) => values.clone(),
        Value::Object(_) | Value::String(_) => vec![raw.clone()],
        _ => Vec::new(),
    };
    for value in values {
        match value {
            Value::String(name) => append_required_env(
                RequiredEnvironmentVariable {
                    name,
                    prompt: None,
                    help: setup_help.map(ToOwned::to_owned),
                    required_for: None,
                    optional: false,
                },
                out,
                seen,
            ),
            Value::Object(object) => {
                let name = object
                    .get("name")
                    .or_else(|| object.get("env_var"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(name) = name {
                    append_required_env(
                        RequiredEnvironmentVariable {
                            name: name.to_string(),
                            prompt: object
                                .get("prompt")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToOwned::to_owned),
                            help: object
                                .get("help")
                                .or_else(|| object.get("provider_url"))
                                .or_else(|| object.get("url"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToOwned::to_owned)
                                .or_else(|| setup_help.map(ToOwned::to_owned)),
                            required_for: object
                                .get("required_for")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToOwned::to_owned),
                            optional: object
                                .get("optional")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        },
                        out,
                        seen,
                    );
                }
            }
            _ => {}
        }
    }
}

fn append_required_env(
    env: RequiredEnvironmentVariable,
    out: &mut Vec<RequiredEnvironmentVariable>,
    seen: &mut BTreeSet<String>,
) {
    if env.name.is_empty() || !valid_env_name(&env.name) || !seen.insert(env.name.clone()) {
        return;
    }
    out.push(env);
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn skill_matches_current_os(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true;
    }
    let current = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    platforms
        .iter()
        .any(|platform| platform.eq_ignore_ascii_case(current))
}

fn strip_frontmatter(content: &str) -> &str {
    if !content.starts_with("---") {
        return content;
    }
    let normalized = content;
    let Some(end) = normalized[3..].find("\n---") else {
        return content;
    };
    let body_start = 3 + end + "\n---".len();
    normalized[body_start..].trim_start_matches(['\n', '\r'])
}

fn preprocess_skill_content(
    content: &str,
    skill_dir: &Path,
    session_id: Option<&str>,
    settings: Option<&SkillSettings>,
) -> String {
    let default_settings;
    let settings = if let Some(settings) = settings {
        settings
    } else {
        default_settings = SkillSettings::default();
        &default_settings
    };
    let mut out = content.to_string();
    if settings.template_vars {
        let skill_dir = skill_dir.display().to_string();
        out = out
            .replace("${PSYCHEVO_SKILL_DIR}", &skill_dir)
            .replace("${HERMES_SKILL_DIR}", &skill_dir);
        if let Some(session_id) = session_id {
            out = out
                .replace("${PSYCHEVO_SESSION_ID}", session_id)
                .replace("${HERMES_SESSION_ID}", session_id);
        }
    }
    if settings.inline_shell {
        out = expand_inline_shell(&out, skill_dir, settings.inline_shell_timeout_secs);
    }
    out
}

fn expand_inline_shell(content: &str, skill_dir: &Path, timeout_secs: u64) -> String {
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find("!`") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('`') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let command = after[..end].trim();
        if command.contains('\n') || command.is_empty() {
            out.push_str("");
        } else {
            out.push_str(&run_inline_shell(command, skill_dir, timeout_secs));
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn run_inline_shell(command: &str, skill_dir: &Path, timeout_secs: u64) -> String {
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(skill_dir)
        .output();
    match output {
        Ok(output) => {
            let mut text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr)
                    .trim_end()
                    .to_string()
            } else {
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string()
            };
            if text.len() > 4000 {
                text.truncate(4000);
                text.push_str("...[truncated]");
            }
            text
        }
        Err(err) => format!("[inline-shell error after {timeout_secs}s: {err}]"),
    }
}

fn available_files(base_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in ["references", "scripts", "assets", "templates"] {
        files.extend(collect_relative_files(base_dir, &base_dir.join(dir)).unwrap_or_default());
    }
    files.sort();
    files
}

fn validate_name(name: &str, parent: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if name != parent {
        errors.push(format!(
            "name \"{name}\" does not match parent or file name \"{parent}\""
        ));
    }
    if name.len() > MAX_NAME_LENGTH {
        errors.push(format!(
            "name exceeds {MAX_NAME_LENGTH} characters ({})",
            name.len()
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        errors.push(
            "name contains invalid characters (must be lowercase a-z, 0-9, hyphens only)"
                .to_string(),
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        errors.push("name must not start or end with a hyphen".to_string());
    }
    if name.contains("--") {
        errors.push("name must not contain consecutive hyphens".to_string());
    }
    errors
}

fn truncate_description(description: &str) -> String {
    if description.len() <= MAX_DESCRIPTION_LENGTH {
        description.to_string()
    } else {
        let mut truncated = description
            .chars()
            .take(MAX_DESCRIPTION_LENGTH.saturating_sub(3))
            .collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

fn find_skill<'a>(catalog: &'a SkillCatalog, name: &str) -> Result<&'a Skill> {
    if let Some(matches) = catalog.collisions.get(name) {
        return Err(Error::Message(format!(
            "ambiguous skill name {name}: {} matching skills ({})",
            matches.len(),
            matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    catalog
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| Error::Message(format!("skill not found: {name}")))
}

fn resolve_skill_relative_path(skill: &Skill, raw: &str) -> Result<PathBuf> {
    let rel = Path::new(raw);
    if rel.is_absolute()
        || rel
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::Message(
            "skill file path must stay inside the skill directory".to_string(),
        ));
    }
    let target = skill.base_dir.join(rel);
    let canonical_base = skill.base_dir.canonicalize()?;
    let canonical_target = target.canonicalize()?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(Error::Message(
            "skill file path escapes skill directory".to_string(),
        ));
    }
    Ok(canonical_target)
}

fn resolve_skill_write_path(skill: &Skill, raw: &str) -> Result<PathBuf> {
    let rel = Path::new(raw);
    if rel.is_absolute()
        || rel
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::Message(
            "skill file path must stay inside the skill directory".to_string(),
        ));
    }
    let first = rel
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .unwrap_or_default();
    if !["references", "scripts", "assets", "templates"].contains(&first) {
        return Err(Error::Message(
            "supporting skill files must be under references/, scripts/, assets/, or templates/"
                .to_string(),
        ));
    }
    Ok(skill.base_dir.join(rel))
}

fn linked_files(base_dir: &Path) -> Value {
    let mut result = serde_json::Map::new();
    for dir in ["references", "scripts", "assets", "templates"] {
        let path = base_dir.join(dir);
        let files = collect_relative_files(base_dir, &path).unwrap_or_default();
        if !files.is_empty() {
            result.insert(dir.to_string(), json!(files));
        }
    }
    Value::Object(result)
}

fn collect_relative_files(base_dir: &Path, dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    collect_relative_files_inner(base_dir, dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_relative_files_inner(
    base_dir: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files_inner(base_dir, &path, files)?;
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(base_dir)
        {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn ensure_mutable_skill(skill: &Skill, home: &Path, workdir: &Path) -> Result<()> {
    let global = home.join("skills").canonicalize().ok();
    let project = workdir.join(".psychevo").join("skills").canonicalize().ok();
    let target = skill.file_path.canonicalize()?;
    let mutable = global.as_ref().is_some_and(|root| target.starts_with(root))
        || project
            .as_ref()
            .is_some_and(|root| target.starts_with(root));
    if mutable {
        Ok(())
    } else {
        Err(Error::Message(format!(
            "skill is not in a mutable Psychevo skills directory: {}",
            skill.file_path.display()
        )))
    }
}

fn yaml_scalar(value: &str) -> String {
    serde_yaml::to_string(value)
        .unwrap_or_else(|_| format!("{value:?}"))
        .trim()
        .trim_start_matches("---")
        .trim()
        .to_string()
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn scan_text(text: &str, file: &Path, findings: &mut Vec<ScanFinding>) {
    let lower = text.to_lowercase();
    let patterns = [
        (
            "prompt_injection",
            "critical",
            "prompt injection",
            [
                "ignore previous instructions",
                "disregard your instructions",
                "system prompt override",
                "do not tell the user",
            ]
            .as_slice(),
        ),
        (
            "exfiltration",
            "critical",
            "secret exfiltration",
            [
                "curl", "wget", "printenv", ".env", ".ssh", "api_key", "token", "password",
            ]
            .as_slice(),
        ),
        (
            "destructive",
            "critical",
            "destructive command",
            ["rm -rf /", "mkfs", "dd if=", "/etc/sudoers"].as_slice(),
        ),
        (
            "persistence",
            "high",
            "persistence",
            [
                "crontab",
                "authorized_keys",
                ".bashrc",
                ".zshrc",
                "systemctl enable",
            ]
            .as_slice(),
        ),
        (
            "network",
            "high",
            "network tunnel",
            ["ngrok", "cloudflared", "reverse shell", "nc -l", "ncat -l"].as_slice(),
        ),
        (
            "obfuscation",
            "medium",
            "obfuscation",
            ["base64 -d", "eval(", "fromcharcode", "\\u200b"].as_slice(),
        ),
    ];
    for (category, severity, label, needles) in patterns {
        if needles.iter().any(|needle| lower.contains(needle)) {
            findings.push(ScanFinding {
                category: category.to_string(),
                severity: severity.to_string(),
                file: file.to_path_buf(),
                pattern: label.to_string(),
            });
        }
    }
}

fn text_files_under(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        text_files_under_inner(path, &mut files)?;
    }
    Ok(files)
}

fn text_files_under_inner(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".git") || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            text_files_under_inner(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn clone_git_source(source: &str) -> Result<PathBuf> {
    let temp = tempfile::Builder::new()
        .prefix("psychevo-skill-git-")
        .tempdir()?;
    let path = temp.keep();
    let status = Command::new("git")
        .args(["clone", "--depth", "1", source])
        .arg(&path)
        .status()?;
    if status.success() {
        Ok(path)
    } else {
        Err(Error::Message(format!("git clone failed for {source}")))
    }
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target_path = target.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target_path)?;
        } else if path.is_file() {
            fs::copy(path, target_path)?;
        }
    }
    Ok(())
}

fn ancestor_agents_skill_dirs(workdir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = workdir.to_path_buf();
    loop {
        result.push(current.join(".agents").join("skills"));
        if current.join(".git").exists() {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    result
}

fn existing_input_path(
    input: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    let path = resolve_configured_path(input, workdir, env)?;
    Ok(path.exists().then_some(path))
}

fn looks_like_existing_path(input: &str, workdir: &Path, env: &BTreeMap<String, String>) -> bool {
    existing_input_path(input, workdir, env)
        .ok()
        .flatten()
        .is_some()
}

fn resolve_configured_path(
    raw: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let raw = raw.trim();
    let path = if raw == "~" {
        home_path(env)?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_path(env)?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        workdir.join(path)
    })
}

fn home_path(env: &BTreeMap<String, String>) -> Result<PathBuf> {
    env.get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

fn env_path(name: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    env.get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    deep_merge(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}
