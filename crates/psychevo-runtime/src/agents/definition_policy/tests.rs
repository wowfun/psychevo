
impl ToolBinding for HookedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.inner.execution_mode()
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        self.inner.display_spec()
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let inner = Arc::clone(&self.inner);
        let hooks = self.hooks.clone();
        let agent_name = self.agent_name.clone();
        let tool_name = self.inner.name().to_string();
        let workdir = self.workdir.clone();
        Box::pin(async move {
            let pre_payload = json!({
                "event": "PreToolUse",
                "agent": agent_name,
                "tool": tool_name,
                "tool_call_id": tool_call_id.clone(),
                "arguments": args.clone(),
            });
            if let Some(blocked) =
                run_hook_commands(hooks.as_ref(), "PreToolUse", &workdir, &pre_payload)
            {
                return ToolOutput::error(blocked);
            }

            let output = inner
                .execute(tool_call_id.clone(), args.clone(), abort)
                .await;
            let post_payload = json!({
                "event": "PostToolUse",
                "agent": agent_name,
                "tool": tool_name,
                "tool_call_id": tool_call_id,
                "arguments": args.clone(),
                "output": output.json.clone(),
                "is_error": output.is_error,
            });
            let _ = run_hook_commands(hooks.as_ref(), "PostToolUse", &workdir, &post_payload);
            output
        })
    }
}

impl ToolBinding for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Spawn a focused child agent. Named agents start with fresh context by default; set fork_context true to include the parent context snapshot."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_type": {
                    "type": "string",
                    "description": "Agent definition name to run. Defaults to general when omitted and no @agent mention requires a specific target."
                },
                "prompt": {
                    "type": "string",
                    "description": "Complete task instructions for the child agent."
                },
                "task_name": {
                    "type": "string",
                    "description": "Optional durable task label used later by wait/send/close/resume control tools; does not select the agent definition."
                },
                "background": {
                    "type": "boolean",
                    "description": "When true, return a handle immediately and deliver completion through the parent mailbox; false waits for the child summary."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this child run; omitted means inherit the resolved model."
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "When true, include a snapshot of parent context instead of starting with fresh child context."
                },
                "fork_turns": {
                    "type": "string",
                    "description": "Parent-context slice for fork_context: none, all, or a positive integer count of recent parent messages."
                },
                "max_turns": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum model turns for the child run; omitted uses the agent definition or runtime default."
                },
                "max_spawn_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_AGENT_SPAWN_DEPTH_CAP,
                    "description": "Additional descendant spawn levels this child may create. 0 makes it a leaf; values above the runtime cap are rejected."
                }
            },
            "required": ["prompt"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let context = self.context.clone();
        Box::pin(async move {
            let parsed: AgentToolArgs = match serde_json::from_value(args) {
                Ok(args) => args,
                Err(err) => {
                    return ToolOutput::error(format!("invalid Agent arguments: {err}"));
                }
            };
            match spawn_subagent(context, parsed, tool_call_id, abort).await {
                Ok(output) => output,
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}
