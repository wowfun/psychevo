use serde_json::Value;

use crate::types::{MessageAccounting, ModelCost, ModelCostTier, ModelMetadata};

const CONTEXT_OVER_200K_THRESHOLD: u64 = 200_000;

pub(crate) fn account_usage(
    usage: Option<&Value>,
    metadata: &ModelMetadata,
) -> Option<MessageAccounting> {
    let usage = usage?;
    let input = usage_u64(
        usage,
        &["input_tokens", "prompt_tokens", "context_input_tokens"],
    );
    let output = usage_u64(usage, &["output_tokens", "completion_tokens"]);
    let total = usage_u64(usage, &["total_tokens", "reported_total_tokens"]);
    let reasoning = usage_u64(usage, &["reasoning_tokens"]).unwrap_or(0);
    let cache_read = usage_u64(
        usage,
        &["cached_tokens", "cache_read_tokens", "cached_input_tokens"],
    )
    .unwrap_or(0);
    let cache_write = usage_u64(
        usage,
        &[
            "cache_write_tokens",
            "cache_creation_input_tokens",
            "cache_written_tokens",
        ],
    )
    .unwrap_or(0);
    let input_tokens = input.unwrap_or(0);
    let output_tokens = output.unwrap_or(0);
    let billable_input = input_tokens
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let billable_output = output_tokens.saturating_sub(reasoning);
    let mut accounting = MessageAccounting {
        context_input_tokens: input,
        billable_input_tokens: input.map(|_| billable_input),
        billable_output_tokens: output.map(|_| billable_output),
        reasoning_tokens: (reasoning > 0).then_some(reasoning),
        cache_read_tokens: (cache_read > 0).then_some(cache_read),
        cache_write_tokens: (cache_write > 0).then_some(cache_write),
        reported_total_tokens: total,
        ..Default::default()
    };
    if let Some(cost) = &metadata.cost {
        let use_over_200k = billable_input.saturating_add(cache_read) > CONTEXT_OVER_200K_THRESHOLD;
        let tier = use_over_200k
            .then_some(cost.context_over_200k.as_ref())
            .flatten();
        accounting.pricing_tier = Some(if tier.is_some() {
            "context_over_200k".to_string()
        } else {
            "standard".to_string()
        });
        accounting.pricing_source = cost
            .source
            .clone()
            .or_else(|| metadata.source.clone())
            .or_else(|| Some("unknown".to_string()));
        accounting.estimated_cost_nanodollars = estimate_nanodollars(
            cost,
            tier,
            billable_input,
            billable_output,
            reasoning,
            cache_read,
            cache_write,
        );
    }
    Some(accounting)
}

fn estimate_nanodollars(
    cost: &ModelCost,
    tier: Option<&ModelCostTier>,
    billable_input: u64,
    billable_output: u64,
    reasoning: u64,
    cache_read: u64,
    cache_write: u64,
) -> Option<i64> {
    let input_price = tier.and_then(|tier| tier.input).or(cost.input);
    let output_price = tier.and_then(|tier| tier.output).or(cost.output);
    let cache_read_price = tier
        .and_then(|tier| tier.cache_read)
        .or(cost.cache_read)
        .unwrap_or(0.0);
    let cache_write_price = tier
        .and_then(|tier| tier.cache_write)
        .or(cost.cache_write)
        .unwrap_or(0.0);
    let mut nanodollars = 0.0;
    nanodollars += priced_nanodollars(billable_input, input_price)?;
    nanodollars += priced_nanodollars(billable_output, output_price)?;
    nanodollars += priced_nanodollars(reasoning, output_price)?;
    nanodollars += cache_read as f64 * cache_read_price * 1_000.0;
    nanodollars += cache_write as f64 * cache_write_price * 1_000.0;
    Some(nanodollars.round() as i64)
}

fn priced_nanodollars(tokens: u64, price_per_million: Option<f64>) -> Option<f64> {
    if tokens == 0 {
        Some(0.0)
    } else {
        price_per_million.map(|price| tokens as f64 * price * 1_000.0)
    }
}

fn usage_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}
