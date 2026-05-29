#[allow(unused_imports)]
use super::*;

pub(crate) fn validate_case(case: &CasePlan) -> Result<()> {
    reject_unsupported(case.task_set.schema_version, &case.task_set.manifest_path)?;
    reject_unsupported(case.agent.schema_version, &case.agent.manifest_path)?;
    reject_unsupported(case.task.schema_version, &case.task.manifest_path)?;
    if case.agent.kind.is_removed_wrapper() {
        bail_removed_agent_kind(case.agent.kind)?;
    }
    if case.agent.kind == AgentKind::HumanInLoop {
        bail!(
            "agent kind `human-in-loop` is reserved for `peval env verify` and cannot be configured in eval manifests"
        );
    }
    let execution = effective_execution_backend(&case.task);
    match execution {
        ExecutionBackend::Container => {
            if !case.agent.kind.is_acp_adapter() {
                bail!(
                    "incompatible_source_agent: task `{}` uses container execution and requires an ACP agent kind; got `{:?}`",
                    case.task.id,
                    case.agent.kind
                );
            }
            validate_acp_manifest(&case.agent)?;
            if !matches!(
                case.task.source_kind,
                TaskSourceKind::Harbor | TaskSourceKind::SweBench
            ) {
                bail!(
                    "task `{}` source kind `{:?}` does not support container execution",
                    case.task.id,
                    case.task.source_kind
                );
            }
            return Ok(());
        }
        ExecutionBackend::Local | ExecutionBackend::Auto => match case.agent.kind {
            AgentKind::Command => {
                validate_local_source_agent_compatibility(case)?;
                validate_command_agent(&case.agent, &case.task.dir)?
            }
            AgentKind::Acp
            | AgentKind::PsychevoAcp
            | AgentKind::OpencodeAcp
            | AgentKind::HermesAcp => {
                validate_local_source_agent_compatibility(case)?;
                validate_acp_agent(&case.agent, &case.task.dir)?
            }
            AgentKind::Fake => validate_local_source_agent_compatibility(case)?,
            AgentKind::HumanInLoop => bail!(
                "agent kind `human-in-loop` is reserved for `peval env verify` and cannot be configured in eval manifests"
            ),
            AgentKind::Psychevo | AgentKind::Opencode | AgentKind::Hermes => {
                bail_removed_agent_kind(case.agent.kind)?
            }
        },
    }
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    if !workspace_source.is_dir() {
        bail!(
            "task `{}` workspace source does not exist: {}",
            case.task.id,
            workspace_source.display()
        );
    }
    validate_peval_agent_task(&case.task)?;
    Ok(())
}

pub(crate) fn validate_local_source_agent_compatibility(case: &CasePlan) -> Result<()> {
    if matches!(
        case.task.source_kind,
        TaskSourceKind::SweBench | TaskSourceKind::Tau2
    ) {
        bail!(
            "incompatible_source_agent: task `{}` source kind `{:?}` requires an official bridge and is not executable by the local runner",
            case.task.id,
            case.task.source_kind
        );
    }
    Ok(())
}

pub(crate) fn effective_execution_backend(task: &TaskManifest) -> ExecutionBackend {
    match task.execution {
        ExecutionBackend::Local | ExecutionBackend::Container => task.execution,
        ExecutionBackend::Auto => match task.source_kind {
            TaskSourceKind::PevalAgent | TaskSourceKind::Tau2 => ExecutionBackend::Local,
            TaskSourceKind::Harbor | TaskSourceKind::SweBench => ExecutionBackend::Container,
        },
    }
}

pub(crate) fn bail_removed_agent_kind(kind: AgentKind) -> Result<()> {
    if let Some(hint) = kind.migration_hint() {
        bail!("agent kind `{:?}` was removed; {hint}", kind);
    }
    bail!("agent kind `{:?}` is not supported", kind)
}

pub(crate) fn validate_command_agent(agent: &AgentManifest, dir: &Path) -> Result<()> {
    let command = agent.command.command.clone().with_context(|| {
        format!(
            "command agent `{}` must declare [agents.command].command",
            agent.id
        )
    })?;
    let mut parts = vec![command];
    parts.extend(agent.command.args.clone());
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(agent.command.timeout_seconds),
        },
        dir,
        "command agent command",
    )
}

pub(crate) fn validate_acp_agent(agent: &AgentManifest, dir: &Path) -> Result<()> {
    validate_acp_manifest(agent)?;
    let (command, default_args) = acp_command_and_args(agent)?;
    let mut parts = vec![command];
    if agent.acp.args.is_empty() {
        parts.extend(default_args);
    } else {
        parts.extend(agent.acp.args.clone());
    }
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(agent.acp.timeout_seconds),
        },
        dir,
        "ACP agent command",
    )
}

pub(crate) fn validate_acp_manifest(agent: &AgentManifest) -> Result<()> {
    validate_requested_acp_capabilities(agent, &Value::Null)?;
    for value in agent.acp.env.values() {
        let _ = resolve_env_template(value)?;
    }
    let _ = acp_command_and_args(agent)?;
    Ok(())
}

pub(crate) fn validate_peval_agent_task(task: &TaskManifest) -> Result<()> {
    let task_toml = task.dir.join("task.toml");
    let instruction = task.dir.join("instruction.md");
    let environment = task.dir.join("environment");
    let verifier = task.dir.join("tests").join("test.sh");
    if !task_toml.is_file() {
        bail!("task `{}` missing task.toml", task.id);
    }
    let raw = fs::read_to_string(&task_toml)
        .with_context(|| format!("failed to read {}", task_toml.display()))?;
    let _: toml::Value =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", task_toml.display()))?;
    if !instruction.is_file() {
        bail!("task `{}` missing instruction.md", task.id);
    }
    if !environment.is_dir() {
        bail!("task `{}` missing environment/", task.id);
    }
    if !verifier.is_file() {
        bail!("task `{}` missing tests/test.sh", task.id);
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn validate_wrapper_command(
    adapter: &str,
    options: &WrapperAgentOptions,
    dir: &Path,
) -> Result<()> {
    if options.command.is_none() && options.args.is_empty() {
        return Ok(());
    }
    let command = options
        .command
        .clone()
        .unwrap_or_else(|| adapter.to_string());
    let mut parts = vec![command];
    parts.extend(options.args.clone());
    validate_command(
        &CommandManifest {
            command: parts,
            timeout_seconds: Some(600),
        },
        dir,
        &format!("{adapter} wrapper command"),
    )
}

pub(crate) fn validate_command(command: &CommandManifest, dir: &Path, label: &str) -> Result<()> {
    if command.command.is_empty() {
        bail!("{label} declaration is empty");
    }
    let program = &command.command[0];
    if is_declared_path(program, dir) {
        let path = resolve_relative(dir, Path::new(program));
        if !path.exists() {
            bail!("{label} path does not exist: {}", path.display());
        }
    }
    for arg in &command.command[1..] {
        if is_declared_path(arg, dir) {
            let path = resolve_relative(dir, Path::new(arg));
            if !path.exists() {
                bail!("{label} argument path does not exist: {}", path.display());
            }
        }
    }
    Ok(())
}

pub(crate) fn selected_task_sets(
    project: &EvalProject,
    task_set_filter: Option<&str>,
) -> Result<Vec<TaskSetManifest>> {
    if let Some(id) = task_set_filter {
        return Ok(vec![
            project
                .task_sets
                .get(id)
                .with_context(|| format!("unknown task set `{id}`"))?
                .clone(),
        ]);
    }
    Ok(project.task_sets.values().cloned().collect())
}

pub(crate) fn selected_agent_ids(
    project: &EvalProject,
    agent_filter: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(agent_id) = agent_filter {
        if !project.agents.contains_key(agent_id) {
            bail!("unknown agent `{agent_id}`");
        }
        return Ok(vec![agent_id.to_string()]);
    }
    Ok(project.agents.keys().cloned().collect())
}

pub(crate) fn validate_direct_benchmark_selection(
    benchmark: Option<&str>,
    agent: Option<&str>,
    task_set: Option<&str>,
    task: Option<&str>,
) -> Result<()> {
    if benchmark.is_some() {
        if agent.is_none() {
            bail!("--benchmark requires an explicit --agent");
        }
        if task_set.is_none() && task.is_none() {
            bail!("--benchmark requires an explicit --task-set or --task");
        }
    }
    Ok(())
}
