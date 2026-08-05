#[path = "fullscreen/composer_and_popups.rs"]
mod composer_and_popups;
#[path = "fullscreen/history_and_selection.rs"]
mod history_and_selection;
#[path = "fullscreen/shell_events.rs"]
mod shell_events;
#[path = "fullscreen/stream_events.rs"]
mod stream_events;
pub(crate) use shell_events::shell_outcome_label;
#[path = "fullscreen/turn_state.rs"]
mod turn_state;
pub(crate) use turn_state::{
    TUI_TURN_START_TRANSCRIPT_SOURCE, agent_child_status_text, append_agent_child_live_fragment,
    apply_agent_child_value_preview, auxiliary_agent_live_for_session, bounded_stdin_display,
    clarify_request_args_value, current_session_matches, exec_result_completed,
    exec_result_running, exec_row_full_text_without_history_marker, exec_session_id_from_args,
    exec_session_id_from_result, refresh_agent_child_preview, selected_skill_names_from_event,
    set_exec_row_text, tool_result_output, turn_start_local_row, with_exec_history_running_marker,
    write_stdin_non_empty_chars,
};
