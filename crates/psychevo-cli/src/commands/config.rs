use std::env;
use std::fs;
use std::process::{Command, ExitCode};

use anyhow::{Result, anyhow};
use psychevo::{
    Configuration, CreateCustomProviderRequest, config::ConfigScope, paths::canonicalize_cwd,
};
use serde_json::{Value, json};

use crate::args::{
    ConfigArgs, ConfigCommand, ConfigEditArgs, ConfigJsonArgs, ConfigPermissionRemoveArgs,
    ConfigPermissionsArgs, ConfigPermissionsCommand, ConfigProviderAddArgs, ConfigProviderArgs,
    ConfigProviderCommand, ConfigSetArgs, ConfigShowArgs,
};
use crate::commands::common::{CommandConfiguration, print_json_error, read_secret_from_stdin};
use crate::env::{env_path, inherited_env, resolve_psychevo_home, resolve_state_db};

pub(crate) async fn run_config_command(args: ConfigArgs) -> Result<ExitCode> {
    match run_config_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if config_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) async fn run_config_command_inner(args: &ConfigArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    match &args.command {
        ConfigCommand::Path(args) => print_paths(args, &env_map, &home, &cwd)?,
        ConfigCommand::Edit(args) => edit_config(args, &home, &cwd)?,
        command => {
            let context = CommandConfiguration::open(&env_map, &home, &cwd).await?;
            let result = run_configuration_command(command, context.configuration());
            context.finish(result).await?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_configuration_command(command: &ConfigCommand, configuration: &Configuration) -> Result<()> {
    match command {
        ConfigCommand::Show(args) => {
            let value = configuration.config_value(config_scope(args))?;
            print_config_document(&value, args.json)
        }
        ConfigCommand::Set(args) => set_config(args, configuration),
        ConfigCommand::Validate(args) => validate_config(args, configuration),
        ConfigCommand::Doctor(args) | ConfigCommand::Status(args) => {
            doctor_config(args, configuration)
        }
        ConfigCommand::Provider(args) => run_provider_command(args, configuration),
        ConfigCommand::Permissions(args) => run_permissions_command(args, configuration),
        ConfigCommand::Path(_) | ConfigCommand::Edit(_) => unreachable!("handled without state"),
    }
}

pub(crate) fn edit_config(
    args: &ConfigEditArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let config_dir = if args.global {
        home.to_path_buf()
    } else {
        canonicalize_cwd(cwd)?.join(".psychevo")
    };
    fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("config.toml");
    if !path.exists() {
        fs::write(&path, "")?;
    }
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(editor).arg(&path).status()?;
    if !status.success() {
        anyhow::bail!("editor exited with status {status}");
    }
    Ok(())
}

pub(crate) fn set_config(args: &ConfigSetArgs, configuration: &Configuration) -> Result<()> {
    let scope = mutation_scope(args.global);
    if let Some(provider) = api_key_provider_from_key(&args.key) {
        let api_key = parse_config_set_string_value(&args.value)?;
        let result = configuration.set_provider_api_key(scope, &provider, &api_key)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "wrote api key env: {}",
                result["api_key_env"].as_str().unwrap_or("-")
            );
            println!("path: {}", result["env_path"].as_str().unwrap_or("-"));
        }
        return Ok(());
    }
    let value = parse_toml_literal(&args.value)?;
    let result = configuration.set_value(scope, &args.key, value)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "path": result.path,
                "key": result.key,
                "changed": result.changed,
            }))?
        );
    } else {
        println!(
            "{} config: {}",
            if result.changed {
                "updated"
            } else {
                "unchanged"
            },
            result.key
        );
        println!("path: {}", result.path.display());
    }
    Ok(())
}

pub(crate) fn parse_toml_literal(value: &str) -> Result<Value> {
    let parsed: toml::Value = toml::from_str(&format!("value = {value}\n"))?;
    let value = parsed
        .get("value")
        .cloned()
        .ok_or_else(|| anyhow!("failed to parse TOML literal"))?;
    Ok(serde_json::to_value(value)?)
}

pub(crate) fn parse_config_set_string_value(value: &str) -> Result<String> {
    match parse_toml_literal(value)? {
        Value::String(value) => Ok(value),
        _ => anyhow::bail!("API key config values must be TOML strings"),
    }
}

pub(crate) fn api_key_provider_from_key(key: &str) -> Option<String> {
    let parts = key.split('.').collect::<Vec<_>>();
    if parts.len() == 4
        && parts[0] == "provider"
        && parts[2] == "options"
        && matches!(parts[3], "api_key" | "apiKey")
    {
        Some(parts[1].to_string())
    } else {
        None
    }
}

pub(crate) fn validate_config(args: &ConfigShowArgs, configuration: &Configuration) -> Result<()> {
    let value = configuration.permission_rules(config_scope(args))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "scope": value["scope"],
                "path": value["path"],
            }))?
        );
    } else {
        println!("config ok");
        if let Some(path) = value["path"].as_str() {
            println!("path: {path}");
        }
    }
    Ok(())
}

pub(crate) fn doctor_config(args: &ConfigShowArgs, configuration: &Configuration) -> Result<()> {
    let config = configuration.config_value(config_scope(args))?;
    let permissions = configuration.permission_rules(config_scope(args))?;
    let value = json!({
        "scope": config["scope"],
        "path": config["path"],
        "sources": config["sources"],
        "exists": config["exists"],
        "permissions": permissions["permissions"],
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        if let Some(path) = value["path"].as_str() {
            println!("path: {path}");
        }
        let permissions = &value["permissions"];
        println!(
            "approval_policy: {}",
            permissions["approval_policy"].as_str().unwrap_or("-")
        );
        println!(
            "approvals_reviewer: {}",
            permissions["approvals_reviewer"].as_str().unwrap_or("-")
        );
        println!(
            "default_permissions: {}",
            permissions["default_permissions"].as_str().unwrap_or("-")
        );
    }
    Ok(())
}

pub(crate) fn print_paths(
    args: &ConfigJsonArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let value = json!({
        "home": home,
        "global_config": home.join("config.toml"),
        "global_env": home.join(".env"),
        "local_dir": cwd.join(".psychevo"),
        "local_config": cwd.join(".psychevo").join("config.toml"),
        "local_env": cwd.join(".psychevo").join(".env"),
        "state_db": resolve_state_db(env_map, home, cwd)?,
        "explicit_config": env_path("PSYCHEVO_CONFIG", env_map, cwd)?,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        for key in [
            "home",
            "global_config",
            "global_env",
            "local_dir",
            "local_config",
            "local_env",
            "state_db",
            "explicit_config",
        ] {
            if value[key].is_null() {
                println!("{key}: -");
            } else {
                println!("{key}: {}", value[key].as_str().unwrap_or("-"));
            }
        }
    }
    Ok(())
}

pub(crate) fn run_provider_command(
    args: &ConfigProviderArgs,
    configuration: &Configuration,
) -> Result<()> {
    match &args.command {
        ConfigProviderCommand::List(args) => {
            let value = configuration.provider_list(config_scope(args))?;
            print_provider_list(&value, args.json)
        }
        ConfigProviderCommand::Add(args) => add_provider(args, configuration),
    }
}

pub(crate) fn run_permissions_command(
    args: &ConfigPermissionsArgs,
    configuration: &Configuration,
) -> Result<()> {
    match &args.command {
        ConfigPermissionsCommand::List(args) => {
            let value = configuration.permission_rules(ConfigScope::Local)?;
            print_permissions_list(&value, args.json)
        }
        ConfigPermissionsCommand::Remove(args) => remove_permission_rule(args, configuration),
    }
}

pub(crate) fn remove_permission_rule(
    args: &ConfigPermissionRemoveArgs,
    configuration: &Configuration,
) -> Result<()> {
    let result = configuration.remove_local_permission_rule(args.kind.as_str(), &args.rule)?;
    let value = json!({
        "scope": "local",
        "path": result.config_path,
        "kind": result.kind,
        "rule": result.rule,
        "changed": result.changed,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else if result.changed {
        println!("removed {} rule: {}", result.kind, result.rule);
        println!("path: {}", result.config_path.display());
    } else {
        println!("permission rule not found: {}", result.rule);
        println!("path: {}", result.config_path.display());
    }
    Ok(())
}

pub(crate) fn add_provider(
    args: &ConfigProviderAddArgs,
    configuration: &Configuration,
) -> Result<()> {
    let api_key = read_secret_from_stdin(args.api_key_stdin)?;
    let result = configuration.create_custom_provider(
        mutation_scope(args.global),
        CreateCustomProviderRequest {
            provider_id: args.id.clone(),
            label: args.label.clone(),
            base_url: args.base_url.clone(),
            api_key_env: args.api_key_env.clone(),
            require_api_key: args.api_key_env.is_none() && api_key.is_none(),
            api_key,
            no_auth: args.no_auth,
        },
    )?;
    let value = json!({
        "scope": scope_label(mutation_scope(args.global)),
        "provider": result.provider_id,
        "label": result.label,
        "base_url": result.base_url,
        "api_key_env": result.api_key_env,
        "no_auth": args.no_auth,
        "wrote_api_key": result.wrote_api_key,
        "reused_existing_api_key": result.reused_existing_api_key,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("provider: {}", value["provider"].as_str().unwrap_or("-"));
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        println!(
            "api_key_env: {}",
            value["api_key_env"].as_str().unwrap_or("-")
        );
        println!(
            "wrote_api_key: {}",
            value["wrote_api_key"].as_bool().unwrap_or(false)
        );
    }
    Ok(())
}

pub(crate) fn print_config_document(value: &Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        if let Some(path) = value["path"].as_str() {
            println!("path: {path}");
        }
        if let Some(sources) = value["sources"].as_array()
            && !sources.is_empty()
        {
            println!("sources:");
            for source in sources {
                println!("  {}", source.as_str().unwrap_or("-"));
            }
        }
        println!("{}", serde_json::to_string_pretty(&value["value"])?);
    }
    Ok(())
}

pub(crate) fn print_provider_list(value: &Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        let rows = value["providers"].as_array().cloned().unwrap_or_default();
        if rows.is_empty() {
            println!("No configured providers found.");
        } else {
            println!("Provider\tLabel\tBase URL\tAPI key env\tModels");
            for row in rows {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    row["provider"].as_str().unwrap_or("-"),
                    row["label"].as_str().unwrap_or("-"),
                    row["base_url"].as_str().unwrap_or("-"),
                    row["api_key_env"].as_str().unwrap_or("-"),
                    row["models"]
                        .as_array()
                        .map(|models| models.len())
                        .unwrap_or(0)
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn print_permissions_list(value: &Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        if let Some(path) = value["path"].as_str() {
            println!("path: {path}");
        }
        let permissions = &value["permissions"];
        println!(
            "approval_policy: {}",
            permissions["approval_policy"].as_str().unwrap_or("-")
        );
        println!(
            "approvals_reviewer: {}",
            permissions["approvals_reviewer"].as_str().unwrap_or("-")
        );
        println!(
            "default_permissions: {}",
            permissions["default_permissions"].as_str().unwrap_or("-")
        );
        println!("profiles:");
        let profiles = permissions["profiles"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        if profiles.is_empty() {
            println!("  -");
        } else {
            for key in profiles.keys() {
                println!("  {key}");
            }
        }
        println!("exec_policy:");
        let rules = permissions["exec_policy"]["rules"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if rules.is_empty() {
            println!("  -");
        } else {
            for rule in rules {
                let prefix = rule["prefix"]
                    .as_array()
                    .map(|values| format_exec_prefix(values))
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "  {} -> {}",
                    prefix,
                    rule["decision"].as_str().unwrap_or("-")
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn format_exec_prefix(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(|value| match value {
            Value::String(raw) => Some(raw.clone()),
            Value::Array(alternatives) => Some(format!(
                "[{}]",
                alternatives
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|")
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn config_scope(args: &ConfigShowArgs) -> ConfigScope {
    if args.global {
        ConfigScope::Global
    } else if args.local {
        ConfigScope::Local
    } else {
        ConfigScope::Effective
    }
}

fn mutation_scope(global: bool) -> ConfigScope {
    if global {
        ConfigScope::Global
    } else {
        ConfigScope::Local
    }
}

fn scope_label(scope: ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Global => "global",
        ConfigScope::Local => "local",
        ConfigScope::Effective => "effective",
    }
}

pub(crate) fn config_json(args: &ConfigArgs) -> bool {
    match &args.command {
        ConfigCommand::Path(args) => args.json,
        ConfigCommand::Show(args) => args.json,
        ConfigCommand::Edit(_) => false,
        ConfigCommand::Set(args) => args.json,
        ConfigCommand::Validate(args)
        | ConfigCommand::Doctor(args)
        | ConfigCommand::Status(args) => args.json,
        ConfigCommand::Provider(args) => match &args.command {
            ConfigProviderCommand::List(args) => args.json,
            ConfigProviderCommand::Add(args) => args.json,
        },
        ConfigCommand::Permissions(args) => match &args.command {
            ConfigPermissionsCommand::List(args) => args.json,
            ConfigPermissionsCommand::Remove(args) => args.json,
        },
    }
}
