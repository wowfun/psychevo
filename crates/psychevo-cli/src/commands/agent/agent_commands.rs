use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Result, anyhow};
use psychevo::{
    AgentMailboxWaitOutcome, AgentRelationship, AgentRelationshipStatus, Application,
    Client as FrameworkClient, HistoryReader, StartThreadRequest, ThreadItem, ThreadListQuery,
    ThreadSummary, TurnOutcome, TurnRequest, accounting::effective_usage_total,
    agents::AgentBackendConfig, agents::AgentControl, agents::AgentRunRecord,
    agents::list_agents_value, agents::resolve_agent_definition, agents::valid_agent_name,
    agents::view_agent_value_with_catalog, config::load_agent_backend_configs,
    config::set_config_value,
};
use serde_json::{Value, json};

use crate::args::{
    AgentArgs, AgentBackendAddArgs, AgentBackendArgs, AgentBackendCommand, AgentBackendDoctorArgs,
    AgentBackendListArgs, AgentCommand, AgentIdArgs, AgentInspectArgs, AgentListArgs,
    AgentLogsArgs, AgentNameArgs, AgentRunArgs, AgentSendArgs, AgentStatusArgs, AgentWaitArgs,
    RunFormatArg,
};
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

use super::support::{
    agent_backend_diagnostics, agent_backend_doctor_value, catalog, catalog_for,
    command_application, print_agent_record, print_agent_status, print_wait_report, read_prompt,
};

pub(crate) async fn run_agent_command(args: AgentArgs) -> Result<ExitCode> {
    match args.command {
        AgentCommand::List(args) => list_agents(args),
        AgentCommand::View(args) => view_agent(args),
        AgentCommand::Validate(args) => validate_agent(args),
        AgentCommand::Run(args) => run_agent(args).await,
        AgentCommand::Status(args) => agent_status(args).await,
        AgentCommand::Inspect(args) => inspect_agent(args).await,
        AgentCommand::Wait(args) => wait_agent(args).await,
        AgentCommand::Close(args) => close_agent(args).await,
        AgentCommand::Resume(args) => resume_agent(args).await,
        AgentCommand::Send(args) => send_agent(args).await,
        AgentCommand::Attach(args) => attach_agent(args).await,
        AgentCommand::Logs(args) => agent_logs(args).await,
        AgentCommand::Backend(args) => agent_backend(args),
    }
}

fn agent_backend(args: AgentBackendArgs) -> Result<ExitCode> {
    match args.command {
        AgentBackendCommand::List(args) => agent_backend_list(args),
        AgentBackendCommand::Add(args) => agent_backend_add(args),
        AgentBackendCommand::Doctor(args) => agent_backend_doctor(args),
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

fn agent_backend_list(args: AgentBackendListArgs) -> Result<ExitCode> {
    let (home, cwd, env_map) = backend_context()?;
    let backends = load_agent_backend_configs(&home, &cwd, &env_map)?;
    let values = backends
        .values()
        .map(agent_backend_value)
        .collect::<Vec<_>>();
    if args.json {
        println!("{}", serde_json::to_string(&json!({ "backends": values }))?);
    } else if backends.is_empty() {
        println!("No agent backends configured.");
    } else {
        for backend in backends.values() {
            println!(
                "{}\t{}\t{}\t{}",
                backend.id,
                backend.kind.as_str(),
                if backend.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                backend.command.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_backend_add(args: AgentBackendAddArgs) -> Result<ExitCode> {
    if !valid_agent_name(&args.id) {
        return Err(anyhow!("invalid backend id: {}", args.id));
    }
    if args.command.trim().is_empty() {
        return Err(anyhow!("backend command must be non-empty"));
    }
    let (home, cwd, _env_map) = backend_context()?;
    let config_dir = if args.local {
        cwd.join(".psychevo")
    } else {
        home.clone()
    };
    let entrypoints = if args.entrypoints.is_empty() {
        vec!["peer".to_string(), "subagent".to_string()]
    } else {
        validate_backend_entrypoints(&args.entrypoints)?
    };
    let client_capabilities = if args.client_capabilities.is_empty() {
        vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "terminal".to_string(),
        ]
    } else {
        validate_backend_client_capabilities(&args.client_capabilities)?
    };
    let mut value = serde_json::Map::new();
    value.insert("kind".to_string(), json!("acp"));
    value.insert("enabled".to_string(), json!(true));
    if let Some(label) = args
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        value.insert("label".to_string(), json!(label));
    }
    if let Some(description) = args
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        value.insert("description".to_string(), json!(description));
    }
    value.insert("command".to_string(), json!(args.command.trim()));
    value.insert("args".to_string(), json!(args.args));
    value.insert("entrypoints".to_string(), json!(entrypoints));
    value.insert(
        "client_capabilities".to_string(),
        json!(client_capabilities),
    );
    value.insert("cwd".to_string(), json!("invocation"));
    value.insert("env".to_string(), json!({}));
    let value = Value::Object(value);
    let result = set_config_value(config_dir, &format!("agents.backends.{}", args.id), value)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "path": result.path,
                "key": result.key,
                "changed": result.changed,
            }))?
        );
    } else {
        println!("backend: {}", args.id);
        println!("path: {}", result.path.display());
        println!("changed: {}", result.changed);
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_backend_doctor(args: AgentBackendDoctorArgs) -> Result<ExitCode> {
    let (home, cwd, env_map) = backend_context()?;
    let backends = load_agent_backend_configs(&home, &cwd, &env_map)?;
    let backend = backends
        .get(&args.id)
        .ok_or_else(|| anyhow!("unknown backend: {}", args.id))?;
    let value = agent_backend_doctor_value(backend, &env_map);
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!(
            "backend: {}\t{}\t{}",
            backend.id,
            backend.kind.as_str(),
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                "ok"
            } else {
                "failed"
            }
        );
        if let Some(checks) = value.get("checks").and_then(Value::as_array) {
            for check in checks {
                println!(
                    "{}\t{}\t{}",
                    check.get("name").and_then(Value::as_str).unwrap_or("-"),
                    if check.get("ok").and_then(Value::as_bool) == Some(true) {
                        "ok"
                    } else {
                        "failed"
                    },
                    check.get("message").and_then(Value::as_str).unwrap_or("")
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn view_agent(args: AgentNameArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let catalog = catalog_for(&home, &cwd, env_map.clone())?;
    let agent = resolve_agent_definition(&catalog, &args.name, &cwd, &env_map)?;
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
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let catalog = catalog_for(&home, &cwd, env_map.clone())?;
    let agent = resolve_agent_definition(&catalog, &args.name, &cwd, &env_map)?;
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

    let cwd = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let catalog = catalog_for(&home, &cwd, env_map.clone())?;
    let selected = resolve_agent_definition(&catalog, &args.name, &cwd, &env_map)?;
    let mut prompt = read_prompt(&args.message)?;
    if prompt.trim().is_empty()
        && let Some(initial) = selected.initial_prompt.clone()
    {
        prompt = initial;
    }
    if prompt.trim().is_empty() {
        return Err(anyhow!("You must provide a message"));
    }

    let mut builder = Application::builder().home(&home).database_path(&db_path);
    if let Some(config_path) = config_path.as_ref() {
        builder = builder.config_path(config_path);
    }
    let application = builder.build().await?;
    let execution = async {
        let mut start = StartThreadRequest::new(&cwd);
        start.source = "agent-run".to_string();
        start.metadata = Some(json!({
            "caller": "pevo agent run",
            "agent": selected.name.clone(),
            "pid": std::process::id(),
        }));
        let thread = application.client().start_thread(start).await?;
        let request = TurnRequest::new(prompt)
            .with_identity("agent-run", None)
            .with_model(
                args.model,
                args.variant.map(|variant| variant.as_str().to_string()),
            )
            .with_environment(Some(env_map), None, None)
            .with_agent(Some(selected.name), false, false);
        let handle = thread.start_turn(request).await?;
        let mut stream = handle.events();
        let events = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = stream.next().await {
                events.push(event);
            }
            events
        });
        let result = handle.wait().await?;
        let events = events
            .await
            .map_err(|error| anyhow!("Agent event task failed: {error}"))?;
        Ok::<_, anyhow::Error>((result, events))
    }
    .await;
    let shutdown = application.shutdown().await;
    let (result, events) = execution?;
    shutdown?.require_clean()?;

    if args.format == RunFormatArg::Json {
        for event in &events {
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

    let success = result.outcome == TurnOutcome::Completed && result.tool_failures == 0;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

async fn agent_status(args: AgentStatusArgs) -> Result<ExitCode> {
    let (application, cwd) = command_application().await?;
    let execution = async {
        let parent = if args.all {
            None
        } else {
            latest_thread_id(application.client(), &cwd, &["run", "tui"]).await?
        };
        Ok::<_, anyhow::Error>(
            application
                .agent_control()
                .status_value_for(parent.as_deref(), args.all)
                .await,
        )
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let value = execution?;
    shutdown?;
    if args.json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        print_agent_status(&value);
    }
    Ok(ExitCode::SUCCESS)
}

async fn inspect_agent(args: AgentInspectArgs) -> Result<ExitCode> {
    let (application, _cwd) = command_application().await?;
    let client = application.client();
    let control = application.agent_control();
    let execution = async {
        let relationship = client
            .agent_relationship(&args.id)
            .await?
            .ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
        let mut record =
            serde_json::to_value(agent_status_record(&control, &args.id, &relationship).await?)?;
        let parent_session = client
            .resume_thread(relationship.parent_thread_id.clone())
            .await?
            .summary()
            .await?;
        let child_thread = client
            .resume_thread(relationship.child_thread_id.clone())
            .await?;
        let child_session = child_thread.summary().await?;
        let history = child_thread.history();
        let latest_usage = history.latest_assistant_usage().await?;
        let messages = latest_history_items(&history, args.limit).await?;
        let latest_total_tokens = latest_usage.as_ref().and_then(usage_total_tokens);
        if let Some(object) = record.as_object_mut() {
            if let Some(usage) = latest_usage.clone() {
                object.insert("latest_usage".to_string(), usage);
            }
            if let Some(tokens) = latest_total_tokens {
                object.insert("latest_total_tokens".to_string(), Value::from(tokens));
            }
        }
        Ok::<_, anyhow::Error>((
            relationship,
            record,
            parent_session,
            child_session,
            messages,
        ))
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let (relationship, record, parent_session, child_session, messages) = execution?;
    shutdown?;

    if args.json {
        let messages = messages
            .iter()
            .map(tui_message_summary_value)
            .collect::<Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "agent": record,
                "edge": agent_relationship_value(&relationship),
                "parent_session": session_summary_value(&parent_session),
                "child_session": session_summary_value(&child_session),
                "messages": messages,
            }))?
        );
    } else {
        print_agent_inspect(
            &record,
            &relationship,
            Some(&parent_session),
            Some(&child_session),
            &messages,
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn wait_agent(args: AgentWaitArgs) -> Result<ExitCode> {
    let (application, cwd) = command_application().await?;
    let execution: Result<AgentMailboxWaitOutcome> = async {
        let thread_id = latest_thread_id(application.client(), &cwd, &["run"])
            .await?
            .ok_or_else(|| anyhow!("no run session found for {}", cwd.display()))?;
        Ok(application
            .client()
            .resume_thread(thread_id)
            .await?
            .wait_for_agent_mailbox(Duration::from_millis(args.timeout_ms))
            .await?)
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let outcome = execution?;
    shutdown?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&agent_wait_report_value(outcome))?
        );
    } else {
        print_wait_report(outcome);
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_wait_report_value(outcome: AgentMailboxWaitOutcome) -> Value {
    let timed_out = outcome == AgentMailboxWaitOutcome::TimedOut;
    json!({
        "message": if timed_out { "Wait timed out." } else { "Wait completed." },
        "timed_out": timed_out,
    })
}

async fn close_agent(args: AgentIdArgs) -> Result<ExitCode> {
    let (application, _cwd) = command_application().await?;
    let record = application.agent_control().close(&args.id).await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let record = record?;
    shutdown?;
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

async fn send_agent(args: AgentSendArgs) -> Result<ExitCode> {
    let (application, _cwd) = command_application().await?;
    let record = application
        .agent_control()
        .send(&args.id, &args.message.join(" "))
        .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let record = record?;
    shutdown?;
    if args.json {
        println!("{}", serde_json::to_string(&json!({ "agent": record }))?);
    } else if let Some(record) = record {
        print_agent_record(&record);
    } else {
        return Err(anyhow!("agent not found: {}", args.id));
    }
    Ok(ExitCode::SUCCESS)
}

async fn resume_agent(args: AgentIdArgs) -> Result<ExitCode> {
    let (application, _cwd) = command_application().await?;
    let record = application.agent_control().resume(&args.id).await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let record = record?;
    shutdown?;
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
    let (application, _cwd) = command_application().await?;
    let relationship = application.client().agent_relationship(&args.id).await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let relationship = relationship?.ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
    shutdown?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&json!({ "session": relationship.child_thread_id }))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("tui")
        .arg("--session")
        .arg(relationship.child_thread_id)
        .status()?;
    Ok(status
        .code()
        .map(|code| ExitCode::from(code as u8))
        .unwrap_or(ExitCode::FAILURE))
}

async fn agent_logs(args: AgentLogsArgs) -> Result<ExitCode> {
    let (application, _cwd) = command_application().await?;
    let client = application.client();
    let execution = async {
        let relationship = client
            .agent_relationship(&args.id)
            .await?
            .ok_or_else(|| anyhow!("agent not found: {}", args.id))?;
        let history = client
            .resume_thread(relationship.child_thread_id)
            .await?
            .history();
        let messages = latest_history_items(&history, args.limit).await?;
        Ok::<_, anyhow::Error>(messages)
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let messages = execution?;
    shutdown?;
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

async fn agent_status_record(
    control: &AgentControl,
    target: &str,
    relationship: &AgentRelationship,
) -> Result<AgentRunRecord> {
    control
        .status_records(None, true)
        .await
        .into_iter()
        .find(|record| {
            record.id == target
                || record.task_name.as_deref() == Some(target)
                || record.child_session_id.as_deref() == Some(target)
                || record.child_session_id.as_deref() == Some(relationship.child_thread_id.as_str())
        })
        .ok_or_else(|| anyhow!("agent not found: {target}"))
}

fn agent_relationship_value(relationship: &AgentRelationship) -> Value {
    json!({
        "parent_session_id": relationship.parent_thread_id,
        "child_session_id": relationship.child_thread_id,
        "status": relationship.status,
        "created_at_ms": relationship.created_at_ms,
        "updated_at_ms": relationship.updated_at_ms,
        "agent": relationship.agent,
    })
}

fn session_summary_value(summary: &ThreadSummary) -> Value {
    json!({
        "id": summary.id,
        "source": summary.source,
        "cwd": summary.cwd,
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

fn tui_message_summary_value(summary: &ThreadItem) -> Result<Value> {
    Ok(json!({
        "message": serde_json::to_value(&summary.message)?,
        "usage": summary.usage,
        "metadata": summary.metadata,
        "accounting": summary.accounting,
    }))
}

fn print_agent_inspect(
    record: &Value,
    relationship: &AgentRelationship,
    parent_session: Option<&ThreadSummary>,
    child_session: Option<&ThreadSummary>,
    messages: &[ThreadItem],
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
    println!(
        "edge: {}",
        match relationship.status {
            AgentRelationshipStatus::Open => "open",
            AgentRelationshipStatus::Closed => "closed",
        }
    );
    println!(
        "parent session: {}",
        session_summary_label(parent_session, &relationship.parent_thread_id)
    );
    println!(
        "child session: {}",
        session_summary_label(child_session, &relationship.child_thread_id)
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

fn session_summary_label(summary: Option<&ThreadSummary>, fallback_id: &str) -> String {
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

async fn latest_history_items(history: &HistoryReader, limit: usize) -> Result<Vec<ThreadItem>> {
    const PAGE_SIZE: usize = 200;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut items = Vec::with_capacity(limit.min(PAGE_SIZE));
    let mut before = None;
    while items.len() < limit {
        let page = history
            .before(before, Some((limit - items.len()).min(PAGE_SIZE)))
            .await?;
        let mut older = page.items;
        older.append(&mut items);
        items = older;
        let Some(next_before) = page.next_before else {
            break;
        };
        before = Some(next_before);
    }
    Ok(items)
}

async fn latest_thread_id(
    client: FrameworkClient,
    cwd: &std::path::Path,
    sources: &[&str],
) -> Result<Option<String>> {
    Ok(client
        .list_threads(ThreadListQuery {
            cwd: Some(cwd.to_path_buf()),
            archived: false,
            sources: sources.iter().map(|source| (*source).to_string()).collect(),
            cursor: None,
            limit: 1,
        })
        .await?
        .threads
        .into_iter()
        .next()
        .map(|thread| thread.id))
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    effective_usage_total(Some(usage)).tokens
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

fn backend_context() -> Result<(PathBuf, PathBuf, std::collections::BTreeMap<String, String>)> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    Ok((home, cwd, env_map))
}

fn validate_backend_entrypoints(values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if !matches!(value, "peer" | "subagent") {
            return Err(anyhow!(
                "backend entrypoint must be peer or subagent: {value}"
            ));
        }
        out.push(value.to_string());
    }
    Ok(out)
}

fn validate_backend_client_capabilities(values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if !matches!(value, "fs.read" | "fs.write" | "terminal") {
            return Err(anyhow!(
                "backend client capability must be fs.read, fs.write, or terminal: {value}"
            ));
        }
        out.push(value.to_string());
    }
    Ok(out)
}

fn agent_backend_value(backend: &AgentBackendConfig) -> Value {
    json!({
        "id": backend.id,
        "kind": backend.kind.as_str(),
        "enabled": backend.enabled,
        "label": backend.label,
        "description": backend.description,
        "command": backend.command,
        "args": backend.args,
        "cwd": backend.cwd,
        "entrypoints": backend.entrypoints,
        "clientCapabilities": backend.client_capabilities,
        "mcpServers": backend.mcp_servers,
        "envKeys": backend.env.keys().cloned().collect::<Vec<_>>(),
        "diagnostics": agent_backend_diagnostics(backend),
    })
}
