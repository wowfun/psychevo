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

    fn exposure(&self) -> psychevo_agent_core::ToolExposure {
        self.inner.exposure()
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
        let hook_runtime = self.hook_runtime.clone();
        let tool_name = self.inner.name().to_string();
        Box::pin(async move {
            let pre_payload = json!({
                "event": "PreToolUse",
                "tool": tool_name,
                "tool_call_id": tool_call_id.clone(),
                "arguments": args.clone(),
            });
            let pre_result = hook_runtime.run_pre_tool_use(&pre_payload);
            if let Some(blocked) = pre_result.block_reason {
                return ToolOutput::error(blocked);
            }
            let effective_args = pre_result.updated_input.unwrap_or(args);

            let output = inner
                .execute(tool_call_id.clone(), effective_args.clone(), abort)
                .await;
            let post_payload = json!({
                "event": "PostToolUse",
                "tool": tool_name,
                "tool_call_id": tool_call_id,
                "arguments": effective_args,
                "output": output.json.clone(),
                "is_error": output.is_error,
            });
            let post_result = hook_runtime.run_post_tool_use(&post_payload);
            match post_result.model_content {
                Some(model_content) => output.with_model_content(model_content),
                None => output,
            }
        })
    }
}

impl ToolBinding for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Spawn a focused child agent thread. Provide a canonical task_name and a complete message for the child agent."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_type": {
                    "type": "string",
                    "description": "Agent definition name to run. Defaults to general when omitted and no @agent mention requires a specific target."
                },
                "task_name": {
                    "type": "string",
                    "pattern": "^[a-z0-9_]+$",
                    "description": "Required canonical task key using lowercase ASCII letters, digits, and underscores only."
                },
                "message": {
                    "type": "string",
                    "description": "Complete task instructions for the child agent."
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
            "required": ["task_name", "message"],
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
            let parsed: SpawnAgentArgs = match serde_json::from_value(args) {
                Ok(args) => args,
                Err(err) => {
                    return ToolOutput::error(format!("invalid spawn_agent arguments: {err}"));
                }
            };
            match spawn_subagent(context, parsed, tool_call_id, abort).await {
                Ok(output) => output,
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}
