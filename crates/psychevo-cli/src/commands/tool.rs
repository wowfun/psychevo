use std::env;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo_runtime::{
    ConfigScope, create_local_toolset, remove_local_toolset, set_local_toolset_enabled,
    toolsets_value,
};
use serde_json::{Value, json};

use crate::args::{
    ToolArgs, ToolCommand, ToolCreateArgs, ToolListArgs, ToolModeMutationArgs, ToolRemoveArgs,
    ToolShowArgs,
};
use crate::commands::common::{base_run_options, config_scope_dir, print_json_error, scope_label};
use crate::env::{inherited_env, resolve_psychevo_home};

pub(crate) fn run_tool_command(args: ToolArgs) -> Result<ExitCode> {
    match run_tool_command_inner(&args) {
        Ok(code) => Ok(code),
        Err(err) if tool_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

fn run_tool_command_inner(args: &ToolArgs) -> Result<ExitCode> {
    let Some(command) = args.command.as_ref() else {
        list_toolsets(&ToolListArgs { json: false })?;
        return Ok(ExitCode::SUCCESS);
    };
    match command {
        ToolCommand::List(args) => list_toolsets(args)?,
        ToolCommand::Show(args) => show_toolset(args)?,
        ToolCommand::Enable(args) => set_toolset_enabled(args, true)?,
        ToolCommand::Disable(args) => set_toolset_enabled(args, false)?,
        ToolCommand::Create(args) => create_toolset(args)?,
        ToolCommand::Remove(args) => remove_toolset(args)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn list_toolsets(args: &ToolListArgs) -> Result<()> {
    let (env_map, home, cwd) = command_context()?;
    let options = base_run_options(&env_map, &home, &cwd)?;
    let value = toolsets_value(&options, ConfigScope::Effective)?;
    print_toolsets_value(&value, args.json)
}

fn show_toolset(args: &ToolShowArgs) -> Result<()> {
    let (env_map, home, cwd) = command_context()?;
    let options = base_run_options(&env_map, &home, &cwd)?;
    let value = toolsets_value(&options, ConfigScope::Effective)?;
    let row = value["toolsets"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["name"].as_str() == Some(args.name.as_str()))
        })
        .cloned()
        .ok_or_else(|| anyhow!("unknown toolset: {}", args.name))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&row)?);
    } else {
        print_toolset_row(&row);
    }
    Ok(())
}

fn set_toolset_enabled(args: &ToolModeMutationArgs, enabled: bool) -> Result<()> {
    let (env_map, home, cwd) = command_context()?;
    let result = set_local_toolset_enabled(
        config_scope_dir(&home, &cwd, true)?,
        args.mode.run_mode(),
        &args.name,
        enabled,
    )?;
    let value = json!({
        "scope": scope_label(true),
        "path": result.config_path,
        "name": result.name,
        "mode": args.mode.run_mode().as_str(),
        "enabled": enabled,
        "changed": result.changed,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "{} toolset `{}` for {} mode",
            if enabled { "enabled" } else { "disabled" },
            value["name"].as_str().unwrap_or("-"),
            value["mode"].as_str().unwrap_or("-")
        );
        println!("path: {}", value["path"].as_str().unwrap_or("-"));
    }
    drop(env_map);
    Ok(())
}

fn create_toolset(args: &ToolCreateArgs) -> Result<()> {
    let (_env_map, home, cwd) = command_context()?;
    let result = create_local_toolset(
        config_scope_dir(&home, &cwd, true)?,
        &args.name,
        args.description.clone(),
        args.tools.clone(),
        args.includes.clone(),
        args.force,
    )?;
    let value = json!({
        "scope": scope_label(true),
        "path": result.config_path,
        "name": result.name,
        "changed": result.changed,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("toolset: {}", value["name"].as_str().unwrap_or("-"));
        println!("path: {}", value["path"].as_str().unwrap_or("-"));
        println!("changed: {}", value["changed"].as_bool().unwrap_or(false));
    }
    Ok(())
}

fn remove_toolset(args: &ToolRemoveArgs) -> Result<()> {
    let (_env_map, home, cwd) = command_context()?;
    let result = remove_local_toolset(config_scope_dir(&home, &cwd, true)?, &args.name)?;
    let value = json!({
        "scope": scope_label(true),
        "path": result.config_path,
        "name": result.name,
        "changed": result.changed,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else if value["changed"].as_bool().unwrap_or(false) {
        println!(
            "removed toolset `{}`",
            value["name"].as_str().unwrap_or("-")
        );
        println!("path: {}", value["path"].as_str().unwrap_or("-"));
    } else {
        println!(
            "toolset not found: {}",
            value["name"].as_str().unwrap_or("-")
        );
        println!("path: {}", value["path"].as_str().unwrap_or("-"));
    }
    Ok(())
}

fn command_context() -> Result<(
    std::collections::BTreeMap<String, String>,
    std::path::PathBuf,
    std::path::PathBuf,
)> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    Ok((env_map, home, cwd))
}

fn print_toolsets_value(value: &Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }
    println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
    if let Some(path) = value["path"].as_str() {
        println!("path: {path}");
    }
    for mode in ["plan", "default"] {
        let tools = value["modes"][mode]["effective_tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        println!("{mode}: {tools}");
    }
    println!("toolsets:");
    for row in value["toolsets"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {}\t{}\t{}",
            row["name"].as_str().unwrap_or("-"),
            row["source"].as_str().unwrap_or("-"),
            row["description"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

fn print_toolset_row(row: &Value) {
    println!("name: {}", row["name"].as_str().unwrap_or("-"));
    println!("source: {}", row["source"].as_str().unwrap_or("-"));
    println!(
        "description: {}",
        row["description"].as_str().unwrap_or("-")
    );
    println!("includes: {}", join_strings(&row["includes"]));
    println!("tools: {}", join_strings(&row["tools"]));
    let unknown = join_strings(&row["unknown_tools"]);
    if !unknown.is_empty() {
        println!("unknown_tools: {unknown}");
    }
}

fn join_strings(value: &Value) -> String {
    value
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn tool_json(args: &ToolArgs) -> bool {
    match &args.command {
        Some(ToolCommand::List(args)) => args.json,
        Some(ToolCommand::Show(args)) => args.json,
        Some(ToolCommand::Enable(args)) | Some(ToolCommand::Disable(args)) => args.json,
        Some(ToolCommand::Create(args)) => args.json,
        Some(ToolCommand::Remove(args)) => args.json,
        None => false,
    }
}
