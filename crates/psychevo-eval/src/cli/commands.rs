#[allow(unused_imports)]
use super::*;

pub(crate) fn run_init(args: InitArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let config = service
        .init(InitStoreRequest {
            root: args.root,
            make_default: args.make_default,
            force: args.force,
        })
        .map_err(anyhow::Error::new)?;
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&config)?));
    }
    let templates = default_eval_template_paths(&config.root)
        .into_iter()
        .map(|path| format!("- {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(success(format!(
        "peval workspace: {}\ndefault workspace: {}\ntemplates:\n{}\n",
        config.root.display(),
        config.default_workspace,
        templates
    )))
}

pub(crate) fn run_project(args: ProjectCommands) -> Result<CliOutcome> {
    let wants_json = match &args {
        ProjectCommands::Add(args) => args.json,
        ProjectCommands::List(args) => args.json,
        ProjectCommands::Remove(args) => args.json,
    };
    let message = "`peval project` is removed; use `--config <eval-config.toml>`, `--benchmark <id-or-path>`, and agent/benchmark registries in eval, workspace, or user config files";
    if wants_json {
        let diagnostic = EvalDiagnostic::error("removed_command", message);
        return Ok(CliOutcome {
            code: 1,
            stdout: String::new(),
            stderr: format!("{}\n", serde_json::to_string_pretty(&diagnostic)?),
        });
    }
    bail!("{message}")
}

pub(crate) fn run_doctor(args: ProjectArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let project = service
        .load_project(
            args.config.as_deref(),
            args.benchmark.as_deref(),
            args.store_root.clone(),
        )
        .map_err(anyhow::Error::new)?;
    let store = service.store(args.store_root).map_err(anyhow::Error::new)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval": &project.name,
        "benchmark": &project.benchmark_id,
        "root": &project.benchmark_root,
        "eval_root": &store.root,
        "agents": project.agents.len(),
        "sets": project.task_sets.len(),
        "fake_adapter": "available",
        "command_adapter": "available",
        "acp_adapter": "wrapper",
        "psychevo_adapter": "wrapper",
        "opencode_adapter": "wrapper",
        "hermes_adapter": "wrapper",
        "views": ["html", "json"],
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!(
        "eval: {}\nbenchmark: {}\nroot: {}\neval root: {}\nagents: {}\ntask sets: {}\nfake adapter: available\npsychevo adapter: wrapper\nopencode adapter: wrapper\nhermes adapter: wrapper\n",
        project.name,
        project.benchmark_id,
        project.benchmark_root.display(),
        store.root.display(),
        project.agents.len(),
        project.task_sets.len(),
    )))
}

pub(crate) fn run_list(args: ListArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let needs_project = matches!(
        args.kind,
        ListKind::All | ListKind::TaskSets | ListKind::Tasks
    );
    let project = if needs_project {
        Some(
            service
                .load_project(
                    args.config.as_deref(),
                    args.benchmark.as_deref(),
                    args.store_root.clone(),
                )
                .map_err(anyhow::Error::new)?,
        )
    } else {
        service
            .try_load_project(
                args.config.as_deref(),
                args.benchmark.as_deref(),
                args.store_root.clone(),
            )
            .map_err(anyhow::Error::new)?
    };
    let needs_store = matches!(args.kind, ListKind::All | ListKind::Datasets);
    let store = if needs_store {
        Some(
            service
                .store(args.store_root.clone())
                .map_err(anyhow::Error::new)?,
        )
    } else {
        None
    };
    let tasks = project
        .as_ref()
        .map(list_tasks)
        .transpose()?
        .unwrap_or_default();
    let datasets = if needs_store {
        service
            .list_datasets(args.store_root.clone())
            .map_err(anyhow::Error::new)?
    } else {
        Vec::new()
    };
    let eval_root = store.as_ref().map(|store| store.root.clone());
    let registry_agents = if let Some(project) = project.as_ref() {
        project.agents.values().cloned().collect::<Vec<_>>()
    } else {
        list_registry_agents(args.store_root.clone())?
    };
    let registry_benchmarks = list_registry_benchmarks(args.store_root.clone())?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval_root": eval_root,
        "benchmarks": registry_benchmarks,
        "sets": project.as_ref().map(|project| project.task_sets.values().map(|task_set| json!({
            "id": &task_set.id,
            "name": &task_set.name,
            "tasks": &task_set.tasks,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "agents": registry_agents.iter().map(|agent| json!({
            "id": &agent.id,
            "name": &agent.name,
            "kind": agent.kind,
        })).collect::<Vec<_>>(),
        "tasks": tasks,
        "views": ["html", "json"],
        "datasets": datasets,
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    let mut out = String::new();
    if matches!(args.kind, ListKind::All | ListKind::TaskSets) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("task sets\n");
        for task_set in project.task_sets.values() {
            out.push_str(&format!("- {}\n", task_set.id));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Agents) {
        out.push_str("agents\n");
        for agent in &registry_agents {
            out.push_str(&format!("- {} ({:?})\n", agent.id, agent.kind));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Benchmarks) {
        out.push_str("benchmarks\n");
        for benchmark in list_registry_benchmarks(args.store_root.clone())? {
            out.push_str(&format!(
                "- {} {}\n",
                benchmark["id"].as_str().unwrap_or("unknown"),
                benchmark["path"].as_str().unwrap_or("")
            ));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Tasks) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("tasks\n");
        for task in list_tasks(project)? {
            out.push_str(&format!("- {}\n", task["id"].as_str().unwrap_or("unknown")));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Views) {
        out.push_str("views\n- html\n- json\n");
    }
    if matches!(args.kind, ListKind::All | ListKind::Datasets) {
        let _store = store.as_ref().context("list kind requires peval root")?;
        out.push_str("datasets\n");
        for dataset in &datasets {
            out.push_str(&format!(
                "- {} ({}) payload={} exists={}\n",
                dataset.id,
                dataset.kind,
                dataset.payload.display(),
                dataset.payload_exists
            ));
        }
    }
    Ok(success(out))
}

pub(crate) fn run_check(args: SelectArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    validate_direct_benchmark_selection(
        args.benchmark.as_deref(),
        args.agent.as_deref(),
        args.task_set.as_deref(),
        args.task.as_deref(),
    )?;
    let project = service
        .load_project(
            args.config.as_deref(),
            args.benchmark.as_deref(),
            args.store_root,
        )
        .map_err(anyhow::Error::new)?;
    let cases = service
        .check(
            &project,
            args.task_set.as_deref(),
            args.task.as_deref(),
            args.agent.as_deref(),
        )
        .map_err(anyhow::Error::new)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval": project.name,
        "benchmark": project.benchmark_id,
        "cases": cases.len(),
        "live": args.live,
        "status": "ok",
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!("check ok: {} case(s)\n", cases.len())))
}

pub(crate) fn run_run(args: RunArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let summary = service
        .run(RunRequest {
            config: args.config,
            benchmark: args.benchmark,
            task_set: args.task_set,
            task: args.task,
            agent: args.agent,
            overwrite: args.overwrite,
            store_root: args.store_root,
            output_root: args.output_root,
            include_artifacts: args.include,
        })
        .map_err(anyhow::Error::new)?;
    let code = if summary.status == RunStatus::Passed {
        0
    } else {
        1
    };
    if args.json {
        return Ok(CliOutcome {
            code,
            stdout: serde_json::to_string_pretty(&summary)?,
            stderr: String::new(),
        });
    }
    Ok(CliOutcome {
        code,
        stdout: format!(
            "run {:?}\nbenchmark: {}\ncells: {} selected / {} executed / {} reused / {} overwritten / {} retried\nresults: {} passed / {} failed\n",
            summary.status,
            summary.benchmark,
            summary.selected_cells,
            summary.executed_cells,
            summary.reused_cells,
            summary.overwritten_cells,
            summary.retried_cells,
            summary.passed_cells,
            summary.failed_cells,
        ),
        stderr: String::new(),
    })
}

pub(crate) fn run_task_env(args: TaskEnvCommands) -> Result<CliOutcome> {
    match args {
        TaskEnvCommands::Create(args) => run_task_env_create(args),
        TaskEnvCommands::Verify(args) => run_task_env_verify(args),
    }
}

pub(crate) fn run_task_env_create(args: TaskEnvCreateArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let result = service
        .create_task_env(TaskEnvCreateRequest {
            config: args.config,
            benchmark: args.benchmark,
            task_set: args.task_set,
            task: args.task,
            store_root: args.store_root,
        })
        .map_err(anyhow::Error::new)?;
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&result)?));
    }
    Ok(success(format!(
        "created task environment\nbenchmark: {}\ntask: {}\nenv: {}\nworkspace: {}\nprompt: {}\nverify: {}\n",
        result.benchmark,
        result.task_id,
        result.env_root.display(),
        result.workspace.display(),
        result.prompt.display(),
        shell_words(&result.verify_command),
    )))
}

pub(crate) fn run_task_env_verify(args: TaskEnvVerifyArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let result = service
        .verify_task_env(TaskEnvVerifyRequest {
            env_root: args.env_root,
            duration_seconds: args.duration_seconds,
        })
        .map_err(anyhow::Error::new)?;
    let code = if result.status.is_passed() { 0 } else { 1 };
    if args.json {
        return Ok(CliOutcome {
            code,
            stdout: serde_json::to_string_pretty(&result)?,
            stderr: String::new(),
        });
    }
    Ok(CliOutcome {
        code,
        stdout: format!(
            "verified task environment {:?}\nbenchmark: {}\ntask: {}\nscore: {}\nrun: {}\n",
            result.status,
            result.benchmark,
            result.task_id,
            result
                .score
                .map(|score| format!("{score:.2}"))
                .unwrap_or_else(|| "-".to_string()),
            result.run_json.display(),
        ),
        stderr: String::new(),
    })
}

fn shell_words(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| {
            if part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
            {
                part.clone()
            } else {
                format!("'{}'", part.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn run_view(args: ViewArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let output = args.output.clone();
    let format = effective_view_format(
        args.format,
        output.as_ref().and_then(|output| output.as_deref()),
        matches!(output, Some(None)),
    )?;
    let view = service
        .view(ViewRequest {
            config: args.config,
            benchmark: args.benchmark,
            report: args.report,
            store_root: args.store_root,
            paths: args.paths,
            task_set: args.task_set,
            agent: args.agent,
            task: args.task,
            status: args.status,
            group_by: parse_view_groups(&args.group_by)?,
            include: parse_view_includes(&args.include)?,
            notes: parse_view_notes(&args.notes)?,
        })
        .map_err(anyhow::Error::new)?;
    let rendered = render_view(&view, format)?;
    let output = match output {
        Some(Some(path)) => Some(path),
        Some(None) => Some(default_view_output_path(&view, format)?),
        None => None,
    };
    if let Some(output) = output {
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&output, rendered.as_bytes())
            .with_context(|| format!("failed to write {}", output.display()))?;
        Ok(success(format!("wrote {}\n", output.display())))
    } else {
        Ok(success(rendered))
    }
}

pub(crate) fn run_serve_command(args: ServeArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    run_serve_blocking(
        service,
        ServeOptions {
            config: args.config,
            benchmark: args.benchmark,
            report: args.report,
            store_root: args.store_root,
            path: args.path,
            task_set: args.task_set,
            agent: args.agent,
            task: args.task,
            status: args.status,
            host: args.host,
            port: args.port,
        },
    )?;
    Ok(success(String::new()))
}

pub(crate) fn run_dataset(args: DatasetCommands) -> Result<CliOutcome> {
    match args {
        DatasetCommands::Import(args) => {
            let service = process_service()?;
            let entry = service
                .dataset_import(DatasetImportRequest {
                    store_root: args.store_root,
                    path: args.path,
                    id: args.id,
                    name: args.name,
                    kind: args.kind,
                    loader: args.loader,
                    split: args.split,
                    sample_limit: args.sample_limit,
                    cache_key: args.cache_key,
                    license: args.license,
                    tags: args.tags,
                    notes: args.notes,
                })
                .map_err(anyhow::Error::new)?;
            if args.json {
                Ok(success(serde_json::to_string_pretty(&entry)?))
            } else {
                Ok(success(format!(
                    "dataset {}: {}\npayload: {}\npayload exists: {}\n",
                    entry.id,
                    entry.kind,
                    entry.payload.display(),
                    entry.payload_exists
                )))
            }
        }
    }
}
