#[allow(unused_imports)]
pub(crate) use super::*;
pub fn set_default_model(home: &Path, workdir: &Path, global: bool, model: &str) -> Result<Value> {
    set_default_model_with_reasoning(home, workdir, global, model, None)
}

pub fn set_auxiliary_model(
    home: &Path,
    workdir: &Path,
    global: bool,
    task: &str,
    provider: &str,
    model: &str,
) -> Result<Value> {
    set_auxiliary_model_with_reasoning(home, workdir, global, task, provider, model, None)
}

pub fn set_auxiliary_model_with_reasoning(
    home: &Path,
    workdir: &Path,
    global: bool,
    task: &str,
    provider: &str,
    model: &str,
    reasoning_effort: Option<&str>,
) -> Result<Value> {
    let task = validate_auxiliary_model_task(task)?;
    let provider = normalize_provider_id(provider);
    let model = model.trim().to_string();
    let reasoning_effort = validate_reasoning_effort(reasoning_effort.map(str::to_string))?;
    if !model.is_empty() {
        if provider.is_empty() || provider == "auto" {
            return Err(Error::Config(
                "auxiliary model save requires a concrete provider".to_string(),
            ));
        }
        validate_default_model_provider(home, workdir, global, &provider)?;
    }
    let provider_value = if model.is_empty() {
        "auto".to_string()
    } else {
        provider
    };
    let config_dir = if global {
        home.to_path_buf()
    } else {
        canonical_workdir(workdir)?.join(".psychevo")
    };
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut value = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut value);
    set_config_path_value(
        &mut value,
        &["auxiliary", task, "provider"],
        Value::String(provider_value.clone()),
    )?;
    let model_value = if !model.is_empty() {
        if let Some(reasoning_effort) = &reasoning_effort {
            json!({
                "id": model.clone(),
                "reasoning_effort": reasoning_effort,
            })
        } else {
            Value::String(model.clone())
        }
    } else {
        Value::String(model.clone())
    };
    set_config_path_value(&mut value, &["auxiliary", task, "model"], model_value)?;
    write_toml_config_file(&config_path, &value)?;
    Ok(json!({
        "scope": if global { "global" } else { "local" },
        "path": config_path,
        "task": task,
        "provider": provider_value,
        "model": model,
        "reasoning_effort": reasoning_effort,
    }))
}

pub fn set_default_model_with_reasoning(
    home: &Path,
    workdir: &Path,
    global: bool,
    model: &str,
    reasoning_effort: Option<&str>,
) -> Result<Value> {
    let (provider, model) = parse_provider_model_spec(model)?;
    let reasoning_effort = validate_reasoning_effort(reasoning_effort.map(str::to_string))?;
    validate_default_model_provider(home, workdir, global, &provider)?;
    let config_dir = if global {
        home.to_path_buf()
    } else {
        canonical_workdir(workdir)?.join(".psychevo")
    };
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut value = load_toml_config_file(&config_path, false)?;
    ensure_json_object(&mut value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let model_spec = format!("{provider}/{model}");
    let model_value = if let Some(reasoning_effort) = &reasoning_effort {
        json!({
            "id": model_spec.clone(),
            "reasoning_effort": reasoning_effort,
        })
    } else {
        Value::String(model_spec.clone())
    };
    root.insert("model".to_string(), model_value);
    write_toml_config_file(&config_path, &value)?;
    Ok(json!({
        "scope": if global { "global" } else { "local" },
        "path": config_path,
        "model": model_spec,
        "reasoning_effort": reasoning_effort,
    }))
}

pub(crate) fn parse_provider_model_spec(model: &str) -> Result<(String, String)> {
    let Some((provider, model)) = model.trim().split_once('/') else {
        return Err(Error::Config(
            "model must use provider/model form".to_string(),
        ));
    };
    let provider = normalize_provider_id(provider);
    let model = model.trim().to_string();
    if provider.is_empty() || model.is_empty() {
        return Err(Error::Config(
            "model must use provider/model form".to_string(),
        ));
    }
    Ok((provider, model))
}

pub(crate) fn validate_auxiliary_model_task(task: &str) -> Result<&'static str> {
    match task.trim() {
        "title_generation" => Ok("title_generation"),
        "compression" => Ok("compression"),
        value => Err(Error::Config(format!(
            "unknown auxiliary model task: {value}"
        ))),
    }
}

pub(crate) fn validate_default_model_provider(
    home: &Path,
    workdir: &Path,
    global: bool,
    provider: &str,
) -> Result<()> {
    if built_in_provider(provider).is_some() {
        return Ok(());
    }
    let global_config =
        parse_run_config(load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?)?;
    if global_config.provider.contains_key(provider) {
        return Ok(());
    }
    if global {
        return Err(Error::Config(format!(
            "unknown provider for global model: {provider}"
        )));
    }
    let local_config = parse_run_config(load_toml_config_file(
        &canonical_workdir(workdir)?
            .join(".psychevo")
            .join(CONFIG_FILE_NAME),
        false,
    )?)?;
    if local_config.provider.contains_key(provider) {
        return Ok(());
    }
    Err(Error::Config(format!("unknown provider: {provider}")))
}
