struct LocalPeerSession {
    session_id: String,
    native_session_id: Option<String>,
    created: bool,
}

fn ensure_local_session(
    peer: &ResolvedPeerTurn,
    options: &psychevo_runtime::RunOptions,
) -> psychevo_runtime::Result<LocalPeerSession> {
    let store = options.state.store();
    if let Some(session_id) = &options.session {
        store.resume_session(session_id)?;
        let metadata_native = store
            .session_metadata(session_id)?
            .and_then(|metadata| peer_native_session_id(&metadata, &peer.backend.id));
        let native = match metadata_native {
            Some(native) => Some(native),
            None => store
                .gateway_runtime_binding(session_id)?
                .filter(|binding| {
                    binding.status == psychevo_runtime::GatewayRuntimeBindingStatus::Resolved
                        && binding.native_kind.as_deref()
                            == Some(psychevo_runtime::RuntimeProfileKind::Acp.as_str())
                })
                .and_then(|binding| binding.native_session_id),
        };
        let top_level = store
            .session_summary(session_id)?
            .is_some_and(|summary| summary.parent_session_id.is_none());
        let created = native.is_none() && top_level && store.load_messages(session_id)?.is_empty();
        return Ok(LocalPeerSession {
            session_id: session_id.clone(),
            native_session_id: native,
            // Gateway materializes the public thread before backend dispatch so the immutable
            // Runtime binding can be persisted first. An empty top-level shell is still the first
            // visible ACP turn; managed child titles remain owned by their parent lifecycle.
            created,
        });
    }
    let session_id = store.create_session_with_metadata(
        &options.cwd,
        "peer_agent",
        &peer.agent.name,
        &format!("acp:{}", peer.backend.id),
        Some(peer_root_metadata(peer, None)),
    )?;
    Ok(LocalPeerSession {
        session_id,
        native_session_id: None,
        created: true,
    })
}

fn peer_session_metadata(
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
            serde_json::to_value(session_projection).expect("product-safe ACP projection serializes"),
        );
    }
    value
}

fn peer_root_metadata(peer: &ResolvedPeerTurn, native_session_id: Option<&str>) -> Value {
    json!({
        ACP_PEER_METADATA_KEY: peer_session_metadata(
            peer,
            native_session_id,
            None,
            &BTreeMap::new(),
            None,
        ),
    })
}

fn peer_native_session_id(metadata: &Value, backend_id: &str) -> Option<String> {
    let peer = metadata.get(ACP_PEER_METADATA_KEY)?;
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
        stream(RunStreamEvent::value(value));
    }
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
        .elicitation(ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()))
        .terminal(peer_allows_terminal(peer))
}

fn peer_allows_fs_read(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.read")
        && agent_allows_any_tool(&peer.agent, &["read"])
}

fn peer_allows_fs_write(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("fs.write")
        && agent_allows_any_tool(&peer.agent, &["write", "edit"])
}

fn peer_allows_terminal(peer: &ResolvedPeerTurn) -> bool {
    peer.backend.client_capabilities.contains("terminal")
        && agent_allows_any_tool(&peer.agent, &["exec_command", "write_stdin"])
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

fn acp_request_context(
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

async fn read_text_file(
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
    if !context.fs_read {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.read is not allowed"));
    }
    let path = guarded_existing_path(&context.cwd, path)?;
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(acp_internal_error)?;
    Ok(apply_line_window(text, line, limit))
}

async fn write_text_file(
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
    if !context.fs_write {
        return Err(agent_client_protocol::Error::invalid_request().data("fs.write is not allowed"));
    }
    let decision = if let Some(handler) = &context.approval_handler {
        handler
            .request_permission(PermissionApprovalRequest {
                tool_call_id: format!("acp-write-{}", uuid::Uuid::now_v7()),
                tool_name: "fs/write_text_file".to_string(),
                summary: format!("Write {}", path.display()),
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
    let path = guarded_writable_path(&context.cwd, path)?;
    tokio::fs::write(&path, content)
        .await
        .map_err(acp_internal_error)?;
    Ok(())
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

fn guarded_existing_path(cwd: &Path, path: &Path) -> Result<PathBuf, agent_client_protocol::Error> {
    let path = path
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::invalid_request().data(err.to_string()))?;
    let cwd = cwd
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !path.starts_with(&cwd) {
        return Err(agent_client_protocol::Error::invalid_request()
            .data("path is outside the ACP peer workspace"));
    }
    Ok(path)
}

fn guarded_writable_path(cwd: &Path, path: &Path) -> Result<PathBuf, agent_client_protocol::Error> {
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
    let cwd = cwd
        .canonicalize()
        .map_err(|err| agent_client_protocol::Error::internal_error().data(err.to_string()))?;
    if !parent.starts_with(&cwd) {
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

fn backend_cwd(value: &str, cwd: &Path) -> PathBuf {
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

fn acp_internal_error(err: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(err.to_string())
}
