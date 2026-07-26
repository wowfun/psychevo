#[allow(unused_imports)]
use super::*;
pub(crate) fn resolve_config_path(
    options: &RunOptions,
    env_map: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    if let Some(path) = &options.config_path {
        return Ok(Some(resolve_explicit_path(path, env_map)?));
    }
    env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()
}

pub(crate) fn resolve_psychevo_home(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    if let Some(value) = env_map
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        resolve_explicit_path(Path::new(value), env_map)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map)
    }
}

pub(crate) fn resolve_explicit_path(
    path: &Path,
    env_map: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(env::current_dir()?.join(expanded))
    }
}

pub(crate) fn expand_tilde(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_path(env_map);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_path(env_map)?.join(rest));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_map
        .get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

pub(crate) const CONFIG_FILE_NAME: &str = "config.toml";

pub(crate) fn load_toml_config_file(path: &Path, required: bool) -> Result<Value> {
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
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    let value = serde_json::to_value(parsed)?;
    if !value.is_object() {
        return Err(Error::Config(format!(
            "{} must contain a TOML table",
            path.display()
        )));
    }
    Ok(value)
}

pub(crate) fn write_toml_config_file(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml_config_string(value)?)?;
    Ok(())
}

pub(crate) fn toml_config_string(value: &Value) -> Result<String> {
    let toml_value = json_to_toml_value(value)?;
    let mut text = toml::to_string_pretty(&toml_value)?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

pub(crate) fn json_to_toml_value(value: &Value) -> Result<toml::Value> {
    match value {
        Value::Null => Err(Error::Config(
            "TOML config does not support null values".to_string(),
        )),
        Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(toml::Value::Integer(value))
            } else if let Some(value) = value.as_u64() {
                let value = i64::try_from(value)
                    .map_err(|_| Error::Config("TOML config integer exceeds i64".to_string()))?;
                Ok(toml::Value::Integer(value))
            } else if let Some(value) = value.as_f64() {
                Ok(toml::Value::Float(value))
            } else {
                Err(Error::Config(
                    "TOML config number is not representable".to_string(),
                ))
            }
        }
        Value::String(value) => Ok(toml::Value::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(json_to_toml_value)
            .collect::<Result<Vec<_>>>()
            .map(toml::Value::Array),
        Value::Object(values) => {
            let mut table = toml::map::Map::new();
            for (key, value) in values {
                if value.is_null() {
                    continue;
                }
                table.insert(key.clone(), json_to_toml_value(value)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

pub(crate) fn deep_merge(base: &mut Value, overlay: Value) {
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

pub(crate) fn load_dotenv_file(path: &Path, env_map: &mut BTreeMap<String, String>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_env_name(name) {
            continue;
        }
        env_map.insert(name.to_string(), strip_env_quotes(value.trim()).to_string());
    }
    Ok(())
}

pub(crate) fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

pub(crate) fn strip_env_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
