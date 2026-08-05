#[path = "transcript/layout_rows.rs"]
mod layout_rows;
pub(crate) use layout_rows::{
    refresh_transcript_layout, render_active_selection, render_transcript,
    transcript_layout_matches_current, transcript_render_blocks, transcript_total_height_for_ui,
    wrap_command_text,
};
#[cfg(test)]
pub(crate) use layout_rows::{status_lines, transcript_layout_row_key, transcript_lines};
#[path = "transcript/content_blocks.rs"]
mod content_blocks;
pub(crate) use content_blocks::{
    DISPLAY_TOKEN_CHUNK_CELLS, DISPLAY_TOKEN_LONG_RUN_FREE_CELLS, ToolRowPhase, answer_lines,
    append_expandable_evidence_body, collapsed_more_line_count, display_token_count,
    display_token_count_segment, foldable_evidence_body, foldable_tool_title, is_agent_tool_row,
    ledger_body_collapse_policy, prompt_lines, row_expand_hint, suffix_display_width,
    thinking_lines, toggle_transcript_row_details, tool_display_title, tool_lines,
    tool_title_detail, user_shell_lines,
};
#[cfg(test)]
pub(crate) use content_blocks::{
    LEDGER_BODY_COLLAPSE_HEAD_LINES, LEDGER_BODY_COLLAPSE_TAIL_LINES, LEDGER_BODY_COLLAPSE_TOKENS,
    LEDGER_BODY_COLLAPSE_WIDTH,
};
#[path = "transcript/styles_truncation.rs"]
mod styles_truncation;
pub(crate) use styles_truncation::{
    active_tool_elapsed, collapse_ledger_body, focus_marker_style, interruption_style, label_style,
    ledger_title_line, ledger_title_right_text, style_for_body, suffix_display_tokens,
    tool_elapsed_label, truncate_display_width,
};
