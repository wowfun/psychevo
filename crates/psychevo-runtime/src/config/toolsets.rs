#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsetMutationResult {
    pub config_path: PathBuf,
    pub name: String,
    pub changed: bool,
}

pub fn toolsets_value(options: &RunOptions, scope: ConfigScope) -> Result<Value> {
    let document = config_show_value(options, scope)?;
    let value = document.get("value").cloned().unwrap_or_else(|| json!({}));
    let config = parse_run_config(value)?;
    let plan_names = crate::tools::effective_tool_names_for_mode_with_config(
        RunMode::Plan,
        &config.tools,
        &config.toolsets,
    );
    let default_names = crate::tools::effective_tool_names_for_mode_with_config(
        RunMode::Default,
        &config.tools,
        &config.toolsets,
    );
    let mode_config = |mode: RunMode| {
        let entry = config.tools.modes.get(mode.as_str());
        json!({
            "enabled_toolsets": entry.and_then(|entry| entry.enabled_toolsets.clone()),
            "disabled_toolsets": entry.map(|entry| entry.disabled_toolsets.clone()).unwrap_or_default(),
            "effective_tools": match mode {
                RunMode::Plan => plan_names.clone(),
                RunMode::Default => default_names.clone(),
            },
        })
    };
    let mut toolsets = Vec::new();
    for name in crate::tools::builtin_toolset_names() {
        let tools = crate::tools::builtin_toolset_tools(name)
            .unwrap_or(&[])
            .iter()
            .map(|tool| (*tool).to_string())
            .collect::<Vec<_>>();
        toolsets.push(json!({
            "name": name,
            "source": "built_in",
            "description": crate::tools::builtin_toolset_description(name).unwrap_or(""),
            "tools": tools,
            "includes": [],
            "unknown_tools": [],
        }));
    }
    for (name, toolset) in &config.toolsets {
        let unknown_tools = toolset
            .tools
            .iter()
            .filter(|tool| !crate::tools::known_tool_name(tool))
            .cloned()
            .collect::<Vec<_>>();
        toolsets.push(json!({
            "name": name,
            "source": "custom",
            "description": toolset.description.clone(),
            "tools": toolset.tools.clone(),
            "includes": toolset.includes.clone(),
            "unknown_tools": unknown_tools,
        }));
    }
    Ok(json!({
        "scope": document.get("scope").cloned().unwrap_or(Value::String("effective".to_string())),
        "path": document.get("path").cloned().unwrap_or(Value::Null),
        "sources": document.get("sources").cloned().unwrap_or(Value::Array(Vec::new())),
        "default_enabled_toolsets": crate::tools::default_enabled_toolsets(),
        "modes": {
            "plan": mode_config(RunMode::Plan),
            "default": mode_config(RunMode::Default),
        },
        "toolsets": toolsets,
    }))
}

pub fn set_local_toolset_enabled(
    config_dir: PathBuf,
    mode: RunMode,
    name: &str,
    enabled: bool,
) -> Result<ToolsetMutationResult> {
    let name = normalize_toolset_name(name)?;
    let mode_key = mode.as_str();
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let before = parsed.clone();
    if enabled {
        remove_mode_toolset_entry(&mut parsed, mode_key, "disabled_toolsets", &name)?;
        let defaults = crate::tools::default_enabled_toolsets()
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let enabled_toolsets = mode_toolset_array_mut(&mut parsed, mode_key, "enabled_toolsets")?;
        if enabled_toolsets.is_empty() {
            enabled_toolsets.extend(defaults.into_iter().map(Value::String));
        }
        push_unique_string(enabled_toolsets, &name);
    } else {
        remove_mode_toolset_entry(&mut parsed, mode_key, "enabled_toolsets", &name)?;
        push_unique_string(
            mode_toolset_array_mut(&mut parsed, mode_key, "disabled_toolsets")?,
            &name,
        );
    }
    let changed = parsed != before;
    if changed {
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(ToolsetMutationResult {
        config_path,
        name,
        changed,
    })
}

pub fn create_local_toolset(
    config_dir: PathBuf,
    name: &str,
    description: Option<String>,
    tools: Vec<String>,
    includes: Vec<String>,
    force: bool,
) -> Result<ToolsetMutationResult> {
    let name = normalize_toolset_name(name)?;
    validate_toolset_entries(&tools, "tool")?;
    validate_toolset_entries(&includes, "include")?;
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut parsed);
    let root = parsed
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let toolsets = root
        .entry("toolsets".to_string())
        .or_insert_with(|| json!({}));
    ensure_json_object(toolsets);
    let toolsets = toolsets
        .as_object_mut()
        .ok_or_else(|| Error::Config("toolsets must be an object".to_string()))?;
    if toolsets.contains_key(&name) && !force {
        return Err(Error::Config(format!(
            "toolset {name} already exists; pass --force to overwrite"
        )));
    }
    let mut value = json!({
        "tools": tools,
        "includes": includes,
    });
    if let Some(description) = description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        value["description"] = Value::String(description);
    }
    let changed = toolsets.get(&name) != Some(&value);
    toolsets.insert(name.clone(), value);
    if changed {
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(ToolsetMutationResult {
        config_path,
        name,
        changed,
    })
}

pub fn remove_local_toolset(config_dir: PathBuf, name: &str) -> Result<ToolsetMutationResult> {
    let name = normalize_toolset_name(name)?;
    if crate::tools::builtin_toolset_names().contains(&name.as_str()) {
        return Err(Error::Config(format!(
            "built-in toolset {name} cannot be removed"
        )));
    }
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let mut changed = false;
    if let Some(toolsets) = parsed
        .get_mut("toolsets")
        .and_then(Value::as_object_mut)
        && toolsets.remove(&name).is_some()
    {
        changed = true;
    }
    for mode in ["plan", "default"] {
        changed |= remove_mode_toolset_entry(&mut parsed, mode, "enabled_toolsets", &name)?;
        changed |= remove_mode_toolset_entry(&mut parsed, mode, "disabled_toolsets", &name)?;
    }
    if changed {
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(ToolsetMutationResult {
        config_path,
        name,
        changed,
    })
}

fn normalize_toolset_name(name: &str) -> Result<String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if valid {
        Ok(name.to_string())
    } else {
        Err(Error::Config(format!("invalid toolset name: {name}")))
    }
}

fn validate_toolset_entries(entries: &[String], label: &str) -> Result<()> {
    for entry in entries {
        if entry.trim().is_empty() {
            return Err(Error::Config(format!("toolset {label} must not be empty")));
        }
    }
    Ok(())
}

fn mode_toolset_array_mut<'a>(
    value: &'a mut Value,
    mode: &str,
    key: &str,
) -> Result<&'a mut Vec<Value>> {
    ensure_json_object(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let tools = root.entry("tools".to_string()).or_insert_with(|| json!({}));
    ensure_json_object(tools);
    let tools = tools
        .as_object_mut()
        .ok_or_else(|| Error::Config("tools must be an object".to_string()))?;
    let modes = tools.entry("modes".to_string()).or_insert_with(|| json!({}));
    ensure_json_object(modes);
    let modes = modes
        .as_object_mut()
        .ok_or_else(|| Error::Config("tools.modes must be an object".to_string()))?;
    let mode_value = modes.entry(mode.to_string()).or_insert_with(|| json!({}));
    ensure_json_object(mode_value);
    let mode_value = mode_value
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("tools.modes.{mode} must be an object")))?;
    let values = mode_value
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    values
        .as_array_mut()
        .ok_or_else(|| Error::Config(format!("tools.modes.{mode}.{key} must be an array")))
}

fn remove_mode_toolset_entry(
    value: &mut Value,
    mode: &str,
    key: &str,
    name: &str,
) -> Result<bool> {
    let Some(values) = value
        .get_mut("tools")
        .and_then(Value::as_object_mut)
        .and_then(|tools| tools.get_mut("modes"))
        .and_then(Value::as_object_mut)
        .and_then(|modes| modes.get_mut(mode))
        .and_then(Value::as_object_mut)
        .and_then(|mode| mode.get_mut(key))
    else {
        return Ok(false);
    };
    let values = values
        .as_array_mut()
        .ok_or_else(|| Error::Config(format!("tools.modes.{mode}.{key} must be an array")))?;
    let before = values.len();
    values.retain(|value| value.as_str() != Some(name));
    Ok(values.len() != before)
}

fn push_unique_string(values: &mut Vec<Value>, name: &str) {
    if !values.iter().any(|value| value.as_str() == Some(name)) {
        values.push(Value::String(name.to_string()));
    }
}
