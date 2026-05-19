fn increment_row_index(value: &mut Option<usize>, inserted_at: usize) {
    if let Some(index) = value
        && *index >= inserted_at
    {
        *index += 1;
    }
}

fn decrement_row_index(value: &mut Option<usize>, removed_at: usize) {
    if let Some(index) = value {
        if *index == removed_at {
            *value = None;
        } else if *index > removed_at {
            *index -= 1;
        }
    }
}

fn format_configured_model(model: &ConfiguredModel) -> String {
    let mut parts = vec![format!("{}/{}", model.provider, model.model)];
    if let Some(variant) = &model.reasoning_effort {
        parts.push(format!("variant={variant}"));
    }
    if let Some(limit) = model.context_limit {
        parts.push(format!("context={limit}"));
    }
    parts.join(" ")
}

fn format_model_spec(model: &ConfiguredModel) -> String {
    format!("{}/{}", model.provider, model.model)
}

fn variant_description(variant: &str) -> &'static str {
    match variant {
        "none" => "suppress provider reasoning field",
        "minimal" => "smallest reasoning request",
        "low" => "lighter reasoning",
        "medium" => "balanced reasoning",
        "high" => "deeper reasoning",
        "xhigh" => "extra high reasoning",
        "max" => "maximum configured reasoning",
        _ => "custom reasoning setting",
    }
}

#[cfg(test)]
fn resolve_session_ref_from_summaries(
    sessions: &[SessionSummary],
    reference: &str,
) -> Result<String> {
    if reference == "latest" {
        return sessions
            .first()
            .map(|session| session.id.clone())
            .ok_or_else(|| anyhow!("no latest session for this workdir"));
    }
    if let Some(session) = sessions.iter().find(|session| session.id == reference) {
        return Ok(session.id.clone());
    }
    let matches = sessions
        .iter()
        .filter(|session| session.id.starts_with(reference))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [session] => Ok(session.id.clone()),
        [] => Err(anyhow!("session not found: {reference}")),
        _ => Err(anyhow!("ambiguous session prefix: {reference}")),
    }
}
