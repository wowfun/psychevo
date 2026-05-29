#[allow(unused_imports)]
use super::*;

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
