#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn tool_output_text(value: &Value) -> (String, Option<String>) {
    if let Some(timeout) = exec_timeout_output_text(value) {
        return collapse_ledger_body(&timeout);
    }
    if value.get("tool_name").and_then(Value::as_str) == Some("spawn_agent")
        && let Some(output) = agent_tool_output_text(value)
    {
        return output;
    }
    if value.get("tool_name").and_then(Value::as_str) == Some("clarify") {
        return (clarify_tool_output_text(value), None);
    }
    let tool = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let display = tool_display_spec(tool, value);
    let result = value.get("result").unwrap_or(&Value::Null);
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return collapse_ledger_body(error);
    }
    if display.body_policy == ToolDisplayBodyPolicy::Body
        && let Some(body) = body_text_from_keys(&display.body_keys, result)
    {
        return collapse_ledger_body(&body);
    }
    let summary = format_tool_summary(value);
    let detail = body_text_from_keys(&display.body_keys, result);
    match detail {
        Some(detail) if detail != summary => {
            (summary.clone(), Some(format!("{summary}\n\n{detail}")))
        }
        _ => (summary, None),
    }
}

pub(crate) fn format_tool_summary(value: &Value) -> String {
    let tool = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let outcome = value
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("normal");
    let result = value.get("result").unwrap_or(&Value::Null);
    let display = tool_display_spec(tool, value);
    let summary = summarize_tool_result(&display, result);
    if summary.is_empty() {
        format!("{tool} {outcome}")
    } else {
        format!("{tool} {outcome}: {summary}")
    }
}

pub(crate) fn summarize_tool_result(display: &ToolDisplaySpec, result: &Value) -> String {
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return truncate_inline(error, 140);
    }
    let mut parts = Vec::new();
    for key in &display.summary_keys {
        let Some(value) = result.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let Some(display) = display_value_inline(value) else {
            continue;
        };
        if display.trim().is_empty() {
            continue;
        }
        parts.push(format!("{key}={}", truncate_inline(&display, 60)));
        if parts.len() >= 4 {
            break;
        }
    }
    truncate_inline(&parts.join(" "), 180)
}

pub(crate) fn body_text_from_keys(keys: &[String], result: &Value) -> Option<String> {
    let mut bodies = Vec::new();
    for key in keys {
        let Some(value) = result.get(key) else {
            continue;
        };
        let Some(text) = display_value_block(value).filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        bodies.push((key.as_str(), text));
    }
    match bodies.as_slice() {
        [] => None,
        [(_, text)] => Some(text.clone()),
        many => Some(
            many.iter()
                .map(|(key, text)| format!("{key}:\n{text}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

pub(crate) fn display_value_inline(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.split_whitespace().collect::<Vec<_>>().join(" ")),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        Value::Array(items) if items.is_empty() => None,
        Value::Object(items) if items.is_empty() => None,
        other => serde_json::to_string(other).ok(),
    }
}

pub(crate) fn display_value_block(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Array(items) if items.is_empty() => None,
        Value::Object(items) if items.is_empty() => None,
        other => serde_json::to_string_pretty(other)
            .or_else(|_| serde_json::to_string(other))
            .ok(),
    }
}

pub(crate) fn truncate_inline(input: &str, max_chars: usize) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = normalized
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

pub(crate) fn clarify_tool_title(value: &Value) -> String {
    if value
        .get("result")
        .and_then(|result| result.get("error"))
        .is_some()
        && !clarify_no_answer_result(value)
    {
        return "Clarify failed".to_string();
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    let total = args
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let answered = result
        .get("answers")
        .and_then(Value::as_array)
        .map(|answers| {
            answers
                .iter()
                .filter(|answer| {
                    answer
                        .get("answers")
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
                .count()
        })
        .unwrap_or_default();
    if total == 0 && answered > 0 {
        format!("Questions {answered}/{answered} answered")
    } else if total == 0 {
        "Questions 0/0 answered".to_string()
    } else {
        format!("Questions {answered}/{total} answered")
    }
}

pub(crate) fn clarify_tool_output_text(value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    if let Some(error) = result.get("error").and_then(Value::as_str)
        && !clarify_no_answer_result(value)
    {
        return error.to_string();
    }
    let questions = args
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let answers = result.get("answers").and_then(Value::as_array);
    let mut lines = Vec::new();
    for (index, question) in questions.iter().enumerate() {
        let text = question
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or("question");
        let Some(answer_items) = answers
            .and_then(|answers| answers.get(index))
            .and_then(|answer| answer.get("answers"))
            .and_then(Value::as_array)
        else {
            lines.push(format!("{text} (unanswered)"));
            continue;
        };
        if answer_items.is_empty() {
            lines.push(format!("{text} (unanswered)"));
            continue;
        }
        lines.push(text.to_string());
        for item in answer_items {
            let Some(answer) = item.as_str() else {
                continue;
            };
            if let Some(note) = answer.strip_prefix("user_note: ") {
                lines.push(format!("note: {note}"));
            } else {
                lines.push(format!("answer: {answer}"));
            }
        }
    }
    if lines.is_empty() {
        format_tool_summary(value)
    } else {
        lines.join("\n")
    }
}
