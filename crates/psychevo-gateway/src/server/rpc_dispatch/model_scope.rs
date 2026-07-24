fn resolve_model_state_request_scope(
    state: &WebState,
    auth: &AuthContext,
    cwd: Option<String>,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<(PathBuf, Option<String>)> {
    if let Some(thread_id) = thread_id {
        authorize_thread(state, auth, &thread_id)?;
        let summary = state
            .inner
            .state

            .session_summary(&thread_id)?
            .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
        return Ok((PathBuf::from(summary.cwd), Some(thread_id)));
    }
    Ok((resolve_cwd_filter(state, auth, cwd)?, None))
}
