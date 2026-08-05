#[path = "evidence/ledger.rs"]
mod ledger;
pub(crate) use ledger::{
    active_tool_row, active_tool_title, agent_child_latest_tokens, agent_relationship_title,
    agent_session_start_title, agent_target_from_tool_event, background_running_agent_result,
    clarify_no_answer_result, completed_live_tool_elapsed, completed_tool_title_from_active,
    evidence_kind, evidence_kind_for_value, format_compact_count, format_tool_result_summary,
    format_tool_summary, matching_agent_relationship, pluralize, running_agent_tool_full_text,
    single_line_preview, tool_event_interrupted, tool_output_text, tool_result_output_text,
    tool_title, tool_title_as_invocation, tool_title_for_update, usage_total_tokens,
    user_shell_title,
};
#[path = "evidence/projection.rs"]
mod projection;
pub(crate) use projection::{
    StreamingToolCall, TurnMetaProjection, assistant_message_stream_event_type, format_count,
    format_duration_compact, format_nanodollars, model_meta_label, scoped_tool_position_key,
    streaming_tool_calls_from_event, tool_id_key, tool_position_key, turn_meta_text,
    usage_context_tokens,
};
