use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::{
    ClientCapabilities, FileSystemCapabilities, Implementation, InitializeRequest,
    LoadSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind,
    ProtocolVersion, ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{ByteStreams, Client};
use psychevo_runtime::{
    AgentDefinition, AssistantBlock, Error, ImageInput, Message, Outcome,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest, RunResult,
    RunStreamEvent, SelectedAgent, UserContentBlock,
};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{BackendTurnRequest, ResolvedPeerTurn, gateway_now_ms};

const PEER_METADATA_KEY: &str = "peer_agent";

#[derive(Debug)]
pub(crate) struct AcpPeerTurnResult {
    pub(crate) run: RunResult,
    pub(crate) native_session_id: String,
}

#[derive(Clone)]
struct AcpClientContext {
    workdir: PathBuf,
    fs_read: bool,
    fs_write: bool,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
}

pub(crate) async fn run_acp_peer_turn(
    peer: ResolvedPeerTurn,
    request: BackendTurnRequest,
    _turn_id: String,
) -> psychevo_runtime::Result<AcpPeerTurnResult> {
    let options = request.options;
    let state = options.state.clone();
    let store = state.store();
    let (session_id, existing_native_id) = ensure_local_session(&peer, &options)?;
    let is_new_native_session = existing_native_id.is_none();
    let prompt = peer_prompt_text(
        &peer.agent,
        &options.prompt,
        &options.image_inputs,
        is_new_native_session,
    );
    let prompt_for_history = prompt_history_text(&options.prompt, &options.image_inputs);

    emit_runtime_event(
        &request.stream,
        json!({
            "type": "turn_started",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "agent_name": peer.agent.name.clone(),
            "backend_id": peer.backend.id.clone(),
        }),
    );
    store.append_message(
        &session_id,
        &Message::User {
            content: vec![UserContentBlock::text(prompt_for_history.clone())],
            timestamp_ms: gateway_now_ms(),
        },
    )?;

    let acp = run_acp_stdio_turn(
        &peer,
        &options.workdir,
        existing_native_id,
        prompt,
        options.approval_handler.clone(),
    )
    .await;
    let acp = match acp {
        Ok(acp) => acp,
        Err(err) => {
            emit_runtime_event(
                &request.stream,
                json!({
                    "type": "turn_complete",
                    "session_id": session_id.clone(),
                    "source": "peer_agent",
                    "outcome": "failed",
                    "error": err.to_string(),
                }),
            );
            store.append_message(
                &session_id,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: err.to_string(),
                    }],
                    timestamp_ms: gateway_now_ms(),
                    finish_reason: Some("error".to_string()),
                    outcome: Outcome::Failed,
                    model: Some(peer.agent.name.clone()),
                    provider: Some(format!("acp:{}", peer.backend.id)),
                },
            )?;
            return Err(err);
        }
    };

    store.set_session_metadata_field(
        &session_id,
        PEER_METADATA_KEY,
        Some(peer_session_metadata(&peer, Some(&acp.native_session_id))),
    )?;
    if !acp.final_answer.trim().is_empty() {
        store.append_message(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: acp.final_answer.clone(),
                }],
                timestamp_ms: gateway_now_ms(),
                finish_reason: Some("end_turn".to_string()),
                outcome: Outcome::Normal,
                model: Some(peer.agent.name.clone()),
                provider: Some(format!("acp:{}", peer.backend.id)),
            },
        )?;
    }
    emit_runtime_event(
        &request.stream,
        json!({
            "type": "message_end",
            "session_id": session_id.clone(),
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": acp.final_answer.clone()}],
            },
        }),
    );
    emit_runtime_event(
        &request.stream,
        json!({
            "type": "turn_complete",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "outcome": "normal",
        }),
    );

    let run = RunResult {
        session_id: session_id.clone(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: acp.final_answer,
        db_path: state.db_path().to_path_buf(),
        workdir: options.workdir,
        provider: format!("acp:{}", peer.backend.id),
        model: peer.agent.name.clone(),
        base_url: String::new(),
        api_key_env: None,
        reasoning_effort: options.reasoning_effort,
        context_limit: None,
        tool_failures: 0,
        selected_agent: Some(SelectedAgent {
            name: peer.agent.name.clone(),
            source: peer.agent.source.as_str().to_string(),
            path: peer.agent.file_path.clone(),
        }),
        selected_skills: Vec::new(),
        context_snapshot: None,
        events: Vec::new(),
        warnings: Vec::new(),
    };
    Ok(AcpPeerTurnResult {
        run,
        native_session_id: acp.native_session_id,
    })
}

struct AcpTurnOutput {
    native_session_id: String,
    final_answer: String,
}

async fn run_acp_stdio_turn(
    peer: &ResolvedPeerTurn,
    workdir: &Path,
    native_session_id: Option<String>,
    prompt: String,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    let command = peer
        .backend
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            Error::Message(format!(
                "agent backend `{}` is missing command",
                peer.backend.id
            ))
        })?;
    let cwd = backend_cwd(&peer.backend.cwd, workdir);
    let mut child = Command::new(command);
    child
        .args(&peer.backend.args)
        .envs(&peer.backend.env)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().map_err(|err| {
        Error::Message(format!(
            "failed to spawn ACP backend `{}` ({command}): {err}",
            peer.backend.id
        ))
    })?;
    let stdin = child.stdin.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        ))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        ))
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let context = Arc::new(AcpClientContext {
        workdir: workdir.to_path_buf(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler,
    });
    let workdir = workdir.to_path_buf();

    let result = Client
        .builder()
        .name("psychevo-gateway-acp-peer")
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: ReadTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(read_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: WriteTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(write_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(request_permission(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let capabilities = client_capabilities(peer);
            cx.send_request(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(capabilities)
                    .client_info(
                        Implementation::new("psychevo-gateway", env!("CARGO_PKG_VERSION"))
                            .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;

            let mut session = if let Some(native_session_id) = native_session_id {
                let loaded = cx
                    .send_request(LoadSessionRequest::new(native_session_id.clone(), &workdir))
                    .block_task()
                    .await?;
                cx.attach_session(
                    NewSessionResponse::new(native_session_id)
                        .modes(loaded.modes)
                        .meta(loaded.meta),
                    Vec::new(),
                )?
            } else {
                cx.build_session(&workdir)
                    .block_task()
                    .start_session()
                    .await?
            };
            session.send_prompt(prompt)?;
            let final_answer = session.read_to_string().await?;
            Ok(AcpTurnOutput {
                native_session_id: session.session_id().to_string(),
                final_answer,
            })
        })
        .await
        .map_err(|err| Error::Message(format!("ACP peer `{}` failed: {err}", peer.backend.id)));

    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn ensure_local_session(
    peer: &ResolvedPeerTurn,
    options: &psychevo_runtime::RunOptions,
) -> psychevo_runtime::Result<(String, Option<String>)> {
    let store = options.state.store();
    if let Some(session_id) = &options.session {
        store.resume_session(session_id)?;
        let native = store
            .session_metadata(session_id)?
            .and_then(|metadata| peer_native_session_id(&metadata, &peer.backend.id));
        return Ok((session_id.clone(), native));
    }
    let session_id = store.create_session_with_metadata(
        &options.workdir,
        "peer_agent",
        &peer.agent.name,
        &format!("acp:{}", peer.backend.id),
        Some(peer_root_metadata(peer, None)),
    )?;
    Ok((session_id, None))
}

fn peer_session_metadata(peer: &ResolvedPeerTurn, native_session_id: Option<&str>) -> Value {
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
    value
}

fn peer_root_metadata(peer: &ResolvedPeerTurn, native_session_id: Option<&str>) -> Value {
    json!({
        PEER_METADATA_KEY: peer_session_metadata(peer, native_session_id),
    })
}

fn peer_native_session_id(metadata: &Value, backend_id: &str) -> Option<String> {
    let peer = metadata.get(PEER_METADATA_KEY)?;
    let stored_backend = peer.get("backendId").and_then(Value::as_str)?;
    if stored_backend != backend_id {
        return None;
    }
    peer.get("nativeSessionId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn emit_runtime_event(stream: &Option<psychevo_runtime::RunStreamSink>, value: Value) {
    if let Some(stream) = stream {
        stream(RunStreamEvent::Event(value));
    }
}

fn peer_prompt_text(
    agent: &AgentDefinition,
    prompt: &str,
    images: &[ImageInput],
    include_instructions: bool,
) -> String {
    let mut parts = Vec::new();
    if include_instructions && !agent.instructions.trim().is_empty() {
        parts.push(agent.instructions.trim().to_string());
    }
    parts.push(prompt.to_string());
    for image in images {
        match image {
            ImageInput::ImageUrl(url) => parts.push(format!("[image: {url}]")),
            ImageInput::LocalPath(path) => parts.push(format!(
                "[local image omitted for ACP peer: {}]",
                path.display()
            )),
        }
    }
    parts.join("\n\n")
}

fn prompt_history_text(prompt: &str, images: &[ImageInput]) -> String {
    let mut parts = vec![prompt.to_string()];
    for image in images {
        match image {
            ImageInput::ImageUrl(url) => parts.push(format!("[image: {url}]")),
            ImageInput::LocalPath(path) => parts.push(format!("[local image: {}]", path.display())),
        }
    }
    parts.join("\n\n")
}

fn client_capabilities(peer: &ResolvedPeerTurn) -> ClientCapabilities {
    ClientCapabilities::new()
        .fs(FileSystemCapabilities::new()
            .read_text_file(peer_allows_fs_read(peer))
            .write_text_file(peer_allows_fs_write(peer)))
        .terminal(false)
}

fn peer_allows_fs_read(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.read")
        && agent_allows_any_tool(&peer.agent, &["read"])
}

fn peer_allows_fs_write(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.write")
        && agent_allows_any_tool(&peer.agent, &["write", "edit"])
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

async fn read_text_file(
    context: Arc<AcpClientContext>,
    request: ReadTextFileRequest,
) -> Result<ReadTextFileResponse, agent_client_protocol::Error> {
    if !context.fs_read {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.read is not allowed"));
    }
    let path = guarded_existing_path(&context.workdir, &request.path)?;
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(acp_internal_error)?;
    Ok(ReadTextFileResponse::new(apply_line_window(
        text,
        request.line,
        request.limit,
    )))
}

async fn write_text_file(
    context: Arc<AcpClientContext>,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, agent_client_protocol::Error> {
    if !context.fs_write {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.write is not allowed"));
    }
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: format!("acp-write-{}", uuid::Uuid::now_v7()),
                tool_name: "fs/write_text_file".to_string(),
                summary: format!("Write {}", request.path.display()),
                reason: "ACP peer requested a file write".to_string(),
                matched_rule: None,
                suggested_rule: None,
                allow_always: false,
                timeout_secs: handler.timeout_secs(),
            })
            .await
    } else {
        PermissionApprovalDecision::deny()
    };
    if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
        return Err(agent_client_protocol::Error::invalid_request().data("permission denied"));
    }
    let path = guarded_writable_path(&context.workdir, &request.path)?;
    tokio::fs::write(&path, request.content)
        .await
        .map_err(acp_internal_error)?;
    Ok(WriteTextFileResponse::new())
}

async fn request_permission(
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
        PermissionApprovalOutcome::AllowOnce | PermissionApprovalOutcome::AllowSession => {
            PermissionOptionKind::AllowOnce
        }
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

fn guarded_existing_path(
    workdir: &Path,
    path: &Path,
) -> Result<PathBuf, agent_client_protocol::Error> {
    let path = path
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::invalid_request().data(err.to_string()))?;
    let workdir = workdir
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !path.starts_with(&workdir) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("path is outside the ACP peer workspace"));
    }
    Ok(path)
}

fn guarded_writable_path(
    workdir: &Path,
    path: &Path,
) -> Result<PathBuf, agent_client_protocol::Error> {
    if !path.is_absolute() {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("fs/write_text_file path must be absolute"));
    }
    let parent = path.parent().ok_or_else(|| {
        agent_client_protocol::Error::invalid_request()
            .data("fs/write_text_file path has no parent")
    })?;
    let parent = parent
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::invalid_request().data(err.to_string()))?;
    let workdir = workdir
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !parent.starts_with(&workdir) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("path is outside the ACP peer workspace"));
    }
    Ok(path.to_path_buf())
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

fn backend_cwd(value: &str, workdir: &Path) -> PathBuf {
    let value = value.trim();
    if value.is_empty() || value == "invocation" {
        return workdir.to_path_buf();
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        workdir.join(path)
    }
}

fn acp_internal_error(err: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(err.to_string())
}
