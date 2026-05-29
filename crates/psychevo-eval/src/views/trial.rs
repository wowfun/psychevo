#[allow(unused_imports)]
use super::*;

pub(crate) fn workspace_relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn view_trial(cell: &CellRun, workspace_root: &Path) -> ViewTrial {
    let (prompt_ref, prompt_preview, prompt_truncated) = prompt_preview(cell);
    let artifact_refs = [
        cell.case.artifacts.trajectory.as_path(),
        Path::new("prompt.md"),
        Path::new("run.json"),
        cell.case.artifacts.evaluator_stdout.as_path(),
        cell.case.artifacts.evaluator_stderr.as_path(),
    ]
    .into_iter()
    .filter_map(|relative| data_ref_for_relative(&cell.cell_root, relative, None).ok())
    .collect();
    ViewTrial {
        benchmark: cell.benchmark.clone(),
        trial_key: trial_key(cell),
        matrix_cell_key: matrix_cell_key(cell),
        cell_root_relative: workspace_relative_path(workspace_root, &cell.cell_root),
        case_id: cell.case.case_id.clone(),
        started_at_ms: cell.started_at_ms,
        finished_at_ms: cell.finished_at_ms,
        task_set_id: cell.case.task_set_id.clone(),
        task_id: cell.case.task_id.clone(),
        task_family: cell.case.task_family.clone(),
        agent_id: cell.case.agent_id.clone(),
        adapter: cell.case.candidate.adapter,
        model_name: effective_model_name(cell),
        status: cell.case.status,
        failure_class: cell.case.failure_class.clone(),
        score_passed: cell.case.score.passed,
        score: cell.case.score.score,
        score_message: cell.case.score.message.clone(),
        score_details: cell.case.score.details.clone(),
        duration_ms: cell.case.metrics.duration_ms,
        turns: cell.case.metrics.turns,
        tool_calls: cell.case.metrics.tool_calls,
        tool_errors: cell.case.metrics.tool_errors,
        input_tokens: cell.case.metrics.usage.input_tokens,
        output_tokens: cell.case.metrics.usage.output_tokens,
        cache_read_tokens: cell.case.metrics.usage.cache_read_tokens,
        cache_write_tokens: cell.case.metrics.usage.cache_write_tokens,
        reasoning_tokens: cell.case.metrics.usage.reasoning_tokens,
        total_tokens: cell.case.metrics.usage.total_tokens,
        cost_usd: cell.case.metrics.cost.amount_usd,
        prompt_ref,
        prompt_preview,
        prompt_truncated,
        artifact_refs,
    }
}

pub(crate) fn view_usage_row(cell: &CellRun) -> ViewUsageRow {
    ViewUsageRow {
        trial_key: trial_key(cell),
        input_tokens: cell.case.metrics.usage.input_tokens,
        output_tokens: cell.case.metrics.usage.output_tokens,
        cache_read_tokens: cell.case.metrics.usage.cache_read_tokens,
        cache_write_tokens: cell.case.metrics.usage.cache_write_tokens,
        reasoning_tokens: cell.case.metrics.usage.reasoning_tokens,
        total_tokens: cell.case.metrics.usage.total_tokens,
        cost_usd: cell.case.metrics.cost.amount_usd,
        accounting: cell.case.metrics.accounting.clone(),
    }
}

pub(crate) fn view_warning_rows(cell: &CellRun) -> Vec<ViewWarningRow> {
    cell.case
        .warnings
        .iter()
        .map(|warning| ViewWarningRow {
            trial_key: trial_key(cell),
            warning: warning.clone(),
        })
        .collect()
}

pub(crate) fn build_artifact_index(cell: &CellRun) -> ViewArtifactIndex {
    match list_artifact_files(&cell.cell_root, ArtifactFileMode::Artifact) {
        Ok(files) => {
            let paths = files
                .into_iter()
                .filter_map(|file| {
                    cell.cell_root
                        .join(file.data_ref.relative_path)
                        .canonicalize()
                        .ok()
                })
                .collect();
            ViewArtifactIndex {
                trial_key: trial_key(cell),
                paths,
                error: None,
            }
        }
        Err(err) => ViewArtifactIndex {
            trial_key: trial_key(cell),
            paths: Vec::new(),
            error: Some(format!("{err:#}")),
        },
    }
}

pub(crate) fn effective_model_name(cell: &CellRun) -> Option<String> {
    cell.case
        .candidate
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| {
            read_trajectory_events(cell)
                .ok()
                .and_then(|events| observed_model_name(&events))
        })
}

pub(crate) fn observed_model_name(events: &[TrajectoryEvent]) -> Option<String> {
    events
        .iter()
        .filter_map(|event| observed_psychevo_model(&event.data))
        .next()
        .or_else(|| {
            events
                .iter()
                .filter_map(|event| observed_generic_model(&event.data))
                .next()
        })
}

pub(crate) fn observed_psychevo_model(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(model) = map
                .get("_meta")
                .and_then(|meta| meta.get("psychevo"))
                .and_then(|psychevo| psychevo.get("model"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty())
            {
                return Some(model.to_string());
            }
            map.values().find_map(observed_psychevo_model)
        }
        Value::Array(items) => items.iter().find_map(observed_psychevo_model),
        _ => None,
    }
}

pub(crate) fn observed_generic_model(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["model", "model_name"] {
                if let Some(model) = map
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                {
                    return Some(model.to_string());
                }
            }
            map.values().find_map(observed_generic_model)
        }
        Value::Array(items) => items.iter().find_map(observed_generic_model),
        _ => None,
    }
}
