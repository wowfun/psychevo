#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn agent_child_event_ends_live_backlog(event: &RunStreamEvent) -> bool {
    match event {
        RunStreamEvent::Event(value) => matches!(
            value.get("type").and_then(Value::as_str),
            Some("message_end") | Some("run_end")
        ),
        RunStreamEvent::Scoped { event, .. } => agent_child_event_ends_live_backlog(event),
        _ => false,
    }
}

pub(crate) fn stream_event_session_id(event: &RunStreamEvent) -> Option<&str> {
    match event {
        RunStreamEvent::Event(value) => value.get("session_id").and_then(Value::as_str),
        RunStreamEvent::Scoped { session_id, .. } => Some(session_id.as_str()),
        _ => None,
    }
}

pub(crate) fn buffer_session_live_event(
    ui: &mut FullscreenUi<'_>,
    session_id: &str,
    event: RunStreamEvent,
) {
    if session_live_event_ends_backlog(&event) {
        ui.session_live_event_backlog.remove(session_id);
        return;
    }
    let backlog = ui
        .session_live_event_backlog
        .entry(session_id.to_string())
        .or_default();
    backlog.push(event);
    pub(crate) const MAX_SESSION_LIVE_BACKLOG_EVENTS: usize = 500;
    if backlog.len() > MAX_SESSION_LIVE_BACKLOG_EVENTS {
        let drain = backlog.len() - MAX_SESSION_LIVE_BACKLOG_EVENTS;
        backlog.drain(0..drain);
    }
}

pub(crate) fn session_live_event_ends_backlog(event: &RunStreamEvent) -> bool {
    match event {
        RunStreamEvent::Event(value) => matches!(
            value.get("type").and_then(Value::as_str),
            Some("message_end") | Some("agent_end") | Some("run_end")
        ),
        RunStreamEvent::Scoped { event, .. } => session_live_event_ends_backlog(event),
        _ => false,
    }
}

pub(crate) fn turn_ended_error_message(
    outcome: Outcome,
    terminal_reason: Option<TerminalReason>,
) -> String {
    turn_ended_error_text(
        outcome,
        terminal_reason.map(TerminalReason::message).as_deref(),
    )
}

pub(crate) fn turn_ended_error_text(outcome: Outcome, terminal_message: Option<&str>) -> String {
    match terminal_message.filter(|message| !message.trim().is_empty()) {
        Some(message) => format!("turn ended: {} - {message}", outcome.as_str()),
        None => format!("turn ended: {}", outcome.as_str()),
    }
}
