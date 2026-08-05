use std::time::Duration;

use psychevo::application::{ToolDisplayCategory, ToolDisplaySpec};
use serde_json::Value;

use crate::tui::{
    support_history::metadata_elapsed_duration,
    ui_types::{TranscriptKind, TranscriptRow},
};

pub(crate) fn evidence_kind(tool: &str) -> TranscriptKind {
    evidence_kind_from_display(&ToolDisplaySpec::for_name(tool))
}

pub(crate) fn evidence_kind_for_value(tool: &str, value: &Value) -> TranscriptKind {
    evidence_kind_from_display(&tool_display_spec(tool, value))
}

pub(crate) fn evidence_kind_from_display(display: &ToolDisplaySpec) -> TranscriptKind {
    match display.category {
        ToolDisplayCategory::Explore => TranscriptKind::Explored,
        ToolDisplayCategory::Run => TranscriptKind::Ran,
        ToolDisplayCategory::Update => TranscriptKind::Updated,
        ToolDisplayCategory::Status => TranscriptKind::Status,
    }
}

pub(crate) fn active_tool_row(row: &TranscriptRow) -> bool {
    !row.failed && !row.interrupted && row.tool_started.is_some() && row.tool_elapsed.is_none()
}

pub(crate) fn completed_live_tool_elapsed(
    row: &TranscriptRow,
    metadata: Option<&Value>,
) -> Option<Duration> {
    let runtime = metadata_elapsed_duration(metadata);
    let active = row.tool_started.map(|started| started.elapsed());
    match (runtime, active) {
        (Some(runtime), Some(active)) => Some(runtime.max(active)),
        (Some(runtime), None) => Some(runtime),
        (None, Some(active)) => Some(active),
        (None, None) => None,
    }
}

pub(crate) fn completed_tool_title_from_active(kind: TranscriptKind, title: &str) -> String {
    tool_title_as_invocation(None, kind, title, false)
}

#[path = "ledger/titles.rs"]
mod titles;
pub(crate) use titles::{
    active_tool_title, clarify_no_answer_result, tool_display_spec, tool_title,
    tool_title_as_invocation, tool_title_for_update, user_shell_title,
};

#[path = "ledger/output.rs"]
mod output;
pub(crate) use output::{
    format_tool_result_summary, format_tool_summary, tool_output_text, tool_result_output_text,
};

#[path = "ledger/agents.rs"]
mod agents;
pub(crate) use agents::{
    agent_child_latest_tokens, agent_relationship_title, agent_session_start_title,
    agent_target_from_tool_event, background_running_agent_result, format_compact_count,
    matching_agent_relationship, pluralize, running_agent_tool_full_text, single_line_preview,
    usage_total_tokens,
};

#[path = "ledger/exec_misc.rs"]
mod exec_misc;
pub(crate) use exec_misc::{model_label, tool_event_interrupted};

#[cfg(test)]
mod tests {
    use super::output::body_text_from_keys;
    use serde_json::json;

    #[test]
    fn body_text_from_keys_skips_null_values() {
        let keys = vec!["diff".to_string(), "error".to_string()];
        let result = json!({
            "diff": "diff text",
            "error": null
        });
        assert_eq!(
            body_text_from_keys(&keys, &result),
            Some("diff text".to_string())
        );

        let keys = vec!["error".to_string()];
        assert_eq!(body_text_from_keys(&keys, &result), None);
    }
}
