use psychevo::context_usage::{format_context_total_value, format_context_total_value_parts};
use psychevo_gateway_protocol as wire;
use serde_json::Value;

use super::super::scope_session::ResolvedScope;
use super::WebState;

pub(in super::super) async fn context_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo::Result<Value> {
    Ok(serde_json::to_value(
        context_read_result(state, scope, thread_id).await?,
    )?)
}

async fn context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo::Result<wire::settings_workspace_context::ContextReadResult> {
    let thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => {
            state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)
                .await?
        }
    };
    let Some(thread_id) = thread_id else {
        return Ok(context_unavailable("No active session"));
    };
    let thread = match state.inner.framework.resume_thread(&thread_id).await {
        Ok(thread) => thread,
        Err(error) => return Ok(context_unavailable(&error.to_string())),
    };
    if let Some(usage) = thread.agent_usage_observation().await? {
        return Ok(agent_context_read_result(&usage)
            .unwrap_or_else(|| context_unavailable("Agent context is unavailable")));
    }
    let snapshot = match thread.context_snapshot(None).await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(context_unavailable(&err.to_string()));
        }
    };
    Ok(context_read_result_from_snapshot(&snapshot))
}

fn context_read_result_from_snapshot(
    snapshot: &psychevo::context_usage::ContextSnapshot,
) -> wire::settings_workspace_context::ContextReadResult {
    let status = match snapshot.status.as_str() {
        "reported" | "derived" | "partial" | "unavailable" => snapshot.status.as_str(),
        _ if snapshot.total.estimated => "estimated",
        _ => "reported",
    };
    let categories = snapshot
        .categories
        .iter()
        .filter(|(id, _)| id.as_str() != "free_space")
        .map(
            |(id, category)| wire::settings_workspace_context::ContextUsageCategoryView {
                id: id.clone(),
                label: category.label.clone(),
                tokens: category.tokens,
                estimated: category.estimated,
                status: if category.status == "partial" {
                    "partial".to_string()
                } else if category.estimated {
                    "estimated".to_string()
                } else {
                    "exact".to_string()
                },
                percent: category.percent,
                details: Some(category.details.clone()),
            },
        )
        .collect::<Vec<_>>();
    wire::settings_workspace_context::ContextReadResult {
        available: true,
        label: format_context_total_value(snapshot),
        status: status.to_string(),
        basis: snapshot.basis.clone(),
        applies_to_session_seq: snapshot.applies_to_session_seq,
        used_tokens: snapshot.total.tokens,
        context_limit: snapshot.context_limit,
        percent: snapshot.total.percent,
        categories,
        advice: snapshot
            .advice
            .iter()
            .map(|advice| advice.message.clone())
            .collect(),
    }
}

pub(in super::super) async fn observability_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo::Result<Value> {
    let resolved_thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => {
            state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)
                .await?
        }
    };
    let (context, usage) = match resolved_thread_id {
        Some(session_id) => {
            let thread = state.inner.framework.resume_thread(&session_id).await?;
            let agent_usage = thread.agent_usage_observation().await?;
            let context = match agent_usage.as_ref() {
                Some(usage) => agent_context_read_result(usage)
                    .unwrap_or_else(|| context_unavailable("Agent context is unavailable")),
                None => match thread.context_snapshot(None).await {
                    Ok(snapshot) => context_read_result_from_snapshot(&snapshot),
                    Err(error) => context_unavailable(&error.to_string()),
                },
            };
            let summary = thread.usage_summary().await?;
            let mut view = wire::settings_workspace_context::SessionUsageSummaryView {
                available: true,
                session_id: Some(summary.session_id),
                provider: Some(summary.provider),
                model: Some(summary.model),
                message_count: summary.message_count,
                assistant_message_count: summary.assistant_message_count,
                context_input_tokens: summary.context_input_tokens,
                billable_input_tokens: summary.billable_input_tokens,
                billable_output_tokens: summary.billable_output_tokens,
                reasoning_tokens: summary.reasoning_tokens,
                cache_read_tokens: summary.cache_read_tokens,
                cache_write_tokens: summary.cache_write_tokens,
                effective_total_tokens: summary.effective_total_tokens,
                reported_total_tokens: summary.reported_total_tokens,
                total_status: summary.total_status,
                accounted_provider_call_count: summary.accounted_provider_call_count,
                unaccounted_provider_call_count: summary.unaccounted_provider_call_count,
                estimated_cost_nanodollars: summary.estimated_cost_nanodollars,
                cost_status: summary.cost_status,
                estimated_pricing_count: summary.estimated_pricing_count,
                free_pricing_count: summary.free_pricing_count,
                included_pricing_count: summary.included_pricing_count,
                unknown_pricing_count: summary.unknown_pricing_count,
                cache_read_percent: summary.cache_read_percent,
            };
            apply_agent_usage_to_summary(&mut view, agent_usage.as_ref());
            (context, view)
        }
        None => (
            context_unavailable("No active session"),
            usage_unavailable(),
        ),
    };
    Ok(serde_json::to_value(
        wire::settings_workspace_context::ObservabilityReadResult { context, usage },
    )?)
}

fn agent_context_read_result(
    usage: &psychevo::application::AgentUsageObservation,
) -> Option<wire::settings_workspace_context::ContextReadResult> {
    let used = usage.used_tokens?;
    let size = usage.context_limit?;
    let percent = (size > 0).then(|| (used as f64 / size as f64) * 100.0);
    Some(wire::settings_workspace_context::ContextReadResult {
        available: true,
        label: format_context_total_value_parts(used, false, Some(size), percent),
        status: "partial".to_string(),
        basis: "agent_reported_context".to_string(),
        applies_to_session_seq: None,
        used_tokens: used,
        context_limit: Some(size),
        percent,
        categories: Vec::new(),
        advice: Vec::new(),
    })
}

fn apply_agent_usage_to_summary(
    usage: &mut wire::settings_workspace_context::SessionUsageSummaryView,
    agent_usage: Option<&psychevo::application::AgentUsageObservation>,
) {
    let Some(cost) = agent_usage.and_then(|usage| usage.estimated_cost_nanodollars) else {
        return;
    };
    let has_persisted_pricing =
        usage.estimated_pricing_count + usage.free_pricing_count + usage.included_pricing_count > 0;
    if !has_persisted_pricing {
        usage.estimated_cost_nanodollars = cost;
        usage.cost_status = if cost == 0 {
            "free".to_string()
        } else {
            "estimated".to_string()
        };
        usage.estimated_pricing_count = (cost > 0) as u64;
        usage.free_pricing_count = (cost == 0) as u64;
    }
}

fn context_unavailable(label: &str) -> wire::settings_workspace_context::ContextReadResult {
    wire::settings_workspace_context::ContextReadResult {
        available: false,
        label: label.to_string(),
        status: "unavailable".to_string(),
        basis: "unavailable".to_string(),
        applies_to_session_seq: None,
        used_tokens: 0,
        context_limit: None,
        percent: None,
        categories: Vec::new(),
        advice: Vec::new(),
    }
}

fn usage_unavailable() -> wire::settings_workspace_context::SessionUsageSummaryView {
    wire::settings_workspace_context::SessionUsageSummaryView {
        available: false,
        session_id: None,
        provider: None,
        model: None,
        message_count: 0,
        assistant_message_count: 0,
        context_input_tokens: 0,
        billable_input_tokens: 0,
        billable_output_tokens: 0,
        reasoning_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        effective_total_tokens: None,
        reported_total_tokens: 0,
        total_status: "unavailable".to_string(),
        accounted_provider_call_count: 0,
        unaccounted_provider_call_count: 0,
        estimated_cost_nanodollars: 0,
        cost_status: "unknown".to_string(),
        estimated_pricing_count: 0,
        free_pricing_count: 0,
        included_pricing_count: 0,
        unknown_pricing_count: 0,
        cache_read_percent: None,
    }
}

pub(in super::super) async fn usage_read_value(
    state: &WebState,
    params: wire::settings_workspace_context::UsageReadParams,
) -> psychevo::Result<Value> {
    let result = state
        .inner
        .framework
        .usage_overview(params.activity_days.unwrap_or(365) as usize)
        .await?;
    Ok(serde_json::to_value(
        wire::settings_workspace_context::UsageReadResult {
            generated_at_ms: result.generated_at_ms,
            windows: result
                .windows
                .into_iter()
                .map(
                    |window| wire::settings_workspace_context::UsageWindowSummaryView {
                        id: window.id,
                        label: window.label,
                        since_ms: window.since_ms,
                        session_count: window.session_count,
                        message_count: window.message_count,
                        assistant_message_count: window.assistant_message_count,
                        context_input_tokens: window.context_input_tokens,
                        billable_input_tokens: window.billable_input_tokens,
                        billable_output_tokens: window.billable_output_tokens,
                        reasoning_tokens: window.reasoning_tokens,
                        cache_read_tokens: window.cache_read_tokens,
                        cache_write_tokens: window.cache_write_tokens,
                        effective_total_tokens: window.effective_total_tokens,
                        reported_total_tokens: window.reported_total_tokens,
                        total_status: window.total_status,
                        accounted_provider_call_count: window.accounted_provider_call_count,
                        unaccounted_provider_call_count: window.unaccounted_provider_call_count,
                        estimated_cost_nanodollars: window.estimated_cost_nanodollars,
                        cost_status: window.cost_status,
                        estimated_pricing_count: window.estimated_pricing_count,
                        free_pricing_count: window.free_pricing_count,
                        included_pricing_count: window.included_pricing_count,
                        unknown_pricing_count: window.unknown_pricing_count,
                        cache_read_percent: window.cache_read_percent,
                    },
                )
                .collect(),
            activity: wire::settings_workspace_context::UsageActivityView {
                start_date: result.activity.start_date,
                end_date: result.activity.end_date,
                days: result
                    .activity
                    .days
                    .into_iter()
                    .map(
                        |day| wire::settings_workspace_context::UsageActivityDayView {
                            date: day.date,
                            session_count: day.session_count,
                            message_count: day.message_count,
                            effective_total_tokens: day.effective_total_tokens,
                            reported_total_tokens: day.reported_total_tokens,
                            total_status: day.total_status,
                            accounted_provider_call_count: day.accounted_provider_call_count,
                            unaccounted_provider_call_count: day.unaccounted_provider_call_count,
                            context_input_tokens: day.context_input_tokens,
                            cache_read_tokens: day.cache_read_tokens,
                            cache_write_tokens: day.cache_write_tokens,
                            estimated_cost_nanodollars: day.estimated_cost_nanodollars,
                            cost_status: day.cost_status,
                            estimated_pricing_count: day.estimated_pricing_count,
                            free_pricing_count: day.free_pricing_count,
                            included_pricing_count: day.included_pricing_count,
                            unknown_pricing_count: day.unknown_pricing_count,
                        },
                    )
                    .collect(),
            },
        },
    )?)
}
