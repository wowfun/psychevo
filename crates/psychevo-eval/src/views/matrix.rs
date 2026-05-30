#[allow(unused_imports)]
use super::*;

pub(crate) fn matrix_cell_key(cell: &CellRun) -> String {
    cell.cell_key.clone()
}

pub(crate) fn trial_key(cell: &CellRun) -> String {
    format!("{}:t001", matrix_cell_key(cell))
}

pub(crate) fn view_matrix_cell_key(cell: &ViewCell) -> String {
    match cell.variant_id.as_deref() {
        Some(variant) => format!("{variant}:{}", cell.cell_key),
        None => matrix_cell_key(cell),
    }
}

pub(crate) fn view_trial_key(cell: &ViewCell) -> String {
    format!("{}:t001", view_matrix_cell_key(cell))
}

pub(crate) fn agent_axis_id(cell: &ViewCell) -> String {
    let candidate = match effective_model_name(cell).as_deref() {
        Some(model) if !model.trim().is_empty() => {
            format!("{}::{}", cell.case.agent_id, model.trim())
        }
        _ => cell.case.agent_id.clone(),
    };
    match cell.variant_id.as_deref() {
        Some(variant) => format!("{variant}::{candidate}"),
        None => candidate,
    }
}

pub(crate) fn agent_axis_label(cell: &ViewCell) -> String {
    let candidate = match effective_model_name(cell).as_deref() {
        Some(model) if !model.trim().is_empty() => {
            format!("{} / {}", cell.case.agent_id, model.trim())
        }
        _ => cell.case.agent_id.clone(),
    };
    match cell.variant_label.as_deref() {
        Some(variant) => format!("{variant} / {candidate}"),
        None => candidate,
    }
}

pub(crate) fn cell_identity_key(cell: &ViewCell) -> (String, String, String, String) {
    (
        cell.case.task_id.clone(),
        cell.case.agent_id.clone(),
        effective_model_name(cell).unwrap_or_default(),
        cell.variant_id.clone().unwrap_or_default(),
    )
}

pub(crate) fn latest_cell<'a>(left: &'a ViewCell, right: &'a ViewCell) -> &'a ViewCell {
    let left_key = (left.finished_at_ms, left.started_at_ms, &left.cell_key);
    let right_key = (right.finished_at_ms, right.started_at_ms, &right.cell_key);
    if right_key > left_key { right } else { left }
}

pub(crate) fn build_view_matrix(cells: &[ViewCell]) -> ViewMatrix {
    let mut task_axis = BTreeMap::<String, ViewMatrixAxisEntry>::new();
    let mut agent_axis = BTreeMap::<String, ViewMatrixAxisEntry>::new();
    let mut grouped = BTreeMap::<(String, String, String, String), Vec<&ViewCell>>::new();
    for cell in cells {
        task_axis
            .entry(cell.case.task_id.clone())
            .or_insert_with(|| ViewMatrixAxisEntry {
                id: cell.case.task_id.clone(),
                label: cell.case.task_id.clone(),
            });
        agent_axis
            .entry(agent_axis_id(cell))
            .or_insert_with(|| ViewMatrixAxisEntry {
                id: agent_axis_id(cell),
                label: agent_axis_label(cell),
            });
        grouped
            .entry(cell_identity_key(cell))
            .or_default()
            .push(cell);
    }
    let mut matrix_cells = Vec::new();
    for (_key, mut trials) in grouped {
        trials.sort_by(|left, right| {
            (left.finished_at_ms, left.started_at_ms, &left.cell_key).cmp(&(
                right.finished_at_ms,
                right.started_at_ms,
                &right.cell_key,
            ))
        });
        let representative = trials
            .iter()
            .copied()
            .reduce(latest_cell)
            .unwrap_or(trials[0]);
        matrix_cells.push(ViewMatrixCell {
            benchmark: representative.benchmark.clone(),
            matrix_cell_key: view_matrix_cell_key(representative),
            trial_keys: trials.iter().map(|cell| view_trial_key(cell)).collect(),
            representative_trial_key: view_trial_key(representative),
            agent_axis_id: agent_axis_id(representative),
            variant_id: representative.variant_id.clone(),
            variant_label: representative.variant_label.clone(),
            task_set_id: representative.case.task_set_id.clone(),
            task_id: representative.case.task_id.clone(),
            task_family: representative.case.task_family.clone(),
            agent_id: representative.case.agent_id.clone(),
            adapter: representative.case.candidate.adapter,
            model_name: effective_model_name(representative),
            status: representative.case.status,
            failure_class: representative.case.failure_class.clone(),
            score: representative.case.score.score,
            duration_ms: representative.case.metrics.duration_ms,
            turns: representative.case.metrics.turns,
            tool_calls: representative.case.metrics.tool_calls,
            tool_errors: representative.case.metrics.tool_errors,
        });
    }
    ViewMatrix {
        task_axis: task_axis.into_values().collect(),
        agent_axis: agent_axis.into_values().collect(),
        cells: matrix_cells,
    }
}

pub(crate) fn default_heatmap_metric(cells: &[ViewCell]) -> String {
    ["score", "duration", "tokens", "tools", "turns"]
        .into_iter()
        .find(|metric| {
            metric_varies(
                cells
                    .iter()
                    .filter_map(|cell| heatmap_metric_value(cell, metric)),
            )
        })
        .unwrap_or("score")
        .to_string()
}

fn heatmap_metric_value(cell: &ViewCell, metric: &str) -> Option<f64> {
    match metric {
        "score" => cell.case.score.score,
        "duration" => Some(cell.case.metrics.duration_ms as f64),
        "tokens" => cell
            .case
            .metrics
            .usage
            .total_tokens
            .map(|value| value as f64),
        "tools" => Some(cell.case.metrics.tool_calls as f64),
        "turns" => cell.case.metrics.turns.map(|value| value as f64),
        _ => None,
    }
}

fn metric_varies(values: impl Iterator<Item = f64>) -> bool {
    let mut values = values.filter(|value| value.is_finite());
    let Some(first) = values.next() else {
        return false;
    };
    values.any(|value| value != first)
}

pub(crate) fn build_view_leaderboard(cells: &[ViewCell]) -> ViewLeaderboard {
    let mut groups = BTreeMap::<(String, String, String), Vec<&ViewCell>>::new();
    for cell in cells {
        groups
            .entry((
                cell.case.agent_id.clone(),
                effective_model_name(cell).unwrap_or_default(),
                cell.variant_id.clone().unwrap_or_default(),
            ))
            .or_default()
            .push(cell);
    }
    let mut entries = groups
        .into_iter()
        .map(|((agent_id, model_name, variant_id), mut trials)| {
            trials.sort_by(|left, right| {
                (
                    left.case.task_id.as_str(),
                    left.started_at_ms,
                    &left.cell_key,
                )
                    .cmp(&(
                        right.case.task_id.as_str(),
                        right.started_at_ms,
                        &right.cell_key,
                    ))
            });
            let successes = trials
                .iter()
                .filter(|cell| cell.case.status == CaseStatus::Passed)
                .count();
            let total_trials = trials.len();
            let pass_rate = ratio(successes, total_trials);
            let average_score = average_f64(trials.iter().filter_map(|cell| cell.case.score.score));
            let average_duration_ms = average_f64(
                trials
                    .iter()
                    .map(|cell| cell.case.metrics.duration_ms as f64),
            );
            let total_tokens = sum_optional_u64(
                trials
                    .iter()
                    .map(|cell| cell.case.metrics.usage.total_tokens),
            );
            let total_cost_usd =
                sum_optional_f64(trials.iter().map(|cell| cell.case.metrics.cost.amount_usd));
            let tasks = leaderboard_tasks(&trials);
            let trial_keys = trials.iter().map(|cell| view_trial_key(cell)).collect();
            ViewLeaderboardEntry {
                rank: 0,
                agent_id,
                model_name: non_empty_string(model_name),
                variant_id: non_empty_string(variant_id),
                variant_label: trials.first().and_then(|cell| cell.variant_label.clone()),
                total_trials,
                successes,
                failures: total_trials.saturating_sub(successes),
                pass_rate,
                average_score,
                average_duration_ms,
                total_tokens,
                total_cost_usd,
                tasks,
                trial_keys,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(compare_leaderboard_entries);
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.rank = index + 1;
    }
    ViewLeaderboard { entries }
}

pub(crate) fn leaderboard_tasks(trials: &[&ViewCell]) -> Vec<ViewLeaderboardTask> {
    let mut tasks = BTreeMap::<String, Vec<&ViewCell>>::new();
    for cell in trials {
        tasks
            .entry(cell.case.task_id.clone())
            .or_default()
            .push(*cell);
    }
    tasks
        .into_iter()
        .map(|(task_id, cells)| {
            let total_trials = cells.len();
            let successes = cells
                .iter()
                .filter(|cell| cell.case.status == CaseStatus::Passed)
                .count();
            ViewLeaderboardTask {
                task_id,
                task_family: cells
                    .first()
                    .map(|cell| cell.case.task_family.clone())
                    .unwrap_or_default(),
                total_trials,
                successes,
                pass_rate: ratio(successes, total_trials),
                average_score: average_f64(cells.iter().filter_map(|cell| cell.case.score.score)),
                average_duration_ms: average_f64(
                    cells
                        .iter()
                        .map(|cell| cell.case.metrics.duration_ms as f64),
                ),
                trial_keys: cells.iter().map(|cell| view_trial_key(cell)).collect(),
            }
        })
        .collect()
}

pub(crate) fn compare_leaderboard_entries(
    left: &ViewLeaderboardEntry,
    right: &ViewLeaderboardEntry,
) -> std::cmp::Ordering {
    compare_f64_desc(left.pass_rate, right.pass_rate)
        .then_with(|| compare_option_f64_desc(left.average_score, right.average_score))
        .then_with(|| compare_option_f64_asc(left.average_duration_ms, right.average_duration_ms))
        .then_with(|| compare_option_u64_asc(left.total_tokens, right.total_tokens))
        .then_with(|| compare_option_f64_asc(left.total_cost_usd, right.total_cost_usd))
        .then_with(|| left.agent_id.cmp(&right.agent_id))
        .then_with(|| left.model_name.cmp(&right.model_name))
        .then_with(|| left.variant_id.cmp(&right.variant_id))
}

pub(crate) fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub(crate) fn average_f64(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0_u64;
    for value in values {
        if value.is_finite() {
            total += value;
            count += 1;
        }
    }
    (count > 0).then_some(total / count as f64)
}

pub(crate) fn sum_optional_u64(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let mut total = 0_u64;
    let mut seen = false;
    for value in values.flatten() {
        total = total.saturating_add(value);
        seen = true;
    }
    seen.then_some(total)
}

pub(crate) fn sum_optional_f64(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total = 0.0;
    let mut seen = false;
    for value in values.flatten() {
        if value.is_finite() {
            total += value;
            seen = true;
        }
    }
    seen.then_some(total)
}

pub(crate) fn non_empty_string(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub(crate) fn compare_f64_desc(left: f64, right: f64) -> std::cmp::Ordering {
    right
        .partial_cmp(&left)
        .unwrap_or(std::cmp::Ordering::Equal)
}

pub(crate) fn compare_option_f64_desc(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_f64_desc(left, right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(crate) fn compare_option_f64_asc(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(crate) fn compare_option_u64_asc(left: Option<u64>, right: Option<u64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}
