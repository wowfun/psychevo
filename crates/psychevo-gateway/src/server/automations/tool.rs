#[derive(Clone)]
struct AutomationTool {
    state: WebState,
    cwd: PathBuf,
    current_thread_id: Option<String>,
}

impl ToolBinding for AutomationTool {
    fn name(&self) -> &str {
        "automation"
    }

    fn description(&self) -> &str {
        "Create, list, update, pause, resume, run, or remove Psychevo local automations. Use this when the user asks in natural language to schedule or manage recurring or one-shot work. If the user does not specify a target, create automations for the Gateway-supplied current thread when available; otherwise use the current project."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "update", "pause", "resume", "run", "remove"]
                },
                "automationId": {
                    "type": "string",
                    "description": "Required for update, pause, resume, run, and remove"
                },
                "title": { "type": "string" },
                "prompt": { "type": "string" },
                "target": {
                    "description": "Omit target for the default. Use \"currentThread\" only when this turn has a Gateway-supplied current thread. Use \"project\" for the current project, or an object like {\"kind\":\"threadHeartbeat\",\"threadId\":\"...\"}.",
                    "oneOf": [
                        { "type": "string", "enum": ["project", "currentThread"] },
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["project", "threadHeartbeat"] },
                                "threadId": { "type": "string" }
                            }
                        }
                    ]
                },
                "threadId": {
                    "type": "string",
                    "description": "Shortcut for target {\"kind\":\"threadHeartbeat\",\"threadId\":\"...\"}"
                },
                "schedule": {
                    "type": "object",
                    "description": "One of interval {kind,everyMinutes}, delay {kind,afterMinutes}, once {kind,at}, daily {kind,time}, weekly {kind,weekdays,time}"
                },
                "execution": {
                    "type": "object",
                    "properties": {
                        "policy": { "type": "string", "enum": ["autoSandbox", "askFirst"] }
                    }
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match tool.execute_automation_action(args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

impl AutomationTool {
    fn execute_automation_action(&self, args: Value) -> psychevo_runtime::Result<Value> {
        let action = tool_string(&args, "action")?;
        match action.as_str() {
            "list" => self.list(),
            "create" => self.create_or_update(args, None),
            "update" => {
                let automation_id = tool_string(&args, "automationId")?;
                self.create_or_update(args, Some(automation_id))
            }
            "pause" => self.set_enabled(args, false),
            "resume" => self.set_enabled(args, true),
            "run" => {
                let automation_id = tool_string(&args, "automationId")?;
                let (tx, _rx) = mpsc::unbounded_channel();
                let value = automation_run_result(
                    self.state.clone(),
                    &AuthContext::Bearer,
                    wire::AutomationRunParams {
                        automation_id,
                        trigger: Some("tool".to_string()),
                    },
                    tx,
                )?;
                Ok(tool_result(action, value))
            }
            "remove" => {
                let automation_id = tool_string(&args, "automationId")?;
                let value = automation_delete_result(
                    &self.state,
                    &AuthContext::Bearer,
                    wire::AutomationIdParams { automation_id },
                )?;
                Ok(tool_result(action, value))
            }
            other => Err(Error::Message(format!(
                "unknown automation action: {other}"
            ))),
        }
    }

    fn list(&self) -> psychevo_runtime::Result<Value> {
        let value = automation_list_result(
            &self.state,
            &AuthContext::Bearer,
            wire::AutomationListParams {
                cwd: Some(self.cwd.display().to_string()),
            },
        )?;
        Ok(tool_result("list", value))
    }

    fn create_or_update(
        &self,
        args: Value,
        automation_id: Option<String>,
    ) -> psychevo_runtime::Result<Value> {
        let existing = automation_id
            .as_deref()
            .map(|id| automation_task_for_request(&self.state, &AuthContext::Bearer, id))
            .transpose()?;
        let params = self.write_params_from_args(&args, automation_id, existing.as_ref())?;
        let value = automation_write_result(&self.state, &AuthContext::Bearer, params)?;
        Ok(tool_result(
            if existing.is_some() {
                "update"
            } else {
                "create"
            },
            value,
        ))
    }

    fn set_enabled(&self, args: Value, enabled: bool) -> psychevo_runtime::Result<Value> {
        let automation_id = tool_string(&args, "automationId")?;
        let value = automation_set_enabled_result(
            &self.state,
            &AuthContext::Bearer,
            wire::AutomationIdParams { automation_id },
            enabled,
        )?;
        Ok(tool_result(if enabled { "resume" } else { "pause" }, value))
    }

    fn write_params_from_args(
        &self,
        args: &Value,
        automation_id: Option<String>,
        existing: Option<&AutomationTaskRecord>,
    ) -> psychevo_runtime::Result<wire::AutomationWriteParams> {
        let title = optional_tool_string(args, "title")
            .or_else(|| existing.map(|record| record.title.clone()))
            .ok_or_else(|| Error::Message("automation title is required".to_string()))?;
        let prompt = optional_tool_string(args, "prompt")
            .or_else(|| existing.map(|record| record.prompt.clone()))
            .ok_or_else(|| Error::Message("automation prompt is required".to_string()))?;
        let schedule = match args.get("schedule") {
            Some(value) => serde_json::from_value(value.clone())?,
            None => existing
                .map(|record| serde_json::from_value(record.schedule.clone()))
                .transpose()?
                .ok_or_else(|| Error::Message("automation schedule is required".to_string()))?,
        };
        let target = if args.get("target").is_some() || args.get("threadId").is_some() {
            self.target_from_args(args)?
        } else if let Some(existing) = existing {
            match automation_kind_from_str(&existing.kind)? {
                wire::AutomationTaskKind::Project => wire::AutomationTargetInput::Project,
                wire::AutomationTaskKind::ThreadHeartbeat => {
                    wire::AutomationTargetInput::ThreadHeartbeat {
                        thread_id: existing.target_thread_id.clone().ok_or_else(|| {
                            Error::Message(
                                "thread automation is missing a target thread".to_string(),
                            )
                        })?,
                    }
                }
            }
        } else if let Some(thread_id) = self.current_thread_id.clone() {
            wire::AutomationTargetInput::ThreadHeartbeat { thread_id }
        } else {
            wire::AutomationTargetInput::Project
        };
        let execution = match args.get("execution") {
            Some(value) => Some(serde_json::from_value(value.clone())?),
            None => existing
                .map(|record| automation_execution_from_value(record.execution.clone()))
                .transpose()?,
        };
        Ok(wire::AutomationWriteParams {
            automation_id,
            scope: Some(self.scope()),
            target,
            title,
            prompt,
            schedule,
            execution,
            model: optional_tool_string(args, "model")
                .or_else(|| existing.and_then(|record| record.model.clone())),
            reasoning_effort: optional_tool_string(args, "reasoningEffort")
                .or_else(|| existing.and_then(|record| record.reasoning_effort.clone())),
        })
    }

    fn target_from_args(
        &self,
        args: &Value,
    ) -> psychevo_runtime::Result<wire::AutomationTargetInput> {
        if let Some(thread_id) = optional_tool_string(args, "threadId") {
            return Ok(wire::AutomationTargetInput::ThreadHeartbeat { thread_id });
        }
        match args.get("target") {
            Some(Value::String(value)) if value == "project" => {
                Ok(wire::AutomationTargetInput::Project)
            }
            Some(Value::String(value)) if value == "currentThread" => {
                let thread_id = self
                    .current_thread_id
                    .clone()
                    .ok_or_else(|| Error::Message("current thread is not available".to_string()))?;
                Ok(wire::AutomationTargetInput::ThreadHeartbeat { thread_id })
            }
            Some(value) => serde_json::from_value(value.clone()).map_err(Into::into),
            None => Ok(wire::AutomationTargetInput::Project),
        }
    }

    fn scope(&self) -> wire::GatewayRequestScope {
        wire::GatewayRequestScope {
            cwd: self.cwd.display().to_string(),
            source: wire::GatewaySourceInput {
                kind: "web".to_string(),
                raw_id: None,
                lifetime: Some(wire::GatewaySourceLifetime::Persistent),
                raw_identity: None,
                visible_name: None,
            },
        }
    }
}

#[cfg(test)]
pub(super) fn automation_tool_execute_for_test(
    state: WebState,
    cwd: PathBuf,
    current_thread_id: Option<String>,
    args: Value,
) -> psychevo_runtime::Result<Value> {
    AutomationTool {
        state,
        cwd,
        current_thread_id,
    }
    .execute_automation_action(args)
}

fn tool_string(args: &Value, key: &str) -> psychevo_runtime::Result<String> {
    optional_tool_string(args, key)
        .ok_or_else(|| Error::Message(format!("automation tool requires `{key}`")))
}

fn optional_tool_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn tool_result(action: impl Into<String>, value: Value) -> Value {
    let mut value = value;
    let action = action.into();
    if let Value::Object(map) = &mut value {
        map.insert("success".to_string(), Value::Bool(true));
        map.insert("action".to_string(), Value::String(action));
        return value;
    }
    json!({ "success": true, "action": action, "result": value })
}
