
pub(crate) fn agent_backend_diagnostics(backend: &AgentBackendConfig) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    if !backend.enabled {
        diagnostics.push(json!({"kind": "disabled", "message": "backend is disabled"}));
    }
    if backend.command.is_none() {
        diagnostics.push(json!({
            "kind": "missing_command",
            "message": "backend command is required for execution"
        }));
    }
    diagnostics
}

pub(crate) fn agent_backend_doctor_value(
    backend: &AgentBackendConfig,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Value {
    let mut checks = Vec::new();
    checks.push(json!({
        "name": "enabled",
        "ok": backend.enabled,
        "message": if backend.enabled { "backend enabled" } else { "backend disabled" },
    }));
    checks.push(json!({
        "name": "description",
        "ok": true,
        "message": if backend
            .description
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            "description configured"
        } else {
            "description optional; using backend label"
        },
    }));
    checks.push(match backend.command.as_deref() {
        Some(command) => match resolve_command_path(command, env_map) {
            Some(path) => json!({
                "name": "command",
                "ok": true,
                "message": "command resolved",
                "path": path,
            }),
            None => json!({
                "name": "command",
                "ok": false,
                "message": "command was not found on PATH or as a configured path",
            }),
        },
        None => json!({
            "name": "command",
            "ok": false,
            "message": "command missing",
        }),
    });
    let ok = checks
        .iter()
        .all(|check| check.get("ok").and_then(Value::as_bool).unwrap_or(false));
    json!({
        "id": backend.id,
        "kind": backend.kind.as_str(),
        "ok": ok,
        "checks": checks,
    })
}

pub(crate) fn resolve_command_path(
    command: &str,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Option<PathBuf> {
    let command_path = PathBuf::from(command);
    if command_path.components().count() > 1 {
        return command_path.is_file().then_some(command_path);
    }
    let path_var = env_map
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(command))
        .find(|path| path.is_file())
}

pub(crate) fn catalog() -> Result<AgentCatalog> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    catalog_for(&home, &cwd, env_map)
}

pub(crate) fn catalog_for(
    home: &std::path::Path,
    cwd: &std::path::Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<AgentCatalog> {
    discover_agents(&AgentDiscoveryOptions {
        home: home.to_path_buf(),
        cwd: cwd.to_path_buf(),
        env: env_map,
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .map_err(Into::into)
}

pub(crate) fn command_store() -> Result<SqliteStore> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    Ok(SqliteStore::open(&db_path)?)
}

pub(crate) fn print_agent_status(value: &Value) {
    let Some(agents) = value.get("agents").and_then(Value::as_array) else {
        println!("No agents found.");
        return;
    };
    if agents.is_empty() {
        println!("No agents found.");
        return;
    }
    for item in agents {
        print_agent_value(item);
    }
}

pub(crate) fn print_wait_report(value: &Value) {
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Wait completed.");
    println!("{message}");
    if value
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        eprintln!("timed out");
    }
}

pub(crate) fn print_agent_record(record: &psychevo_runtime::AgentRunRecord) {
    println!(
        "{}\t{}\t{:?}\t{}",
        record.id, record.agent_name, record.status, record.task
    );
}

pub(crate) fn print_agent_value(item: &Value) {
    println!(
        "{}\t{}\t{}\t{}",
        item.get("id").and_then(Value::as_str).unwrap_or_default(),
        item.get("agent_name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        item.get("status")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        item.get("task").and_then(Value::as_str).unwrap_or_default(),
    );
}

pub(crate) fn read_prompt(message: &[String]) -> Result<String> {
    let mut prompt = message.join(" ");
    if !io::stdin().is_terminal() {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        if !stdin.is_empty() {
            if prompt.is_empty() {
                prompt = stdin;
            } else {
                prompt.push('\n');
                prompt.push_str(&stdin);
            }
        }
    }
    Ok(prompt)
}
