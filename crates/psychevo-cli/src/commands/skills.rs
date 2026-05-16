use std::env;
use std::process::ExitCode;

use anyhow::Result;
use psychevo_runtime::{
    InstallOptions, SkillCatalog, SkillDiscoveryOptions, SkillTarget, create_skill,
    discover_skills, install_skill, list_skills_value, patch_skill, remove_skill, scan_skill_path,
    set_skill_enabled, view_skill_value,
};
use serde_json::json;

use crate::args::{
    SkillsArgs, SkillsCommand, SkillsCreateArgs, SkillsInstallArgs, SkillsListArgs,
    SkillsNameScopeArgs,
};
use crate::env::{
    ensure_home_initialized, inherited_env, resolve_explicit_path, resolve_psychevo_home,
};

pub(crate) fn run_skills_command(args: SkillsArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    if !matches!(
        args.command,
        SkillsCommand::Create(_) | SkillsCommand::Install(_)
    ) {
        ensure_home_initialized(&home)?;
    }
    let workdir = cwd.canonicalize().unwrap_or(cwd);

    match args.command {
        SkillsCommand::List(list) => list_skills(list, &home, &workdir, env_map)?,
        SkillsCommand::View(view) => {
            let catalog = catalog(&home, &workdir, env_map)?;
            let value = view_skill_value(&catalog, &view.name, view.file_path.as_deref())?;
            println!(
                "{}",
                value
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            );
        }
        SkillsCommand::Create(create) => {
            let value = create_skill(
                &home,
                &workdir,
                target_from_create(&create),
                &create.name,
                &create.description,
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Patch(patch) => {
            let catalog = catalog(&home, &workdir, env_map)?;
            let value = patch_skill(
                &catalog,
                &home,
                &workdir,
                &patch.name,
                &patch.old,
                &patch.new,
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Remove(remove) => {
            let catalog = catalog(&home, &workdir, env_map)?;
            let value = remove_skill(&catalog, &home, &workdir, &remove.name)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Enable(scope) => {
            let value = set_skill_enabled(
                &home,
                &workdir,
                target_from_scope(&scope),
                &scope.name,
                true,
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Disable(scope) => {
            let value = set_skill_enabled(
                &home,
                &workdir,
                target_from_scope(&scope),
                &scope.name,
                false,
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Install(install) => {
            let target = target_from_install(&install);
            let source = normalize_install_source(&install.source, &env_map, &workdir)?;
            let value = install_skill(
                &home,
                &workdir,
                InstallOptions {
                    source,
                    target,
                    name: install.name,
                    all: install.all,
                    force: install.force,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SkillsCommand::Scan(scan) => {
            let path = resolve_explicit_path(&scan.path, &env_map, &workdir)?;
            let value = scan_skill_path(&path)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn list_skills(
    args: SkillsListArgs,
    home: &std::path::Path,
    workdir: &std::path::Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let catalog = catalog(home, workdir, env_map)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&list_skills_value(&catalog, args.all))?
        );
    } else if catalog.skills.is_empty() {
        println!("No skills found.");
    } else {
        for skill in catalog
            .skills
            .iter()
            .filter(|skill| args.all || !skill.disable_model_invocation)
        {
            println!(
                "{}\t{}\t{}",
                skill.name,
                skill.description,
                skill.file_path.display()
            );
        }
        if !catalog.diagnostics.is_empty() {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({"diagnostics": catalog.diagnostics}))?
            );
        }
    }
    Ok(())
}

fn catalog(
    home: &std::path::Path,
    workdir: &std::path::Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<SkillCatalog> {
    discover_skills(&SkillDiscoveryOptions {
        home: home.to_path_buf(),
        workdir: workdir.to_path_buf(),
        config_path: None,
        env: env_map,
        explicit_inputs: Vec::new(),
        no_skills: false,
    })
    .map_err(Into::into)
}

fn target_from_create(args: &SkillsCreateArgs) -> SkillTarget {
    if args.local {
        SkillTarget::Project
    } else {
        SkillTarget::Global
    }
}

fn target_from_scope(args: &SkillsNameScopeArgs) -> SkillTarget {
    if args.local {
        SkillTarget::Project
    } else {
        SkillTarget::Global
    }
}

fn target_from_install(args: &SkillsInstallArgs) -> SkillTarget {
    if args.local {
        SkillTarget::Project
    } else {
        SkillTarget::Global
    }
}

fn normalize_install_source(
    source: &str,
    env_map: &std::collections::BTreeMap<String, String>,
    workdir: &std::path::Path,
) -> Result<String> {
    if source_looks_like_git(source) {
        return Ok(source.to_string());
    }
    Ok(
        resolve_explicit_path(std::path::Path::new(source), env_map, workdir)?
            .to_string_lossy()
            .to_string(),
    )
}

fn source_looks_like_git(source: &str) -> bool {
    source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("file://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || source.ends_with(".git")
}
