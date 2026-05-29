#[allow(unused_imports)]
use crate::*;

mod agent;
mod case;
mod cell;
mod container;
mod dataset;
mod metrics;
mod process;
mod registry;
mod task_env;
mod validation;

pub(crate) use agent::*;
pub(crate) use case::*;
pub(crate) use cell::*;
pub(crate) use container::*;
pub(crate) use dataset::*;
pub(crate) use metrics::*;
pub(crate) use process::*;
pub(crate) use registry::*;
pub(crate) use task_env::*;
pub(crate) use validation::*;

pub(crate) fn expand_matrix(
    project: &EvalProject,
    task_set_filter: Option<&str>,
    task_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CasePlan>> {
    let mut plans = Vec::new();
    let task_sets = selected_task_sets(project, task_set_filter)?;
    for task_set in task_sets {
        let agent_ids = selected_agent_ids(project, agent_filter)?;
        let tasks = match load_task_set_tasks(project, &task_set, task_filter) {
            Ok(tasks) => tasks,
            Err(err) if task_filter.is_some() && task_set_filter.is_none() => {
                let message = format!("{err:#}");
                if message.contains("does not include selected task") {
                    continue;
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        for task in tasks {
            for agent_id in &agent_ids {
                let agent = project
                    .agents
                    .get(agent_id)
                    .with_context(|| {
                        format!("unknown agent `{agent_id}` in task set `{}`", task_set.id)
                    })?
                    .clone();
                let case_id = sanitize_id(&format!("{}__{}__{}", task_set.id, task.id, agent.id));
                plans.push(CasePlan {
                    case_id,
                    task_set: task_set.clone(),
                    task: task.clone(),
                    agent,
                });
            }
        }
    }
    if plans.is_empty() {
        bail!("no cases selected");
    }
    Ok(plans)
}

pub(crate) fn run_evaluation(request: RunRequest) -> Result<RunExecutionSummary> {
    validate_direct_benchmark_selection(
        request.benchmark.as_deref(),
        request.agent.as_deref(),
        request.task_set.as_deref(),
        request.task.as_deref(),
    )?;
    let project = load_project_from_selection(
        request.config.as_deref(),
        request.benchmark.as_deref(),
        request.store_root.clone(),
    )?;
    let cases = expand_matrix(
        &project,
        request.task_set.as_deref(),
        request.task.as_deref(),
        request.agent.as_deref(),
    )?;
    if cases.is_empty() {
        bail!("no cases selected");
    }
    for case in &cases {
        validate_case(case)?;
    }

    let explicit_output = request.output_root.is_some();
    let output_store = if let Some(path) = request.output_root {
        EvalStore::new(resolve_cli_path(&path)?)
    } else {
        EvalStore::resolve(request.store_root)?
    };
    let output_base = output_store.cell_runs_root(&project);
    fs::create_dir_all(&output_base)
        .with_context(|| format!("failed to create {}", output_base.display()))?;
    let artifact_includes = resolved_artifact_includes(&project, &request.include_artifacts);

    let mut cells = Vec::new();
    for case in cases {
        let fingerprint = cell_fingerprint(&project, &case)?;
        let cell_key = cell_key(&fingerprint);
        let cell_root = output_store.cell_root(&project, &case, &cell_key);
        let mut action = CellRunAction::Executed;
        if !explicit_output
            && !request.overwrite
            && let Ok(existing) = read_cell_run(&cell_root)
            && existing.fingerprint == fingerprint
            && existing.case.status.is_terminal_reusable()
        {
            cells.push(RunExecutionCell {
                cell_key: existing.cell_key,
                fingerprint: existing.fingerprint,
                cell_root: existing.cell_root,
                task_set_id: existing.case.task_set_id,
                task_id: existing.case.task_id,
                agent_id: existing.case.agent_id,
                status: existing.case.status,
                action: CellRunAction::Reused,
            });
            continue;
        }
        if !explicit_output && request.overwrite && cell_root.exists() {
            action = CellRunAction::Overwritten;
        } else if !explicit_output && cell_root.exists() {
            action = CellRunAction::Retried;
        }
        let cell = execute_cell(
            &project,
            case,
            &cell_root,
            &cell_key,
            &fingerprint,
            &artifact_includes,
        )?;
        cells.push(RunExecutionCell {
            cell_key: cell.cell_key,
            fingerprint: cell.fingerprint,
            cell_root: cell.cell_root,
            task_set_id: cell.case.task_set_id,
            task_id: cell.case.task_id,
            agent_id: cell.case.agent_id,
            status: cell.case.status,
            action,
        });
    }

    let passed_cells = cells
        .iter()
        .filter(|cell| cell.status == CaseStatus::Passed)
        .count();
    let failed_cells = cells.len().saturating_sub(passed_cells);
    let status = if failed_cells == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let benchmark_slug = project.slug();
    Ok(RunExecutionSummary {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        project: project.name.clone(),
        benchmark: project.benchmark_id,
        benchmark_slug,
        selected_cells: cells.len(),
        executed_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Executed)
            .count(),
        reused_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Reused)
            .count(),
        overwritten_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Overwritten)
            .count(),
        retried_cells: cells
            .iter()
            .filter(|cell| cell.action == CellRunAction::Retried)
            .count(),
        failed_cells,
        passed_cells,
        status,
        cells,
    })
}

#[cfg(test)]
mod tests;
