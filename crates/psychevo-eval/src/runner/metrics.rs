#[allow(unused_imports)]
use super::*;

pub(crate) struct CaseObservability {
    pub(crate) metrics: CaseMetrics,
    pub(crate) warnings: Vec<String>,
}

#[allow(dead_code)]
pub(crate) fn collect_case_metrics(events: &[TrajectoryEvent], duration_ms: u128) -> CaseMetrics {
    collect_case_observability(events, duration_ms).metrics
}

pub(crate) fn collect_case_observability(
    events: &[TrajectoryEvent],
    duration_ms: u128,
) -> CaseObservability {
    let mut metrics = CaseMetrics {
        duration_ms,
        ..CaseMetrics::default()
    };
    let mut turns = 0_u64;
    let mut usage = UsageAccumulator::default();
    let mut warnings = Vec::new();
    let mut tool_error_ids = BTreeSet::new();
    let acp_windowed = events
        .iter()
        .any(|event| event.kind == "acp_agent_prompt_started");
    let mut in_acp_prompt = !acp_windowed;
    for event in events {
        if event.kind == "acp_agent_prompt_started" {
            in_acp_prompt = true;
            continue;
        }
        if acp_windowed && !in_acp_prompt {
            continue;
        }
        if event.kind == "acp_session_update" {
            collect_acp_session_update_metrics(
                event,
                &mut metrics,
                &mut usage,
                &mut warnings,
                &mut tool_error_ids,
            );
        } else if event.kind == "acp_agent_prompt_finished" {
            if let Some(prompt_result) = event.data.get("prompt_result") {
                usage.add_from_value(prompt_result);
                collect_psychevo_meta(prompt_result, &mut warnings, &mut turns);
            }
            in_acp_prompt = false;
        } else if event.kind.ends_with("turn_start") {
            turns += 1;
        } else if event.kind.ends_with("tool_execution_start") {
            metrics.tool_calls += 1;
        } else if event.kind.ends_with("tool_execution_end") && event_indicates_tool_error(event) {
            if let Some(id) = tool_call_id_for_event(event) {
                if tool_error_ids.insert(id) {
                    metrics.tool_errors += 1;
                }
            } else {
                metrics.tool_errors += 1;
            }
        }
        if let Some(raw) = event.data.get("raw_event") {
            usage.add_from_value(raw);
            collect_warning(raw, &mut warnings);
        } else {
            usage.add_from_value(&event.data);
            collect_warning(&event.data, &mut warnings);
        }
    }
    metrics.turns = (turns > 0).then_some(turns);
    let (usage, accounting, cost) = usage.finish();
    metrics.usage = usage;
    metrics.accounting = accounting;
    metrics.cost = cost;
    CaseObservability { metrics, warnings }
}

#[derive(Default)]
pub(crate) struct UsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
    has_input: bool,
    has_output: bool,
    has_cache_read: bool,
    has_cache_write: bool,
    has_reasoning: bool,
    has_total: bool,
    has_cost: bool,
    has_cumulative_cost: bool,
    accounting: AccountingAccumulator,
}

impl UsageAccumulator {
    pub(crate) fn add_from_value(&mut self, value: &Value) {
        let mut saw_usage = false;
        if let Some(accounting) = value.get("accounting") {
            self.add_accounting(accounting);
        }
        if let Some(usage) = value.get("usage") {
            saw_usage = self.has_usage_tokens();
            self.add_usage_tokens(usage);
            saw_usage = saw_usage || self.has_usage_tokens();
            self.add_cost(usage);
        }
        if let Some(psychevo) = value.get("_meta").and_then(|meta| meta.get("psychevo"))
            && let Some(accounting) = psychevo.get("accounting")
        {
            self.add_accounting(accounting);
            if !saw_usage {
                self.add_usage_from_accounting(accounting);
            }
        }
        if let Some(accounting) = value.get("accounting")
            && !saw_usage
        {
            self.add_usage_from_accounting(accounting);
        }
        if let Some(cost) = value.get("cost") {
            self.add_cumulative_cost(cost);
        }
        self.add_cost(value);
    }

    pub(crate) fn add_usage_tokens(&mut self, value: &Value) {
        self.add_field(value, "input_tokens", "input");
        self.add_field(value, "inputTokens", "input");
        self.add_field(value, "prompt_tokens", "input");
        self.add_field(value, "output_tokens", "output");
        self.add_field(value, "outputTokens", "output");
        self.add_field(value, "completion_tokens", "output");
        self.add_field(value, "cached_read_tokens", "cache_read");
        self.add_field(value, "cachedReadTokens", "cache_read");
        self.add_field(value, "cache_read_tokens", "cache_read");
        self.add_field(value, "cached_input_tokens", "cache_read");
        self.add_field(value, "cached_tokens", "cache_read");
        self.add_field(value, "cached_write_tokens", "cache_write");
        self.add_field(value, "cachedWriteTokens", "cache_write");
        self.add_field(value, "cache_write_tokens", "cache_write");
        self.add_field(value, "thought_tokens", "reasoning");
        self.add_field(value, "thoughtTokens", "reasoning");
        self.add_field(value, "reasoning_tokens", "reasoning");
        self.add_field(value, "reported_total_tokens", "total");
        self.add_field(value, "total_tokens", "total");
        self.add_field(value, "totalTokens", "total");
    }

    pub(crate) fn add_usage_from_accounting(&mut self, value: &Value) {
        let cache_read = value.get("cache_read_tokens").and_then(json_u64);
        let cache_write = value.get("cache_write_tokens").and_then(json_u64);
        let reasoning = value.get("reasoning_tokens").and_then(json_u64);
        let input = value
            .get("context_input_tokens")
            .and_then(json_u64)
            .or_else(|| {
                value
                    .get("billable_input_tokens")
                    .and_then(json_u64)
                    .map(|amount| {
                        amount
                            .saturating_add(cache_read.unwrap_or(0))
                            .saturating_add(cache_write.unwrap_or(0))
                    })
            });
        let output = value
            .get("billable_output_tokens")
            .and_then(json_u64)
            .map(|amount| amount.saturating_add(reasoning.unwrap_or(0)));
        self.add_amount(input, "input");
        self.add_amount(output, "output");
        self.add_amount(cache_read, "cache_read");
        self.add_amount(cache_write, "cache_write");
        self.add_amount(reasoning, "reasoning");
        self.add_amount(
            value.get("reported_total_tokens").and_then(json_u64),
            "total",
        );
    }

    pub(crate) fn add_accounting(&mut self, value: &Value) {
        self.accounting.add(value);
        if let Some(nanodollars) = value.get("estimated_cost_nanodollars").and_then(json_i64) {
            let amount = nanodollars as f64 / 1_000_000_000.0;
            if self.has_cumulative_cost {
                self.cost_usd = self.cost_usd.max(amount);
            } else {
                self.cost_usd += amount;
            }
            self.has_cost = true;
        }
    }

    pub(crate) fn add_field(&mut self, value: &Value, field: &str, target: &str) {
        let Some(amount) = value.get(field).and_then(json_u64) else {
            return;
        };
        self.add_amount(Some(amount), target);
    }

    fn add_amount(&mut self, amount: Option<u64>, target: &str) {
        let Some(amount) = amount else {
            return;
        };
        match target {
            "input" => {
                self.input_tokens += amount;
                self.has_input = true;
            }
            "output" => {
                self.output_tokens += amount;
                self.has_output = true;
            }
            "cache_read" => {
                self.cache_read_tokens += amount;
                self.has_cache_read = true;
            }
            "cache_write" => {
                self.cache_write_tokens += amount;
                self.has_cache_write = true;
            }
            "reasoning" => {
                self.reasoning_tokens += amount;
                self.has_reasoning = true;
            }
            "total" => {
                self.total_tokens += amount;
                self.has_total = true;
            }
            _ => {}
        }
    }

    fn has_usage_tokens(&self) -> bool {
        self.has_input
            || self.has_output
            || self.has_cache_read
            || self.has_cache_write
            || self.has_reasoning
            || self.has_total
    }

    pub(crate) fn add_cost(&mut self, value: &Value) {
        for field in ["amount_usd", "cost_usd", "total_cost_usd"] {
            if let Some(amount) = value.get(field).and_then(Value::as_f64) {
                self.cost_usd += amount;
                self.has_cost = true;
            }
        }
        if value
            .get("currency")
            .and_then(Value::as_str)
            .is_none_or(|currency| currency.eq_ignore_ascii_case("USD"))
            && let Some(amount) = value.get("amount").and_then(Value::as_f64)
        {
            self.cost_usd += amount;
            self.has_cost = true;
        }
    }

    pub(crate) fn add_cumulative_cost(&mut self, value: &Value) {
        if value
            .get("currency")
            .and_then(Value::as_str)
            .is_none_or(|currency| currency.eq_ignore_ascii_case("USD"))
            && let Some(amount) = value.get("amount").and_then(Value::as_f64)
        {
            self.cost_usd = if self.has_cost {
                self.cost_usd.max(amount)
            } else {
                amount
            };
            self.has_cost = true;
            self.has_cumulative_cost = true;
        }
    }

    pub(crate) fn finish(self) -> (UsageMetrics, AccountingMetrics, CostMetrics) {
        let computed_total = self.input_tokens + self.output_tokens;
        (
            UsageMetrics {
                input_tokens: self.has_input.then_some(self.input_tokens),
                output_tokens: self.has_output.then_some(self.output_tokens),
                cache_read_tokens: self.has_cache_read.then_some(self.cache_read_tokens),
                cache_write_tokens: self.has_cache_write.then_some(self.cache_write_tokens),
                reasoning_tokens: self.has_reasoning.then_some(self.reasoning_tokens),
                total_tokens: if self.has_total {
                    Some(self.total_tokens)
                } else if self.has_input
                    || self.has_output
                    || self.has_cache_read
                    || self.has_cache_write
                    || self.has_reasoning
                {
                    Some(computed_total)
                } else {
                    None
                },
            },
            self.accounting.finish(),
            CostMetrics {
                amount_usd: self.has_cost.then_some(self.cost_usd),
                source: self.has_cost.then(|| "event_usage".to_string()),
            },
        )
    }
}

#[derive(Default)]
pub(crate) struct AccountingAccumulator {
    metrics: AccountingMetrics,
}

impl AccountingAccumulator {
    pub(crate) fn add(&mut self, value: &Value) {
        add_accounting_u64(
            &mut self.metrics.context_input_tokens,
            value.get("context_input_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.billable_input_tokens,
            value.get("billable_input_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.billable_output_tokens,
            value.get("billable_output_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.reasoning_tokens,
            value.get("reasoning_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.cache_read_tokens,
            value.get("cache_read_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.cache_write_tokens,
            value.get("cache_write_tokens").and_then(json_u64),
        );
        add_accounting_u64(
            &mut self.metrics.reported_total_tokens,
            value.get("reported_total_tokens").and_then(json_u64),
        );
        add_accounting_i64(
            &mut self.metrics.estimated_cost_nanodollars,
            value.get("estimated_cost_nanodollars").and_then(json_i64),
        );
        merge_accounting_string(
            &mut self.metrics.pricing_source,
            value.get("pricing_source").and_then(Value::as_str),
        );
        merge_accounting_string(
            &mut self.metrics.pricing_tier,
            value.get("pricing_tier").and_then(Value::as_str),
        );
    }

    pub(crate) fn finish(self) -> AccountingMetrics {
        self.metrics
    }
}

pub(crate) fn collect_acp_session_update_metrics(
    event: &TrajectoryEvent,
    metrics: &mut CaseMetrics,
    usage: &mut UsageAccumulator,
    warnings: &mut Vec<String>,
    tool_error_ids: &mut BTreeSet<String>,
) {
    let Some(update) = acp_update_value(event) else {
        return;
    };
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("tool_call") => metrics.tool_calls += 1,
        Some("tool_call_update") => {
            if update
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.eq_ignore_ascii_case("failed"))
            {
                let id = update
                    .get("toolCallId")
                    .or_else(|| update.get("tool_call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                if tool_error_ids.insert(id) {
                    metrics.tool_errors += 1;
                }
            }
        }
        Some("usage_update") => usage.add_from_value(update),
        _ => collect_warning(update, warnings),
    }
}

pub(crate) fn acp_update_value(event: &TrajectoryEvent) -> Option<&Value> {
    event.data.get("raw_event")?.get("params")?.get("update")
}

pub(crate) fn collect_psychevo_meta(value: &Value, warnings: &mut Vec<String>, turns: &mut u64) {
    let Some(psychevo) = value.get("_meta").and_then(|meta| meta.get("psychevo")) else {
        return;
    };
    if let Some(meta_turns) = psychevo.get("turns").and_then(json_u64) {
        *turns = turns.saturating_add(meta_turns);
    }
    if let Some(items) = psychevo.get("warnings").and_then(Value::as_array) {
        for item in items {
            if let Some(message) = item.as_str() {
                push_warning(warnings, message);
            } else if let Some(message) = item.get("message").and_then(Value::as_str) {
                push_warning(warnings, message);
            }
        }
    }
}

pub(crate) fn collect_warning(value: &Value, warnings: &mut Vec<String>) {
    let event_type = value.get("type").and_then(Value::as_str);
    if matches!(event_type, Some("warning"))
        && let Some(message) = value.get("message").and_then(Value::as_str)
    {
        push_warning(warnings, message);
    }
}

pub(crate) fn push_warning(warnings: &mut Vec<String>, message: &str) {
    let message = message.trim();
    if !message.is_empty() && !warnings.iter().any(|warning| warning == message) {
        warnings.push(message.to_string());
    }
}

pub(crate) fn tool_call_id_for_event(event: &TrajectoryEvent) -> Option<String> {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    raw.get("tool_call_id")
        .or_else(|| raw.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

pub(crate) fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.parse::<i64>().ok()))
}

pub(crate) fn add_accounting_u64(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
    }
}

pub(crate) fn add_accounting_i64(target: &mut Option<i64>, value: Option<i64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
    }
}

pub(crate) fn merge_accounting_string(target: &mut Option<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    match target {
        None => *target = Some(value.to_string()),
        Some(current) if current == value || current == "mixed" => {}
        Some(current) => *current = "mixed".to_string(),
    }
}

pub(crate) fn event_indicates_tool_error(event: &TrajectoryEvent) -> bool {
    let raw = event.data.get("raw_event").unwrap_or(&event.data);
    raw.get("outcome")
        .and_then(Value::as_str)
        .is_some_and(|value| value != "normal" && value != "ok" && value != "success")
        || raw
            .get("result")
            .and_then(|result| result.get("error"))
            .is_some_and(|value| !value.is_null())
        || raw
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
}

pub(crate) fn aggregate_run_metrics(cases: &[CaseResult], duration_ms: u128) -> RunMetrics {
    let mut usage = UsageMetrics::default();
    let mut has_input = false;
    let mut has_output = false;
    let mut has_cache_read = false;
    let mut has_cache_write = false;
    let mut has_reasoning = false;
    let mut has_total = false;
    let mut total_turns = 0_u64;
    let mut has_turns = false;
    let mut accounting = AccountingAccumulator::default();
    let mut metrics = RunMetrics {
        duration_ms,
        ..RunMetrics::default()
    };
    for case in cases {
        metrics.total_tool_calls += case.metrics.tool_calls;
        metrics.total_tool_errors += case.metrics.tool_errors;
        if let Some(turns) = case.metrics.turns {
            has_turns = true;
            total_turns += turns;
        }
        add_optional_u64(
            &mut usage.input_tokens,
            &mut has_input,
            case.metrics.usage.input_tokens,
        );
        add_optional_u64(
            &mut usage.output_tokens,
            &mut has_output,
            case.metrics.usage.output_tokens,
        );
        add_optional_u64(
            &mut usage.cache_read_tokens,
            &mut has_cache_read,
            case.metrics.usage.cache_read_tokens,
        );
        add_optional_u64(
            &mut usage.cache_write_tokens,
            &mut has_cache_write,
            case.metrics.usage.cache_write_tokens,
        );
        add_optional_u64(
            &mut usage.reasoning_tokens,
            &mut has_reasoning,
            case.metrics.usage.reasoning_tokens,
        );
        add_optional_u64(
            &mut usage.total_tokens,
            &mut has_total,
            case.metrics.usage.total_tokens,
        );
        accounting.add(&serde_json::to_value(&case.metrics.accounting).unwrap_or_default());
        if let Some(amount) = case.metrics.cost.amount_usd {
            metrics.cost.amount_usd = Some(metrics.cost.amount_usd.unwrap_or_default() + amount);
            metrics.cost.source = Some("case_metrics".to_string());
        }
    }
    metrics.total_turns = has_turns.then_some(total_turns);
    metrics.usage = usage;
    metrics.accounting = accounting.finish();
    metrics
}

pub(crate) fn add_optional_u64(target: &mut Option<u64>, seen: &mut bool, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
        *seen = true;
    }
}
