use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_ai::Outcome;
use psychevo_gateway::{Gateway, GatewayInputPart, GatewaySource, ThreadTurnRequest};
use psychevo_runtime::state::StateRuntime;
use psychevo_runtime::{
    types::PermissionMode, types::ProjectContextInstructionMode, types::RunMode,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::args::{McpArgs, McpCommand, McpServeArgs, PermissionModeArg};
use crate::commands::run::interactive_approval_handler;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) async fn run_mcp_command(args: McpArgs) -> Result<ExitCode> {
    match args.command {
        McpCommand::Serve(args) => {
            run_mcp_stdio(CliMcpRunner { args }).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

trait PsychevoMcpRunner: Send + Sync {
    fn run_turn(
        &self,
        request: PsychevoMcpTurnRequest,
    ) -> BoxFuture<'static, Result<PsychevoMcpTurnResult>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PsychevoMcpTurnRequest {
    prompt: String,
    session_id: Option<String>,
    cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PsychevoMcpTurnResult {
    session_id: String,
    final_answer: String,
    outcome: String,
    tool_failures: usize,
}

#[derive(Clone)]
struct CliMcpRunner {
    args: McpServeArgs,
}

impl PsychevoMcpRunner for CliMcpRunner {
    fn run_turn(
        &self,
        request: PsychevoMcpTurnRequest,
    ) -> BoxFuture<'static, Result<PsychevoMcpTurnResult>> {
        let args = self.args.clone();
        Box::pin(async move { run_cli_mcp_turn(args, request).await })
    }
}

async fn run_mcp_stdio(runner: impl PsychevoMcpRunner + 'static) -> Result<()> {
    let runner = Arc::new(runner);
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(line) {
            Ok(message) => handle_mcp_jsonrpc(runner.as_ref(), message).await,
            Err(err) => Some(jsonrpc_error(
                Value::Null,
                -32700,
                format!("parse error: {err}"),
            )),
        };
        if let Some(response) = response {
            stdout
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

async fn handle_mcp_jsonrpc(runner: &dyn PsychevoMcpRunner, message: Value) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(jsonrpc_error(
            Value::Null,
            -32600,
            "request must be an object",
        ));
    };
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return id.map(|id| jsonrpc_error(id, -32600, "request.method is required"));
    };
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "notifications/initialized" => None,
        "initialize" => id.map(|id| jsonrpc_response(id, initialize_result(&params))),
        "ping" => id.map(|id| jsonrpc_response(id, json!({}))),
        "tools/list" => id.map(|id| {
            jsonrpc_response(
                id,
                json!({
                    "tools": psychevo_mcp_tools(),
                    "nextCursor": null,
                }),
            )
        }),
        "tools/call" => {
            let id = id?;
            Some(handle_mcp_tool_call(runner, id, params).await)
        }
        _ => id.map(|id| jsonrpc_error(id, -32601, format!("method not found: {method}"))),
    }
}

async fn handle_mcp_tool_call(runner: &dyn PsychevoMcpRunner, id: Value, params: Value) -> Value {
    let Some(params) = params.as_object() else {
        return jsonrpc_error(id, -32602, "tools/call params must be an object");
    };
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return jsonrpc_error(id, -32602, "tools/call params.name is required");
    };
    let arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match name {
        "psychevo" | "psychevo-reply" => {
            let prompt = match string_argument(&arguments, "prompt") {
                Some(prompt) => prompt,
                None => {
                    return jsonrpc_response(
                        id,
                        mcp_tool_error_result("Missing required `prompt` argument."),
                    );
                }
            };
            let session_id = session_argument(&arguments);
            if name == "psychevo-reply" && session_id.is_none() {
                return jsonrpc_response(
                    id,
                    mcp_tool_error_result(
                        "Missing required session argument for psychevo-reply; provide one of `sessionId`, `session_id`, `threadId`, or `thread_id`.",
                    ),
                );
            }
            let cwd = string_argument(&arguments, "cwd").map(PathBuf::from);
            match runner
                .run_turn(PsychevoMcpTurnRequest {
                    prompt,
                    session_id,
                    cwd,
                })
                .await
            {
                Ok(result) => jsonrpc_response(id, mcp_tool_success_result(result)),
                Err(err) => jsonrpc_response(id, mcp_tool_error_result(format!("{err:#}"))),
            }
        }
        _ => jsonrpc_response(id, mcp_tool_error_result(format!("Unknown tool `{name}`."))),
    }
}

async fn run_cli_mcp_turn(
    args: McpServeArgs,
    request: PsychevoMcpTurnRequest,
) -> Result<PsychevoMcpTurnResult> {
    let env_map = inherited_env();
    let process_cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &process_cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &process_cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &process_cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let cwd = match request.cwd.or_else(|| args.dir.clone()) {
        Some(dir) => resolve_explicit_path(&dir, &env_map, &process_cwd)?,
        None => process_cwd,
    };
    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("prompt must not be empty"));
    }
    if args.permission_mode == Some(PermissionModeArg::BypassPermissions) {
        return Err(anyhow!(
            "use --dangerously-skip-permissions to select bypassPermissions"
        ));
    }
    let mode_arg = if args.dangerously_skip_permissions {
        Some(PermissionModeArg::BypassPermissions)
    } else {
        args.permission_mode
    };
    let run_mode = mode_arg
        .map(PermissionModeArg::run_mode)
        .unwrap_or(RunMode::Default);
    let permission_mode = mode_arg
        .map(PermissionModeArg::permission_mode)
        .filter(|mode| *mode != PermissionMode::Default);
    let project_context_override = if args.isolated {
        Some(ProjectContextInstructionMode::Cwd)
    } else {
        args.project_context.map(|mode| mode.mode())
    };

    let state = StateRuntime::open(&db_path)?;
    let gateway = Gateway::new(state.clone());
    let source = GatewaySource::new("mcp", format!("mcp:{}", std::process::id()))
        .invocation()
        .with_raw_identity(json!({
            "kind": "mcp",
            "entrypoint": "mcp serve",
            "cwd": cwd.display().to_string(),
        }));
    let mut turn = ThreadTurnRequest::new(cwd, vec![GatewayInputPart::Text { text: prompt }]);
    turn.thread_id = request.session_id;
    turn.source = Some(source);
    turn.policy.snapshot_root = Some(home.join("snapshots"));
    turn.policy.extract_prompt_image_sources = true;
    turn.policy.config_path = config_path;
    turn.policy.project_context_override = project_context_override;
    turn.policy.model = args.model;
    turn.policy.reasoning_effort = args.variant.map(|variant| variant.as_str().to_string());
    turn.policy.mode = run_mode;
    turn.policy.permission_mode = permission_mode;
    turn.policy.approval_handler = interactive_approval_handler();
    turn.policy.inherited_env = Some(env_map);
    turn.policy.agent_ref = args.agent;
    turn.policy.no_agents = args.no_agents;
    turn.policy.no_skills = args.no_skills;
    turn.policy.skill_inputs = args.skill;
    turn.runtime_source = Some("mcp".to_string());
    turn.continue_sources = vec!["mcp".to_string()];
    let turn_result = gateway.run_turn(turn).await?;
    let result = turn_result.result;
    Ok(PsychevoMcpTurnResult {
        session_id: result.session_id,
        final_answer: result.final_answer,
        outcome: result.outcome.as_str().to_string(),
        tool_failures: result.tool_failures,
    })
}

fn initialize_result(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .or_else(|| params.get("protocol_version"))
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": true
            }
        },
        "serverInfo": {
            "name": "psychevo-mcp-server",
            "title": "Psychevo",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn psychevo_mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "psychevo",
            "title": "Psychevo",
            "description": "Start or continue a Psychevo turn from prompt text.",
            "inputSchema": psychevo_tool_schema(false),
        }),
        json!({
            "name": "psychevo-reply",
            "title": "Psychevo Reply",
            "description": "Reply to an existing Psychevo session by providing prompt and a session alias.",
            "inputSchema": psychevo_tool_schema(true),
        }),
    ]
}

fn psychevo_tool_schema(reply: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "prompt": {
                "type": "string",
                "description": "Prompt text for the Psychevo turn."
            },
            "sessionId": {
                "type": "string",
                "description": "Psychevo session id to continue."
            },
            "session_id": {
                "type": "string",
                "description": "Alias for sessionId."
            },
            "threadId": {
                "type": "string",
                "description": "Alias for sessionId for clients that use thread terminology."
            },
            "thread_id": {
                "type": "string",
                "description": "Alias for sessionId for clients that use thread terminology."
            },
            "cwd": {
                "type": "string",
                "description": "Optional cwd for this turn."
            }
        },
        "required": ["prompt"],
        "additionalProperties": false
    });
    if reply {
        schema["anyOf"] = json!([
            {"required": ["sessionId"]},
            {"required": ["session_id"]},
            {"required": ["threadId"]},
            {"required": ["thread_id"]}
        ]);
    }
    schema
}

fn mcp_tool_success_result(result: PsychevoMcpTurnResult) -> Value {
    let success = result.outcome == Outcome::Normal.as_str() && result.tool_failures == 0;
    let structured = json!({
        "sessionId": result.session_id,
        "threadId": result.session_id,
        "content": result.final_answer,
        "outcome": result.outcome,
        "toolFailures": result.tool_failures,
    });
    json!({
        "content": [{
            "type": "text",
            "text": structured["content"],
        }],
        "structuredContent": structured,
        "isError": !success,
    })
}

fn mcp_tool_error_result(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [{
            "type": "text",
            "text": message,
        }],
        "structuredContent": {
            "content": message,
        },
        "isError": true,
    })
}

fn string_argument(arguments: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn session_argument(arguments: &serde_json::Map<String, Value>) -> Option<String> {
    string_argument(arguments, "sessionId")
        .or_else(|| string_argument(arguments, "session_id"))
        .or_else(|| string_argument(arguments, "threadId"))
        .or_else(|| string_argument(arguments, "thread_id"))
}

fn jsonrpc_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRunner {
        requests: Mutex<Vec<PsychevoMcpTurnRequest>>,
    }

    impl PsychevoMcpRunner for FakeRunner {
        fn run_turn(
            &self,
            request: PsychevoMcpTurnRequest,
        ) -> BoxFuture<'static, Result<PsychevoMcpTurnResult>> {
            self.requests.lock().expect("requests").push(request);
            Box::pin(async {
                Ok(PsychevoMcpTurnResult {
                    session_id: "session-1".to_string(),
                    final_answer: "done".to_string(),
                    outcome: "normal".to_string(),
                    tool_failures: 0,
                })
            })
        }
    }

    #[tokio::test]
    async fn initialize_and_list_tools_return_minimal_mcp_shapes() {
        let runner = FakeRunner::default();
        let initialized = handle_mcp_jsonrpc(
            &runner,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "2025-06-18"}
            }),
        )
        .await
        .expect("initialize response");

        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "psychevo-mcp-server"
        );
        assert_eq!(
            initialized["result"]["capabilities"]["tools"]["listChanged"],
            true
        );

        let listed = handle_mcp_jsonrpc(
            &runner,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            }),
        )
        .await
        .expect("tools/list response");
        assert_eq!(listed["result"]["tools"][0]["name"], "psychevo");
        assert_eq!(listed["result"]["tools"][1]["name"], "psychevo-reply");
        let reply_schema = &listed["result"]["tools"][1]["inputSchema"];
        assert_eq!(reply_schema["required"], json!(["prompt"]));
        assert_eq!(
            reply_schema["properties"]["thread_id"]["type"],
            json!("string")
        );
        let any_of = reply_schema["anyOf"].as_array().expect("reply anyOf");
        for alias in ["sessionId", "session_id", "threadId", "thread_id"] {
            assert!(
                any_of
                    .iter()
                    .any(|entry| entry["required"] == json!([alias])),
                "reply schema accepts {alias}"
            );
        }
    }

    #[tokio::test]
    async fn tool_call_runs_turn_and_returns_session_content() {
        let runner = FakeRunner::default();
        let response = handle_mcp_jsonrpc(
            &runner,
            json!({
                "jsonrpc": "2.0",
                "id": "call-1",
                "method": "tools/call",
                "params": {
                    "name": "psychevo-reply",
                    "arguments": {
                        "prompt": "continue",
                        "thread_id": "session-0",
                        "cwd": "/tmp"
                    }
                }
            }),
        )
        .await
        .expect("tools/call response");

        assert_eq!(
            response["result"]["structuredContent"]["sessionId"],
            "session-1"
        );
        assert_eq!(response["result"]["structuredContent"]["content"], "done");
        assert_eq!(response["result"]["isError"], false);
        let requests = runner.requests.lock().expect("requests");
        assert_eq!(requests[0].session_id.as_deref(), Some("session-0"));
        assert_eq!(requests[0].prompt, "continue");
    }

    #[tokio::test]
    async fn reply_requires_session_alias() {
        let runner = FakeRunner::default();
        let response = handle_mcp_jsonrpc(
            &runner,
            json!({
                "jsonrpc": "2.0",
                "id": "call-1",
                "method": "tools/call",
                "params": {
                    "name": "psychevo-reply",
                    "arguments": {"prompt": "continue"}
                }
            }),
        )
        .await
        .expect("tools/call response");

        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("error text")
                .contains("sessionId`, `session_id`, `threadId`, or `thread_id")
        );
        assert!(runner.requests.lock().expect("requests").is_empty());
    }
}
