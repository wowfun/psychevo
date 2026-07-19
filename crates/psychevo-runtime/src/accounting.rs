use serde_json::Value;

use crate::types::{CostStatus, MessageAccounting, ModelCost, ModelCostTier, ModelMetadata};

pub(crate) const CONTEXT_OVER_200K_THRESHOLD: u64 = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageTotalStatus {
    Reported,
    Derived,
    Partial,
    Unavailable,
}

impl UsageTotalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Derived => "derived",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveUsageTotal {
    pub tokens: Option<u64>,
    pub reported_tokens: Option<u64>,
    pub status: UsageTotalStatus,
}

pub fn effective_usage_total(usage: Option<&Value>) -> EffectiveUsageTotal {
    let Some(usage) = usage else {
        return effective_usage_total_from_parts(None, None, None);
    };
    effective_usage_total_from_parts(
        usage_u64(
            usage,
            &[
                "total_tokens",
                "reported_total_tokens",
                "totalTokens",
                "total",
            ],
        ),
        usage_u64(
            usage,
            &[
                "input_tokens",
                "prompt_tokens",
                "context_input_tokens",
                "inputTokens",
                "input",
            ],
        ),
        usage_u64(
            usage,
            &[
                "output_tokens",
                "completion_tokens",
                "outputTokens",
                "output",
            ],
        ),
    )
}

pub fn effective_usage_total_from_parts(
    reported_tokens: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
) -> EffectiveUsageTotal {
    if let Some(tokens) = reported_tokens {
        return EffectiveUsageTotal {
            tokens: Some(tokens),
            reported_tokens: Some(tokens),
            status: UsageTotalStatus::Reported,
        };
    }
    match (input_tokens, output_tokens) {
        (Some(input), Some(output)) => EffectiveUsageTotal {
            tokens: Some(input.saturating_add(output)),
            reported_tokens: None,
            status: UsageTotalStatus::Derived,
        },
        (Some(tokens), None) | (None, Some(tokens)) => EffectiveUsageTotal {
            tokens: Some(tokens),
            reported_tokens: None,
            status: UsageTotalStatus::Partial,
        },
        (None, None) => EffectiveUsageTotal {
            tokens: None,
            reported_tokens: None,
            status: UsageTotalStatus::Unavailable,
        },
    }
}

pub(crate) fn account_usage(
    usage: Option<&Value>,
    metadata: &ModelMetadata,
) -> Option<MessageAccounting> {
    let usage = usage?;
    let input = usage_u64(
        usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "context_input_tokens",
            "inputTokens",
        ],
    );
    let output = usage_u64(
        usage,
        &["output_tokens", "completion_tokens", "outputTokens"],
    );
    let total = effective_usage_total(Some(usage)).reported_tokens;
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
        accounting.pricing_version = cost.version.clone();
        match estimate_nanodollars(
            cost,
            tier,
            billable_input,
            billable_output,
            reasoning,
            cache_read,
            cache_write,
        ) {
            Ok(estimated) => {
                accounting.cost_status = Some(if estimated == 0 {
                    CostStatus::Free
                } else {
                    CostStatus::Estimated
                });
                accounting.estimated_cost_nanodollars = Some(estimated);
            }
            Err(reason) => {
                accounting.cost_status = Some(CostStatus::Unknown);
                accounting.pricing_missing_reason = Some(reason.to_string());
            }
        }
    } else {
        accounting.cost_status = Some(CostStatus::Unknown);
        accounting.pricing_missing_reason = Some("missing_model_cost".to_string());
    }
    Some(accounting)
}

pub(crate) fn estimate_nanodollars(
    cost: &ModelCost,
    tier: Option<&ModelCostTier>,
    billable_input: u64,
    billable_output: u64,
    reasoning: u64,
    cache_read: u64,
    cache_write: u64,
) -> Result<i64, &'static str> {
    let input_price = tier.and_then(|tier| tier.input).or(cost.input);
    let output_price = tier.and_then(|tier| tier.output).or(cost.output);
    let cache_read_price = tier.and_then(|tier| tier.cache_read).or(cost.cache_read);
    let cache_write_price = tier.and_then(|tier| tier.cache_write).or(cost.cache_write);
    let mut nanodollars = 0.0;
    nanodollars += priced_nanodollars(billable_input, input_price, "missing_input_price")?;
    nanodollars += priced_nanodollars(billable_output, output_price, "missing_output_price")?;
    nanodollars += priced_nanodollars(reasoning, output_price, "missing_output_price")?;
    nanodollars += priced_nanodollars(cache_read, cache_read_price, "missing_cache_read_price")?;
    nanodollars += priced_nanodollars(cache_write, cache_write_price, "missing_cache_write_price")?;
    if let Some(request_price) = cost.request {
        nanodollars += request_price * 1_000_000_000.0;
    }
    Ok(nanodollars.round() as i64)
}

pub(crate) fn priced_nanodollars(
    tokens: u64,
    price_per_million: Option<f64>,
    missing_reason: &'static str,
) -> Result<f64, &'static str> {
    if tokens == 0 {
        Ok(0.0)
    } else {
        price_per_million
            .map(|price| tokens as f64 * price * 1_000.0)
            .ok_or(missing_reason)
    }
}

pub(crate) fn usage_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}
