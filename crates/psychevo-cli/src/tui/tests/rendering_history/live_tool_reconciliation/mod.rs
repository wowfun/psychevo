#[allow(unused_imports)]
pub(crate) use super::*;

mod agent_rows;
mod completion_elapsed;
mod tool_preambles;

fn agent_rows<'a>(ui: &'a FullscreenUi<'_>) -> Vec<&'a TranscriptRow> {
    ui.transcript
        .iter()
        .filter(|row| row.tool_name.as_deref() == Some("spawn_agent"))
        .collect()
}

fn assert_stable_agent_rows(ui: &FullscreenUi<'_>, expected_len: usize) {
    let rows = agent_rows(ui);
    assert_eq!(rows.len(), expected_len, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");
}

fn assert_agent_row_target(
    ui: &FullscreenUi<'_>,
    tool_call_id: &str,
    expected_target: Option<&str>,
) {
    let row = agent_rows(ui)
        .into_iter()
        .find(|row| {
            if tool_call_id.is_empty() {
                row.tool_call_id.is_none()
            } else {
                row.tool_call_id.as_deref() == Some(tool_call_id)
            }
        })
        .expect("agent row");
    assert_eq!(row.agent_target.as_deref(), expected_target, "{row:#?}");
}
