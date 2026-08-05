use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use psychevo::paths::canonicalize_cwd;
use serde_json::{Value, json};

use crate::args::{
    GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewayStartArgs, WebArgs, WebCommand,
};
use crate::commands::common::CommandConfiguration;
use crate::commands::serve::resolve_static_dir_diagnostic;
use crate::env::resolve_explicit_path;

use super::context::GatewayContext;
use super::managed::{
    ManagedBindPolicy, channel_runtime_summary, create_launch, ensure_started,
    is_recoverable_launch_error, lock_managed_exclusive, lock_managed_shared, managed_status,
    merge_channel_runtime_status, read_channel_runtime_status, stop_managed,
};
use super::output::{open_browser, print_json, print_json_code, workbench_dist_missing};
#[cfg(feature = "native-channels")]
use super::setup::gateway_setup;

pub(crate) async fn run_gateway_command(args: GatewayArgs) -> Result<ExitCode> {
    match args.command {
        Some(GatewayCommand::Open(args)) => open(args).await,
        Some(GatewayCommand::Start(args)) => start(args).await,
        #[cfg(feature = "native-channels")]
        Some(GatewayCommand::Setup(args)) => gateway_setup(args).await,
        Some(GatewayCommand::Status(_args)) => status().await,
        Some(GatewayCommand::Stop) => stop().await,
        Some(GatewayCommand::Restart(args)) => restart(args).await,
        None => {
            open(GatewayOpenArgs {
                cd: None,
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
    let cwd = resolve_open_cwd(&ctx, &args).await?;
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

async fn resolve_open_cwd(ctx: &GatewayContext, args: &GatewayOpenArgs) -> Result<PathBuf> {
    if args.default_workspace {
        let configuration = CommandConfiguration::open(&ctx.env_map, &ctx.home, &ctx.cwd).await?;
        let result = (|| {
            let cwd = configuration.configuration().default_workspace_cwd()?;
            Ok(canonicalize_cwd(&cwd)?)
        })();
        return configuration.finish(result).await;
    }
    match &args.cd {
        Some(cd) => Ok(canonicalize_cwd(&resolve_explicit_path(
            cd,
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
    let configuration = CommandConfiguration::open(&ctx.env_map, &ctx.home, &ctx.cwd).await?;
    status["channels"] = configuration
        .configuration()
        .channel_summary()
        .unwrap_or_else(|_| {
            json!({
                "configured": 0,
                "enabled": 0,
                "ready": 0,
                "blocked": 0,
                "setup_needed": true,
            })
        });
    status["channelDetails"] = configuration
        .configuration()
        .channels()
        .unwrap_or_else(|_| {
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
    configuration.finish(print_json(status)).await
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::commands::gateway::managed::managed_paths;

    #[tokio::test]
    async fn default_workspace_uses_framework_configuration_and_canonicalizes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("caller");
        let workspace_root = temp.path().join("configured-workspaces");
        let default_workspace = workspace_root.join("general");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&default_workspace).expect("default workspace");
        let root = workspace_root.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(
            home.join("config.toml"),
            format!("[workspaces]\nroot = \"{root}\"\n"),
        )
        .expect("config");
        std::fs::create_dir_all(cwd.join(".psychevo")).expect("project config dir");
        std::fs::write(
            cwd.join(".psychevo/config.toml"),
            "[workspaces]\nroot = \"ignored-project-workspaces\"\n",
        )
        .expect("project config");
        let env_map = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            (
                "PSYCHEVO_HOME".to_string(),
                home.as_path().display().to_string(),
            ),
        ]);
        let ctx = GatewayContext {
            cwd,
            home: home.clone(),
            profile_name: "default".to_string(),
            env_map,
            paths: managed_paths(&home),
        };
        let args = GatewayOpenArgs {
            cd: None,
            default_workspace: true,
            bind: None,
            no_browser: true,
            print_url: false,
        };

        let resolved = resolve_open_cwd(&ctx, &args)
            .await
            .expect("default workspace");

        assert_eq!(
            resolved,
            psychevo::paths::canonicalize_cwd(&default_workspace).expect("canonical workspace")
        );
    }
}
