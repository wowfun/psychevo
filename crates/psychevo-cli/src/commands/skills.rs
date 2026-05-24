use std::env;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use clap::CommandFactory;
use psychevo_runtime::{
    InstallOptions, ListSkillsOptions, SaveSkillBundleOptions, SkillBundle, SkillCatalog,
    SkillDiscoveryOptions, SkillTarget, delete_skill_bundle, discover_skills, install_skill,
    list_skill_bundles, list_skills_value_with_options, remove_installed_skill, save_skill_bundle,
    scan_skill_path, set_skill_config_value, set_skill_enabled, view_skill_value,
};
use serde_json::{Value, json};

use crate::args::{
    SkillsArgs, SkillsAuditArgs, SkillsBundleArgs, SkillsBundleCommand, SkillsBundleCreateArgs,
    SkillsBundleDeleteArgs, SkillsBundleNameArgs, SkillsCommand, SkillsConfigArgs,
    SkillsConfigCommand, SkillsConfigSetArgs, SkillsInspectArgs, SkillsInstallArgs, SkillsListArgs,
    SkillsNameArgs, SkillsNameScopeArgs, SkillsPublishArgs, SkillsQueryArgs, SkillsViewArgs,
};
use crate::env::{
    ensure_home_initialized, inherited_env, resolve_explicit_path, resolve_psychevo_home,
};

pub(crate) fn run_skills_command(args: SkillsArgs) -> Result<ExitCode> {
    let Some(command) = args.command else {
        SkillsArgs::command().print_help()?;
        println!();
        return Ok(ExitCode::SUCCESS);
    };

    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);

    match command {
        SkillsCommand::List(args) => list_skills(args, &home, &workdir, env_map)?,
        SkillsCommand::View(args) => view_skill(args, &home, &workdir, env_map)?,
        SkillsCommand::Browse(args) => query_skills("browse", args, &home, &workdir, env_map)?,
        SkillsCommand::Search(args) => query_skills("search", args, &home, &workdir, env_map)?,
        SkillsCommand::Inspect(args) => inspect_skill(args, &home, &workdir, env_map)?,
        SkillsCommand::Install(args) => install_skill_cli(args, &home, &workdir, &env_map)?,
        SkillsCommand::Check(args) => print_value(
            json!({"success": true, "updates": [], "message": "no hub update source configured"}),
            args.json,
        )?,
        SkillsCommand::Update(args) => print_value(
            json!({"success": true, "updated": [], "message": "hub update is not configured for this source"}),
            args.json,
        )?,
        SkillsCommand::Audit(args) => audit_skills(args, &home, &workdir, &env_map)?,
        SkillsCommand::Uninstall(args) => uninstall_skill(args, &home, &workdir, env_map)?,
        SkillsCommand::Publish(args) => publish_skill(args, &env_map, &workdir)?,
        SkillsCommand::Config(args) => config_command(args, &home, &workdir, env_map)?,
        SkillsCommand::Bundle(args) => bundle_command(args, &home, &workdir)?,
        SkillsCommand::Snapshot(args) => print_value(
            json!({"success": true, "message": "hub snapshots are CLI-only and no snapshot backend is configured"}),
            args.json,
        )?,
        SkillsCommand::Tap(args) => print_value(
            json!({"success": true, "message": "hub taps are CLI-only and no tap backend is configured"}),
            args.json,
        )?,
        SkillsCommand::Reset(args) => print_value(
            json!({"success": true, "message": "bundled manifest reset has no bundled seed state in this build"}),
            args.json,
        )?,
    }

    Ok(ExitCode::SUCCESS)
}

pub(crate) fn list_skills(
    args: SkillsListArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let catalog = catalog(home, workdir, env_map)?;
    let value = list_skills_value_with_options(&catalog, &list_options(&args));
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        print_skill_list(&value, args.detail);
        print_diagnostics(&catalog)?;
    }
    Ok(())
}

pub(crate) fn view_skill(
    args: SkillsViewArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let catalog = catalog(home, workdir, env_map)?;
    let value = view_skill_value(&catalog, &args.name, args.file_path.as_deref())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "{}",
            value
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
    }
    Ok(())
}

pub(crate) fn query_skills(
    action: &str,
    args: SkillsQueryArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let catalog = catalog(home, workdir, env_map)?;
    let query = args.query.unwrap_or_default().to_lowercase();
    let skills = catalog
        .skills
        .iter()
        .filter(|skill| {
            query.is_empty()
                || skill.name.to_lowercase().contains(&query)
                || skill.description.to_lowercase().contains(&query)
        })
        .take(args.limit)
        .map(|skill| {
            json!({
                "name": skill.name,
                "description": skill.description,
                "source": skill.source.as_str(),
                "path": skill.file_path,
            })
        })
        .collect::<Vec<_>>();
    let value = json!({"success": true, "action": action, "skills": skills});
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_skill_rows(
            value["skills"].as_array().cloned().unwrap_or_default(),
            false,
        );
    }
    Ok(())
}

pub(crate) fn inspect_skill(
    args: SkillsInspectArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let catalog = catalog(home, workdir, env_map)?;
    let value = view_skill_value(&catalog, &args.identifier, None)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("name: {}", value["name"].as_str().unwrap_or("-"));
        println!(
            "description: {}",
            value["description"].as_str().unwrap_or("-")
        );
        println!("source: {}", value["source"].as_str().unwrap_or("-"));
        println!("readiness: {}", value["readiness"].as_str().unwrap_or("-"));
        if let Some(path) = value["path"].as_str() {
            println!("path: {path}");
        }
    }
    Ok(())
}

pub(crate) fn install_skill_cli(
    args: SkillsInstallArgs,
    home: &Path,
    workdir: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let source = normalize_install_source(&args.source, env_map, workdir)?;
    let value = install_skill(
        home,
        workdir,
        InstallOptions {
            source,
            target: target_from_install(&args),
            name: args.name,
            all: args.all,
            force: args.force,
        },
    )?;
    print_value(value, args.json)
}

pub(crate) fn audit_skills(
    args: SkillsAuditArgs,
    home: &Path,
    workdir: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let value = if let Some(path) = args.path {
        let path = resolve_explicit_path(&path, env_map, workdir)?;
        scan_skill_path(&path).map(|scan| json!({"success": true, "scan": scan, "path": path}))?
    } else {
        let catalog = catalog(home, workdir, env_map.clone())?;
        let scans = catalog
            .skills
            .iter()
            .map(|skill| {
                scan_skill_path(&skill.base_dir)
                    .map(|scan| json!({"name": skill.name, "path": skill.base_dir, "scan": scan}))
            })
            .collect::<psychevo_runtime::Result<Vec<_>>>()?;
        json!({"success": true, "scans": scans})
    };
    print_value(value, args.json)
}

pub(crate) fn uninstall_skill(
    args: SkillsNameArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    drop(env_map);
    let value = remove_installed_skill(home, workdir, target_from_uninstall(&args), &args.name)?;
    print_value(value, args.json)
}

pub(crate) fn publish_skill(
    args: SkillsPublishArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    workdir: &Path,
) -> Result<()> {
    let path = resolve_explicit_path(&args.path, env_map, workdir)?;
    let scan = scan_skill_path(&path)?;
    if scan.verdict == psychevo_runtime::ScanVerdict::Dangerous {
        return Err(anyhow!(
            "cannot publish a skill with dangerous scanner verdict"
        ));
    }
    print_value(
        json!({
            "success": false,
            "error": "GitHub PR publish requires hub authentication flow",
            "path": path,
            "repo": args.repo,
            "scan": scan,
        }),
        args.json,
    )
}

pub(crate) fn config_command(
    args: SkillsConfigArgs,
    home: &Path,
    workdir: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    match args.command {
        SkillsConfigCommand::Status(args) => {
            let catalog = catalog(home, workdir, env_map)?;
            let value = list_skills_value_with_options(
                &catalog,
                &ListSkillsOptions {
                    include_hidden: true,
                    detail: true,
                    ..ListSkillsOptions::default()
                },
            );
            print_value(value, args.json)
        }
        SkillsConfigCommand::Enable(args) => set_enabled(args, home, workdir, true),
        SkillsConfigCommand::Disable(args) => set_enabled(args, home, workdir, false),
        SkillsConfigCommand::Set(args) => set_config_value(args, home, workdir),
    }
}

pub(crate) fn set_enabled(
    args: SkillsNameScopeArgs,
    home: &Path,
    workdir: &Path,
    enabled: bool,
) -> Result<()> {
    let value = set_skill_enabled(home, workdir, target_from_scope(&args), &args.name, enabled)?;
    print_value(value, args.json)
}

pub(crate) fn set_config_value(
    args: SkillsConfigSetArgs,
    home: &Path,
    workdir: &Path,
) -> Result<()> {
    let value =
        serde_json::from_str(&args.value).unwrap_or_else(|_| Value::String(args.value.clone()));
    let result = set_skill_config_value(
        home,
        workdir,
        target_from_config_set(&args),
        &args.key,
        value,
    )?;
    print_value(result, args.json)
}

pub(crate) fn bundle_command(args: SkillsBundleArgs, home: &Path, workdir: &Path) -> Result<()> {
    match args.command {
        SkillsBundleCommand::List(args) => {
            let bundles = list_skill_bundles(home, workdir)?;
            print_bundles(bundles, args.json)
        }
        SkillsBundleCommand::Show(args) => show_bundle(args, home, workdir),
        SkillsBundleCommand::Create(args) => create_bundle(args, home, workdir),
        SkillsBundleCommand::Delete(args) => delete_bundle(args, home, workdir),
        SkillsBundleCommand::Reload(args) => {
            let bundles = list_skill_bundles(home, workdir)?;
            print_value(
                json!({"success": true, "count": bundles.len(), "bundles": bundles}),
                args.json,
            )
        }
    }
}

pub(crate) fn show_bundle(args: SkillsBundleNameArgs, home: &Path, workdir: &Path) -> Result<()> {
    let bundle = find_bundle(home, workdir, &args.name)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&bundle)?);
    } else {
        println!("{} ({})", bundle.name, bundle.scope.as_str());
        println!("{}", bundle.description);
        println!("skills: {}", bundle.skills.join(", "));
        if let Some(instruction) = bundle.instruction {
            println!("instruction: {instruction}");
        }
    }
    Ok(())
}

pub(crate) fn create_bundle(
    args: SkillsBundleCreateArgs,
    home: &Path,
    workdir: &Path,
) -> Result<()> {
    let value = save_skill_bundle(
        home,
        workdir,
        SaveSkillBundleOptions {
            target: target_from_bundle_create(&args),
            name: args.name,
            skills: args.skills,
            description: args.description,
            instruction: args.instruction,
            overwrite: args.force,
        },
    )?;
    print_value(value, args.json)
}

pub(crate) fn delete_bundle(
    args: SkillsBundleDeleteArgs,
    home: &Path,
    workdir: &Path,
) -> Result<()> {
    let value = delete_skill_bundle(home, workdir, target_from_bundle_delete(&args), &args.name)?;
    print_value(value, args.json)
}

pub(crate) fn print_bundles(bundles: Vec<SkillBundle>, as_json: bool) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"bundles": bundles}))?
        );
    } else if bundles.is_empty() {
        println!("No skill bundles found.");
    } else {
        for bundle in bundles {
            println!(
                "{}\t{}\t{}\t{}",
                bundle.slug,
                bundle.name,
                bundle.scope.as_str(),
                bundle.skills.join(",")
            );
        }
    }
    Ok(())
}

pub(crate) fn print_value(value: Value, _as_json: bool) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

pub(crate) fn print_skill_list(value: &Value, detail: bool) {
    let rows = value["skills"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        println!("No skills found.");
    } else {
        print_skill_rows(rows, detail);
    }
}

pub(crate) fn print_skill_rows(rows: Vec<Value>, detail: bool) {
    for row in rows {
        if detail {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                row["name"].as_str().unwrap_or("-"),
                row["description"].as_str().unwrap_or("-"),
                row["source"].as_str().unwrap_or("-"),
                row["readiness"].as_str().unwrap_or("-"),
                row["path"].as_str().unwrap_or("-")
            );
        } else {
            println!(
                "{}\t{}\t{}",
                row["name"].as_str().unwrap_or("-"),
                row["description"].as_str().unwrap_or("-"),
                row["path"].as_str().unwrap_or("-")
            );
        }
    }
}

pub(crate) fn print_diagnostics(catalog: &SkillCatalog) -> Result<()> {
    if !catalog.diagnostics.is_empty() {
        eprintln!(
            "{}",
            serde_json::to_string(&json!({"diagnostics": catalog.diagnostics}))?
        );
    }
    Ok(())
}

pub(crate) fn list_options(args: &SkillsListArgs) -> ListSkillsOptions {
    ListSkillsOptions {
        include_hidden: args.all,
        detail: args.detail,
        category: args.category.clone(),
        source: args.source.clone(),
        enabled_only: args.enabled_only,
        platform: args.platform.clone(),
        tag: args.tag.clone(),
        readiness: args.readiness.clone(),
        sort: args.sort.clone(),
    }
}

pub(crate) fn catalog(
    home: &Path,
    workdir: &Path,
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

pub(crate) fn find_bundle(home: &Path, workdir: &Path, name: &str) -> Result<SkillBundle> {
    let wanted = normalize_bundle_name(name);
    list_skill_bundles(home, workdir)?
        .into_iter()
        .find(|bundle| bundle.slug == wanted || normalize_bundle_name(&bundle.name) == wanted)
        .ok_or_else(|| anyhow!("bundle not found: {name}"))
}

pub(crate) fn normalize_bundle_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch)
            } else if ch == '-' || ch == '_' || ch.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn target_from_scope(args: &SkillsNameScopeArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn target_from_config_set(args: &SkillsConfigSetArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn target_from_bundle_create(args: &SkillsBundleCreateArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn target_from_bundle_delete(args: &SkillsBundleDeleteArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn target_from_install(args: &SkillsInstallArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn target_from_uninstall(args: &SkillsNameArgs) -> SkillTarget {
    if args.global {
        SkillTarget::Global
    } else {
        SkillTarget::Project
    }
}

pub(crate) fn normalize_install_source(
    source: &str,
    env_map: &std::collections::BTreeMap<String, String>,
    workdir: &Path,
) -> Result<String> {
    if source_looks_like_git(source) {
        return Ok(source.to_string());
    }
    Ok(resolve_explicit_path(Path::new(source), env_map, workdir)?
        .to_string_lossy()
        .to_string())
}

pub(crate) fn source_looks_like_git(source: &str) -> bool {
    source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("file://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || source.ends_with(".git")
}
