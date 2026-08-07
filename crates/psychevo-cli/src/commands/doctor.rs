use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use psychevo::{Configuration, config::ConfigScope};
use serde_json::{Value, json};

use crate::args::DoctorArgs;
use crate::commands::common::CommandConfiguration;
#[cfg(feature = "gateway")]
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

    let configuration = CommandConfiguration::open(&env_map, &home, &cwd).await;
    let (config, model, auth, live) = match configuration {
        Ok(context) => {
            let configuration = context.configuration();
            let config = capture_value(|| Ok(configuration.config_value(ConfigScope::Effective)?));
            let model = capture_value(|| {
                Ok(json!({
                    "model": configuration.selected_model()?.as_ref().map(model_value),
                }))
            });
            let auth = capture_value(|| Ok(configuration.auth_status(None)?));
            let live = if args.live {
                live_checks(configuration).await
            } else {
                json!({ "enabled": false })
            };
            context.finish(Ok(())).await?;
            (config, model, auth, live)
        }
        Err(err) => {
            let unavailable = json!({ "ok": false, "error": format!("{err:#}") });
            let live = if args.live {
                json!({ "enabled": true, "ok": false, "error": "local configuration is not available" })
            } else {
                json!({ "enabled": false })
            };
            (unavailable.clone(), unavailable.clone(), unavailable, live)
        }
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
    #[cfg(feature = "gateway")]
    let gateway = managed_status_for_home(&home)
        .await
        .unwrap_or_else(|err| json!({ "ok": false, "error": format!("{err:#}") }));
    #[cfg(not(feature = "gateway"))]
    let gateway = json!({"ok": false, "available": false, "reason": "gateway feature omitted"});
    let tools = json!({
        "git": tool_value("git", &env_map),
        "rg": tool_value("rg", &env_map),
        "pnpm": tool_value("pnpm", &env_map),
    });
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

async fn live_checks(configuration: &Configuration) -> Value {
    let providers = match configuration.model_catalog_providers() {
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
        match configuration.fetch_model_catalog(&provider).await {
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
