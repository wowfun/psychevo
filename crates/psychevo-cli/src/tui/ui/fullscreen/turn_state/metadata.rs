
pub(crate) fn exec_row_full_text_without_history_marker(row: &TranscriptRow) -> String {
    let full = if row.full_text.is_some() {
        row.full_text.clone().unwrap_or_default()
    } else if matches!(row.text.as_str(), "running" | "preparing") {
        String::new()
    } else {
        row.text.clone()
    };
    strip_exec_history_running_marker(full)
}

pub(crate) fn set_exec_row_text(row: &mut TranscriptRow, full: String) {
    if full.is_empty() {
        row.text.clear();
        row.full_text = None;
        row.expanded = false;
        row.details_collapsed = false;
        return;
    }
    row.set_evidence_body_text(full);
}

pub(crate) fn with_exec_history_running_marker(mut full: String) -> String {
    if full.trim().is_empty() {
        return "last seen running".to_string();
    }
    if !full.ends_with('\n') {
        full.push('\n');
    }
    full.push_str("last seen running");
    full
}

pub(crate) fn strip_exec_history_running_marker(mut full: String) -> String {
    if full == "last seen running" {
        return String::new();
    }
    if full.ends_with("\nlast seen running") {
        let new_len = full.len() - "\nlast seen running".len();
        full.truncate(new_len);
    }
    full
}

pub(crate) fn clarify_request_args_value(request: &ClarifyRequestEvent) -> Value {
    serde_json::json!({
        "questions": request
            .questions
            .iter()
            .map(|question| {
                serde_json::json!({
                    "question": question.question.clone(),
                    "options": question
                        .options
                        .iter()
                        .map(|option| {
                            serde_json::json!({
                                "label": option.label.clone(),
                                "description": option.description.clone(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
    })
}

pub(crate) fn selected_skill_names_from_event(value: &Value) -> Option<Vec<String>> {
    value.get("selected_skills")?.as_array().map(|skills| {
        skills
            .iter()
            .filter_map(|skill| skill.get("name").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect()
    })
}
