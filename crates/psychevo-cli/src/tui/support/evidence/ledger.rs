#[allow(unused_imports)]
pub(crate) use super::*;

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

pub(crate) fn is_write_like_tool(tool: &str) -> bool {
    matches!(tool, "write" | "edit")
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
#[allow(unused_imports)]
pub use titles::*;

#[path = "ledger/output.rs"]
mod output;
#[allow(unused_imports)]
pub use output::*;

#[path = "ledger/agents.rs"]
mod agents;
#[allow(unused_imports)]
pub use agents::*;

#[path = "ledger/exec_misc.rs"]
mod exec_misc;
#[allow(unused_imports)]
pub use exec_misc::*;

#[cfg(test)]
mod tests {
    use super::*;
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
