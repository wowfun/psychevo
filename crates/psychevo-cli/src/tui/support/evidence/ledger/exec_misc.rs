#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn exec_timeout_output_text(value: &Value) -> Option<String> {
    if value.get("tool_name").and_then(Value::as_str) != Some("exec_command") {
        return None;
    }
    let result = value.get("result").unwrap_or(&Value::Null);
    let error = result.get("error").and_then(Value::as_str)?;
    if !error.starts_with("command timed out after ") {
        return None;
    }
    let prompt = format!("timeout: {error}");
    let output = result
        .get("output")
        .and_then(Value::as_str)
        .filter(|output| {
            let trimmed = output.trim();
            !trimmed.is_empty() && trimmed != "(no output)"
        });
    Some(match output {
        Some(output) => format!("{prompt}; partial output follows\n{output}"),
        None => prompt,
    })
}

pub(crate) fn tool_event_interrupted(value: &Value) -> bool {
    value.get("outcome").and_then(Value::as_str) == Some("aborted")
        || value
            .get("result")
            .and_then(|result| result.get("error"))
            .and_then(Value::as_str)
            == Some("aborted")
}

pub(crate) fn model_label(provider: &str, model: &str) -> String {
    match (provider.is_empty(), model.is_empty()) {
        (false, false) => format!("{provider}/{model}"),
        (false, true) => provider.to_string(),
        (true, false) => model.to_string(),
        (true, true) => String::new(),
    }
}
