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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeToolLedgerRow {
    turn_id: Option<String>,
    entry_id: Option<String>,
    block_id: Option<String>,
    source: Option<String>,
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    status: &'static str,
    order: usize,
    title: String,
    has_result: bool,
    active_elapsed_owner: bool,
}

fn runtime_tool_ledger(ui: &FullscreenUi<'_>) -> Vec<RuntimeToolLedgerRow> {
    ui.transcript
        .iter()
        .enumerate()
        .filter(|(_, row)| row.tool_name.is_some())
        .map(|(order, row)| {
            let active_elapsed_owner = active_tool_row(row);
            RuntimeToolLedgerRow {
                turn_id: row.transcript_turn_id.clone(),
                entry_id: row.transcript_entry_id.clone(),
                block_id: row.transcript_block_id.clone(),
                source: row.transcript_source.clone(),
                tool_name: row.tool_name.clone(),
                tool_call_id: row.tool_call_id.clone(),
                status: if row.failed {
                    "failed"
                } else if row.interrupted {
                    "cancelled"
                } else if active_elapsed_owner {
                    "running"
                } else if row.tool_elapsed.is_some() || row.tool_call_id.is_some() {
                    "completed"
                } else {
                    "info"
                },
                order,
                title: row.title.clone(),
                has_result: row.tool_elapsed.is_some() || !row.text.trim().is_empty(),
                active_elapsed_owner,
            }
        })
        .collect()
}

fn assert_runtime_tool_ledger(checkpoint: &str, rows: &[RuntimeToolLedgerRow]) {
    let mut block_ids = std::collections::BTreeSet::new();
    let mut tool_ids = std::collections::BTreeSet::new();
    for row in rows {
        if let Some(block_id) = &row.block_id {
            assert!(
                block_ids.insert((row.turn_id.clone(), block_id.clone())),
                "{checkpoint}: duplicate block id in runtime ledger: {rows:#?}"
            );
        }
        if let (Some(tool_name), Some(tool_call_id)) = (&row.tool_name, &row.tool_call_id) {
            assert_ne!(
                tool_call_id, tool_name,
                "{checkpoint}: tool_call_id fell back to tool name: {rows:#?}"
            );
            assert!(
                tool_ids.insert((row.turn_id.clone(), tool_name.clone(), tool_call_id.clone())),
                "{checkpoint}: duplicate tool identity in runtime ledger: {rows:#?}"
            );
        }
        if matches!(row.status, "completed" | "failed" | "cancelled") {
            assert!(
                !row.active_elapsed_owner,
                "{checkpoint}: terminal row still owns active elapsed: {rows:#?}"
            );
        }
    }
}
