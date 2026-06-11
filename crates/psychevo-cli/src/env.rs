use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

pub(crate) fn ensure_home_initialized(home: &Path) -> Result<()> {
    let config = home.join("config.toml");
    if !config.exists() {
        return Err(anyhow!(
            "Psychevo home is not initialized; run `pevo init` to create {}",
            config.display()
        ));
    }
    Ok(())
}

pub(crate) fn resolve_state_db(
    env_map: &BTreeMap<String, String>,
    home: &Path,
    cwd: &Path,
) -> Result<PathBuf> {
    if let Some(value) = env_value("PSYCHEVO_DB", env_map) {
        if value == ":memory:" {
            Ok(PathBuf::from(value))
        } else {
            resolve_explicit_path(Path::new(&value), env_map, cwd)
        }
    } else {
        Ok(home.join("state.db"))
    }
}

pub(crate) fn resolve_psychevo_home(
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    Ok(crate::profiles::resolve_active_profile(env_map, cwd)?.home)
}

pub(crate) fn env_path(
    name: &str,
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Option<PathBuf>> {
    env_value(name, env_map)
        .map(|value| resolve_explicit_path(Path::new(&value), env_map, cwd))
        .transpose()
}

pub(crate) fn resolve_explicit_path(
    path: &Path,
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(cwd.join(expanded))
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
    env_value("HOME", env_map)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is required to expand ~"))
}

pub(crate) fn env_value(name: &str, env_map: &BTreeMap<String, String>) -> Option<String> {
    env_map
        .get(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn inherited_env() -> BTreeMap<String, String> {
    let mut env_map: BTreeMap<String, String> = env::vars().collect();
    if let Ok(cwd) = env::current_dir()
        && let Ok(profile) = crate::profiles::resolve_active_profile(&env_map, &cwd)
    {
        env_map.insert(
            "PSYCHEVO_HOME".to_string(),
            profile.home.display().to_string(),
        );
        env_map.insert(
            crate::profiles::PROFILE_ENV.to_string(),
            crate::profiles::profile_env_value(&profile),
        );
        env_map.insert(
            crate::profiles::PROFILE_HOME_ENV.to_string(),
            profile.home.display().to_string(),
        );
    }
    env_map
}
