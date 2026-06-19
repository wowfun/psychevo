use std::env;
use std::fs::{self, OpenOptions};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use anyhow::{Context, Result, anyhow};
use psychevo_runtime::{canonicalize_workdir, resolve_default_workspace_workdir};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::args::{GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewayStartArgs};
use crate::commands::serve::{
    StaticDirResolution, resolve_static_dir_diagnostic, static_dir_build_command,
    static_dir_install_command,
};
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home,
};

const GATEWAY_DIR: &str = "gateway";
const MANAGED_GATEWAY_DEFAULT_PORT: u16 = 58_080;
const MANAGED_GATEWAY_FALLBACK_PORTS: u16 = 19;

#[derive(Debug, Clone)]
struct ManagedPaths {
    dir: PathBuf,
    server_json: PathBuf,
    token: PathBuf,
    lock: PathBuf,
    log: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedServerState {
    pid: u32,
    base_url: String,
    readyz_url: String,
    started_at_ms: i64,
    version: String,
    executable_path: Option<String>,
    executable_modified_ms: Option<i64>,
    executable_size: Option<u64>,
    executable_inode: Option<u64>,
    static_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutableFingerprint {
    path: String,
    modified_ms: i64,
    size: u64,
    inode: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedReuseTarget {
    executable: ExecutableFingerprint,
    static_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessExecutable {
    path: String,
    inode: Option<u64>,
    deleted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManagedBindPolicy {
    bind_addr: SocketAddr,
    fallback_ports: u16,
}

impl ManagedBindPolicy {
    fn new(explicit: Option<SocketAddr>) -> Self {
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

    fn bind_addr(self) -> SocketAddr {
        self.bind_addr
    }

    fn fallback_ports(self) -> u16 {
        self.fallback_ports
    }

    fn allows_bound_addr(self, addr: SocketAddr) -> bool {
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

pub(crate) async fn run_gateway_command(args: GatewayArgs) -> Result<ExitCode> {
    match args.command {
        Some(GatewayCommand::Open(args)) => open(args).await,
        Some(GatewayCommand::Start(args)) => start(args).await,
        Some(GatewayCommand::Status) => status().await,
        Some(GatewayCommand::Stop) => stop().await,
        Some(GatewayCommand::Restart(args)) => restart(args).await,
        None => {
            open(GatewayOpenArgs {
                dir: None,
                default_workspace: false,
                bind: None,
                no_browser: false,
                print_url: false,
            })
            .await
        }
    }
}

pub(crate) async fn run_web_command(args: GatewayOpenArgs) -> Result<ExitCode> {
    open(args).await
}

pub(crate) async fn open(args: GatewayOpenArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    let workdir = resolve_open_workdir(&ctx, &args)?;
    let launch = create_launch(&state, &ctx.paths, &workdir).await?;
    if !args.no_browser {
        let _ = open_browser(launch.open_url.as_str());
    }
    let mut output = json!({
        "ok": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "workdir": workdir,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
        "openedBrowser": !args.no_browser,
    });
    if args.print_url {
        output["openUrlExpiresAtMs"] = Value::from(launch.expires_at_ms);
        output["openUrlOneTime"] = Value::Bool(true);
        output["openUrl"] = Value::String(launch.open_url);
    }
    print_json(output)
}

fn resolve_open_workdir(ctx: &GatewayContext, args: &GatewayOpenArgs) -> Result<PathBuf> {
    if args.default_workspace {
        let options = ctx.run_options(ctx.cwd.clone())?;
        return Ok(canonicalize_workdir(&resolve_default_workspace_workdir(
            &options, &ctx.cwd,
        )?)?);
    }
    match &args.dir {
        Some(dir) => Ok(canonicalize_workdir(&resolve_explicit_path(
            dir,
            &ctx.env_map,
            &ctx.cwd,
        )?)?),
        None => Ok(canonicalize_workdir(&ctx.cwd)?),
    }
}

async fn start(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "version": state.version,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
    }))
}

async fn status() -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let mut status = managed_status(&ctx.paths)?;
    status["profile"] = Value::String(ctx.profile_name);
    status["profileHome"] = Value::String(ctx.home.display().to_string());
    print_json(status)
}

async fn stop() -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let stopped = stop_managed(&ctx.paths)?;
    print_json(json!({
        "ok": true,
        "stopped": stopped,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
    }))
}

async fn restart(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let _ = stop_managed(&ctx.paths)?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "version": state.version,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
        "restarted": true,
    }))
}

struct GatewayContext {
    cwd: PathBuf,
    home: PathBuf,
    profile_name: String,
    env_map: std::collections::BTreeMap<String, String>,
    paths: ManagedPaths,
}

impl GatewayContext {
    fn load() -> Result<Self> {
        let env_map = inherited_env();
        let cwd = env::current_dir()?;
        let home = resolve_psychevo_home(&env_map, &cwd)?;
        let profile_name = env_value(crate::profiles::PROFILE_ENV, &env_map)
            .unwrap_or_else(|| crate::profiles::DEFAULT_PROFILE.to_string());
        let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
        let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
        if !bypass_home {
            ensure_home_initialized(&home)?;
        }
        let paths = managed_paths(&home);
        ensure_managed_dir(&paths)?;
        Ok(Self {
            cwd,
            home,
            profile_name,
            env_map,
            paths,
        })
    }

    fn run_options(&self, workdir: PathBuf) -> Result<psychevo_runtime::RunOptions> {
        Ok(psychevo_runtime::RunOptions {
            state: psychevo_runtime::StateRuntime::open(self.home.join("state.db"))?,
            workdir,
            snapshot_root: Some(self.home.join("snapshots")),
            session: None,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            max_context_messages: None,
            config_path: None,
            project_context_override: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            external_agent_delegate: None,
            include_reasoning: false,
            mode: psychevo_runtime::RunMode::Default,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: Some(self.env_map.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        })
    }
}

async fn ensure_started(
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
            let _ = kill_pid(state.pid);
        }
    }
    cleanup_state(&ctx.paths)?;
    rotate_token(&ctx.paths)?;
    spawn_serve(ctx, bind_policy, static_dir)?;
    wait_for_state(&ctx.paths).await
}

fn spawn_serve(
    ctx: &GatewayContext,
    bind_policy: ManagedBindPolicy,
    static_dir: &Path,
) -> Result<()> {
    let exe = env::current_exe().context("resolve pevo executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.paths.log)?;
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
    let _ = child.id();
    Ok(())
}

async fn wait_for_state(paths: &ManagedPaths) -> Result<ManagedServerState> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if let Some(state) = read_state(paths)?
            && pid_alive(state.pid)
        {
            return Ok(state);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(anyhow!(
        "managed gateway did not become ready; see {}",
        paths.log.display()
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchResponse {
    expires_at_ms: i64,
    open_url: String,
}

async fn create_launch(
    state: &ManagedServerState,
    paths: &ManagedPaths,
    workdir: &Path,
) -> Result<LaunchResponse> {
    let token = fs::read_to_string(&paths.token)?.trim().to_string();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/_gateway/launch", state.base_url.trim_end_matches('/')))
        .bearer_auth(token)
        .json(&json!({
            "workdir": workdir,
            "source": {
                "kind": "web",
                "lifetime": "persistent",
                "visibleName": workdir.file_name().and_then(|name| name.to_str()).unwrap_or("workdir"),
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

fn managed_status(paths: &ManagedPaths) -> Result<Value> {
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

pub(crate) fn managed_status_for_home(home: &Path) -> Result<Value> {
    managed_status(&managed_paths(home))
}

pub(crate) fn workbench_dist_missing(resolution: &StaticDirResolution) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": "workbench_dist_missing",
            "message": format!("Workbench assets not found at {}", resolution.path.display()),
            "path": resolution.path.display().to_string(),
            "source": resolution.source,
            "searched": resolution.searched.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "envVar": "PSYCHEVO_WEB_DIST",
            "buildCommand": static_dir_build_command(),
            "installCommand": static_dir_install_command(),
        }
    })
}

fn managed_reuse_target(static_dir: &Path) -> Result<ManagedReuseTarget> {
    Ok(ManagedReuseTarget {
        executable: current_executable_fingerprint()?,
        static_dir: canonical_path_string(static_dir),
    })
}

fn managed_status_value(
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

fn managed_stale_reason(
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

fn stop_managed(paths: &ManagedPaths) -> Result<bool> {
    let Some(state) = read_state(paths)? else {
        cleanup_state(paths)?;
        return Ok(false);
    };
    let stopped = if pid_alive(state.pid) {
        kill_pid(state.pid)?;
        true
    } else {
        false
    };
    cleanup_state(paths)?;
    Ok(stopped)
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

fn managed_paths(home: &Path) -> ManagedPaths {
    let dir = home.join(GATEWAY_DIR);
    ManagedPaths {
        server_json: dir.join("server.json"),
        token: dir.join("token"),
        lock: dir.join("lock"),
        log: dir.join("server.log"),
        dir,
    }
}

fn ensure_managed_dir(paths: &ManagedPaths) -> Result<()> {
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

fn print_json(value: Value) -> Result<ExitCode> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(ExitCode::SUCCESS)
}

fn print_json_code(value: Value) -> Result<ExitCode> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(ExitCode::from(1))
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
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

#[cfg(not(unix))]
fn kill_pid(pid: u32) -> Result<()> {
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    Ok(())
}
