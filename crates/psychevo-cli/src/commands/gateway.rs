use std::env;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use psychevo_runtime::canonicalize_workdir;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::args::{GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewayStartArgs};
use crate::commands::serve::resolve_static_dir;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home,
};

const GATEWAY_DIR: &str = "gateway";

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
                bind: "127.0.0.1:0".parse().expect("default bind address"),
                no_browser: false,
                print_url: false,
            })
            .await
        }
    }
}

async fn open(args: GatewayOpenArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.join("index.html").exists() {
        return print_json_code(json!({
            "ok": false,
            "error": {
                "code": "workbench_dist_missing",
                "message": format!("Workbench assets not found at {}", static_dir.display())
            }
        }));
    }
    let state = ensure_started(&ctx, args.bind, &static_dir).await?;
    let workdir = match &args.dir {
        Some(dir) => canonicalize_workdir(&resolve_explicit_path(dir, &ctx.env_map, &ctx.cwd)?)?,
        None => canonicalize_workdir(&ctx.cwd)?,
    };
    let launch = create_launch(&state, &ctx.paths, &workdir).await?;
    if !args.no_browser {
        let _ = open_browser(launch.open_url.as_str());
    }
    let mut output = json!({
        "ok": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "workdir": workdir,
        "openedBrowser": !args.no_browser,
    });
    if args.print_url {
        output["openUrl"] = Value::String(launch.open_url);
    }
    print_json(output)
}

async fn start(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.join("index.html").exists() {
        return print_json_code(json!({
            "ok": false,
            "error": {
                "code": "workbench_dist_missing",
                "message": format!("Workbench assets not found at {}", static_dir.display())
            }
        }));
    }
    let state = ensure_started(&ctx, args.bind, &static_dir).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "version": state.version,
    }))
}

async fn status() -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let status = managed_status(&ctx.paths)?;
    print_json(status)
}

async fn stop() -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let stopped = stop_managed(&ctx.paths)?;
    print_json(json!({"ok": true, "stopped": stopped}))
}

async fn restart(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let _ = stop_managed(&ctx.paths)?;
    let static_dir = resolve_static_dir(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.join("index.html").exists() {
        return print_json_code(json!({
            "ok": false,
            "error": {
                "code": "workbench_dist_missing",
                "message": format!("Workbench assets not found at {}", static_dir.display())
            }
        }));
    }
    let state = ensure_started(&ctx, args.bind, &static_dir).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "version": state.version,
        "restarted": true,
    }))
}

struct GatewayContext {
    cwd: PathBuf,
    env_map: std::collections::BTreeMap<String, String>,
    paths: ManagedPaths,
}

impl GatewayContext {
    fn load() -> Result<Self> {
        let env_map = inherited_env();
        let cwd = env::current_dir()?;
        let home = resolve_psychevo_home(&env_map, &cwd)?;
        let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
        let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
        if !bypass_home {
            ensure_home_initialized(&home)?;
        }
        let paths = managed_paths(&home);
        ensure_managed_dir(&paths)?;
        Ok(Self {
            cwd,
            env_map,
            paths,
        })
    }
}

async fn ensure_started(
    ctx: &GatewayContext,
    bind: std::net::SocketAddr,
    static_dir: &Path,
) -> Result<ManagedServerState> {
    if let Some(state) = read_state(&ctx.paths)?
        && pid_alive(state.pid)
    {
        return Ok(state);
    }
    cleanup_state(&ctx.paths)?;
    rotate_token(&ctx.paths)?;
    spawn_serve(ctx, bind, static_dir)?;
    wait_for_state(&ctx.paths).await
}

fn spawn_serve(ctx: &GatewayContext, bind: std::net::SocketAddr, static_dir: &Path) -> Result<()> {
    let exe = env::current_exe().context("resolve pevo executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.paths.log)?;
    let log_err = log.try_clone()?;
    let child = Command::new(exe)
        .arg("serve")
        .arg("--bind")
        .arg(bind.to_string())
        .arg("--token-file")
        .arg(&ctx.paths.token)
        .arg("--internal-static-dir")
        .arg(static_dir)
        .arg("--internal-managed-state")
        .arg(&ctx.paths.server_json)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .context("spawn pevo serve")?;
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
        return Ok(json!({
            "ok": true,
            "running": running,
            "pid": state.pid,
            "baseUrl": state.base_url,
            "readyzUrl": state.readyz_url,
            "startedAtMs": state.started_at_ms,
            "version": state.version,
            "stale": !running,
        }));
    }
    Ok(json!({"ok": true, "running": false}))
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
