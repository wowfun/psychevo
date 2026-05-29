#[allow(unused_imports)]
use super::*;

pub(crate) fn parse_view_includes(values: &[String]) -> Result<Vec<ViewInclude>> {
    let mut includes = Vec::new();
    for value in values {
        for item in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            if item.eq_ignore_ascii_case("all") {
                includes.extend(all_view_includes());
            } else if item.eq_ignore_ascii_case("atif") {
                anyhow::bail!("view include `atif` was removed in schema v12; use `trajectory`");
            } else if item.eq_ignore_ascii_case("logs") || item.eq_ignore_ascii_case("diff") {
                anyhow::bail!(
                    "view include `{}` was removed in schema v12; use `artifacts` for local artifact paths",
                    item
                );
            } else {
                let include = ViewInclude::from_str(item, true)
                    .map_err(|err| anyhow::anyhow!("invalid view include `{item}`: {err}"))?;
                includes.push(include);
            }
        }
    }
    Ok(includes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

pub(crate) fn parse_view_groups(values: &[String]) -> Result<Vec<ViewGroupBy>> {
    let mut groups = Vec::new();
    for value in values {
        for item in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let group = ViewGroupBy::from_str(item, true)
                .map_err(|err| anyhow::anyhow!("invalid view group `{item}`: {err}"))?;
            groups.push(group);
        }
    }
    Ok(groups
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

pub(crate) fn effective_view_format(
    explicit: Option<ViewFormat>,
    output: Option<&Path>,
    default_output: bool,
) -> Result<ViewFormat> {
    if let Some(format) = explicit {
        return Ok(format);
    }
    let Some(output) = output else {
        let _ = default_output;
        return Ok(ViewFormat::Html);
    };
    match output
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("json") => Ok(ViewFormat::Json),
        Some("html") | Some("htm") => Ok(ViewFormat::Html),
        Some("md") | Some("markdown") => {
            bail!("markdown view output was removed; use --format html or --format json")
        }
        _ => Ok(ViewFormat::Html),
    }
}

pub(crate) fn default_view_output_path(view: &ViewReport, format: ViewFormat) -> Result<PathBuf> {
    let runs_root = view.scope.workspace_root.join("runs");
    let relative_scope = view.scope.path.strip_prefix(&runs_root).with_context(|| {
        format!(
            "default view output requires a scope under {}; pass -o PATH for external paths",
            runs_root.display()
        )
    })?;
    if relative_scope.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!(
            "default view output requires a normalized scope under {}; pass -o PATH",
            runs_root.display()
        );
    }
    let mut output = view.scope.workspace_root.join("views");
    if !relative_scope.as_os_str().is_empty() {
        output = output.join(relative_scope);
    }
    Ok(output.join(format!("index.{}", view_format_extension(format))))
}

pub(crate) fn view_format_extension(format: ViewFormat) -> &'static str {
    match format {
        ViewFormat::Json => "json",
        ViewFormat::Html => "html",
    }
}

pub(crate) fn success(stdout: String) -> CliOutcome {
    CliOutcome {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

pub(crate) fn list_tasks(project: &EvalProject) -> Result<Vec<Value>> {
    let mut tasks = Vec::new();
    let mut seen = BTreeSet::new();
    for task_set in project.task_sets.values() {
        for task in load_task_set_tasks(project, task_set, None)? {
            if seen.insert(task.id.clone()) {
                tasks.push(json!({
                    "id": task.id,
                    "name": task.name,
                    "kind": task.kind,
                    "manifest": task.manifest_path,
                }));
            }
        }
    }
    Ok(tasks)
}

pub(crate) fn resolved_registry_for_cli(store_root: Option<PathBuf>) -> Result<ResolvedRegistry> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    ResolvedRegistry::load(
        None,
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )
}

pub(crate) fn list_registry_agents(store_root: Option<PathBuf>) -> Result<Vec<AgentManifest>> {
    Ok(resolved_registry_for_cli(store_root)?
        .agents
        .into_values()
        .collect())
}

pub(crate) fn list_registry_benchmarks(store_root: Option<PathBuf>) -> Result<Vec<Value>> {
    Ok(resolved_registry_for_cli(store_root)?
        .benchmarks
        .into_values()
        .map(|benchmark| {
            json!({
                "id": benchmark.id,
                "name": benchmark.name,
                "path": benchmark.path,
                "path_exists": benchmark.path.is_file(),
            })
        })
        .collect())
}
