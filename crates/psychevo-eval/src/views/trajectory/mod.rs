#[allow(unused_imports)]
use super::*;

mod acp;
mod atif;
mod jsonl;
mod meta;
mod metrics;
mod read;

pub(crate) use acp::*;
pub(crate) use atif::*;
pub(crate) use jsonl::*;
pub(crate) use meta::*;
pub(crate) use metrics::*;
pub(crate) use read::*;

#[derive(Debug, Clone)]
pub(crate) struct ViewTrajectoryBundle {
    pub(crate) trajectory: AtifTrajectory,
    pub(crate) meta: ViewTrajectoryMetaReport,
}

pub(crate) fn build_trajectory_bundle(
    cell: &ViewCell,
    workspace_root: &Path,
) -> ViewTrajectoryBundle {
    let data_ref = data_ref_for_relative(&cell.cell_root, &cell.case.artifacts.trajectory, None)
        .unwrap_or_else(|_| missing_data_ref("trajectory", &cell.case.artifacts.trajectory));
    match read_trajectory_events(cell) {
        Ok(events) => {
            let atif = derive_atif_trajectory(cell, &events);
            let steps = view_trajectory_steps(&atif);
            let meta = ViewTrajectoryMetaReport {
                trial_key: view_trial_key(cell),
                matrix_cell_key: view_matrix_cell_key(cell),
                benchmark: cell.benchmark.clone(),
                cell_root_relative: workspace_relative_path(workspace_root, &cell.cell_root),
                variant_id: cell.variant_id.clone(),
                variant_label: cell.variant_label.clone(),
                case_id: cell.case.case_id.clone(),
                task_set_id: cell.case.task_set_id.clone(),
                task_id: cell.case.task_id.clone(),
                task_family: cell.case.task_family.clone(),
                adapter: cell.case.candidate.adapter,
                started_at_ms: cell.started_at_ms,
                finished_at_ms: cell.finished_at_ms,
                duration_ms: cell.case.metrics.duration_ms,
                status: cell.case.status,
                failure_class: cell.case.failure_class.clone(),
                score_passed: cell.case.score.passed,
                score: cell.case.score.score,
                score_message: cell.case.score.message.clone(),
                score_details: cell.case.score.details.clone(),
                warnings: cell.case.warnings.clone(),
                data_ref,
                total_events: events.len(),
                unmapped_events: events.len().saturating_sub(steps.len().saturating_sub(1)),
                prompt_unavailable: atif_prompt_unavailable(&atif),
                steps,
                error: None,
            };
            ViewTrajectoryBundle {
                trajectory: atif,
                meta,
            }
        }
        Err(err) => {
            let atif = derive_atif_trajectory(cell, &[]);
            let meta = ViewTrajectoryMetaReport {
                trial_key: view_trial_key(cell),
                matrix_cell_key: view_matrix_cell_key(cell),
                benchmark: cell.benchmark.clone(),
                cell_root_relative: workspace_relative_path(workspace_root, &cell.cell_root),
                variant_id: cell.variant_id.clone(),
                variant_label: cell.variant_label.clone(),
                case_id: cell.case.case_id.clone(),
                task_set_id: cell.case.task_set_id.clone(),
                task_id: cell.case.task_id.clone(),
                task_family: cell.case.task_family.clone(),
                adapter: cell.case.candidate.adapter,
                started_at_ms: cell.started_at_ms,
                finished_at_ms: cell.finished_at_ms,
                duration_ms: cell.case.metrics.duration_ms,
                status: cell.case.status,
                failure_class: cell.case.failure_class.clone(),
                score_passed: cell.case.score.passed,
                score: cell.case.score.score,
                score_message: cell.case.score.message.clone(),
                score_details: cell.case.score.details.clone(),
                warnings: cell.case.warnings.clone(),
                data_ref,
                total_events: 0,
                unmapped_events: 0,
                prompt_unavailable: atif_prompt_unavailable(&atif),
                steps: Vec::new(),
                error: Some(format!("{err:#}")),
            };
            ViewTrajectoryBundle {
                trajectory: atif,
                meta,
            }
        }
    }
}
