use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::env::{env_value, home_path, resolve_explicit_path};

pub(crate) const DEFAULT_PROFILE: &str = "default";
pub(crate) const PROFILE_ENV: &str = "PSYCHEVO_PROFILE";
pub(crate) const PROFILE_HOME_ENV: &str = "PSYCHEVO_PROFILE_HOME";
pub(crate) const PROFILES_DIR: &str = "profiles";
pub(crate) const ACTIVE_PROFILE_FILE: &str = "active_profile";
pub(crate) const PROFILE_METADATA_FILE: &str = "profile.toml";

static CLI_PROFILE_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedProfile {
    pub(crate) name: String,
    pub(crate) home: PathBuf,
    pub(crate) registry_root: PathBuf,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct ProfileMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description_auto: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProfileSummary {
    pub(crate) name: String,
    pub(crate) home: PathBuf,
    pub(crate) active: bool,
    pub(crate) is_default: bool,
    pub(crate) metadata: ProfileMetadata,
    pub(crate) metadata_error: Option<String>,
}

#[derive(Debug, Clone)]
struct ProfileRoot {
    registry_root: PathBuf,
    explicit_named_profile: Option<(String, PathBuf)>,
}

pub(crate) fn set_cli_profile_override(profile: Option<String>) -> Result<()> {
    if let Some(name) = profile.as_deref() {
        validate_profile_selector(name)?;
    }
    CLI_PROFILE_OVERRIDE
        .set(profile)
        .map_err(|_| anyhow!("profile override already initialized"))?;
    Ok(())
}

pub(crate) fn resolve_active_profile(
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ResolvedProfile> {
    let root = profile_root(env_map, cwd)?;
    if let Some(name) = cli_profile_override() {
        return resolve_profile_selector(&root.registry_root, name, true);
    }
    if let Some((name, home)) = root.explicit_named_profile {
        return Ok(ResolvedProfile {
            is_default: false,
            registry_root: root.registry_root,
            name,
            home,
        });
    }
    if let Some(name) = read_sticky_profile(&root.registry_root)? {
        return resolve_profile_selector(&root.registry_root, &name, true);
    }
    Ok(default_profile(&root.registry_root))
}

pub(crate) fn registry_root(env_map: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf> {
    Ok(profile_root(env_map, cwd)?.registry_root)
}

pub(crate) fn named_profile_home(root: &Path, name: &str) -> PathBuf {
    root.join(PROFILES_DIR).join(name)
}

pub(crate) fn default_profile(root: &Path) -> ResolvedProfile {
    ResolvedProfile {
        name: DEFAULT_PROFILE.to_string(),
        home: root.to_path_buf(),
        registry_root: root.to_path_buf(),
        is_default: true,
    }
}

pub(crate) fn profile_for_name(root: &Path, name: &str) -> Result<ResolvedProfile> {
    resolve_profile_selector(root, name, true)
}

pub(crate) fn profile_for_name_unchecked(root: &Path, name: &str) -> Result<ResolvedProfile> {
    resolve_profile_selector(root, name, false)
}

pub(crate) fn list_profiles(root: &Path, active_name: Option<&str>) -> Result<Vec<ProfileSummary>> {
    let active = active_name.unwrap_or(DEFAULT_PROFILE);
    let mut summaries = Vec::new();
    summaries.push(summary_for_profile(root, DEFAULT_PROFILE, root, active)?);
    let dir = root.join(PROFILES_DIR);
    if dir.exists() {
        let mut names = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        names.sort();
        for name in names {
            let home = named_profile_home(root, &name);
            summaries.push(summary_for_profile(root, &name, &home, active)?);
        }
    }
    Ok(summaries)
}

pub(crate) fn read_metadata(home: &Path) -> (ProfileMetadata, Option<String>) {
    let path = home.join(PROFILE_METADATA_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (ProfileMetadata::default(), None);
        }
        Err(err) => return (ProfileMetadata::default(), Some(err.to_string())),
    };
    match toml::from_str::<ProfileMetadata>(&text) {
        Ok(metadata) => (metadata, None),
        Err(err) => (ProfileMetadata::default(), Some(err.to_string())),
    }
}

pub(crate) fn write_metadata(home: &Path, metadata: &ProfileMetadata) -> Result<()> {
    fs::create_dir_all(home)?;
    let text = toml::to_string_pretty(metadata)?;
    fs::write(home.join(PROFILE_METADATA_FILE), text)?;
    Ok(())
}

pub(crate) fn read_sticky_profile(root: &Path) -> Result<Option<String>> {
    let path = root.join(ACTIVE_PROFILE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    let name = text.trim();
    if name.is_empty() || name == DEFAULT_PROFILE {
        return Ok(None);
    }
    validate_profile_name(name)?;
    Ok(Some(name.to_string()))
}

pub(crate) fn write_sticky_profile(root: &Path, name: &str) -> Result<()> {
    fs::create_dir_all(root)?;
    let path = root.join(ACTIVE_PROFILE_FILE);
    if name == DEFAULT_PROFILE {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    validate_profile_name(name)?;
    fs::write(path, format!("{name}\n"))?;
    Ok(())
}

pub(crate) fn validate_profile_selector(name: &str) -> Result<()> {
    if name == DEFAULT_PROFILE {
        return Ok(());
    }
    validate_profile_name(name)
}

pub(crate) fn validate_profile_name(name: &str) -> Result<()> {
    if name == DEFAULT_PROFILE {
        bail!("profile name `{DEFAULT_PROFILE}` is reserved");
    }
    if RESERVED_PROFILE_NAMES.contains(&name) {
        bail!("profile name `{name}` is reserved");
    }
    validate_commandish_name(name, "profile name")
}

pub(crate) fn validate_alias_name(name: &str) -> Result<()> {
    if RESERVED_ALIAS_NAMES.contains(&name) || RESERVED_PROFILE_NAMES.contains(&name) {
        bail!("alias `{name}` conflicts with a reserved command");
    }
    validate_commandish_name(name, "alias")
}

pub(crate) fn copy_profile_setup(source: &Path, target: &Path) -> Result<()> {
    for file in ["config.toml", ".env"] {
        let source_file = source.join(file);
        if source_file.exists() {
            fs::copy(&source_file, target.join(file))
                .with_context(|| format!("copy {}", source_file.display()))?;
            protect_env_file(&target.join(file))?;
        }
    }
    for dir in ["skills", "agents"] {
        let source_dir = source.join(dir);
        if source_dir.exists() {
            copy_dir_contents(&source_dir, &target.join(dir))
                .with_context(|| format!("copy {}", source_dir.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn protect_env_file(path: &Path) -> Result<()> {
    if path.file_name().and_then(|name| name.to_str()) != Some(".env") || !path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

pub(crate) fn active_profile_name_for_env(
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> String {
    resolve_active_profile(env_map, cwd)
        .map(|profile| profile.name)
        .unwrap_or_else(|_| DEFAULT_PROFILE.to_string())
}

pub(crate) fn profile_env_value(profile: &ResolvedProfile) -> String {
    profile.name.clone()
}

pub(crate) fn alias_dir(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    Ok(home_path(env_map)?.join(".local/bin"))
}

pub(crate) fn alias_path(alias_dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        alias_dir.join(format!("{name}.cmd"))
    } else {
        alias_dir.join(name)
    }
}

pub(crate) fn write_alias(
    alias_dir: &Path,
    name: &str,
    profile: &str,
    env_map: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    validate_alias_name(name)?;
    validate_profile_name(profile)?;
    let path = alias_path(alias_dir, name);
    reject_alias_path_conflict(name, &path, profile, env_map)?;
    fs::create_dir_all(alias_dir)?;
    fs::write(&path, alias_script(profile))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
    }
    Ok(path)
}

pub(crate) fn remove_alias(alias_dir: &Path, name: &str, profile: &str) -> Result<bool> {
    validate_alias_name(name)?;
    validate_profile_name(profile)?;
    let path = alias_path(alias_dir, name);
    if !path.exists() {
        return Ok(false);
    }
    if !is_managed_alias_for_profile(&path, profile) {
        bail!(
            "refusing to remove non-Psychevo alias wrapper: {}",
            path.display()
        );
    }
    fs::remove_file(path)?;
    Ok(true)
}

pub(crate) fn is_managed_alias_for_profile(path: &Path, profile: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.contains("# psychevo-profile-alias")
        && text.contains(&format!("PEVO_PROFILE_NAME={profile}"))
}

fn profile_root(env_map: &BTreeMap<String, String>, cwd: &Path) -> Result<ProfileRoot> {
    let Some(value) = env_value("PSYCHEVO_HOME", env_map) else {
        return Ok(ProfileRoot {
            registry_root: resolve_explicit_path(Path::new("~/.psychevo"), env_map, cwd)?,
            explicit_named_profile: None,
        });
    };
    let home = resolve_explicit_path(Path::new(&value), env_map, cwd)?;
    if let Some((registry_root, name)) = named_profile_parts(&home) {
        return Ok(ProfileRoot {
            registry_root,
            explicit_named_profile: Some((name, home)),
        });
    }
    Ok(ProfileRoot {
        registry_root: home,
        explicit_named_profile: None,
    })
}

fn resolve_profile_selector(
    root: &Path,
    name: &str,
    require_existing: bool,
) -> Result<ResolvedProfile> {
    validate_profile_selector(name)?;
    if name == DEFAULT_PROFILE {
        return Ok(default_profile(root));
    }
    let home = named_profile_home(root, name);
    if require_existing && !home.is_dir() {
        bail!("profile `{name}` does not exist; create it with `pevo profile create {name}`");
    }
    Ok(ResolvedProfile {
        name: name.to_string(),
        home,
        registry_root: root.to_path_buf(),
        is_default: false,
    })
}

fn named_profile_parts(home: &Path) -> Option<(PathBuf, String)> {
    let parent = home.parent()?;
    if parent.file_name().and_then(|name| name.to_str()) != Some(PROFILES_DIR) {
        return None;
    }
    let root = parent.parent()?.to_path_buf();
    let name = home.file_name()?.to_str()?.to_string();
    if validate_profile_name(&name).is_err() {
        return None;
    }
    Some((root, name))
}

fn summary_for_profile(
    root: &Path,
    name: &str,
    home: &Path,
    active: &str,
) -> Result<ProfileSummary> {
    let (metadata, metadata_error) = read_metadata(home);
    Ok(ProfileSummary {
        name: name.to_string(),
        home: home.to_path_buf(),
        active: name == active,
        is_default: home == root,
        metadata,
        metadata_error,
    })
}

fn cli_profile_override() -> Option<&'static str> {
    CLI_PROFILE_OVERRIDE
        .get()
        .and_then(|name| name.as_deref())
        .filter(|name| !name.trim().is_empty())
}

fn validate_commandish_name(name: &str, label: &str) -> Result<()> {
    if name.len() > 64 {
        bail!("{label} must be 64 characters or fewer");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        bail!("{label} cannot be empty");
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        bail!("{label} must start with a lowercase ASCII letter or digit");
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_') {
        bail!("{label} may contain only lowercase ASCII letters, digits, '-' and '_'");
    }
    Ok(())
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn alias_script(profile: &str) -> String {
    if cfg!(windows) {
        format!(
            "@echo off\r\nrem psychevo-profile-alias\r\nset PEVO_PROFILE_NAME={profile}\r\npevo -p {profile} %*\r\n"
        )
    } else {
        format!(
            "#!/bin/sh\n# psychevo-profile-alias\nPEVO_PROFILE_NAME={profile}\nexport PEVO_PROFILE_NAME\nexec pevo -p {profile} \"$@\"\n"
        )
    }
}

fn reject_alias_path_conflict(
    name: &str,
    target: &Path,
    profile: &str,
    env_map: &BTreeMap<String, String>,
) -> Result<()> {
    if target.exists() && !is_managed_alias_for_profile(target, profile) {
        bail!("alias target already exists: {}", target.display());
    }
    let Some(path_env) = env_value("PATH", env_map) else {
        return Ok(());
    };
    for dir in std::env::split_paths(&path_env) {
        let candidate = alias_path(&dir, name);
        if !candidate.exists() {
            continue;
        }
        if candidate == target && is_managed_alias_for_profile(&candidate, profile) {
            continue;
        }
        bail!(
            "alias `{name}` conflicts with existing command {}",
            candidate.display()
        );
    }
    Ok(())
}

const RESERVED_PROFILE_NAMES: &[&str] = &[
    "acp", "agent", "agents", "auth", "config", "context", "doctor", "gateway", "help", "init",
    "model", "pevo", "profile", "run", "serve", "session", "setup", "skill", "skills", "stats",
    "tool", "tools", "tui", "web",
];

const RESERVED_ALIAS_NAMES: &[&str] = &[
    "cd", "clear", "cp", "env", "false", "git", "grep", "ls", "mkdir", "mv", "pwd", "rm", "sh",
    "true",
];
