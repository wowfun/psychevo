use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use psychevo_agent_core::{
    AgentLoopRequest, ControlHandle, Message, NoopEventSink, run_agent_loop, user_text_message,
};
use psychevo_ai::{GenerationProvider, OpenAiChatProvider, Outcome};
use serde_json::json;
use tokio::time;

use crate::config::{ResolvedRunProvider, load_run_config, resolve_run_provider};
use crate::context::prune_context;
use crate::error::{Error, Result};
use crate::events::PersistenceSink;
use crate::messages::assistant_text;
use crate::paths::canonical_workdir;
use crate::skills::{
    SelectedSkill, SkillCatalog, SkillDiscoveryOptions, discover_skills, format_skills_for_prompt,
    resolve_skills_home, select_explicit_skills, select_skills_for_prompt, skill_context_messages,
};
use crate::snapshot::SnapshotStore;
use crate::store::SqliteStore;
use crate::tools::{coding_core_tools_for_mode, mode_instruction, skill_tools_for_mode};
use crate::types::{
    RunControl, RunOptions, RunResult, RunStreamEvent, RunStreamSink, SmokeControl,
};

const TITLE_GENERATION_TIMEOUT_SECS: u64 = 15;
const DEFAULT_AGENT_MAX_TURNS: usize = 32;
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

async fn run_live_internal(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream_events: Option<RunStreamSink>,
    control: Option<RunControl>,
) -> Result<RunResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.prompt.trim().is_empty() {
        return Err(Error::Message("prompt is empty".to_string()));
    }

    let loaded = load_run_config(&options, &workdir)?;
    let resolved = resolve_run_provider(&options, &loaded)?;
    let skills_home = resolve_skills_home(&loaded.env, &workdir)?;
    let skill_options = SkillDiscoveryOptions {
        home: skills_home.clone(),
        workdir: workdir.clone(),
        config_path: options.config_path.clone(),
        env: loaded.env.clone(),
        explicit_inputs: options.skill_inputs.clone(),
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let selected_skills = selected_skills_for_run(
        &skill_catalog,
        &options.prompt,
        &options.skill_inputs,
        &workdir,
        &loaded.env,
    );
    let skill_context_messages = skill_context_messages(&selected_skills, &skill_catalog)?
        .into_iter()
        .map(user_text_message)
        .collect::<Vec<_>>();
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
                        "mode": options.mode.as_str(),
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
                    "mode": options.mode.as_str(),
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
        "mode": options.mode.as_str(),
        "selected_skills": selected_skills.clone(),
    });
    if let Some(stream) = &stream_events {
        stream(RunStreamEvent::Event(run_start.clone()));
    }
    let events = Arc::new(Mutex::new(vec![run_start]));

    let previous_messages = prune_context(
        store.load_messages(&session_id)?,
        options.max_context_messages,
    );
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let (control_handle, control_receivers) = match control {
        Some(control) => (control.handle.inner.clone(), control.receivers),
        None => ControlHandle::new(),
    };
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: Some(control_handle),
        events: Some(Arc::clone(&events)),
        stream_events,
        include_reasoning: options.include_reasoning,
        reasoning_effort: resolved.reasoning_effort.clone(),
    });
    let generation_metadata = resolved
        .reasoning_effort
        .as_ref()
        .map(|effort| json!({ "reasoning_effort": effort }))
        .unwrap_or_else(|| json!({}));
    let mut system_instructions = vec![mode_instruction(options.mode).to_string()];
    let skills_prompt = format_skills_for_prompt(&skill_catalog.skills);
    if !skills_prompt.trim().is_empty() {
        system_instructions.push(skills_prompt);
    }
    let mut tools = coding_core_tools_for_mode(&workdir, options.mode);
    if !options.no_skills || !options.skill_inputs.is_empty() {
        tools.extend(skill_tools_for_mode(skill_options, options.mode));
    }
    let request = AgentLoopRequest {
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        generation_metadata,
        system_instructions,
        previous_messages,
        context_messages: skill_context_messages,
        prompt_messages: vec![user_text_message(options.prompt.clone())],
        tools,
        max_turns: DEFAULT_AGENT_MAX_TURNS,
    };
    let completion =
        run_agent_loop(Arc::clone(&provider), request, sink, control_receivers).await?;
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
            provider,
            &resolved,
        )
        .await?;
    }

    let events = events.lock().expect("event lock poisoned").clone();
    Ok(RunResult {
        session_id,
        outcome: completion.outcome,
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
        selected_skills,
        events,
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
        system_instructions: vec![
            "Generate a concise title for this coding-agent session. Return only the title, no punctuation wrapper, no explanation. Keep it under 8 words.".to_string(),
        ],
        previous_messages: Vec::new(),
        context_messages: Vec::new(),
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
