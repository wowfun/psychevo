#[allow(unused_imports)]
use super::*;

pub(crate) fn install_container_acp_agent(
    case: &CasePlan,
    runtime: &ContainerRuntime,
    artifact_root: &Path,
) -> Result<()> {
    match case.agent.acp.install.strategy {
        AcpInstallStrategy::Preinstalled => return Ok(()),
        AcpInstallStrategy::InstallCommand => {
            let command = case.agent.acp.install.command.clone().with_context(|| {
                format!(
                    "agent `{}` install_command strategy requires install.command",
                    case.agent.id
                )
            })?;
            let outcome = docker_compose_exec_process(
                runtime,
                "/",
                &BTreeMap::new(),
                &command,
                &case.agent.acp.install.args,
            )?;
            let outcome = wait_for_command(outcome, Some(Duration::from_secs(600)), artifact_root)?;
            if !outcome.success {
                bail!("ACP agent install command failed: {}", outcome.stderr);
            }
            return Ok(());
        }
        AcpInstallStrategy::CopyBinary | AcpInstallStrategy::ProfileDefault => {}
    }
    if case.agent.kind == AgentKind::PsychevoAcp
        || case.agent.acp.install.strategy == AcpInstallStrategy::CopyBinary
    {
        let host_binary = resolve_acp_binary_for_container(&case.agent)?;
        let mut cp = docker_compose_command(runtime, &["cp"])?;
        cp.arg(&host_binary).arg("main:/usr/local/bin/pevo");
        let cp = wait_for_command(cp, Some(Duration::from_secs(120)), artifact_root)?;
        if !cp.success {
            bail!(
                "failed to copy ACP binary {} into container: {}",
                host_binary.display(),
                cp.stderr
            );
        }
        let chmod = docker_compose_exec_shell(
            runtime,
            "/",
            &BTreeMap::new(),
            "chmod +x /usr/local/bin/pevo",
        )?;
        let chmod = wait_for_command(chmod, Some(Duration::from_secs(30)), artifact_root)?;
        if !chmod.success {
            bail!("failed to chmod container ACP binary: {}", chmod.stderr);
        }
    }
    Ok(())
}

pub(crate) fn resolve_acp_binary_for_container(agent: &AgentManifest) -> Result<PathBuf> {
    if let Some(binary) = &agent.acp.binary {
        let path = if binary.is_absolute() {
            binary.clone()
        } else {
            resolve_relative(
                agent
                    .manifest_path
                    .parent()
                    .unwrap_or_else(|| Path::new(".")),
                binary,
            )
        };
        if path.is_file() {
            return Ok(path);
        }
        bail!("ACP binary does not exist: {}", path.display());
    }
    if let Some(path) = find_program_on_path("pevo") {
        return Ok(path);
    }
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("failed to resolve Psychevo workspace root")?;
    let mut build = Command::new("cargo");
    build
        .arg("build")
        .arg("-p")
        .arg("psychevo-cli")
        .arg("--bin")
        .arg("pevo")
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let outcome = wait_for_command(build, Some(Duration::from_secs(900)), workspace_root)?;
    if !outcome.success {
        bail!("failed to build pevo for container use: {}", outcome.stderr);
    }
    let binary = workspace_root.join("target").join("debug").join("pevo");
    if !binary.is_file() {
        bail!("built pevo binary not found at {}", binary.display());
    }
    Ok(binary)
}

pub(crate) fn find_program_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn resolve_container_acp_env(
    agent: &AgentManifest,
    artifact_root: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut env_map = BTreeMap::new();
    for (key, value) in &agent.acp.env {
        env_map.insert(key.clone(), resolve_env_template(value)?);
    }
    let state_root = format!("/peval/agent-state/{}", sanitize_id(&agent.id));
    fs::create_dir_all(
        artifact_root
            .join("agent-state")
            .join(sanitize_id(&agent.id)),
    )
    .with_context(|| {
        format!(
            "failed to create agent state under {}",
            artifact_root.display()
        )
    })?;
    match agent.kind {
        AgentKind::PsychevoAcp => {
            env_map
                .entry("PSYCHEVO_HOME".to_string())
                .or_insert_with(|| state_root.clone());
            env_map
                .entry("PSYCHEVO_DB".to_string())
                .or_insert_with(|| format!("{state_root}/psychevo.sqlite"));
            env_map
                .entry("PSYCHEVO_CONFIG".to_string())
                .or_insert_with(|| format!("{state_root}/config.toml"));
        }
        AgentKind::OpencodeAcp => {
            env_map
                .entry("HOME".to_string())
                .or_insert_with(|| state_root.clone());
            env_map
                .entry("XDG_CONFIG_HOME".to_string())
                .or_insert_with(|| format!("{state_root}/config"));
            env_map
                .entry("XDG_CACHE_HOME".to_string())
                .or_insert_with(|| format!("{state_root}/cache"));
            env_map
                .entry("OPENCODE_FAKE_VCS".to_string())
                .or_insert_with(|| "git".to_string());
        }
        AgentKind::HermesAcp => {
            env_map
                .entry("HERMES_HOME".to_string())
                .or_insert_with(|| state_root.clone());
            env_map
                .entry("HOME".to_string())
                .or_insert_with(|| state_root);
        }
        _ => {}
    }
    infer_provider_env(agent, &mut env_map)?;
    Ok(env_map)
}

pub(crate) fn run_acp_agent_in_container(
    case: &CasePlan,
    runtime: &ContainerRuntime,
    logs_dir: &Path,
    artifact_root: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    let prompt = task_prompt(&case.task)?;
    let profile = acp_profile(&case.agent);
    let (command, configured_args) = acp_command_and_args(&case.agent)?;
    let mut env_map = resolve_container_acp_env(&case.agent, artifact_root)?;
    env_map.insert("PEVAL_WORKSPACE".to_string(), runtime.cwd.clone());
    env_map.insert("PEVAL_TASK_DIR".to_string(), "/task".to_string());
    env_map.insert("PEVAL_LOGS".to_string(), "/logs".to_string());
    env_map.insert("PEVAL_TASK_ID".to_string(), case.task.id.clone());
    env_map.insert(
        "PEVAL_NATIVE_TASK_ID".to_string(),
        case.task.native_id.clone(),
    );
    env_map.insert("PEVAL_SOURCE_ID".to_string(), case.task.source_id.clone());
    env_map.insert(
        "PEVAL_PROMPT_FILE".to_string(),
        "/peval/prompt.md".to_string(),
    );
    let prompt_file = Path::new("/peval/prompt.md");
    let container_task_dir = Path::new("/task");
    let args = if case.agent.acp.args.is_empty() {
        configured_args
            .iter()
            .map(|arg| {
                render_agent_template(
                    arg,
                    Path::new(&runtime.cwd),
                    container_task_dir,
                    &prompt,
                    prompt_file,
                )
            })
            .collect::<Vec<_>>()
    } else {
        case.agent
            .acp
            .args
            .iter()
            .map(|arg| {
                render_agent_template(
                    arg,
                    Path::new(&runtime.cwd),
                    container_task_dir,
                    &prompt,
                    prompt_file,
                )
            })
            .collect::<Vec<_>>()
    };

    push_event(
        events,
        &case.case_id,
        "acp_agent_started",
        "container ACP agent stdio session started",
        json!({
            "agent": case.agent.id,
            "task": case.task.id,
            "profile": profile,
            "project": runtime.project_name,
        }),
    );
    let mut process =
        docker_compose_exec_process(runtime, &runtime.cwd, &env_map, &command, &args)?;
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().with_context(|| {
        format!(
            "failed to spawn container ACP agent `{}` in {}",
            case.agent.id, runtime.cwd
        )
    })?;
    let mut stdin = child.stdin.take().context("ACP agent stdin unavailable")?;
    let stdout = child
        .stdout
        .take()
        .context("ACP agent stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("ACP agent stderr unavailable")?;
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap_or_default();
            if stdout_tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut content = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut content);
        content
    });

    let timeout =
        harbor_task_agent_timeout_seconds(&case.task).unwrap_or(case.agent.acp.timeout_seconds);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let raw_log_path = logs_dir.join("agent").join("acp.raw.jsonl");
    let mut raw_log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&raw_log_path)
        .with_context(|| format!("failed to open {}", raw_log_path.display()))?;
    let mut next_id = 1_u64;
    acp_send_logged(
        &mut stdin,
        &mut raw_log,
        next_id,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "psychevo-eval",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
    )?;
    let initialize_result = acp_recv_response(
        AcpResponseInput {
            stdin: &mut stdin,
            raw_log: &mut raw_log,
            stdout_rx: &stdout_rx,
            deadline,
            events,
            case_id: &case.case_id,
            permission: case.agent.acp.permission.as_deref().unwrap_or("allow_once"),
        },
        next_id,
    )?;
    validate_requested_acp_capabilities(&case.agent, &initialize_result)?;
    next_id += 1;

    acp_send_logged(
        &mut stdin,
        &mut raw_log,
        next_id,
        "session/new",
        json!({
            "cwd": runtime.cwd,
            "mcpServers": [],
        }),
    )?;
    let session = acp_recv_response(
        AcpResponseInput {
            stdin: &mut stdin,
            raw_log: &mut raw_log,
            stdout_rx: &stdout_rx,
            deadline,
            events,
            case_id: &case.case_id,
            permission: case.agent.acp.permission.as_deref().unwrap_or("allow_once"),
        },
        next_id,
    )?;
    let session_id = session
        .get("sessionId")
        .and_then(Value::as_str)
        .context("ACP session/new response missing sessionId")?
        .to_string();
    next_id += 1;

    if let Some(mode) = &case.agent.acp.mode {
        acp_send_logged(
            &mut stdin,
            &mut raw_log,
            next_id,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": mode,
            }),
        )?;
        let _ = acp_recv_response(
            AcpResponseInput {
                stdin: &mut stdin,
                raw_log: &mut raw_log,
                stdout_rx: &stdout_rx,
                deadline,
                events,
                case_id: &case.case_id,
                permission: case.agent.acp.permission.as_deref().unwrap_or("allow_once"),
            },
            next_id,
        )?;
        next_id += 1;
    }
    if let Some(model) = &case.agent.acp.model {
        acp_send_logged(
            &mut stdin,
            &mut raw_log,
            next_id,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": model,
            }),
        )?;
        let _ = acp_recv_response(
            AcpResponseInput {
                stdin: &mut stdin,
                raw_log: &mut raw_log,
                stdout_rx: &stdout_rx,
                deadline,
                events,
                case_id: &case.case_id,
                permission: case.agent.acp.permission.as_deref().unwrap_or("allow_once"),
            },
            next_id,
        )?;
        next_id += 1;
    }

    push_event(
        events,
        &case.case_id,
        "acp_agent_prompt_started",
        "container ACP agent prompt started",
        json!({
            "session_id": session_id,
            "prompt_bytes": prompt.len(),
        }),
    );
    acp_send_logged(
        &mut stdin,
        &mut raw_log,
        next_id,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt,
                }
            ],
        }),
    )?;
    let prompt_result = acp_recv_response(
        AcpResponseInput {
            stdin: &mut stdin,
            raw_log: &mut raw_log,
            stdout_rx: &stdout_rx,
            deadline,
            events,
            case_id: &case.case_id,
            permission: case.agent.acp.permission.as_deref().unwrap_or("allow_once"),
        },
        next_id,
    )?;
    push_event(
        events,
        &case.case_id,
        "acp_agent_prompt_finished",
        "container ACP agent prompt finished",
        json!({ "prompt_result": prompt_result.clone() }),
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
        push_event(
            events,
            &case.case_id,
            "acp_stderr_line",
            "container ACP agent stderr line",
            json!({ "line": line }),
        );
    }
    push_event(
        events,
        &case.case_id,
        "acp_agent_finished",
        "container ACP agent stdio session finished",
        json!({ "prompt_result": prompt_result }),
    );
    Ok(())
}
