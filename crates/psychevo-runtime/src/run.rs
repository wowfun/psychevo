use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use psychevo_agent_core::{
    AgentLoopRequest, AssistantBlock, ControlHandle, Message, NoopEventSink, PromptInstruction,
    run_agent_loop, user_text_message,
};
use psychevo_ai::{GenerationProvider, OpenAiChatProvider, Outcome};
use serde_json::json;
use tokio::time;

use crate::agents::{
    AgentDefinition, AgentDiscoveryOptions, AgentToolContext, agent_catalog_for_prompt,
    agent_catalog_for_selected_policy, agent_mailbox_event_message,
    agent_policy_allows_agent_spawn, agent_project_instructions_enabled, agent_tools,
    apply_agent_hooks, apply_agent_tool_policy, discover_agents, effective_tool_names,
    narrow_permission_mode_for_agent, resolve_agent_definition, resolve_agents_home,
    run_agent_hook_event, skill_catalog_visible_for_tools, spawn_child_agent_background,
};
use crate::config::{ResolvedRunProvider, load_run_config, resolve_run_provider};
use crate::context::prune_context;
use crate::context_usage::{
    ContextRecorder, ContextRecordingProvider, LiveContextProfile, context_counting_metadata,
};
use crate::error::{Error, Result};
use crate::events::PersistenceSink;
use crate::messages::assistant_text;
use crate::paths::canonical_workdir;
use crate::permissions::PermissionRuntime;
use crate::project_instructions::load_project_instructions;
use crate::prompt_assembly::{
    PROMPT_PREFIX_NOTICE_METADATA_KEY, PromptPrefixRecordInput, assemble_main_prompt_prefix,
    assembly_from_prefix_record, context_evidence_for_request, developer_provider_role,
    prompt_prefix_record, skill_contextual_user_messages, tool_declarations_hash,
    turn_prefix_notice_instruction, turn_required_agent_instruction,
};
use crate::prompt_image::prompt_message_from_inputs_with_options;
use crate::skills::{
    SelectedSkill, SkillCatalog, SkillDiscoveryOptions, discover_skills, resolve_skills_home,
    select_explicit_skills, select_skills_for_prompt, skill_context_fragments,
};
use crate::snapshot::SnapshotStore;
use crate::store::{PromptPrefixRecord, SqliteStore};
use crate::tools::{coding_core_tools_for_mode, skill_tools_for_mode};
use crate::types::{
    AgentSpawnOptions, AgentSpawnResult, ModelMetadata, PermissionConfig, ReloadContextOptions,
    ReloadContextResult, RunControl, RunOptions, RunResult, RunStreamEvent, RunStreamSink,
    RunWarning, SelectedAgent, SmokeControl,
};

const TITLE_GENERATION_TIMEOUT_SECS: u64 = 15;
const DEFAULT_AGENT_MAX_TURNS: usize = 128;
pub(crate) const SESSION_TITLE_MAX_CHARS: usize = 100;

pub async fn run_live(options: RunOptions) -> Result<RunResult> {
    run_live_internal(options, "run", &["run"], None, None).await
}

pub async fn run_live_streaming(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
) -> Result<RunResult> {
    run_live_internal(options, source, continue_sources, Some(stream), None).await
}

pub async fn run_live_streaming_controlled(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
    control: RunControl,
) -> Result<RunResult> {
    run_live_internal(
        options,
        source,
        continue_sources,
        Some(stream),
        Some(control),
    )
    .await
}

pub fn reload_session_context(options: ReloadContextOptions) -> Result<ReloadContextResult> {
    let store = SqliteStore::open(&options.db_path)?;
    let summary = store
        .session_summary(&options.session)?
        .ok_or_else(|| Error::Message(format!("session not found: {}", options.session)))?;
    let metadata = store.session_metadata(&summary.id)?.unwrap_or(json!({}));
    let workdir = canonical_workdir(std::path::Path::new(&summary.workdir))?;
    let mode = options
        .mode
        .or_else(|| {
            metadata
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .and_then(crate::types::RunMode::parse)
        })
        .unwrap_or_default();
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let agents_home = resolve_agents_home(&env, &workdir)?;
    let agent_input = options
        .agent
        .clone()
        .or_else(|| session_agent_input_from_metadata(&metadata));
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        workdir: workdir.clone(),
        env: env.clone(),
        explicit_inputs: agent_input.iter().cloned().collect(),
        no_agents: options.no_agents,
    })?;
    let selected_agent = match &agent_input {
        Some(input) if !options.no_agents => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &workdir,
            &env,
        )?),
        _ => None,
    };
    let skills_home = resolve_skills_home(&env, &workdir)?;
    let mut explicit_skill_inputs = Vec::new();
    if let Some(agent) = &selected_agent {
        explicit_skill_inputs.extend(agent.skills.clone());
    }
    let skill_options = SkillDiscoveryOptions {
        home: skills_home,
        workdir: workdir.clone(),
        config_path: options.config_path.clone(),
        env: env.clone(),
        explicit_inputs: explicit_skill_inputs,
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let project_instructions = load_project_instructions(&workdir)?;
    let model_metadata = session_model_metadata(&metadata);
    let mut tools = coding_core_tools_for_mode(&workdir, mode);
    if !options.no_skills {
        tools.extend(skill_tools_for_mode(skill_options, mode));
    }
    if !options.no_agents {
        let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
            String::new(),
            String::new(),
            summary.provider.clone(),
        ));
        tools.extend(agent_tools(AgentToolContext {
            provider,
            model_provider: summary.provider.clone(),
            model: summary.model.clone(),
            provider_label: metadata
                .get("provider_label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(summary.provider.as_str())
                .to_string(),
            base_url: metadata
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            api_key_env: metadata
                .get("api_key_env")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            reasoning_effort: metadata
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            context_limit: metadata
                .get("context_limit")
                .and_then(serde_json::Value::as_u64),
            generation_metadata: json!({
                "model_metadata": model_metadata.public_json(),
            }),
            workdir: workdir.clone(),
            mode,
            permission_config: PermissionConfig::default(),
            permission_mode: Default::default(),
            approval_mode: Default::default(),
            approval_handler: None,
            store: store.clone(),
            parent_session_id: summary.id.clone(),
            parent_context_snapshot: Vec::new(),
            catalog: agent_catalog.clone(),
            control_handle: None,
            stream_events: None,
            model_metadata: model_metadata.clone(),
            env: env.clone(),
            allowed_agent_names: selected_agent
                .as_ref()
                .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
            denied_agent_names: selected_agent
                .as_ref()
                .map(|agent| agent.tool_policy.denied_agents.clone())
                .unwrap_or_default(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
        }));
    }
    tools = apply_agent_tool_policy(tools, selected_agent.as_ref(), mode);
    tools = apply_agent_hooks(tools, selected_agent.as_ref(), &workdir);
    let effective_tool_names = effective_tool_names(&tools);
    let prompt_agents = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_prompt(&agent_catalog.agents, selected_agent.as_ref(), &tools)
    };
    let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
        skill_catalog.skills.clone()
    } else {
        Vec::new()
    };
    let prompt_project_instructions = if agent_project_instructions_enabled(selected_agent.as_ref())
    {
        project_instructions.fragments.as_slice()
    } else {
        &[]
    };
    let project_instructions_role = (!prompt_project_instructions.is_empty())
        .then(|| developer_provider_role(&model_metadata.capabilities).to_string());
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let selected_agent_summary = selected_agent_for_result(selected_agent.as_ref());
    let assembly = assemble_main_prompt_prefix(
        mode,
        selected_agent.as_ref(),
        &prompt_agents,
        &prompt_skills,
        prompt_project_instructions,
        &model_metadata.capabilities,
        !tools.is_empty(),
    );
    let record = prompt_prefix_record(PromptPrefixRecordInput {
        session_id: &summary.id,
        provider: &summary.provider,
        model: &summary.model,
        prefix_hash: assembly.prefix_hash,
        tool_declarations_hash,
        invalidation_reason: Some(options.invalidation_reason),
        slots: assembly.prefix_slots,
        metadata: Some(json!({
            "mode": mode.as_str(),
            "selected_agent": selected_agent_summary,
            "agents_enabled": !options.no_agents,
            "effective_tools": effective_tool_names,
            "agent_catalog_visible": !prompt_agents.is_empty(),
            "visible_agents": prompt_agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
            "skill_catalog_visible": !prompt_skills.is_empty(),
            "project_instructions_visible": !prompt_project_instructions.is_empty(),
            "project_instructions_role": project_instructions_role,
        })),
    });
    let record = store.upsert_session_prompt_prefix(record)?;
    if let Some(notice) = options.notice {
        store.set_session_metadata_field(
            &summary.id,
            PROMPT_PREFIX_NOTICE_METADATA_KEY,
            Some(serde_json::Value::String(notice)),
        )?;
    }
    Ok(ReloadContextResult {
        session_id: summary.id,
        prefix_hash: record.prefix_hash,
        version: record.version,
        provider: record.provider,
        model: record.model,
        invalidation_reason: record.invalidation_reason,
    })
}

pub async fn spawn_agent_background(options: AgentSpawnOptions) -> Result<AgentSpawnResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.prompt.trim().is_empty() {
        return Err(Error::Message("Agent prompt is empty".to_string()));
    }
    let run_options = RunOptions {
        db_path: options.db_path.clone(),
        workdir: workdir.clone(),
        snapshot_root: None,
        session: options.parent_session.clone(),
        continue_latest: false,
        prompt: options.prompt.clone(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        model: options.model.clone(),
        reasoning_effort: options.reasoning_effort.clone(),
        include_reasoning: false,
        mode: options.mode,
        permission_mode: options.permission_mode,
        approval_mode: options.approval_mode,
        approval_handler: options.approval_handler.clone(),
        inherited_env: options.inherited_env.clone(),
        agent: options.selected_parent_agent.clone(),
        no_agents: false,
        no_skills: options.no_skills,
        skill_inputs: options.skill_inputs.clone(),
    };
    let loaded = load_run_config(&run_options, &workdir)?;
    let permission_mode = options
        .permission_mode
        .or(loaded.config.permissions.permission_mode)
        .unwrap_or_default();
    let approval_mode = options
        .approval_mode
        .or(loaded.config.permissions.approval_mode)
        .unwrap_or_default();
    let agents_home = resolve_agents_home(&loaded.env, &workdir)?;
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        workdir: workdir.clone(),
        env: loaded.env.clone(),
        explicit_inputs: options.selected_parent_agent.iter().cloned().collect(),
        no_agents: false,
    })?;
    let selected_parent_agent = match &options.selected_parent_agent {
        Some(input) => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &workdir,
            &loaded.env,
        )?),
        None => None,
    };
    let permission_mode =
        narrow_permission_mode_for_agent(permission_mode, selected_parent_agent.as_ref());
    let child_agent =
        resolve_agent_definition(&agent_catalog, &options.agent, &workdir, &loaded.env)?;
    if selected_parent_agent
        .as_ref()
        .is_some_and(|agent| !agent_policy_allows_agent_spawn(agent))
    {
        return Err(Error::Config(
            "agent spawning is not allowed by selected-agent tool policy".to_string(),
        ));
    }
    if let Some(allowed) = selected_parent_agent
        .as_ref()
        .and_then(|agent| agent.tool_policy.allowed_agents.as_ref())
        && !allowed.contains(&child_agent.name)
    {
        return Err(Error::Config(format!(
            "agent `{}` is not allowed by selected-agent tool policy",
            child_agent.name
        )));
    }
    if selected_parent_agent
        .as_ref()
        .is_some_and(|agent| agent.tool_policy.denied_agents.contains(&child_agent.name))
    {
        return Err(Error::Config(format!(
            "agent `{}` is denied by selected-agent tool policy",
            child_agent.name
        )));
    }
    let mut resolved_options = run_options.clone();
    if resolved_options.model.is_none()
        && let Some(model) = selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.model.clone())
    {
        resolved_options.model = Some(model);
    }
    if resolved_options.reasoning_effort.is_none()
        && let Some(effort) = selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.effort.clone())
    {
        resolved_options.reasoning_effort = Some(effort);
    }
    let resolved = resolve_run_provider(&resolved_options, &loaded)?;
    let store = SqliteStore::open(&options.db_path)?;
    let selected_parent_summary = selected_agent_for_result(selected_parent_agent.as_ref());
    let parent_session_id = if let Some(session_id) = options.parent_session.clone() {
        store.resume_session(&session_id)?;
        session_id
    } else {
        store.create_session_with_metadata(
            &workdir,
            "tui",
            &resolved.model,
            &resolved.provider,
            Some(json!({
                "provider_label": resolved.display_label.clone(),
                "base_url": resolved.base_url.clone(),
                "api_key_env": resolved.api_key_env.clone(),
                "reasoning_effort": resolved.reasoning_effort.clone(),
                "context_limit": resolved.context_limit,
                "model_metadata": resolved.metadata.public_json(),
                "mode": options.mode.as_str(),
                "permission_mode": permission_mode.as_str(),
                "approval_mode": approval_mode.as_str(),
                "selected_agent": selected_parent_summary,
            })),
        )?
    };
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let context = AgentToolContext {
        provider,
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        provider_label: resolved.display_label.clone(),
        base_url: resolved.base_url.clone(),
        api_key_env: resolved.api_key_env.clone(),
        reasoning_effort: resolved.reasoning_effort.clone(),
        context_limit: resolved.context_limit,
        generation_metadata: json!({
            "model_metadata": resolved.metadata.public_json(),
            "reasoning_effort": resolved.reasoning_effort.clone(),
        }),
        workdir,
        mode: options.mode,
        permission_config: loaded.config.permissions.clone(),
        permission_mode,
        approval_mode,
        approval_handler: options.approval_handler.clone(),
        store,
        parent_session_id: parent_session_id.clone(),
        parent_context_snapshot: Vec::new(),
        catalog: agent_catalog,
        control_handle: None,
        stream_events: None,
        model_metadata: resolved.metadata,
        env: loaded.env,
        allowed_agent_names: selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
        denied_agent_names: selected_parent_agent
            .as_ref()
            .map(|agent| agent.tool_policy.denied_agents.clone())
            .unwrap_or_default(),
        required_agent_names: Vec::new(),
        spawn_depth_remaining: None,
    };
    let agent = spawn_child_agent_background(context, child_agent, options.prompt)?;
    Ok(AgentSpawnResult {
        parent_session_id,
        agent,
    })
}

async fn run_live_internal(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream_events: Option<RunStreamSink>,
    control: Option<RunControl>,
) -> Result<RunResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.prompt.trim().is_empty() && options.image_inputs.is_empty() {
        return Err(Error::Message("prompt is empty".to_string()));
    }
    let project_instructions = load_project_instructions(&workdir)?;

    if options.no_agents && options.agent.is_some() {
        return Err(Error::Config(
            "--agent cannot be used together with no_agents".to_string(),
        ));
    }
    let loaded = load_run_config(&options, &workdir)?;
    let permission_mode = options
        .permission_mode
        .or(loaded.config.permissions.permission_mode)
        .unwrap_or_default();
    let approval_mode = options
        .approval_mode
        .or(loaded.config.permissions.approval_mode)
        .unwrap_or_default();
    let agents_home = resolve_agents_home(&loaded.env, &workdir)?;
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        workdir: workdir.clone(),
        env: loaded.env.clone(),
        explicit_inputs: options.agent.iter().cloned().collect(),
        no_agents: options.no_agents,
    })?;
    let selected_agent = match &options.agent {
        Some(input) => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &workdir,
            &loaded.env,
        )?),
        None => None,
    };
    let permission_mode =
        narrow_permission_mode_for_agent(permission_mode, selected_agent.as_ref());
    let mut resolved_options = options.clone();
    if resolved_options.model.is_none()
        && let Some(model) = selected_agent
            .as_ref()
            .and_then(|agent| agent.model.clone())
    {
        resolved_options.model = Some(model);
    }
    if resolved_options.reasoning_effort.is_none()
        && let Some(effort) = selected_agent
            .as_ref()
            .and_then(|agent| agent.effort.clone())
    {
        resolved_options.reasoning_effort = Some(effort);
    }
    let resolved = resolve_run_provider(&resolved_options, &loaded)?;
    let skills_home = resolve_skills_home(&loaded.env, &workdir)?;
    let mut explicit_skill_inputs = options.skill_inputs.clone();
    if let Some(agent) = &selected_agent {
        explicit_skill_inputs.extend(agent.skills.clone());
    }
    let skill_options = SkillDiscoveryOptions {
        home: skills_home.clone(),
        workdir: workdir.clone(),
        config_path: options.config_path.clone(),
        env: loaded.env.clone(),
        explicit_inputs: explicit_skill_inputs.clone(),
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let selected_skills = selected_skills_for_run(
        &skill_catalog,
        &options.prompt,
        &explicit_skill_inputs,
        &workdir,
        &loaded.env,
    );
    let selected_agent_summary = selected_agent_for_result(selected_agent.as_ref());
    let required_agent_catalog = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_selected_policy(&agent_catalog.agents, selected_agent.as_ref())
    };
    let required_agent_mentions = required_agent_mentions(&options.prompt, &required_agent_catalog);
    let skill_context_fragments = skill_context_fragments(&selected_skills, &skill_catalog)?;
    let selected_skill_context_message_count = skill_context_fragments.len();
    let store = SqliteStore::open(&options.db_path)?;
    let (session_id, created_session) = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        (session_id, false)
    } else if options.continue_latest {
        if let Some(session_id) =
            store.latest_session_for_workdir_with_sources(&workdir, continue_sources)?
        {
            store.resume_session(&session_id)?;
            (session_id, false)
        } else {
            (
                store.create_session_with_metadata(
                    &workdir,
                    source,
                    &resolved.model,
                    &resolved.provider,
                    Some(json!({
                        "provider_label": resolved.display_label.clone(),
                        "base_url": resolved.base_url.clone(),
                        "api_key_env": resolved.api_key_env.clone(),
                        "reasoning_effort": resolved.reasoning_effort.clone(),
                        "context_limit": resolved.context_limit,
                        "model_metadata": resolved.metadata.public_json(),
                        "mode": options.mode.as_str(),
                        "permission_mode": permission_mode.as_str(),
                        "approval_mode": approval_mode.as_str(),
                        "selected_agent": selected_agent_summary.clone(),
                    })),
                )?,
                true,
            )
        }
    } else {
        (
            store.create_session_with_metadata(
                &workdir,
                source,
                &resolved.model,
                &resolved.provider,
                Some(json!({
                    "provider_label": resolved.display_label.clone(),
                    "base_url": resolved.base_url.clone(),
                    "api_key_env": resolved.api_key_env.clone(),
                    "reasoning_effort": resolved.reasoning_effort.clone(),
                    "context_limit": resolved.context_limit,
                "model_metadata": resolved.metadata.public_json(),
                "mode": options.mode.as_str(),
                "permission_mode": permission_mode.as_str(),
                "approval_mode": approval_mode.as_str(),
                "selected_agent": selected_agent_summary.clone(),
                })),
            )?,
            true,
        )
    };

    store.cleanup_reverted_messages(&session_id)?;
    let prompt_snapshot = options.snapshot_root.as_ref().and_then(|root| {
        SnapshotStore::new(root.clone(), session_id.clone(), workdir.clone())
            .track()
            .ok()
            .flatten()
    });

    let run_start = json!({
        "type": "run_start",
        "source": source,
        "session_id": session_id.clone(),
        "provider": resolved.provider.clone(),
        "model": resolved.model.clone(),
        "db": options.db_path.clone(),
        "workdir": workdir.clone(),
        "base_url": resolved.base_url.clone(),
        "api_key_env": resolved.api_key_env.clone(),
        "reasoning_effort": resolved.reasoning_effort.clone(),
        "context_limit": resolved.context_limit,
        "model_metadata": resolved.metadata.public_json(),
        "mode": options.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": approval_mode.as_str(),
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "agent_count": agent_catalog.agents.len(),
        "selected_skills": selected_skills.clone(),
    });
    if let Some(stream) = &stream_events {
        stream(RunStreamEvent::Event(run_start.clone()));
    }
    let events = Arc::new(Mutex::new(vec![run_start]));
    emit_warning_events(
        &project_instructions.warnings,
        &events,
        stream_events.as_ref(),
    );

    let previous_messages = prune_context(
        store.load_messages(&session_id)?,
        options.max_context_messages,
    );
    let prompt_session_seq = store.next_message_seq(&session_id)?;
    let mailbox_context_messages = store
        .deliver_pending_agent_mailbox_events_for_prompt(&session_id, prompt_session_seq)?
        .into_iter()
        .filter(|record| record.delivered_at_ms.is_some())
        .map(|record| agent_mailbox_event_message(&record))
        .collect::<Vec<_>>();
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let context_recorder = ContextRecorder::default();
    let provider_for_title = Arc::clone(&provider);
    let provider: Arc<dyn GenerationProvider> = Arc::new(ContextRecordingProvider::new(
        Arc::clone(&provider),
        context_recorder.clone(),
        LiveContextProfile {
            session_id: session_id.clone(),
            base_url: resolved.base_url.clone(),
            context_limit: resolved.context_limit,
            mode: options.mode,
        },
    ));
    let (control_handle, control_receivers) = match control {
        Some(control) => (control.handle.inner.clone(), control.receivers),
        None => ControlHandle::new(),
    };
    let stream_events_after = stream_events.clone();
    let mut generation_metadata = json!({
        "model_metadata": resolved.metadata.public_json(),
    });
    if let Some(effort) = &resolved.reasoning_effort
        && let Some(object) = generation_metadata.as_object_mut()
    {
        object.insert(
            "reasoning_effort".to_string(),
            serde_json::Value::String(effort.clone()),
        );
    }
    let mut tools = coding_core_tools_for_mode(&workdir, options.mode);
    if !options.no_skills || !explicit_skill_inputs.is_empty() {
        tools.extend(skill_tools_for_mode(skill_options, options.mode));
    }
    if !options.no_agents {
        tools.extend(agent_tools(AgentToolContext {
            provider: Arc::clone(&provider),
            model_provider: resolved.provider.clone(),
            model: resolved.model.clone(),
            provider_label: resolved.display_label.clone(),
            base_url: resolved.base_url.clone(),
            api_key_env: resolved.api_key_env.clone(),
            reasoning_effort: resolved.reasoning_effort.clone(),
            context_limit: resolved.context_limit,
            generation_metadata: generation_metadata.clone(),
            workdir: workdir.clone(),
            mode: options.mode,
            permission_config: loaded.config.permissions.clone(),
            permission_mode,
            approval_mode,
            approval_handler: options.approval_handler.clone(),
            store: store.clone(),
            parent_session_id: session_id.clone(),
            parent_context_snapshot: previous_messages.clone(),
            catalog: agent_catalog.clone(),
            control_handle: Some(control_handle.clone()),
            stream_events: stream_events.clone(),
            model_metadata: resolved.metadata.clone(),
            env: loaded.env.clone(),
            allowed_agent_names: selected_agent
                .as_ref()
                .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
            denied_agent_names: selected_agent
                .as_ref()
                .map(|agent| agent.tool_policy.denied_agents.clone())
                .unwrap_or_default(),
            required_agent_names: required_agent_mentions.clone(),
            spawn_depth_remaining: None,
        }));
    }
    tools = apply_agent_tool_policy(tools, selected_agent.as_ref(), options.mode);
    tools = apply_agent_hooks(tools, selected_agent.as_ref(), &workdir);
    let permission_runtime = PermissionRuntime::new(
        workdir.clone(),
        workdir.join(".psychevo"),
        loaded.config.permissions.clone(),
        permission_mode,
        approval_mode,
        options.approval_handler.clone(),
    );
    tools = permission_runtime.wrap_tools(tools);
    let effective_tool_names = effective_tool_names(&tools);
    let prompt_agents = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_prompt(&agent_catalog.agents, selected_agent.as_ref(), &tools)
    };
    let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
        skill_catalog.skills.clone()
    } else {
        Vec::new()
    };
    let prompt_project_instructions = if agent_project_instructions_enabled(selected_agent.as_ref())
    {
        project_instructions.fragments.as_slice()
    } else {
        &[]
    };
    let project_instructions_role = (!prompt_project_instructions.is_empty())
        .then(|| developer_provider_role(&resolved.metadata.capabilities).to_string());
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let prefix_metadata = json!({
        "mode": options.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": approval_mode.as_str(),
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "effective_tools": effective_tool_names,
        "agent_catalog_visible": !prompt_agents.is_empty(),
        "visible_agents": prompt_agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
        "skill_catalog_visible": !prompt_skills.is_empty(),
        "project_instructions_visible": !prompt_project_instructions.is_empty(),
        "project_instructions_role": project_instructions_role,
    });
    let stored_prefix = store.load_session_prompt_prefix(&session_id)?;
    let invalidation_reason = stored_prefix.as_ref().and_then(|record| {
        prompt_prefix_invalidation_reason(
            record,
            &resolved.provider,
            &resolved.model,
            options.mode,
            selected_agent_summary.as_ref(),
            &tool_declarations_hash,
            &prefix_metadata,
        )
    });
    let needs_prefix_rebuild =
        created_session || stored_prefix.is_none() || invalidation_reason.is_some();
    let (prompt_assembly, prompt_prefix_record) = if needs_prefix_rebuild {
        let assembly = assemble_main_prompt_prefix(
            options.mode,
            selected_agent.as_ref(),
            &prompt_agents,
            &prompt_skills,
            prompt_project_instructions,
            &resolved.metadata.capabilities,
            !tools.is_empty(),
        );
        let reason = if created_session {
            "new_session".to_string()
        } else {
            invalidation_reason.unwrap_or_else(|| "lazy_create".to_string())
        };
        let record = prompt_prefix_record(PromptPrefixRecordInput {
            session_id: &session_id,
            provider: &resolved.provider,
            model: &resolved.model,
            prefix_hash: assembly.prefix_hash.clone(),
            tool_declarations_hash: tool_declarations_hash.clone(),
            invalidation_reason: Some(reason),
            slots: assembly.prefix_slots.clone(),
            metadata: Some(prefix_metadata.clone()),
        });
        let record = store.upsert_session_prompt_prefix(record)?;
        (assembly, record)
    } else {
        let record = stored_prefix.expect("checked above");
        (assembly_from_prefix_record(&record), record)
    };
    let prefix_notice = take_prompt_prefix_notice(&store, &session_id)?;
    let mut turn_prompt_instructions = Vec::new();
    if let Some(notice) = prefix_notice.as_deref()
        && let Some(instruction) =
            turn_prefix_notice_instruction(notice, &resolved.metadata.capabilities, 0)
    {
        turn_prompt_instructions.push(instruction);
    }
    if let Some(instruction) = turn_required_agent_instruction(
        &required_agent_mentions,
        &resolved.metadata.capabilities,
        turn_prompt_instructions.len(),
    ) {
        turn_prompt_instructions.push(instruction);
    }
    let turn_contextual_user_messages = skill_contextual_user_messages(&skill_context_fragments);
    let prompt_context_evidence = context_evidence_for_request(
        &prompt_assembly.prompt_instructions,
        &turn_prompt_instructions,
        &prompt_assembly.prefix_contextual_user_messages,
        &skill_context_fragments,
    );
    let prompt_prefix_metadata = json!({
        "hash": prompt_prefix_record.prefix_hash,
        "version": prompt_prefix_record.version,
        "created_at_ms": prompt_prefix_record.created_at_ms,
        "provider": prompt_prefix_record.provider,
        "model": prompt_prefix_record.model,
        "tool_declarations_hash": prompt_prefix_record.tool_declarations_hash,
        "invalidation_reason": prompt_prefix_record.invalidation_reason,
        "effective_tools": prefix_metadata.get("effective_tools").cloned().unwrap_or_default(),
        "agent_catalog_visible": prefix_metadata.get("agent_catalog_visible").cloned().unwrap_or_default(),
        "visible_agents": prefix_metadata.get("visible_agents").cloned().unwrap_or_default(),
        "skill_catalog_visible": prefix_metadata.get("skill_catalog_visible").cloned().unwrap_or_default(),
        "project_instructions_visible": prefix_metadata.get("project_instructions_visible").cloned().unwrap_or_default(),
        "project_instructions_role": prefix_metadata.get("project_instructions_role").cloned().unwrap_or_default(),
    });
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(prompt_context_evidence),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: Some(control_handle.clone()),
        events: Some(Arc::clone(&events)),
        stream_events: stream_events.clone(),
        include_reasoning: options.include_reasoning,
        reasoning_effort: resolved.reasoning_effort.clone(),
        model_metadata: resolved.metadata.clone(),
        context_recorder: Some(context_recorder.clone()),
        prompt_display: options.prompt_display.clone(),
        selected_agent: selected_agent_summary.clone(),
        prompt_prefix_metadata: Some(prompt_prefix_metadata.clone()),
    });
    if let Some(object) = generation_metadata.as_object_mut() {
        object.insert("prompt_prefix".to_string(), prompt_prefix_metadata);
        object.insert(
            "context_counting".to_string(),
            context_counting_metadata(
                &prompt_assembly.prompt_instructions,
                turn_prompt_instructions.len(),
                previous_messages.len(),
                prompt_assembly.prefix_contextual_user_messages.len(),
                selected_skill_context_message_count,
                prompt_skills
                    .iter()
                    .map(|skill| skill.name.clone())
                    .collect(),
            ),
        );
    }
    let request = AgentLoopRequest {
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        generation_metadata,
        prompt_instructions: prompt_assembly.prompt_instructions,
        turn_prompt_instructions,
        previous_messages,
        context_messages: mailbox_context_messages,
        prefix_contextual_user_messages: prompt_assembly.prefix_contextual_user_messages,
        turn_contextual_user_messages,
        prompt_messages: vec![
            prompt_message_from_inputs_with_options(
                &options.prompt,
                &options.image_inputs,
                &workdir,
                &resolved.metadata,
                options.extract_prompt_image_sources,
            )?
            .message,
        ],
        tools,
        max_turns: DEFAULT_AGENT_MAX_TURNS,
    };
    let completion =
        run_agent_loop(Arc::clone(&provider), request, sink, control_receivers).await?;
    record_missed_required_agents(
        &store,
        &session_id,
        &completion.messages,
        &required_agent_mentions,
    )?;
    run_agent_hook_event(
        selected_agent.as_ref(),
        "Stop",
        &workdir,
        json!({ "outcome": completion.outcome.as_str() }),
    );
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();
    if created_session && source == "tui" && completion.outcome == Outcome::Normal {
        ensure_new_tui_session_title(
            &store,
            &session_id,
            &options.prompt,
            &selected_skills,
            &skill_catalog,
            provider_for_title,
            &resolved,
        )
        .await?;
    }

    tokio::task::yield_now().await;
    let mut events = events.lock().expect("event lock poisoned").clone();
    let context_snapshot = context_recorder.latest_snapshot();
    if let Some(snapshot) = &context_snapshot {
        let value = serde_json::to_value(snapshot)?;
        events.push(value.clone());
        if let Some(stream) = stream_events_after {
            stream(RunStreamEvent::Event(value));
        }
    }
    Ok(RunResult {
        session_id,
        outcome: completion.outcome,
        terminal_reason: completion.terminal_reason,
        final_answer,
        db_path: options.db_path,
        workdir,
        provider: resolved.provider,
        model: resolved.model,
        base_url: resolved.base_url,
        api_key_env: resolved.api_key_env,
        reasoning_effort: resolved.reasoning_effort,
        context_limit: resolved.context_limit,
        tool_failures,
        selected_agent: selected_agent_summary,
        selected_skills,
        context_snapshot,
        events,
        warnings: project_instructions.warnings,
    })
}

fn selected_skills_for_run(
    catalog: &crate::skills::SkillCatalog,
    prompt: &str,
    explicit_inputs: &[String],
    workdir: &std::path::Path,
    env: &BTreeMap<String, String>,
) -> Vec<SelectedSkill> {
    let mut selected = select_explicit_skills(catalog, explicit_inputs, workdir, env);
    selected.extend(select_skills_for_prompt(catalog, prompt));
    let mut seen = std::collections::BTreeSet::new();
    selected
        .into_iter()
        .filter(|skill| seen.insert(skill.path.clone()))
        .collect()
}

fn selected_agent_for_result(agent: Option<&AgentDefinition>) -> Option<SelectedAgent> {
    agent.map(|agent| SelectedAgent {
        name: agent.name.clone(),
        source: agent.source.as_str().to_string(),
        path: agent.file_path.clone(),
    })
}

fn session_model_metadata(metadata: &serde_json::Value) -> ModelMetadata {
    metadata
        .get("model_metadata")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn session_agent_input_from_metadata(metadata: &serde_json::Value) -> Option<String> {
    if let Some(main_agent) = metadata.get("main_agent") {
        if main_agent
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|mode| mode == "default")
            || main_agent.is_null()
        {
            return None;
        }
        if let Some(input) = main_agent
            .get("input")
            .and_then(serde_json::Value::as_str)
            .or_else(|| main_agent.get("name").and_then(serde_json::Value::as_str))
            .or_else(|| main_agent.get("path").and_then(serde_json::Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(input.to_string());
        }
    }
    metadata
        .get("selected_agent")
        .and_then(|value| {
            value
                .get("input")
                .or_else(|| value.get("name"))
                .or_else(|| value.get("path"))
        })
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn prompt_prefix_invalidation_reason(
    record: &PromptPrefixRecord,
    provider: &str,
    model: &str,
    mode: crate::types::RunMode,
    selected_agent: Option<&SelectedAgent>,
    tool_declarations_hash: &str,
    expected_metadata: &serde_json::Value,
) -> Option<String> {
    if record.provider != provider
        || record.model != model
        || record.tool_declarations_hash != tool_declarations_hash
    {
        return Some("runtime_context_changed".to_string());
    }
    let Some(metadata) = record.metadata.as_ref() else {
        return Some("prefix_metadata_missing".to_string());
    };
    if metadata.get("mode").and_then(serde_json::Value::as_str) != Some(mode.as_str()) {
        return Some("runtime_context_changed".to_string());
    }
    let expected_agent = serde_json::to_value(selected_agent).unwrap_or(serde_json::Value::Null);
    if metadata
        .get("selected_agent")
        .unwrap_or(&serde_json::Value::Null)
        != &expected_agent
    {
        return Some("main_agent_changed".to_string());
    }
    for key in [
        "effective_tools",
        "agent_catalog_visible",
        "visible_agents",
        "skill_catalog_visible",
        "project_instructions_visible",
        "project_instructions_role",
    ] {
        if metadata.get(key).unwrap_or(&serde_json::Value::Null)
            != expected_metadata
                .get(key)
                .unwrap_or(&serde_json::Value::Null)
        {
            return Some("runtime_context_changed".to_string());
        }
    }
    None
}

fn take_prompt_prefix_notice(store: &SqliteStore, session_id: &str) -> Result<Option<String>> {
    let notice = store
        .session_metadata(session_id)?
        .and_then(|metadata| metadata.get(PROMPT_PREFIX_NOTICE_METADATA_KEY).cloned())
        .and_then(|value| value.as_str().map(str::to_string));
    if notice.is_some() {
        store.set_session_metadata_field(session_id, PROMPT_PREFIX_NOTICE_METADATA_KEY, None)?;
    }
    Ok(notice)
}

fn required_agent_mentions(prompt: &str, agents: &[AgentDefinition]) -> Vec<String> {
    let known = agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut found = std::collections::BTreeSet::new();
    for raw in prompt.split_whitespace() {
        let Some(rest) = raw.strip_prefix('@') else {
            continue;
        };
        let name = rest.trim_matches(|ch: char| {
            !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        });
        if known.contains(name) {
            found.insert(name.to_string());
        }
    }
    found.into_iter().collect()
}

fn record_missed_required_agents(
    store: &SqliteStore,
    session_id: &str,
    messages: &[Message],
    required: &[String],
) -> Result<()> {
    if required.is_empty() {
        return Ok(());
    }
    let called = called_agent_names(messages, required);
    let missed = required
        .iter()
        .filter(|name| !called.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if missed.is_empty() {
        return Ok(());
    }
    let text = format!(
        "Required agent delegation was not performed: {}",
        missed.join(", ")
    );
    store.append_message_with_metrics(
        session_id,
        &user_text_message(text),
        None,
        Some(json!({
            "agent_notification": {
                "type": "missing_required_agent_call",
                "agents": missed,
                "hidden": true
            }
        })),
    )
}

fn called_agent_names(
    messages: &[Message],
    required: &[String],
) -> std::collections::BTreeSet<String> {
    let mut called = std::collections::BTreeSet::new();
    for message in messages {
        let Message::Assistant { content, .. } = message else {
            continue;
        };
        for block in content {
            let AssistantBlock::ToolCall(call) = block else {
                continue;
            };
            if call.name != "Agent" {
                continue;
            }
            let agent_type = call
                .arguments
                .get("agent_type")
                .or_else(|| call.arguments.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or(match required {
                    [single] => Some(single.as_str()),
                    _ => Some("general"),
                })
                .unwrap_or("general");
            called.insert(agent_type.to_string());
        }
    }
    called
}

fn emit_warning_events(
    warnings: &[RunWarning],
    events: &Arc<Mutex<Vec<serde_json::Value>>>,
    stream_events: Option<&RunStreamSink>,
) {
    for warning in warnings {
        let value = warning_event(warning);
        events
            .lock()
            .expect("event lock poisoned")
            .push(value.clone());
        if let Some(stream) = stream_events {
            stream(RunStreamEvent::Event(value));
        }
    }
}

fn warning_event(warning: &RunWarning) -> serde_json::Value {
    let mut value = json!({
        "type": "warning",
        "kind": warning.kind.clone(),
        "message": warning.message.clone(),
    });
    if let Some(object) = value.as_object_mut() {
        if let Some(path) = &warning.source_path {
            object.insert(
                "source_path".to_string(),
                serde_json::Value::String(path.display().to_string()),
            );
        }
        if let Some(suggestion) = &warning.suggestion {
            object.insert(
                "suggestion".to_string(),
                serde_json::Value::String(suggestion.clone()),
            );
        }
    }
    value
}

pub(crate) async fn ensure_new_tui_session_title(
    store: &SqliteStore,
    session_id: &str,
    prompt: &str,
    selected_skills: &[SelectedSkill],
    skill_catalog: &SkillCatalog,
    provider: Arc<dyn GenerationProvider>,
    resolved: &ResolvedRunProvider,
) -> Result<()> {
    if store
        .session_summary(session_id)?
        .and_then(|summary| summary.title)
        .and_then(|title| normalize_session_title(&title))
        .is_some()
    {
        return Ok(());
    }

    let generated = time::timeout(
        Duration::from_secs(TITLE_GENERATION_TIMEOUT_SECS),
        generate_session_title(provider, resolved, prompt, selected_skills, skill_catalog),
    )
    .await
    .ok()
    .and_then(|result| result.ok())
    .flatten();
    let title = generated.unwrap_or_else(|| fallback_session_title(prompt, selected_skills));
    store.set_session_title(session_id, &title)?;
    Ok(())
}

async fn generate_session_title(
    provider: Arc<dyn GenerationProvider>,
    resolved: &ResolvedRunProvider,
    prompt: &str,
    selected_skills: &[SelectedSkill],
    skill_catalog: &SkillCatalog,
) -> Result<Option<String>> {
    let (_control_handle, control) = ControlHandle::new();
    let request = AgentLoopRequest {
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        generation_metadata: json!({}),
        prompt_instructions: vec![PromptInstruction::inline_system(
            "session_title_instruction",
            0,
            "Generate a concise title for this coding-agent session. Return only the title, no punctuation wrapper, no explanation. Keep it under 8 words.",
        )],
        turn_prompt_instructions: Vec::new(),
        previous_messages: Vec::new(),
        context_messages: Vec::new(),
        prefix_contextual_user_messages: Vec::new(),
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message(session_title_request(
            prompt,
            selected_skills,
            skill_catalog,
        ))],
        tools: Vec::new(),
        max_turns: 1,
    };
    let completion = run_agent_loop(provider, request, Arc::new(NoopEventSink), control).await?;
    Ok(completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .as_deref()
        .and_then(clean_generated_session_title))
}

pub(crate) fn session_title_request(
    prompt: &str,
    selected_skills: &[SelectedSkill],
    skill_catalog: &SkillCatalog,
) -> String {
    let mut text = String::from("Title this user request.");
    let skill_lines = selected_skill_title_lines(selected_skills, skill_catalog);
    if !skill_lines.is_empty() {
        text.push_str(
            "\n\nThe request includes explicit selected skills. Use their names and descriptions to infer the task; do not title the literal `$skill-name` marker.",
        );
        text.push_str("\n\nSelected skills:\n");
        text.push_str(&skill_lines.join("\n"));
    }
    text.push_str("\n\nUser request:\n");
    text.push_str(prompt);
    text
}

fn selected_skill_title_lines(
    selected_skills: &[SelectedSkill],
    skill_catalog: &SkillCatalog,
) -> Vec<String> {
    selected_skills
        .iter()
        .map(|selected| {
            let description = skill_catalog
                .skills
                .iter()
                .find(|skill| skill.name == selected.name && skill.file_path == selected.path)
                .map(|skill| skill.description.trim())
                .filter(|description| !description.is_empty());
            match description {
                Some(description) => format!("- {}: {}", selected.name, description),
                None => format!("- {}", selected.name),
            }
        })
        .collect()
}

pub(crate) fn fallback_session_title(prompt: &str, selected_skills: &[SelectedSkill]) -> String {
    let without_markers = prompt_without_selected_skill_markers(prompt, selected_skills);
    normalize_session_title(&without_markers)
        .or_else(|| selected_skills_fallback_title(selected_skills))
        .or_else(|| normalize_session_title(prompt))
        .unwrap_or_else(|| "New session".to_string())
}

fn prompt_without_selected_skill_markers(
    prompt: &str,
    selected_skills: &[SelectedSkill],
) -> String {
    let selected_names = selected_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    prompt
        .split_whitespace()
        .filter(|token| {
            token
                .strip_prefix('$')
                .is_none_or(|name| !selected_names.contains(name))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn selected_skills_fallback_title(selected_skills: &[SelectedSkill]) -> Option<String> {
    let title = selected_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    normalize_session_title(&title)
}

fn clean_generated_session_title(text: &str) -> Option<String> {
    let without_think = remove_think_blocks(text);
    without_think
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(strip_wrapping_title_quotes)
        .and_then(normalize_session_title)
}

fn remove_think_blocks(text: &str) -> String {
    let mut out = text.to_string();
    loop {
        let lower = out.to_lowercase();
        let Some(start) = lower.find("<think>") else {
            break;
        };
        let Some(end_rel) = lower[start + "<think>".len()..].find("</think>") else {
            break;
        };
        let end = start + "<think>".len() + end_rel + "</think>".len();
        out.replace_range(start..end, "");
    }
    out
}

fn strip_wrapping_title_quotes(text: &str) -> &str {
    let trimmed = text.trim();
    for quote in ['"', '\'', '`'] {
        if trimmed.starts_with(quote) && trimmed.ends_with(quote) && trimmed.len() >= 2 {
            return trimmed
                .strip_prefix(quote)
                .and_then(|value| value.strip_suffix(quote))
                .unwrap_or(trimmed)
                .trim();
        }
    }
    trimmed
}

pub(crate) fn normalize_session_title(title: &str) -> Option<String> {
    let collapsed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(truncate_chars(&collapsed, SESSION_TITLE_MAX_CHARS))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}
