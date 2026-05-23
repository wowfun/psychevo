use std::env;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Result, anyhow};
use psychevo_ai::Outcome;
use psychevo_runtime::{
    AgentCatalog, AgentDiscoveryOptions, AgentEdgeRecord, RunMode, RunOptions, SessionSummary,
    SqliteStore, TuiMessageSummary, agent_status_value, close_agent_id, discover_agents,
    list_agents_value, resolve_agent_definition, resume_agent_id, send_agent_message,
    view_agent_value_with_catalog, wait_agent_mailbox,
};
use serde_json::{Value, json};

use crate::args::{
    AgentArgs, AgentCommand, AgentIdArgs, AgentInspectArgs, AgentListArgs, AgentLogsArgs,
    AgentNameArgs, AgentRunArgs, AgentSendArgs, AgentStatusArgs, AgentWaitArgs, RunFormatArg,
};
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) async fn run_agent_command(args: AgentArgs) -> Result<ExitCode> {
    match args.command {
        AgentCommand::List(args) => list_agents(args),
        AgentCommand::View(args) => view_agent(args),
        AgentCommand::Validate(args) => validate_agent(args),
        AgentCommand::Run(args) => run_agent(args).await,
        AgentCommand::Status(args) => agent_status(args),
        AgentCommand::Inspect(args) => inspect_agent(args),
        AgentCommand::Wait(args) => wait_agent(args).await,
        AgentCommand::Close(args) => close_agent(args),
        AgentCommand::Resume(args) => resume_agent(args),
        AgentCommand::Send(args) => send_agent(args),
        AgentCommand::Attach(args) => attach_agent(args).await,
        AgentCommand::Logs(args) => agent_logs(args),
    }
}

fn list_agents(args: AgentListArgs) -> Result<ExitCode> {
    let catalog = catalog()?;
    if args.json {
        println!("{}", serde_json::to_string(&list_agents_value(&catalog))?);
    } else if catalog.agents.is_empty() {
        println!("No agents found.");
    } else {
        for agent in &catalog.agents {
            let path = agent
                .file_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| format!("<{}>", agent.source.as_str()));
            println!("{}\t{}\t{}", agent.name, agent.description, path);
        }
        if !catalog.diagnostics.is_empty() {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({"diagnostics": catalog.diagnostics}))?
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn view_agent(args: AgentNameArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);
    let catalog = catalog_for(&home, &workdir, env_map.clone())?;
    let agent = resolve_agent_definition(&catalog, &args.name, &workdir, &env_map)?;
    let value = view_agent_value_with_catalog(&agent, Some(&catalog));
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(ExitCode::SUCCESS)
}

fn validate_agent(args: AgentNameArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);
    let catalog = catalog_for(&home, &workdir, env_map.clone())?;
    let agent = resolve_agent_definition(&catalog, &args.name, &workdir, &env_map)?;
    let value = json!({
        "valid": true,
        "agent": view_agent_value_with_catalog(&agent, Some(&catalog)),
    });
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!("valid: {}", agent.name);
        for diagnostic in &agent.diagnostics {
            eprintln!("{}: {}", diagnostic.kind, diagnostic.message);
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn run_agent(args: AgentRunArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let workdir = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let catalog = catalog_for(&home, &workdir, env_map.clone())?;
    let selected = resolve_agent_definition(&catalog, &args.name, &workdir, &env_map)?;
    let mut prompt = read_prompt(&args.message)?;
    if prompt.trim().is_empty()
        && let Some(initial) = selected.initial_prompt.clone()
    {
        prompt = initial;
    }
    if prompt.trim().is_empty() {
        return Err(anyhow!("You must provide a message"));
    }

    let result = psychevo_runtime::run_live(RunOptions {
        db_path,
        workdir,
        snapshot_root: Some(home.join("snapshots")),
        session: None,
        continue_latest: false,
        prompt,
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path,
        model: args.model.clone(),
        reasoning_effort: args.variant.map(|variant| variant.as_str().to_string()),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(env_map),
        agent: Some(args.name),
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
    })
    .await?;

    if args.format == RunFormatArg::Json {
        for event in &result.events {
            println!("{}", serde_json::to_string(event)?);
        }
    } else {
        for warning in &result.warnings {
            eprintln!("warning: {}", warning.message);
            if let Some(suggestion) = &warning.suggestion {
                eprintln!("suggestion: {suggestion}");
            }
        }
        println!("{}", result.final_answer);
    }

    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn agent_status(args: AgentStatusArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let store = SqliteStore::open(&db_path)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);
    let parent = if args.all {
        None
    } else {
        store.latest_session_for_workdir_with_sources(&workdir, &["run", "tui"])?
    };
    let value = agent_status_value(Some(&store), parent.as_deref(), args.all);
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        print_agent_status(&value);
    }
    Ok(ExitCode::SUCCESS)
}

fn inspect_agent(args: AgentInspectArgs) -> Result<ExitCode> {
    let store = command_store()?;
    let edge = store
        .find_agent_edge(&args.id)?
        .ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
    let mut record = agent_status_record_value(&store, &args.id, &edge)?;
    let parent_session = store.session_summary(&edge.parent_session_id)?;
    let child_session = store.session_summary(&edge.child_session_id)?;
    let mut messages = store.load_tui_message_summaries(&edge.child_session_id)?;
    let latest_usage = latest_usage_from_summaries(&messages);
    let latest_total_tokens = latest_usage.as_ref().and_then(usage_total_tokens);
    if let Some(object) = record.as_object_mut() {
        if let Some(usage) = latest_usage.clone() {
            object.insert("latest_usage".to_string(), usage);
        }
        if let Some(tokens) = latest_total_tokens {
            object.insert("latest_total_tokens".to_string(), Value::from(tokens));
        }
    }
    let keep_from = messages.len().saturating_sub(args.limit);
    messages.drain(..keep_from);

    if args.json {
        let messages = messages
            .iter()
            .map(tui_message_summary_value)
            .collect::<Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "agent": record,
                "edge": agent_edge_value(&edge),
                "parent_session": parent_session.as_ref().map(session_summary_value),
                "child_session": child_session.as_ref().map(session_summary_value),
                "messages": messages,
            }))?
        );
    } else {
        print_agent_inspect(
            &record,
            &edge,
            parent_session.as_ref(),
            child_session.as_ref(),
            &messages,
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn wait_agent(args: AgentWaitArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let store = SqliteStore::open(&db_path)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);
    let session_id = store
        .latest_run_session_for_workdir(&workdir)?
        .ok_or_else(|| anyhow!("no run session found for {}", workdir.display()))?;
    let value =
        wait_agent_mailbox(&session_id, Duration::from_millis(args.timeout_ms), &store).await?;
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        print_wait_report(&value);
    }
    Ok(ExitCode::SUCCESS)
}

fn close_agent(args: AgentIdArgs) -> Result<ExitCode> {
    let store = command_store()?;
    let record = close_agent_id(&args.id, Some(&store))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&json!({ "previous_status": record }))?
        );
    } else if let Some(record) = record {
        print_agent_record(&record);
    } else {
        return Err(anyhow!("agent not found: {}", args.id));
    }
    Ok(ExitCode::SUCCESS)
}

fn send_agent(args: AgentSendArgs) -> Result<ExitCode> {
    let store = command_store()?;
    let record = send_agent_message(&args.id, &args.message.join(" "), Some(&store))?;
    if args.json {
        println!("{}", serde_json::to_string(&json!({ "agent": record }))?);
    } else if let Some(record) = record {
        print_agent_record(&record);
    } else {
        return Err(anyhow!("agent not found: {}", args.id));
    }
    Ok(ExitCode::SUCCESS)
}

fn resume_agent(args: AgentIdArgs) -> Result<ExitCode> {
    let store = command_store()?;
    let record = resume_agent_id(&args.id, Some(&store))?;
    if args.json {
        println!("{}", serde_json::to_string(&json!({ "agent": record }))?);
    } else if let Some(record) = record {
        print_agent_record(&record);
    } else {
        return Err(anyhow!("agent not found: {}", args.id));
    }
    Ok(ExitCode::SUCCESS)
}

async fn attach_agent(args: AgentIdArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let store = SqliteStore::open(&db_path)?;
    let edge = store
        .find_agent_edge(&args.id)?
        .ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&json!({ "session": edge.child_session_id }))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("tui")
        .arg("--session")
        .arg(edge.child_session_id)
        .status()?;
    Ok(status
        .code()
        .map(|code| ExitCode::from(code as u8))
        .unwrap_or(ExitCode::FAILURE))
}

fn agent_logs(args: AgentLogsArgs) -> Result<ExitCode> {
    let store = command_store()?;
    let edge = store
        .find_agent_edge(&args.id)?
        .ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
    let mut messages = store.load_tui_message_summaries(&edge.child_session_id)?;
    let keep_from = messages.len().saturating_sub(args.limit);
    messages.drain(..keep_from);
    if args.json {
        let values = messages
            .iter()
            .map(|summary| {
                json!({
                    "message": summary.message,
                    "usage": summary.usage,
                    "metadata": summary.metadata,
                    "accounting": summary.accounting,
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string(&json!({ "messages": values }))?);
    } else {
        for summary in messages {
            println!("{}", serde_json::to_string(&summary.message)?);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_status_record_value(
    store: &SqliteStore,
    target: &str,
    edge: &AgentEdgeRecord,
) -> Result<Value> {
    let value = agent_status_value(Some(store), None, true);
    let agents = value
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("agent status projection is missing agents"))?;
    agents
        .iter()
        .find(|item| agent_value_matches_target(item, target, edge))
        .cloned()
        .ok_or_else(|| anyhow!("agent not found: {target}"))
}

fn agent_value_matches_target(item: &Value, target: &str, edge: &AgentEdgeRecord) -> bool {
    item.get("id").and_then(Value::as_str) == Some(target)
        || item.get("task_name").and_then(Value::as_str) == Some(target)
        || item.get("child_session_id").and_then(Value::as_str) == Some(target)
        || item.get("child_session_id").and_then(Value::as_str)
            == Some(edge.child_session_id.as_str())
}

fn agent_edge_value(edge: &AgentEdgeRecord) -> Value {
    json!({
        "parent_session_id": edge.parent_session_id,
        "child_session_id": edge.child_session_id,
        "status": edge.status.as_str(),
        "created_at_ms": edge.created_at_ms,
        "updated_at_ms": edge.updated_at_ms,
        "metadata": edge.metadata,
    })
}

fn session_summary_value(summary: &SessionSummary) -> Value {
    json!({
        "id": summary.id,
        "source": summary.source,
        "workdir": summary.workdir,
        "model": summary.model,
        "provider": summary.provider,
        "started_at_ms": summary.started_at_ms,
        "updated_at_ms": summary.updated_at_ms,
        "ended_at_ms": summary.ended_at_ms,
        "end_reason": summary.end_reason,
        "archived_at_ms": summary.archived_at_ms,
        "message_count": summary.message_count,
        "tool_call_count": summary.tool_call_count,
        "title": summary.title,
    })
}

fn tui_message_summary_value(summary: &TuiMessageSummary) -> Result<Value> {
    Ok(json!({
        "message": serde_json::to_value(&summary.message)?,
        "usage": summary.usage,
        "metadata": summary.metadata,
        "accounting": summary.accounting,
    }))
}

fn print_agent_inspect(
    record: &Value,
    edge: &AgentEdgeRecord,
    parent_session: Option<&SessionSummary>,
    child_session: Option<&SessionSummary>,
    messages: &[TuiMessageSummary],
) -> Result<()> {
    let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
    let agent_name = record
        .get("agent_name")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    let status = record
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("agent: {id}\t{agent_name}\t{status}");
    if let Some(task_name) = record.get("task_name").and_then(Value::as_str) {
        println!("task name: {task_name}");
    }
    if let Some(depth) = record
        .get("effective_max_spawn_depth")
        .and_then(Value::as_u64)
    {
        println!("max spawn depth: {depth}");
    }
    if let Some(tokens) = record.get("latest_total_tokens").and_then(Value::as_u64) {
        println!("latest tokens: {tokens}");
    }
    if let Some(task) = record.get("task").and_then(Value::as_str)
        && !task.trim().is_empty()
    {
        println!("task: {}", truncate_preview(task, 180));
    }
    println!("edge: {}", edge.status.as_str());
    println!(
        "parent session: {}",
        session_summary_label(parent_session, &edge.parent_session_id)
    );
    println!(
        "child session: {}",
        session_summary_label(child_session, &edge.child_session_id)
    );
    println!("logs: pevo agent logs {id}");
    println!("attach: pevo agent attach {id}");
    println!("transcript:");
    if messages.is_empty() {
        println!("  (empty)");
        return Ok(());
    }
    for summary in messages {
        let message = serde_json::to_value(&summary.message)?;
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("message");
        println!("  {role}: {}", message_preview(&message));
    }
    Ok(())
}

fn session_summary_label(summary: Option<&SessionSummary>, fallback_id: &str) -> String {
    let Some(summary) = summary else {
        return fallback_id.to_string();
    };
    let mut parts = vec![
        summary.id.clone(),
        summary.source.clone(),
        format!("{}/{}", summary.provider, summary.model),
        format!("messages={}", summary.message_count),
    ];
    if let Some(reason) = &summary.end_reason {
        parts.push(format!("ended={reason}"));
    }
    if summary.archived_at_ms.is_some() {
        parts.push("archived".to_string());
    }
    parts.join(" ")
}

fn message_preview(message: &Value) -> String {
    match message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "user" => truncate_preview(&message_content_text(message), 180),
        "assistant" => {
            let text = message_content_text(message);
            if !text.trim().is_empty() {
                truncate_preview(&text, 180)
            } else {
                let calls = assistant_tool_call_names(message);
                if calls.is_empty() {
                    "(no visible text)".to_string()
                } else {
                    format!("tool calls: {}", calls.join(", "))
                }
            }
        }
        "tool_result" => {
            let tool = message
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            format!("{tool}: {}", truncate_preview(content, 160))
        }
        _ => truncate_preview(&serde_json::to_string(message).unwrap_or_default(), 180),
    }
}

fn message_content_text(message: &Value) -> String {
    message
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter_map(|block| {
                    let block_type = block.get("type").and_then(Value::as_str);
                    if block_type.is_none() || block_type == Some("text") {
                        block.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn assistant_tool_call_names(message: &Value) -> Vec<String> {
    message
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_call"))
                .filter_map(|block| block.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn latest_usage_from_summaries(messages: &[TuiMessageSummary]) -> Option<Value> {
    messages
        .iter()
        .rev()
        .find_map(|summary| summary.usage.clone())
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            let mut total = 0u64;
            let mut any = false;
            for key in [
                "input_tokens",
                "output_tokens",
                "reasoning_tokens",
                "cached_tokens",
                "cache_write_tokens",
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    total = total.saturating_add(value);
                    any = true;
                }
            }
            any.then_some(total)
        })
}

fn truncate_preview(input: &str, max_chars: usize) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = normalized
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn catalog() -> Result<AgentCatalog> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let workdir = cwd.canonicalize().unwrap_or(cwd);
    catalog_for(&home, &workdir, env_map)
}

fn catalog_for(
    home: &std::path::Path,
    workdir: &std::path::Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<AgentCatalog> {
    discover_agents(&AgentDiscoveryOptions {
        home: home.to_path_buf(),
        workdir: workdir.to_path_buf(),
        env: env_map,
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .map_err(Into::into)
}

fn command_store() -> Result<SqliteStore> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    Ok(SqliteStore::open(&db_path)?)
}

fn print_agent_status(value: &Value) {
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

fn print_wait_report(value: &Value) {
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

fn print_agent_record(record: &psychevo_runtime::AgentRunRecord) {
    println!(
        "{}\t{}\t{:?}\t{}",
        record.id, record.agent_name, record.status, record.task
    );
}

fn print_agent_value(item: &Value) {
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

fn read_prompt(message: &[String]) -> Result<String> {
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
