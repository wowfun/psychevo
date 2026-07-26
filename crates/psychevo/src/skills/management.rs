use super::catalog::{
    BUNDLES_DIR, InstallOptions, MAX_DESCRIPTION_LENGTH, SaveSkillBundleOptions, ScanResult,
    ScanVerdict, SelectedSkill, Skill, SkillBundle, SkillCatalog, SkillContextFragment,
    SkillDiagnostic, SkillSource, SkillTarget,
};
use super::selection_scan::{
    add_skill, clone_git_source, copy_dir_recursive, ensure_mutable_skill, escape_xml,
    existing_input_path, find_skill, merge_string_values, metadata_pointer, parse_frontmatter,
    required_environment_variables, resolve_skill_relative_path, resolve_skill_write_path,
    scan_text, setup_help, skill_matches_current_os, string_values, string_values_from_value,
    strip_frontmatter, text_files_under, validate_name, yaml_scalar,
};
use super::{
    BTreeMap, BTreeSet, CONFIG_FILE_NAME, Error, Path, PathBuf, Result, Value, fs, json,
    load_toml_config_file, write_toml_config_file,
};

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
            concat!(
                "<skill>\n",
                "<name>{}</name>\n",
                "<path>{}</path>\n",
                "<skill_dir>{}</skill_dir>\n",
                "<loaded_for_turn>true</loaded_for_turn>\n",
                "<usage>Follow this already-loaded skill body directly; ",
                "do not reload this SKILL.md unless you need a referenced supporting file.</usage>\n",
                "{}\n",
                "</skill>"
            ),
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
    cwd: &Path,
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
    let root = target_skills_dir(home, cwd, target);
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
    cwd: &Path,
    name: &str,
    old: &str,
    new: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, cwd)?;
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
    cwd: &Path,
    name: &str,
    content: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, cwd)?;
    fs::write(&skill.file_path, content)?;
    Ok(json!({"success": true, "name": skill.name, "path": skill.file_path}))
}

pub fn write_installed_skill(
    home: &Path,
    cwd: &Path,
    target: SkillTarget,
    name: &str,
    selector_path: Option<&Path>,
    raw_markdown: &str,
) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Message("skill name is required".to_string()));
    }
    let root = target_skills_dir(home, cwd, target);
    let file = if let Some(selector_path) = selector_path {
        installed_skill_write_path_at_selector(&root, target, selector_path)?
    } else {
        root.join(name).join("SKILL.md")
    };
    if !file.is_file() {
        return Err(Error::Message(format!(
            "skill not found in {} scope: {name}",
            target.as_str()
        )));
    }
    validate_installed_skill_markdown(&file, raw_markdown)?;
    fs::write(&file, raw_markdown)?;
    Ok(json!({
        "written": true,
        "name": name,
        "target": target.as_str(),
        "path": file,
    }))
}

fn installed_skill_write_path_at_selector(
    root: &Path,
    target: SkillTarget,
    selector_path: &Path,
) -> Result<PathBuf> {
    let canonical_root = root.canonicalize().map_err(|_| {
        Error::Message(format!(
            "skill root not found in {} scope: {}",
            target.as_str(),
            root.display()
        ))
    })?;
    let canonical_selector = selector_path.canonicalize()?;
    if !canonical_selector.starts_with(&canonical_root) {
        return Err(Error::Message(format!(
            "skill is not writable from {} scope: {}",
            target.as_str(),
            selector_path.display()
        )));
    }
    if canonical_selector
        .file_name()
        .and_then(|value| value.to_str())
        != Some("SKILL.md")
    {
        return Err(Error::Message(format!(
            "skill write only supports SKILL.md package files: {}",
            selector_path.display()
        )));
    }
    Ok(canonical_selector)
}

fn validate_installed_skill_markdown(file: &Path, raw_markdown: &str) -> Result<()> {
    if raw_markdown.as_bytes().contains(&0) {
        return Err(Error::Message(
            "skill markdown content must be text".to_string(),
        ));
    }
    let (frontmatter, _body) = parse_frontmatter(raw_markdown)
        .map_err(|err| Error::Message(format!("failed to parse skill frontmatter: {err}")))?;
    let parent = file
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let name = frontmatter.name.unwrap_or_else(|| parent.to_string());
    if let Some(err) = validate_name(&name, parent).into_iter().next() {
        return Err(Error::Message(err));
    }
    let Some(description) = frontmatter
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Err(Error::Message("description is required".to_string()));
    };
    if description.len() > MAX_DESCRIPTION_LENGTH {
        return Err(Error::Message(format!(
            "description exceeds {MAX_DESCRIPTION_LENGTH} characters ({})",
            description.len()
        )));
    }
    Ok(())
}

pub fn write_skill_file(
    catalog: &SkillCatalog,
    home: &Path,
    cwd: &Path,
    name: &str,
    file_path: &str,
    content: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, cwd)?;
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
    cwd: &Path,
    name: &str,
    file_path: &str,
) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, cwd)?;
    let target = resolve_skill_relative_path(skill, file_path)?;
    if target == skill.file_path {
        return Err(Error::Message(
            "remove_file cannot remove SKILL.md; use delete".to_string(),
        ));
    }
    fs::remove_file(&target)?;
    Ok(json!({"success": true, "name": skill.name, "path": target}))
}

pub fn remove_skill(catalog: &SkillCatalog, home: &Path, cwd: &Path, name: &str) -> Result<Value> {
    let skill = find_skill(catalog, name)?;
    ensure_mutable_skill(skill, home, cwd)?;
    if skill.file_path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
        fs::remove_dir_all(&skill.base_dir)?;
    } else {
        fs::remove_file(&skill.file_path)?;
    }
    Ok(json!({"success": true, "name": skill.name}))
}

pub fn remove_installed_skill(
    home: &Path,
    cwd: &Path,
    target: SkillTarget,
    name: &str,
    selector_path: Option<&Path>,
) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Message("skill name is required".to_string()));
    }
    let root = target_skills_dir(home, cwd, target);
    if let Some(selector_path) = selector_path {
        return remove_installed_skill_at_path(&root, target, name, selector_path);
    }
    let package = root.join(name);
    let markdown = root.join(format!("{name}.md"));
    let path = if package.join("SKILL.md").is_file() {
        fs::remove_dir_all(&package)?;
        package
    } else if markdown.is_file() {
        fs::remove_file(&markdown)?;
        markdown
    } else {
        return Err(Error::Message(format!(
            "skill not found in {} scope: {name}",
            target.as_str()
        )));
    };
    Ok(json!({"success": true, "name": name, "scope": target.as_str(), "path": path}))
}

fn remove_installed_skill_at_path(
    root: &Path,
    target: SkillTarget,
    name: &str,
    selector_path: &Path,
) -> Result<Value> {
    let canonical_root = root.canonicalize().map_err(|_| {
        Error::Message(format!(
            "skill root not found in {} scope: {}",
            target.as_str(),
            root.display()
        ))
    })?;
    let canonical_selector = selector_path.canonicalize()?;
    if !canonical_selector.starts_with(&canonical_root) {
        return Err(Error::Message(format!(
            "skill is not removable from {} scope: {}",
            target.as_str(),
            selector_path.display()
        )));
    }
    let removed = if canonical_selector
        .file_name()
        .and_then(|value| value.to_str())
        == Some("SKILL.md")
    {
        let Some(package_dir) = canonical_selector.parent() else {
            return Err(Error::Message("invalid skill package path".to_string()));
        };
        if !package_dir.join("SKILL.md").is_file() {
            return Err(Error::Message(format!(
                "skill package not found: {}",
                selector_path.display()
            )));
        }
        fs::remove_dir_all(package_dir)?;
        package_dir.to_path_buf()
    } else if canonical_selector
        .extension()
        .and_then(|value| value.to_str())
        == Some("md")
    {
        fs::remove_file(&canonical_selector)?;
        canonical_selector
    } else {
        return Err(Error::Message(format!(
            "skill path is not a removable SKILL.md package or root markdown file: {}",
            selector_path.display()
        )));
    };
    Ok(json!({"success": true, "name": name, "scope": target.as_str(), "path": removed}))
}

pub fn set_skill_enabled(
    home: &Path,
    cwd: &Path,
    target: SkillTarget,
    name: &str,
    enabled: bool,
) -> Result<Value> {
    let config_path = match target {
        SkillTarget::Global => home.join(CONFIG_FILE_NAME),
        SkillTarget::Project => cwd.join(".psychevo").join(CONFIG_FILE_NAME),
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
    cwd: &Path,
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
        SkillTarget::Project => cwd.join(".psychevo").join(CONFIG_FILE_NAME),
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

pub(crate) fn ensure_object(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
}

pub(crate) fn valid_config_key_segment(segment: &str) -> bool {
    segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

pub(crate) fn set_nested_skill_config_value(
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

pub(crate) fn skills_disabled_array_mut(value: &mut Value) -> Result<&mut Vec<Value>> {
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

pub fn install_skill(home: &Path, cwd: &Path, options: InstallOptions) -> Result<Value> {
    let source_path = match existing_input_path(&options.source, cwd, &BTreeMap::new())? {
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

    let target_root = target_skills_dir(home, cwd, options.target);
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
                    if options.force {
                        remove_existing_skill_target(&target)?;
                    } else {
                        return Err(Error::Message(format!(
                            "skill already exists: {}",
                            skill.name
                        )));
                    }
                }
                copy_dir_recursive(&skill.base_dir, &target)?;
                target.join("SKILL.md")
            } else {
                let target = target_root.join(format!("{}.md", skill.name));
                if target.exists() {
                    if options.force {
                        remove_existing_skill_target(&target)?;
                    } else {
                        return Err(Error::Message(format!(
                            "skill already exists: {}",
                            skill.name
                        )));
                    }
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

fn remove_existing_skill_target(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn target_skills_dir(home: &Path, cwd: &Path, target: SkillTarget) -> PathBuf {
    match target {
        SkillTarget::Global => home.join("skills"),
        SkillTarget::Project => cwd.join(".psychevo").join("skills"),
    }
}

pub fn list_skill_bundles(home: &Path, cwd: &Path) -> Result<Vec<SkillBundle>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (scope, root) in [
        (SkillTarget::Project, project_bundles_dir(cwd)),
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
    cwd: &Path,
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
    let root = bundles_dir(home, cwd, options.target);
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
    cwd: &Path,
    target: SkillTarget,
    name: &str,
) -> Result<Value> {
    let slug = slugify(name);
    let path = bundles_dir(home, cwd, target).join(format!("{slug}.toml"));
    if !path.exists() {
        return Err(Error::Message(format!("bundle not found: {name}")));
    }
    fs::remove_file(&path)?;
    Ok(json!({"success": true, "name": name, "scope": target.as_str(), "path": path}))
}

pub(crate) fn project_bundles_dir(cwd: &Path) -> PathBuf {
    cwd.join(".psychevo").join(BUNDLES_DIR)
}

pub(crate) fn bundles_dir(home: &Path, cwd: &Path, target: SkillTarget) -> PathBuf {
    match target {
        SkillTarget::Global => home.join(BUNDLES_DIR),
        SkillTarget::Project => project_bundles_dir(cwd),
    }
}

pub(crate) fn load_bundle_file(path: &Path, scope: SkillTarget) -> Result<Option<SkillBundle>> {
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

pub(crate) fn slugify(name: &str) -> String {
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

pub(crate) fn add_path_skills(
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

pub(crate) fn load_skills_from_dir(
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

pub(crate) fn load_skill_file(
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
    let required_tools = string_values(metadata_pointer(&metadata, "/hermes/requires_tools"));
    let fallback_for_tools =
        string_values(metadata_pointer(&metadata, "/hermes/fallback_for_tools"));
    let required_toolsets = string_values(metadata_pointer(&metadata, "/hermes/requires_toolsets"));
    let fallback_for_toolsets =
        string_values(metadata_pointer(&metadata, "/hermes/fallback_for_toolsets"));
    Ok(Some(Skill {
        name,
        description,
        file_path: file.to_path_buf(),
        base_dir,
        source,
        enabled: true,
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
        required_tools,
        fallback_for_tools,
        required_toolsets,
        fallback_for_toolsets,
        supported_on_current_platform,
        collision_group: Vec::new(),
    }))
}
