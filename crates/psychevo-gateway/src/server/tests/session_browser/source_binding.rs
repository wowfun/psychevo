#[test]
fn bind_source_to_thread_keeps_previous_history_active() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let first = state
        .inner
        .state

        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("first");
    let second = state
        .inner
        .state

        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("second");

    bind_source_to_thread(&state, &scope, &first).expect("bind first");
    bind_source_to_thread(&state, &scope, &second).expect("bind second");

    assert!(
        state
            .inner
            .state

            .session_summary(&first)
            .expect("first summary")
            .expect("first exists")
            .archived_at_ms
            .is_none()
    );
}

#[test]
fn active_completion_token_keeps_at_paths_with_slashes() {
    let token = active_completion_token("@src/ma", 7).expect("token");

    assert_eq!(token.sigil, '@');
    assert_eq!(token.query, "src/ma");
    assert_eq!(token.start, 0);
    assert_eq!(token.end, 7);
}
