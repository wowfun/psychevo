#[allow(unused_imports)]
use super::*;

pub(crate) fn load_eval_config(path: &Path, store_root: Option<PathBuf>) -> Result<EvalProject> {
    let manifest_path = discover_manifest(path)?;
    let eval_root = manifest_path
        .parent()
        .context("eval config TOML has no parent directory")?
        .to_path_buf();
    let config = read_eval_config_manifest(&manifest_path)?;
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    let registry = ResolvedRegistry::load(
        Some((
            &config.agents,
            &config.benchmarks,
            &eval_root,
            &manifest_path,
        )),
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )?;
    let benchmark = resolve_benchmark_reference(&config.benchmark, &registry, &eval_root)?;
    let agents = resolve_selected_agents(&config.selection.agents, &registry)?;
    let (task_sets, tasks) = select_benchmark_tasks(&benchmark, &config.selection)?;
    Ok(EvalProject {
        eval_root: Some(eval_root),
        eval_manifest_path: Some(manifest_path),
        id: config.id,
        name: config.name,
        benchmark_root: benchmark.root,
        benchmark_manifest_path: benchmark.manifest_path,
        benchmark_id: benchmark.id,
        benchmark_name: benchmark.name,
        schema_version: config.schema_version,
        output_root: config.output_root,
        artifacts: config.artifacts,
        agents,
        task_sets,
        tasks,
        selection: config.selection,
    })
}

pub(crate) fn load_one_off_benchmark(
    benchmark_ref: &str,
    store_root: Option<PathBuf>,
) -> Result<EvalProject> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    let registry = ResolvedRegistry::load(
        None,
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )?;
    let reference = if Path::new(benchmark_ref).exists()
        || benchmark_ref.ends_with(".toml")
        || benchmark_ref.contains('/')
    {
        BenchmarkReference {
            id: None,
            path: Some(PathBuf::from(benchmark_ref)),
        }
    } else {
        BenchmarkReference {
            id: Some(benchmark_ref.to_string()),
            path: None,
        }
    };
    let benchmark = resolve_benchmark_reference(&reference, &registry, &cwd)?;
    Ok(EvalProject {
        eval_root: None,
        eval_manifest_path: None,
        id: format!("{}-one-off", benchmark.id),
        name: format!("{} one-off", benchmark.name),
        benchmark_root: benchmark.root,
        benchmark_manifest_path: benchmark.manifest_path,
        benchmark_id: benchmark.id,
        benchmark_name: benchmark.name,
        schema_version: MANIFEST_SCHEMA_VERSION,
        output_root: None,
        artifacts: ArtifactSelection::default(),
        agents: registry.agents,
        task_sets: benchmark.task_sets,
        tasks: benchmark.tasks,
        selection: EvalSelection::default(),
    })
}

pub(crate) fn resolve_optional_store(store_root: Option<PathBuf>) -> Result<Option<EvalStore>> {
    match store_root {
        Some(root) => EvalStore::resolve(Some(root)).map(Some),
        None => Ok(EvalStore::resolve(None).ok()),
    }
}

pub(crate) struct ResolvedRegistry {
    pub agents: BTreeMap<String, AgentManifest>,
    pub benchmarks: BTreeMap<String, RegistryBenchmark>,
}

impl ResolvedRegistry {
    pub(crate) fn load(
        eval_layer: Option<(&[AgentManifest], &[RegistryBenchmark], &Path, &Path)>,
        workspace_root: Option<&Path>,
        home: &Path,
    ) -> Result<Self> {
        let mut agents = BTreeMap::new();
        let mut benchmarks = BTreeMap::new();

        merge_benchmarks(&mut benchmarks, builtin_benchmarks(), Path::new("."))?;

        let global = read_global_peval_config(home)?;
        merge_agents(&mut agents, global.agents, &global_peval_config_path(home))?;
        merge_benchmarks(&mut benchmarks, global.benchmarks, home)?;

        if let Some(root) = workspace_root {
            let workspace = read_workspace_config(root)?;
            merge_agents(&mut agents, workspace.agents, &workspace_config_path(root))?;
            merge_benchmarks(&mut benchmarks, workspace.benchmarks, root)?;
        }

        if let Some((eval_agents, eval_benchmarks, eval_root, eval_path)) = eval_layer {
            merge_agents(&mut agents, eval_agents.to_vec(), eval_path)?;
            merge_benchmarks(&mut benchmarks, eval_benchmarks.to_vec(), eval_root)?;
        }

        Ok(Self { agents, benchmarks })
    }
}

pub(crate) fn builtin_benchmarks() -> Vec<RegistryBenchmark> {
    vec![RegistryBenchmark {
        id: "pidx-coding".to_string(),
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("benchmarks")
            .join("pidx-coding")
            .join("benchmark.toml"),
        name: Some("pidx-coding".to_string()),
    }]
}

pub(crate) fn merge_agents(
    target: &mut BTreeMap<String, AgentManifest>,
    agents: Vec<AgentManifest>,
    path: &Path,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for mut agent in agents {
        reject_unsupported(agent.schema_version, path)?;
        agent.id = slugify(&agent.id);
        if !seen.insert(agent.id.clone()) {
            bail!("duplicate agent id `{}` in {}", agent.id, path.display());
        }
        agent.manifest_path = path.to_path_buf();
        target.insert(agent.id.clone(), agent);
    }
    Ok(())
}

pub(crate) fn merge_benchmarks(
    target: &mut BTreeMap<String, RegistryBenchmark>,
    benchmarks: Vec<RegistryBenchmark>,
    base: &Path,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for mut benchmark in benchmarks {
        benchmark.id = slugify(&benchmark.id);
        if !seen.insert(benchmark.id.clone()) {
            bail!(
                "duplicate benchmark id `{}` in registry config rooted at {}",
                benchmark.id,
                base.display()
            );
        }
        benchmark.path = resolve_relative(base, &benchmark.path);
        target.insert(benchmark.id.clone(), benchmark);
    }
    Ok(())
}

pub(crate) fn resolve_selected_agents(
    selected: &[String],
    registry: &ResolvedRegistry,
) -> Result<BTreeMap<String, AgentManifest>> {
    let mut agents = BTreeMap::new();
    for id in selected {
        let id = slugify(id);
        let agent = registry
            .agents
            .get(&id)
            .with_context(|| format!("unknown agent `{id}`"))?
            .clone();
        agents.insert(id, agent);
    }
    Ok(agents)
}

pub(crate) fn resolve_benchmark_reference(
    reference: &BenchmarkReference,
    registry: &ResolvedRegistry,
    base: &Path,
) -> Result<BenchmarkManifest> {
    match (&reference.id, &reference.path) {
        (Some(_), Some(_)) => bail!("benchmark reference must use either id or path, not both"),
        (None, None) => bail!("benchmark reference must declare id or path"),
        (Some(id), None) => {
            let id = slugify(id);
            let entry = registry
                .benchmarks
                .get(&id)
                .with_context(|| format!("unknown benchmark `{id}`"))?;
            BenchmarkManifest::load(&entry.path)
        }
        (None, Some(path)) => BenchmarkManifest::load(resolve_relative(base, path)),
    }
}

pub(crate) fn select_benchmark_tasks(
    benchmark: &BenchmarkManifest,
    selection: &EvalSelection,
) -> Result<(
    BTreeMap<String, TaskSetManifest>,
    BTreeMap<String, TaskManifest>,
)> {
    let selected_tasks = selection.tasks.iter().cloned().collect::<BTreeSet<_>>();
    let mut task_sets = if selection.sets.is_empty() {
        benchmark.task_sets.clone()
    } else {
        let mut out = BTreeMap::new();
        for id in &selection.sets {
            let task_set = benchmark
                .task_sets
                .get(id)
                .with_context(|| format!("unknown task set `{id}`"))?
                .clone();
            out.insert(task_set.id.clone(), task_set);
        }
        out
    };
    if !selected_tasks.is_empty() {
        for task_set in task_sets.values_mut() {
            task_set
                .tasks
                .retain(|task_id| selected_tasks.contains(task_id));
        }
        task_sets.retain(|_, task_set| !task_set.tasks.is_empty());
    }

    let mut tasks = BTreeMap::new();
    for task_set in task_sets.values() {
        for task_id in &task_set.tasks {
            if !selected_tasks.is_empty() && !selected_tasks.contains(task_id) {
                continue;
            }
            let task = benchmark
                .tasks
                .get(task_id)
                .with_context(|| {
                    format!(
                        "task set `{}` references unknown task `{task_id}`",
                        task_set.id
                    )
                })?
                .clone();
            tasks.insert(task.id.clone(), task);
        }
    }
    for task_id in &selected_tasks {
        if !tasks.contains_key(task_id) {
            bail!("selected task `{task_id}` is not present in selected task sets");
        }
    }
    if task_sets.is_empty() {
        bail!("no task sets selected");
    }
    if tasks.is_empty() {
        bail!("no tasks selected");
    }
    Ok((task_sets, tasks))
}

pub(crate) fn load_task_set_tasks(
    project: &EvalProject,
    task_set: &TaskSetManifest,
    task_filter: Option<&str>,
) -> Result<Vec<TaskManifest>> {
    if task_set.tasks.is_empty() {
        bail!("task set `{}` does not declare any tasks", task_set.id);
    }
    let tasks = task_set
        .tasks
        .iter()
        .map(|id| {
            project.tasks.get(id).cloned().with_context(|| {
                format!("task set `{}` references unknown task `{id}`", task_set.id)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(task_id) = task_filter {
        let selected = tasks
            .into_iter()
            .filter(|task| task.id == task_id)
            .collect::<Vec<_>>();
        if selected.is_empty() {
            bail!(
                "task set `{}` does not include selected task `{task_id}`",
                task_set.id
            );
        }
        return Ok(selected);
    }
    Ok(tasks)
}

pub(crate) fn read_eval_config_manifest(path: &Path) -> Result<EvalConfigManifest> {
    let raw: RawEvalConfigManifest = read_toml(path)?;
    reject_unsupported(raw.schema_version, path)?;
    if let Some(output_root) = raw.output_root.as_ref() {
        validate_store_namespace(output_root)
            .with_context(|| format!("invalid output_root in {}", path.display()))?;
    }
    validate_eval_selection(&raw.select, path)?;
    Ok(EvalConfigManifest {
        schema_version: raw.schema_version,
        id: slugify(&raw.id),
        name: raw.name.unwrap_or_else(|| raw.id.clone()),
        output_root: raw.output_root,
        artifacts: raw.artifacts,
        analysis: raw.analysis,
        reports: raw.reports,
        benchmark: raw.benchmark,
        selection: raw.select,
        agents: raw.agents,
        benchmarks: raw.benchmarks,
    })
}

pub(crate) fn validate_eval_selection(selection: &EvalSelection, path: &Path) -> Result<()> {
    if selection.agents.is_empty() {
        bail!("{} select.agents must not be empty", path.display());
    }
    if selection.sets.is_empty() && selection.tasks.is_empty() {
        bail!(
            "{} select.sets or select.tasks must declare at least one item",
            path.display()
        );
    }
    Ok(())
}

type LoadedBenchmarkSources = (
    Vec<BenchmarkSourceSummary>,
    BTreeMap<String, TaskSetManifest>,
    BTreeMap<String, TaskManifest>,
);

pub(crate) fn load_benchmark_sources(
    root: &Path,
    manifest_path: &Path,
    sources: BenchmarkSources,
) -> Result<LoadedBenchmarkSources> {
    if sources.is_empty() {
        bail!("no sources declared in {}", manifest_path.display());
    }
    let mut seen_sources = BTreeSet::new();
    let mut summaries = Vec::new();
    let mut task_sets = BTreeMap::new();
    let mut tasks = BTreeMap::new();

    for source in sources.peval_agent {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::PevalAgent,
            execution: source.execution,
        });
        let source_root = resolve_relative(root, &source.path);
        let loaded = load_directory_source(
            &source_id,
            TaskSourceKind::PevalAgent,
            source.execution,
            &source_root,
            manifest_path,
            source.verifier_timeout_seconds,
        )
        .with_context(|| format!("failed to load peval_agent source `{source_id}`"))?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.harbor {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::Harbor,
            execution: source.execution,
        });
        let source_root = resolve_relative(&resolve_relative(root, &source.root), &source.path);
        let loaded = load_directory_source(
            &source_id,
            TaskSourceKind::Harbor,
            source.execution,
            &source_root,
            manifest_path,
            None,
        )
        .with_context(|| format!("failed to load harbor source `{source_id}`"))?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.swe_bench {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::SweBench,
            execution: source.execution,
        });
        let loaded = load_declared_official_source(
            &source_id,
            TaskSourceKind::SweBench,
            source.execution,
            manifest_path,
            resolve_relative(root, &source.root),
            format!("{}:{}", source.dataset, source.split),
        )?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    for source in sources.tau2 {
        let source_id = unique_source_id(&source.id, &mut seen_sources, manifest_path)?;
        summaries.push(BenchmarkSourceSummary {
            id: source_id.clone(),
            kind: TaskSourceKind::Tau2,
            execution: source.execution,
        });
        let mut native = source.domain.clone();
        if let Some(split) = source.split {
            native.push('-');
            native.push_str(&split);
        }
        if let Some(task_set) = source.task_set {
            native.push('-');
            native.push_str(&task_set);
        }
        let loaded = load_declared_official_source(
            &source_id,
            TaskSourceKind::Tau2,
            source.execution,
            manifest_path,
            resolve_relative(root, &source.root),
            native,
        )?;
        insert_loaded_source(
            &mut task_sets,
            &mut tasks,
            &source_id,
            manifest_path,
            loaded,
            source.sets,
        )?;
    }

    if task_sets.is_empty() {
        bail!(
            "sources did not declare any sets in {}",
            manifest_path.display()
        );
    }
    if tasks.is_empty() {
        bail!(
            "sources did not declare any tasks in {}",
            manifest_path.display()
        );
    }
    Ok((summaries, task_sets, tasks))
}

pub(crate) struct LoadedSource {
    native_ids: Vec<String>,
    tasks: BTreeMap<String, TaskManifest>,
}

pub(crate) fn unique_source_id(
    id: &str,
    seen_sources: &mut BTreeSet<String>,
    manifest_path: &Path,
) -> Result<String> {
    let source_id = slugify(id);
    if !seen_sources.insert(source_id.clone()) {
        bail!(
            "duplicate source id `{}` in {}",
            source_id,
            manifest_path.display()
        );
    }
    Ok(source_id)
}

pub(crate) fn load_directory_source(
    source_id: &str,
    source_kind: TaskSourceKind,
    execution: ExecutionBackend,
    source_root: &Path,
    manifest_path: &Path,
    default_timeout: Option<u64>,
) -> Result<LoadedSource> {
    if !source_root.is_dir() {
        bail!("source directory does not exist: {}", source_root.display());
    }
    let mut native_ids = Vec::new();
    let mut tasks = BTreeMap::new();
    for entry in fs::read_dir(source_root)
        .with_context(|| format!("failed to read {}", source_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let native_id = entry.file_name().to_string_lossy().to_string();
        let task_dir = entry.path();
        if source_kind == TaskSourceKind::PevalAgent {
            validate_task_directory_files(&task_dir)
                .with_context(|| format!("invalid task `{native_id}`"))?;
        }
        let task_toml = task_dir.join("task.toml");
        let task_value = if task_toml.is_file() {
            let raw = fs::read_to_string(&task_toml)
                .with_context(|| format!("failed to read {}", task_toml.display()))?;
            Some(
                toml::from_str::<toml::Value>(&raw)
                    .with_context(|| format!("failed to parse {}", task_toml.display()))?,
            )
        } else {
            None
        };
        let instruction = task_dir.join("instruction.md");
        let problem_statement = if instruction.is_file() {
            fs::read_to_string(&instruction)
                .with_context(|| format!("failed to read {}", instruction.display()))?
        } else {
            format!("Run official source task `{native_id}`.")
        };
        let canonical_id = canonical_task_id(source_id, &native_id);
        let task = TaskManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: canonical_id.clone(),
            name: task_value
                .as_ref()
                .and_then(|value| value.get("name"))
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            kind: task_value
                .as_ref()
                .and_then(|value| value.get("kind"))
                .and_then(toml::Value::as_str)
                .unwrap_or(match source_kind {
                    TaskSourceKind::PevalAgent => "coding",
                    TaskSourceKind::Harbor => "harbor",
                    TaskSourceKind::SweBench => "swe-bench",
                    TaskSourceKind::Tau2 => "tau2",
                })
                .to_string(),
            problem_statement,
            workspace: WorkspaceManifest {
                source: PathBuf::from("environment"),
            },
            test_spec: TestSpecManifest { checks: Vec::new() },
            source_kind,
            source_id: source_id.to_string(),
            native_id: native_id.clone(),
            execution,
            verifier_timeout_seconds: task_value
                .as_ref()
                .and_then(read_task_verifier_timeout)
                .or(default_timeout),
            manifest_path: manifest_path.to_path_buf(),
            dir: task_dir,
        };
        native_ids.push(native_id);
        if tasks.insert(canonical_id, task).is_some() {
            bail!("duplicate task id in source `{source_id}`");
        }
    }
    native_ids.sort();
    if native_ids.is_empty() {
        bail!("source `{source_id}` did not declare any task directories");
    }
    Ok(LoadedSource { native_ids, tasks })
}

pub(crate) fn load_declared_official_source(
    source_id: &str,
    source_kind: TaskSourceKind,
    execution: ExecutionBackend,
    manifest_path: &Path,
    root: PathBuf,
    native_id: String,
) -> Result<LoadedSource> {
    let native_id = slugify(&native_id);
    let canonical_id = canonical_task_id(source_id, &native_id);
    let task = TaskManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        id: canonical_id.clone(),
        name: None,
        kind: match source_kind {
            TaskSourceKind::PevalAgent => "coding",
            TaskSourceKind::Harbor => "harbor",
            TaskSourceKind::SweBench => "swe-bench",
            TaskSourceKind::Tau2 => "tau2",
        }
        .to_string(),
        problem_statement: format!("Run official {:?} task `{native_id}`.", source_kind),
        workspace: WorkspaceManifest {
            source: PathBuf::from("."),
        },
        test_spec: TestSpecManifest { checks: Vec::new() },
        source_kind,
        source_id: source_id.to_string(),
        native_id: native_id.clone(),
        execution,
        verifier_timeout_seconds: None,
        manifest_path: manifest_path.to_path_buf(),
        dir: root,
    };
    Ok(LoadedSource {
        native_ids: vec![native_id],
        tasks: BTreeMap::from([(canonical_id, task)]),
    })
}

pub(crate) fn insert_loaded_source(
    task_sets: &mut BTreeMap<String, TaskSetManifest>,
    tasks: &mut BTreeMap<String, TaskManifest>,
    source_id: &str,
    manifest_path: &Path,
    loaded: LoadedSource,
    sets: Vec<SourceSetManifest>,
) -> Result<()> {
    let native_ids = loaded.native_ids;
    for (task_id, task) in loaded.tasks {
        if tasks.insert(task_id.clone(), task).is_some() {
            bail!("duplicate canonical task id `{task_id}`");
        }
    }
    let all_tasks = native_ids
        .iter()
        .map(|native_id| canonical_task_id(source_id, native_id))
        .collect::<Vec<_>>();
    insert_task_set(
        task_sets,
        TaskSetManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: source_id.to_string(),
            name: Some(source_id.to_string()),
            description: None,
            tasks: all_tasks,
            manifest_path: manifest_path.to_path_buf(),
        },
    )?;
    for set in sets {
        let set_id = format!("{source_id}/{}", slugify(&set.id));
        let selected = filter_source_set(source_id, &native_ids, &set);
        if selected.is_empty() {
            bail!("source set `{set_id}` selected no tasks");
        }
        insert_task_set(
            task_sets,
            TaskSetManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                id: set_id,
                name: set.name,
                description: set.description,
                tasks: selected,
                manifest_path: manifest_path.to_path_buf(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn insert_task_set(
    task_sets: &mut BTreeMap<String, TaskSetManifest>,
    task_set: TaskSetManifest,
) -> Result<()> {
    if task_sets.insert(task_set.id.clone(), task_set).is_some() {
        bail!("duplicate set id");
    }
    Ok(())
}

pub(crate) fn filter_source_set(
    source_id: &str,
    native_ids: &[String],
    set: &SourceSetManifest,
) -> Vec<String> {
    let include_all = set.include.is_empty();
    let mut selected = native_ids
        .iter()
        .filter(|native_id| {
            (include_all
                || set
                    .include
                    .iter()
                    .any(|pattern| glob_match(pattern, native_id)))
                && !set
                    .exclude
                    .iter()
                    .any(|pattern| glob_match(pattern, native_id))
        })
        .map(|native_id| canonical_task_id(source_id, native_id))
        .collect::<Vec<_>>();
    selected.sort();
    if let Some(limit) = set.limit {
        selected.truncate(limit);
    }
    selected
}

pub(crate) fn canonical_task_id(source_id: &str, native_id: &str) -> String {
    format!("{}/{}", source_id, native_id)
}

pub(crate) fn validate_task_directory_files(task_dir: &Path) -> Result<()> {
    let task_toml = task_dir.join("task.toml");
    let instruction = task_dir.join("instruction.md");
    let environment = task_dir.join("environment");
    let verifier = task_dir.join("tests").join("test.sh");
    if !task_toml.is_file() {
        bail!("missing task.toml");
    }
    let raw = fs::read_to_string(&task_toml)
        .with_context(|| format!("failed to read {}", task_toml.display()))?;
    let _: toml::Value =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", task_toml.display()))?;
    if !instruction.is_file() {
        bail!("missing instruction.md");
    }
    if !environment.is_dir() {
        bail!("missing environment/");
    }
    if !verifier.is_file() {
        bail!("missing tests/test.sh");
    }
    Ok(())
}

pub(crate) fn read_task_verifier_timeout(value: &toml::Value) -> Option<u64> {
    value.get("verifier").and_then(|verifier| {
        toml_value_as_u64(verifier.get("timeout_seconds"))
            .or_else(|| toml_value_as_u64(verifier.get("timeout_sec")))
    })
}

pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                inner(rest, value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && inner(rest, &value[1..]),
            Some((&expected, rest)) => value
                .split_first()
                .is_some_and(|(&actual, tail)| actual == expected && inner(rest, tail)),
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}
