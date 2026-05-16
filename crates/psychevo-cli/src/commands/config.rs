use std::env;
use std::process::ExitCode;

use anyhow::Result;
use psychevo_runtime::{
    ConfigScope, ScopedCustomProviderInput, config_provider_list_value, config_show_value,
    create_scoped_custom_provider,
};
use serde_json::{Value, json};

use crate::args::{
    ConfigArgs, ConfigCommand, ConfigJsonArgs, ConfigProviderAddArgs, ConfigProviderArgs,
    ConfigProviderCommand, ConfigShowArgs,
};
use crate::commands::common::{
    base_run_options, config_scope_dir, print_json_error, read_secret_from_stdin, scope_label,
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

fn run_config_command_inner(args: &ConfigArgs) -> Result<ExitCode> {
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
        ConfigCommand::Provider(args) => run_provider_command(args, &env_map, &home, &cwd)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn print_paths(
    args: &ConfigJsonArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let value = json!({
        "home": home,
        "global_config": home.join("config.jsonc"),
        "global_env": home.join(".env"),
        "local_dir": cwd.join(".psychevo"),
        "local_config": cwd.join(".psychevo").join("config.jsonc"),
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

fn run_provider_command(
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

fn add_provider(
    args: &ConfigProviderAddArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let api_key = read_secret_from_stdin(args.api_key_stdin)?;
    let result = create_scoped_custom_provider(ScopedCustomProviderInput {
        config_dir: config_scope_dir(home, cwd, args.local)?,
        provider_id: args.id.clone(),
        label: args.label.clone(),
        base_url: args.base_url.clone(),
        api_key_env: args.api_key_env.clone(),
        require_api_key: args.api_key_env.is_none() && api_key.is_none(),
        api_key,
    })?;
    let value = json!({
        "scope": scope_label(args.local),
        "provider": result.provider_id,
        "label": result.label,
        "base_url": result.base_url,
        "api_key_env": result.api_key_env,
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

fn print_config_document(value: &Value, as_json: bool) -> Result<()> {
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

fn print_provider_list(value: &Value, as_json: bool) -> Result<()> {
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

fn config_scope(args: &ConfigShowArgs) -> ConfigScope {
    if args.global {
        ConfigScope::Global
    } else if args.local {
        ConfigScope::Local
    } else {
        ConfigScope::Effective
    }
}

fn config_json(args: &ConfigArgs) -> bool {
    match &args.command {
        ConfigCommand::Path(args) => args.json,
        ConfigCommand::Show(args) => args.json,
        ConfigCommand::Provider(args) => match &args.command {
            ConfigProviderCommand::List(args) => args.json,
            ConfigProviderCommand::Add(args) => args.json,
        },
    }
}
