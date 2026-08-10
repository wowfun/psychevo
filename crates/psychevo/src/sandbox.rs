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
pub(crate) enum SandboxBackend {
    Disabled,
    #[cfg(target_os = "macos")]
    Seatbelt,
    #[cfg(any(target_os = "linux", test))]
    Landlock,
    #[cfg(any(windows, test))]
    WindowsRestricted,
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Unsupported,
}

impl SandboxBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            #[cfg(target_os = "macos")]
            Self::Seatbelt => "seatbelt",
            #[cfg(any(target_os = "linux", test))]
            Self::Landlock => "landlock",
            #[cfg(any(windows, test))]
            Self::WindowsRestricted => "windows-restricted-token-advisory",
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    workspace_root_identities: Vec<crate::filesystem_identity::CapturedDirectoryIdentity>,
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
    turn_roots: Arc<Mutex<Vec<crate::filesystem_identity::CapturedDirectoryIdentity>>>,
    authorized_files:
        Arc<Mutex<BTreeMap<String, Vec<crate::filesystem_identity::CapturedFileTarget>>>>,
}

#[derive(Debug, Default)]
struct SandboxWriteGrantState {
    once: BTreeMap<String, Vec<PathBuf>>,
    session: BTreeMap<String, Vec<PathBuf>>,
    session_roots: Vec<crate::filesystem_identity::CapturedDirectoryIdentity>,
}

impl SandboxWriteGrants {
    pub(crate) fn install_authorized_files(
        &self,
        tool_call_id: &str,
        targets: Vec<crate::filesystem_identity::CapturedFileTarget>,
    ) {
        if let Ok(mut files) = self.authorized_files.lock() {
            files.insert(tool_call_id.to_string(), targets);
        }
    }

    pub(crate) fn take_authorized_files(
        &self,
        tool_call_id: &str,
    ) -> Option<Vec<crate::filesystem_identity::CapturedFileTarget>> {
        self.authorized_files.lock().ok()?.remove(tool_call_id)
    }

    pub(crate) fn clear_authorized_files(&self, tool_call_id: &str) {
        if let Ok(mut files) = self.authorized_files.lock() {
            files.remove(tool_call_id);
        }
    }

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

    pub(crate) fn grant_scope(&self, scope: &crate::types::FilesystemApprovalScope) -> Result<()> {
        let requested_root = PathBuf::from(&scope.directory);
        let root = crate::filesystem_identity::CapturedDirectoryIdentity::capture(&requested_root)?;
        if root.path() != requested_root {
            return Err(Error::Message(
                "path_identity_changed: approved directory identity changed before grant"
                    .to_string(),
            ));
        }
        match scope.lifetime {
            crate::types::FilesystemApprovalLifetime::Turn => {
                if let Ok(mut roots) = self.turn_roots.lock() {
                    push_unique_identity(&mut roots, root);
                }
            }
            crate::types::FilesystemApprovalLifetime::Session => {
                if let Ok(mut state) = self.inner.lock() {
                    push_unique_identity(&mut state.session_roots, root);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn with_turn_scopes_from(mut self, source: &Self) -> Self {
        self.turn_roots = Arc::clone(&source.turn_roots);
        self
    }
    pub(crate) fn grant_call_from_scopes(
        &self,
        tool_call_id: &str,
        paths: &[PathBuf],
    ) -> Result<bool> {
        let paths = canonicalize_grant_paths(paths)?;
        let turn_roots = self
            .turn_roots
            .lock()
            .map(|roots| roots.clone())
            .unwrap_or_default();
        let Ok(mut state) = self.inner.lock() else {
            return Ok(false);
        };
        let roots = turn_roots
            .iter()
            .chain(state.session_roots.iter())
            .collect::<Vec<_>>();
        for root in &roots {
            root.validate()?;
        }
        let allowed = paths.iter().all(|path| {
            roots
                .iter()
                .any(|root| crate::filesystem_identity::is_within(root.path(), path))
        });
        if allowed {
            merge_paths(
                state.once.entry(tool_call_id.to_string()).or_default(),
                paths,
            );
        }
        Ok(allowed)
    }

    pub(crate) fn scoped_roots(&self) -> Vec<PathBuf> {
        let turn_roots = self
            .turn_roots
            .lock()
            .map(|roots| roots.clone())
            .unwrap_or_default();
        let Ok(state) = self.inner.lock() else {
            return turn_roots
                .into_iter()
                .filter_map(|root| root.validate().ok().map(|_| root.path().to_path_buf()))
                .collect();
        };
        let mut roots = Vec::new();
        for root in turn_roots.iter().chain(state.session_roots.iter()) {
            if root.validate().is_ok() {
                push_unique(&mut roots, root.path().to_path_buf());
            }
        }
        roots
    }

    pub(crate) fn clear_turn_scopes(&self) {
        if let Ok(mut roots) = self.turn_roots.lock() {
            roots.clear();
        }
    }

    pub(crate) fn clear_session_scopes(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.session_roots.clear();
            state.session.clear();
        }
        self.clear_turn_scopes();
    }

    pub(crate) fn allows_once(&self, tool_call_id: &str, path: &Path) -> Result<bool> {
        let path = crate::filesystem_identity::canonicalize_deepest_existing(path)?;
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
            workspace_root_identities: Vec::new(),
        }
    }

    pub(crate) fn from_config(
        config: &SandboxConfig,
        cwd: &Path,
        run_mode: RunMode,
        env: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let plan_mode = matches!(run_mode, RunMode::Plan);
        if !config.enabled && !plan_mode {
            let mut policy = Self::disabled();
            policy.configured_mode = config.mode;
            policy.effective_mode = config.mode;
            return Ok(policy);
        }

        let effective_mode = if plan_mode {
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
                let root = crate::filesystem_identity::canonicalize_deepest_existing(&root)?;
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

        let workspace_root_identities = if matches!(effective_mode, SandboxMode::WorkspaceWrite) {
            vec![crate::filesystem_identity::CapturedDirectoryIdentity::capture(&cwd)?]
        } else {
            Vec::new()
        };
        Ok(Self {
            enabled: true,
            configured_mode: config.mode,
            effective_mode,
            platform: platform_name(),
            backend,
            writable_roots,
            shell_extra_roots,
            workspace_root_identities,
        })
    }

    pub(crate) fn narrowed_for_run_mode(mut self, run_mode: RunMode) -> Self {
        if matches!(run_mode, RunMode::Plan) {
            self.enabled = true;
            self.effective_mode = SandboxMode::ReadOnly;
            self.backend = backend_for_platform();
            self.writable_roots.clear();
            self.shell_extra_roots.clear();
            self.workspace_root_identities.clear();
        }
        self
    }

    pub(crate) fn with_approval_roots(mut self, roots: Vec<PathBuf>) -> Self {
        if self.enabled && matches!(self.effective_mode, SandboxMode::WorkspaceWrite) {
            for root in roots {
                push_unique(&mut self.writable_roots, root);
            }
        }
        self
    }

    pub(crate) fn with_workspace_roots(mut self, roots: &[PathBuf]) -> Result<Self> {
        if self.enabled && matches!(self.effective_mode, SandboxMode::WorkspaceWrite) {
            for root in roots {
                let identity =
                    crate::filesystem_identity::CapturedDirectoryIdentity::capture(root)?;
                push_unique(&mut self.writable_roots, identity.path().to_path_buf());
                if !self
                    .workspace_root_identities
                    .iter()
                    .any(|current| current.path() == identity.path())
                {
                    self.workspace_root_identities.push(identity);
                }
            }
        }
        Ok(self)
    }

    pub(crate) fn with_workspace_root_capture(
        mut self,
        capture: &crate::filesystem_identity::WorkspaceRootCapture,
    ) -> Self {
        if self.enabled && matches!(self.effective_mode, SandboxMode::WorkspaceWrite) {
            self.workspace_root_identities = capture.identities().to_vec();
            for identity in capture.identities() {
                push_unique(&mut self.writable_roots, identity.path().to_path_buf());
            }
        }
        self
    }

    #[cfg(test)]
    pub(crate) fn ensure_shell_supported(&self) -> Result<()> {
        self.validate_workspace_root_identities()?;
        self.ensure_shell_platform_supported()
    }

    pub(crate) async fn ensure_shell_supported_async(&self) -> Result<()> {
        let identities = self.workspace_root_identities.clone();
        tokio::task::spawn_blocking(move || {
            for identity in identities {
                identity.validate()?;
            }
            Ok::<(), Error>(())
        })
        .await
        .map_err(|error| Error::Message(format!("sandbox root validation failed: {error}")))??;
        self.ensure_shell_platform_supported()
    }

    pub(crate) fn ensure_shell_platform_supported(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if self.enabled && matches!(self.effective_mode, SandboxMode::WorkspaceWrite) {
            return Err(sandbox_denied(
                "macOS Seatbelt cannot bind workspace-write rules to captured directory identities",
            ));
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        if self.enabled && matches!(self.backend, SandboxBackend::Unsupported) {
            return Err(sandbox_denied(format!(
                "sandbox is not supported on platform {}",
                self.platform
            )));
        }
        #[cfg(any(windows, test))]
        if self.enabled
            && matches!(self.backend, SandboxBackend::WindowsRestricted)
            && !matches!(self.effective_mode, SandboxMode::ReadOnly)
        {
            return Err(sandbox_denied(
                "the native Windows restricted-token backend currently enforces read-only Plan execution only",
            ));
        }
        Ok(())
    }

    pub(crate) fn write_decision(&self, path: &Path) -> Result<SandboxWriteDecision> {
        if !self.enabled {
            return Ok(SandboxWriteDecision::Allowed);
        }
        self.validate_workspace_root_identities()?;
        let path = crate::filesystem_identity::canonicalize_deepest_existing(path)?;
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
                    "write is outside configured writable roots; {} is a shell-only writable root for sandboxed shell children and does not expand write/edit",
                    root.display()
                ),
                path,
            });
        }
        Ok(SandboxWriteDecision::Grantable {
            reason: "write is outside configured writable roots".to_string(),
            path,
        })
    }

    pub(crate) fn ensure_write_allowed(&self, path: &Path) -> Result<()> {
        match self.write_decision(path)? {
            SandboxWriteDecision::Allowed => Ok(()),
            SandboxWriteDecision::Grantable { path, reason } => Err(sandbox_denied(format!(
                "write to {} is denied: {reason}",
                path.display()
            ))),
            SandboxWriteDecision::Denied { reason } => Err(sandbox_denied(reason)),
        }
    }

    pub(crate) fn ensure_stdin_write_allowed(&self) -> Result<()> {
        if matches!(self.effective_mode, SandboxMode::ReadOnly) {
            return Err(sandbox_denied(
                "read-only mode forbids non-empty write_stdin input",
            ));
        }
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn shell_writable_roots(&self) -> Vec<PathBuf> {
        let mut roots = self.writable_roots.clone();
        for root in &self.shell_extra_roots {
            push_unique(&mut roots, root.clone());
        }
        roots
    }

    pub(crate) fn validate_workspace_root_identities(&self) -> Result<()> {
        for identity in &self.workspace_root_identities {
            identity.validate()?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn non_workspace_shell_writable_roots(&self) -> Vec<PathBuf> {
        self.shell_writable_roots()
            .into_iter()
            .filter(|root| {
                !self
                    .workspace_root_identities
                    .iter()
                    .any(|identity| identity.path() == root)
            })
            .collect()
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
        push_unique(
            &mut out,
            crate::filesystem_identity::canonicalize_deepest_existing(path)?,
        );
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
    let policy = SandboxPolicy::from_config(&loaded.config.sandbox, &cwd, mode, &loaded.env)?
        .with_workspace_roots(&options.workspace_roots)?;
    Ok(policy.status_value())
}

pub fn sandbox_status_text(options: &RunOptions, mode: RunMode) -> Result<String> {
    let cwd = canonical_cwd(&options.cwd)?;
    let loaded = load_run_config(options, &cwd)?;
    let policy = SandboxPolicy::from_config(&loaded.config.sandbox, &cwd, mode, &loaded.env)?
        .with_workspace_roots(&options.workspace_roots)?;
    Ok(policy.status_text())
}

pub(crate) fn sandbox_denied(message: impl Into<String>) -> Error {
    Error::Message(format!("denied by sandbox policy: {}", message.into()))
}

#[cfg(target_os = "linux")]
pub(crate) fn apply_landlock(policy: &SandboxPolicy) -> std::io::Result<()> {
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, Ruleset, RulesetAttr,
        RulesetCreatedAttr, path_beneath_rules,
    };

    if !policy.enabled {
        return Ok(());
    }

    let abi = ABI::V3;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let writable_roots = policy.non_workspace_shell_writable_roots();

    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
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
    for identity in &policy.workspace_root_identities {
        let root = identity.open_verified()?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(root, write_access))
            .map_err(landlock_io_error)?;
    }

    let status = ruleset
        .no_new_privs(true)
        .restrict_self()
        .map_err(landlock_io_error)?;
    ensure_landlock_fully_enforced(status.ruleset)
}

#[cfg(target_os = "linux")]
fn ensure_landlock_fully_enforced(status: landlock::RulesetStatus) -> std::io::Result<()> {
    if matches!(status, landlock::RulesetStatus::FullyEnforced) {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "landlock did not fully enforce the sandbox ruleset",
        ))
    }
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
    } else if policy.ensure_shell_platform_supported().is_err() {
        "unsupported"
    } else {
        match policy.backend {
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
            SandboxBackend::Unsupported => "unsupported",
            #[cfg(any(windows, test))]
            SandboxBackend::WindowsRestricted => "not-confined",
            _ => "confined",
        }
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
    #[cfg(windows)]
    {
        SandboxBackend::WindowsRestricted
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    Ok(crate::host_paths::normalized_native_path(
        &path.canonicalize()?,
    ))
}

fn push_existing_unique(roots: &mut Vec<PathBuf>, path: PathBuf) -> Result<()> {
    if path.exists() {
        let path = canonicalize_existing(&path)?;
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

fn push_unique_identity(
    roots: &mut Vec<crate::filesystem_identity::CapturedDirectoryIdentity>,
    identity: crate::filesystem_identity::CapturedDirectoryIdentity,
) {
    if roots
        .iter()
        .all(|current| current.path() != identity.path())
    {
        roots.push(identity);
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
    fn plan_mode_enables_sandbox_even_when_the_configured_baseline_is_disabled() {
        let dir = tempdir().unwrap();
        let env = BTreeMap::new();
        let config = SandboxConfig {
            enabled: false,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: vec![dir.path().display().to_string()],
            include_tmp: true,
            include_common_caches: true,
        };

        let policy = SandboxPolicy::from_config(&config, dir.path(), RunMode::Plan, &env).unwrap();

        assert!(policy.enabled);
        assert_eq!(policy.configured_mode, SandboxMode::WorkspaceWrite);
        assert_eq!(policy.effective_mode, SandboxMode::ReadOnly);
        assert!(policy.writable_roots.is_empty());
        assert!(policy.shell_extra_roots.is_empty());
        assert!(!matches!(policy.backend, SandboxBackend::Disabled));
    }

    #[test]
    fn workspace_write_policy_includes_all_runtime_roots() {
        let temp = tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        std::fs::create_dir_all(&primary).expect("primary");
        std::fs::create_dir_all(&secondary).expect("secondary");
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy =
            SandboxPolicy::from_config(&config, &primary, RunMode::Default, &BTreeMap::new())
                .expect("base policy")
                .with_workspace_roots(std::slice::from_ref(&secondary))
                .expect("Workspace roots");

        assert_eq!(
            policy
                .write_decision(&secondary.join("file.txt"))
                .expect("decision"),
            SandboxWriteDecision::Allowed
        );
    }

    #[cfg(unix)]
    #[test]
    fn workspace_write_policy_rejects_a_recreated_root_before_spawn() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("root");
        let original = temp.path().join("original");
        std::fs::create_dir(&root).expect("root");
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };
        let policy = SandboxPolicy::from_config(&config, &root, RunMode::Default, &BTreeMap::new())
            .expect("policy")
            .with_workspace_roots(std::slice::from_ref(&root))
            .expect("Workspace root");
        std::fs::rename(&root, &original).expect("retain original");
        std::fs::create_dir(&root).expect("replacement");

        let error = policy
            .validate_workspace_root_identities()
            .expect_err("replacement must fail closed");

        assert!(error.to_string().contains("path_identity_changed"));
    }

    #[test]
    fn filesystem_scope_grant_does_not_transfer_to_a_recreated_directory() {
        let temp = tempdir().expect("temp");
        let granted = temp.path().join("granted");
        let original = temp.path().join("original");
        std::fs::create_dir(&granted).expect("granted");
        let grants = SandboxWriteGrants::default();
        grants
            .grant_scope(&crate::types::FilesystemApprovalScope {
                directory: granted.display().to_string(),
                lifetime: crate::types::FilesystemApprovalLifetime::Session,
            })
            .expect("scope grant");
        std::fs::rename(&granted, &original).expect("retain original");
        std::fs::create_dir(&granted).expect("replacement");

        let error = grants
            .grant_call_from_scopes("write", &[granted.join("file.txt")])
            .expect_err("replacement must revoke the grant");

        assert!(error.to_string().contains("path_identity_changed"));
        assert!(grants.scoped_roots().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn invocation_policy_reuses_the_pre_acceptance_root_capture() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("root");
        let original = temp.path().join("original");
        std::fs::create_dir(&root).expect("root");
        let capture =
            crate::filesystem_identity::WorkspaceRootCapture::capture(std::slice::from_ref(&root))
                .expect("admission capture");
        std::fs::rename(&root, &original).expect("retain original");
        std::fs::create_dir(&root).expect("replacement");
        let config = SandboxConfig {
            enabled: true,
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: Vec::new(),
            include_tmp: false,
            include_common_caches: false,
        };

        let policy = SandboxPolicy::from_config(&config, &root, RunMode::Default, &BTreeMap::new())
            .expect("invocation config")
            .with_workspace_root_capture(&capture);

        let error = policy
            .ensure_shell_supported()
            .expect_err("replacement must not become the invocation baseline");
        assert!(error.to_string().contains("path_identity_changed"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_partial_or_missing_enforcement_fails_closed() {
        assert!(
            ensure_landlock_fully_enforced(landlock::RulesetStatus::PartiallyEnforced).is_err()
        );
        assert!(ensure_landlock_fully_enforced(landlock::RulesetStatus::NotEnforced).is_err());
        ensure_landlock_fully_enforced(landlock::RulesetStatus::FullyEnforced)
            .expect("full enforcement");
    }

    #[test]
    fn child_plan_narrowing_clears_parent_writable_and_extra_roots() {
        let dir = tempdir().unwrap();
        let policy = SandboxPolicy {
            enabled: false,
            configured_mode: SandboxMode::WorkspaceWrite,
            effective_mode: SandboxMode::WorkspaceWrite,
            platform: platform_name(),
            backend: SandboxBackend::Disabled,
            writable_roots: vec![dir.path().to_path_buf()],
            shell_extra_roots: vec![dir.path().join("tmp")],
            workspace_root_identities: Vec::new(),
        }
        .narrowed_for_run_mode(RunMode::Plan);

        assert!(policy.enabled);
        assert_eq!(policy.effective_mode, SandboxMode::ReadOnly);
        assert!(policy.writable_roots.is_empty());
        assert!(policy.shell_extra_roots.is_empty());
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

    #[test]
    fn windows_restricted_backend_reports_advisory_shell_enforcement() {
        let policy = SandboxPolicy {
            enabled: true,
            configured_mode: SandboxMode::ReadOnly,
            effective_mode: SandboxMode::ReadOnly,
            platform: "windows".to_string(),
            backend: SandboxBackend::WindowsRestricted,
            writable_roots: Vec::new(),
            shell_extra_roots: Vec::new(),
            workspace_root_identities: Vec::new(),
        };

        assert_eq!(shell_enforcement(&policy), "not-confined");
        assert_eq!(
            policy.status_value()["shell_enforcement"],
            serde_json::json!("not-confined")
        );
    }

    #[test]
    fn status_reports_unsupported_when_spawn_support_check_rejects_policy() {
        let policy = SandboxPolicy {
            enabled: true,
            configured_mode: SandboxMode::WorkspaceWrite,
            effective_mode: SandboxMode::WorkspaceWrite,
            platform: "windows".to_string(),
            backend: SandboxBackend::WindowsRestricted,
            writable_roots: Vec::new(),
            shell_extra_roots: Vec::new(),
            workspace_root_identities: Vec::new(),
        };

        assert!(policy.ensure_shell_supported().is_err());
        assert_eq!(shell_enforcement(&policy), "unsupported");
        assert_eq!(
            policy.status_value()["shell_enforcement"],
            serde_json::json!("unsupported")
        );
    }
}
