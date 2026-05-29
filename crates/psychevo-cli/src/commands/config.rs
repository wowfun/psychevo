use std::env;
use std::fs;
use std::process::{Command, ExitCode};

use anyhow::{Result, anyhow};
use psychevo_runtime::{
    ConfigScope, ScopedCustomProviderInput, config_provider_list_value, config_show_value,
    create_scoped_custom_provider, permission_rules_value, remove_local_permission_rule,
    set_config_value, set_provider_api_key,
};
use serde_json::{Value, json};

use crate::args::{
    ConfigArgs, ConfigCommand, ConfigEditArgs, ConfigJsonArgs, ConfigPermissionRemoveArgs,
    ConfigPermissionsArgs, ConfigPermissionsCommand, ConfigProviderAddArgs, ConfigProviderArgs,
    ConfigProviderCommand, ConfigSetArgs, ConfigShowArgs,
};
use crate::commands::common::{
    base_run_options, config_scope_dir, print_json_error, read_secret_from_stdin, scope_label,
    scoped_config_dir, scoped_label,
};
use crate::env::{env_path, inherited_env, resolve_psychevo_home, resolve_state_db};

pub(crate) fn run_config_command(args: ConfigArgs) -> Result<ExitCode> {
    match run_config_command_inner(&args) {
        Ok(code) => Ok(code),
        Err(err) if config_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn run_config_command_inner(args: &ConfigArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    match &args.command {
        ConfigCommand::Path(args) => print_paths(args, &env_map, &home, &cwd)?,
        ConfigCommand::Show(args) => {
            let options = base_run_options(&env_map, &home, &cwd)?;
            let value = config_show_value(&options, config_scope(args))?;
            print_config_document(&value, args.json)?;
        }
        ConfigCommand::Edit(args) => edit_config(args, &home, &cwd)?,
        ConfigCommand::Set(args) => set_config(args, &env_map, &home, &cwd)?,
        ConfigCommand::Validate(args) => validate_config(args, &env_map, &home, &cwd)?,
        ConfigCommand::Doctor(args) => doctor_config(args, &env_map, &home, &cwd)?,
        ConfigCommand::Status(args) => doctor_config(args, &env_map, &home, &cwd)?,
        ConfigCommand::Provider(args) => run_provider_command(args, &env_map, &home, &cwd)?,
        ConfigCommand::Permissions(args) => run_permissions_command(args, &env_map, &home, &cwd)?,
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn edit_config(
    args: &ConfigEditArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let config_dir = scoped_config_dir(home, cwd, args.global)?;
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

pub(crate) fn set_config(
    args: &ConfigSetArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let config_dir = scoped_config_dir(home, cwd, args.global)?;
    let options = base_run_options(env_map, home, cwd)?;
    if let Some(provider) = api_key_provider_from_key(&args.key) {
        let api_key = parse_config_set_string_value(&args.value)?;
        let result = set_provider_api_key(&options, config_dir, &provider, &api_key)?;
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
    let result = set_config_value(config_dir, &args.key, value)?;
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

pub(crate) fn validate_config(
    args: &ConfigShowArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let options = base_run_options(env_map, home, cwd)?;
    let value = permission_rules_value(&options, config_scope(args))?;
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

pub(crate) fn doctor_config(
    args: &ConfigShowArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let options = base_run_options(env_map, home, cwd)?;
    let config = config_show_value(&options, config_scope(args))?;
    let permissions = permission_rules_value(&options, config_scope(args))?;
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
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    match &args.command {
        ConfigProviderCommand::List(args) => {
            let options = base_run_options(env_map, home, cwd)?;
            let value = config_provider_list_value(&options, config_scope(args))?;
            print_provider_list(&value, args.json)
        }
        ConfigProviderCommand::Add(args) => add_provider(args, home, cwd),
    }
}

pub(crate) fn run_permissions_command(
    args: &ConfigPermissionsArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    match &args.command {
        ConfigPermissionsCommand::List(args) => {
            let options = base_run_options(env_map, home, cwd)?;
            let value = permission_rules_value(&options, ConfigScope::Local)?;
            print_permissions_list(&value, args.json)
        }
        ConfigPermissionsCommand::Remove(args) => remove_permission_rule(args, home, cwd),
    }
}

pub(crate) fn remove_permission_rule(
    args: &ConfigPermissionRemoveArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let result = remove_local_permission_rule(
        config_scope_dir(home, cwd, true)?,
        args.kind.as_str(),
        &args.rule,
    )?;
    let value = json!({
        "scope": scope_label(true),
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
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let api_key = read_secret_from_stdin(args.api_key_stdin)?;
    let result = create_scoped_custom_provider(ScopedCustomProviderInput {
        config_dir: scoped_config_dir(home, cwd, args.global)?,
        provider_id: args.id.clone(),
        label: args.label.clone(),
        base_url: args.base_url.clone(),
        api_key_env: args.api_key_env.clone(),
        require_api_key: args.api_key_env.is_none() && api_key.is_none(),
        api_key,
        no_auth: args.no_auth,
    })?;
    let value = json!({
        "scope": scoped_label(args.global),
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
