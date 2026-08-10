use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    ClientCapabilities, ElicitationCapabilities, ElicitationFormCapabilities,
    FileSystemCapabilities, PermissionOption, PermissionOptionKind, ReadTextFileRequest,
    ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, WriteTextFileRequest,
    WriteTextFileResponse,
};
use psychevo::{
    agents::AgentDefinition,
    application::{
        ImageInput, PermissionApprovalDecision, PermissionApprovalOutcome,
        PermissionApprovalRequest, RunStreamEvent, RunStreamSink,
    },
};
use serde_json::{Value, json};

use crate::gateway::peer_runtime::ResolvedPeerTurn;

use super::session_projection::AcpSessionSnapshot;
use super::turn::AcpClientContext;

pub(crate) fn peer_session_metadata(
    peer: &ResolvedPeerTurn,
    native_session_id: Option<&str>,
    usage_update: Option<&Value>,
    runtime_options: &BTreeMap<String, String>,
    session_projection: Option<&AcpSessionSnapshot>,
) -> Value {
    let mut value = json!({
        "agentName": peer.agent.name.clone(),
        "backendId": peer.backend.id.clone(),
        "backendKind": peer.backend.kind.as_str(),
    });
    if let Some(native_session_id) = native_session_id
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "nativeSessionId".to_string(),
            Value::String(native_session_id.to_string()),
        );
        object.insert(
            "nativeAlias".to_string(),
            Value::String(format!("acp:{}:{native_session_id}", peer.backend.id)),
        );
    }
    if let Some(usage_update) = usage_update
        && let Some(object) = value.as_object_mut()
    {
        object.insert("usageUpdate".to_string(), usage_update.clone());
    }
    if !runtime_options.is_empty()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("runtimeOptions".to_string(), json!(runtime_options));
    }
    if let Some(session_projection) = session_projection
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "sessionProjection".to_string(),
            serde_json::to_value(session_projection)
                .expect("product-safe ACP projection serializes"),
        );
    }
    value
}

pub(super) fn emit_runtime_event(stream: &Option<RunStreamSink>, value: Value) {
    if let Some(stream) = stream {
        stream(RunStreamEvent::value(value));
    }
}

pub(super) fn prompt_history_text(prompt: &str, images: &[ImageInput]) -> String {
    let mut parts = vec![prompt.to_string()];
    for image in images {
        match image {
            ImageInput::ImageUrl(url) => parts.push(format!("[image: {url}]")),
            ImageInput::LocalPath(path) => parts.push(format!("[local image: {}]", path.display())),
        }
    }
    parts.join("\n\n")
}

pub(super) fn client_capabilities(peer: &ResolvedPeerTurn) -> ClientCapabilities {
    ClientCapabilities::new()
        .fs(FileSystemCapabilities::new()
            .read_text_file(peer_allows_fs_read(peer))
            .write_text_file(peer_allows_fs_write(peer)))
        .elicitation(ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()))
        .terminal(peer_allows_terminal(peer))
}

pub(super) fn peer_allows_fs_read(peer: &ResolvedPeerTurn) -> bool {
    platform_callback_enabled(
        peer.backend.client_capabilities.contains("fs.read"),
        cfg!(unix),
    ) && agent_allows_any_tool(&peer.agent, &["read"])
}

pub(super) fn peer_allows_fs_write(peer: &ResolvedPeerTurn) -> bool {
    platform_callback_enabled(
        peer.backend.client_capabilities.contains("fs.write"),
        psychevo::IDENTITY_BOUND_FILE_MUTATIONS_SUPPORTED,
    ) && agent_allows_any_tool(&peer.agent, &["write", "edit"])
}

pub(super) fn peer_allows_terminal(peer: &ResolvedPeerTurn) -> bool {
    platform_callback_enabled(
        peer.backend.client_capabilities.contains("terminal"),
        cfg!(unix),
    ) && agent_allows_any_tool(&peer.agent, &["exec_command", "write_stdin"])
}

fn platform_callback_enabled(configured: bool, identity_backend_supported: bool) -> bool {
    configured && identity_backend_supported
}

fn agent_allows_any_tool(agent: &AgentDefinition, tools: &[&str]) -> bool {
    let allowed = agent
        .tool_policy
        .allowed
        .as_ref()
        .is_none_or(|allowed| tools.iter().any(|tool| allowed.contains(*tool)));
    let denied = tools
        .iter()
        .all(|tool| agent.tool_policy.denied.contains(*tool));
    allowed && !denied
}

pub(super) fn acp_request_context(
    contexts: &Arc<std::sync::Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    session_id: &str,
) -> Result<Arc<AcpClientContext>, agent_client_protocol::Error> {
    contexts
        .lock()
        .map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP session context lock poisoned")
        })?
        .get(session_id)
        .cloned()
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_request()
                .data(format!("unknown ACP session context: {session_id}"))
        })
}

pub(super) async fn read_text_file(
    context: Arc<AcpClientContext>,
    request: ReadTextFileRequest,
) -> Result<ReadTextFileResponse, agent_client_protocol::Error> {
    let content =
        read_text_file_content(context, &request.path, request.line, request.limit).await?;
    Ok(ReadTextFileResponse::new(content))
}

async fn read_text_file_content(
    context: Arc<AcpClientContext>,
    path: &Path,
    line: Option<u32>,
    limit: Option<u32>,
) -> Result<String, agent_client_protocol::Error> {
    context.attachment.ensure_active()?;
    if !context.fs_read {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.read is not allowed"));
    }
    let authorizer = context.filesystem_authorizer.as_ref().ok_or_else(|| {
        agent_client_protocol::Error::invalid_request().data("filesystem authorization unavailable")
    })?;
    let authorized = authorizer
        .authorize_workspace_read(&format!("acp-read-{}", uuid::Uuid::now_v7()), path)
        .await
        .map_err(acp_permission_error)?;
    context.attachment.ensure_active()?;
    let mut file =
        tokio::fs::File::from_std(authorized.into_read_file().map_err(acp_internal_error)?);
    let mut text = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut file, &mut text)
        .await
        .map_err(acp_internal_error)?;
    Ok(apply_line_window(text, line, limit))
}

pub(super) async fn write_text_file(
    context: Arc<AcpClientContext>,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, agent_client_protocol::Error> {
    write_text_file_content(context, &request.path, request.content).await?;
    Ok(WriteTextFileResponse::new())
}

async fn write_text_file_content(
    context: Arc<AcpClientContext>,
    path: &Path,
    content: String,
) -> Result<(), agent_client_protocol::Error> {
    context.attachment.ensure_active()?;
    if !context.fs_write {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.write is not allowed"));
    }
    let authorizer = context.filesystem_authorizer.as_ref().ok_or_else(|| {
        agent_client_protocol::Error::invalid_request().data("filesystem authorization unavailable")
    })?;
    let authorized = authorizer
        .authorize_workspace_write(&format!("acp-write-{}", uuid::Uuid::now_v7()), path)
        .await
        .map_err(acp_permission_error)?;
    context.attachment.ensure_active()?;
    let attachment = context.attachment.clone();
    tokio::task::spawn_blocking(move || {
        with_active_attachment(&attachment, || {
            authorized
                .write_all(content.as_bytes())
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(acp_internal_error)?
    .map_err(acp_internal_error)?;
    Ok(())
}

fn with_active_attachment<T>(
    attachment: &super::turn::AcpAttachmentGuard,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    attachment.ensure_active().map_err(|error| {
        error
            .data
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "ACP session attachment was revoked".to_string())
    })?;
    operation()
}

fn acp_permission_error(reason: String) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_request().data(reason)
}

pub(super) async fn request_permission(
    context: Arc<AcpClientContext>,
    request: RequestPermissionRequest,
) -> Result<RequestPermissionResponse, agent_client_protocol::Error> {
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: request.tool_call.tool_call_id.to_string(),
                tool_name: request
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "ACP tool".to_string()),
                summary: request
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "ACP peer requested permission".to_string()),
                reason: "ACP peer requested permission".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: request
                    .options
                    .iter()
                    .any(|option| option.kind == PermissionOptionKind::AllowAlways),
                filesystem: None,
                mcp_startup: None,
                timeout_secs: handler.timeout_secs(),
            })
            .await
    } else {
        PermissionApprovalDecision::deny()
    };
    let Some(option_id) = permission_option_id(&request.options, decision.outcome) else {
        return Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    };
    Ok(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
    ))
}

fn permission_option_id(
    options: &[PermissionOption],
    outcome: PermissionApprovalOutcome,
) -> Option<String> {
    let preferred = match outcome {
        PermissionApprovalOutcome::AllowAlways => PermissionOptionKind::AllowAlways,
        PermissionApprovalOutcome::AllowOnce
        | PermissionApprovalOutcome::AllowTurn
        | PermissionApprovalOutcome::AllowSession => PermissionOptionKind::AllowOnce,
        PermissionApprovalOutcome::Deny => PermissionOptionKind::RejectOnce,
    };
    options
        .iter()
        .find(|option| option.kind == preferred)
        .or_else(|| {
            options.iter().find(|option| {
                matches!(
                    (outcome, option.kind),
                    (
                        PermissionApprovalOutcome::AllowOnce
                            | PermissionApprovalOutcome::AllowTurn
                            | PermissionApprovalOutcome::AllowSession
                            | PermissionApprovalOutcome::AllowAlways,
                        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                    ) | (
                        PermissionApprovalOutcome::Deny,
                        PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways
                    )
                )
            })
        })
        .map(|option| option.option_id.to_string())
}

fn apply_line_window(text: String, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text;
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let limit = limit.unwrap_or(u32::MAX) as usize;
    text.lines()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn backend_cwd(value: &str, cwd: &Path) -> PathBuf {
    let value = value.trim();
    if value.is_empty() || value == "invocation" {
        return cwd.to_path_buf();
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(super) fn acp_internal_error(err: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{platform_callback_enabled, read_text_file_content};
    use crate::acp_peer::turn::AcpClientContext;

    #[tokio::test]
    async fn filesystem_callbacks_fail_closed_without_runtime_authorization() {
        let temp = tempfile::tempdir().expect("temp");
        let file = temp.path().join("visible.txt");
        std::fs::write(&file, "secret").expect("file");
        let context = Arc::new(AcpClientContext {
            cwd: temp.path().to_path_buf(),
            workspace_roots: vec![temp.path().to_path_buf()],
            fs_read: true,
            fs_write: true,
            approval_handler: None,
            filesystem_authorizer: None,
            turn_control: None,
            terminal: false,
            terminal_env: BTreeMap::new(),
            attachment: Default::default(),
        });

        let error = read_text_file_content(context, &file, None, None)
            .await
            .expect_err("missing authorization must deny");

        assert_eq!(
            error.data,
            Some(serde_json::Value::String(
                "filesystem authorization unavailable".to_string()
            ))
        );
    }

    #[test]
    fn unsupported_platform_callbacks_are_not_advertised() {
        assert!(!platform_callback_enabled(true, false));
        assert!(platform_callback_enabled(true, true));
        assert!(!platform_callback_enabled(false, true));
    }

    #[test]
    fn revoked_attachment_cannot_enter_a_queued_write_operation() {
        let attachment = super::super::turn::AcpAttachmentGuard::default();
        attachment.revoke();
        let mutated = AtomicBool::new(false);

        let error = super::with_active_attachment(&attachment, || {
            mutated.store(true, Ordering::SeqCst);
            Ok(())
        })
        .expect_err("revoked attachment");

        assert!(error.contains("revoked"));
        assert!(!mutated.load(Ordering::SeqCst));
    }
}
