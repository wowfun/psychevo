use std::env;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo_runtime::{
    config::auth_status_value, config::create_scoped_custom_provider, config::set_default_model,
    config::set_provider_api_key, types::ScopedCustomProviderInput,
};
use serde_json::Value;

use crate::args::{AuthArgs, AuthCommand, AuthSetArgs, AuthSetupArgs, AuthStatusArgs};
use crate::commands::common::{
    base_run_options, print_json_error, read_secret_from_stdin, scoped_config_dir,
};
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) fn run_auth_command(args: AuthArgs) -> Result<ExitCode> {
    match run_auth_command_inner(&args) {
        Ok(code) => Ok(code),
        Err(err) if auth_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn run_auth_command_inner(args: &AuthArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let options = base_run_options(&env_map, &home, &cwd)?;
    match &args.command {
        AuthCommand::Status(args) => auth_status(args, &options),
        AuthCommand::Setup(args) => auth_setup(args, &home, &cwd),
        AuthCommand::Set(args) => auth_set(args, &options, &home, &cwd),
    }
}

pub(crate) fn auth_status(
    args: &AuthStatusArgs,
    options: &psychevo_runtime::types::RunOptions,
) -> Result<ExitCode> {
    let value = auth_status_value(options, args.provider.as_deref())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_auth_status(&value);
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn auth_setup(
    args: &AuthSetupArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<ExitCode> {
    if args.api_kind != "openai-compatible" {
        return Err(anyhow!(
            "pevo auth setup only supports --api-kind openai-compatible"
        ));
    }
    let api_key = read_secret_from_stdin(args.api_key_stdin)?;
    if !args.no_auth && args.api_key_env.is_none() && api_key.is_none() {
        return Err(anyhow!(
            "interactive setup is unavailable; use --api-key-stdin, --api-key-env, or --no-auth"
        ));
    }
    let base_url = args
        .base_url
        .clone()
        .ok_or_else(|| anyhow!("pevo auth setup requires --base-url"))?;
    let label = args.label.clone().unwrap_or_else(|| args.provider.clone());
    let global = args.global || !args.local;
    let result = create_scoped_custom_provider(ScopedCustomProviderInput {
        config_dir: scoped_config_dir(home, cwd, global)?,
        provider_id: args.provider.clone(),
        label,
        base_url: base_url.clone(),
        api_key_env: args.api_key_env.clone(),
        api_key,
        require_api_key: !args.no_auth && args.api_key_env.is_none(),
        no_auth: args.no_auth,
    })?;
    let model_spec = format!("{}/{}", result.provider_id, args.model);
    let model_value = set_default_model(home, cwd, global, &model_spec)?;
    let warnings = if args.no_auth
        && !(base_url.starts_with("http://127.0.0.1")
            || base_url.starts_with("http://localhost")
            || base_url.starts_with("http://[::1]"))
    {
        vec!["no_auth is enabled for a non-loopback URL".to_string()]
    } else {
        Vec::new()
    };
    let value = serde_json::json!({
        "scope": if global { "global" } else { "local" },
        "provider": result.provider_id,
        "model": model_spec,
        "base_url": result.base_url,
        "api_key_env": result.api_key_env,
        "no_auth": args.no_auth,
        "wrote_api_key": result.wrote_api_key,
        "reused_existing_api_key": result.reused_existing_api_key,
        "default_model": model_value,
        "warnings": warnings,
        "fetch": args.fetch && !args.no_fetch,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("provider: {}", value["provider"].as_str().unwrap_or("-"));
        println!("model: {}", value["model"].as_str().unwrap_or("-"));
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn auth_set(
    args: &AuthSetArgs,
    options: &psychevo_runtime::types::RunOptions,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<ExitCode> {
    if !args.api_key_stdin {
        return Err(anyhow!("pevo auth set requires --api-key-stdin"));
    }
    let api_key = read_secret_from_stdin(true)?.expect("required stdin secret");
    let value = set_provider_api_key(
        options,
        scoped_config_dir(home, cwd, args.global)?,
        &args.provider,
        &api_key,
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("provider: {}", value["provider"].as_str().unwrap_or("-"));
        println!(
            "api_key_env: {}",
            value["api_key_env"].as_str().unwrap_or("-")
        );
        println!("env_path: {}", value["env_path"].as_str().unwrap_or("-"));
        println!(
            "replaced_existing: {}",
            value["replaced_existing"].as_bool().unwrap_or(false)
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn print_auth_status(value: &Value) {
    let rows = value["providers"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        println!("No providers found.");
        return;
    }
    println!("Provider\tStatus\tAPI key env");
    for row in rows {
        println!(
            "{}\t{}\t{}",
            row["provider"].as_str().unwrap_or("-"),
            row["status"].as_str().unwrap_or("-"),
            row["api_key_env"].as_str().unwrap_or("-")
        );
    }
}

pub(crate) fn auth_json(args: &AuthArgs) -> bool {
    match &args.command {
        AuthCommand::Status(args) => args.json,
        AuthCommand::Setup(args) => args.json,
        AuthCommand::Set(args) => args.json,
    }
}
