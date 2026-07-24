use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use psychevo_runtime::{
    plugins::PluginInspectOptions, plugins::PluginInstallOptions, plugins::PluginMarketplaceEntry,
    plugins::PluginScope, plugins::PluginSourceKind, plugins::plugin_doctor_value,
    plugins::plugin_import_inspect_value, plugins::plugin_install_value,
    plugins::plugin_list_value, plugins::plugin_marketplace_add_value,
    plugins::plugin_marketplace_list_value, plugins::plugin_marketplace_remove_value,
    plugins::plugin_set_enabled_value, plugins::plugin_uninstall_value, plugins::plugin_view_value,
};
use serde_json::Value;

use crate::args::{
    PluginArgs, PluginCommand, PluginDoctorArgs, PluginInspectArgs, PluginInstallArgs,
    PluginListArgs, PluginMarketplaceArgs, PluginMarketplaceCommand, PluginViewArgs,
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
        PluginCommand::Inspect(args) => inspect_plugin(args, &home, &cwd)?,
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
        PluginCommand::Catalog(args) => marketplace_command(args, &home, &cwd)?,
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

fn inspect_plugin(
    args: PluginInspectArgs,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let value = plugin_import_inspect_value(
        home,
        cwd,
        PluginInspectOptions {
            source: args.source,
            source_kind: parse_source_kind(args.kind.as_deref())?,
            git_ref: args.git_ref,
            npm_version: args.npm_version,
            npm_registry: args.npm_registry,
        },
    )?;
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
            source_kind: parse_source_kind(args.kind.as_deref())?,
            scope: write_scope(args.global, args.local),
            git_ref: args.git_ref,
            npm_version: args.npm_version,
            npm_registry: args.npm_registry,
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
                    npm_version: args.npm_version,
                    npm_registry: args.npm_registry,
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

fn parse_source_kind(value: Option<&str>) -> Result<Option<PluginSourceKind>> {
    value
        .map(|value| {
            PluginSourceKind::parse(value).ok_or_else(|| {
                anyhow::anyhow!("unknown plugin source kind `{value}`; expected local, git, or npm")
            })
        })
        .transpose()
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
                let display_name = plugin
                    .get("manifest")
                    .and_then(|manifest| manifest.get("interface"))
                    .and_then(|interface| interface.get("displayName"))
                    .and_then(Value::as_str);
                let display_suffix = display_name
                    .filter(|display| *display != name)
                    .map(|display| format!(" - {display}"))
                    .unwrap_or_default();
                println!(
                    "{name} {version} [{scope}] {}{display_suffix}",
                    if enabled { "enabled" } else { "disabled" },
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
        if let Some(interface) = value
            .get("manifest")
            .and_then(|manifest| manifest.get("interface"))
            .and_then(Value::as_object)
        {
            print_interface_summary(interface);
        }
        return Ok(());
    }
    if let Some(inspection) = value.get("inspection").and_then(Value::as_object) {
        let name = inspection
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("plugin");
        let framework = inspection
            .get("framework")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let status = inspection
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("Available");
        println!("{name} [{framework}] {status}");
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_interface_summary(interface: &serde_json::Map<String, Value>) {
    if let Some(display_name) = interface.get("displayName").and_then(Value::as_str) {
        println!("Display: {display_name}");
    }
    if let Some(category) = interface.get("category").and_then(Value::as_str) {
        println!("Category: {category}");
    }
    if let Some(capabilities) = interface
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .filter(|items| !items.is_empty())
    {
        println!("Capabilities: {}", capabilities.join(", "));
    }
    if let Some(description) = interface.get("shortDescription").and_then(Value::as_str) {
        println!("{description}");
    }
}
