fn context_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(context_read_result(
        state, scope, thread_id,
    )?)?)
}

fn context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ContextReadResult> {
    let thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let Some(thread_id) = thread_id else {
        return Ok(context_unavailable("No active session"));
    };
    let acp = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)?
        .is_some_and(|binding| binding.backend_kind.as_deref() == Some("acp"));
    if acp {
        let usage = state
            .inner
            .state
            .store()
            .session_metadata(&thread_id)?
            .as_ref()
            .and_then(acp_peer_usage_update)
            .and_then(acp_peer_context_read_result);
        return Ok(usage.unwrap_or_else(|| context_unavailable("Agent context is unavailable")));
    }
    let snapshot = match context_snapshot(ContextOptions {
        state: state.inner.state.clone(),
        cwd: scope.cwd.clone(),
        session: thread_id,
        config_path: state.inner.config_path.clone(),
        inherited_env: Some(state.inner.inherited_env.clone()),
    }) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(context_unavailable(&err.to_string()));
        }
    };
    Ok(context_read_result_from_snapshot(&snapshot))
}

fn context_read_result_from_snapshot(
    snapshot: &psychevo_runtime::ContextSnapshot,
) -> wire::ContextReadResult {
    let status = if snapshot.status == "partial" {
        "partial"
    } else if snapshot.total.estimated {
        "estimated"
    } else {
        "exact"
    };
    let categories = snapshot
        .categories
        .iter()
        .filter(|(id, _)| id.as_str() != "free_space")
        .map(|(id, category)| wire::ContextUsageCategoryView {
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
        })
        .collect::<Vec<_>>();
    wire::ContextReadResult {
        available: true,
        label: format_context_total_value(snapshot),
        status: status.to_string(),
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

fn observability_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let resolved_thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let metadata = match resolved_thread_id.as_deref() {
        Some(session_id) => state.inner.state.store().session_metadata(session_id)?,
        None => None,
    };
    let peer_usage = metadata.as_ref().and_then(acp_peer_usage_update);
    let context = match peer_usage.and_then(acp_peer_context_read_result) {
        Some(context) => context,
        None => context_read_result(state, scope, resolved_thread_id.as_deref())?,
    };
    let usage = match resolved_thread_id {
        Some(session_id) => {
            let summary = session_usage_summary(SessionUsageOptions {
                state: state.inner.state.clone(),
                session_id,
            })?;
            let mut view = wire::SessionUsageSummaryView {
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
                reported_total_tokens: summary.reported_total_tokens,
                estimated_cost_nanodollars: summary.estimated_cost_nanodollars,
                cost_status: summary.cost_status,
                estimated_pricing_count: summary.estimated_pricing_count,
                free_pricing_count: summary.free_pricing_count,
                included_pricing_count: summary.included_pricing_count,
                unknown_pricing_count: summary.unknown_pricing_count,
                cache_read_percent: summary.cache_read_percent,
            };
            apply_acp_peer_usage_to_summary(&mut view, peer_usage);
            view
        }
        None => usage_unavailable(),
    };
    Ok(serde_json::to_value(wire::ObservabilityReadResult {
        context,
        usage,
    })?)
}

fn acp_peer_usage_update(metadata: &Value) -> Option<&Value> {
    metadata.get(ACP_PEER_METADATA_KEY)?.get("usageUpdate")
}

fn acp_peer_context_read_result(usage: &Value) -> Option<wire::ContextReadResult> {
    let used = usage_u64_field(usage, "used")?;
    let size = usage_u64_field(usage, "size")?;
    let percent = (size > 0).then(|| (used as f64 / size as f64) * 100.0);
    Some(wire::ContextReadResult {
        available: true,
        label: format_context_total_value_parts(used, false, Some(size), percent),
        status: "exact".to_string(),
        used_tokens: used,
        context_limit: Some(size),
        percent,
        categories: Vec::new(),
        advice: Vec::new(),
    })
}

fn apply_acp_peer_usage_to_summary(
    usage: &mut wire::SessionUsageSummaryView,
    peer_usage: Option<&Value>,
) {
    let Some(peer_usage) = peer_usage else {
        return;
    };
    if let Some(used) = usage_u64_field(peer_usage, "used") {
        if usage.reported_total_tokens == 0 {
            usage.reported_total_tokens = used;
        }
        if usage.context_input_tokens == 0 {
            usage.context_input_tokens = used;
        }
    }
    let has_persisted_pricing =
        usage.estimated_pricing_count + usage.free_pricing_count + usage.included_pricing_count > 0;
    if !has_persisted_pricing && let Some(cost) = acp_peer_usage_cost_nanodollars(peer_usage) {
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

fn usage_u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(|value| {
        value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|number| *number >= 0.0)
                .map(|number| number as u64)
        })
    })
}

fn acp_peer_usage_cost_nanodollars(usage: &Value) -> Option<i64> {
    let cost = usage.get("cost")?;
    let amount = cost.get("amount").and_then(Value::as_f64)?;
    let currency = cost
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    if !currency.eq_ignore_ascii_case("USD") || amount < 0.0 {
        return None;
    }
    Some((amount * 1_000_000_000.0).round() as i64)
}

fn context_unavailable(label: &str) -> wire::ContextReadResult {
    wire::ContextReadResult {
        available: false,
        label: label.to_string(),
        status: "unavailable".to_string(),
        used_tokens: 0,
        context_limit: None,
        percent: None,
        categories: Vec::new(),
        advice: Vec::new(),
    }
}

fn usage_unavailable() -> wire::SessionUsageSummaryView {
    wire::SessionUsageSummaryView {
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
        reported_total_tokens: 0,
        estimated_cost_nanodollars: 0,
        cost_status: "unknown".to_string(),
        estimated_pricing_count: 0,
        free_pricing_count: 0,
        included_pricing_count: 0,
        unknown_pricing_count: 0,
        cache_read_percent: None,
    }
}

fn usage_read_value(
    state: &WebState,
    params: wire::UsageReadParams,
) -> psychevo_runtime::Result<Value> {
    let result = usage_read(UsageReadOptions {
        state: state.inner.state.clone(),
        activity_days: params.activity_days.unwrap_or(365) as usize,
    })?;
    Ok(serde_json::to_value(wire::UsageReadResult {
        generated_at_ms: result.generated_at_ms,
        windows: result
            .windows
            .into_iter()
            .map(|window| wire::UsageWindowSummaryView {
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
                reported_total_tokens: window.reported_total_tokens,
                estimated_cost_nanodollars: window.estimated_cost_nanodollars,
                cost_status: window.cost_status,
                estimated_pricing_count: window.estimated_pricing_count,
                free_pricing_count: window.free_pricing_count,
                included_pricing_count: window.included_pricing_count,
                unknown_pricing_count: window.unknown_pricing_count,
                cache_read_percent: window.cache_read_percent,
            })
            .collect(),
        activity: wire::UsageActivityView {
            start_date: result.activity.start_date,
            end_date: result.activity.end_date,
            days: result
                .activity
                .days
                .into_iter()
                .map(|day| wire::UsageActivityDayView {
                    date: day.date,
                    session_count: day.session_count,
                    message_count: day.message_count,
                    reported_total_tokens: day.reported_total_tokens,
                    context_input_tokens: day.context_input_tokens,
                    cache_read_tokens: day.cache_read_tokens,
                    cache_write_tokens: day.cache_write_tokens,
                    estimated_cost_nanodollars: day.estimated_cost_nanodollars,
                    cost_status: day.cost_status,
                    estimated_pricing_count: day.estimated_pricing_count,
                    free_pricing_count: day.free_pricing_count,
                    included_pricing_count: day.included_pricing_count,
                    unknown_pricing_count: day.unknown_pricing_count,
                })
                .collect(),
        },
    })?)
}
