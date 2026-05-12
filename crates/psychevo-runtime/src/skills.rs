use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};

const MAX_NAME_LENGTH: usize = 64;
const MAX_DESCRIPTION_LENGTH: usize = 1024;
const DISABLED_FILE: &str = ".disabled.json";
const PROVENANCE_FILE: &str = ".provenance.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub source: SkillSource,
    pub disable_model_invocation: bool,
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
    fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, Default)]
pub struct SkillSettings {
    pub disabled: BTreeSet<String>,
    pub paths: Vec<PathBuf>,
    pub enable_commands: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTarget {
    Global,
    Project,
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

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSkillSettings {
    disabled: Option<Vec<String>>,
    paths: Option<Vec<String>>,
    enable_commands: Option<bool>,
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
        deep_merge(&mut merged, load_jsonc(config_path, true)?);
    } else {
        deep_merge(&mut merged, load_jsonc(&home.join("config.jsonc"), false)?);
        deep_merge(
            &mut merged,
            load_jsonc(&workdir.join(".psychevo").join("config.jsonc"), false)?,
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
    home: &Path,
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
        for raw_path in raw.paths.unwrap_or_default() {
            settings
                .paths
                .push(resolve_configured_path(&raw_path, workdir, env)?);
        }
        settings.enable_commands = raw.enable_commands;
    }
    settings
        .disabled
        .extend(read_disabled_names(&home.join("skills"))?);
    settings.disabled.extend(read_disabled_names(
        &workdir.join(".psychevo").join("skills"),
    )?);
    Ok(settings)
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
        .filter(|skill| !skill.disable_model_invocation)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::new(),
        "The following skills provide specialized instructions for specific tasks.".to_string(),
        "When the task matches a skill description, use view_skill to load the skill before following it.".to_string(),
        "Resolve relative paths in skill files against the skill directory.".to_string(),
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
    let skills = catalog
        .skills
        .iter()
        .filter(|skill| include_hidden || !skill.disable_model_invocation)
        .map(|skill| {
            json!({
                "name": skill.name,
                "description": skill.description,
                "location": skill.file_path,
                "source": skill.source.as_str(),
                "disable_model_invocation": skill.disable_model_invocation,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "success": true,
        "skills": skills,
        "diagnostics": catalog.diagnostics,
        "count": skills.len(),
    })
}

pub fn view_skill_value(
    catalog: &SkillCatalog,
    name: &str,
    file_path: Option<&str>,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    let target = match file_path {
        Some(file_path) => resolve_skill_relative_path(skill, file_path)?,
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
    let text = String::from_utf8(bytes).map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let content = if file_path.is_none() {
        strip_frontmatter(&text).trim().to_string()
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
        "content": content,
        "linked_files": linked_files(&skill.base_dir),
    }))
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
    let root = target_skills_dir(home, workdir, target);
    fs::create_dir_all(&root)?;
    let mut disabled = read_disabled_names(&root)?;
    if enabled {
        disabled.remove(name);
    } else {
        disabled.insert(name.to_string());
    }
    write_disabled_names(&root, &disabled)?;
    Ok(json!({"success": true, "name": name, "enabled": enabled, "path": root.join(DISABLED_FILE)}))
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
        write_provenance(
            &target_root,
            &skill.name,
            &options.source,
            scan.verdict.clone(),
            &skill.name,
        )?;
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
    Ok(Some(Skill {
        name,
        description,
        file_path: file.to_path_buf(),
        base_dir,
        source,
        disable_model_invocation: frontmatter.disable_model_invocation.unwrap_or(false),
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
        if let Some(skill) = catalog.skills.iter().find(|skill| skill.name == name)
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

fn read_disabled_names(root: &Path) -> Result<BTreeSet<String>> {
    let path = root.join(DISABLED_FILE);
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(value
        .get("disabled")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn write_disabled_names(root: &Path, disabled: &BTreeSet<String>) -> Result<()> {
    fs::write(
        root.join(DISABLED_FILE),
        serde_json::to_string_pretty(&json!({ "disabled": disabled }))?,
    )?;
    Ok(())
}

fn write_provenance(
    target_root: &Path,
    name: &str,
    source: &str,
    verdict: ScanVerdict,
    original_name: &str,
) -> Result<()> {
    let path = target_root.join(PROVENANCE_FILE);
    let mut value = if path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(&path)?)?
    } else {
        json!({})
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::Message("provenance sidecar is not an object".to_string()))?;
    object.insert(
        name.to_string(),
        json!({
            "source": source,
            "source_type": if is_probable_git_source(source) { "git" } else { "local" },
            "installed_at_ms": now_ms(),
            "scanner_verdict": verdict,
            "original_skill_name": original_name,
        }),
    );
    fs::write(path, serde_json::to_string_pretty(&value)?)?;
    Ok(())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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

fn is_probable_git_source(source: &str) -> bool {
    source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("file://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || source.ends_with(".git")
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

fn load_jsonc(path: &Path, required: bool) -> Result<Value> {
    if !path.exists() {
        if required {
            return Err(Error::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path)?;
    let parsed: Option<Value> = jsonc_parser::parse_to_serde_value(&text, &Default::default())
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    Ok(parsed.unwrap_or_else(|| json!({})))
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
