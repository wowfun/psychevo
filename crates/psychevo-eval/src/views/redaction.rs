#[allow(unused_imports)]
use super::*;

pub(crate) fn analysis_summary_from_preview(preview: &str) -> Option<String> {
    serde_json::from_str::<Value>(preview)
        .ok()
        .and_then(|value| {
            value
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub(crate) fn has_accounting(accounting: &AccountingMetrics) -> bool {
    accounting.context_input_tokens.is_some()
        || accounting.billable_input_tokens.is_some()
        || accounting.billable_output_tokens.is_some()
        || accounting.reasoning_tokens.is_some()
        || accounting.cache_read_tokens.is_some()
        || accounting.cache_write_tokens.is_some()
        || accounting.reported_total_tokens.is_some()
        || accounting.estimated_cost_nanodollars.is_some()
        || accounting.pricing_source.is_some()
        || accounting.pricing_tier.is_some()
}

pub(crate) fn has_usage(usage: &UsageMetrics) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.cache_read_tokens.is_some()
        || usage.cache_write_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.total_tokens.is_some()
}

pub(crate) fn truncate_chars_with_flag(value: &str, max_chars: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            truncated = true;
            break;
        }
        out.push(ch);
    }
    (out, truncated)
}

pub(crate) fn redact_preview_text(value: &str) -> String {
    const SECRET_MARKERS: [&str; 7] = [
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "password",
        "secret",
        "token",
    ];
    value
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if SECRET_MARKERS.iter().any(|marker| lower.contains(marker)) {
                "[redacted sensitive line]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
