use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use psychevo_runtime::{
    PluginInstallOptions, PluginMarketplaceEntry, PluginScope, plugin_doctor_value,
    plugin_install_value, plugin_list_value, plugin_marketplace_add_value,
    plugin_marketplace_list_value, plugin_marketplace_remove_value, plugin_set_enabled_value,
    plugin_uninstall_value, plugin_view_value,
};
use serde_json::Value;

use crate::args::{
    PluginArgs, PluginCommand, PluginDoctorArgs, PluginInstallArgs, PluginListArgs,
    PluginMarketplaceArgs, PluginMarketplaceCommand, PluginViewArgs,
};
use crate::commands::common::base_run_options;
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) fn run_plugin_command(args: PluginArgs) -> Result<ExitCode> {
    let Some(command) = args.command else {
        PluginArgs::command().print_help()?;
        println!();
        return Ok(ExitCode::SUCCESS);
    };

    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);

    match command {
        PluginCommand::List(args) => list_plugins(args, &env_map, &home, &cwd)?,
        PluginCommand::View(args) => view_plugin(args, &env_map, &home, &cwd)?,
        PluginCommand::Doctor(args) => doctor_plugins(args, &env_map, &home, &cwd)?,
        PluginCommand::Install(args) => install_plugin(args, &home, &cwd)?,
        PluginCommand::Uninstall(args) => {
            let value = plugin_uninstall_value(
                &home,
                &cwd,
                write_scope(args.global, args.local),
                &args.selector,
            )?;
            print_plugin_value(&value, args.json)?;
        }
        PluginCommand::Enable(args) => {
            let value = plugin_set_enabled_value(
                &home,
                &cwd,
                write_scope(args.global, args.local),
                &args.selector,
                true,
            )?;
            print_plugin_value(&value, args.json)?;
        }
        PluginCommand::Disable(args) => {
            let value = plugin_set_enabled_value(
                &home,
                &cwd,
                write_scope(args.global, args.local),
                &args.selector,
                false,
            )?;
            print_plugin_value(&value, args.json)?;
        }
        PluginCommand::Marketplace(args) => marketplace_command(args, &home, &cwd)?,
    }

    Ok(ExitCode::SUCCESS)
}

fn list_plugins(
    args: PluginListArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let options = base_run_options(env_map, home, cwd)?;
    let value = plugin_list_value(&options)?;
    print_plugin_value(&value, args.json)
}

fn view_plugin(
    args: PluginViewArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let options = base_run_options(env_map, home, cwd)?;
    let value = plugin_view_value(&options, &args.selector)?;
    print_plugin_value(&value, args.json)
}

fn doctor_plugins(
    args: PluginDoctorArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let options = base_run_options(env_map, home, cwd)?;
    let value = plugin_doctor_value(&options, args.selector.as_deref())?;
    print_plugin_value(&value, args.json)
}

fn install_plugin(
    args: PluginInstallArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let value = plugin_install_value(
        home,
        cwd,
        PluginInstallOptions {
            source: args.source,
            scope: write_scope(args.global, args.local),
            git_ref: args.git_ref,
            force: args.force,
        },
    )?;
    print_plugin_value(&value, args.json)
}

fn marketplace_command(
    args: PluginMarketplaceArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    match args.command {
        PluginMarketplaceCommand::List(args) => {
            let value =
                plugin_marketplace_list_value(home, cwd, write_scope(args.global, args.local))?;
            print_plugin_value(&value, args.json)?;
        }
        PluginMarketplaceCommand::Add(args) => {
            let value = plugin_marketplace_add_value(
                home,
                cwd,
                write_scope(args.global, args.local),
                PluginMarketplaceEntry {
                    name: args.name,
                    source: args.source,
                    kind: args.kind,
                    git_ref: args.git_ref,
                },
            )?;
            print_plugin_value(&value, args.json)?;
        }
        PluginMarketplaceCommand::Remove(args) => {
            let value = plugin_marketplace_remove_value(
                home,
                cwd,
                write_scope(args.global, args.local),
                &args.name,
            )?;
            print_plugin_value(&value, args.json)?;
        }
    }
    Ok(())
}

fn write_scope(_global: bool, local: bool) -> PluginScope {
    if local {
        PluginScope::Local
    } else {
        PluginScope::Global
    }
}

fn print_plugin_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }
    if let Some(plugins) = value.get("plugins").and_then(Value::as_array) {
        if plugins.is_empty() {
            println!("No plugins found.");
            return Ok(());
        }
        for plugin in plugins {
            if let Some(row) = plugin.get("plugin").unwrap_or(plugin).as_object() {
                let name = row.get("name").and_then(Value::as_str).unwrap_or("plugin");
                let version = row.get("version").and_then(Value::as_str).unwrap_or("");
                let scope = row.get("scope").and_then(Value::as_str).unwrap_or("");
                let enabled = row.get("enabled").and_then(Value::as_bool).unwrap_or(false);
                println!(
                    "{name} {version} [{scope}] {}",
                    if enabled { "enabled" } else { "disabled" }
                );
            }
        }
        return Ok(());
    }
    if let Some(plugin) = value.get("plugin") {
        let name = plugin
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("plugin");
        let version = plugin.get("version").and_then(Value::as_str).unwrap_or("");
        let scope = plugin.get("scope").and_then(Value::as_str).unwrap_or("");
        println!("{name} {version} [{scope}]");
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
