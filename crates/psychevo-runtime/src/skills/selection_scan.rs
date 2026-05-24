#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn add_skill(
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

pub(crate) fn skill_mentions(prompt: &str) -> Vec<String> {
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

pub(crate) fn select_skills<'a>(
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

pub(crate) fn selected_skill(skill: &Skill) -> SelectedSkill {
    SelectedSkill {
        name: skill.name.clone(),
        path: skill.file_path.clone(),
    }
}

pub(crate) fn explicit_path_selects_skill(path: &Path, skill_path: &Path, base_dir: &Path) -> bool {
    if skill_path == path {
        return true;
    }
    if path.is_dir() && skill_path.starts_with(path) {
        return true;
    }
    let package_skill = skill_path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md");
    package_skill && path.starts_with(base_dir)
}

pub(crate) fn is_skill_name_start(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

pub(crate) fn is_skill_name_char(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

pub(crate) fn parse_frontmatter(
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

pub(crate) fn metadata_pointer<'a>(metadata: &'a Value, pointer: &str) -> Option<&'a Value> {
    metadata.pointer(pointer)
}

pub(crate) fn merge_string_values(
    primary: Option<&Value>,
    secondary: Option<&Value>,
) -> Vec<String> {
    let mut out = string_values(primary);
    let mut seen = out.iter().cloned().collect::<BTreeSet<_>>();
    for value in string_values(secondary) {
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    out
}

pub(crate) fn string_values(value: Option<&Value>) -> Vec<String> {
    value.map(string_values_from_value).unwrap_or_default()
}

pub(crate) fn string_values_from_value(value: &Value) -> Vec<String> {
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

pub(crate) fn setup_help(setup: Option<&Value>) -> Option<String> {
    setup
        .and_then(Value::as_object)
        .and_then(|object| object.get("help"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn required_environment_variables(
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

pub(crate) fn append_required_env_values(
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

pub(crate) fn append_required_env(
    env: RequiredEnvironmentVariable,
    out: &mut Vec<RequiredEnvironmentVariable>,
    seen: &mut BTreeSet<String>,
) {
    if env.name.is_empty() || !valid_env_name(&env.name) || !seen.insert(env.name.clone()) {
        return;
    }
    out.push(env);
}

pub(crate) fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub(crate) fn skill_matches_current_os(platforms: &[String]) -> bool {
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

pub(crate) fn strip_frontmatter(content: &str) -> &str {
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

pub(crate) fn preprocess_skill_content(
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

pub(crate) fn expand_inline_shell(content: &str, skill_dir: &Path, timeout_secs: u64) -> String {
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

pub(crate) fn run_inline_shell(command: &str, skill_dir: &Path, timeout_secs: u64) -> String {
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

pub(crate) fn available_files(base_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in ["references", "scripts", "assets", "templates"] {
        files.extend(collect_relative_files(base_dir, &base_dir.join(dir)).unwrap_or_default());
    }
    files.sort();
    files
}

pub(crate) fn validate_name(name: &str, parent: &str) -> Vec<String> {
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

pub(crate) fn truncate_description(description: &str) -> String {
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

pub(crate) fn find_skill<'a>(catalog: &'a SkillCatalog, name: &str) -> Result<&'a Skill> {
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

pub(crate) fn resolve_skill_relative_path(skill: &Skill, raw: &str) -> Result<PathBuf> {
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

pub(crate) fn resolve_skill_write_path(skill: &Skill, raw: &str) -> Result<PathBuf> {
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

pub(crate) fn linked_files(base_dir: &Path) -> Value {
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

pub(crate) fn collect_relative_files(base_dir: &Path, dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    collect_relative_files_inner(base_dir, dir, &mut files)?;
    files.sort();
    Ok(files)
}

pub(crate) fn collect_relative_files_inner(
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

pub(crate) fn ensure_mutable_skill(skill: &Skill, home: &Path, workdir: &Path) -> Result<()> {
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

pub(crate) fn yaml_scalar(value: &str) -> String {
    serde_yaml::to_string(value)
        .unwrap_or_else(|_| format!("{value:?}"))
        .trim()
        .trim_start_matches("---")
        .trim()
        .to_string()
}

pub(crate) fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(crate) fn scan_text(text: &str, file: &Path, findings: &mut Vec<ScanFinding>) {
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

pub(crate) fn text_files_under(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        text_files_under_inner(path, &mut files)?;
    }
    Ok(files)
}

pub(crate) fn text_files_under_inner(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
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

pub(crate) fn clone_git_source(source: &str) -> Result<PathBuf> {
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

pub(crate) fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
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

pub(crate) fn ancestor_agents_skill_dirs(workdir: &Path) -> Vec<PathBuf> {
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

pub(crate) fn existing_input_path(
    input: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    let path = resolve_configured_path(input, workdir, env)?;
    Ok(path.exists().then_some(path))
}

pub(crate) fn looks_like_existing_path(
    input: &str,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> bool {
    existing_input_path(input, workdir, env)
        .ok()
        .flatten()
        .is_some()
}

pub(crate) fn resolve_configured_path(
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

pub(crate) fn home_path(env: &BTreeMap<String, String>) -> Result<PathBuf> {
    env.get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}
