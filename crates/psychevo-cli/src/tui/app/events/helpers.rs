pub(crate) use crate::tui::support_turn_event::{
    turn_event_ends_agent_child_backlog as agent_child_event_ends_live_backlog,
    turn_event_ends_session_backlog as session_live_event_ends_backlog,
    turn_event_is_clarify_request, turn_event_session_id,
};
use crate::tui::{
    AuxiliaryAgentTask, FullscreenUi, GatewayActionKind, GatewayEvent, Outcome, TerminalReason,
    TuiLiveEvent, TurnEvent, TurnOutcome,
};

pub(crate) fn tui_live_event_is_clarify_request(event: &TuiLiveEvent) -> bool {
    match event {
        TuiLiveEvent::Turn(event) => turn_event_is_clarify_request(event),
        TuiLiveEvent::Shell(_) => false,
        TuiLiveEvent::Gateway(event) => matches!(
            event.as_ref(),
            GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action }
                if action.kind == GatewayActionKind::Clarify
        ),
    }
}

pub(crate) fn buffer_session_live_event(
    ui: &mut FullscreenUi<'_>,
    session_id: &str,
    event: impl Into<TuiLiveEvent>,
) {
    let event = event.into();
    if matches!(&event, TuiLiveEvent::Turn(event) if session_live_event_ends_backlog(event))
        || matches!(&event, TuiLiveEvent::Gateway(event) if matches!(event.as_ref(), GatewayEvent::TurnCompleted { .. }))
    {
        ui.session_live_event_backlog.remove(session_id);
        return;
    }
    let backlog = ui
        .session_live_event_backlog
        .entry(session_id.to_string())
        .or_default();
    if let (TuiLiveEvent::Gateway(current), Some(TuiLiveEvent::Gateway(previous))) =
        (&event, backlog.last_mut())
        && let (
            GatewayEvent::EntryBlockTextDelta {
                turn_id,
                entry_id,
                block_id,
                text,
                updated_at_ms,
                ..
            },
            GatewayEvent::EntryBlockTextDelta {
                turn_id: previous_turn_id,
                entry_id: previous_entry_id,
                block_id: previous_block_id,
                text: previous_text,
                updated_at_ms: previous_updated_at_ms,
                ..
            },
        ) = (current.as_ref(), previous.as_mut())
        && turn_id == previous_turn_id
        && entry_id == previous_entry_id
        && block_id == previous_block_id
    {
        previous_text.push_str(text);
        *previous_updated_at_ms = *updated_at_ms;
        return;
    }
    backlog.push(event);
    pub(crate) const MAX_SESSION_LIVE_BACKLOG_EVENTS: usize = 500;
    if backlog.len() > MAX_SESSION_LIVE_BACKLOG_EVENTS {
        let drain = backlog.len() - MAX_SESSION_LIVE_BACKLOG_EVENTS;
        backlog.drain(0..drain);
    }
}

pub(crate) fn push_pending_unowned_agent_event(agent: &mut AuxiliaryAgentTask, event: TurnEvent) {
    agent.pending_unowned_live_events.push(event);
    const MAX_PENDING_UNOWNED_AGENT_EVENTS: usize = 500;
    if agent.pending_unowned_live_events.len() > MAX_PENDING_UNOWNED_AGENT_EVENTS {
        let drain = agent.pending_unowned_live_events.len() - MAX_PENDING_UNOWNED_AGENT_EVENTS;
        agent.pending_unowned_live_events.drain(0..drain);
    }
}

pub(crate) fn flush_pending_unowned_agent_events(
    ui: &mut FullscreenUi<'_>,
    agent: &mut AuxiliaryAgentTask,
) {
    let Some(session_id) = agent.session_id.clone() else {
        return;
    };
    for event in agent.pending_unowned_live_events.drain(..) {
        buffer_session_live_event(ui, &session_id, event);
    }
}

pub(crate) fn turn_ended_error_message(
    outcome: TurnOutcome,
    terminal_reason: Option<TerminalReason>,
) -> String {
    let outcome = match outcome {
        TurnOutcome::Completed => "normal",
        TurnOutcome::Stopped => "stopped",
        TurnOutcome::Failed => "failed",
        TurnOutcome::Interrupted => "aborted",
    };
    format_turn_ended_error(
        outcome,
        terminal_reason.map(TerminalReason::message).as_deref(),
    )
}

pub(crate) fn turn_ended_error_text(outcome: Outcome, terminal_message: Option<&str>) -> String {
    format_turn_ended_error(outcome.as_str(), terminal_message)
}

fn format_turn_ended_error(outcome: &str, terminal_message: Option<&str>) -> String {
    match terminal_message.filter(|message| !message.trim().is_empty()) {
        Some(message) => format!("turn ended: {outcome} - {message}"),
        None => format!("turn ended: {outcome}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_turn_outcomes_keep_existing_tui_labels() {
        for (outcome, label) in [
            (TurnOutcome::Completed, "normal"),
            (TurnOutcome::Stopped, "stopped"),
            (TurnOutcome::Failed, "failed"),
            (TurnOutcome::Interrupted, "aborted"),
        ] {
            assert_eq!(
                turn_ended_error_message(outcome, None),
                format!("turn ended: {label}")
            );
        }
    }
}
