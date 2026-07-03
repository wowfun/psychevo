pub(crate) async fn read_acp_peer_runtime_options(
    peer: ResolvedPeerTurn,
    cwd: PathBuf,
    native_session_id: Option<String>,
) -> psychevo_runtime::Result<AcpPeerRuntimeOptions> {
    match read_acp_peer_runtime_options_v2(&peer, cwd.clone(), native_session_id.clone()).await
    {
        Ok(result) => Ok(result),
        Err(v2_error) if v2_error.fallback_safe => {
            match read_acp_peer_runtime_options_v1(&peer, cwd, native_session_id).await {
                Ok(result) => Ok(result),
                Err(v1_error) => Err(Error::Message(format!(
                    "ACP peer `{}` runtime options failed: {}; v1 fallback failed: {}",
                    peer.backend.id, v2_error.error, v1_error
                ))),
            }
        }
        Err(v2_error) => Err(v2_error.error),
    }
}

async fn read_acp_peer_runtime_options_v2(
    peer: &ResolvedPeerTurn,
    cwd: PathBuf,
    native_session_id: Option<String>,
) -> Result<AcpPeerRuntimeOptions, AcpProtocolAttemptError> {
    let (mut child, cwd) = acp_backend_attempt_command(peer, &cwd)?;
    let mut child = child.spawn().map_err(|err| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "failed to spawn ACP backend `{}` ({}): {err}",
            peer.backend.id,
            acp_backend_command_text(peer).unwrap_or("<missing>")
        )),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        )),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        )),
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let result = Client
        .v2()
        .name("psychevo-gateway-acp-options")
        .connect_with(transport, async move |cx| {
            cx.send_request(
                acp_v2::InitializeRequest::new(ProtocolVersion::V2)
                    .capabilities(client_capabilities_v2())
                    .client_info(
                        acp_v2::Implementation::new("psychevo-gateway", env!("CARGO_PKG_VERSION"))
                            .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;

            let (native_session_id, config_options) =
                if let Some(native_session_id) = native_session_id {
                    let loaded = cx
                        .send_request(acp_v2::LoadSessionRequest::new(
                            native_session_id.clone(),
                            &cwd,
                        ))
                        .block_task()
                        .await?;
                    (native_session_id, loaded.config_options.unwrap_or_default())
                } else {
                    let created = cx
                        .send_request(acp_v2::NewSessionRequest::new(&cwd))
                        .block_task()
                        .await?;
                    (
                        created.session_id.to_string(),
                        created.config_options.unwrap_or_default(),
                    )
                };
            Ok(AcpPeerRuntimeOptions {
                native_session_id: Some(native_session_id),
                options: project_acp_runtime_options(
                    serde_json::to_value(config_options).unwrap_or(Value::Null),
                ),
            })
        })
        .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result.map_err(|err| AcpProtocolAttemptError {
        fallback_safe: true,
        error: Error::Message(format!("ACP peer `{}` v2 failed: {err}", peer.backend.id)),
    })
}

async fn read_acp_peer_runtime_options_v1(
    peer: &ResolvedPeerTurn,
    cwd: PathBuf,
    native_session_id: Option<String>,
) -> psychevo_runtime::Result<AcpPeerRuntimeOptions> {
    let (mut child, cwd) = acp_backend_command(peer, &cwd)?;
    let mut child = child.spawn().map_err(|err| {
        Error::Message(format!(
            "failed to spawn ACP backend `{}` ({}): {err}",
            peer.backend.id,
            acp_backend_command_text(peer).unwrap_or("<missing>")
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
    let result = Client
        .builder()
        .name("psychevo-gateway-acp-options")
        .connect_with(transport, async move |cx| {
            cx.send_request(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(client_capabilities(peer))
                    .client_info(
                        Implementation::new("psychevo-gateway", env!("CARGO_PKG_VERSION"))
                            .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;

            let (native_session_id, config_options) =
                if let Some(native_session_id) = native_session_id {
                    let loaded = cx
                        .send_request(LoadSessionRequest::new(native_session_id.clone(), &cwd))
                        .block_task()
                        .await?;
                    (native_session_id, loaded.config_options.unwrap_or_default())
                } else {
                    let created = cx
                        .send_request(NewSessionRequest::new(&cwd))
                        .block_task()
                        .await?;
                    (
                        created.session_id.to_string(),
                        created.config_options.unwrap_or_default(),
                    )
                };
            Ok(AcpPeerRuntimeOptions {
                native_session_id: Some(native_session_id),
                options: project_acp_runtime_options(
                    serde_json::to_value(config_options).unwrap_or(Value::Null),
                ),
            })
        })
        .await
        .map_err(|err| Error::Message(format!("ACP peer `{}` v1 failed: {err}", peer.backend.id)));

    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn project_acp_runtime_options(value: Value) -> Vec<wire::RuntimeConfigOptionView> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(project_acp_runtime_option)
        .collect()
}

fn project_acp_runtime_option(option: &Value) -> Option<wire::RuntimeConfigOptionView> {
    let id = string_field(option, "id")?;
    let name = string_field(option, "name").unwrap_or_else(|| id.clone());
    Some(wire::RuntimeConfigOptionView {
        id,
        name,
        description: string_field(option, "description"),
        category: string_field(option, "category"),
        option_type: string_field(option, "type").unwrap_or_else(|| "unknown".to_string()),
        current_value: current_value_string(option.get("currentValue")),
        values: project_acp_runtime_option_values(option),
    })
}

fn project_acp_runtime_option_values(option: &Value) -> Vec<wire::RuntimeConfigOptionValueView> {
    let Some(values) = option.get("options").and_then(Value::as_array) else {
        return Vec::new();
    };
    let grouped = values.iter().any(|value| value.get("options").is_some());
    if grouped {
        return values
            .iter()
            .flat_map(|group| {
                let group_name =
                    string_field(group, "name").or_else(|| string_field(group, "group"));
                group
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(move |value| {
                        project_acp_runtime_option_value(value, group_name.clone())
                    })
            })
            .collect();
    }
    values
        .iter()
        .filter_map(|value| project_acp_runtime_option_value(value, None))
        .collect()
}

fn project_acp_runtime_option_value(
    value: &Value,
    group: Option<String>,
) -> Option<wire::RuntimeConfigOptionValueView> {
    let id = string_field(value, "value")?;
    Some(wire::RuntimeConfigOptionValueView {
        value: id.clone(),
        name: string_field(value, "name").unwrap_or(id),
        description: string_field(value, "description"),
        group,
    })
}

fn current_value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        value => string_field(value, "value"),
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}
