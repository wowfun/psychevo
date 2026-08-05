mod agent_commands;
mod live_projection;
mod slash_diff_sessions;

pub(crate) use agent_commands::{
    ACP_COMMAND_ADVERTISEMENT_LIMIT, SlashPromptAction, TERMINAL_SETUP_AUTH_METHOD_ID,
};
pub(crate) use live_projection::{AcpTurnProjection, send_turn_event_update};
pub(crate) use slash_diff_sessions::{
    AcpApprovalHandler, acp_command_capabilities, agent_message_update, ambiguous_session_matches,
    available_command_lines_from, available_commands_from, reasoning_effort_value,
    resolve_session_reference, send_diff_tool_call, send_session_setup_updates,
    send_session_update, send_slash_text,
};

#[cfg(test)]
mod tests;
