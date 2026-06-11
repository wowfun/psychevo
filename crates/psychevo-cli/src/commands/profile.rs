use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow, bail};
use psychevo_runtime::SqliteStore;
use serde_json::{Value, json};

use crate::args::{
    ProfileAliasArgs, ProfileArgs, ProfileCommand, ProfileCreateArgs, ProfileDeleteArgs,
    ProfileListArgs, ProfileRenameArgs, ProfileShowArgs, ProfileUseArgs,
};
use crate::commands::init::{STARTER_CONFIG, STARTER_ENV};
use crate::env::inherited_env;
use crate::profiles::{
    DEFAULT_PROFILE, ProfileMetadata, ProfileSummary, active_profile_name_for_env, alias_dir,
    copy_profile_setup, list_profiles, named_profile_home, profile_for_name,
    profile_for_name_unchecked, read_sticky_profile, registry_root, remove_alias,
    validate_profile_name, validate_profile_selector, write_alias, write_metadata,
    write_sticky_profile,
};

pub(crate) fn run_profile_command(args: ProfileArgs) -> Result<ExitCode> {
    let command = args
        .command
        .unwrap_or(ProfileCommand::List(ProfileListArgs { json: false }));
    match command {
        ProfileCommand::List(args) => list(args),
        ProfileCommand::Show(args) => show(args),
        ProfileCommand::Create(args) => create(args),
        ProfileCommand::Use(args) => use_profile(args),
        ProfileCommand::Delete(args) => delete(args),
        ProfileCommand::Rename(args) => rename(args),
        ProfileCommand::Alias(args) => alias(args),
    }
}

fn list(args: ProfileListArgs) -> Result<ExitCode> {
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let active = active_profile_name_for_env(&inherited_env(), &cwd);
    let profiles = list_profiles(&root, Some(&active))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&profiles_json(&profiles))?
        );
    } else {
        for profile in profiles {
            let marker = if profile.active { "*" } else { " " };
            let description = profile.metadata.description.unwrap_or_default();
            if description.is_empty() {
                println!("{marker} {:<16} {}", profile.name, profile.home.display());
            } else {
                println!(
                    "{marker} {:<16} {}  {}",
                    profile.name,
                    profile.home.display(),
                    description
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn show(args: ProfileShowArgs) -> Result<ExitCode> {
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let active = active_profile_name_for_env(&inherited_env(), &cwd);
    let name = args.name.unwrap_or(active.clone());
    validate_profile_selector(&name)?;
    let profile = if name == DEFAULT_PROFILE {
        profile_for_name_unchecked(&root, DEFAULT_PROFILE)?
    } else {
        profile_for_name(&root, &name)?
    };
    let summaries = list_profiles(&root, Some(&active))?;
    let summary = summaries
        .into_iter()
        .find(|summary| summary.name == profile.name)
        .ok_or_else(|| anyhow!("profile `{}` does not exist", profile.name))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&profile_json(&summary))?);
    } else {
        println!("name: {}", summary.name);
        println!("home: {}", summary.home.display());
        println!("active: {}", summary.active);
        if let Some(description) = summary.metadata.description {
            println!("description: {description}");
        }
        if let Some(description) = summary.metadata.description_auto {
            println!("description_auto: {description}");
        }
        if let Some(error) = summary.metadata_error {
            println!("metadata_error: {error}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn create(args: ProfileCreateArgs) -> Result<ExitCode> {
    validate_profile_name(&args.name)?;
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let home = named_profile_home(&root, &args.name);
    if home.exists() {
        bail!(
            "profile `{}` already exists at {}",
            args.name,
            home.display()
        );
    }
    let clone_source = clone_source(&args, &cwd, &root)?;
    fs::create_dir_all(&home)?;
    create_profile_home(&home, args.description.as_deref())?;
    if let Some(source) = clone_source {
        copy_profile_setup(&source, &home)?;
    }
    let alias_path = if let Some(alias_value) = args.alias.as_deref() {
        let command = if alias_value.is_empty() {
            args.name.as_str()
        } else {
            alias_value
        };
        Some(write_alias(
            &alias_dir(&env_map)?,
            command,
            &args.name,
            &env_map,
        )?)
    } else {
        None
    };

    println!("created: {}", args.name);
    println!("home: {}", home.display());
    if let Some(path) = alias_path {
        println!("alias: {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn use_profile(args: ProfileUseArgs) -> Result<ExitCode> {
    validate_profile_selector(&args.name)?;
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    if args.name != DEFAULT_PROFILE {
        let _ = profile_for_name(&root, &args.name)?;
    }
    write_sticky_profile(&root, &args.name)?;
    println!("active_profile: {}", args.name);
    Ok(ExitCode::SUCCESS)
}

fn delete(args: ProfileDeleteArgs) -> Result<ExitCode> {
    validate_profile_name(&args.name)?;
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let active = active_profile_name_for_env(&inherited_env(), &cwd);
    if active == args.name || read_sticky_profile(&root)?.as_deref() == Some(args.name.as_str()) {
        bail!("cannot delete active profile `{}`", args.name);
    }
    let profile = profile_for_name(&root, &args.name)?;
    if !args.yes {
        if io::stdin().is_terminal() {
            bail!(
                "deleting profile `{}` requires --yes in this non-interactive command path",
                args.name
            );
        }
        bail!("deleting profile `{}` requires --yes", args.name);
    }
    fs::remove_dir_all(&profile.home)?;
    println!("deleted: {}", args.name);
    Ok(ExitCode::SUCCESS)
}

fn rename(args: ProfileRenameArgs) -> Result<ExitCode> {
    validate_profile_name(&args.old)?;
    validate_profile_name(&args.new)?;
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let old = profile_for_name(&root, &args.old)?;
    let new = profile_for_name_unchecked(&root, &args.new)?;
    if new.home.exists() {
        bail!(
            "profile `{}` already exists at {}",
            args.new,
            new.home.display()
        );
    }
    fs::rename(&old.home, &new.home)?;
    if read_sticky_profile(&root)?.as_deref() == Some(args.old.as_str()) {
        write_sticky_profile(&root, &args.new)?;
    }
    println!("renamed: {} -> {}", args.old, args.new);
    println!("home: {}", new.home.display());
    Ok(ExitCode::SUCCESS)
}

fn alias(args: ProfileAliasArgs) -> Result<ExitCode> {
    validate_profile_name(&args.profile)?;
    let env_map = raw_env();
    let cwd = env::current_dir()?;
    let root = registry_root(&env_map, &cwd)?;
    let _ = profile_for_name(&root, &args.profile)?;
    let command = args.name.as_deref().unwrap_or(&args.profile);
    let dir = alias_dir(&env_map)?;
    if args.remove {
        let removed = remove_alias(&dir, command, &args.profile)?;
        println!("removed: {removed}");
    } else {
        let path = write_alias(&dir, command, &args.profile, &env_map)?;
        println!("alias: {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn clone_source(args: &ProfileCreateArgs, cwd: &Path, root: &Path) -> Result<Option<PathBuf>> {
    if !args.clone && args.clone_from.is_none() {
        return Ok(None);
    }
    let source = if let Some(name) = args.clone_from.as_deref() {
        profile_for_name(root, name)?
    } else {
        crate::profiles::resolve_active_profile(&inherited_env(), cwd)?
    };
    if source.name == args.name {
        bail!("cannot clone profile `{}` from itself", args.name);
    }
    Ok(Some(source.home))
}

fn create_profile_home(home: &Path, description: Option<&str>) -> Result<()> {
    fs::create_dir_all(home)?;
    fs::create_dir_all(home.join("sessions"))?;
    fs::create_dir_all(home.join("logs"))?;
    fs::create_dir_all(home.join("cache"))?;
    fs::create_dir_all(home.join("skills"))?;
    fs::create_dir_all(home.join("agents"))?;
    write_if_absent(&home.join("config.toml"), STARTER_CONFIG)?;
    write_if_absent(&home.join(".env"), STARTER_ENV)?;
    crate::profiles::protect_env_file(&home.join(".env"))?;
    SqliteStore::open(&home.join("state.db"))?;
    write_metadata(
        home,
        &ProfileMetadata {
            description: description.map(str::to_string),
            description_auto: None,
        },
    )?;
    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents)?;
    Ok(())
}

fn raw_env() -> BTreeMap<String, String> {
    env::vars().collect()
}

fn profiles_json(profiles: &[ProfileSummary]) -> Value {
    json!({
        "profiles": profiles.iter().map(profile_json).collect::<Vec<_>>(),
    })
}

fn profile_json(profile: &ProfileSummary) -> Value {
    json!({
        "name": profile.name,
        "home": profile.home,
        "active": profile.active,
        "default": profile.is_default,
        "description": profile.metadata.description,
        "descriptionAuto": profile.metadata.description_auto,
        "metadataError": profile.metadata_error,
    })
}
