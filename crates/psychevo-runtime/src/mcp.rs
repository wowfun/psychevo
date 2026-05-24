use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use futures::future::BoxFuture;
use http::{HeaderName, HeaderValue};
use psychevo_agent_core::{
    ToolBinding, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolOutput,
};
use psychevo_ai::AbortSignal;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{Peer, RoleClient, ServiceExt};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::permissions::PermissionRuntime;
use crate::types::{McpServerInput, McpTransportInput, RunWarning};

pub(crate) async fn mcp_tool_bindings(
    inputs: &[McpServerInput],
    workdir: &Path,
    permission_runtime: Option<&PermissionRuntime>,
) -> (Vec<Arc<dyn ToolBinding>>, Vec<RunWarning>) {
    let mut tools = Vec::<Arc<dyn ToolBinding>>::new();
    let mut warnings = Vec::new();
    let mut seen_servers = BTreeSet::new();
    let mut seen_tools = BTreeSet::new();

    for input in inputs {
        let server_name = normalize_mcp_server_name(&input.name);
        if !seen_servers.insert(server_name.clone()) {
            warnings.push(mcp_warning(format!(
                "MCP server `{}` conflicts with another server after name normalization; omitted",
                input.name
            )));
            continue;
        }

        if let Some(permission_runtime) = permission_runtime
            && let Err(err) = permission_runtime
                .authorize_mcp_startup(&server_name, mcp_transport_kind(&input.transport))
                .await
        {
            warnings.push(mcp_warning(format!(
                "MCP server `{}` startup omitted: {err}",
                input.name
            )));
            continue;
        }

        let service = match connect_mcp_server(input, workdir).await {
            Ok(service) => service,
            Err(err) => {
                warnings.push(mcp_warning(format!(
                    "MCP server `{}` is unavailable: {err}",
                    input.name
                )));
                continue;
            }
        };
        let peer = service.peer().clone();
        let listed = match peer.list_all_tools().await {
            Ok(listed) => listed,
            Err(err) => {
                warnings.push(mcp_warning(format!(
                    "MCP server `{}` did not list tools: {err}",
                    input.name
                )));
                continue;
            }
        };
        let connection = Arc::new(McpConnection {
            peer,
            _service: Mutex::new(service),
        });

        for tool in listed {
            let raw_tool_name = tool.name.to_string();
            let visible_name = mcp_tool_visible_name(&server_name, &raw_tool_name);
            if !seen_tools.insert(visible_name.clone()) {
                warnings.push(mcp_warning(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` conflicts with another model-visible name; omitted"
                )));
                continue;
            }

            let title = tool
                .title
                .clone()
                .or_else(|| tool.annotations.as_ref().and_then(|a| a.title.clone()));
            let description = mcp_tool_description(
                &server_name,
                &raw_tool_name,
                title.as_deref(),
                tool.description.as_deref(),
            );
            tools.push(Arc::new(McpToolBinding {
                visible_name,
                server_name: server_name.clone(),
                raw_tool_name,
                description,
                parameters: Value::Object((*tool.input_schema).clone()),
                connection: Arc::clone(&connection),
            }));
        }
    }

    (tools, warnings)
}

pub(crate) fn mcp_transport_kind(transport: &McpTransportInput) -> &'static str {
    match transport {
        McpTransportInput::Stdio { .. } => "stdio",
        McpTransportInput::StreamableHttp { .. } => "streamable_http",
        McpTransportInput::Unsupported { .. } => "unsupported",
    }
}

pub(crate) fn mcp_tool_name_parts(tool_name: &str) -> Option<(&str, &str)> {
    let rest = tool_name.strip_prefix("mcp__")?;
    rest.split_once("__")
}

pub(crate) fn normalize_mcp_server_name(name: &str) -> String {
    sanitize_mcp_identifier(name, "server")
}

pub(crate) fn mcp_tool_visible_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        sanitize_mcp_identifier(server_name, "server"),
        sanitize_mcp_identifier(tool_name, "tool")
    )
}

pub(crate) fn sanitize_mcp_identifier(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '_'
        };
        if next == '_' {
            if !previous_underscore {
                out.push(next);
            }
            previous_underscore = true;
        } else {
            out.push(next);
            previous_underscore = false;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

pub(crate) async fn connect_mcp_server(
    input: &McpServerInput,
    workdir: &Path,
) -> Result<RunningService<RoleClient, ()>, String> {
    match &input.transport {
        McpTransportInput::Stdio { command, args, env } => {
            let mut cmd = Command::new(command);
            cmd.args(args).envs(env).current_dir(workdir);
            let transport = TokioChildProcess::new(cmd).map_err(|err| err.to_string())?;
            ().serve(transport).await.map_err(|err| err.to_string())
        }
        McpTransportInput::StreamableHttp { url, headers } => {
            let mut parsed_headers = HashMap::new();
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|err| format!("invalid HTTP header `{name}`: {err}"))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|err| format!("invalid HTTP header value for `{name}`: {err}"))?;
                parsed_headers.insert(name, value);
            }
            let config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .custom_headers(parsed_headers);
            let transport = StreamableHttpClientTransport::from_config(config);
            ().serve(transport).await.map_err(|err| err.to_string())
        }
        McpTransportInput::Unsupported { kind } => Err(format!("unsupported transport `{kind}`")),
    }
}

pub(crate) fn mcp_tool_description(
    server_name: &str,
    raw_tool_name: &str,
    title: Option<&str>,
    description: Option<&str>,
) -> String {
    let mut out = format!("MCP tool `{server_name}/{raw_tool_name}`.");
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        out.push(' ');
        out.push_str(title.trim());
        out.push('.');
    }
    if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
        out.push(' ');
        out.push_str(description.trim());
    }
    out
}

pub(crate) fn mcp_warning(message: String) -> RunWarning {
    RunWarning {
        kind: "mcp".to_string(),
        message,
        source_path: None,
        suggestion: None,
    }
}

pub(crate) struct McpConnection {
    pub(crate) peer: Peer<RoleClient>,
    pub(crate) _service: Mutex<RunningService<RoleClient, ()>>,
}

pub(crate) struct McpToolBinding {
    pub(crate) visible_name: String,
    pub(crate) server_name: String,
    pub(crate) raw_tool_name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
    pub(crate) connection: Arc<McpConnection>,
}

impl ToolBinding for McpToolBinding {
    fn name(&self) -> &str {
        &self.visible_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec {
            category: ToolDisplayCategory::Run,
            title_arg_keys: vec!["name".to_string()],
            title_result_keys: vec!["name".to_string()],
            summary_keys: vec![
                "server".to_string(),
                "tool".to_string(),
                "is_error".to_string(),
            ],
            body_keys: vec!["content".to_string(), "structured_content".to_string()],
            body_policy: ToolDisplayBodyPolicy::Body,
        }
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let server_name = self.server_name.clone();
        let raw_tool_name = self.raw_tool_name.clone();
        let peer = self.connection.peer.clone();
        Box::pin(async move {
            let arguments = match args {
                Value::Object(map) => map,
                Value::Null => serde_json::Map::new(),
                other => {
                    return ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` expects object arguments, got {other}"
                    ));
                }
            };
            if abort.aborted() {
                return ToolOutput::error(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` was aborted before dispatch"
                ));
            }
            let request =
                CallToolRequestParams::new(raw_tool_name.clone()).with_arguments(arguments);
            let mut abort = abort;
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error(format!(
                    "MCP tool `{server_name}/{raw_tool_name}` was aborted"
                )),
                result = peer.call_tool(request) => match result {
                    Ok(result) => mcp_tool_output(&server_name, &raw_tool_name, result),
                    Err(err) => ToolOutput::error(format!(
                        "MCP tool `{server_name}/{raw_tool_name}` failed: {err}"
                    )),
                },
            }
        })
    }
}

pub(crate) fn mcp_tool_output(
    server_name: &str,
    raw_tool_name: &str,
    result: CallToolResult,
) -> ToolOutput {
    let is_error = result.is_error.unwrap_or(false);
    let text_content = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    let model_content = if !text_content.trim().is_empty() {
        text_content
    } else if let Some(structured) = &result.structured_content {
        serde_json::to_string(structured).unwrap_or_else(|_| structured.to_string())
    } else {
        serde_json::to_string(&result.content).unwrap_or_else(|_| String::new())
    };
    ToolOutput {
        json: json!({
            "name": format!("{server_name}/{raw_tool_name}"),
            "server": server_name,
            "tool": raw_tool_name,
            "content": result.content,
            "structured_content": result.structured_content,
            "is_error": is_error,
        }),
        model_content: Some(model_content),
        attachments: Vec::new(),
        is_error,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn normalizes_mcp_names_for_model_visible_tools() {
        assert_eq!(normalize_mcp_server_name("docs server"), "docs_server");
        assert_eq!(
            mcp_tool_visible_name("docs server", "search/repo"),
            "mcp__docs_server__search_repo"
        );
        assert_eq!(
            mcp_tool_name_parts("mcp__docs_server__search_repo"),
            Some(("docs_server", "search_repo"))
        );
    }

    #[test]
    fn mcp_output_prefers_text_for_model_content() {
        let mut result = CallToolResult::success(vec![rmcp::model::Content::text("hello")]);
        result.structured_content = Some(json!({"ok": true}));
        let output = mcp_tool_output("server", "tool", result);
        assert_eq!(output.model_content.as_deref(), Some("hello"));
        assert_eq!(output.json["name"], "server/tool");
        assert!(!output.is_error);
    }
}
