use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
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

#[derive(Debug, Clone)]
pub(super) struct ManagedPaths {
    dir: PathBuf,
    server_json: PathBuf,
    token: PathBuf,
    lock: PathBuf,
    log: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedServerState {
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
    log_offset: u64,
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
    if let Some(state) = read_state(&ctx.paths)? {
        let process = process_executable(state.pid);
        let stale_reason = managed_stale_reason(
            &state,
            pid_alive(state.pid),
            Some(&target.executable),
            Some(target.static_dir.as_str()),
            Some(&bind_policy),
            process.as_ref(),
        );
        if stale_reason.is_none() {
            return Ok(state);
        }
        if pid_alive(state.pid) {
            stop_pid_bounded(state.pid)?;
        }
    }
    cleanup_state(&ctx.paths)?;
    rotate_token(&ctx.paths)?;
    let launch = spawn_serve(ctx, bind_policy, static_dir)?;
    wait_for_state(&ctx.paths, launch).await
}

fn spawn_serve(
    ctx: &GatewayContext,
    bind_policy: ManagedBindPolicy,
    static_dir: &Path,
) -> Result<ManagedLaunch> {
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
        .arg(&ctx.paths.server_json);
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
    Ok(ManagedLaunch { child, log_offset })
}

async fn wait_for_state(
    paths: &ManagedPaths,
    mut launch: ManagedLaunch,
) -> Result<ManagedServerState> {
    let started = Instant::now();
    while started.elapsed() < MANAGED_STARTUP_TIMEOUT {
        if let Some(state) = read_state(paths)?
            && pid_alive(state.pid)
        {
            return Ok(state);
        }
        if let Some(status) = launch
            .child
            .try_wait()
            .context("poll managed gateway process")?
        {
            return Err(managed_startup_error(
                paths,
                launch.log_offset,
                Some(status),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(managed_startup_error(paths, launch.log_offset, None))
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
        .send()
        .await
        .context("request managed gateway launch")?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "managed gateway launch failed with status {}",
            response.status()
        ));
    }
    response.json::<LaunchResponse>().await.map_err(Into::into)
}

pub(super) fn managed_status(paths: &ManagedPaths) -> Result<Value> {
    if let Some(state) = read_state(paths)? {
        let running = pid_alive(state.pid);
        let executable = current_executable_fingerprint().ok();
        let process = process_executable(state.pid);
        return Ok(managed_status_value(
            &state,
            running,
            executable.as_ref(),
            process.as_ref(),
        ));
    }
    Ok(json!({"ok": true, "running": false}))
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

pub(crate) fn managed_status_for_home(home: &Path) -> Result<Value> {
    managed_status(&managed_paths(home))
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

pub(super) fn stop_managed(paths: &ManagedPaths) -> Result<bool> {
    let Some(state) = read_state(paths)? else {
        cleanup_state(paths)?;
        return Ok(false);
    };
    let stopped = if pid_alive(state.pid) {
        stop_pid_bounded(state.pid)?;
        true
    } else {
        false
    };
    cleanup_state(paths)?;
    Ok(stopped)
}

fn stop_pid_bounded(pid: u32) -> Result<()> {
    if let Err(error) = kill_pid(pid) {
        if !pid_alive(pid) {
            return Ok(());
        }
        return Err(error);
    }
    if wait_for_pid_exit(pid, MANAGED_STOP_GRACE_TIMEOUT) {
        return Ok(());
    }
    if let Err(error) = force_kill_pid(pid) {
        if !pid_alive(pid) {
            return Ok(());
        }
        return Err(error);
    }
    if wait_for_pid_exit(pid, MANAGED_STOP_FORCE_TIMEOUT) {
        return Ok(());
    }
    Err(anyhow!(
        "managed gateway pid {pid} did not exit after SIGTERM and forced termination"
    ))
}

pub(crate) fn stop_managed_for_home(home: &Path) -> Result<bool> {
    stop_managed(&managed_paths(home))
}

fn read_state(paths: &ManagedPaths) -> Result<Option<ManagedServerState>> {
    if !paths.server_json.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&paths.server_json)?;
    Ok(Some(serde_json::from_str(&text)?))
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
        log: dir.join("server.log"),
        dir,
    }
}

pub(super) fn ensure_managed_dir(paths: &ManagedPaths) -> Result<()> {
    fs::create_dir_all(&paths.dir)?;
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.lock)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn rotate_token(paths: &ManagedPaths) -> Result<()> {
    let token = Uuid::now_v7().to_string();
    fs::write(&paths.token, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.token, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}

#[cfg(unix)]
fn kill_pid(pid: u32) -> Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
pub(super) fn force_kill_pid(pid: u32) -> Result<()> {
    let process_group = -(pid as libc::pid_t);
    let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(not(unix))]
fn kill_pid(pid: u32) -> Result<()> {
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn force_kill_pid(pid: u32) -> Result<()> {
    kill_pid(pid)
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !pid_alive(pid)
}
