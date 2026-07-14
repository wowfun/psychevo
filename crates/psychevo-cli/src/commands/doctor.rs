use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use psychevo_runtime::{
    auth_status_value, config_show_value, fetch_model_catalog, model_catalog_providers,
    selected_configured_model,
};
use serde_json::{Value, json};

use crate::args::DoctorArgs;
use crate::commands::common::base_run_options;
use crate::commands::gateway::managed_status_for_home;
use crate::commands::model::model_value;
use crate::commands::serve::{
    resolve_static_dir_diagnostic, static_dir_build_command, static_dir_install_command,
};
use crate::env::{env_path, inherited_env, resolve_psychevo_home, resolve_state_db};

pub(crate) async fn run_doctor_command(args: DoctorArgs) -> Result<ExitCode> {
    let report = doctor_report(&args).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(ExitCode::SUCCESS)
}

async fn doctor_report(args: &DoctorArgs) -> Result<Value> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let state_db = resolve_state_db(&env_map, &home, &cwd)?;
    let explicit_config = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let home_config = home.join("config.toml");
    let home_initialized = home_config.exists();

    let options = base_run_options(&env_map, &home, &cwd);
    let config = match &options {
        Ok(options) => capture_value(|| {
            Ok(config_show_value(
                options,
                psychevo_runtime::ConfigScope::Effective,
            )?)
        }),
        Err(err) => json!({ "ok": false, "error": format!("{err:#}") }),
    };
    let model = match &options {
        Ok(options) => capture_value(|| {
            Ok(json!({
                "model": selected_configured_model(options)?.as_ref().map(model_value),
            }))
        }),
        Err(err) => json!({ "ok": false, "error": format!("{err:#}") }),
    };
    let auth = match &options {
        Ok(options) => capture_value(|| Ok(auth_status_value(options, None)?)),
        Err(err) => json!({ "ok": false, "error": format!("{err:#}") }),
    };

    let assets = resolve_static_dir_diagnostic(None, &env_map, &cwd)?;
    let web_assets = json!({
        "ok": assets.found(),
        "path": assets.path.display().to_string(),
        "source": assets.source,
        "searched": assets.searched.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "envVar": "PSYCHEVO_WEB_DIST",
        "buildCommand": static_dir_build_command(),
        "installCommand": static_dir_install_command(),
    });
    let gateway = managed_status_for_home(&home)
        .await
        .unwrap_or_else(|err| json!({ "ok": false, "error": format!("{err:#}") }));
    let tools = json!({
        "git": tool_value("git", &env_map),
        "rg": tool_value("rg", &env_map),
        "pnpm": tool_value("pnpm", &env_map),
    });
    let live = if args.live {
        live_checks(options.as_ref().ok()).await
    } else {
        json!({ "enabled": false })
    };

    let ok = home_initialized
        && config["ok"].as_bool().unwrap_or(false)
        && model["ok"].as_bool().unwrap_or(false)
        && auth["ok"].as_bool().unwrap_or(false)
        && web_assets["ok"].as_bool().unwrap_or(false);

    Ok(json!({
        "ok": ok,
        "live": live,
        "paths": {
            "cwd": cwd,
            "home": home,
            "homeInitialized": home_initialized,
            "homeConfig": home_config,
            "stateDb": state_db,
            "explicitConfig": explicit_config,
        },
        "config": config,
        "model": model,
        "auth": auth,
        "webAssets": web_assets,
        "gateway": gateway,
        "tools": tools,
    }))
}

fn capture_value(f: impl FnOnce() -> Result<Value>) -> Value {
    match f() {
        Ok(value) => {
            if value.get("ok").is_some() {
                value
            } else {
                json!({ "ok": true, "value": value })
            }
        }
        Err(err) => json!({ "ok": false, "error": format!("{err:#}") }),
    }
}

async fn live_checks(options: Option<&psychevo_runtime::RunOptions>) -> Value {
    let Some(options) = options else {
        return json!({ "enabled": true, "ok": false, "error": "local configuration is not available" });
    };
    let providers = match model_catalog_providers(options) {
        Ok(providers) => providers,
        Err(err) => {
            return json!({ "enabled": true, "ok": false, "error": format!("{err:#}") });
        }
    };
    let mut rows = Vec::new();
    for provider in providers
        .into_iter()
        .filter(|provider| provider.fetchable())
    {
        match fetch_model_catalog(&provider).await {
            Ok(models) => rows.push(json!({
                "provider": provider.provider,
                "ok": true,
                "modelCount": models.len(),
            })),
            Err(err) => rows.push(json!({
                "provider": provider.provider,
                "ok": false,
                "error": format!("{err:#}"),
            })),
        }
    }
    let ok = rows.iter().all(|row| row["ok"].as_bool().unwrap_or(false));
    json!({ "enabled": true, "ok": ok, "providers": rows })
}

fn tool_value(name: &str, env_map: &std::collections::BTreeMap<String, String>) -> Value {
    let path = find_on_path(name, env_map);
    json!({
        "ok": path.is_some(),
        "path": path,
    })
}

fn find_on_path(
    name: &str,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let path = env_map.get("PATH")?;
    for dir in env::split_paths(path) {
        for candidate in executable_candidates(&dir, name) {
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    None
}

fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) && !name.ends_with(".exe") {
        vec![dir.join(name), dir.join(format!("{name}.exe"))]
    } else {
        vec![dir.join(name)]
    }
}

fn print_human_report(report: &Value) {
    println!("ok: {}", report["ok"].as_bool().unwrap_or(false));
    println!("home: {}", report["paths"]["home"].as_str().unwrap_or("-"));
    println!(
        "home_initialized: {}",
        report["paths"]["homeInitialized"]
            .as_bool()
            .unwrap_or(false)
    );
    println!(
        "web_assets: {} ({})",
        if report["webAssets"]["ok"].as_bool().unwrap_or(false) {
            "ok"
        } else {
            "missing"
        },
        report["webAssets"]["path"].as_str().unwrap_or("-")
    );
    if let Some(model) = report["model"]["value"]["model"].as_object() {
        println!(
            "model: {}/{}",
            model.get("provider").and_then(Value::as_str).unwrap_or("-"),
            model.get("model").and_then(Value::as_str).unwrap_or("-")
        );
    } else {
        println!("model: -");
    }
    println!(
        "gateway_running: {}",
        report["gateway"]["running"].as_bool().unwrap_or(false)
    );
    if !report["webAssets"]["ok"].as_bool().unwrap_or(false) {
        println!(
            "web_build: {}",
            report["webAssets"]["buildCommand"].as_str().unwrap_or("-")
        );
        println!(
            "web_install: {}",
            report["webAssets"]["installCommand"]
                .as_str()
                .unwrap_or("-")
        );
    }
}
