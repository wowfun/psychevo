#[allow(unused_imports)]
use super::*;

pub(crate) fn run_acp_agent(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    let prompt = task_prompt(&case.task)?;
    let profile = acp_profile(&case.agent);
    let (command, configured_args) = acp_command_and_args(&case.agent)?;
    let env = resolve_acp_env(&case.agent, workspace)?;
    let prompt_dir = workspace.join(".peval");
    fs::create_dir_all(&prompt_dir)
        .with_context(|| format!("failed to create {}", prompt_dir.display()))?;
    let prompt_file = prompt_dir.join("prompt.md");
    fs::write(&prompt_file, prompt.as_bytes())
        .with_context(|| format!("failed to write {}", prompt_file.display()))?;
    let args = case
        .agent
        .acp
        .args
        .iter()
        .map(|arg| render_agent_template(arg, workspace, &case.task.dir, &prompt, &prompt_file))
        .collect::<Vec<_>>();
    let args = if case.agent.acp.args.is_empty() {
        configured_args
            .iter()
            .map(|arg| render_agent_template(arg, workspace, &case.task.dir, &prompt, &prompt_file))
            .collect::<Vec<_>>()
    } else {
        args
    };

    push_event(
        events,
        &case.case_id,
        "acp_agent_started",
        "ACP agent stdio session started",
        json!({
            "agent": case.agent.id,
            "task": case.task.id,
            "profile": profile,
        }),
    );

    let mut process = Command::new(resolve_command_part(&command, &case.task.dir));
    for arg in args {
        process.arg(resolve_command_part(&arg, &case.task.dir));
    }
    process
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", &case.task.dir)
        .env("PEVAL_LOGS", logs_dir)
        .env("PEVAL_TASK_ID", &case.task.id)
        .env("PEVAL_NATIVE_TASK_ID", &case.task.native_id)
        .env("PEVAL_SOURCE_ID", &case.task.source_id)
        .env("PEVAL_PROMPT_FILE", &prompt_file);
    for (key, value) in env {
        process.env(key, value);
    }
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().with_context(|| {
        format!(
            "failed to spawn ACP agent `{}` in {}",
            case.agent.id,
            workspace.display()
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

    let deadline = Instant::now() + Duration::from_secs(case.agent.acp.timeout_seconds);
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
            "cwd": workspace,
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
        "ACP agent prompt started",
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
        "ACP agent prompt finished",
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
            "ACP agent stderr line",
            json!({ "line": line }),
        );
    }
    push_event(
        events,
        &case.case_id,
        "acp_agent_finished",
        "ACP agent stdio session finished",
        json!({ "prompt_result": prompt_result }),
    );
    Ok(())
}

pub(crate) fn acp_profile(agent: &AgentManifest) -> &'static str {
    match agent.kind {
        AgentKind::PsychevoAcp => "psychevo",
        AgentKind::OpencodeAcp => "opencode",
        AgentKind::HermesAcp => "hermes",
        AgentKind::Acp => "custom",
        _ => "none",
    }
}

pub(crate) fn acp_command_and_args(agent: &AgentManifest) -> Result<(String, Vec<String>)> {
    let command = match agent.kind {
        AgentKind::Acp => agent.acp.command.clone().with_context(|| {
            format!("ACP agent `{}` must declare [agents.acp].command", agent.id)
        })?,
        AgentKind::PsychevoAcp => agent
            .acp
            .command
            .clone()
            .or_else(|| {
                agent
                    .acp
                    .binary
                    .as_ref()
                    .map(|path| path.display().to_string())
            })
            .unwrap_or_else(|| "pevo".to_string()),
        AgentKind::OpencodeAcp => agent
            .acp
            .command
            .clone()
            .unwrap_or_else(|| "opencode".to_string()),
        AgentKind::HermesAcp => agent
            .acp
            .command
            .clone()
            .unwrap_or_else(|| "hermes-acp".to_string()),
        _ => bail!("agent `{}` is not an ACP adapter", agent.id),
    };
    let args = match agent.kind {
        AgentKind::PsychevoAcp => vec!["acp".to_string()],
        AgentKind::OpencodeAcp => vec!["acp".to_string()],
        AgentKind::HermesAcp | AgentKind::Acp => Vec::new(),
        _ => Vec::new(),
    };
    Ok((command, args))
}

pub(crate) fn resolve_acp_env(
    agent: &AgentManifest,
    workspace: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for (key, value) in &agent.acp.env {
        env.insert(key.clone(), resolve_env_template(value)?);
    }

    if matches!(agent.kind, AgentKind::OpencodeAcp | AgentKind::HermesAcp) {
        let state_root = workspace
            .join(".peval")
            .join("agent-state")
            .join(sanitize_id(&agent.id));
        fs::create_dir_all(&state_root)
            .with_context(|| format!("failed to create {}", state_root.display()))?;
        match agent.kind {
            AgentKind::OpencodeAcp => {
                env.entry("HOME".to_string())
                    .or_insert_with(|| state_root.display().to_string());
                env.entry("XDG_CONFIG_HOME".to_string())
                    .or_insert_with(|| state_root.join("config").display().to_string());
                env.entry("XDG_CACHE_HOME".to_string())
                    .or_insert_with(|| state_root.join("cache").display().to_string());
                env.entry("OPENCODE_FAKE_VCS".to_string())
                    .or_insert_with(|| "git".to_string());
            }
            AgentKind::HermesAcp => {
                env.entry("HERMES_HOME".to_string())
                    .or_insert_with(|| state_root.display().to_string());
                env.entry("HOME".to_string())
                    .or_insert_with(|| state_root.display().to_string());
            }
            _ => {}
        }
    }

    if matches!(
        agent.kind,
        AgentKind::PsychevoAcp | AgentKind::OpencodeAcp | AgentKind::HermesAcp
    ) {
        infer_provider_env(agent, &mut env)?;
    }

    Ok(env)
}

pub(crate) fn resolve_env_template(value: &str) -> Result<String> {
    let Some(inner) = value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return Ok(value.to_string());
    };
    let (name, default) = if let Some((name, default)) = inner.split_once(":-") {
        (name, Some(default))
    } else {
        (inner, None)
    };
    if name.is_empty() {
        bail!("empty environment variable template `{value}`");
    }
    match env::var(name) {
        Ok(found) => Ok(found),
        Err(_) => default.map(str::to_string).with_context(|| {
            format!("Environment variable `{name}` not found in host environment")
        }),
    }
}

pub(crate) fn infer_provider_env(
    agent: &AgentManifest,
    env_map: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(model) = agent.acp.model.as_deref() else {
        return Ok(());
    };
    let Some((provider, _model)) = model.split_once('/') else {
        return Ok(());
    };
    let keys: &[&str] = match provider {
        "openai" => &["OPENAI_API_KEY", "OPENAI_BASE_URL"],
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        "google" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "github-copilot" => &["GITHUB_TOKEN"],
        "xai" => &["XAI_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "opencode" => &["OPENCODE_API_KEY"],
        _ => &[],
    };
    if keys.is_empty() {
        return Ok(());
    }
    let mut found_required = false;
    for key in keys {
        if env_map.contains_key(*key) {
            found_required = true;
            continue;
        }
        if let Ok(value) = env::var(key) {
            env_map.insert((*key).to_string(), value);
            found_required = true;
        }
    }
    if !found_required {
        bail!(
            "ACP agent `{}` model `{model}` implies provider `{provider}`, but none of these host env vars are set: {}",
            agent.id,
            keys.join(", ")
        );
    }
    Ok(())
}

pub(crate) fn validate_requested_acp_capabilities(
    agent: &AgentManifest,
    _initialize_result: &Value,
) -> Result<()> {
    match agent.acp.permission.as_deref().unwrap_or("allow_once") {
        "deny" | "allow_once" | "allow_all" => Ok(()),
        other => bail!(
            "ACP agent `{}` has unsupported permission policy `{other}`; expected deny, allow_once, or allow_all",
            agent.id
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn acp_send(
    stdin: &mut std::process::ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    std::io::Write::write_all(stdin, serde_json::to_string(&request)?.as_bytes())?;
    std::io::Write::write_all(stdin, b"\n")?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

pub(crate) fn acp_send_logged(
    stdin: &mut std::process::ChildStdin,
    raw_log: &mut dyn std::io::Write,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(
        raw_log,
        "{}",
        serde_json::to_string(&json!({ "direction": "client_to_agent", "message": request }))?
    )?;
    std::io::Write::write_all(stdin, serde_json::to_string(&request)?.as_bytes())?;
    std::io::Write::write_all(stdin, b"\n")?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

pub(crate) struct AcpResponseInput<'a> {
    pub(crate) stdin: &'a mut std::process::ChildStdin,
    pub(crate) raw_log: &'a mut dyn std::io::Write,
    pub(crate) stdout_rx: &'a std::sync::mpsc::Receiver<String>,
    pub(crate) deadline: Instant,
    pub(crate) events: &'a mut Vec<TrajectoryEvent>,
    pub(crate) case_id: &'a str,
    pub(crate) permission: &'a str,
}

pub(crate) fn acp_recv_response(input: AcpResponseInput<'_>, id: u64) -> Result<Value> {
    let AcpResponseInput {
        stdin,
        raw_log,
        stdout_rx,
        deadline,
        events,
        case_id,
        permission,
    } = input;
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("ACP agent timed out waiting for response id {id}");
        }
        let line = stdout_rx
            .recv_timeout(deadline.saturating_duration_since(now))
            .with_context(|| format!("ACP agent did not produce response id {id}"))?;
        if line.trim().is_empty() {
            continue;
        }
        writeln!(
            raw_log,
            "{}",
            serde_json::to_string(&json!({ "direction": "agent_to_client", "line": line }))?
        )?;
        let raw = match serde_json::from_str::<Value>(&line) {
            Ok(raw) => raw,
            Err(err) => {
                push_event(
                    events,
                    case_id,
                    "acp_stdout_line",
                    "ACP agent stdout line",
                    json!({ "line": line, "parse_error": err.to_string() }),
                );
                continue;
            }
        };
        if raw.get("id").and_then(Value::as_u64) == Some(id) && raw.get("method").is_none() {
            if let Some(error) = raw.get("error") {
                bail!("ACP agent returned error for id {id}: {error}");
            }
            return Ok(raw.get("result").cloned().unwrap_or(Value::Null));
        }
        if raw.get("method").and_then(Value::as_str) == Some("session/request_permission")
            && raw.get("id").is_some()
        {
            let option_id = match permission {
                "deny" => "deny",
                "allow_all" => "allow_all",
                _ => "allow_once",
            };
            acp_send_response(
                stdin,
                raw.get("id").cloned().unwrap_or(Value::Null),
                json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id,
                    }
                }),
            )?;
            push_event(
                events,
                case_id,
                if option_id == "deny" {
                    "acp_permission_denied"
                } else {
                    "acp_permission_allowed"
                },
                "ACP permission request handled",
                json!({ "raw_event": raw, "option_id": option_id }),
            );
            continue;
        }
        let kind = if raw.get("method").and_then(Value::as_str) == Some("session/update") {
            "acp_session_update"
        } else if raw.get("method").is_some() {
            "acp_notification"
        } else {
            "acp_unexpected_response"
        };
        push_event(
            events,
            case_id,
            kind,
            "ACP agent protocol message",
            json!({ "raw_event": raw }),
        );
    }
}

pub(crate) fn acp_send_response(
    stdin: &mut std::process::ChildStdin,
    id: Value,
    result: Value,
) -> Result<()> {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    std::io::Write::write_all(stdin, serde_json::to_string(&response)?.as_bytes())?;
    std::io::Write::write_all(stdin, b"\n")?;
    std::io::Write::flush(stdin)?;
    Ok(())
}
