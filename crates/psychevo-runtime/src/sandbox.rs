use crate::config::load_run_config;
use crate::error::{Error, Result};
use crate::paths::canonical_cwd;
use crate::types::{RunMode, RunOptions};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxMode {
    WorkspaceWrite,
    ReadOnly,
}

impl SandboxMode {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "workspace-write" => Ok(Self::WorkspaceWrite),
            "read-only" => Ok(Self::ReadOnly),
            other => Err(Error::Message(format!(
                "invalid sandbox.mode {other:?}; expected workspace-write or read-only"
            ))),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceWrite => "workspace-write",
            Self::ReadOnly => "read-only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SandboxConfig {
    pub(crate) enabled: bool,
    pub(crate) mode: SandboxMode,
    pub(crate) writable_roots: Vec<String>,
    pub(crate) include_tmp: bool,
    pub(crate) include_common_caches: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: true,
            include_common_caches: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SandboxBackend {
    Disabled,
    Seatbelt,
    Landlock,
    Unsupported,
}

impl SandboxBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Seatbelt => "seatbelt",
            Self::Landlock => "landlock",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SandboxPolicy {
    pub(crate) enabled: bool,
    pub(crate) configured_mode: SandboxMode,
    pub(crate) effective_mode: SandboxMode,
    pub(crate) platform: String,
    pub(crate) backend: SandboxBackend,
    pub(crate) writable_roots: Vec<PathBuf>,
    pub(crate) shell_extra_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SandboxWriteDecision {
    Allowed,
    Grantable { path: PathBuf, reason: String },
    Denied { reason: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SandboxWriteGrants {
    inner: Arc<Mutex<SandboxWriteGrantState>>,
}

#[derive(Debug, Default)]
struct SandboxWriteGrantState {
    once: BTreeMap<String, Vec<PathBuf>>,
    session: BTreeMap<String, Vec<PathBuf>>,
}

impl SandboxWriteGrants {
    pub(crate) fn grant_once(&self, tool_call_id: &str, paths: &[PathBuf]) -> Result<()> {
        let paths = canonicalize_grant_paths(paths)?;
        if paths.is_empty() {
            return Ok(());
        }
        if let Ok(mut state) = self.inner.lock() {
            merge_paths(
                state.once.entry(tool_call_id.to_string()).or_default(),
                paths,
            );
        }
        Ok(())
    }

    pub(crate) fn grant_session(&self, session_key: &str, paths: &[PathBuf]) -> Result<()> {
        let paths = canonicalize_grant_paths(paths)?;
        if paths.is_empty() {
            return Ok(());
        }
        if let Ok(mut state) = self.inner.lock() {
            merge_paths(
                state.session.entry(session_key.to_string()).or_default(),
                paths,
            );
        }
        Ok(())
    }

    pub(crate) fn grant_call_from_session(&self, tool_call_id: &str, session_key: &str) -> bool {
        let Ok(mut state) = self.inner.lock() else {
            return false;
        };
        let Some(paths) = state.session.get(session_key).cloned() else {
            return false;
        };
        merge_paths(
            state.once.entry(tool_call_id.to_string()).or_default(),
            paths,
        );
        true
    }

    pub(crate) fn allows_once(&self, tool_call_id: &str, path: &Path) -> Result<bool> {
        let path = canonicalize_deepest_existing(path)?;
        Ok(self.inner.lock().is_ok_and(|state| {
            state
                .once
                .get(tool_call_id)
                .is_some_and(|paths| paths.contains(&path))
        }))
    }
}

impl SandboxPolicy {
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            configured_mode: SandboxMode::WorkspaceWrite,
            effective_mode: SandboxMode::WorkspaceWrite,
            platform: platform_name(),
            backend: SandboxBackend::Disabled,
            writable_roots: Vec::new(),
            shell_extra_roots: Vec::new(),
        }
    }

    pub(crate) fn from_config(
        config: &SandboxConfig,
        cwd: &Path,
        run_mode: RunMode,
        env: &BTreeMap<String, String>,
    ) -> Result<Self> {
        if !config.enabled {
            let mut policy = Self::disabled();
            policy.configured_mode = config.mode;
            policy.effective_mode = config.mode;
            return Ok(policy);
        }

        let effective_mode = if matches!(run_mode, RunMode::Plan) {
            SandboxMode::ReadOnly
        } else {
            config.mode
        };
        let backend = backend_for_platform();
        let cwd = canonicalize_existing(cwd)?;

        let mut writable_roots = Vec::new();
        if matches!(effective_mode, SandboxMode::WorkspaceWrite) {
            push_unique(&mut writable_roots, cwd.clone());
            for root in &config.writable_roots {
                let root = path_from_config(root, cwd.as_path());
                let root = canonicalize_deepest_existing(&root)?;
                push_unique(&mut writable_roots, root);
            }
        }

        let mut shell_extra_roots = Vec::new();
        if matches!(effective_mode, SandboxMode::WorkspaceWrite) {
            if config.include_tmp {
                for root in tmp_roots(env) {
                    push_existing_unique(&mut shell_extra_roots, root)?;
                }
            }
            if config.include_common_caches {
                for root in common_cache_roots(env) {
                    push_existing_unique(&mut shell_extra_roots, root)?;
                }
            }
            for root in shell_device_sink_roots() {
                push_existing_unique(&mut shell_extra_roots, root)?;
            }
        }

        Ok(Self {
            enabled: true,
            configured_mode: config.mode,
            effective_mode,
            platform: platform_name(),
            backend,
            writable_roots,
            shell_extra_roots,
        })
    }

    pub(crate) fn ensure_shell_supported(&self) -> Result<()> {
        if self.enabled && matches!(self.backend, SandboxBackend::Unsupported) {
            return Err(sandbox_denied(format!(
                "sandbox is not supported on platform {}",
                self.platform
            )));
        }
        Ok(())
    }

    pub(crate) fn write_decision(&self, path: &Path) -> Result<SandboxWriteDecision> {
        if !self.enabled {
            return Ok(SandboxWriteDecision::Allowed);
        }
        let path = canonicalize_deepest_existing(path)?;
        if matches!(self.effective_mode, SandboxMode::ReadOnly) {
            return Ok(SandboxWriteDecision::Denied {
                reason: format!(
                    "write to {} is denied because sandbox effective mode is read-only",
                    path.display()
                ),
            });
        }
        if self
            .writable_roots
            .iter()
            .any(|root| path == *root || path.starts_with(root))
        {
            return Ok(SandboxWriteDecision::Allowed);
        }
        if let Some(root) = self
            .shell_extra_roots
            .iter()
            .find(|root| path == **root || path.starts_with(root))
        {
            return Ok(SandboxWriteDecision::Grantable {
                reason: format!(
                    "write to {} is outside configured writable roots; {} is a shell-only writable root for sandboxed shell children and does not expand write/edit",
                    path.display(),
                    root.display()
                ),
                path,
            });
        }
        Ok(SandboxWriteDecision::Grantable {
            reason: format!(
                "write to {} is outside configured writable roots",
                path.display()
            ),
            path,
        })
    }

    pub(crate) fn ensure_write_allowed(&self, path: &Path) -> Result<()> {
        match self.write_decision(path)? {
            SandboxWriteDecision::Allowed => Ok(()),
            SandboxWriteDecision::Grantable { reason, .. }
            | SandboxWriteDecision::Denied { reason } => Err(sandbox_denied(reason)),
        }
    }

    pub(crate) fn shell_writable_roots(&self) -> Vec<PathBuf> {
        let mut roots = self.writable_roots.clone();
        for root in &self.shell_extra_roots {
            push_unique(&mut roots, root.clone());
        }
        roots
    }

    pub(crate) fn env_markers(&self) -> [(&'static str, String); 4] {
        [
            (
                "PSYCHEVO_SANDBOX",
                if self.enabled { "1" } else { "0" }.to_string(),
            ),
            (
                "PSYCHEVO_SANDBOX_MODE",
                self.effective_mode.as_str().to_string(),
            ),
            (
                "PSYCHEVO_SANDBOX_BACKEND",
                self.backend.as_str().to_string(),
            ),
            ("PSYCHEVO_SANDBOX_HELPERS", "not-confined".to_string()),
        ]
    }

    pub(crate) fn status_value(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "configured_mode": self.configured_mode.as_str(),
            "effective_mode": self.effective_mode.as_str(),
            "platform": self.platform,
            "backend": self.backend.as_str(),
            "shell_enforcement": shell_enforcement(self),
            "writer_enforcement": writer_enforcement(self),
            "helper_enforcement": "not-confined",
            "network": "not-confined",
            "writable_roots": self.writable_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "shell_extra_roots": self.shell_extra_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn status_text(&self) -> String {
        let mut lines = vec![
            format!("enabled: {}", self.enabled),
            format!("configured_mode: {}", self.configured_mode.as_str()),
            format!("effective_mode: {}", self.effective_mode.as_str()),
            format!("platform: {}", self.platform),
            format!("backend: {}", self.backend.as_str()),
            format!("shell_enforcement: {}", shell_enforcement(self)),
            format!("writer_enforcement: {}", writer_enforcement(self)),
            "helper_enforcement: not-confined".to_string(),
            "network: not-confined".to_string(),
            "writable_roots:".to_string(),
        ];
        if self.writable_roots.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            lines.extend(
                self.writable_roots
                    .iter()
                    .map(|path| format!("  {}", path.display())),
            );
        }
        lines.push("shell_extra_roots:".to_string());
        if self.shell_extra_roots.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            lines.extend(
                self.shell_extra_roots
                    .iter()
                    .map(|path| format!("  {}", path.display())),
            );
        }
        lines.join("\n")
    }
}

fn canonicalize_grant_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        push_unique(&mut out, canonicalize_deepest_existing(path)?);
    }
    Ok(out)
}

fn merge_paths(target: &mut Vec<PathBuf>, paths: Vec<PathBuf>) {
    for path in paths {
        push_unique(target, path);
    }
}

pub fn sandbox_status_value(options: &RunOptions, mode: RunMode) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let policy = SandboxPolicy::from_config(&loaded.config.sandbox, &cwd, mode, &loaded.env)?;
    Ok(policy.status_value())
}

pub fn sandbox_status_text(options: &RunOptions, mode: RunMode) -> Result<String> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let policy = SandboxPolicy::from_config(&loaded.config.sandbox, &cwd, mode, &loaded.env)?;
    Ok(policy.status_text())
}

pub(crate) fn sandbox_denied(message: impl Into<String>) -> Error {
    Error::Message(format!("denied by sandbox policy: {}", message.into()))
}

pub(crate) fn canonicalize_deepest_existing(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(Error::Message("empty sandbox path".to_string()));
    }

    let mut current = path.to_path_buf();
    let mut tail = PathBuf::new();
    loop {
        if current.exists() {
            let mut resolved = current.canonicalize()?;
            if !tail.as_os_str().is_empty() {
                resolved.push(tail);
            }
            return Ok(resolved);
        }
        let Some(name) = current.file_name().map(|name| name.to_os_string()) else {
            return Err(Error::Message(format!(
                "no existing ancestor for sandbox path {}",
                path.display()
            )));
        };
        let mut next_tail = PathBuf::from(name);
        if !tail.as_os_str().is_empty() {
            next_tail.push(tail);
        }
        tail = next_tail;
        if !current.pop() {
            return Err(Error::Message(format!(
                "no existing ancestor for sandbox path {}",
                path.display()
            )));
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn apply_landlock(policy: &SandboxPolicy) -> std::io::Result<()> {
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, path_beneath_rules,
    };

    if !policy.enabled {
        return Ok(());
    }

    let abi = ABI::V5;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let writable_roots = policy.shell_writable_roots();

    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(read_access | write_access)
        .map_err(landlock_io_error)?
        .create()
        .map_err(landlock_io_error)?
        .add_rules(path_beneath_rules(["/"], read_access))
        .map_err(landlock_io_error)?;

    if !writable_roots.is_empty() {
        ruleset = ruleset
            .add_rules(path_beneath_rules(&writable_roots, write_access))
            .map_err(landlock_io_error)?;
    }

    let status = ruleset
        .no_new_privs(true)
        .restrict_self()
        .map_err(landlock_io_error)?;
    if matches!(status.ruleset, RulesetStatus::NotEnforced) {
        return Err(std::io::Error::other(
            "landlock did not enforce the sandbox ruleset",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn seatbelt_profile(policy: &SandboxPolicy) -> String {
    let mut lines = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(allow process*)".to_string(),
        "(allow signal (target self))".to_string(),
        "(allow sysctl*)".to_string(),
        "(allow mach*)".to_string(),
        "(allow file-read*)".to_string(),
        "(allow network*)".to_string(),
    ];
    for root in policy.shell_writable_roots() {
        lines.push(format!(
            "(allow file-write* (subpath \"{}\"))",
            sbpl_escape(&root)
        ));
    }
    lines.join("\n")
}

#[cfg(target_os = "macos")]
fn sbpl_escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn landlock_io_error<E: std::fmt::Display>(err: E) -> std::io::Error {
    std::io::Error::other(format!("landlock setup failed: {err}"))
}

fn shell_enforcement(policy: &SandboxPolicy) -> &'static str {
    if !policy.enabled {
        "disabled"
    } else if matches!(policy.backend, SandboxBackend::Unsupported) {
        "unsupported"
    } else {
        "confined"
    }
}

fn writer_enforcement(policy: &SandboxPolicy) -> &'static str {
    if policy.enabled {
        "confined"
    } else {
        "disabled"
    }
}

fn backend_for_platform() -> SandboxBackend {
    #[cfg(target_os = "linux")]
    {
        SandboxBackend::Landlock
    }
    #[cfg(target_os = "macos")]
    {
        SandboxBackend::Seatbelt
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        SandboxBackend::Unsupported
    }
}

fn platform_name() -> String {
    #[cfg(target_os = "linux")]
    {
        if std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|text| text.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
        {
            "wsl2".to_string()
        } else {
            "linux".to_string()
        }
    }
    #[cfg(target_os = "macos")]
    {
        "macos".to_string()
    }
    #[cfg(windows)]
    {
        "windows".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        std::env::consts::OS.to_string()
    }
}

fn path_from_config(raw: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    Ok(path.canonicalize()?)
}

fn push_existing_unique(roots: &mut Vec<PathBuf>, path: PathBuf) -> Result<()> {
    if path.exists() {
        let path = path.canonicalize()?;
        push_unique(roots, path);
    }
    Ok(())
}

fn push_unique(roots: &mut Vec<PathBuf>, path: PathBuf) {
    let existing: BTreeSet<_> = roots.iter().cloned().collect();
    if !existing.contains(&path) {
        roots.push(path);
    }
}

fn tmp_roots(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = env.get(key).filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(value));
        }
    }
    roots.push(std::env::temp_dir());
    roots
}

fn common_cache_roots(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir(env) {
        roots.push(
            env.get("XDG_CACHE_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache")),
        );
        roots.push(
            env.get("CARGO_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cargo")),
        );
        roots.push(
            env.get("RUSTUP_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".rustup")),
        );
        roots.push(
            env.get("NPM_CONFIG_CACHE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".npm")),
        );
        roots.push(
            env.get("PNPM_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".pnpm-store")),
        );
        roots.push(
            env.get("YARN_CACHE_FOLDER")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".yarn")),
        );
        roots.push(
            env.get("PIP_CACHE_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache/pip")),
        );
        roots.push(
            env.get("GRADLE_USER_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".gradle"))
                .join("caches"),
        );
        roots.push(home.join(".m2/repository"));
    }

    if let Some(value) = env.get("GOCACHE").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value));
    }
    if let Some(value) = env.get("GOMODCACHE").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value));
    } else if let Some(value) = env.get("GOPATH").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value).join("pkg/mod"));
    }
    roots
}

fn home_dir(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    env.get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

const SHELL_DEVICE_SINK_ROOTS: &[&str] = &["/dev/null", "/dev/zero"];

fn shell_device_sink_roots() -> impl Iterator<Item = PathBuf> {
    SHELL_DEVICE_SINK_ROOTS
        .iter()
        .map(|path| PathBuf::from(*path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn plan_mode_forces_read_only_policy() {
        let dir = tempdir().unwrap();
        let env = BTreeMap::new();
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy = SandboxPolicy::from_config(&config, dir.path(), RunMode::Plan, &env).unwrap();

        assert_eq!(policy.configured_mode, SandboxMode::WorkspaceWrite);
        assert_eq!(policy.effective_mode, SandboxMode::ReadOnly);
        assert!(policy.writable_roots.is_empty());
        assert!(
            policy
                .ensure_write_allowed(&dir.path().join("file.txt"))
                .unwrap_err()
                .to_string()
                .contains("read-only")
        );
        assert!(
            policy.shell_extra_roots.is_empty(),
            "read-only policy should not add shell-only device sinks"
        );
    }

    #[test]
    fn workspace_write_allows_workspace_and_denies_outside() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let env = BTreeMap::new();
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy =
            SandboxPolicy::from_config(&config, dir.path(), RunMode::Default, &env).unwrap();

        policy
            .ensure_write_allowed(&dir.path().join("nested/new.txt"))
            .unwrap();
        let err = policy
            .ensure_write_allowed(&outside.path().join("new.txt"))
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("outside configured writable roots")
        );
    }

    #[cfg(unix)]
    #[test]
    fn workspace_write_adds_device_sinks_only_for_shells() {
        let dir = tempdir().unwrap();
        let env = BTreeMap::new();
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy =
            SandboxPolicy::from_config(&config, dir.path(), RunMode::Default, &env).unwrap();
        let expected = shell_device_sink_roots()
            .filter(|path| path.exists())
            .map(|path| path.canonicalize().unwrap())
            .collect::<Vec<_>>();

        assert!(
            !expected.is_empty(),
            "expected at least one standard device sink to exist"
        );
        for root in expected {
            assert!(
                policy.shell_extra_roots.contains(&root),
                "shell_extra_roots should include {}",
                root.display()
            );
            assert!(
                !policy.writable_roots.contains(&root),
                "writable_roots should not include {}",
                root.display()
            );
            assert!(
                policy.ensure_write_allowed(&root).is_err(),
                "built-in writer policy should still deny {}",
                root.display()
            );
        }
    }

    #[test]
    fn disabled_policy_preserves_configured_mode_for_status() {
        let dir = tempdir().unwrap();
        let env = BTreeMap::new();
        let config = SandboxConfig {
            enabled: false,
            mode: SandboxMode::ReadOnly,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy =
            SandboxPolicy::from_config(&config, dir.path(), RunMode::Default, &env).unwrap();

        assert!(!policy.enabled);
        assert_eq!(policy.configured_mode, SandboxMode::ReadOnly);
        assert_eq!(policy.effective_mode, SandboxMode::ReadOnly);
        assert_eq!(policy.backend, SandboxBackend::Disabled);
    }
}
