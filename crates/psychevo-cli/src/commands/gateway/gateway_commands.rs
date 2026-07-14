use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use psychevo_runtime::{
    canonicalize_cwd, channel_list_value, channel_summary_value, resolve_default_workspace_cwd,
};
use serde_json::{Value, json};

use crate::args::{
    GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewayStartArgs, WebArgs, WebCommand,
};
use crate::commands::serve::resolve_static_dir_diagnostic;
use crate::env::resolve_explicit_path;

use super::context::GatewayContext;
use super::managed::{
    ManagedBindPolicy, channel_runtime_summary, create_launch, ensure_started,
    is_recoverable_launch_error, lock_managed_exclusive, lock_managed_shared, managed_status,
    merge_channel_runtime_status, read_channel_runtime_status, stop_managed,
};
use super::output::{open_browser, print_json, print_json_code, workbench_dist_missing};
use super::setup::gateway_setup;

pub(crate) async fn run_gateway_command(args: GatewayArgs) -> Result<ExitCode> {
    match args.command {
        Some(GatewayCommand::Open(args)) => open(args).await,
        Some(GatewayCommand::Start(args)) => start(args).await,
        Some(GatewayCommand::Setup(args)) => gateway_setup(args).await,
        Some(GatewayCommand::Status(_args)) => status().await,
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

pub(crate) async fn run_web_command(args: WebArgs) -> Result<ExitCode> {
    match args.command {
        Some(WebCommand::Start(args)) => start(args).await,
        Some(WebCommand::Stop) => stop().await,
        Some(WebCommand::Restart(args)) => restart(args).await,
        None => open(args.open).await,
    }
}

pub(crate) async fn open(args: GatewayOpenArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let _lock = lock_managed_exclusive(&ctx.paths)?;
    let mut state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    let cwd = resolve_open_cwd(&ctx, &args)?;
    let launch = match create_launch(&state, &ctx.paths, &cwd).await {
        Ok(launch) => launch,
        Err(error) if is_recoverable_launch_error(&error) => {
            state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
            create_launch(&state, &ctx.paths, &cwd).await?
        }
        Err(error) => return Err(error),
    };
    if !args.no_browser {
        let _ = open_browser(launch.open_url.as_str());
    }
    let mut output = json!({
        "ok": true,
        "instanceId": state.instance_id,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "cwd": cwd,
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

fn resolve_open_cwd(ctx: &GatewayContext, args: &GatewayOpenArgs) -> Result<PathBuf> {
    if args.default_workspace {
        let options = ctx.run_options(ctx.cwd.clone())?;
        return Ok(canonicalize_cwd(&resolve_default_workspace_cwd(
            &options, &ctx.cwd,
        )?)?);
    }
    match &args.dir {
        Some(dir) => Ok(canonicalize_cwd(&resolve_explicit_path(
            dir,
            &ctx.env_map,
            &ctx.cwd,
        )?)?),
        None => Ok(canonicalize_cwd(&ctx.cwd)?),
    }
}

async fn start(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let _lock = lock_managed_exclusive(&ctx.paths)?;
    let state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "instanceId": state.instance_id,
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
    let _lock = lock_managed_shared(&ctx.paths)?;
    let mut status = managed_status(&ctx.paths).await?;
    status["profile"] = Value::String(ctx.profile_name.clone());
    status["profileHome"] = Value::String(ctx.home.display().to_string());
    let options = ctx.run_options(ctx.cwd.clone())?;
    status["channels"] = channel_summary_value(&options).unwrap_or_else(|_| {
        json!({
            "configured": 0,
            "enabled": 0,
            "ready": 0,
            "blocked": 0,
            "setup_needed": true,
        })
    });
    status["channelDetails"] = channel_list_value(&options).unwrap_or_else(|_| {
        json!({
            "channels": [],
        })
    });
    if status
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && let Some(runtime) = read_channel_runtime_status(&ctx.paths)
    {
        merge_channel_runtime_status(&mut status["channelDetails"], &runtime);
        status["channelRuntime"] = channel_runtime_summary(&runtime);
    }
    print_json(status)
}

async fn stop() -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let _lock = lock_managed_exclusive(&ctx.paths)?;
    let stopped = stop_managed(&ctx.paths).await?;
    print_json(json!({
        "ok": true,
        "stopped": stopped,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
    }))
}

async fn restart(args: GatewayStartArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load()?;
    let _lock = lock_managed_exclusive(&ctx.paths)?;
    let _ = stop_managed(&ctx.paths).await?;
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return print_json_code(workbench_dist_missing(&static_dir));
    }
    let bind_policy = ManagedBindPolicy::new(args.bind);
    let state = ensure_started(&ctx, bind_policy, &static_dir.path).await?;
    print_json(json!({
        "ok": true,
        "running": true,
        "instanceId": state.instance_id,
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
