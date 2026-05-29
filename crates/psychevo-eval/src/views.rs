#[allow(unused_imports)]
use crate::*;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::io::Read;

const VIEW_TEXT_PREVIEW_BYTES: usize = 1024 * 1024;
const TRAJECTORY_DATA_PREVIEW_CHARS: usize = 2048;
const RENDER_PREVIEW_CHARS: usize = 1600;
const ATIF_CONTENT_PREVIEW_CHARS: usize = 16 * 1024;
const SMALL_IMAGE_INLINE_BYTES: u64 = 96 * 1024;

pub(crate) fn build_view(request: ViewRequest) -> Result<ViewReport> {
    let (store, scope, benchmark, cells) = load_view_cells(&request)?;
    let includes = normalize_includes(request.include);
    let cases = cells
        .iter()
        .map(|cell| cell.case.clone())
        .collect::<Vec<_>>();
    let passed_trials = cells
        .iter()
        .filter(|cell| cell.case.status == CaseStatus::Passed)
        .count();
    let failed_trials = cells.len().saturating_sub(passed_trials);
    let status = if failed_trials == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let metrics = aggregate_run_metrics(&cases, cases.iter().map(|case| case.duration_ms).sum());
    let summary = ViewSummary {
        total_trials: cells.len(),
        passed_trials,
        failed_trials,
        status,
        metrics,
    };
    let groups = group_view_cells(&cells, &request.group_by);
    let matrix = if includes.contains(&ViewInclude::Matrix) {
        build_view_matrix(&cells)
    } else {
        ViewMatrix::default()
    };
    let trials = if includes.contains(&ViewInclude::Summary)
        || includes.contains(&ViewInclude::Matrix)
        || !includes.is_empty()
    {
        cells.iter().map(view_trial).collect()
    } else {
        Vec::new()
    };
    let usage = if includes.contains(&ViewInclude::Usage) {
        cells.iter().map(view_usage_row).collect()
    } else {
        Vec::new()
    };
    let warnings = if includes.contains(&ViewInclude::Warnings) {
        cells.iter().flat_map(view_warning_rows).collect()
    } else {
        Vec::new()
    };
    let artifacts = if includes.contains(&ViewInclude::Artifacts) {
        cells.iter().map(build_artifact_index).collect()
    } else {
        Vec::new()
    };
    let trajectory = if includes.contains(&ViewInclude::Trajectory) {
        cells.iter().map(build_trajectory_report).collect()
    } else {
        Vec::new()
    };
    let atif = if includes.contains(&ViewInclude::Atif) {
        cells.iter().map(build_atif_report).collect()
    } else {
        Vec::new()
    };
    let logs = if includes.contains(&ViewInclude::Logs) {
        cells.iter().map(build_log_index).collect()
    } else {
        Vec::new()
    };
    let analysis = if includes.contains(&ViewInclude::Analysis) {
        cells.iter().map(build_analysis_report).collect()
    } else {
        Vec::new()
    };
    let diff = if includes.contains(&ViewInclude::Diff) {
        cells.iter().map(build_diff_report).collect()
    } else {
        Vec::new()
    };
    Ok(ViewReport {
        schema_version: VIEW_SCHEMA_VERSION,
        includes,
        scope: ViewScope {
            workspace_root: store.root,
            path: scope,
            benchmark,
        },
        summary,
        groups,
        matrix,
        trials,
        usage,
        warnings,
        artifacts,
        trajectory,
        atif,
        logs,
        analysis,
        diff,
    })
}

pub(crate) fn load_view_cells(
    request: &ViewRequest,
) -> Result<(EvalStore, PathBuf, Option<String>, Vec<CellRun>)> {
    let store = EvalStore::resolve(request.store_root.clone())?;
    let (scope, benchmark) = if let Some(path) = &request.path {
        let scope = resolve_view_scope_path(&store.root, path);
        let benchmark = infer_benchmark_from_scope(&store.root, &scope);
        (scope, benchmark)
    } else {
        let project = load_project_from_selection(
            request.config.as_deref(),
            request.benchmark.as_deref(),
            request.store_root.clone(),
        )?;
        (store.cell_runs_root(&project), Some(project.benchmark_id))
    };
    let mut cells = store.scan_cell_runs(&scope)?;
    cells.retain(|cell| view_cell_matches(cell, request));
    Ok((store, scope, benchmark, cells))
}

pub(crate) fn resolve_view_scope_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

pub(crate) fn infer_benchmark_from_scope(root: &Path, scope: &Path) -> Option<String> {
    scope
        .strip_prefix(root.join("runs"))
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .map(str::to_string)
}

pub(crate) fn view_cell_matches(cell: &CellRun, request: &ViewRequest) -> bool {
    request
        .task_set
        .as_ref()
        .is_none_or(|task_set| cell.case.task_set_id == *task_set)
        && request
            .agent
            .as_ref()
            .is_none_or(|agent| cell.case.agent_id == *agent)
        && request
            .task
            .as_ref()
            .is_none_or(|task| cell.case.task_id == *task)
        && request
            .status
            .is_none_or(|status| cell.case.status == CaseStatus::from(status))
}

pub(crate) fn group_view_cells(cells: &[CellRun], group_by: &[ViewGroupBy]) -> Vec<ViewGroupRow> {
    if group_by.is_empty() {
        return Vec::new();
    }
    let mut groups: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for cell in cells {
        let key = group_by
            .iter()
            .map(|group| match group {
                ViewGroupBy::Agent => cell.case.agent_id.clone(),
                ViewGroupBy::Task => cell.case.task_id.clone(),
                ViewGroupBy::TaskSet => cell.case.task_set_id.clone(),
                ViewGroupBy::Status => format!("{:?}", cell.case.status),
            })
            .collect::<Vec<_>>()
            .join("/");
        let entry = groups.entry(key).or_default();
        entry.0 += 1;
        if cell.case.status == CaseStatus::Passed {
            entry.1 += 1;
        }
    }
    groups
        .into_iter()
        .map(|(key, (total_trials, passed_trials))| {
            let failed_trials = total_trials.saturating_sub(passed_trials);
            ViewGroupRow {
                key,
                total_trials,
                passed_trials,
                failed_trials,
                status: if failed_trials == 0 {
                    RunStatus::Passed
                } else {
                    RunStatus::Failed
                },
            }
        })
        .collect()
}

pub(crate) fn normalize_includes(includes: Vec<ViewInclude>) -> Vec<ViewInclude> {
    let includes = if includes.is_empty() {
        vec![ViewInclude::Summary, ViewInclude::Matrix]
    } else {
        includes
    };
    includes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn matrix_cell_key(cell: &CellRun) -> String {
    cell.cell_key.clone()
}

pub(crate) fn trial_key(cell: &CellRun) -> String {
    format!("{}:t001", matrix_cell_key(cell))
}

pub(crate) fn build_view_matrix(cells: &[CellRun]) -> ViewMatrix {
    let mut task_axis = BTreeMap::<String, ViewMatrixAxisEntry>::new();
    let mut agent_axis = BTreeMap::<String, ViewMatrixAxisEntry>::new();
    let mut matrix_cells = Vec::new();
    for cell in cells {
        task_axis
            .entry(cell.case.task_id.clone())
            .or_insert_with(|| ViewMatrixAxisEntry {
                id: cell.case.task_id.clone(),
                label: cell.case.task_id.clone(),
            });
        agent_axis
            .entry(cell.case.agent_id.clone())
            .or_insert_with(|| ViewMatrixAxisEntry {
                id: cell.case.agent_id.clone(),
                label: cell.case.agent_id.clone(),
            });
        matrix_cells.push(ViewMatrixCell {
            benchmark: cell.benchmark.clone(),
            matrix_cell_key: matrix_cell_key(cell),
            trial_keys: vec![trial_key(cell)],
            representative_trial_key: trial_key(cell),
            task_set_id: cell.case.task_set_id.clone(),
            task_id: cell.case.task_id.clone(),
            task_family: cell.case.task_family.clone(),
            agent_id: cell.case.agent_id.clone(),
            adapter: cell.case.candidate.adapter,
            status: cell.case.status,
            failure_class: cell.case.failure_class.clone(),
            score: cell.case.score.score,
            duration_ms: cell.case.metrics.duration_ms,
            turns: cell.case.metrics.turns,
            tool_calls: cell.case.metrics.tool_calls,
            tool_errors: cell.case.metrics.tool_errors,
        });
    }
    ViewMatrix {
        task_axis: task_axis.into_values().collect(),
        agent_axis: agent_axis.into_values().collect(),
        cells: matrix_cells,
    }
}

pub(crate) fn view_trial(cell: &CellRun) -> ViewTrial {
    let artifact_refs = [
        cell.case.artifacts.trajectory.as_path(),
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
        case_id: cell.case.case_id.clone(),
        task_set_id: cell.case.task_set_id.clone(),
        task_id: cell.case.task_id.clone(),
        task_family: cell.case.task_family.clone(),
        agent_id: cell.case.agent_id.clone(),
        adapter: cell.case.candidate.adapter,
        status: cell.case.status,
        failure_class: cell.case.failure_class.clone(),
        score: cell.case.score.score,
        duration_ms: cell.case.metrics.duration_ms,
        turns: cell.case.metrics.turns,
        tool_calls: cell.case.metrics.tool_calls,
        tool_errors: cell.case.metrics.tool_errors,
        artifact_refs,
    }
}

pub(crate) fn view_usage_row(cell: &CellRun) -> ViewUsageRow {
    ViewUsageRow {
        benchmark: cell.benchmark.clone(),
        trial_key: trial_key(cell),
        matrix_cell_key: matrix_cell_key(cell),
        case_id: cell.case.case_id.clone(),
        agent_id: cell.case.agent_id.clone(),
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
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            case_id: cell.case.case_id.clone(),
            agent_id: cell.case.agent_id.clone(),
            warning: warning.clone(),
        })
        .collect()
}

pub(crate) fn build_artifact_index(cell: &CellRun) -> ViewArtifactIndex {
    match list_artifact_files(&cell.cell_root, ArtifactFileMode::Artifact) {
        Ok(files) => ViewArtifactIndex {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            files,
            error: None,
        },
        Err(err) => ViewArtifactIndex {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            files: Vec::new(),
            error: Some(format!("{err:#}")),
        },
    }
}

pub(crate) fn build_trajectory_report(cell: &CellRun) -> ViewTrajectoryReport {
    let data_ref = data_ref_for_relative(&cell.cell_root, &cell.case.artifacts.trajectory, None)
        .unwrap_or_else(|_| missing_data_ref("trajectory", &cell.case.artifacts.trajectory));
    match read_trajectory_events(cell) {
        Ok(events) => {
            let atif = derive_atif_trajectory(cell, &events);
            let steps = atif
                .steps
                .iter()
                .map(view_trajectory_step)
                .collect::<Vec<_>>();
            let graph = trajectory_graph_from_steps(&steps);
            ViewTrajectoryReport {
                benchmark: cell.benchmark.clone(),
                trial_key: trial_key(cell),
                matrix_cell_key: matrix_cell_key(cell),
                data_ref,
                total_events: events.len(),
                unmapped_events: events.len().saturating_sub(steps.len().saturating_sub(1)),
                total_steps: steps.len(),
                duration_ms: cell.case.metrics.duration_ms,
                tool_calls: cell.case.metrics.tool_calls,
                tool_errors: cell.case.metrics.tool_errors,
                token_total: cell.case.metrics.usage.total_tokens,
                cost_usd: cell.case.metrics.cost.amount_usd,
                steps,
                graph,
                error: None,
            }
        }
        Err(err) => ViewTrajectoryReport {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            data_ref,
            total_events: 0,
            unmapped_events: 0,
            total_steps: 0,
            duration_ms: cell.case.metrics.duration_ms,
            tool_calls: cell.case.metrics.tool_calls,
            tool_errors: cell.case.metrics.tool_errors,
            token_total: cell.case.metrics.usage.total_tokens,
            cost_usd: cell.case.metrics.cost.amount_usd,
            steps: Vec::new(),
            graph: ViewTrajectoryGraph::default(),
            error: Some(format!("{err:#}")),
        },
    }
}

pub(crate) fn build_atif_report(cell: &CellRun) -> ViewAtifReport {
    match read_trajectory_events(cell) {
        Ok(events) => ViewAtifReport {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            trajectory: derive_atif_trajectory(cell, &events),
            error: None,
        },
        Err(err) => ViewAtifReport {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            trajectory: derive_atif_trajectory(cell, &[]),
            error: Some(format!("{err:#}")),
        },
    }
}

pub(crate) fn view_trajectory_step(step: &AtifStep) -> ViewTrajectoryStep {
    let tool_names = step
        .tool_calls
        .iter()
        .map(|tool| tool.function_name.clone())
        .collect::<Vec<_>>();
    let tool_error = step
        .observation
        .as_ref()
        .is_some_and(|observation| observation.results.iter().any(observation_result_is_error));
    let raw_summary = atif_step_summary(step);
    let (summary, truncated) = truncate_chars_with_flag(
        &redact_preview_text(&raw_summary),
        TRAJECTORY_DATA_PREVIEW_CHARS,
    );
    let token_total = step.metrics.as_ref().and_then(|metrics| {
        let values = [
            metrics.prompt_tokens,
            metrics.completion_tokens,
            metrics.cached_tokens,
            metrics.usage.as_ref().and_then(|usage| usage.total_tokens),
        ];
        let total = values.into_iter().flatten().sum::<u64>();
        (total > 0).then_some(total)
    });
    ViewTrajectoryStep {
        step_id: step.step_id,
        source: step.source.clone(),
        label: trajectory_step_label(step, &tool_names),
        summary,
        tool_names,
        tool_error,
        duration_ms: None,
        token_total,
        cost_usd: step.metrics.as_ref().and_then(|metrics| metrics.cost_usd),
        data_preview: step
            .extra
            .as_ref()
            .and_then(|extra| serde_json::to_string(extra).ok())
            .map(|value| {
                truncate_chars_with_flag(
                    &redact_preview_text(&value),
                    TRAJECTORY_DATA_PREVIEW_CHARS,
                )
                .0
            }),
        truncated,
    }
}

pub(crate) fn trajectory_step_label(step: &AtifStep, tool_names: &[String]) -> String {
    if !tool_names.is_empty() {
        return format!("tools: {}", tool_names.join(", "));
    }
    match step.source.as_str() {
        "user" => "user prompt".to_string(),
        "agent" => "agent step".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn atif_step_summary(step: &AtifStep) -> String {
    let mut parts = Vec::new();
    if let Some(text) = step.message.as_str()
        && !text.trim().is_empty()
    {
        parts.push(text.trim().to_string());
    }
    if let Some(reasoning) = &step.reasoning_content
        && !reasoning.trim().is_empty()
    {
        parts.push(format!("reasoning: {}", reasoning.trim()));
    }
    if !step.tool_calls.is_empty() {
        parts.push(format!(
            "tool calls: {}",
            step.tool_calls
                .iter()
                .map(|tool| tool.function_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(observation) = &step.observation {
        let observation_text = observation
            .results
            .iter()
            .filter_map(|result| result.content.as_ref())
            .filter_map(|content| match content {
                Value::String(text) => Some(text.clone()),
                value => serde_json::to_string(value).ok(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !observation_text.trim().is_empty() {
            parts.push(format!("observation: {}", observation_text.trim()));
        }
    }
    if parts.is_empty() {
        step.source.clone()
    } else {
        parts.join("\n")
    }
}

pub(crate) fn observation_result_is_error(result: &AtifObservationResult) -> bool {
    result
        .extra
        .as_ref()
        .and_then(|extra| extra.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| {
            status.eq_ignore_ascii_case("error") || status.eq_ignore_ascii_case("failed")
        })
}

pub(crate) fn trajectory_graph_from_steps(steps: &[ViewTrajectoryStep]) -> ViewTrajectoryGraph {
    let nodes = steps
        .iter()
        .map(|step| ViewTrajectoryGraphNode {
            id: format!("step-{}", step.step_id),
            step_id: step.step_id,
            label: step.label.clone(),
            source: step.source.clone(),
        })
        .collect::<Vec<_>>();
    let edges = nodes
        .windows(2)
        .map(|window| ViewTrajectoryGraphEdge {
            from: window[0].id.clone(),
            to: window[1].id.clone(),
        })
        .collect();
    ViewTrajectoryGraph { nodes, edges }
}

pub(crate) fn build_log_index(cell: &CellRun) -> ViewLogIndex {
    let mut files = Vec::new();
    let mut error = None;
    for rel in [
        cell.case.artifacts.evaluator_stdout.as_path(),
        cell.case.artifacts.evaluator_stderr.as_path(),
    ] {
        match artifact_file_from_relative(&cell.cell_root, rel, ArtifactFileMode::Log) {
            Ok(Some(file)) => files.push(file),
            Ok(None) => {}
            Err(err) => error = Some(format!("{err:#}")),
        }
    }
    let logs_dir = cell.cell_root.join("logs");
    if logs_dir.is_dir() {
        match list_artifact_files_under(&cell.cell_root, &logs_dir, ArtifactFileMode::Log) {
            Ok(mut logs) => files.append(&mut logs),
            Err(err) => error = Some(format!("{err:#}")),
        }
    }
    files.sort_by(|left, right| {
        left.data_ref
            .relative_path
            .cmp(&right.data_ref.relative_path)
    });
    ViewLogIndex {
        benchmark: cell.benchmark.clone(),
        trial_key: trial_key(cell),
        matrix_cell_key: matrix_cell_key(cell),
        files,
        error,
    }
}

pub(crate) fn build_analysis_report(cell: &CellRun) -> ViewAnalysisReport {
    let json = artifact_file_from_relative(
        &cell.cell_root,
        Path::new("analysis.json"),
        ArtifactFileMode::Analysis,
    )
    .ok()
    .flatten();
    let status = if json.is_some() { "cached" } else { "missing" }.to_string();
    let summary = json
        .as_ref()
        .and_then(|file| file.preview.as_deref())
        .and_then(analysis_summary_from_preview);
    ViewAnalysisReport {
        benchmark: cell.benchmark.clone(),
        trial_key: trial_key(cell),
        matrix_cell_key: matrix_cell_key(cell),
        status,
        json_ref: json.as_ref().map(|file| file.data_ref.clone()),
        json_preview: json.and_then(|file| file.preview),
        summary,
    }
}

pub(crate) fn build_diff_report(cell: &CellRun) -> ViewDiffReport {
    match discover_patch_artifacts(&cell.cell_root) {
        Ok(Some(file)) => {
            return ViewDiffReport {
                benchmark: cell.benchmark.clone(),
                trial_key: trial_key(cell),
                matrix_cell_key: matrix_cell_key(cell),
                source: "artifact".to_string(),
                data_ref: Some(file.data_ref),
                preview: file.preview,
                truncated: file.truncated,
                error: None,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return ViewDiffReport {
                benchmark: cell.benchmark.clone(),
                trial_key: trial_key(cell),
                matrix_cell_key: matrix_cell_key(cell),
                source: "error".to_string(),
                data_ref: None,
                preview: None,
                truncated: false,
                error: Some(format!("{err:#}")),
            };
        }
    }
    match read_trajectory_events(cell) {
        Ok(events) => {
            let mut diffs = Vec::new();
            for event in &events {
                collect_diff_strings(&event.data, &mut diffs);
            }
            if let Some(diff) = diffs.into_iter().next() {
                let redacted = redact_preview_text(&diff);
                let (preview, truncated) =
                    truncate_chars_with_flag(&redacted, VIEW_TEXT_PREVIEW_BYTES);
                ViewDiffReport {
                    benchmark: cell.benchmark.clone(),
                    trial_key: trial_key(cell),
                    matrix_cell_key: matrix_cell_key(cell),
                    source: "trajectory".to_string(),
                    data_ref: data_ref_for_relative(
                        &cell.cell_root,
                        &cell.case.artifacts.trajectory,
                        None,
                    )
                    .ok(),
                    preview: Some(preview),
                    truncated,
                    error: None,
                }
            } else {
                ViewDiffReport {
                    benchmark: cell.benchmark.clone(),
                    trial_key: trial_key(cell),
                    matrix_cell_key: matrix_cell_key(cell),
                    source: "missing".to_string(),
                    data_ref: None,
                    preview: None,
                    truncated: false,
                    error: None,
                }
            }
        }
        Err(err) => ViewDiffReport {
            benchmark: cell.benchmark.clone(),
            trial_key: trial_key(cell),
            matrix_cell_key: matrix_cell_key(cell),
            source: "error".to_string(),
            data_ref: None,
            preview: None,
            truncated: false,
            error: Some(format!("{err:#}")),
        },
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ArtifactFileMode {
    Artifact,
    Log,
    Analysis,
    Diff,
}

pub(crate) fn list_artifact_files(
    root: &Path,
    mode: ArtifactFileMode,
) -> Result<Vec<ViewArtifactFile>> {
    list_artifact_files_under(root, root, mode)
}

pub(crate) fn list_artifact_files_under(
    root: &Path,
    start: &Path,
    mode: ArtifactFileMode,
) -> Result<Vec<ViewArtifactFile>> {
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_start = fs::canonicalize(start)
        .with_context(|| format!("failed to canonicalize {}", start.display()))?;
    if !canonical_start.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_start.display(),
            canonical_root.display()
        );
    }
    let mut out = Vec::new();
    collect_artifact_files(&canonical_root, &canonical_start, mode, &mut out)?;
    out.sort_by(|left, right| {
        left.data_ref
            .relative_path
            .cmp(&right.data_ref.relative_path)
    });
    Ok(out)
}

pub(crate) fn collect_artifact_files(
    canonical_root: &Path,
    dir: &Path,
    mode: ArtifactFileMode,
    out: &mut Vec<ViewArtifactFile>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_artifact_files(canonical_root, &path, mode, out)?;
        } else if file_type.is_file() {
            out.push(artifact_file_from_canonical(canonical_root, &path, mode)?);
        }
    }
    Ok(())
}

pub(crate) fn artifact_file_from_relative(
    root: &Path,
    relative: &Path,
    mode: ArtifactFileMode,
) -> Result<Option<ViewArtifactFile>> {
    let path = root.join(relative);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    Ok(Some(artifact_file_from_canonical(
        &canonical_root,
        &canonical_path,
        mode,
    )?))
}

pub(crate) fn artifact_file_from_canonical(
    canonical_root: &Path,
    canonical_path: &Path,
    mode: ArtifactFileMode,
) -> Result<ViewArtifactFile> {
    if !canonical_path.starts_with(canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    let relative = canonical_path
        .strip_prefix(canonical_root)
        .with_context(|| {
            format!(
                "failed to relativize {} under {}",
                canonical_path.display(),
                canonical_root.display()
            )
        })?
        .to_path_buf();
    let mut data_ref = data_ref_for_canonical(canonical_path, &relative)?;
    data_ref.kind = artifact_kind(&relative);
    data_ref.label = relative.display().to_string();
    let is_image = data_ref.mime.starts_with("image/");
    let allow_preview = matches!(mode, ArtifactFileMode::Analysis | ArtifactFileMode::Diff);
    let (preview, truncated, previewable) = if allow_preview {
        read_text_preview(canonical_path)?
    } else {
        (None, false, false)
    };
    let inline_data_url = if matches!(mode, ArtifactFileMode::Artifact)
        && is_image
        && data_ref.size_bytes <= SMALL_IMAGE_INLINE_BYTES
    {
        inline_file_data_url(canonical_path, &data_ref.mime)
            .ok()
            .flatten()
    } else {
        None
    };
    Ok(ViewArtifactFile {
        data_ref,
        previewable,
        truncated,
        preview,
        inline_data_url,
    })
}

pub(crate) fn data_ref_for_relative(
    root: &Path,
    relative: &Path,
    kind_override: Option<&str>,
) -> Result<ViewDataRef> {
    let path = root.join(relative);
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    let relative = canonical_path
        .strip_prefix(canonical_root)
        .with_context(|| format!("failed to relativize {}", canonical_path.display()))?;
    let mut data_ref = data_ref_for_canonical(&canonical_path, relative)?;
    if let Some(kind) = kind_override {
        data_ref.kind = kind.to_string();
    }
    Ok(data_ref)
}

pub(crate) fn data_ref_for_canonical(
    canonical_path: &Path,
    relative: &Path,
) -> Result<ViewDataRef> {
    let metadata = fs::metadata(canonical_path)
        .with_context(|| format!("failed to stat {}", canonical_path.display()))?;
    let content_hash = if metadata.len() <= VIEW_TEXT_PREVIEW_BYTES as u64 {
        fs::read(canonical_path)
            .ok()
            .map(|bytes| stable_hash_bytes(&bytes))
    } else {
        None
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    Ok(ViewDataRef {
        kind: artifact_kind(relative),
        label: relative.display().to_string(),
        relative_path: relative.to_path_buf(),
        mime: mime_for_path(relative).to_string(),
        size_bytes: metadata.len(),
        content_hash,
        modified_ms,
    })
}

pub(crate) fn missing_data_ref(kind: &str, relative: &Path) -> ViewDataRef {
    ViewDataRef {
        kind: kind.to_string(),
        label: relative.display().to_string(),
        relative_path: relative.to_path_buf(),
        mime: mime_for_path(relative).to_string(),
        size_bytes: 0,
        content_hash: None,
        modified_ms: None,
    }
}

pub(crate) fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("md") | Some("markdown") => "text/markdown",
        Some("json") => "application/json",
        Some("jsonl") => "application/jsonl",
        Some("txt") | Some("log") => "text/plain",
        Some("diff") | Some("patch") => "text/x-diff",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

pub(crate) fn inline_file_data_url(path: &Path, mime: &str) -> Result<Option<String>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() as u64 > SMALL_IMAGE_INLINE_BYTES {
        return Ok(None);
    }
    Ok(Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

pub(crate) fn read_text_preview(path: &Path) -> Result<(Option<String>, bool, bool)> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::new();
    let limit = VIEW_TEXT_PREVIEW_BYTES + 1;
    file.by_ref()
        .take(limit as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let truncated = bytes.len() > VIEW_TEXT_PREVIEW_BYTES;
    if truncated {
        bytes.truncate(VIEW_TEXT_PREVIEW_BYTES);
    }
    if bytes.contains(&0) {
        return Ok((None, truncated, false));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok((None, truncated, false));
    };
    Ok((Some(redact_preview_text(&text)), truncated, true))
}

pub(crate) fn artifact_kind(path: &Path) -> String {
    if path == Path::new("run.json") {
        return "run".to_string();
    }
    if path == Path::new("trajectory.jsonl") {
        return "trajectory".to_string();
    }
    if path == Path::new("evaluator.stdout") || path == Path::new("evaluator.stderr") {
        return "verifier-log".to_string();
    }
    if path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == "logs")
    {
        return "log".to_string();
    }
    if path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == "workspace")
    {
        return "workspace".to_string();
    }
    match path.extension().and_then(|value| value.to_str()) {
        Some("diff") | Some("patch") => "diff".to_string(),
        Some("md") => "markdown".to_string(),
        Some("json") | Some("jsonl") => "json".to_string(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp") => {
            "image".to_string()
        }
        _ => "file".to_string(),
    }
}

pub(crate) fn read_trajectory_events(cell: &CellRun) -> Result<Vec<TrajectoryEvent>> {
    let path = cell.cell_root.join(&cell.case.artifacts.trajectory);
    let safe = safe_artifact_path(&cell.cell_root, &path)?;
    let file = fs::File::open(&safe)
        .with_context(|| format!("failed to open trajectory {}", safe.display()))?;
    let mut events = Vec::new();
    for (line_no, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("failed to read {}", safe.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<TrajectoryEvent>(&line).with_context(|| {
            format!(
                "failed to parse trajectory event {} line {}",
                safe.display(),
                line_no + 1
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

pub(crate) fn safe_artifact_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&joined)
        .with_context(|| format!("failed to canonicalize {}", joined.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    Ok(canonical_path)
}

pub(crate) fn derive_atif_trajectory(cell: &CellRun, events: &[TrajectoryEvent]) -> AtifTrajectory {
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
        extra: prompt_unavailable.then(|| json!({ "prompt_unavailable": true })),
        llm_call_count: None,
    });
    next_step_id += 1;

    if cell.case.candidate.adapter == AgentKind::Acp {
        if let Some(step) = derive_acp_atif_step(next_step_id, events) {
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
        "matrix_cell_key": matrix_cell_key(cell),
        "trial_key": trial_key(cell),
    });
    if prompt_unavailable {
        extra["prompt_unavailable"] = Value::Bool(true);
    }
    let total_steps = steps.len() as u64;
    AtifTrajectory {
        schema_version: "ATIF-v1.7".to_string(),
        session_id: Some(cell.case.case_id.clone()),
        trajectory_id: Some(trial_key(cell)),
        agent: AtifAgent {
            name: cell.case.agent_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            model_name: cell.case.candidate.model.clone(),
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
    let path = cell.cell_root.join("workspace/.peval/prompt.md");
    if !path.exists() {
        return Ok(None);
    }
    let safe = safe_artifact_path(&cell.cell_root, &path)?;
    let (preview, _, previewable) = read_text_preview(&safe)?;
    Ok(previewable.then_some(preview).flatten())
}

pub(crate) fn derive_acp_atif_step(step_id: u64, events: &[TrajectoryEvent]) -> Option<AtifStep> {
    let mut reasoning = String::new();
    let mut message = String::new();
    let mut tool_calls = Vec::new();
    let mut observations = Vec::new();
    for event in events {
        let Some(update) = event
            .data
            .get("raw_event")
            .and_then(|raw| raw.get("params"))
            .and_then(|params| params.get("update"))
        else {
            continue;
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_thought_chunk") => {
                if let Some(text) = acp_content_text(update.get("content").unwrap_or(&Value::Null))
                {
                    reasoning.push_str(&text);
                }
            }
            Some("agent_message_chunk") => {
                if let Some(text) = acp_content_text(update.get("content").unwrap_or(&Value::Null))
                {
                    message.push_str(&text);
                }
            }
            Some("tool_call") => {
                let tool_call_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("tool-call")
                    .to_string();
                let function_name = update
                    .get("kind")
                    .and_then(Value::as_str)
                    .or_else(|| update.get("title").and_then(Value::as_str))
                    .unwrap_or("tool")
                    .to_string();
                let arguments = update
                    .get("rawInput")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                tool_calls.push(AtifToolCall {
                    tool_call_id,
                    function_name,
                    arguments,
                    extra: Some(json!({
                        "status": update.get("status").cloned().unwrap_or(Value::Null),
                        "title": update.get("title").cloned().unwrap_or(Value::Null),
                    })),
                });
            }
            Some("tool_call_update") => {
                let tool_call_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let (content, truncated) = bounded_atif_content(
                    update
                        .get("rawOutput")
                        .or_else(|| update.get("content"))
                        .unwrap_or(&Value::Null),
                );
                observations.push(AtifObservationResult {
                    source_call_id: tool_call_id,
                    content: Some(content),
                    extra: Some(json!({
                        "status": update.get("status").cloned().unwrap_or(Value::Null),
                        "title": update.get("title").cloned().unwrap_or(Value::Null),
                        "truncated": truncated,
                    })),
                });
            }
            _ => {}
        }
    }
    if reasoning.is_empty()
        && message.is_empty()
        && tool_calls.is_empty()
        && observations.is_empty()
    {
        return None;
    }
    Some(AtifStep {
        step_id,
        source: "agent".to_string(),
        message: Value::String(message),
        reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
        tool_calls,
        observation: (!observations.is_empty()).then_some(AtifObservation {
            results: observations,
        }),
        metrics: acp_atif_metrics(events),
        extra: Some(json!({ "source": "acp" })),
        llm_call_count: Some(1),
    })
}

pub(crate) fn acp_atif_metrics(events: &[TrajectoryEvent]) -> Option<AtifMetrics> {
    events
        .iter()
        .find(|event| event.kind == "acp_agent_prompt_finished")
        .and_then(|event| event.data.get("prompt_result"))
        .and_then(atif_metrics_from_value)
}

pub(crate) fn derive_jsonl_atif_step(step_id: u64, event: &TrajectoryEvent) -> Option<AtifStep> {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    let event_type = raw
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(&event.kind)
        .to_ascii_lowercase();
    if matches!(event_type.as_str(), "user_message" | "user" | "input") {
        return Some(AtifStep {
            step_id,
            source: "user".to_string(),
            message: Value::String(event_text(raw)),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: None,
            metrics: None,
            extra: Some(json!({ "source_event": event.kind })),
            llm_call_count: None,
        });
    }
    if matches!(
        event_type.as_str(),
        "assistant_message" | "assistant" | "message" | "output"
    ) {
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(event_text(raw)),
            reasoning_content: raw
                .get("reasoning")
                .or_else(|| raw.get("reasoning_content"))
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_calls: Vec::new(),
            observation: None,
            metrics: atif_metrics_from_value(raw),
            extra: Some(json!({ "source_event": event.kind })),
            llm_call_count: Some(1),
        });
    }
    if matches!(event_type.as_str(), "tool_call" | "tool_execution_start") {
        let tool_call = AtifToolCall {
            tool_call_id: raw
                .get("tool_call_id")
                .or_else(|| raw.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            function_name: raw
                .get("function_name")
                .or_else(|| raw.get("name"))
                .or_else(|| raw.get("tool"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: raw
                .get("arguments")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({})),
            extra: Some(json!({ "source_event": event.kind })),
        };
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(String::new()),
            reasoning_content: None,
            tool_calls: vec![tool_call],
            observation: None,
            metrics: None,
            extra: Some(json!({ "source_event": event.kind })),
            llm_call_count: Some(0),
        });
    }
    if matches!(event_type.as_str(), "tool_result" | "tool_execution_end") {
        let (content, truncated) = bounded_atif_content(
            raw.get("result")
                .or_else(|| raw.get("output"))
                .unwrap_or(raw),
        );
        return Some(AtifStep {
            step_id,
            source: "agent".to_string(),
            message: Value::String(String::new()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: Some(AtifObservation {
                results: vec![AtifObservationResult {
                    source_call_id: raw
                        .get("tool_call_id")
                        .or_else(|| raw.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    content: Some(content),
                    extra: Some(json!({
                        "source_event": event.kind,
                        "truncated": truncated,
                    })),
                }],
            }),
            metrics: None,
            extra: Some(json!({ "source_event": event.kind })),
            llm_call_count: Some(0),
        });
    }
    None
}

pub(crate) fn acp_content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    None
}

pub(crate) fn event_text(value: &Value) -> String {
    for key in ["message", "text", "content", "prompt", "output"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    String::new()
}

pub(crate) fn bounded_atif_content(value: &Value) -> (Value, bool) {
    match value {
        Value::String(text) => {
            let (preview, truncated) = truncate_chars_with_flag(text, ATIF_CONTENT_PREVIEW_CHARS);
            (Value::String(redact_preview_text(&preview)), truncated)
        }
        _ => {
            let raw = serde_json::to_string(value).unwrap_or_default();
            let (preview, truncated) = truncate_chars_with_flag(&raw, ATIF_CONTENT_PREVIEW_CHARS);
            if truncated {
                (Value::String(redact_preview_text(&preview)), true)
            } else {
                (value.clone(), false)
            }
        }
    }
}

pub(crate) fn atif_metrics_from_value(value: &Value) -> Option<AtifMetrics> {
    let usage_source = value
        .get("usage")
        .or_else(|| value.get("_meta").and_then(|meta| meta.get("usage")));
    let accounting_source = value.get("accounting").or_else(|| {
        value
            .get("_meta")
            .and_then(|meta| meta.get("psychevo"))
            .and_then(|psychevo| psychevo.get("accounting"))
    });
    let source = usage_source.or(accounting_source).unwrap_or(value);
    let prompt_tokens = source
        .get("prompt_tokens")
        .or_else(|| source.get("input_tokens"))
        .or_else(|| source.get("context_input_tokens"))
        .or_else(|| source.get("billable_input_tokens"))
        .and_then(json_u64);
    let completion_tokens = source
        .get("completion_tokens")
        .or_else(|| source.get("output_tokens"))
        .or_else(|| source.get("billable_output_tokens"))
        .and_then(json_u64);
    let cached_tokens = source
        .get("cached_tokens")
        .or_else(|| source.get("cached_read_tokens"))
        .or_else(|| source.get("cache_read_tokens"))
        .or_else(|| source.get("cached_input_tokens"))
        .and_then(json_u64);
    let cost_usd = source
        .get("cost_usd")
        .or_else(|| source.get("amount_usd"))
        .or_else(|| source.get("total_cost_usd"))
        .and_then(Value::as_f64);
    let usage = usage_source.map(usage_metrics_from_value);
    let accounting = accounting_source
        .map(accounting_metrics_from_value)
        .filter(has_accounting);
    let turns = value
        .get("_meta")
        .and_then(|meta| meta.get("psychevo"))
        .and_then(|psychevo| psychevo.get("turns"))
        .and_then(json_u64);
    (prompt_tokens.is_some()
        || completion_tokens.is_some()
        || cached_tokens.is_some()
        || cost_usd.is_some()
        || turns.is_some()
        || usage.as_ref().is_some_and(has_usage)
        || accounting.is_some())
    .then_some(AtifMetrics {
        prompt_tokens,
        completion_tokens,
        cached_tokens,
        cost_usd,
        turns,
        tool_calls: None,
        tool_errors: None,
        usage,
        accounting,
        extra: None,
    })
}

pub(crate) fn atif_final_metrics(cell: &CellRun, total_steps: u64) -> AtifFinalMetrics {
    let usage = has_usage(&cell.case.metrics.usage).then(|| cell.case.metrics.usage.clone());
    let accounting =
        has_accounting(&cell.case.metrics.accounting).then(|| cell.case.metrics.accounting.clone());
    AtifFinalMetrics {
        total_prompt_tokens: cell.case.metrics.usage.input_tokens,
        total_completion_tokens: cell.case.metrics.usage.output_tokens,
        total_cached_tokens: cell.case.metrics.usage.cache_read_tokens,
        total_cost_usd: cell.case.metrics.cost.amount_usd,
        total_turns: cell.case.metrics.turns,
        total_tool_calls: cell.case.metrics.tool_calls,
        total_tool_errors: cell.case.metrics.tool_errors,
        usage,
        accounting,
        total_steps,
        extra: Some(json!({
            "duration_ms": cell.case.metrics.duration_ms,
            "tool_calls": cell.case.metrics.tool_calls,
            "tool_errors": cell.case.metrics.tool_errors,
            "score": cell.case.score.score,
            "passed": cell.case.score.passed,
        })),
    }
}

pub(crate) fn usage_metrics_from_value(value: &Value) -> UsageMetrics {
    UsageMetrics {
        input_tokens: value
            .get("input_tokens")
            .or_else(|| value.get("prompt_tokens"))
            .or_else(|| value.get("context_input_tokens"))
            .and_then(json_u64),
        output_tokens: value
            .get("output_tokens")
            .or_else(|| value.get("completion_tokens"))
            .and_then(json_u64),
        cache_read_tokens: value
            .get("cached_read_tokens")
            .or_else(|| value.get("cache_read_tokens"))
            .or_else(|| value.get("cached_tokens"))
            .or_else(|| value.get("cached_input_tokens"))
            .and_then(json_u64),
        cache_write_tokens: value
            .get("cached_write_tokens")
            .or_else(|| value.get("cache_write_tokens"))
            .and_then(json_u64),
        reasoning_tokens: value
            .get("thought_tokens")
            .or_else(|| value.get("reasoning_tokens"))
            .and_then(json_u64),
        total_tokens: value
            .get("total_tokens")
            .or_else(|| value.get("reported_total_tokens"))
            .and_then(json_u64),
    }
}

pub(crate) fn accounting_metrics_from_value(value: &Value) -> AccountingMetrics {
    AccountingMetrics {
        context_input_tokens: value.get("context_input_tokens").and_then(json_u64),
        billable_input_tokens: value.get("billable_input_tokens").and_then(json_u64),
        billable_output_tokens: value.get("billable_output_tokens").and_then(json_u64),
        reasoning_tokens: value.get("reasoning_tokens").and_then(json_u64),
        cache_read_tokens: value.get("cache_read_tokens").and_then(json_u64),
        cache_write_tokens: value.get("cache_write_tokens").and_then(json_u64),
        reported_total_tokens: value.get("reported_total_tokens").and_then(json_u64),
        estimated_cost_nanodollars: value.get("estimated_cost_nanodollars").and_then(json_i64),
        pricing_source: value
            .get("pricing_source")
            .and_then(Value::as_str)
            .map(str::to_string),
        pricing_tier: value
            .get("pricing_tier")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

pub(crate) fn discover_patch_artifacts(root: &Path) -> Result<Option<ViewArtifactFile>> {
    let files = list_artifact_files(root, ArtifactFileMode::Diff)?;
    Ok(files.into_iter().find(|file| {
        file.data_ref
            .relative_path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext == "diff" || ext == "patch")
    }))
}

pub(crate) fn collect_diff_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "diff" {
                    if let Some(diff) = value.as_str() {
                        out.push(diff.to_string());
                    }
                } else {
                    collect_diff_strings(value, out);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_diff_strings(value, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn render_view(report: &ViewReport, format: ViewFormat) -> Result<String> {
    match format {
        ViewFormat::Json => Ok(serde_json::to_string_pretty(report)?),
        ViewFormat::Markdown => render_view_markdown(report),
        ViewFormat::Html => Ok(render_view_html(report)),
    }
}

pub(crate) fn render_view_markdown(report: &ViewReport) -> Result<String> {
    let mut out = String::new();
    out.push_str("# peval view\n\n");
    if report.includes.contains(&ViewInclude::Summary) {
        out.push_str("## Summary\n\n");
        out.push_str(&format!(
            "- scope: `{}`\n- trials: {}\n- passed: {}\n- failed: {}\n- status: {:?}\n\n",
            report.scope.path.display(),
            report.summary.total_trials,
            report.summary.passed_trials,
            report.summary.failed_trials,
            report.summary.status,
        ));
    }
    render_groups_markdown(report, &mut out);
    if report.includes.contains(&ViewInclude::Matrix) {
        render_matrix_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Usage) {
        render_usage_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Warnings) {
        render_warnings_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Artifacts) {
        render_artifacts_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Trajectory) {
        render_trajectory_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Atif) {
        render_atif_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Logs) {
        render_logs_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Analysis) {
        render_analysis_markdown(report, &mut out);
    }
    if report.includes.contains(&ViewInclude::Diff) {
        render_diff_markdown(report, &mut out);
    }
    Ok(out)
}

pub(crate) fn render_groups_markdown(report: &ViewReport, out: &mut String) {
    if report.groups.is_empty() {
        return;
    }
    out.push_str("## Groups\n\n");
    out.push_str("| group | status | passed | failed | total |\n");
    out.push_str("| --- | --- | ---: | ---: | ---: |\n");
    for row in &report.groups {
        out.push_str(&format!(
            "| `{}` | {:?} | {} | {} | {} |\n",
            escape_markdown_table(&row.key),
            row.status,
            row.passed_trials,
            row.failed_trials,
            row.total_trials,
        ));
    }
    out.push('\n');
}

pub(crate) fn render_matrix_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Matrix\n\n");
    out.push_str("| benchmark | matrix cell | trial | task set | task | agent | adapter | status | score | duration ms | turns | tools | tool errors |\n");
    out.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for row in &report.matrix.cells {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | {:?} | {:?} | {} | {} | {} | {} | {} |\n",
            escape_markdown_table(&row.benchmark),
            escape_markdown_table(&row.matrix_cell_key),
            escape_markdown_table(&row.representative_trial_key),
            escape_markdown_table(&row.task_set_id),
            escape_markdown_table(&row.task_id),
            escape_markdown_table(&row.agent_id),
            row.adapter,
            row.status,
            option_f64(row.score),
            row.duration_ms,
            option_u64(row.turns),
            row.tool_calls,
            row.tool_errors,
        ));
    }
    out.push('\n');
}

pub(crate) fn render_usage_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Usage\n\n");
    out.push_str("| benchmark | trial | case | agent | input | output | cache read | cache write | reasoning | total | cost usd |\n");
    out.push_str("| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for row in &report.usage {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
            escape_markdown_table(&row.benchmark),
            escape_markdown_table(&row.trial_key),
            escape_markdown_table(&row.case_id),
            escape_markdown_table(&row.agent_id),
            option_u64(row.input_tokens),
            option_u64(row.output_tokens),
            option_u64(row.cache_read_tokens),
            option_u64(row.cache_write_tokens),
            option_u64(row.reasoning_tokens),
            option_u64(row.total_tokens),
            option_f64(row.cost_usd),
        ));
    }
    out.push('\n');
    if report
        .usage
        .iter()
        .any(|row| has_accounting(&row.accounting))
    {
        out.push_str("### Accounting\n\n");
        out.push_str("| benchmark | trial | context input | billable input | billable output | cache read | cache write | reasoning | reported total | cost nanodollars | pricing source | pricing tier |\n");
        out.push_str(
            "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |\n",
        );
        for row in &report.usage {
            let accounting = &row.accounting;
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | `{}` | `{}` |\n",
                escape_markdown_table(&row.benchmark),
                escape_markdown_table(&row.trial_key),
                option_u64(accounting.context_input_tokens),
                option_u64(accounting.billable_input_tokens),
                option_u64(accounting.billable_output_tokens),
                option_u64(accounting.cache_read_tokens),
                option_u64(accounting.cache_write_tokens),
                option_u64(accounting.reasoning_tokens),
                option_u64(accounting.reported_total_tokens),
                accounting
                    .estimated_cost_nanodollars
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                escape_markdown_table(accounting.pricing_source.as_deref().unwrap_or("-")),
                escape_markdown_table(accounting.pricing_tier.as_deref().unwrap_or("-")),
            ));
        }
        out.push('\n');
    }
}

pub(crate) fn render_warnings_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Warnings\n\n");
    if report.warnings.is_empty() {
        out.push_str("No warnings.\n\n");
        return;
    }
    out.push_str("| benchmark | trial | case | agent | warning |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for row in &report.warnings {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            escape_markdown_table(&row.benchmark),
            escape_markdown_table(&row.trial_key),
            escape_markdown_table(&row.case_id),
            escape_markdown_table(&row.agent_id),
            escape_markdown_table(&row.warning),
        ));
    }
    out.push('\n');
}

pub(crate) fn render_artifacts_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Artifacts\n\n");
    out.push_str("| trial | path | kind | mime | bytes | inline |\n");
    out.push_str("| --- | --- | --- | --- | ---: | --- |\n");
    for index in &report.artifacts {
        if let Some(error) = &index.error {
            out.push_str(&format!(
                "| `{}` | - | error | - | 0 | {} |\n",
                escape_markdown_table(&index.trial_key),
                escape_markdown_table(error),
            ));
        }
        for file in &index.files {
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} | {} |\n",
                escape_markdown_table(&index.trial_key),
                escape_markdown_table(&file.data_ref.relative_path.display().to_string()),
                escape_markdown_table(&file.data_ref.kind),
                escape_markdown_table(&file.data_ref.mime),
                file.data_ref.size_bytes,
                if file.inline_data_url.is_some() {
                    "small image"
                } else {
                    "-"
                },
            ));
        }
    }
    out.push('\n');
}

pub(crate) fn render_trajectory_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Trajectory\n\n");
    out.push_str("| trial | steps | events | unmapped | duration ms | tools | tool errors | tokens | cost |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for trajectory in &report.trajectory {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            escape_markdown_table(&trajectory.trial_key),
            trajectory.total_steps,
            trajectory.total_events,
            trajectory.unmapped_events,
            trajectory.duration_ms,
            trajectory.tool_calls,
            trajectory.tool_errors,
            option_u64(trajectory.token_total),
            option_f64(trajectory.cost_usd),
        ));
        if let Some(error) = &trajectory.error {
            out.push_str(&format!(
                "| `{}` | - | - | - | - | - | - | - | {} |\n",
                escape_markdown_table(&trajectory.trial_key),
                escape_markdown_table(error),
            ));
        }
    }
    out.push('\n');
    out.push_str("| trial | step | source | label | summary | tools | tokens | cost |\n");
    out.push_str("| --- | ---: | --- | --- | --- | --- | ---: | ---: |\n");
    for trajectory in &report.trajectory {
        for step in &trajectory.steps {
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {} | {} | {} | {} | {} |\n",
                escape_markdown_table(&trajectory.trial_key),
                step.step_id,
                escape_markdown_table(&step.source),
                escape_markdown_table(&step.label),
                markdown_preview_cell(Some(&step.summary), step.truncated),
                escape_markdown_table(&step.tool_names.join(", ")),
                option_u64(step.token_total),
                option_f64(step.cost_usd),
            ));
        }
    }
    out.push('\n');
}

pub(crate) fn render_atif_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## ATIF\n\n");
    out.push_str("| trial | schema | steps | agent | model | notes |\n");
    out.push_str("| --- | --- | ---: | --- | --- | --- |\n");
    for atif in &report.atif {
        out.push_str(&format!(
            "| `{}` | `{}` | {} | `{}` | `{}` | {} |\n",
            escape_markdown_table(&atif.trial_key),
            escape_markdown_table(&atif.trajectory.schema_version),
            atif.trajectory.steps.len(),
            escape_markdown_table(&atif.trajectory.agent.name),
            escape_markdown_table(atif.trajectory.agent.model_name.as_deref().unwrap_or("-")),
            escape_markdown_table(atif.error.as_deref().unwrap_or("-")),
        ));
    }
    out.push('\n');
}

pub(crate) fn render_logs_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Logs\n\n");
    out.push_str("| trial | path | bytes | ref |\n");
    out.push_str("| --- | --- | ---: | --- |\n");
    for logs in &report.logs {
        if let Some(error) = &logs.error {
            out.push_str(&format!(
                "| `{}` | error | 0 | {} |\n",
                escape_markdown_table(&logs.trial_key),
                escape_markdown_table(error),
            ));
        }
        for file in &logs.files {
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} |\n",
                escape_markdown_table(&logs.trial_key),
                escape_markdown_table(&file.data_ref.relative_path.display().to_string()),
                file.data_ref.size_bytes,
                escape_markdown_table(&file.data_ref.kind),
            ));
        }
    }
    out.push('\n');
}

pub(crate) fn render_analysis_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Analysis\n\n");
    out.push_str("| trial | status | summary | json ref |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for analysis in &report.analysis {
        out.push_str(&format!(
            "| `{}` | `{}` | {} | `{}` |\n",
            escape_markdown_table(&analysis.trial_key),
            escape_markdown_table(&analysis.status),
            markdown_preview_cell(analysis.summary.as_deref(), false),
            escape_markdown_table(
                &analysis
                    .json_ref
                    .as_ref()
                    .map(|data_ref| data_ref.relative_path.display().to_string())
                    .unwrap_or_else(|| "-".to_string())
            ),
        ));
    }
    out.push('\n');
}

pub(crate) fn render_diff_markdown(report: &ViewReport, out: &mut String) {
    out.push_str("## Diff\n\n");
    out.push_str("| trial | source | path | preview |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for diff in &report.diff {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} |\n",
            escape_markdown_table(&diff.trial_key),
            escape_markdown_table(&diff.source),
            escape_markdown_table(
                &diff
                    .data_ref
                    .as_ref()
                    .map(|data_ref| data_ref.relative_path.display().to_string())
                    .unwrap_or_else(|| "-".to_string())
            ),
            markdown_preview_cell(diff.preview.as_deref(), diff.truncated),
        ));
    }
    out.push('\n');
}

pub(crate) fn render_view_html(report: &ViewReport) -> String {
    let mut sections = String::new();
    if report.includes.contains(&ViewInclude::Matrix) {
        sections.push_str(&render_matrix_html(report));
    }
    if report.includes.contains(&ViewInclude::Usage) {
        sections.push_str(&render_usage_html(report));
    }
    if report.includes.contains(&ViewInclude::Warnings) {
        sections.push_str(&render_warnings_html(report));
    }
    if report.includes.contains(&ViewInclude::Artifacts) {
        sections.push_str(&render_artifacts_html(report));
    }
    if report.includes.contains(&ViewInclude::Trajectory) {
        sections.push_str(&render_trajectory_html(report));
    }
    if report.includes.contains(&ViewInclude::Atif) {
        sections.push_str(&render_atif_html(report));
    }
    if report.includes.contains(&ViewInclude::Logs) {
        sections.push_str(&render_logs_html(report));
    }
    if report.includes.contains(&ViewInclude::Analysis) {
        sections.push_str(&render_analysis_html(report));
    }
    if report.includes.contains(&ViewInclude::Diff) {
        sections.push_str(&render_diff_html(report));
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval view</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">peval view</p><h1>{}</h1><p class=\"subline\">{} trials · {} passed · {} failed · includes {}</p></div><div class=\"verdict {}\">{:?}</div></section>{}</main></body></html>",
        report_css(),
        escape_html(
            report
                .scope
                .benchmark
                .as_deref()
                .unwrap_or("evaluation cells")
        ),
        report.summary.total_trials,
        report.summary.passed_trials,
        report.summary.failed_trials,
        escape_html(&include_list(report)),
        status_class_for_run(report.summary.status),
        report.summary.status,
        sections,
    )
}

pub(crate) fn render_matrix_html(report: &ViewReport) -> String {
    let rows = report
        .matrix
        .cells
        .iter()
        .map(|row| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:?}</td><td><span class=\"stamp {}\">{:?}</span></td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                escape_html(&row.matrix_cell_key),
                escape_html(&row.representative_trial_key),
                escape_html(&row.task_set_id),
                escape_html(&row.task_id),
                escape_html(&row.agent_id),
                row.adapter,
                status_class_for_case(row.status),
                row.status,
                option_f64(row.score),
                row.duration_ms,
            )
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Matrix</h2><table><thead><tr><th>matrix cell</th><th>trial</th><th>task set</th><th>task</th><th>agent</th><th>adapter</th><th>status</th><th>score</th><th>duration</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_usage_html(report: &ViewReport) -> String {
    let rows = report
        .usage
        .iter()
        .map(|row| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                escape_html(&row.trial_key),
                escape_html(&row.case_id),
                escape_html(&row.agent_id),
                option_u64(row.input_tokens),
                option_u64(row.output_tokens),
                option_u64(row.cache_read_tokens),
                option_u64(row.total_tokens),
                option_f64(row.cost_usd),
            )
        })
        .collect::<String>();
    let accounting = if report
        .usage
        .iter()
        .any(|row| has_accounting(&row.accounting))
    {
        let rows = report
            .usage
            .iter()
            .map(|row| {
                let accounting = &row.accounting;
                format!(
                    "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{}</td><td>{}</td></tr>",
                    escape_html(&row.trial_key),
                    escape_html(&row.case_id),
                    option_u64(accounting.context_input_tokens),
                    option_u64(accounting.billable_input_tokens),
                    option_u64(accounting.billable_output_tokens),
                    option_u64(accounting.cache_read_tokens),
                    option_u64(accounting.cache_write_tokens),
                    option_u64(accounting.reasoning_tokens),
                    accounting
                        .estimated_cost_nanodollars
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    escape_html(accounting.pricing_source.as_deref().unwrap_or("-")),
                    escape_html(accounting.pricing_tier.as_deref().unwrap_or("-")),
                )
            })
            .collect::<String>();
        format!(
            "<h3>Accounting</h3><table><thead><tr><th>trial</th><th>case</th><th>context input</th><th>billable input</th><th>billable output</th><th>cache read</th><th>cache write</th><th>reasoning</th><th>cost nanos</th><th>pricing source</th><th>pricing tier</th></tr></thead><tbody>{rows}</tbody></table>"
        )
    } else {
        String::new()
    };
    format!(
        "<section class=\"ledger\"><h2>Usage</h2><table><thead><tr><th>trial</th><th>case</th><th>agent</th><th>input</th><th>output</th><th>cache read</th><th>total</th><th>cost usd</th></tr></thead><tbody>{rows}</tbody></table>{accounting}</section>"
    )
}

pub(crate) fn render_warnings_html(report: &ViewReport) -> String {
    let rows = if report.warnings.is_empty() {
        "<tr><td colspan=\"5\">No warnings.</td></tr>".to_string()
    } else {
        report
            .warnings
            .iter()
            .map(|row| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape_html(&row.benchmark),
                    escape_html(&row.trial_key),
                    escape_html(&row.case_id),
                    escape_html(&row.agent_id),
                    escape_html(&row.warning),
                )
            })
            .collect::<String>()
    };
    format!(
        "<section class=\"ledger\"><h2>Warnings</h2><table><thead><tr><th>benchmark</th><th>trial</th><th>case</th><th>agent</th><th>warning</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_artifacts_html(report: &ViewReport) -> String {
    let rows = report
        .artifacts
        .iter()
        .flat_map(|index| {
            index.files.iter().map(|file| {
                let inline = file.inline_data_url.as_ref().map(|data_url| {
                    format!(
                        "<img class=\"artifact-img\" alt=\"{}\" src=\"{}\">",
                        escape_html(&file.data_ref.label),
                        escape_html(data_url)
                    )
                }).unwrap_or_else(|| "-".to_string());
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
                    escape_html(&index.trial_key),
                    escape_html(&file.data_ref.relative_path.display().to_string()),
                    escape_html(&file.data_ref.kind),
                    escape_html(&file.data_ref.mime),
                    file.data_ref.size_bytes,
                    inline,
                )
            })
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Artifacts</h2><table><thead><tr><th>trial</th><th>path</th><th>kind</th><th>mime</th><th>bytes</th><th>inline</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_trajectory_html(report: &ViewReport) -> String {
    let summary_rows = report
        .trajectory
        .iter()
        .map(|trajectory| {
            format!(
                "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                escape_html(&trajectory.trial_key),
                trajectory.total_steps,
                trajectory.total_events,
                trajectory.unmapped_events,
                trajectory.duration_ms,
                trajectory.tool_calls,
                trajectory.tool_errors,
                option_u64(trajectory.token_total),
            )
        })
        .collect::<String>();
    let cards = report
        .trajectory
        .iter()
        .map(render_trajectory_card_html)
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Trajectory</h2><table><thead><tr><th>trial</th><th>steps</th><th>events</th><th>unmapped</th><th>duration</th><th>tools</th><th>tool errors</th><th>tokens</th></tr></thead><tbody>{summary_rows}</tbody></table>{cards}</section>"
    )
}

pub(crate) fn render_atif_html(report: &ViewReport) -> String {
    let rows = report
        .atif
        .iter()
        .map(|atif| {
            format!(
                "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&atif.trial_key),
                escape_html(&atif.trajectory.schema_version),
                atif.trajectory.steps.len(),
                escape_html(&atif.trajectory.agent.name),
                escape_html(atif.trajectory.agent.model_name.as_deref().unwrap_or("-")),
                escape_html(atif.error.as_deref().unwrap_or("-")),
            )
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>ATIF</h2><table><thead><tr><th>trial</th><th>schema</th><th>steps</th><th>agent</th><th>model</th><th>notes</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_logs_html(report: &ViewReport) -> String {
    let rows = report
        .logs
        .iter()
        .flat_map(|logs| {
            logs.files.iter().map(|file| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
                    escape_html(&logs.trial_key),
                    escape_html(&file.data_ref.relative_path.display().to_string()),
                    file.data_ref.size_bytes,
                    escape_html(&file.data_ref.kind),
                )
            })
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Logs</h2><table><thead><tr><th>trial</th><th>path</th><th>bytes</th><th>ref</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_analysis_html(report: &ViewReport) -> String {
    let rows = report
        .analysis
        .iter()
        .map(|analysis| {
            format!(
                "<tr><td>{}</td><td><span class=\"stamp {}\">{}</span></td><td><pre>{}</pre></td><td><pre>{}</pre></td></tr>",
                escape_html(&analysis.trial_key),
                if analysis.status == "cached" { "present" } else { "missing" },
                escape_html(&analysis.status),
                escape_html(&render_preview_text(analysis.summary.as_deref(), false)),
                escape_html(&render_preview_text(analysis.json_preview.as_deref(), false)),
            )
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Analysis</h2><table><thead><tr><th>trial</th><th>status</th><th>summary</th><th>json preview</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_diff_html(report: &ViewReport) -> String {
    let rows = report
        .diff
        .iter()
        .map(|diff| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td><pre>{}</pre></td></tr>",
                escape_html(&diff.trial_key),
                escape_html(&diff.source),
                escape_html(
                    &diff
                        .data_ref
                        .as_ref()
                        .map(|data_ref| data_ref.relative_path.display().to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
                escape_html(&render_preview_text(
                    diff.preview.as_deref(),
                    diff.truncated
                )),
            )
        })
        .collect::<String>();
    format!(
        "<section class=\"ledger\"><h2>Diff</h2><table><thead><tr><th>trial</th><th>source</th><th>path</th><th>preview</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

pub(crate) fn render_trajectory_card_html(trajectory: &ViewTrajectoryReport) -> String {
    let graph = render_trajectory_svg(&trajectory.graph);
    let max_duration = trajectory
        .steps
        .iter()
        .filter_map(|step| step.duration_ms)
        .max()
        .unwrap_or(trajectory.duration_ms.max(1));
    let steps = trajectory
        .steps
        .iter()
        .map(|step| {
            let duration_width = step
                .duration_ms
                .map(|duration| ((duration as f64 / max_duration as f64) * 100.0).max(4.0))
                .unwrap_or(8.0);
            let token_width = step
                .token_total
                .map(|tokens| ((tokens as f64 / trajectory.token_total.unwrap_or(tokens).max(1) as f64) * 100.0).max(4.0))
                .unwrap_or(0.0);
            let tools = if step.tool_names.is_empty() {
                "-".to_string()
            } else {
                escape_html(&step.tool_names.join(", "))
            };
            format!(
                "<details class=\"step\" open><summary><span>{}</span><span class=\"muted\">step {} · {} · tools {}</span></summary><div class=\"bars\"><i style=\"width:{:.1}%\"></i><b style=\"width:{:.1}%\"></b></div><pre>{}</pre></details>",
                escape_html(&step.label),
                step.step_id,
                escape_html(&step.source),
                tools,
                duration_width,
                token_width,
                escape_html(&render_preview_text(Some(&step.summary), step.truncated)),
            )
        })
        .collect::<String>();
    format!(
        "<article class=\"trajectory-card\" data-trial=\"{}\"><h3>{}</h3>{}<div class=\"steps\">{}</div></article>",
        escape_html(&trajectory.trial_key),
        escape_html(&trajectory.trial_key),
        graph,
        steps,
    )
}

pub(crate) fn render_trajectory_svg(graph: &ViewTrajectoryGraph) -> String {
    if graph.nodes.is_empty() {
        return String::new();
    }
    let width = 920_u32;
    let height = 92_u32;
    let step = if graph.nodes.len() <= 1 {
        width / 2
    } else {
        (width - 80) / (graph.nodes.len().saturating_sub(1) as u32).max(1)
    };
    let mut svg = format!(
        "<svg class=\"trajectory-graph\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"ATIF step graph\">"
    );
    for (index, _edge) in graph.edges.iter().enumerate() {
        let x1 = 40 + (index as u32 * step);
        let x2 = 40 + ((index + 1) as u32 * step);
        svg.push_str(&format!(
            "<path d=\"M{x1} 42 L{x2} 42\" stroke=\"#a8b1bd\" stroke-width=\"2\" marker-end=\"url(#arrow)\"/>"
        ));
    }
    svg.push_str("<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"5\" markerHeight=\"5\" orient=\"auto-start-reverse\"><path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#a8b1bd\"/></marker></defs>");
    for (index, node) in graph.nodes.iter().enumerate() {
        let x = 40 + (index as u32 * step);
        let class = if node.source == "user" {
            "user"
        } else {
            "agent"
        };
        svg.push_str(&format!(
            "<g class=\"node {class}\"><circle cx=\"{x}\" cy=\"42\" r=\"13\"></circle><text x=\"{x}\" y=\"47\" text-anchor=\"middle\">{}</text><title>{}</title></g>",
            node.step_id,
            escape_html(&node.label),
        ));
    }
    svg.push_str("</svg>");
    svg
}

pub(crate) fn include_list(report: &ViewReport) -> String {
    report
        .includes
        .iter()
        .map(|include| format!("{include:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn status_class_for_case(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::Passed => "present",
        CaseStatus::Failed
        | CaseStatus::SetupFailed
        | CaseStatus::RuntimeFailed
        | CaseStatus::EvaluatorFailed
        | CaseStatus::Timeout => "failed",
    }
}

pub(crate) fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn option_f64(value: Option<f64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn analysis_summary_from_preview(preview: &str) -> Option<String> {
    serde_json::from_str::<Value>(preview)
        .ok()
        .and_then(|value| {
            value
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub(crate) fn has_accounting(accounting: &AccountingMetrics) -> bool {
    accounting.context_input_tokens.is_some()
        || accounting.billable_input_tokens.is_some()
        || accounting.billable_output_tokens.is_some()
        || accounting.reasoning_tokens.is_some()
        || accounting.cache_read_tokens.is_some()
        || accounting.cache_write_tokens.is_some()
        || accounting.reported_total_tokens.is_some()
        || accounting.estimated_cost_nanodollars.is_some()
        || accounting.pricing_source.is_some()
        || accounting.pricing_tier.is_some()
}

pub(crate) fn has_usage(usage: &UsageMetrics) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.cache_read_tokens.is_some()
        || usage.cache_write_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.total_tokens.is_some()
}

pub(crate) fn markdown_preview_cell(preview: Option<&str>, truncated: bool) -> String {
    let rendered = render_preview_text(preview, truncated);
    if rendered == "-" {
        rendered
    } else {
        format!("`{}`", escape_markdown_table(&rendered))
    }
}

pub(crate) fn render_preview_text(preview: Option<&str>, truncated: bool) -> String {
    let Some(preview) = preview else {
        return "-".to_string();
    };
    let (mut short, short_truncated) = truncate_chars_with_flag(preview, RENDER_PREVIEW_CHARS);
    short = short.replace('\n', "\\n");
    if truncated || short_truncated {
        short.push_str(" ... [truncated]");
    }
    if short.is_empty() {
        "-".to_string()
    } else {
        short
    }
}

pub(crate) fn truncate_chars_with_flag(value: &str, max_chars: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            truncated = true;
            break;
        }
        out.push(ch);
    }
    (out, truncated)
}

pub(crate) fn escape_markdown_table(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\n', '\r'], " ")
}

pub(crate) fn redact_preview_text(value: &str) -> String {
    const SECRET_MARKERS: [&str; 7] = [
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "password",
        "secret",
        "token",
    ];
    value
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if SECRET_MARKERS.iter().any(|marker| lower.contains(marker)) {
                "[redacted sensitive line]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
