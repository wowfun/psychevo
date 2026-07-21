use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use psychevo_runtime::host_process::{
    ManagedProcess, ProcessIdentityError, atomic_replace_private, instance_lease_is_held,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::context::GatewayContext;

const GATEWAY_DIR: &str = "gateway";
const MANAGED_GATEWAY_DEFAULT_PORT: u16 = 58_080;
const MANAGED_GATEWAY_FALLBACK_PORTS: u16 = 19;
const MANAGED_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MANAGED_STARTUP_LOG_EXCERPT_BYTES: u64 = 16 * 1024;
const MANAGED_STOP_GRACE_TIMEOUT: Duration = Duration::from_secs(15);
const MANAGED_STOP_FORCE_TIMEOUT: Duration = Duration::from_secs(2);
const MANAGED_IDENTITY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub(super) struct ManagedPaths {
    dir: PathBuf,
    server_json: PathBuf,
    token: PathBuf,
    lock: PathBuf,
    instance_lock: PathBuf,
    log: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedServerState {
    pub(super) instance_id: Option<String>,
    pub(super) pid: u32,
    pub(super) base_url: String,
    pub(super) readyz_url: String,
    pub(super) started_at_ms: i64,
    pub(super) version: String,
    pub(super) executable_path: Option<String>,
    pub(super) executable_modified_ms: Option<i64>,
    pub(super) executable_size: Option<u64>,
    pub(super) executable_inode: Option<u64>,
    pub(super) static_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExecutableFingerprint {
    pub(super) path: String,
    pub(super) modified_ms: i64,
    pub(super) size: u64,
    pub(super) inode: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedReuseTarget {
    executable: ExecutableFingerprint,
    static_dir: String,
}

struct ManagedLaunch {
    child: Child,
    instance_id: String,
    log_offset: u64,
}

pub(super) struct ManagedLifecycleLock {
    _file: File,
}

#[derive(Debug)]
enum StateSnapshot {
    Missing,
    Invalid,
    Valid(Box<ManagedServerState>),
}

#[derive(Debug)]
enum ProcessOwnership {
    Owned(ManagedProcess),
    Stale(&'static str),
    Unavailable(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProcessExecutable {
    pub(super) path: String,
    pub(super) inode: Option<u64>,
    pub(super) deleted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ManagedBindPolicy {
    bind_addr: SocketAddr,
    fallback_ports: u16,
}

impl ManagedBindPolicy {
    pub(super) fn new(explicit: Option<SocketAddr>) -> Self {
        match explicit {
            Some(bind_addr) => Self {
                bind_addr,
                fallback_ports: 0,
            },
            None => Self {
                bind_addr: default_managed_bind_addr(),
                fallback_ports: MANAGED_GATEWAY_FALLBACK_PORTS,
            },
        }
    }

    pub(super) fn bind_addr(self) -> SocketAddr {
        self.bind_addr
    }

    pub(super) fn fallback_ports(self) -> u16 {
        self.fallback_ports
    }

    pub(super) fn allows_bound_addr(self, addr: SocketAddr) -> bool {
        if self.bind_addr.port() == 0 {
            return addr.ip() == self.bind_addr.ip() && addr.port() != 0;
        }
        if self.fallback_ports == 0 {
            return addr == self.bind_addr;
        }
        addr.ip() == self.bind_addr.ip()
            && addr.port() >= self.bind_addr.port()
            && addr.port() <= self.bind_addr.port().saturating_add(self.fallback_ports)
    }
}

fn default_managed_bind_addr() -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        MANAGED_GATEWAY_DEFAULT_PORT,
    )
}

pub(super) async fn ensure_started(
    ctx: &GatewayContext,
    bind_policy: ManagedBindPolicy,
    static_dir: &Path,
) -> Result<ManagedServerState> {
    let target = managed_reuse_target(static_dir)?;
    match read_state_snapshot(&ctx.paths)? {
        StateSnapshot::Missing => {
            ensure_idle_lease(&ctx.paths, "managed state is missing")?;
        }
        StateSnapshot::Invalid => {
            ensure_idle_lease(&ctx.paths, "managed state is invalid")?;
        }
        StateSnapshot::Valid(state) => {
            let state = *state;
            let Some(instance_id) = state.instance_id.as_deref() else {
                ensure_idle_lease(&ctx.paths, "managed state is missing instanceId")?;
                return start_new_instance(ctx, bind_policy, static_dir).await;
            };
            match inspect_process_ownership(&ctx.paths, &state, instance_id)? {
                ProcessOwnership::Stale(_) => {}
                ProcessOwnership::Unavailable(reason) => {
                    return Err(anyhow!(
                        "managed gateway ownership cannot be proven ({reason}); refusing to signal the recorded pid or start a second instance"
                    ));
                }
                ProcessOwnership::Owned(process) => {
                    let process_executable = process_executable(state.pid);
                    let stale_reason = managed_stale_reason(
                        &state,
                        true,
                        Some(&target.executable),
                        Some(target.static_dir.as_str()),
                        Some(&bind_policy),
                        process_executable.as_ref(),
                    )
                    .or(gateway_identity_stale_reason(&state, &ctx.paths).await);
                    if stale_reason.is_none() {
                        return Ok(state);
                    }
                    stop_owned_instance(&state, &ctx.paths, &process).await?;
                }
            }
        }
    }
    start_new_instance(ctx, bind_policy, static_dir).await
}

async fn start_new_instance(
    ctx: &GatewayContext,
    bind_policy: ManagedBindPolicy,
    static_dir: &Path,
) -> Result<ManagedServerState> {
    cleanup_state(&ctx.paths)?;
    rotate_token(&ctx.paths)?;
    let launch = spawn_serve(ctx, bind_policy, static_dir)?;
    wait_for_state(&ctx.paths, launch).await
}

fn ensure_idle_lease(paths: &ManagedPaths, context: &str) -> Result<()> {
    if instance_lease_is_held(&paths.instance_lock)? {
        return Err(anyhow!(
            "{context} while the managed instance lease is held; refusing to start a second instance"
        ));
    }
    Ok(())
}

fn inspect_process_ownership(
    paths: &ManagedPaths,
    state: &ManagedServerState,
    instance_id: &str,
) -> Result<ProcessOwnership> {
    if !instance_lease_is_held(&paths.instance_lock)? {
        return Ok(ProcessOwnership::Stale("instance_lease_missing"));
    }
    Ok(match ManagedProcess::inspect(state.pid, instance_id) {
        Ok(process) => ProcessOwnership::Owned(process),
        Err(ProcessIdentityError::Dead) => ProcessOwnership::Unavailable("pid_not_running"),
        Err(ProcessIdentityError::Mismatch(_)) => {
            ProcessOwnership::Unavailable("process_identity_mismatch")
        }
        Err(ProcessIdentityError::Unavailable(_)) => {
            ProcessOwnership::Unavailable("process_identity_unavailable")
        }
    })
}

fn spawn_serve(
    ctx: &GatewayContext,
    bind_policy: ManagedBindPolicy,
    static_dir: &Path,
) -> Result<ManagedLaunch> {
    let instance_id = Uuid::now_v7().to_string();
    let exe = env::current_exe().context("resolve pevo executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.paths.log)?;
    let log_offset = log.metadata()?.len();
    let log_err = log.try_clone()?;
    let mut command = Command::new(exe);
    command
        .arg("serve")
        .arg("--bind")
        .arg(bind_policy.bind_addr().to_string())
        .arg("--token-file")
        .arg(&ctx.paths.token)
        .arg("--internal-static-dir")
        .arg(static_dir)
        .arg("--internal-managed-state")
        .arg(&ctx.paths.server_json)
        .arg("--internal-managed-instance")
        .arg(&instance_id)
        .arg("--internal-managed-lease")
        .arg(&ctx.paths.instance_lock);
    if bind_policy.fallback_ports() > 0 {
        command
            .arg("--internal-bind-fallbacks")
            .arg(bind_policy.fallback_ports().to_string());
    }
    command
        .stdin(Stdio::null())
        .env("PSYCHEVO_HOME", &ctx.home)
        .env(crate::profiles::PROFILE_ENV, &ctx.profile_name)
        .env(crate::profiles::PROFILE_HOME_ENV, &ctx.home)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let child = command.spawn().context("spawn pevo serve")?;
    Ok(ManagedLaunch {
        child,
        instance_id,
        log_offset,
    })
}

async fn wait_for_state(
    paths: &ManagedPaths,
    mut launch: ManagedLaunch,
) -> Result<ManagedServerState> {
    let started = Instant::now();
    while started.elapsed() < MANAGED_STARTUP_TIMEOUT {
        if let StateSnapshot::Valid(state) = read_state_snapshot(paths)?
            && state.instance_id.as_deref() == Some(launch.instance_id.as_str())
            && state.pid == launch.child.id()
            && matches!(
                inspect_process_ownership(paths, &state, &launch.instance_id)?,
                ProcessOwnership::Owned(_)
            )
            && gateway_identity_stale_reason(&state, paths).await.is_none()
        {
            return Ok(*state);
        }
        if let Some(status) = launch
            .child
            .try_wait()
            .context("poll managed gateway process")?
        {
            cleanup_server_state_for_instance(paths, &launch.instance_id)?;
            return Err(managed_startup_error(
                paths,
                launch.log_offset,
                Some(status),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let startup_error = managed_startup_error(paths, launch.log_offset, None);
    let termination = terminate_launch(&mut launch);
    let cleanup = cleanup_server_state_for_instance(paths, &launch.instance_id);
    if let Err(error) = termination {
        return Err(anyhow!(
            "{startup_error}\nfailed to terminate timed-out managed instance: {error:#}"
        ));
    }
    cleanup?;
    Err(startup_error)
}

fn terminate_launch(launch: &mut ManagedLaunch) -> Result<()> {
    if let Ok(process) = ManagedProcess::inspect(launch.child.id(), &launch.instance_id) {
        process.terminate_tree(1)?;
        if !process.wait_for_exit(MANAGED_STOP_FORCE_TIMEOUT)?
            || !process.wait_for_tree_exit(MANAGED_STOP_FORCE_TIMEOUT)?
        {
            return Err(anyhow!(
                "managed startup process tree did not exit after forced termination"
            ));
        }
    } else {
        launch
            .child
            .kill()
            .context("terminate exact managed startup child")?;
    }
    let _ = launch.child.wait();
    Ok(())
}

fn cleanup_server_state_for_instance(paths: &ManagedPaths, instance_id: &str) -> Result<()> {
    if let StateSnapshot::Valid(state) = read_state_snapshot(paths)?
        && state.instance_id.as_deref() != Some(instance_id)
    {
        return Ok(());
    }
    for path in [&paths.server_json, &paths.token] {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(super) fn managed_startup_error(
    paths: &ManagedPaths,
    log_offset: u64,
    status: Option<ExitStatus>,
) -> anyhow::Error {
    let summary = status.map_or_else(
        || "managed gateway did not become ready".to_string(),
        |status| format!("managed gateway did not become ready (child exited with {status})"),
    );
    match startup_log_excerpt(&paths.log, log_offset) {
        Some(excerpt) => anyhow!(
            "{summary}\nmanaged gateway output:\n{excerpt}\nfull log: {}",
            paths.log.display()
        ),
        None => anyhow!("{summary}; see {}", paths.log.display()),
    }
}

fn startup_log_excerpt(path: &Path, log_offset: u64) -> Option<String> {
    let mut log = fs::File::open(path).ok()?;
    let log_len = log.metadata().ok()?.len();
    if log_len <= log_offset {
        return None;
    }

    let read_start = log_offset.max(log_len.saturating_sub(MANAGED_STARTUP_LOG_EXCERPT_BYTES));
    let read_len = log_len.saturating_sub(read_start);
    log.seek(SeekFrom::Start(read_start)).ok()?;
    let mut bytes = Vec::with_capacity(read_len as usize);
    log.take(read_len).read_to_end(&mut bytes).ok()?;
    let output = String::from_utf8_lossy(&bytes).trim().to_string();
    if output.is_empty() {
        return None;
    }
    if read_start > log_offset {
        Some(format!("[earlier startup output omitted]\n{output}"))
    } else {
        Some(output)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LaunchResponse {
    pub(super) expires_at_ms: i64,
    pub(super) open_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedIdentityResponse {
    ok: bool,
    instance_id: String,
    pid: u32,
    version: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct RecoverableLaunchError(String);

pub(super) fn is_recoverable_launch_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<RecoverableLaunchError>().is_some()
}

async fn gateway_identity_stale_reason(
    state: &ManagedServerState,
    paths: &ManagedPaths,
) -> Option<&'static str> {
    let Ok(token) = fs::read_to_string(&paths.token) else {
        return Some("gateway_identity_unavailable");
    };
    let request = reqwest::Client::new()
        .get(format!(
            "{}/_gateway/managed/identity",
            state.base_url.trim_end_matches('/')
        ))
        .bearer_auth(token.trim())
        .timeout(MANAGED_IDENTITY_TIMEOUT)
        .send()
        .await;
    let Ok(response) = request else {
        return Some("gateway_identity_unavailable");
    };
    if !response.status().is_success() {
        return Some("gateway_identity_unavailable");
    }
    let Ok(identity) = response.json::<ManagedIdentityResponse>().await else {
        return Some("gateway_identity_unavailable");
    };
    if !identity.ok
        || Some(identity.instance_id.as_str()) != state.instance_id.as_deref()
        || identity.pid != state.pid
        || identity.version != state.version
    {
        return Some("gateway_identity_mismatch");
    }
    None
}

pub(super) async fn create_launch(
    state: &ManagedServerState,
    paths: &ManagedPaths,
    cwd: &Path,
) -> Result<LaunchResponse> {
    let token = fs::read_to_string(&paths.token)?.trim().to_string();
    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/_gateway/launch",
            state.base_url.trim_end_matches('/')
        ))
        .bearer_auth(token)
        .json(&json!({
            "cwd": cwd.to_string_lossy(),
            "source": {
                "kind": "web",
                "lifetime": "persistent",
                "visibleName": cwd.file_name().and_then(|name| name.to_str()).unwrap_or("cwd"),
            }
        }))
        .timeout(MANAGED_IDENTITY_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            anyhow::Error::new(RecoverableLaunchError(format!(
                "request managed gateway launch: {error}"
            )))
        })?;
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err(anyhow::Error::new(RecoverableLaunchError(format!(
            "managed gateway launch failed with status {}",
            response.status()
        ))));
    }
    if !response.status().is_success() {
        return Err(anyhow!(
            "managed gateway launch failed with status {}",
            response.status()
        ));
    }
    response.json::<LaunchResponse>().await.map_err(Into::into)
}

pub(super) async fn managed_status(paths: &ManagedPaths) -> Result<Value> {
    let state = match read_state_snapshot(paths)? {
        StateSnapshot::Missing => return Ok(json!({"ok": true, "running": false})),
        StateSnapshot::Invalid => {
            return Ok(json!({
                "ok": true,
                "running": false,
                "stale": true,
                "staleReason": "invalid_state",
            }));
        }
        StateSnapshot::Valid(state) => *state,
    };
    let executable = current_executable_fingerprint().ok();
    let process_executable = process_executable(state.pid);
    let Some(instance_id) = state.instance_id.as_deref() else {
        return Ok(managed_status_value(
            &state,
            false,
            executable.as_ref(),
            process_executable.as_ref(),
        ));
    };
    let (running, ownership_reason) = match inspect_process_ownership(paths, &state, instance_id)? {
        ProcessOwnership::Owned(_) => (true, None),
        ProcessOwnership::Stale(reason) | ProcessOwnership::Unavailable(reason) => {
            (false, Some(reason))
        }
    };
    let mut value = managed_status_value(
        &state,
        running,
        executable.as_ref(),
        process_executable.as_ref(),
    );
    let identity_reason = if running {
        gateway_identity_stale_reason(&state, paths).await
    } else {
        None
    };
    if let Some(reason) = ownership_reason.or(identity_reason) {
        value["stale"] = Value::Bool(true);
        value["staleReason"] = Value::String(reason.to_string());
    }
    Ok(value)
}

pub(super) fn read_channel_runtime_status(paths: &ManagedPaths) -> Option<Value> {
    let path = paths.dir.join("channels-status.json");
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub(super) fn merge_channel_runtime_status(details: &mut Value, runtime: &Value) {
    let runners = runtime.get("channels").and_then(Value::as_object);
    let Some(channels) = details.get_mut("channels").and_then(Value::as_array_mut) else {
        return;
    };
    for channel in channels {
        let Some(id) = channel.get("id").and_then(Value::as_str) else {
            continue;
        };
        let runner = runners
            .and_then(|runners| runners.get(id))
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "state": "stopped",
                    "lastPollAtMs": null,
                    "lastInboundAtMs": null,
                    "lastOutboundAtMs": null,
                    "lastError": null,
                })
            });
        if let Some(object) = channel.as_object_mut() {
            object.insert("runner".to_string(), runner);
        }
    }
}

pub(super) fn channel_runtime_summary(runtime: &Value) -> Value {
    let mut running = 0;
    let mut stopped = 0;
    let mut blocked = 0;
    let mut error = 0;
    if let Some(channels) = runtime.get("channels").and_then(Value::as_object) {
        for channel in channels.values() {
            match channel
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("stopped")
            {
                "running" => running += 1,
                "blocked" => blocked += 1,
                "error" => error += 1,
                _ => stopped += 1,
            }
        }
    }
    json!({
        "running": running,
        "stopped": stopped,
        "blocked": blocked,
        "error": error,
    })
}

pub(crate) async fn managed_status_for_home(home: &Path) -> Result<Value> {
    let paths = managed_paths(home);
    ensure_managed_dir(&paths)?;
    let _lock = lock_managed_shared(&paths)?;
    managed_status(&paths).await
}

fn managed_reuse_target(static_dir: &Path) -> Result<ManagedReuseTarget> {
    Ok(ManagedReuseTarget {
        executable: current_executable_fingerprint()?,
        static_dir: canonical_path_string(static_dir),
    })
}

pub(super) fn managed_status_value(
    state: &ManagedServerState,
    running: bool,
    expected_executable: Option<&ExecutableFingerprint>,
    process_executable: Option<&ProcessExecutable>,
) -> Value {
    let stale_reason = managed_stale_reason(
        state,
        running,
        expected_executable,
        None,
        None,
        process_executable,
    );
    json!({
        "ok": true,
        "running": running,
        "instanceId": state.instance_id,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "version": state.version,
        "executablePath": state.executable_path,
        "executableModifiedMs": state.executable_modified_ms,
        "executableSize": state.executable_size,
        "executableInode": state.executable_inode,
        "staticDir": state.static_dir,
        "stale": stale_reason.is_some(),
        "staleReason": stale_reason,
    })
}

fn current_executable_fingerprint() -> Result<ExecutableFingerprint> {
    executable_fingerprint(&env::current_exe().context("resolve pevo executable")?)
}

fn executable_fingerprint(path: &Path) -> Result<ExecutableFingerprint> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = fs::metadata(&path)?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    Ok(ExecutableFingerprint {
        path: path.display().to_string(),
        modified_ms,
        size: metadata.len(),
        inode: executable_inode(&metadata),
    })
}

fn canonical_path_string(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

pub(super) fn managed_stale_reason(
    state: &ManagedServerState,
    pid_running: bool,
    expected_executable: Option<&ExecutableFingerprint>,
    expected_static_dir: Option<&str>,
    expected_bind_policy: Option<&ManagedBindPolicy>,
    process_executable: Option<&ProcessExecutable>,
) -> Option<&'static str> {
    if state.instance_id.is_none() {
        return Some("missing_instance_id");
    }
    if !pid_running {
        return Some("pid_not_running");
    }
    let Some(state_executable) = state_executable_fingerprint(state) else {
        return Some("missing_executable_fingerprint");
    };
    if let Some(expected) = expected_executable
        && &state_executable != expected
    {
        return Some("executable_fingerprint_mismatch");
    }
    if let Some(process) = process_executable {
        if process.deleted {
            return Some("process_executable_deleted");
        }
        if let Some(expected) = expected_executable {
            if let (Some(process_inode), Some(expected_inode)) = (process.inode, expected.inode)
                && process_inode != expected_inode
            {
                return Some("process_executable_mismatch");
            }
            if process.path != expected.path {
                return Some("process_executable_mismatch");
            }
        }
    }
    if let Some(expected_static_dir) = expected_static_dir {
        let Some(static_dir) = state.static_dir.as_deref() else {
            return Some("missing_static_dir");
        };
        if static_dir != expected_static_dir {
            return Some("static_dir_mismatch");
        }
    }
    if let Some(policy) = expected_bind_policy {
        let Some(bound_addr) = state_bound_addr(state) else {
            return Some("bind_addr_mismatch");
        };
        if !policy.allows_bound_addr(bound_addr) {
            return Some("bind_addr_mismatch");
        }
    }
    None
}

fn state_bound_addr(state: &ManagedServerState) -> Option<SocketAddr> {
    state
        .base_url
        .strip_prefix("http://")?
        .parse::<SocketAddr>()
        .ok()
}

fn state_executable_fingerprint(state: &ManagedServerState) -> Option<ExecutableFingerprint> {
    Some(ExecutableFingerprint {
        path: state.executable_path.clone()?,
        modified_ms: state.executable_modified_ms?,
        size: state.executable_size?,
        inode: state.executable_inode,
    })
}

#[cfg(unix)]
fn process_executable(pid: u32) -> Option<ProcessExecutable> {
    let path = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let path_text = path.display().to_string();
    let deleted = path_text.ends_with(" (deleted)");
    let metadata = fs::metadata(&path).ok();
    Some(ProcessExecutable {
        path: path_text.trim_end_matches(" (deleted)").to_string(),
        inode: metadata.as_ref().and_then(executable_inode),
        deleted,
    })
}

#[cfg(not(unix))]
fn process_executable(_pid: u32) -> Option<ProcessExecutable> {
    None
}

#[cfg(unix)]
fn executable_inode(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    Some(metadata.ino())
}

#[cfg(not(unix))]
fn executable_inode(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

pub(super) async fn stop_managed(paths: &ManagedPaths) -> Result<bool> {
    let state = match read_state_snapshot(paths)? {
        StateSnapshot::Missing => {
            ensure_idle_lease(paths, "managed state is missing")?;
            cleanup_state(paths)?;
            return Ok(false);
        }
        StateSnapshot::Invalid => {
            ensure_idle_lease(paths, "managed state is invalid")?;
            cleanup_state(paths)?;
            return Ok(false);
        }
        StateSnapshot::Valid(state) => *state,
    };
    let Some(instance_id) = state.instance_id.as_deref() else {
        ensure_idle_lease(paths, "managed state is missing instanceId")?;
        cleanup_state(paths)?;
        return Ok(false);
    };
    match inspect_process_ownership(paths, &state, instance_id)? {
        ProcessOwnership::Stale(_) => {
            cleanup_state(paths)?;
            Ok(false)
        }
        ProcessOwnership::Unavailable(reason) => Err(anyhow!(
            "managed gateway ownership cannot be proven ({reason}); refusing to signal pid {}",
            state.pid
        )),
        ProcessOwnership::Owned(process) => {
            stop_owned_instance(&state, paths, &process).await?;
            cleanup_state(paths)?;
            Ok(true)
        }
    }
}

async fn stop_owned_instance(
    state: &ManagedServerState,
    paths: &ManagedPaths,
    process: &ManagedProcess,
) -> Result<()> {
    let requested = request_managed_shutdown(state, paths).await;
    #[cfg(unix)]
    if !requested {
        process
            .request_graceful_termination()
            .context("send SIGTERM to verified managed gateway")?;
    }
    if (requested || cfg!(unix))
        && process.wait_for_exit(MANAGED_STOP_GRACE_TIMEOUT)?
        && process.wait_for_tree_exit(Duration::ZERO)?
    {
        return Ok(());
    }
    process
        .terminate_tree(1)
        .context("terminate verified managed process tree")?;
    if process.wait_for_exit(MANAGED_STOP_FORCE_TIMEOUT)?
        && process.wait_for_tree_exit(MANAGED_STOP_FORCE_TIMEOUT)?
    {
        return Ok(());
    }
    Err(anyhow!(
        "managed gateway pid {} did not exit after graceful and forced termination",
        state.pid
    ))
}

async fn request_managed_shutdown(state: &ManagedServerState, paths: &ManagedPaths) -> bool {
    let Some(instance_id) = state.instance_id.as_deref() else {
        return false;
    };
    let Ok(token) = fs::read_to_string(&paths.token) else {
        return false;
    };
    reqwest::Client::new()
        .post(format!(
            "{}/_gateway/managed/shutdown",
            state.base_url.trim_end_matches('/')
        ))
        .bearer_auth(token.trim())
        .json(&json!({"instanceId": instance_id}))
        .timeout(MANAGED_IDENTITY_TIMEOUT)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

pub(crate) async fn stop_managed_for_home(home: &Path) -> Result<bool> {
    let paths = managed_paths(home);
    ensure_managed_dir(&paths)?;
    let _lock = lock_managed_exclusive(&paths)?;
    stop_managed(&paths).await
}

fn read_state_snapshot(paths: &ManagedPaths) -> Result<StateSnapshot> {
    if !paths.server_json.exists() {
        return Ok(StateSnapshot::Missing);
    }
    let text = match fs::read_to_string(&paths.server_json) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StateSnapshot::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    Ok(match serde_json::from_str(&text) {
        Ok(state) => StateSnapshot::Valid(Box::new(state)),
        Err(_) => StateSnapshot::Invalid,
    })
}

fn cleanup_state(paths: &ManagedPaths) -> Result<()> {
    for path in [&paths.server_json, &paths.token] {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(super) fn managed_paths(home: &Path) -> ManagedPaths {
    let dir = home.join(GATEWAY_DIR);
    ManagedPaths {
        server_json: dir.join("server.json"),
        token: dir.join("token"),
        lock: dir.join("lock"),
        instance_lock: dir.join("instance.lock"),
        log: dir.join("server.log"),
        dir,
    }
}

pub(super) fn ensure_managed_dir(paths: &ManagedPaths) -> Result<()> {
    fs::create_dir_all(&paths.dir)?;
    let _ = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&paths.lock)?;
    let _ = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&paths.instance_lock)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn lock_managed_exclusive(paths: &ManagedPaths) -> Result<ManagedLifecycleLock> {
    let file = open_lifecycle_lock(paths)?;
    file.lock()?;
    Ok(ManagedLifecycleLock { _file: file })
}

pub(super) fn lock_managed_shared(paths: &ManagedPaths) -> Result<ManagedLifecycleLock> {
    let file = open_lifecycle_lock(paths)?;
    file.lock_shared()?;
    Ok(ManagedLifecycleLock { _file: file })
}

fn open_lifecycle_lock(paths: &ManagedPaths) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&paths.lock)?)
}

fn rotate_token(paths: &ManagedPaths) -> Result<()> {
    let token = Uuid::now_v7().to_string();
    atomic_replace_private(&paths.token, token.as_bytes())?;
    Ok(())
}
