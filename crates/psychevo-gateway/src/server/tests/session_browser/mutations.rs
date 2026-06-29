#[test]
fn workspace_dir_name_rejects_path_components() {
    assert_eq!(workspace_dir_name(" notes ").expect("trimmed"), "notes");
    let err = workspace_dir_name("../notes").expect_err("parent path rejected");
    assert!(
        err.to_string()
            .contains("workspace name must be a single directory name")
    );
}

#[test]
fn reset_source_to_empty_archives_previous_binding_without_replacement() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let first_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    bind_source_to_thread(&state, &scope, &first_id).expect("bind");

    let snapshot = reset_source_to_empty(&state, &scope).expect("reset");

    assert!(snapshot.get("thread").is_some_and(Value::is_null));
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source lookup")
            .is_none()
    );
    assert!(
        state
            .inner
            .state
            .store()
            .session_summary(&first_id)
            .expect("first summary")
            .expect("first exists")
            .archived_at_ms
            .is_some()
    );
    assert_eq!(
        state
            .inner
            .state
            .store()
            .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
            .expect("active sessions")
            .len(),
        0
    );
}

#[test]
fn bind_source_to_thread_rebinds_existing_session() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");

    bind_source_to_thread(&state, &scope, &session_id).expect("bind");

    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source lookup")
            .as_deref(),
        Some(session_id.as_str())
    );
}
