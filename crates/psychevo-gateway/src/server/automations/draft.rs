fn automation_draft_prompt(request: &str, cwd: &str, current_thread_id: Option<&str>) -> String {
    let thread_guidance = match current_thread_id {
        Some(thread_id) => format!(
            r#"A current thread is available. Use {{"kind":"threadHeartbeat","threadId":"{thread_id}"}} only when the user asks to continue, check, or heartbeat the current thread."#
        ),
        None => {
            "No current thread is available. The target must be {\"kind\":\"project\"}.".to_string()
        }
    };
    format!(
        r#"You draft Psychevo local automations from natural language.
Return only one JSON object. Do not use markdown. Do not call tools.

Rules:
- The draft is not saved yet, so produce editable fields for a confirmation form.
- Prefer a project automation unless the user clearly asks for the current thread.
- If no schedule is specified, use {{"kind":"interval","everyMinutes":60}}.
- For one-shot delay schedules use {{"kind":"delay","afterMinutes":30}}.
- For one-shot absolute schedules use {{"kind":"once","at":"YYYY-MM-DDTHH:mm"}} or an RFC3339 timestamp.
- For daily schedules use {{"kind":"daily","time":"HH:mm"}}.
- For weekly schedules use {{"kind":"weekly","weekdays":[1],"time":"HH:mm"}}, where Monday is 1 and Sunday is 7.
- Interval everyMinutes must be at least 1.
- Default execution is {{"policy":"autoSandbox"}}. Use {{"policy":"askFirst"}} only when the user asks to approve first or review before actions.
- Keep the title short and concrete.
- The prompt must be the exact instruction the agent should run every time, not an explanation of the schedule.
- model and reasoningEffort should be null unless the user explicitly asks for one.

Context:
- Cwd: {cwd}
- {thread_guidance}

Output JSON shape:
{{
  "target": {{"kind":"project"}},
  "title": "Morning repository check",
  "prompt": "Review the current repository state and summarize risks that need attention.",
  "schedule": {{"kind":"interval","everyMinutes":60}},
  "execution": {{"policy":"autoSandbox"}},
  "model": null,
  "reasoningEffort": null
}}

User request:
{request}
"#
    )
}

fn parse_automation_draft_response(
    text: &str,
    current_thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::AutomationDraftView> {
    let value = extract_json_object(text)?;
    let mut draft: wire::AutomationDraftView = serde_json::from_value(value)?;
    draft.title = draft.title.trim().to_string();
    if draft.title.is_empty() {
        return Err(Error::Message(
            "automation draft is missing a title".to_string(),
        ));
    }
    draft.prompt = draft.prompt.trim().to_string();
    if draft.prompt.is_empty() {
        return Err(Error::Message(
            "automation draft is missing a prompt".to_string(),
        ));
    }
    match &mut draft.target {
        wire::AutomationTargetInput::Project => {}
        wire::AutomationTargetInput::ThreadHeartbeat { thread_id } => {
            let Some(current_thread_id) = current_thread_id else {
                return Err(Error::Message(
                    "automation draft requested a thread target without a current thread"
                        .to_string(),
                ));
            };
            if thread_id.trim().is_empty() {
                *thread_id = current_thread_id.to_string();
            }
            if thread_id != current_thread_id {
                return Err(Error::Message(
                    "automation draft target thread does not match the current thread".to_string(),
                ));
            }
        }
    }
    let schedule = automation_schedule_from_value(serde_json::to_value(&draft.schedule)?)?;
    next_run_at_ms(&schedule, gateway_now_ms(), None, gateway_now_ms())?;
    draft.model = normalize_optional(draft.model);
    draft.reasoning_effort = normalize_reasoning_effort(draft.reasoning_effort);
    Ok(draft)
}

fn extract_json_object(text: &str) -> psychevo_runtime::Result<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    let unfenced = strip_json_fence(trimmed);
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return Ok(value);
    }
    let start = unfenced.find('{').ok_or_else(|| {
        Error::Message("automation draft response did not contain JSON".to_string())
    })?;
    let end = unfenced.rfind('}').ok_or_else(|| {
        Error::Message("automation draft response did not contain JSON".to_string())
    })?;
    serde_json::from_str(&unfenced[start..=end]).map_err(|err| {
        Error::Message(format!(
            "automation draft response was not valid JSON: {err}"
        ))
    })
}

fn strip_json_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest)
        .trim_start();
    rest.rsplit_once("```")
        .map(|(body, _)| body.trim())
        .unwrap_or(text)
}
