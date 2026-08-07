use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use psychevo::extensions::protocol::{
    CommandEffect, CommandRunParams, ExtensionSurface, HostAction,
};
use psychevo::extensions::{
    ExtensionCommandCatalog, ExtensionHostMode, ExtensionInstallRecord, ExtensionManifest,
    ExtensionRuntime, ExtensionScope, ExtensionStore, load_extension_manifest,
};
use serde_json::json;

use crate::args::{
    Cli, ExtensionInstallArgs, ExtensionListArgs, ExtensionRemoveArgs, ExtensionUpdateArgs,
};
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) async fn run_install_command(args: ExtensionInstallArgs) -> Result<ExitCode> {
    let (store, _) = open_store()?;
    let scope = scope(args.local);
    let source_path = PathBuf::from(&args.source);
    let record = if source_path.is_dir() {
        store.install_local(&source_path, scope)?
    } else {
        let descriptor =
            first_party_descriptor(&args.source).unwrap_or_else(|| args.source.clone());
        store.install_remote(&descriptor, scope).await?
    };
    print_record("installed", &record, args.json)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_remove_command(args: ExtensionRemoveArgs) -> Result<ExitCode> {
    let (store, _) = open_store()?;
    let selected_scope = scope(args.local);
    let Some(record) = store.remove(&args.selector, selected_scope)? else {
        bail!(
            "Extension `{}` is not installed in {} scope",
            args.selector,
            selected_scope.as_str()
        );
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "success": true,
                "removed": record,
                "dataRetained": true,
            }))?
        );
    } else {
        println!(
            "Removed Extension `{}` from {} scope. Data retained at {}.",
            record.id,
            record.scope.as_str(),
            record.data_root.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_list_command(args: ExtensionListArgs) -> Result<ExitCode> {
    let (store, _) = open_store()?;
    let selected_scope = scope(args.local);
    let records = store.records(selected_scope)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "scope": selected_scope.as_str(),
                "extensions": records,
            }))?
        );
    } else if records.is_empty() {
        println!(
            "No Extensions installed in {} scope.",
            selected_scope.as_str()
        );
    } else {
        for record in records {
            println!(
                "{} {} [{}] {} {}",
                record.id,
                record.version,
                record.scope.as_str(),
                if record.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                if record.fingerprint == record.trusted_fingerprint {
                    "trusted"
                } else {
                    "changed"
                }
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn run_update_command(args: ExtensionUpdateArgs) -> Result<ExitCode> {
    let (store, _) = open_store()?;
    if !args.extensions && !args.all && args.selector.is_none() {
        bail!(
            "This pevo binary has no safe self-updater. Reinstall from the source checkout with `scripts/install.sh`."
        );
    }
    let mut results = Vec::new();
    if args.all {
        results.push(json!({
            "kind": "pevo",
            "status": "manual",
            "guidance": "Reinstall from the source checkout with scripts/install.sh"
        }));
    }
    let records = if let Some(selector) = args.selector.as_deref() {
        let record = store
            .effective_records()?
            .into_iter()
            .find(|record| record.id == selector)
            .with_context(|| format!("Extension `{selector}` is not installed"))?;
        vec![record]
    } else {
        store.effective_records()?
    };
    for record in records {
        if record.source_kind == "local" {
            results.push(json!({
                "kind": "extension",
                "id": record.id,
                "status": "unchanged",
                "reason": "local in-place Extension"
            }));
            continue;
        }
        let updated = store
            .install_remote(&record.source, record.scope)
            .await
            .with_context(|| format!("failed to update Extension `{}`", record.id))?;
        results.push(json!({
            "kind": "extension",
            "id": updated.id,
            "version": updated.version,
            "status": "updated"
        }));
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"results": results}))?
        );
    } else {
        for result in results {
            let id = result
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("pevo");
            let status = result
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            println!("{id}: {status}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn run_external_command(
    arguments: Vec<OsString>,
    temporary_paths: Vec<PathBuf>,
) -> Result<ExitCode> {
    let (store, env) = open_store()?;
    let mut records = store
        .effective_records()?
        .into_iter()
        .filter(|record| record.enabled)
        .collect::<Vec<_>>();
    let mut manifests = Vec::new();
    for record in &records {
        let manifest = load_extension_manifest(&record.package_root)?;
        let fingerprint = psychevo::plugins::external_plugin_fingerprint(
            Some(&record.package_root),
            &record.id,
            Some(&record.version),
        )?;
        if fingerprint != record.fingerprint || fingerprint != record.trusted_fingerprint {
            bail!(
                "Extension `{}` changed after installation; reinstall it before use",
                record.id
            );
        }
        manifests.push(manifest);
    }

    let mut temporary_data = Vec::new();
    for path in temporary_paths {
        let root = path
            .canonicalize()
            .with_context(|| format!("failed to resolve temporary Extension {}", path.display()))?;
        let manifest = load_extension_manifest(&root)?;
        let data = tempfile::TempDir::new()?;
        let fingerprint = psychevo::plugins::external_plugin_fingerprint(
            Some(&root),
            &manifest.id,
            Some(&manifest.version),
        )?;
        records.push(ExtensionInstallRecord {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            scope: ExtensionScope::Profile,
            source: root.display().to_string(),
            source_kind: "temporary".to_string(),
            package_root: root,
            data_root: data.path().to_path_buf(),
            fingerprint: fingerprint.clone(),
            trusted_fingerprint: fingerprint,
            enabled: true,
            manifest_path: manifest.manifest_path.clone(),
            plugin_manifest: manifest.plugin_manifest.clone(),
        });
        manifests.push(manifest);
        temporary_data.push(data);
    }

    let mut arguments = arguments.into_iter();
    let command = arguments
        .next()
        .context("missing Extension command")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("Extension command name must be UTF-8"))?;
    let argv = arguments
        .map(|value| {
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("Extension command arguments must be UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;
    let cli_command = Cli::command();
    let builtins = cli_command
        .get_subcommands()
        .map(|command| command.get_name())
        .collect::<Vec<_>>();
    let catalog = ExtensionCommandCatalog::build(&manifests, &builtins)?;
    let Some(owner) = catalog.owner(&command) else {
        if let Some(extension) = first_party_command_owner(&command) {
            bail!(
                "Command `{command}` requires first-party Extension `{extension}`. Install it with `pevo install {extension}`."
            );
        }
        bail!("unrecognized pevo command `{command}`");
    };
    let index = manifests
        .iter()
        .position(|manifest| manifest.id == owner)
        .context("Extension command owner disappeared")?;
    let manifest: ExtensionManifest = manifests.swap_remove(index);
    let record_index = records
        .iter()
        .position(|record| record.id == owner)
        .context("Extension command record disappeared")?;
    let record = records.swap_remove(record_index);
    let runtime = ExtensionRuntime::new(record, manifest, env, ExtensionHostMode::OneShot)?;
    let lease = runtime.acquire().await?;
    let effect = lease
        .command_run(CommandRunParams {
            command,
            args: argv,
            cwd: std::env::current_dir()?.canonicalize()?,
            surface: ExtensionSurface::Cli,
            interactive: std::io::IsTerminal::is_terminal(&std::io::stdin()),
            terminal: std::io::IsTerminal::is_terminal(&std::io::stdout()),
            host_capabilities: Default::default(),
        })
        .await;
    let release = lease.release().await;
    let effect = effect?;
    release?;
    print_effect(effect)?;
    drop(temporary_data);
    Ok(ExitCode::SUCCESS)
}

fn open_store() -> Result<(ExtensionStore, BTreeMap<String, String>)> {
    let env = inherited_env();
    let cwd = std::env::current_dir()?;
    let home = resolve_psychevo_home(&env, &cwd)?;
    ensure_home_initialized(&home)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    Ok((ExtensionStore::new(home, cwd), env))
}

fn scope(local: bool) -> ExtensionScope {
    if local {
        ExtensionScope::Local
    } else {
        ExtensionScope::Profile
    }
}

fn print_record(action: &str, record: &ExtensionInstallRecord, json_output: bool) -> Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "success": true,
                "action": action,
                "extension": record,
            }))?
        );
    } else {
        println!(
            "{} Extension `{}` {} in {} scope (enabled, trusted {}).",
            capitalize(action),
            record.id,
            record.version,
            record.scope.as_str(),
            &record.fingerprint[..12]
        );
        if let Some(plugin) = &record.plugin_manifest {
            println!(
                "Co-root Plugin found at {}; add and enable it separately if needed.",
                plugin.display()
            );
        }
    }
    Ok(())
}

fn print_effect(effect: CommandEffect) -> Result<()> {
    match effect {
        CommandEffect::BoundedText { text } | CommandEffect::PromptSubmission { text } => {
            println!("{text}");
        }
        CommandEffect::StructuredDisplay {
            schema,
            value,
            fallback,
        } => {
            println!("{fallback}");
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"schema": schema, "value": value}))?
            );
        }
        CommandEffect::Artifact {
            name, media_type, ..
        } => {
            bail!(
                "Extension returned artifact `{name}` ({media_type}); CLI artifact writes require an explicit output contract"
            );
        }
        CommandEffect::HostRequest { action, .. } => match action {
            HostAction::OpenResource
            | HostAction::RequestApproval
            | HostAction::StartChannel
            | HostAction::StopChannel => {
                bail!("Extension requested a host action unavailable on the CLI")
            }
        },
    }
    Ok(())
}

fn first_party_descriptor(id: &str) -> Option<String> {
    let artifact = match id {
        "psychevo.channel.wechat" => "psychevo.channel.wechat.release.json",
        "psychevo.channel.telegram" => "psychevo.channel.telegram.release.json",
        "psychevo.channel.feishu-lark" => "psychevo.channel.feishu-lark.release.json",
        _ => return None,
    };
    Some(format!(
        "https://github.com/wowfun/psychevo/releases/download/v{}/{artifact}",
        env!("CARGO_PKG_VERSION")
    ))
}

fn first_party_command_owner(_command: &str) -> Option<&'static str> {
    None
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
