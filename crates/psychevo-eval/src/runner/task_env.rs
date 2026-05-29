#[allow(unused_imports)]
use super::*;

pub(crate) const HUMAN_IN_LOOP_AGENT_ID: &str = "human-in-loop";

pub(crate) fn create_task_env(request: TaskEnvCreateRequest) -> Result<TaskEnvCreateResult> {
    validate_task_env_benchmark_selection(
        request.benchmark.as_deref(),
        request.task_set.as_deref(),
        request.task.as_deref(),
    )?;
    let project = load_project_from_selection(
        request.config.as_deref(),
        request.benchmark.as_deref(),
        request.store_root.clone(),
    )?;
    let (task_set, task) = select_single_human_task(
        &project,
        request.task_set.as_deref(),
        request.task.as_deref(),
    )?;
    ensure_local_human_task(&task)?;

    let store = EvalStore::resolve(request.store_root)?;
    let env_key = unique_task_env_key(&store, &project, &task)?;
    let env_root = task_env_root(&store, &project, &task, &env_key);
    let workspace = env_root.join("workspace");
    let prompt = env_root.join("prompt.md");
    let metadata = env_root.join("env.json");
    let readme = env_root.join("README.md");
    fs::create_dir_all(&env_root)
        .with_context(|| format!("failed to create {}", env_root.display()))?;

    let workspace_source = absolute_path(&resolve_relative(&task.dir, &task.workspace.source));
    copy_dir(&workspace_source, &workspace).with_context(|| {
        format!(
            "failed to copy task workspace {}",
            workspace_source.display()
        )
    })?;
    let prompt_text = task_prompt(&task)?;
    fs::write(&prompt, prompt_text.as_bytes())
        .with_context(|| format!("failed to write {}", prompt.display()))?;

    let case_id = sanitize_id(&format!(
        "{}__{}__{}",
        task_set.id, task.id, HUMAN_IN_LOOP_AGENT_ID
    ));
    let manifest = TaskEnvManifest {
        schema_version: TASK_ENV_SCHEMA_VERSION,
        benchmark: project.benchmark_id.clone(),
        benchmark_slug: project.slug(),
        project: project.name.clone(),
        env_key: env_key.clone(),
        case_id,
        task_set: task_set.clone(),
        task: task.clone(),
        task_set_manifest_path: absolute_path(&task_set.manifest_path),
        task_manifest_path: absolute_path(&task.manifest_path),
        task_dir: absolute_path(&task.dir),
        workspace_source,
        created_at_ms: now_ms(),
    };
    write_json_pretty(&metadata, &manifest)?;
    fs::write(&readme, task_env_readme(&manifest, &workspace, &prompt))
        .with_context(|| format!("failed to write {}", readme.display()))?;

    Ok(TaskEnvCreateResult {
        schema_version: TASK_ENV_SCHEMA_VERSION,
        benchmark: project.benchmark_id,
        task_set_id: task_set.id,
        task_id: task.id,
        env_key,
        env_root: env_root.clone(),
        workspace,
        prompt,
        metadata,
        readme,
        verify_command: vec![
            "peval".to_string(),
            "env".to_string(),
            "verify".to_string(),
            "--env".to_string(),
            env_root.display().to_string(),
            "--duration-seconds".to_string(),
            "<seconds>".to_string(),
        ],
    })
}

pub(crate) fn verify_task_env(request: TaskEnvVerifyRequest) -> Result<TaskEnvVerifyResult> {
    let env_root = absolute_path(&request.env_root);
    let manifest_path = env_root.join("env.json");
    let mut manifest = read_task_env_manifest(&manifest_path)?;
    manifest.task_set.manifest_path = manifest.task_set_manifest_path.clone();
    manifest.task.manifest_path = manifest.task_manifest_path.clone();
    manifest.task.dir = manifest.task_dir.clone();
    ensure_local_human_task(&manifest.task)?;
    let workspace = env_root.join("workspace");
    if !workspace.is_dir() {
        bail!(
            "human-in-loop workspace does not exist: {}",
            workspace.display()
        );
    }

    let logs_dir = env_root.join("logs");
    fs::create_dir_all(logs_dir.join("agent"))
        .with_context(|| format!("failed to create {}", logs_dir.join("agent").display()))?;
    fs::create_dir_all(logs_dir.join("verifier"))
        .with_context(|| format!("failed to create {}", logs_dir.join("verifier").display()))?;

    let case = task_env_case_plan(&manifest, &manifest_path);
    let mut events = Vec::new();
    push_event(
        &mut events,
        &case.case_id,
        "case_started",
        "human-in-loop verification started",
        json!({
            "task_set": case.task_set.id,
            "task": case.task.id,
            "agent": HUMAN_IN_LOOP_AGENT_ID,
            "env_key": manifest.env_key,
        }),
    );
    push_event(
        &mut events,
        &case.case_id,
        "workspace_prepared",
        "human-in-loop workspace selected",
        json!({
            "workspace": workspace,
            "workspace_source": manifest.workspace_source,
        }),
    );

    let started_at_ms = now_ms();
    let verifier_result = run_task_verifier(&case, &workspace, &logs_dir);
    let finished_at_ms = now_ms();
    let (status, score, evaluator_stdout, evaluator_stderr) = match verifier_result {
        Ok(result) => result,
        Err(err) => {
            let message = format!("{err:#}");
            (
                CaseStatus::EvaluatorFailed,
                ScoreResult {
                    schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
                    passed: false,
                    score: Some(0.0),
                    message: message.clone(),
                    details: Value::Null,
                },
                String::new(),
                message,
            )
        }
    };
    fs::write(
        env_root.join("evaluator.stdout"),
        evaluator_stdout.as_bytes(),
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            env_root.join("evaluator.stdout").display()
        )
    })?;
    fs::write(
        env_root.join("evaluator.stderr"),
        evaluator_stderr.as_bytes(),
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            env_root.join("evaluator.stderr").display()
        )
    })?;
    push_event(
        &mut events,
        &case.case_id,
        "evaluator_finished",
        &score.message,
        json!({
            "status": status,
            "passed": score.passed,
            "manual_duration_seconds": request.duration_seconds,
        }),
    );
    push_event(
        &mut events,
        &case.case_id,
        "case_finished",
        "human-in-loop verification finished",
        json!({ "status": status }),
    );

    let duration_ms = u128::from(request.duration_seconds).saturating_mul(1000);
    let observed = collect_case_observability(&events, duration_ms);
    let case_result = CaseResult {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        identity: CaseIdentity {
            case_id: case.case_id.clone(),
            task_set_id: case.task_set.id.clone(),
            task_id: case.task.id.clone(),
            task_family: case.task.kind.clone(),
        },
        candidate: CandidateIdentity {
            agent_id: HUMAN_IN_LOOP_AGENT_ID.to_string(),
            adapter: AgentKind::HumanInLoop,
            model: None,
        },
        factors: CaseFactors::default(),
        case_id: case.case_id,
        task_set_id: case.task_set.id,
        task_id: case.task.id,
        task_family: case.task.kind,
        agent_id: HUMAN_IN_LOOP_AGENT_ID.to_string(),
        status,
        failure_class: failure_class(status, &score),
        score,
        duration_ms,
        metrics: observed.metrics,
        warnings: observed.warnings,
        artifacts: CaseArtifacts {
            result: PathBuf::from("run.json"),
            trajectory: PathBuf::from("trajectory.jsonl"),
            evaluator_stdout: PathBuf::from("evaluator.stdout"),
            evaluator_stderr: PathBuf::from("evaluator.stderr"),
        },
    };
    write_jsonl(&env_root.join("trajectory.jsonl"), &events)?;
    let cell = CellRun {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        benchmark: manifest.benchmark.clone(),
        benchmark_slug: manifest.benchmark_slug.clone(),
        cell_key: manifest.env_key.clone(),
        fingerprint: manifest.env_key.clone(),
        cell_root: env_root.clone(),
        started_at_ms,
        finished_at_ms,
        case: case_result,
    };
    write_json_pretty(&env_root.join("run.json"), &cell)?;

    Ok(TaskEnvVerifyResult {
        schema_version: TASK_ENV_SCHEMA_VERSION,
        benchmark: manifest.benchmark,
        task_set_id: manifest.task_set.id,
        task_id: manifest.task.id,
        env_key: manifest.env_key,
        env_root: env_root.clone(),
        run_json: env_root.join("run.json"),
        status,
        passed: cell.case.score.passed,
        score: cell.case.score.score,
        message: cell.case.score.message,
        duration_ms,
    })
}

pub(crate) fn task_env_root(
    store: &EvalStore,
    project: &EvalProject,
    task: &TaskManifest,
    env_key: &str,
) -> PathBuf {
    store
        .cell_runs_root(project)
        .join(HUMAN_IN_LOOP_AGENT_ID)
        .join(sanitize_id(&task.id))
        .join(env_key)
}

fn validate_task_env_benchmark_selection(
    benchmark: Option<&str>,
    task_set: Option<&str>,
    task: Option<&str>,
) -> Result<()> {
    if benchmark.is_some() && task_set.is_none() && task.is_none() {
        bail!("--benchmark requires an explicit --task-set or --task for `peval env create`");
    }
    Ok(())
}

fn select_single_human_task(
    project: &EvalProject,
    task_set_filter: Option<&str>,
    task_filter: Option<&str>,
) -> Result<(TaskSetManifest, TaskManifest)> {
    let mut selected = Vec::new();
    for task_set in selected_task_sets(project, task_set_filter)? {
        let tasks = match load_task_set_tasks(project, &task_set, task_filter) {
            Ok(tasks) => tasks,
            Err(err) if task_filter.is_some() && task_set_filter.is_none() => {
                let message = format!("{err:#}");
                if message.contains("does not include selected task") {
                    continue;
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        selected.extend(tasks.into_iter().map(|task| (task_set.clone(), task)));
    }
    match selected.len() {
        0 => bail!("no task selected for `peval env create`"),
        1 => Ok(selected.remove(0)),
        count => bail!(
            "`peval env create` requires exactly one selected task; selected {count}. Pass --task-set and --task to narrow the selection."
        ),
    }
}

fn ensure_local_human_task(task: &TaskManifest) -> Result<()> {
    let execution = effective_execution_backend(task);
    if execution != ExecutionBackend::Local
        || !matches!(
            task.source_kind,
            TaskSourceKind::PevalAgent | TaskSourceKind::Harbor
        )
    {
        bail!(
            "`peval env` supports only local-directory task environments in this version; task `{}` uses source kind `{:?}` with execution `{:?}`",
            task.id,
            task.source_kind,
            execution,
        );
    }
    let workspace_source = resolve_relative(&task.dir, &task.workspace.source);
    if !workspace_source.is_dir() {
        bail!(
            "task `{}` workspace source does not exist: {}",
            task.id,
            workspace_source.display()
        );
    }
    Ok(())
}

fn unique_task_env_key(
    store: &EvalStore,
    project: &EvalProject,
    task: &TaskManifest,
) -> Result<String> {
    for _ in 0..10 {
        let raw = Uuid::now_v7().simple().to_string();
        let env_key = raw.chars().take(16).collect::<String>();
        if !task_env_root(store, project, task, &env_key).exists() {
            return Ok(env_key);
        }
    }
    bail!("failed to allocate a unique human-in-loop env key")
}

fn read_task_env_manifest(path: &Path) -> Result<TaskEnvManifest> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read human-in-loop env metadata {}",
            path.display()
        )
    })?;
    let manifest: TaskEnvManifest = serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse human-in-loop env metadata {}",
            path.display()
        )
    })?;
    if manifest.schema_version != TASK_ENV_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported human env schema_version {}; supported schema_version is {}",
            path.display(),
            manifest.schema_version,
            TASK_ENV_SCHEMA_VERSION
        );
    }
    Ok(manifest)
}

fn task_env_case_plan(manifest: &TaskEnvManifest, manifest_path: &Path) -> CasePlan {
    let mut task_set = manifest.task_set.clone();
    task_set.manifest_path = manifest.task_set_manifest_path.clone();
    let mut task = manifest.task.clone();
    task.manifest_path = manifest.task_manifest_path.clone();
    task.dir = manifest.task_dir.clone();
    CasePlan {
        case_id: manifest.case_id.clone(),
        task_set,
        task,
        agent: AgentManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: HUMAN_IN_LOOP_AGENT_ID.to_string(),
            name: Some("Human in loop".to_string()),
            kind: AgentKind::HumanInLoop,
            fake: FakeAgentOptions::default(),
            command: CommandAgentOptions::default(),
            acp: AcpAgentOptions::default(),
            psychevo: PsychevoAgentOptions::default(),
            opencode: WrapperAgentOptions::default(),
            hermes: WrapperAgentOptions::default(),
            manifest_path: manifest_path.to_path_buf(),
        },
    }
}

fn task_env_readme(manifest: &TaskEnvManifest, workspace: &Path, prompt: &Path) -> String {
    format!(
        "# peval human-in-loop environment\n\nTask: `{}`\nTask set: `{}`\n\nPrompt: `{}`\nWorkspace: `{}`\n\nEdit files under `workspace/`, then run:\n\n```bash\npeval env verify --env {} --duration-seconds <seconds>\n```\n",
        manifest.task.id,
        manifest.task_set.id,
        prompt.display(),
        workspace.display(),
        workspace.parent().unwrap_or(workspace).display(),
    )
}
