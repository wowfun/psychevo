use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use psychevo::command_registry::{CLI_COMMANDS, SLASH_COMMANDS};
use psychevo::extensions::protocol::{
    CommandDescriptor, CommandEffect, CommandRunParams, ExtensionSurface,
};
use psychevo::extensions::{
    ExtensionCommandCatalog, ExtensionHostMode, ExtensionInstallRecord, ExtensionRuntime,
    ExtensionScope, ExtensionStore, load_extension_manifest,
};
use tempfile::TempDir;

use crate::tui::SlashMenuItem;

const TUI_EXTENSION_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(crate) struct TuiExtensions {
    commands: BTreeMap<String, TuiExtensionCommand>,
    runtimes: BTreeMap<String, Arc<ExtensionRuntime>>,
    _temporary_data: Vec<TempDir>,
}

struct TuiExtensionCommand {
    owner: String,
    descriptor: CommandDescriptor,
}

impl TuiExtensions {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            commands: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            _temporary_data: Vec::new(),
        }
    }

    pub(crate) fn load(
        home: &Path,
        cwd: &Path,
        env: &BTreeMap<String, String>,
        temporary_paths: &[PathBuf],
    ) -> Result<Self> {
        let store = ExtensionStore::new(home, cwd);
        let mut packages = Vec::new();
        let mut ids = BTreeSet::new();
        for record in store
            .effective_records()?
            .into_iter()
            .filter(|record| record.enabled)
        {
            let manifest = load_extension_manifest(&record.package_root)?;
            verify_record_fingerprint(&record)?;
            ids.insert(record.id.clone());
            packages.push((record, manifest));
        }

        let mut temporary_data = Vec::new();
        for path in temporary_paths {
            let root = path.canonicalize().with_context(|| {
                format!("failed to resolve temporary Extension {}", path.display())
            })?;
            let manifest = load_extension_manifest(&root)?;
            if manifest.version != "local" {
                bail!(
                    "temporary Extension `{}` must declare version `local`",
                    manifest.id
                );
            }
            if !ids.insert(manifest.id.clone()) {
                bail!(
                    "temporary Extension `{}` duplicates another effective Extension id",
                    manifest.id
                );
            }
            let data = tempfile::tempdir()?;
            let fingerprint = psychevo::plugins::external_plugin_fingerprint(
                Some(&root),
                &manifest.id,
                Some(&manifest.version),
            )?;
            let record = ExtensionInstallRecord {
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
            };
            packages.push((record, manifest));
            temporary_data.push(data);
        }

        let manifests = packages
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        ExtensionCommandCatalog::build(&manifests, &builtin_command_names())?;

        let mut commands = BTreeMap::new();
        let mut runtimes = BTreeMap::new();
        for (record, manifest) in packages {
            let owner = manifest.id.clone();
            let runtime = ExtensionRuntime::new(
                record,
                manifest.clone(),
                env.clone(),
                ExtensionHostMode::Leased {
                    idle_timeout: TUI_EXTENSION_IDLE_TIMEOUT,
                },
            )?;
            for descriptor in manifest.contributions.commands {
                if descriptor.surfaces.is_empty()
                    || descriptor.surfaces.contains(&ExtensionSurface::Tui)
                {
                    commands.insert(
                        descriptor.name.clone(),
                        TuiExtensionCommand {
                            owner: owner.clone(),
                            descriptor,
                        },
                    );
                }
            }
            runtimes.insert(owner, runtime);
        }

        Ok(Self {
            commands,
            runtimes,
            _temporary_data: temporary_data,
        })
    }

    pub(crate) fn parse_invocation(
        &self,
        command: &str,
        args: &str,
    ) -> Result<Option<(String, Vec<String>)>> {
        let name = command.strip_prefix('/').unwrap_or(command);
        if !self.commands.contains_key(name) {
            return Ok(None);
        }
        Ok(Some((name.to_string(), split_tui_argv(args)?)))
    }

    pub(crate) async fn invoke(
        &self,
        command: String,
        args: Vec<String>,
        cwd: &Path,
        terminal: bool,
    ) -> Result<CommandEffect> {
        let entry = self
            .commands
            .get(&command)
            .ok_or_else(|| anyhow!("unknown Extension command `/{command}`"))?;
        let runtime = self
            .runtimes
            .get(&entry.owner)
            .ok_or_else(|| anyhow!("Extension `{}` runtime is unavailable", entry.owner))?;
        let lease = runtime.acquire().await?;
        let result = lease
            .command_run(CommandRunParams {
                command,
                args,
                cwd: cwd.to_path_buf(),
                surface: ExtensionSurface::Tui,
                interactive: true,
                terminal,
                host_capabilities: Default::default(),
            })
            .await;
        let release = lease.release().await;
        let effect = result?;
        release?;
        Ok(effect)
    }

    pub(crate) fn menu_items(&self) -> Vec<SlashMenuItem> {
        self.commands
            .iter()
            .map(|(name, entry)| {
                let command = format!("/{name}");
                SlashMenuItem {
                    command: command.clone(),
                    description: format!("{} ({})", entry.descriptor.summary, entry.owner),
                    upcoming: false,
                    aliases: Vec::new(),
                    replacement: command.clone(),
                    completion: command,
                    configured_alias: false,
                }
            })
            .collect()
    }

    pub(crate) fn help_rows(&self) -> Vec<String> {
        self.commands
            .iter()
            .map(|(name, entry)| {
                format!("/{name} - {} ({})", entry.descriptor.summary, entry.owner)
            })
            .collect()
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        let mut first_error = None;
        for runtime in self.runtimes.values() {
            if let Err(error) = runtime.shutdown().await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), |error| Err(error.into()))
    }
}

fn verify_record_fingerprint(record: &ExtensionInstallRecord) -> Result<()> {
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
    Ok(())
}

fn builtin_command_names() -> Vec<&'static str> {
    CLI_COMMANDS
        .iter()
        .flat_map(|command| {
            std::iter::once(command.canonical).chain(command.aliases.iter().copied())
        })
        .chain(
            SLASH_COMMANDS
                .iter()
                .flat_map(|command| {
                    std::iter::once(command.canonical).chain(command.aliases.iter().copied())
                })
                .filter_map(|command| command.strip_prefix('/')),
        )
        .collect()
}

fn split_tui_argv(input: &str) -> Result<Vec<String>> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for ch in input.trim().chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            started = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            } else {
                current.push(ch);
            }
            started = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if started {
                output.push(std::mem::take(&mut current));
                started = false;
            }
        } else {
            current.push(ch);
            started = true;
        }
    }
    if escaped {
        bail!("Extension command arguments end with an incomplete escape");
    }
    if let Some(delimiter) = quote {
        bail!("Extension command arguments contain an unclosed `{delimiter}` quote");
    }
    if started {
        output.push(current);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::split_tui_argv;

    #[test]
    fn tui_extension_argv_preserves_quotes_and_escapes() {
        assert_eq!(
            split_tui_argv(r#"one "two words" 'three words' four\ five """#).expect("arguments"),
            ["one", "two words", "three words", "four five", ""]
        );
    }

    #[test]
    fn tui_extension_argv_rejects_incomplete_syntax() {
        assert!(split_tui_argv("one \\").is_err());
        assert!(split_tui_argv("one 'two").is_err());
    }
}
