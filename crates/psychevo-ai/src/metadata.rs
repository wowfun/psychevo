pub fn normalize_usage(usage: &Value) -> Option<Value> {
    let object = usage.as_object()?;
    let mut out = serde_json::Map::new();
    copy_number_field(
        object,
        &mut out,
        &["input_tokens", "prompt_tokens", "input"],
        "input_tokens",
    );
    copy_number_field(
        object,
        &mut out,
        &["output_tokens", "completion_tokens", "output"],
        "output_tokens",
    );
    copy_number_field(object, &mut out, &["total_tokens", "total"], "total_tokens");
    if let Some(reasoning_tokens) = first_nested_number(
        usage,
        &[
            &["reasoning_tokens"],
            &["completion_tokens_details", "reasoning_tokens"],
            &["output_tokens_details", "reasoning_tokens"],
        ],
    ) {
        out.insert("reasoning_tokens".to_string(), reasoning_tokens);
    }
    if let Some(cached_tokens) = first_nested_number(
        usage,
        &[
            &["cached_tokens"],
            &["cache_read_tokens"],
            &["prompt_tokens_details", "cached_tokens"],
            &["input_tokens_details", "cached_tokens"],
            &["input_tokens_details", "cache_read_tokens"],
        ],
    ) {
        out.insert("cached_tokens".to_string(), cached_tokens);
    }
    if let Some(cache_write_tokens) = first_nested_number(
        usage,
        &[
            &["cache_write_tokens"],
            &["cache_creation_input_tokens"],
            &["prompt_tokens_details", "cache_write_tokens"],
            &["prompt_tokens_details", "cache_creation_input_tokens"],
            &["input_tokens_details", "cache_write_tokens"],
            &["input_tokens_details", "cache_creation_input_tokens"],
        ],
    ) {
        out.insert("cache_write_tokens".to_string(), cache_write_tokens);
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

pub fn allowlisted_provider_metadata(metadata: &Value) -> Option<Value> {
    let object = metadata.as_object()?;
    let mut out = serde_json::Map::new();
    for key in [
        "provider_response_id",
        "response_id",
        "model",
        "system_fingerprint",
        "service_tier",
        "created",
        "finish_reason",
        "request_id",
    ] {
        if let Some(value) = object.get(key)
            && is_safe_metadata_value(value)
        {
            out.insert(key.to_string(), value.clone());
        }
    }
    if !out.contains_key("provider_response_id")
        && let Some(value) = object.get("id")
        && is_safe_metadata_value(value)
    {
        out.insert("provider_response_id".to_string(), value.clone());
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn copy_number_field(
    object: &serde_json::Map<String, Value>,
    out: &mut serde_json::Map<String, Value>,
    candidates: &[&str],
    target: &str,
) {
    if let Some(value) = candidates.iter().find_map(|key| object.get(*key))
        && value.as_i64().is_some()
    {
        out.insert(target.to_string(), value.clone());
    }
}

fn first_nested_number(value: &Value, paths: &[&[&str]]) -> Option<Value> {
    paths.iter().find_map(|path| {
        let mut current = value;
        for key in *path {
            current = current.get(*key)?;
        }
        current.as_i64().map(|_| current.clone())
    })
}

fn is_safe_metadata_value(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
    )
}
