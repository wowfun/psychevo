#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn tool_title(tool: &str, value: &Value) -> String {
    if tool == "spawn_agent" {
        return agent_tool_title(value).unwrap_or_else(|| "spawn_agent".to_string());
    }
    if tool == "clarify" {
        return clarify_tool_title(value);
    }
    if is_user_shell_value(value) {
        let command = value
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line);
        return user_shell_title(command);
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    let display = tool_display_spec(tool, value);
    let detail = title_detail_from_keys(&display.title_arg_keys, args).or_else(|| {
        if tool == "exec_command" {
            None
        } else {
            title_detail_from_keys(&display.title_result_keys, result)
        }
    });
    tool_name_title(tool, detail.as_deref())
}

pub(crate) fn clarify_no_answer_result(value: &Value) -> bool {
    if value.get("tool_name").and_then(Value::as_str) != Some("clarify") {
        return false;
    }
    let Some(error) = value
        .get("result")
        .and_then(|result| result.get("error"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    matches!(
        error,
        "clarify was cancelled by the user"
            | "timed out waiting for user input"
            | "clarify was interrupted because the turn ended"
            | "clarify response channel closed"
    )
}

pub(crate) fn active_tool_title(tool: &str, value: &Value) -> String {
    if tool == "spawn_agent" {
        return active_agent_tool_title(value);
    }
    if tool == "clarify" {
        return "Questions pending".to_string();
    }
    if is_user_shell_value(value) {
        let command = value
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line);
        return user_shell_title(command);
    }
    let args = value.get("args").unwrap_or(&Value::Null);
    let display = tool_display_spec(tool, value);
    tool_name_title(
        tool,
        title_detail_from_keys(&display.title_arg_keys, args).as_deref(),
    )
}

pub(crate) fn tool_title_for_update(tool: &str, value: &Value, existing_title: &str) -> String {
    let title = tool_title(tool, value);
    if tool == "exec_command" && matches!(title.as_str(), "exec_command" | "!") {
        let existing = tool_title_as_invocation(
            Some(tool),
            evidence_kind_for_value(tool, value),
            existing_title,
            is_user_shell_value(value),
        );
        if existing != "exec_command" && existing != "!" {
            return existing;
        }
    }
    if title == tool {
        let existing = tool_title_as_invocation(
            Some(tool),
            evidence_kind_for_value(tool, value),
            existing_title,
            false,
        );
        if existing != tool && !existing.starts_with("Tool ") {
            return existing;
        }
    }
    title
}

pub(crate) fn tool_display_spec(tool: &str, value: &Value) -> ToolDisplaySpec {
    value
        .get("display")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| ToolDisplaySpec::for_name(tool))
}

pub(crate) fn title_detail_from_keys(keys: &[String], source: &Value) -> Option<String> {
    for key in keys {
        let Some(value) = source.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if key == "cmd" {
            if let Some(command) = value.as_str().and_then(first_shell_command_line) {
                return Some(command.to_string());
            }
            continue;
        }
        if let Some(detail) = display_value_inline(value).filter(|value| !value.trim().is_empty()) {
            return Some(detail);
        }
    }
    None
}

pub(crate) fn tool_name_title(tool: &str, detail: Option<&str>) -> String {
    let Some(detail) = detail.map(str::trim).filter(|detail| !detail.is_empty()) else {
        return tool.to_string();
    };
    format!("{tool} {detail}")
}

pub(crate) fn user_shell_title(command: Option<&str>) -> String {
    let Some(command) = command.map(str::trim).filter(|command| !command.is_empty()) else {
        return "!".to_string();
    };
    format!("! {command}")
}

pub(crate) fn tool_title_as_invocation(
    tool: Option<&str>,
    kind: TranscriptKind,
    title: &str,
    user_shell: bool,
) -> String {
    let title = title.trim();
    if title.is_empty() {
        return tool
            .map(|tool| tool_name_title(tool, None))
            .unwrap_or_default();
    }
    if user_shell
        || title == "!"
        || title.starts_with("! ")
        || title.starts_with("Ran ! ")
        || title.starts_with("Running ! ")
    {
        return legacy_user_shell_title(title);
    }
    if let Some(tool) = tool
        && (title == tool || title.starts_with(&format!("{tool} ")))
    {
        return title.to_string();
    }
    match kind {
        TranscriptKind::Explored => {
            let tool = tool.unwrap_or("read");
            if matches!(title, "Exploring" | "Explored") {
                return tool.to_string();
            }
            if let Some(rest) = title
                .strip_prefix("Exploring ")
                .or_else(|| title.strip_prefix("Explored "))
            {
                return tool_name_title(tool, Some(rest));
            }
            title.to_string()
        }
        TranscriptKind::Ran => {
            if matches!(title, "Running" | "Ran" | "Running command" | "Ran command") {
                return "exec_command".to_string();
            }
            if let Some(command) = title
                .strip_prefix("Running ")
                .or_else(|| title.strip_prefix("Ran "))
            {
                return tool_name_title("exec_command", Some(command));
            }
            title.to_string()
        }
        TranscriptKind::Updated => {
            let tool = tool.unwrap_or("write");
            if matches!(
                title,
                "Updating" | "Updated" | "Updating files" | "Updated files"
            ) {
                return tool.to_string();
            }
            if let Some(path) = title
                .strip_prefix("Updating ")
                .or_else(|| title.strip_prefix("Updated "))
            {
                let path = path.trim();
                if path == "files" {
                    return tool.to_string();
                }
                return tool_name_title(tool, Some(path));
            }
            title.to_string()
        }
        _ => title.to_string(),
    }
}

pub(crate) fn legacy_user_shell_title(title: &str) -> String {
    let title = title.trim();
    if title == "!" {
        return "!".to_string();
    }
    for prefix in ["Running ! ", "Ran ! ", "! "] {
        if let Some(command) = title.strip_prefix(prefix) {
            return user_shell_title(Some(command));
        }
    }
    title.to_string()
}

pub(crate) fn is_user_shell_value(value: &Value) -> bool {
    value.get("source").and_then(Value::as_str) == Some("user_shell")
}

pub(crate) fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}
