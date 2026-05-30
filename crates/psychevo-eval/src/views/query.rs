#[allow(unused_imports)]
use super::*;

pub(crate) fn build_view(request: ViewRequest) -> Result<ViewReport> {
    let loaded = load_view_cells(&request)?;
    let mut includes = normalize_includes(request.include);
    if !request.notes.is_empty() && !includes.contains(&ViewInclude::Annotations) {
        includes.push(ViewInclude::Annotations);
        includes = normalize_includes(includes);
    }
    let workspace_root = loaded.store.root.clone();
    let cases = loaded
        .cells
        .iter()
        .map(|cell| cell.case.clone())
        .collect::<Vec<_>>();
    let passed_trials = loaded
        .cells
        .iter()
        .filter(|cell| cell.case.status == CaseStatus::Passed)
        .count();
    let failed_trials = loaded.cells.len().saturating_sub(passed_trials);
    let status = if failed_trials == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let metrics = aggregate_run_metrics(&cases, cases.iter().map(|case| case.duration_ms).sum());
    let summary = ViewSummary {
        total_trials: loaded.cells.len(),
        passed_trials,
        failed_trials,
        status,
        metrics,
    };
    let comparison = if includes.contains(&ViewInclude::Comparison) {
        Some(ViewComparisonReport {
            default_metric: default_heatmap_metric(&loaded.cells),
            summary,
            groups: group_view_cells(&loaded.cells, &request.group_by),
            matrix: build_view_matrix(&loaded.cells),
            leaderboard: build_view_leaderboard(&loaded.cells),
        })
    } else {
        None
    };
    let annotations = if includes.contains(&ViewInclude::Annotations) {
        Some(ViewAnnotationsReport {
            report_notes: build_report_notes(&request.notes),
            notes: build_note_reports(&loaded.cells, &request.notes)?,
            analysis: loaded.cells.iter().map(build_analysis_report).collect(),
        })
    } else {
        None
    };
    let attachments = if includes.contains(&ViewInclude::Attachments) {
        Some(ViewAttachmentsReport {
            artifacts: loaded.cells.iter().map(build_attachment_report).collect(),
        })
    } else {
        None
    };
    let trajectory_bundles = if includes.contains(&ViewInclude::Core) {
        loaded
            .cells
            .iter()
            .map(|cell| build_trajectory_bundle(cell, &workspace_root))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let trajectory = trajectory_bundles
        .iter()
        .map(|bundle| bundle.trajectory.clone())
        .collect();
    let trajectory_meta = trajectory_bundles
        .into_iter()
        .map(|bundle| bundle.meta)
        .collect();
    Ok(ViewReport {
        schema_version: VIEW_SCHEMA_VERSION,
        includes,
        scope: ViewScope {
            workspace_root: loaded.store.root,
            path: loaded.scope,
            benchmark: loaded.benchmark,
        },
        path_selections: loaded.path_selections,
        trajectory,
        trajectory_meta,
        comparison,
        annotations,
        attachments,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct ViewCell {
    pub cell: CellRun,
    pub variant_id: Option<String>,
    pub variant_label: Option<String>,
}

impl ViewCell {
    pub(crate) fn unselected(cell: CellRun) -> Self {
        Self {
            cell,
            variant_id: None,
            variant_label: None,
        }
    }

    pub(crate) fn selected(cell: CellRun, selection: &ViewPathSelection) -> Self {
        Self {
            cell,
            variant_id: Some(selection.id.clone()),
            variant_label: Some(selection.label.clone()),
        }
    }
}

impl std::ops::Deref for ViewCell {
    type Target = CellRun;

    fn deref(&self) -> &Self::Target {
        &self.cell
    }
}

pub(crate) struct LoadedViewCells {
    pub store: EvalStore,
    pub scope: PathBuf,
    pub benchmark: Option<String>,
    pub path_selections: Vec<ViewPathSelection>,
    pub cells: Vec<ViewCell>,
}

struct ExplicitPathCells {
    scope: PathBuf,
    benchmark: Option<String>,
    path_selections: Vec<ViewPathSelection>,
    cells: Vec<ViewCell>,
}

pub(crate) fn load_view_cells(request: &ViewRequest) -> Result<LoadedViewCells> {
    let store = EvalStore::resolve(request.store_root.clone())?;
    let (scope, benchmark, path_selections, mut cells) = if request.paths.is_empty() {
        let project = load_project_from_selection(
            request.config.as_deref(),
            request.benchmark.as_deref(),
            request.store_root.clone(),
        )?;
        let scope = store.cell_runs_root(&project);
        let cells = store
            .scan_cell_runs(&scope)?
            .into_iter()
            .map(ViewCell::unselected)
            .collect();
        (scope, Some(project.benchmark_id), Vec::new(), cells)
    } else {
        let loaded = load_explicit_path_cells(&store, &request.paths)?;
        (
            loaded.scope,
            loaded.benchmark,
            loaded.path_selections,
            loaded.cells,
        )
    };
    cells.retain(|cell| view_cell_matches(cell, request));
    Ok(LoadedViewCells {
        store,
        scope,
        benchmark,
        path_selections,
        cells,
    })
}

fn load_explicit_path_cells(store: &EvalStore, paths: &[PathBuf]) -> Result<ExplicitPathCells> {
    let mut raw = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let scope = resolve_view_scope_path(&store.root, path);
        let cells = store.scan_cell_runs(&scope)?;
        if cells.is_empty() {
            bail!(
                "view path {} resolved to zero cell runs",
                view_display_path(&store.root, &scope).display()
            );
        }
        raw.push(RawPathSelection {
            id: format!("p{:02}", index + 1),
            label: view_display_path(&store.root, &scope)
                .to_string_lossy()
                .into_owned(),
            scope,
            cells,
        });
    }

    let mut label_counts = BTreeMap::<String, usize>::new();
    for selection in &raw {
        *label_counts.entry(selection.label.clone()).or_default() += 1;
    }

    let selections = raw
        .iter()
        .map(|selection| {
            let label = if label_counts.get(&selection.label).copied().unwrap_or(0) > 1 {
                format!("{} {}", selection.id, selection.label)
            } else {
                selection.label.clone()
            };
            ViewPathSelection {
                id: selection.id.clone(),
                label,
                path: view_display_path(&store.root, &selection.scope),
                cell_count: selection.cells.len(),
            }
        })
        .collect::<Vec<_>>();

    let use_path_variants = paths.len() > 1;
    let mut seen = BTreeMap::<PathBuf, String>::new();
    let mut cells = Vec::new();
    for (raw_selection, selection) in raw.into_iter().zip(selections.iter()) {
        for cell in raw_selection.cells {
            let key = fs::canonicalize(&cell.cell_root)
                .unwrap_or_else(|_| absolute_path(&cell.cell_root));
            if let Some(existing) = seen.get(&key) {
                bail!(
                    "cell {} was selected by both {} and {}",
                    view_display_path(&store.root, &key).display(),
                    existing,
                    selection.id
                );
            }
            seen.insert(key, selection.id.clone());
            if use_path_variants {
                cells.push(ViewCell::selected(cell, selection));
            } else {
                cells.push(ViewCell::unselected(cell));
            }
        }
    }
    cells.sort_by(|left, right| {
        left.variant_id
            .cmp(&right.variant_id)
            .then_with(|| left.benchmark.cmp(&right.benchmark))
            .then_with(|| left.case.agent_id.cmp(&right.case.agent_id))
            .then_with(|| left.case.task_id.cmp(&right.case.task_id))
            .then_with(|| left.cell_key.cmp(&right.cell_key))
    });

    let scope = if selections.len() == 1 {
        scope_from_selection(store, &selections[0])
    } else {
        store.root.join("runs")
    };
    let benchmark = infer_common_benchmark(&store.root, &selections);
    Ok(ExplicitPathCells {
        scope,
        benchmark,
        path_selections: selections,
        cells,
    })
}

#[derive(Debug)]
struct RawPathSelection {
    id: String,
    label: String,
    scope: PathBuf,
    cells: Vec<CellRun>,
}

pub(crate) fn view_display_path(root: &Path, path: &Path) -> PathBuf {
    workspace_relative_path(root, path)
}

fn scope_from_selection(store: &EvalStore, selection: &ViewPathSelection) -> PathBuf {
    if selection.path.is_absolute() {
        selection.path.clone()
    } else {
        store.root.join(&selection.path)
    }
}

fn infer_common_benchmark(root: &Path, selections: &[ViewPathSelection]) -> Option<String> {
    let mut benchmarks = selections
        .iter()
        .filter_map(|selection| {
            let scope = if selection.path.is_absolute() {
                selection.path.clone()
            } else {
                root.join(&selection.path)
            };
            infer_benchmark_from_scope(root, &scope)
        })
        .collect::<BTreeSet<_>>();
    if benchmarks.len() == 1 {
        benchmarks.pop_first()
    } else {
        None
    }
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

pub(crate) fn group_view_cells(cells: &[ViewCell], group_by: &[ViewGroupBy]) -> Vec<ViewGroupRow> {
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
        vec![
            ViewInclude::Core,
            ViewInclude::Comparison,
            ViewInclude::Annotations,
        ]
    } else {
        includes
    };
    includes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
