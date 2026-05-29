#[allow(unused_imports)]
use super::*;

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
    let leaderboard = build_view_leaderboard(&cells);
    let trials = if includes.contains(&ViewInclude::Summary)
        || includes.contains(&ViewInclude::Matrix)
        || !includes.is_empty()
    {
        cells
            .iter()
            .map(|cell| view_trial(cell, &store.root))
            .collect()
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
    let trajectory_bundles = if includes.contains(&ViewInclude::Trajectory)
        || includes.contains(&ViewInclude::TrajectoryMeta)
    {
        cells
            .iter()
            .map(build_trajectory_bundle)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let trajectory = if includes.contains(&ViewInclude::Trajectory) {
        trajectory_bundles
            .iter()
            .map(|bundle| bundle.trajectory.clone())
            .collect()
    } else {
        Vec::new()
    };
    let trajectory_meta = if includes.contains(&ViewInclude::TrajectoryMeta) {
        trajectory_bundles
            .into_iter()
            .map(|bundle| bundle.meta)
            .collect()
    } else {
        Vec::new()
    };
    let analysis = if includes.contains(&ViewInclude::Analysis) {
        cells.iter().map(build_analysis_report).collect()
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
        leaderboard,
        trials,
        usage,
        warnings,
        artifacts,
        trajectory,
        trajectory_meta,
        analysis,
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
