use crate::server::binding::AuthContext;
use crate::server::completion::active_completion_token;
use crate::server::scope_session::{bind_source_to_thread, default_resolved_scope};
use crate::server::tests::helpers::web_state;

#[tokio::test]
async fn bind_source_to_thread_keeps_previous_history_active() {
    let (_temp, state) = web_state().await;
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let mut first_request = psychevo::StartThreadRequest::new(&state.inner.cwd);
    first_request.source = "web".to_string();
    let first = state
        .inner
        .framework
        .start_thread(first_request)
        .await
        .expect("first")
        .id()
        .to_string();
    let mut second_request = psychevo::StartThreadRequest::new(&state.inner.cwd);
    second_request.source = "web".to_string();
    let second = state
        .inner
        .framework
        .start_thread(second_request)
        .await
        .expect("second")
        .id()
        .to_string();

    bind_source_to_thread(&state, &scope, &first)
        .await
        .expect("bind first");
    bind_source_to_thread(&state, &scope, &second)
        .await
        .expect("bind second");

    assert!(
        state
            .inner
            .framework
            .resume_thread(&first)
            .await
            .expect("first thread")
            .snapshot()
            .await
            .expect("first snapshot")
            .summary
            .archived_at_ms
            .is_none()
    );
}

#[tokio::test]
async fn active_completion_token_keeps_at_paths_with_slashes() {
    let token = active_completion_token("@src/ma", 7).expect("token");

    assert_eq!(token.sigil, '@');
    assert_eq!(token.query, "src/ma");
    assert_eq!(token.start, 0);
    assert_eq!(token.end, 7);
}
