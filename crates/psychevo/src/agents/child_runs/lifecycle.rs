use super::super::{
    AbortSignal, AgentEdgeStatus, CompactSessionOptions, CompactionReason, ControlHandle, Error,
    ExternalAgentDelegateRequest, LanguageModel, Message, Result, ToolOutput, Uuid, Value,
    compact_session, json,
};
use super::super::{
    Deserialize,
    catalog_surface::{
        AgentContribution, AgentDefinition, AgentEntrypoint, AgentInvocationRole, AgentRunRecord,
        AgentRunStatus, AgentToolContext,
    },
    definition_policy::clamp_agent_spawn_depth,
    lifecycle::{agent_child_session_summary_value, model_content_string, subagent_summary_value},
    mailbox_tools::{
        append_parent_agent_start_notification, fork_messages, now_ms, update_run_child_session,
        update_run_completed, update_run_failed,
    },
    teams::AgentTeamMember,
};
use super::ActiveAgentRunGuard;
use super::policy::{
    ChildAgentMetadataInput, ExternalAgentSessionStart, agent_path, bind_child_model,
    child_agent_metadata, default_task_name, emit_external_agent_session_start, run_child_agent,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpawnAgentArgs {
    #[serde(default)]
    pub(crate) agent_type: Option<String>,
    pub(crate) task_name: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) background: Option<bool>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) fork_context: bool,
    #[serde(default)]
    pub(crate) fork_turns: Option<String>,
    #[serde(default)]
    pub(crate) max_turns: Option<usize>,
    #[serde(default)]
    pub(crate) max_spawn_depth: Option<u8>,
    #[serde(default)]
    pub(crate) team_member: Option<String>,
}

fn child_concurrency_limit(context: &AgentToolContext) -> usize {
    context
        .active_team
        .as_ref()
        .map(|team| team.max_parallel_agents as usize)
        .unwrap_or(super::super::supervisor::DEFAULT_CHILD_CONCURRENCY)
        .min(super::super::supervisor::DEFAULT_CHILD_CONCURRENCY)
}

pub(crate) async fn spawn_subagent(
    context: AgentToolContext,
    args: SpawnAgentArgs,
    tool_call_id: String,
    abort: AbortSignal,
) -> Result<ToolOutput> {
    if args.message.trim().is_empty() {
        return Err(Error::Message("spawn_agent message is empty".to_string()));
    }
    validate_task_name(&args.task_name)?;
    if context
        .supervisor
        .spawning_paused(&context.parent_session_id)
    {
        return Err(Error::Config("agent spawning is paused".to_string()));
    }
    if context.spawn_depth_remaining == Some(0) {
        return Err(Error::Config(
            "agent spawning is disabled for this child agent".to_string(),
        ));
    }
    let team_member = resolve_team_member_for_spawn(&context, &args)?;
    let agent_name =
        resolve_agent_tool_name(&args, &context.required_agent_names, team_member.as_ref())?;
    let agent = context
        .catalog
        .agents
        .iter()
        .find(|agent| agent.name == agent_name)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {agent_name}")))?;
    if !agent.supports_entrypoint(AgentEntrypoint::Subagent) {
        return Err(Error::Config(format!(
            "agent `{}` does not support subagent execution",
            agent.name
        )));
    }
    if let Some(allowed) = &context.allowed_agent_names
        && !allowed.contains(&agent.name)
    {
        return Err(Error::Config(format!(
            "agent `{}` is not allowed by selected-agent tool policy",
            agent.name
        )));
    }
    if context.denied_agent_names.contains(&agent.name) {
        return Err(Error::Config(format!(
            "agent `{}` is denied by selected-agent tool policy",
            agent.name
        )));
    }
    if external_agent_runtime_ref(&agent, team_member.as_ref()).is_some() {
        return spawn_external_subagent(context, args, tool_call_id, abort, agent, team_member)
            .await;
    }
    let (child_model, child_provider) = bind_child_model(&context, &agent, args.model.as_deref())?;
    let id = Uuid::now_v7().to_string();
    let task_name = args.task_name.trim().to_string();
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let background =
        args.fork_context || agent.background.unwrap_or(false) || args.background.unwrap_or(false);
    let role = if args.fork_context {
        AgentInvocationRole::Fork
    } else {
        AgentInvocationRole::Subagent
    };
    let mut record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.message.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: None,
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
        team_run_id: context
            .active_team
            .as_ref()
            .map(|team| team.team_run_id.clone()),
        mission_run_id: context
            .active_team
            .as_ref()
            .and_then(|team| team.mission_run_id.clone()),
        team_name: context
            .active_team
            .as_ref()
            .map(|team| team.team_name.clone()),
        team_member_id: team_member.as_ref().map(|member| member.id.clone()),
        agent_path: Some(agent_path(&task_name)),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    context.supervisor.register(
        record.clone(),
        Some(control_handle.clone()),
        child_concurrency_limit(&context),
    )?;
    let precreated_child_session = if background {
        match create_internal_child_session(InternalChildSessionInput {
            context: &context,
            agent: &agent,
            id: &id,
            task_name: &task_name,
            prompt: &args.message,
            model: &child_model,
            role,
            background,
            fork_context: args.fork_context,
            spawn_depth_remaining,
            team_member_id: team_member.as_ref().map(|member| member.id.as_str()),
            parent_tool_call_id: Some(&tool_call_id),
        })
        .await
        {
            Ok(child_session) => Some(child_session),
            Err(error) => {
                context.supervisor.remove(&id);
                return Err(error);
            }
        }
    } else {
        None
    };
    if let Some(child_session) = precreated_child_session.as_deref() {
        update_run_child_session(&context.supervisor, &id, child_session);
        record.child_session_id = Some(child_session.to_string());
    }
    let previous_messages_override = precreated_child_session.as_ref().map(|_| {
        fork_messages(
            &context.parent_context_snapshot,
            args.fork_context,
            args.fork_turns.as_deref(),
        )
    });
    let response_record = record.clone();

    let response_agent_name = agent.name.clone();
    let response_agent_description = agent.description.clone();
    let response_task_name = task_name.clone();
    let response_store = context.state.clone();
    let response_child_session_id = precreated_child_session.clone();
    let response_tool_call_id = tool_call_id.clone();
    let child_team_member_id = team_member.as_ref().map(|member| member.id.clone());
    let child = ChildRun {
        id: id.clone(),
        context,
        agent,
        prompt: args.message,
        task_name: task_name.clone(),
        model: child_model,
        provider: child_provider,
        fork_context: args.fork_context,
        fork_turns: args.fork_turns,
        max_turns: args.max_turns,
        spawn_depth_remaining,
        role,
        background,
        team_member_id: child_team_member_id,
        parent_tool_call_id: Some(tool_call_id),
        existing_child_session: precreated_child_session,
        previous_messages_override,
        control_handle,
        control_receivers,
        abort,
    };

    if background {
        let supervisor = child.context.supervisor.clone();
        if let Err(finalizer) = supervisor.spawn_background(Box::pin(async move {
            let _ = run_child_agent(child).await;
        })) {
            finalizer.await;
            return Err(Error::Message(
                "agent supervisor is shutting down".to_string(),
            ));
        }
        let system_value = json!({
            "id": id,
            "agent_name": response_agent_name.clone(),
            "agent_description": response_agent_description,
            "agent_type": response_agent_name,
            "agent_path": agent_path(&response_task_name),
            "task_name": response_task_name,
            "message": response_record.task.clone(),
            "task": response_record.task.clone(),
            "tool_call_id": response_tool_call_id,
            "parent_thread_id": response_record.parent_session_id.clone(),
            "status": "running",
            "background": true,
            "session_id": response_child_session_id,
            "child_thread_id": response_record.child_session_id.clone(),
            "child_session_id": response_record.child_session_id.clone(),
            "effective_max_spawn_depth": spawn_depth_remaining
        });
        let model_value =
            subagent_summary_value(Some(&response_store), &response_record, true).await;
        Ok(ToolOutput::ok_with_model_content(
            system_value,
            model_content_string(&model_value),
        ))
    } else {
        let record = run_child_agent(child).await;
        let record = record?;
        let model_value = subagent_summary_value(Some(&response_store), &record, false).await;
        let response_child_session_id = record.child_session_id.clone();
        let child_summary = match record.child_session_id.as_deref() {
            Some(session_id) => match response_store
                .session_summary(session_id)
                .await
                .ok()
                .flatten()
            {
                Some(summary) => {
                    Some(agent_child_session_summary_value(&response_store, &summary).await)
                }
                None => None,
            },
            None => None,
        };
        let system_value = json!({
            "id": record.id,
            "agent_name": record.agent_name.clone(),
            "agent_description": response_agent_description,
            "agent_type": record.agent_name,
            "agent_path": record.task_name.as_deref().map(agent_path),
            "task_name": record.task_name,
            "message": record.task.clone(),
            "task": record.task,
            "parent_thread_id": record.parent_session_id,
            "status": record.status.as_str(),
            "background": false,
            "session_id": response_child_session_id,
            "child_thread_id": record.child_session_id.clone(),
            "child_session_id": record.child_session_id,
            "outcome": record.outcome,
            "final_answer": record.final_answer,
            "error": record.error,
            "child_session": child_summary,
            "effective_max_spawn_depth": record.effective_max_spawn_depth,
        });
        Ok(ToolOutput::ok_with_model_content(
            system_value,
            model_content_string(&model_value),
        ))
    }
}

struct InternalChildSessionInput<'a> {
    context: &'a AgentToolContext,
    agent: &'a AgentDefinition,
    id: &'a str,
    task_name: &'a str,
    prompt: &'a str,
    model: &'a str,
    role: AgentInvocationRole,
    background: bool,
    fork_context: bool,
    spawn_depth_remaining: u8,
    team_member_id: Option<&'a str>,
    parent_tool_call_id: Option<&'a str>,
}

async fn create_internal_child_session(input: InternalChildSessionInput<'_>) -> Result<String> {
    let context = input.context;
    let agent = input.agent;
    let mut metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: input.id,
        task_name: input.task_name,
        agent,
        parent_session_id: &context.parent_session_id,
        role: input.role,
        task: input.prompt,
        background: input.background,
        fork_context: input.fork_context,
        spawn_depth_remaining: input.spawn_depth_remaining,
        team_member_id: input.team_member_id,
        context: Some(context),
        parent_tool_call_id: input.parent_tool_call_id,
    });
    let child_session = context
        .state
        .create_child_session_with_metadata(
            &context.parent_session_id,
            &context.cwd,
            "agent",
            input.model,
            &context.model_provider,
            Some(metadata.clone()),
        )
        .await?;
    attach_child_thread_metadata(&mut metadata, &child_session);
    context
        .state
        .upsert_agent_edge(
            &context.parent_session_id,
            &child_session,
            AgentEdgeStatus::Open,
            Some(metadata),
        )
        .await?;
    Ok(child_session)
}

fn attach_child_thread_metadata(metadata: &mut Value, child_session: &str) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "child_thread_id".to_string(),
            Value::String(child_session.to_string()),
        );
        object.insert(
            "child_session_id".to_string(),
            Value::String(child_session.to_string()),
        );
        if let Some(agent) = object.get_mut("agent").and_then(Value::as_object_mut) {
            agent.insert(
                "child_thread_id".to_string(),
                Value::String(child_session.to_string()),
            );
            agent.insert(
                "child_session_id".to_string(),
                Value::String(child_session.to_string()),
            );
        }
    }
}

pub(super) fn external_agent_runtime_ref(
    agent: &AgentDefinition,
    team_member: Option<&AgentTeamMember>,
) -> Option<String> {
    if let Some(runtime_ref) = team_member
        .and_then(|member| member.runtime_ref.as_deref())
        .map(str::trim)
        .filter(|runtime_ref| !runtime_ref.is_empty())
    {
        return (runtime_ref != "native").then(|| runtime_ref.to_string());
    }
    agent.backend.as_ref().map(|backend| {
        crate::config::generated_runtime_profile_id_for_backend(backend.name.as_str())
    })
}

async fn spawn_external_subagent(
    context: AgentToolContext,
    args: SpawnAgentArgs,
    tool_call_id: String,
    abort: AbortSignal,
    agent: AgentDefinition,
    team_member: Option<AgentTeamMember>,
) -> Result<ToolOutput> {
    if abort.aborted() {
        return Err(Error::Message("parent invocation aborted".to_string()));
    }
    if args.fork_context {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is paired with an external Runtime Profile and does not support fork_context; run it as a foreground delegated task",
            agent.name
        )));
    }
    if args.background.unwrap_or(false) || agent.background.unwrap_or(false) {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is paired with an external Runtime Profile and does not support background delegation yet; run it as a foreground delegated task",
            agent.name
        )));
    }
    let Some(delegate) = context.external_delegate.clone() else {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is paired with an external Runtime Profile, but this execution context cannot delegate runtime-backed members",
            agent.name
        )));
    };
    let Some(runtime_ref) = external_agent_runtime_ref(&agent, team_member.as_ref()) else {
        return Ok(ToolOutput::error(format!(
            "agent `{}` does not select an external Runtime Profile",
            agent.name
        )));
    };
    let backend_ref = agent.backend.as_ref().map(|backend| backend.name.clone());
    let required_contributions = unsupported_external_agent_contributions(&agent);
    if !required_contributions.is_empty() {
        return Ok(ToolOutput::error(format!(
            "Agent Definition `{}` requires {} contribution(s), but the selected runtime delegate cannot faithfully inject them; pairing refused instead of silently omitting them",
            agent.name,
            required_contributions.join(", ")
        )));
    }

    let id = Uuid::now_v7().to_string();
    let task_name = args.task_name.trim().to_string();
    let runtime_options = team_member
        .as_ref()
        .map(|member| member.runtime_options.clone())
        .unwrap_or_default();
    let model = args
        .model
        .clone()
        .or_else(|| runtime_options.get("model").cloned())
        .or_else(|| agent.model.clone());
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let mut record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.message.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: None,
        role: AgentInvocationRole::Subagent,
        background: false,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
        team_run_id: context
            .active_team
            .as_ref()
            .map(|team| team.team_run_id.clone()),
        mission_run_id: context
            .active_team
            .as_ref()
            .and_then(|team| team.mission_run_id.clone()),
        team_name: context
            .active_team
            .as_ref()
            .map(|team| team.team_name.clone()),
        team_member_id: team_member.as_ref().map(|member| member.id.clone()),
        agent_path: Some(agent_path(&task_name)),
    };
    context
        .supervisor
        .register(record.clone(), None, child_concurrency_limit(&context))?;
    let _active_run = ActiveAgentRunGuard::new(context.supervisor.clone(), id.clone());
    let mut metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: &id,
        task_name: &task_name,
        agent: &agent,
        parent_session_id: &context.parent_session_id,
        role: AgentInvocationRole::Subagent,
        task: &args.message,
        background: false,
        fork_context: false,
        spawn_depth_remaining,
        team_member_id: team_member.as_ref().map(|member| member.id.as_str()),
        context: Some(&context),
        parent_tool_call_id: Some(&tool_call_id),
    });
    let child_provider = backend_ref
        .as_deref()
        .map(|backend| format!("acp:{backend}"))
        .unwrap_or_else(|| runtime_ref.clone());
    let child_session = context
        .state
        .create_child_session_with_metadata(
            &context.parent_session_id,
            &context.cwd,
            "peer_agent",
            model.as_deref().unwrap_or(&agent.name),
            &child_provider,
            Some(metadata.clone()),
        )
        .await?;
    attach_child_thread_metadata(&mut metadata, &child_session);
    context
        .state
        .upsert_agent_edge(
            &context.parent_session_id,
            &child_session,
            AgentEdgeStatus::Open,
            Some(metadata),
        )
        .await?;
    update_run_child_session(&context.supervisor, &id, &child_session);
    record.child_session_id = Some(child_session.clone());
    emit_external_agent_session_start(ExternalAgentSessionStart {
        context: &context,
        agent: &agent,
        id: &id,
        task_name: &task_name,
        task: &args.message,
        tool_call_id: &tool_call_id,
        child_session_id: &child_session,
        spawn_depth_remaining,
        team_member_id: team_member.as_ref().map(|member| member.id.as_str()),
        runtime_ref: Some(runtime_ref.as_str()),
    });

    let request = ExternalAgentDelegateRequest {
        run_id: id.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: child_session.clone(),
        agent_name: agent.name.clone(),
        agent_description: agent.description.clone(),
        runtime_ref,
        backend_ref,
        instructions: (!agent.instructions.trim().is_empty())
            .then(|| agent.instructions.trim().to_string()),
        prompt: args.message.clone(),
        task_name: task_name.clone(),
        model,
        runtime_options,
        expected_runtime_profile_revision: team_member
            .as_ref()
            .and_then(|member| member.runtime_profile_revision),
        abort,
    };
    let result = delegate.run(request).await;
    let record = match result {
        Ok(result) => {
            let record = update_run_completed(
                &context.supervisor,
                &id,
                result.outcome,
                result.final_answer.clone(),
            );
            context.supervisor.finish(&context.state, &id).await?;
            record
        }
        Err(err) => {
            update_run_failed(&context.supervisor, &id, &err.to_string());
            let record = {
                let runs = context.supervisor.slots();
                runs.get(&id)
                    .map(|state| state.record.clone())
                    .expect("external Agent run exists")
            };
            context.supervisor.finish(&context.state, &id).await?;
            let model_value = subagent_summary_value(Some(&context.state), &record, false).await;
            return Ok(ToolOutput::error(model_content_string(&model_value)));
        }
    };
    let model_value = subagent_summary_value(Some(&context.state), &record, false).await;
    let child_summary = match record.child_session_id.as_deref() {
        Some(session_id) => match context
            .state
            .session_summary(session_id)
            .await
            .ok()
            .flatten()
        {
            Some(summary) => {
                Some(agent_child_session_summary_value(&context.state, &summary).await)
            }
            None => None,
        },
        None => None,
    };
    let response_child_session_id = record.child_session_id.clone();
    let system_value = json!({
        "id": record.id,
        "agent_name": record.agent_name.clone(),
        "agent_description": agent.description,
        "agent_type": record.agent_name,
        "agent_path": record.task_name.as_deref().map(agent_path),
        "task_name": record.task_name,
        "message": record.task.clone(),
        "task": record.task,
        "parent_thread_id": record.parent_session_id,
        "status": record.status.as_str(),
        "background": false,
        "session_id": response_child_session_id,
        "child_thread_id": record.child_session_id.clone(),
        "child_session_id": record.child_session_id,
        "outcome": record.outcome,
        "final_answer": record.final_answer,
        "error": record.error,
        "child_session": child_summary,
        "effective_max_spawn_depth": record.effective_max_spawn_depth,
    });
    Ok(ToolOutput::ok_with_model_content(
        system_value,
        model_content_string(&model_value),
    ))
}

fn unsupported_external_agent_contributions(agent: &AgentDefinition) -> Vec<&'static str> {
    external_agent_contributions_without_injection(agent)
        .into_iter()
        .filter(|contribution| !agent.contribution_is_optional(*contribution))
        .map(AgentContribution::as_str)
        .collect()
}

pub(crate) fn optional_external_agent_contributions(agent: &AgentDefinition) -> Vec<&'static str> {
    external_agent_contributions_without_injection(agent)
        .into_iter()
        .filter(|contribution| agent.contribution_is_optional(*contribution))
        .map(AgentContribution::as_str)
        .collect()
}

fn external_agent_contributions_without_injection(
    agent: &AgentDefinition,
) -> Vec<AgentContribution> {
    let mut contributions = Vec::new();
    if agent.tool_policy.allowed.is_some()
        || !agent.tool_policy.denied.is_empty()
        || agent.tool_policy.allowed_agents.is_some()
        || !agent.tool_policy.denied_agents.is_empty()
    {
        contributions.push(AgentContribution::Tools);
    }
    if !agent.tool_policy.mcp_servers.is_empty() {
        contributions.push(AgentContribution::Mcp);
    }
    if !agent.skills.is_empty() {
        contributions.push(AgentContribution::Skills);
    }
    contributions
}

pub(crate) fn resolve_agent_tool_name(
    args: &SpawnAgentArgs,
    required_agent_names: &[String],
    team_member: Option<&AgentTeamMember>,
) -> Result<String> {
    if let Some(member) = team_member {
        if let Some(agent_type) = normalized_optional_name(args.agent_type.as_deref())
            && agent_type != member.agent
        {
            return Err(Error::Config(format!(
                "team_member `{}` uses agent `{}`; spawn_agent agent_type `{agent_type}` does not match",
                member.id, member.agent
            )));
        }
        return Ok(member.agent.clone());
    }
    let agent_type = normalized_optional_name(args.agent_type.as_deref());
    if let Some(name) = agent_type {
        return Ok(name);
    }
    match required_agent_names {
        [single] => Ok(single.clone()),
        [] => Ok("general".to_string()),
        many => Err(Error::Config(format!(
            "spawn_agent call must set agent_type when the user mentioned multiple agents: {}",
            many.join(", ")
        ))),
    }
}

pub(crate) fn resolve_team_member_for_spawn(
    context: &AgentToolContext,
    args: &SpawnAgentArgs,
) -> Result<Option<AgentTeamMember>> {
    let Some(member_id) = normalized_optional_name(args.team_member.as_deref()) else {
        return Ok(None);
    };
    let Some(team) = &context.active_team else {
        return Err(Error::Config(
            "spawn_agent team_member is only valid inside an active team context".to_string(),
        ));
    };
    let Some(member) = team.member(&member_id) else {
        return Err(Error::Config(format!(
            "unknown team member `{member_id}` for team `{}`",
            team.team_name
        )));
    };
    Ok(Some(member.clone()))
}

pub(crate) fn normalized_optional_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn validate_task_name(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "root" | "." | "..") {
        return Err(Error::Message(
            "task_name must use lowercase letters, digits, and underscores".to_string(),
        ));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        Ok(())
    } else {
        Err(Error::Message(
            "task_name must use lowercase letters, digits, and underscores".to_string(),
        ))
    }
}

pub(crate) fn child_spawn_depth_remaining(
    context: &AgentToolContext,
    agent: &AgentDefinition,
    requested: Option<u8>,
) -> u8 {
    resolved_child_spawn_depth_remaining(
        context.spawn_depth_remaining,
        agent.max_spawn_depth,
        requested,
    )
}

pub(crate) fn resolved_child_spawn_depth_remaining(
    parent_remaining: Option<u8>,
    definition_depth: u8,
    requested: Option<u8>,
) -> u8 {
    let requested = clamp_agent_spawn_depth(requested.or(Some(definition_depth)));
    match parent_remaining {
        Some(parent_remaining) => requested.min(parent_remaining.saturating_sub(1)),
        None => requested,
    }
}

pub(crate) async fn spawn_child_agent_background(
    context: AgentToolContext,
    agent: AgentDefinition,
    prompt: String,
) -> Result<AgentRunRecord> {
    if prompt.trim().is_empty() {
        return Err(Error::Message("agent message is empty".to_string()));
    }
    if context
        .supervisor
        .spawning_paused(&context.parent_session_id)
    {
        return Err(Error::Config("agent spawning is paused".to_string()));
    }
    let worker_lease = context.extension_inputs.acquire_worker_lease();
    let id = Uuid::now_v7().to_string();
    let task_name = default_task_name(&agent.name, &id);
    let role = AgentInvocationRole::Subagent;
    let background = true;
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, None);
    let (child_model, child_provider) = bind_child_model(&context, &agent, None)?;
    let metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: &id,
        task_name: &task_name,
        agent: &agent,
        parent_session_id: &context.parent_session_id,
        role,
        task: &prompt,
        background,
        fork_context: false,
        spawn_depth_remaining,
        team_member_id: None,
        context: Some(&context),
        parent_tool_call_id: None,
    });
    let mut record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: prompt.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: None,
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
        team_run_id: context
            .active_team
            .as_ref()
            .map(|team| team.team_run_id.clone()),
        mission_run_id: context
            .active_team
            .as_ref()
            .and_then(|team| team.mission_run_id.clone()),
        team_name: context
            .active_team
            .as_ref()
            .map(|team| team.team_name.clone()),
        team_member_id: None,
        agent_path: Some(agent_path(&task_name)),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    context.supervisor.register(
        record.clone(),
        Some(control_handle.clone()),
        child_concurrency_limit(&context),
    )?;
    let child_session = match context
        .state
        .create_child_session_with_metadata(
            &context.parent_session_id,
            &context.cwd,
            "agent",
            &child_model,
            &context.model_provider,
            Some(metadata.clone()),
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            context.supervisor.remove(&id);
            return Err(error);
        }
    };
    if let Err(error) = context
        .state
        .upsert_agent_edge(
            &context.parent_session_id,
            &child_session,
            AgentEdgeStatus::Open,
            Some(metadata),
        )
        .await
    {
        context.supervisor.remove(&id);
        return Err(error);
    }
    update_run_child_session(&context.supervisor, &id, &child_session);
    record.child_session_id = Some(child_session.clone());
    if let Err(error) =
        append_parent_agent_start_notification(&context.state, &context.parent_session_id, &record)
            .await
    {
        let _ = context
            .state
            .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed)
            .await;
        context.supervisor.remove(&id);
        return Err(error);
    }
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let child = ChildRun {
        id,
        context,
        agent,
        prompt,
        task_name,
        model: child_model,
        provider: child_provider,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        spawn_depth_remaining,
        role,
        background,
        team_member_id: None,
        parent_tool_call_id: None,
        existing_child_session: Some(child_session),
        previous_messages_override: Some(Vec::new()),
        control_handle,
        control_receivers,
        abort: AbortSignal::new(abort_rx),
    };
    let supervisor = child.context.supervisor.clone();
    if let Err(finalizer) = supervisor.spawn_background(Box::pin(async move {
        let _ = run_child_agent(child).await;
        if let Some(lease) = worker_lease {
            lease.shutdown().await;
        }
    })) {
        finalizer.await;
        return Err(Error::Message(
            "agent supervisor is shutting down".to_string(),
        ));
    }
    Ok(record)
}

pub(crate) struct ChildRun {
    pub(crate) id: String,
    pub(crate) context: AgentToolContext,
    pub(crate) agent: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task_name: String,
    pub(crate) model: String,
    pub(crate) provider: LanguageModel,
    pub(crate) fork_context: bool,
    pub(crate) fork_turns: Option<String>,
    pub(crate) max_turns: Option<usize>,
    pub(crate) spawn_depth_remaining: u8,
    pub(crate) role: AgentInvocationRole,
    pub(crate) background: bool,
    pub(crate) team_member_id: Option<String>,
    pub(crate) parent_tool_call_id: Option<String>,
    pub(crate) existing_child_session: Option<String>,
    pub(crate) previous_messages_override: Option<Vec<Message>>,
    pub(crate) control_handle: ControlHandle,
    pub(crate) control_receivers: psychevo_agent_core::ControlReceivers,
    pub(crate) abort: AbortSignal,
}

pub(crate) async fn maybe_preflight_child_compaction(
    context: &AgentToolContext,
    child_session: &str,
    child_model: &str,
) -> Result<()> {
    let _ = compact_session(CompactSessionOptions {
        state: context.state.clone(),
        cwd: context.cwd.clone(),
        session: child_session.to_string(),
        config_path: context.config_path.clone(),
        model: Some(format!("{}/{}", context.model_provider, child_model)),
        reasoning_effort: context.reasoning_effort.clone(),
        inherited_env: Some(context.env.clone()),
        reason: CompactionReason::AutoThreshold,
        instructions: None,
        force: false,
    })
    .await?;
    Ok(())
}
