#[allow(unused_imports)]
use crate::*;

pub(crate) fn build_view(request: ViewRequest) -> Result<ViewReport> {
    let store = EvalStore::resolve(request.store_root.clone())?;
    let (scope, benchmark) = if let Some(path) = &request.path {
        let scope = resolve_view_scope_path(&store.root, &path);
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
    cells.retain(|cell| view_cell_matches(cell, &request));
    let includes = normalize_includes(request.include);
    let cases = cells
        .iter()
        .map(|cell| cell.case.clone())
        .collect::<Vec<_>>();
    let passed_cells = cells
        .iter()
        .filter(|cell| cell.case.status == CaseStatus::Passed)
        .count();
    let failed_cells = cells.len().saturating_sub(passed_cells);
    let status = if failed_cells == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let metrics = aggregate_run_metrics(&cases, cases.iter().map(|case| case.duration_ms).sum());
    let summary = ViewSummary {
        total_cells: cells.len(),
        passed_cells,
        failed_cells,
        status,
        metrics,
    };
    let groups = group_view_cells(&cells, &request.group_by);
    let matrix = if includes.contains(&ViewInclude::Matrix) {
        cells
            .iter()
            .map(|cell| ViewMatrixRow {
                benchmark: cell.benchmark.clone(),
                cell_key: cell.cell_key.clone(),
                artifact_root: Some(cell.cell_root.clone()),
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
            })
            .collect()
    } else {
        Vec::new()
    };
    let usage = if includes.contains(&ViewInclude::Usage) {
        cells
            .iter()
            .map(|cell| ViewUsageRow {
                benchmark: cell.benchmark.clone(),
                cell_key: cell.cell_key.clone(),
                case_id: cell.case.case_id.clone(),
                agent_id: cell.case.agent_id.clone(),
                input_tokens: cell.case.metrics.usage.input_tokens,
                output_tokens: cell.case.metrics.usage.output_tokens,
                cache_read_tokens: cell.case.metrics.usage.cache_read_tokens,
                cache_write_tokens: cell.case.metrics.usage.cache_write_tokens,
                reasoning_tokens: cell.case.metrics.usage.reasoning_tokens,
                total_tokens: cell.case.metrics.usage.total_tokens,
                cost_usd: cell.case.metrics.cost.amount_usd,
            })
            .collect()
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
        usage,
    })
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
        .map(|(key, (total_cells, passed_cells))| {
            let failed_cells = total_cells.saturating_sub(passed_cells);
            ViewGroupRow {
                key,
                total_cells,
                passed_cells,
                failed_cells,
                status: if failed_cells == 0 {
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
            "- scope: `{}`\n- cells: {}\n- passed: {}\n- failed: {}\n- status: {:?}\n\n",
            report.scope.path.display(),
            report.summary.total_cells,
            report.summary.passed_cells,
            report.summary.failed_cells,
            report.summary.status,
        ));
    }
    if !report.groups.is_empty() {
        out.push_str("## Groups\n\n");
        out.push_str("| group | status | passed | failed | total |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
        for row in &report.groups {
            out.push_str(&format!(
                "| `{}` | {:?} | {} | {} | {} |\n",
                row.key, row.status, row.passed_cells, row.failed_cells, row.total_cells,
            ));
        }
        out.push('\n');
    }
    if report.includes.contains(&ViewInclude::Matrix) {
        out.push_str("## Matrix\n\n");
        out.push_str("| benchmark | cell | task set | task | agent | adapter | status | score | duration ms | turns | tools | tool errors |\n");
        out.push_str(
            "| --- | --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |\n",
        );
        for row in &report.matrix {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | `{}` | `{}` | {:?} | {:?} | {} | {} | {} | {} | {} |\n",
                row.benchmark,
                row.cell_key,
                row.task_set_id,
                row.task_id,
                row.agent_id,
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
    if report.includes.contains(&ViewInclude::Usage) {
        out.push_str("## Usage\n\n");
        out.push_str("| benchmark | cell | case | agent | input | output | cache read | cache write | reasoning | total | cost usd |\n");
        out.push_str(
            "| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        );
        for row in &report.usage {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                row.benchmark,
                row.cell_key,
                row.case_id,
                row.agent_id,
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
    }
    Ok(out)
}

pub(crate) fn render_view_html(report: &ViewReport) -> String {
    let mut rows = String::new();
    for row in &report.matrix {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:?}</td><td>{:?}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            escape_html(&row.cell_key),
            escape_html(&row.task_set_id),
            escape_html(&row.task_id),
            escape_html(&row.agent_id),
            row.adapter,
            row.status,
            option_f64(row.score),
            row.duration_ms,
        ));
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval view</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">peval view</p><h1>{}</h1><p class=\"subline\">{} cells · {} passed · {} failed</p></div><div class=\"verdict {}\">{:?}</div></section><section class=\"ledger\"><table><thead><tr><th>cell</th><th>task set</th><th>task</th><th>agent</th><th>adapter</th><th>status</th><th>score</th><th>duration</th></tr></thead><tbody>{}</tbody></table></section></main></body></html>",
        report_css(),
        escape_html(
            report
                .scope
                .benchmark
                .as_deref()
                .unwrap_or("evaluation cells")
        ),
        report.summary.total_cells,
        report.summary.passed_cells,
        report.summary.failed_cells,
        status_class_for_run(report.summary.status),
        report.summary.status,
        rows,
    )
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
