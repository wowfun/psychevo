pub(crate) fn first_use_empty_visible_session(
    store: &StateRuntime,
    session_id: &str,
) -> Result<bool> {
    let Some(summary) = store.session_summary(session_id)? else {
        return Ok(false);
    };
    Ok(summary.message_count == 0
        && summary.parent_session_id.is_none()
        && summary.ended_at_ms.is_none()
        && visible_session_source_allows_auto_title(&summary.source))
}

pub(crate) fn materialize_first_use_empty_session(
    store: &StateRuntime,
    session_id: &str,
    provider: &str,
    model: &str,
    metadata: Value,
) -> Result<bool> {
    if !first_use_empty_visible_session(store, session_id)? {
        return Ok(false);
    }
    store.set_session_model(session_id, provider, model)?;
    store.set_session_metadata(session_id, Some(metadata))?;
    Ok(true)
}

pub(crate) fn should_title_completed_session(
    created_session: bool,
    first_use_empty_visible_session: bool,
    outcome: Outcome,
) -> bool {
    (created_session || first_use_empty_visible_session) && outcome == Outcome::Normal
}

pub(crate) fn selected_skills_for_run(
    catalog: &crate::skills::SkillCatalog,
    prompt: &str,
    explicit_inputs: &[String],
    cwd: &std::path::Path,
    env: &BTreeMap<String, String>,
) -> Vec<SelectedSkill> {
    let mut selected = select_explicit_skills(catalog, explicit_inputs, cwd, env);
    selected.extend(select_skills_for_prompt(catalog, prompt));
    let mut seen = std::collections::BTreeSet::new();
    selected
        .into_iter()
        .filter(|skill| seen.insert(skill.path.clone()))
        .collect()
}

pub(crate) async fn maybe_preflight_compact_session(
    options: &RunOptions,
    cwd: &std::path::Path,
    session_id: &str,
    provider: &str,
    model: &str,
    reasoning_effort: &Option<String>,
    env: &BTreeMap<String, String>,
) -> Result<()> {
    let model_override = options
        .model
        .clone()
        .or_else(|| Some(format!("{provider}/{model}")));
    let result = compact_session(CompactSessionOptions {
        state: options.state.clone(),
        cwd: cwd.to_path_buf(),
        session: session_id.to_string(),
        config_path: options.config_path.clone(),
        model: model_override,
        reasoning_effort: options
            .reasoning_effort
            .clone()
            .or_else(|| reasoning_effort.clone()),
        inherited_env: Some(env.clone()),
        reason: CompactionReason::AutoThreshold,
        instructions: None,
        force: false,
    })
    .await?;
    let _ = result;
    Ok(())
}

pub(crate) fn selected_agent_for_result(agent: Option<&AgentDefinition>) -> Option<SelectedAgent> {
    agent.map(|agent| SelectedAgent {
        name: agent.name.clone(),
        source: agent.source.as_str().to_string(),
        path: agent.file_path.clone(),
    })
}

pub(crate) fn session_model_metadata(metadata: &serde_json::Value) -> ModelMetadata {
    metadata
        .get("model_metadata")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub(crate) fn main_agent_input_from_sources(
    no_agents: bool,
    explicit_agent: Option<&str>,
    session_metadata: Option<&serde_json::Value>,
) -> Option<String> {
    if no_agents {
        return None;
    }
    if let Some(input) = explicit_agent
        .map(str::trim)
        .filter(|input| !input.is_empty())
    {
        return Some(input.to_string());
    }
    if let Some(input) = session_metadata.and_then(session_agent_input_from_metadata) {
        return Some(input);
    }
    None
}
