use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo_gateway::{Gateway, GatewayWebServerConfig, bind_gateway_web_server};
use psychevo_runtime::{StateRuntime, canonicalize_workdir};
use serde_json::json;

use crate::args::ServeArgs;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) async fn run_serve_command(args: ServeArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let requested_workdir = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd.clone(),
    };
    let workdir = canonicalize_workdir(&requested_workdir)?;
    let token = serve_token(&args, &env_map, &cwd)?;
    let static_dir = args
        .static_dir
        .as_deref()
        .map(|path| resolve_explicit_path(path, &env_map, &cwd))
        .transpose()?;
    let managed_state = args
        .managed_state
        .as_deref()
        .map(|path| resolve_explicit_path(path, &env_map, &cwd))
        .transpose()?;

    let state = StateRuntime::open(&db_path)?;
    let gateway = Gateway::new(state);
    let mut config =
        GatewayWebServerConfig::headless(gateway, home, workdir, config_path, env_map, token);
    config.bind_addr = args.bind;
    config.static_dir = static_dir;
    config.managed_state_path = managed_state;

    let bound = bind_gateway_web_server(config).await?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ready": true,
            "baseUrl": bound.url(),
            "readyzUrl": format!("{}/readyz", bound.url()),
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        }))?
    );
    bound.run().await?;
    Ok(ExitCode::SUCCESS)
}

fn serve_token(
    args: &ServeArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String> {
    if let Some(path) = &args.token_file {
        let path = resolve_explicit_path(path, env_map, cwd)?;
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(anyhow!("token file is empty: {}", path.display()));
        }
        return Ok(token);
    }
    env_value("PSYCHEVO_SERVE_TOKEN", env_map)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow!("pevo serve requires PSYCHEVO_SERVE_TOKEN or --token-file"))
}

#[allow(dead_code)]
pub(crate) fn resolve_static_dir(
    explicit: Option<&Path>,
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return resolve_explicit_path(path, env_map, cwd);
    }
    if let Some(path) = env_value("PSYCHEVO_WEB_DIST", env_map) {
        return resolve_explicit_path(Path::new(&path), env_map, cwd);
    }
    for candidate in static_dir_candidates(cwd) {
        if candidate.join("index.html").exists() {
            return Ok(candidate);
        }
    }
    Ok(cwd.join("apps/workbench/dist"))
}

#[allow(dead_code)]
pub(crate) fn static_dir_candidates(cwd: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![cwd.join("apps/workbench/dist")];
    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        candidates.push(exe_dir.join("web"));
        candidates.push(exe_dir.join("../share/psychevo/web"));
    }
    candidates
}
