#[allow(unused_imports)]
use super::*;

pub(crate) fn called_agent_names(
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
            if call.name != "spawn_agent" {
                continue;
            }
            let agent_type = call
                .arguments
                .get("agent_type")
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

pub(crate) fn emit_warning_events(
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
            stream(RunStreamEvent::value(value));
        }
    }
}

pub(crate) fn warning_event(warning: &RunWarning) -> serde_json::Value {
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

pub(crate) async fn ensure_new_visible_session_title(
    store: &SqliteStore,
    session_id: &str,
    prompt: &str,
    selected_skills: &[SelectedSkill],
    skill_catalog: &SkillCatalog,
    provider: Arc<dyn GenerationProvider>,
    resolved: &ResolvedRunProvider,
) -> Result<()> {
    let Some(summary) = store.session_summary(session_id)? else {
        return Ok(());
    };
    if summary.parent_session_id.is_some()
        || !visible_session_source_allows_auto_title(&summary.source)
        || summary
            .title
            .as_deref()
            .and_then(normalize_session_title)
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
    let _ = store.set_session_title_if_empty(session_id, &title)?;
    Ok(())
}

pub(crate) fn visible_session_source_allows_auto_title(source: &str) -> bool {
    matches!(source, "run" | "tui" | "web" | "automation" | "peer_agent")
        || source.starts_with("channel/")
}

pub(crate) async fn generate_session_title(
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
            prompt_templates::session_title_instruction(),
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
        tool_search: psychevo_agent_core::ToolSearchOptions::disabled(),
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
    let skill_lines = selected_skill_title_lines(selected_skills, skill_catalog);
    if skill_lines.is_empty() {
        prompt_templates::session_title_request(prompt)
    } else {
        prompt_templates::session_title_request_with_selected_skills(
            &skill_lines.join("\n"),
            prompt,
        )
    }
}

pub(crate) fn selected_skill_title_lines(
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

pub fn fallback_visible_session_title(prompt: &str) -> String {
    fallback_session_title(prompt, &[])
}

pub(crate) fn prompt_without_selected_skill_markers(
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

pub(crate) fn selected_skills_fallback_title(selected_skills: &[SelectedSkill]) -> Option<String> {
    let title = selected_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    normalize_session_title(&title)
}

pub(crate) fn clean_generated_session_title(text: &str) -> Option<String> {
    let without_think = remove_think_blocks(text);
    without_think
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(strip_wrapping_title_quotes)
        .and_then(normalize_session_title)
}

pub(crate) fn remove_think_blocks(text: &str) -> String {
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

pub(crate) fn strip_wrapping_title_quotes(text: &str) -> &str {
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

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}
