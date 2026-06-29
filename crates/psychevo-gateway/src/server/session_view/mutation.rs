fn guard_session_mutation(
    state: &WebState,
    auth: &AuthContext,
    session_id: &str,
    allow_current_idle: bool,
) -> psychevo_runtime::Result<()> {
    let scope = default_resolved_scope(state, auth)?;
    let activity = state.activity(&scope.source, Some(session_id));
    if activity.running {
        return Err(Error::Message(
            "running session cannot be archived, restored, or deleted".to_string(),
        ));
    }
    if !allow_current_idle
        && let Some(bound) = state.inner.gateway.resolve_source_thread(&scope.source)?
        && bound == session_id
    {
        return Err(Error::Message(
            "current bound session cannot be deleted; reset the source first".to_string(),
        ));
    }
    Ok(())
}

fn session_summary_by_id(state: &WebState, session_id: &str) -> psychevo_runtime::Result<Value> {
    state
        .inner
        .state
        .store()
        .session_summary(session_id)?
        .map(|summary| session_summary_value(state, summary))
        .transpose()?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))
}
