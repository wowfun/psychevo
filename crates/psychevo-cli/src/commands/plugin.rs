use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use psychevo::{
    plugins::PluginInspectOptions, plugins::PluginMarketplaceEntry, plugins::PluginScope,
    plugins::PluginSourceKind, plugins::plugin_import_inspect_value,
    plugins::plugin_marketplace_add_value, plugins::plugin_marketplace_install_value,
    plugins::plugin_marketplace_list_value, plugins::plugin_marketplace_remove_value,
    plugins::plugin_marketplace_upgrade_value, plugins::plugin_set_enabled_value,
    plugins::plugin_uninstall_value,
};
use serde_json::Value;

use crate::args::{
    PluginAddArgs, PluginArgs, PluginCommand, PluginDoctorArgs, PluginInspectArgs, PluginListArgs,
    PluginMarketplaceArgs, PluginMarketplaceCommand, PluginViewArgs,
};
use crate::commands::common::CommandConfiguration;
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) async fn run_plugin_command(args: PluginArgs) -> Result<ExitCode> {
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
        PluginCommand::List(args) => list_plugins(args, &env_map, &home, &cwd).await?,
        PluginCommand::View(args) => view_plugin(args, &env_map, &home, &cwd).await?,
        PluginCommand::Doctor(args) => doctor_plugins(args, &env_map, &home, &cwd).await?,
        PluginCommand::Inspect(args) => inspect_plugin(args, &home, &cwd)?,
        PluginCommand::Add(args) => add_plugin(args, &home, &cwd)?,
        PluginCommand::Remove(args) => {
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

async fn list_plugins(
    args: PluginListArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let context = CommandConfiguration::open(env_map, home, cwd).await?;
    let result = plugin_value_result(context.configuration().plugins(), args.json);
    context.finish(result).await
}

async fn view_plugin(
    args: PluginViewArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let context = CommandConfiguration::open(env_map, home, cwd).await?;
    let result = plugin_value_result(context.configuration().plugin(&args.selector), args.json);
    context.finish(result).await
}

async fn doctor_plugins(
    args: PluginDoctorArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<()> {
    let context = CommandConfiguration::open(env_map, home, cwd).await?;
    let value = context
        .configuration()
        .diagnose_plugins(args.selector.as_deref())
        .await;
    let result = plugin_value_result(value, args.json);
    context.finish(result).await
}

fn plugin_value_result(value: psychevo::Result<Value>, json_output: bool) -> Result<()> {
    print_plugin_value(&value?, json_output)
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

fn add_plugin(args: PluginAddArgs, home: &std::path::Path, cwd: &std::path::Path) -> Result<()> {
    let (plugin_name, marketplace_name) = args.selector.rsplit_once('@').ok_or_else(|| {
        anyhow::anyhow!(
            "plugin selector `{}` must use <plugin>@<marketplace>",
            args.selector
        )
    })?;
    if plugin_name.is_empty() || marketplace_name.is_empty() {
        anyhow::bail!(
            "plugin selector `{}` must use <plugin>@<marketplace>",
            args.selector
        );
    }
    let value = plugin_marketplace_install_value(
        home,
        cwd,
        write_scope(args.global, args.local),
        plugin_name,
        marketplace_name,
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
            let name = args.name.unwrap_or_default();
            let kind = args
                .kind
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| infer_marketplace_kind(&args.source));
            let (source, git_ref) = normalize_marketplace_source(args.source, &kind, args.git_ref)?;
            let value = plugin_marketplace_add_value(
                home,
                cwd,
                write_scope(args.global, args.local),
                PluginMarketplaceEntry {
                    name,
                    source,
                    kind,
                    git_ref,
                    npm_version: args.npm_version,
                    npm_registry: args.npm_registry,
                },
            )?;
            print_plugin_value(&value, args.json)?;
        }
        PluginMarketplaceCommand::Upgrade(args) => {
            let value = plugin_marketplace_upgrade_value(
                home,
                cwd,
                write_scope(args.global, args.local),
                args.name.as_deref(),
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

fn infer_marketplace_kind(source: &str) -> String {
    let source = source.trim();
    if looks_like_local_marketplace_source(source) || std::path::Path::new(source).exists() {
        "local".to_string()
    } else if looks_like_git_marketplace_source(source) {
        "git".to_string()
    } else {
        "npm".to_string()
    }
}

fn looks_like_local_marketplace_source(source: &str) -> bool {
    let bytes = source.as_bytes();
    std::path::Path::new(source).is_absolute()
        || source.starts_with("./")
        || source.starts_with(".\\")
        || source.starts_with("../")
        || source.starts_with("..\\")
        || source.starts_with("~/")
        || matches!(source, "." | "..")
        || source.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

fn looks_like_git_marketplace_source(source: &str) -> bool {
    source.starts_with("https://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || source.ends_with(".git")
        || github_shorthand(source_ref_base(source)).is_some()
}

fn normalize_marketplace_source(
    source: String,
    kind: &str,
    explicit_ref: Option<String>,
) -> Result<(String, Option<String>)> {
    if kind != "git" {
        return Ok((source, explicit_ref));
    }
    let (base, parsed_ref) = split_marketplace_source_ref(source.trim());
    let git_ref = explicit_ref.or(parsed_ref);
    let source = if github_shorthand(&base).is_some() {
        format!("https://github.com/{base}.git")
    } else if base.starts_with("https://github.com/") && !base.ends_with(".git") {
        format!("{}.git", base.trim_end_matches('/'))
    } else {
        base
    };
    Ok((source, git_ref))
}

fn source_ref_base(source: &str) -> &str {
    source
        .rsplit_once('#')
        .map(|(base, _)| base)
        .or_else(|| {
            (!source.contains("://") && !source.starts_with("git@"))
                .then(|| source.rsplit_once('@').map(|(base, _)| base))
                .flatten()
        })
        .unwrap_or(source)
}

fn split_marketplace_source_ref(source: &str) -> (String, Option<String>) {
    if let Some((base, git_ref)) = source.rsplit_once('#') {
        return (
            base.to_string(),
            (!git_ref.trim().is_empty()).then(|| git_ref.trim().to_string()),
        );
    }
    if !source.contains("://")
        && !source.starts_with("git@")
        && let Some((base, git_ref)) = source.rsplit_once('@')
    {
        return (
            base.to_string(),
            (!git_ref.trim().is_empty()).then(|| git_ref.trim().to_string()),
        );
    }
    (source.to_string(), None)
}

fn github_shorthand(source: &str) -> Option<(&str, &str)> {
    let mut parts = source.split('/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    if parts.next().is_some()
        || !github_segment(owner)
        || !github_segment(repository.trim_end_matches(".git"))
    {
        return None;
    }
    Some((owner, repository))
}

fn github_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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

#[cfg(test)]
mod tests {
    use super::{infer_marketplace_kind, normalize_marketplace_source};

    #[test]
    fn marketplace_source_kind_matches_codex_style_forms_and_npm() {
        assert_eq!(infer_marketplace_kind("./marketplace"), "local");
        assert_eq!(infer_marketplace_kind(r"C:\marketplace"), "local");
        assert_eq!(infer_marketplace_kind("owner/repo@main"), "git");
        assert_eq!(
            infer_marketplace_kind("https://github.com/owner/repo"),
            "git"
        );
        assert_eq!(infer_marketplace_kind("@scope/catalog"), "npm");
        assert_eq!(infer_marketplace_kind("catalog-package"), "npm");
    }

    #[test]
    fn marketplace_git_source_normalizes_github_shorthand_and_inline_ref() {
        assert_eq!(
            normalize_marketplace_source("owner/repo@main".to_string(), "git", None)
                .expect("normalize shorthand"),
            (
                "https://github.com/owner/repo.git".to_string(),
                Some("main".to_string())
            )
        );
        assert_eq!(
            normalize_marketplace_source(
                "https://github.com/owner/repo#main".to_string(),
                "git",
                Some("release".to_string()),
            )
            .expect("normalize URL"),
            (
                "https://github.com/owner/repo.git".to_string(),
                Some("release".to_string())
            )
        );
    }
}
