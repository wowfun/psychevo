#[allow(unused_imports)]
use super::*;

pub(crate) fn derive_atif_trajectory(
    cell: &ViewCell,
    events: &[TrajectoryEvent],
) -> AtifTrajectory {
    let prompt = retained_prompt(cell).ok().flatten();
    let prompt_unavailable = prompt.is_none();
    let mut steps = Vec::new();
    let mut next_step_id = 1_u64;
    steps.push(AtifStep {
        step_id: next_step_id,
        source: "user".to_string(),
        message: Value::String(prompt.unwrap_or_default()),
        reasoning_content: None,
        tool_calls: Vec::new(),
        observation: None,
        metrics: None,
        extra: Some(if prompt_unavailable {
            json!({ "prompt_unavailable": true, "timestamp_ms": cell.started_at_ms })
        } else {
            json!({ "timestamp_ms": cell.started_at_ms })
        }),
        llm_call_count: None,
    });
    next_step_id += 1;

    if cell.case.candidate.adapter.is_acp_adapter() {
        for step in derive_acp_atif_steps(next_step_id, events) {
            steps.push(step);
        }
    } else {
        for event in events {
            if let Some(step) = derive_jsonl_atif_step(next_step_id, event) {
                steps.push(step);
                next_step_id += 1;
            }
        }
    }

    let mut extra = json!({
        "benchmark": cell.benchmark,
        "case_id": cell.case.case_id,
        "task_set_id": cell.case.task_set_id,
        "task_id": cell.case.task_id,
        "matrix_cell_key": view_matrix_cell_key(cell),
        "trial_key": view_trial_key(cell),
    });
    if prompt_unavailable {
        extra["prompt_unavailable"] = Value::Bool(true);
    }
    let total_steps = steps.len() as u64;
    AtifTrajectory {
        schema_version: "ATIF-v1.7".to_string(),
        session_id: trajectory_session_id(cell, events),
        trajectory_id: Some(view_trial_key(cell)),
        agent: AtifAgent {
            name: cell.case.agent_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            model_name: effective_model_name(cell),
            extra: Some(json!({
                "adapter": cell.case.candidate.adapter,
            })),
        },
        steps,
        notes: Some("Derived from peval trajectory.jsonl; unmapped lifecycle events remain in peval trajectory metadata.".to_string()),
        final_metrics: Some(atif_final_metrics(cell, total_steps)),
        extra: Some(extra),
    }
}

pub(crate) fn retained_prompt(cell: &CellRun) -> Result<Option<String>> {
    Ok(prompt_preview(cell).1)
}

pub(crate) fn prompt_preview(cell: &CellRun) -> (Option<ViewDataRef>, Option<String>, bool) {
    for relative in [
        Path::new("prompt.md"),
        Path::new("workspace/.peval/prompt.md"),
    ] {
        let path = cell.cell_root.join(relative);
        if !path.exists() {
            continue;
        }
        let Ok(safe) = safe_artifact_path(&cell.cell_root, &path) else {
            continue;
        };
        let Ok(data_ref) = data_ref_for_relative(&cell.cell_root, relative, Some("prompt")) else {
            continue;
        };
        let Ok((preview, truncated, previewable)) = read_text_preview(&safe) else {
            continue;
        };
        return (
            Some(data_ref),
            previewable.then_some(preview).flatten(),
            truncated,
        );
    }
    (None, None, false)
}
