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

pub(crate) fn build_trajectory_bundle(cell: &CellRun) -> ViewTrajectoryBundle {
    let data_ref = data_ref_for_relative(&cell.cell_root, &cell.case.artifacts.trajectory, None)
        .unwrap_or_else(|_| missing_data_ref("trajectory", &cell.case.artifacts.trajectory));
    match read_trajectory_events(cell) {
        Ok(events) => {
            let atif = derive_atif_trajectory(cell, &events);
            let steps = view_trajectory_steps(&atif);
            let graph = trajectory_graph_from_steps(&steps);
            let meta = ViewTrajectoryMetaReport {
                trial_key: trial_key(cell),
                data_ref,
                total_events: events.len(),
                unmapped_events: events.len().saturating_sub(steps.len().saturating_sub(1)),
                total_steps: steps.len(),
                duration_ms: cell.case.metrics.duration_ms,
                tool_calls: cell.case.metrics.tool_calls,
                tool_errors: cell.case.metrics.tool_errors,
                token_total: cell.case.metrics.usage.total_tokens,
                cost_usd: cell.case.metrics.cost.amount_usd,
                prompt_unavailable: atif_prompt_unavailable(&atif),
                system_exposed: atif.steps.iter().any(|step| step.source == "system"),
                reasoning_exposed: atif
                    .steps
                    .iter()
                    .any(|step| step.reasoning_content.is_some()),
                steps,
                graph,
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
                trial_key: trial_key(cell),
                data_ref,
                total_events: 0,
                unmapped_events: 0,
                total_steps: 0,
                duration_ms: cell.case.metrics.duration_ms,
                tool_calls: cell.case.metrics.tool_calls,
                tool_errors: cell.case.metrics.tool_errors,
                token_total: cell.case.metrics.usage.total_tokens,
                cost_usd: cell.case.metrics.cost.amount_usd,
                prompt_unavailable: atif_prompt_unavailable(&atif),
                system_exposed: false,
                reasoning_exposed: false,
                steps: Vec::new(),
                graph: ViewTrajectoryGraph::default(),
                error: Some(format!("{err:#}")),
            };
            ViewTrajectoryBundle {
                trajectory: atif,
                meta,
            }
        }
    }
}
