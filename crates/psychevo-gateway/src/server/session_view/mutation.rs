async fn guard_session_mutation(
    state: &WebState,
    auth: &AuthContext,
    session_id: &str,
) -> psychevo::Result<()> {
    let scope = default_resolved_scope(state, auth)?;
    let activity = state.activity(&scope.source, Some(session_id)).await;
    if activity.running {
        return Err(Error::Message(
            "running session cannot be archived, restored, or deleted".to_string(),
        ));
    }
    Ok(())
}

async fn session_summary_by_id(
    state: &WebState,
    session_id: &str,
) -> psychevo::Result<Value> {
    let projection = state
        .inner
        .state

        .session_list_projection(session_id)
        .await?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
    let mut activity = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(session_id))
        .await;
    if let Ok(thread) = state.inner.framework.resume_thread(session_id).await {
        let (running, active_turn_id, queued_turns) = thread.__activity();
        if running {
            activity.running = true;
        }
        if active_turn_id.is_some() {
            activity.active_turn_id = active_turn_id.clone();
        }
        if let Some(turn_id) = active_turn_id {
            let kind = projection
                .summary
                .parent_session_id
                .as_ref()
                .map_or(wire::FrameworkTurnKind::Root, |_| {
                    wire::FrameworkTurnKind::DelegatedChild
                });
            activity.activities.insert(
                0,
                wire::ThreadActivityView::FrameworkTurn {
                    activity_id: turn_id.clone(),
                    turn_id,
                    kind,
                    queued_turns,
                },
            );
        }
        activity.queued_turns = queued_turns;
    }
    Ok(session_summary_value(projection, activity))
}
