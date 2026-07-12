use super::*;

#[derive(Debug, Clone)]
pub(super) struct AcpPeerConfigSelection {
    config_id: String,
    category: Option<SessionConfigOptionCategory>,
    requested: String,
}

pub(super) fn requested_acp_config_selections(
    turn: &AcpPeerTurnContext,
) -> Vec<AcpPeerConfigSelection> {
    let mut requested = turn.peer_runtime_options.clone();
    if let Some(model) = turn.peer_model.as_ref() {
        requested.insert("model".to_string(), model.clone());
    }
    if let Some(effort) = turn.peer_reasoning_effort.as_ref() {
        requested.insert("effort".to_string(), effort.clone());
    }

    let mut selections = Vec::new();
    for id in ["model", "effort", "mode"] {
        if let Some(value) = requested.remove(id) {
            selections.push(AcpPeerConfigSelection {
                config_id: id.to_string(),
                category: match id {
                    "model" => Some(SessionConfigOptionCategory::Model),
                    "effort" => Some(SessionConfigOptionCategory::ThoughtLevel),
                    "mode" => Some(SessionConfigOptionCategory::Mode),
                    _ => None,
                },
                requested: value,
            });
        }
    }
    selections.extend(
        requested
            .into_iter()
            .map(|(config_id, requested)| AcpPeerConfigSelection {
                config_id,
                category: None,
                requested,
            }),
    );
    selections
}

pub(super) async fn apply_acp_v1_config_options(
    cx: &ConnectionTo<Agent>,
    notification_ingress: &AcpNotificationIngress,
    config_options: &mut Vec<SessionConfigOption>,
    native_session_id: &str,
    local_session_id: &str,
    stream: &Option<RunStreamSink>,
    selections: Vec<AcpPeerConfigSelection>,
) -> psychevo_runtime::Result<()> {
    for selection in selections {
        let option = matching_acp_config_option(config_options, &selection).ok_or_else(|| {
            acp_not_delivered_error(
                "acp_control_invalid",
                format!(
                    "ACP session does not expose required config option `{}`",
                    selection.config_id
                ),
            )
        })?;
        let config_id = option.id.to_string();
        let value = acp_config_option_value(option, &selection.requested)
            .map_err(|error| acp_not_delivered_error("acp_control_invalid", error.to_string()))?;
        let (response, _response_barrier) = acp_response_with_projection_barrier(
            cx.send_request(SetSessionConfigOptionRequest::new(
                native_session_id.to_string(),
                config_id.clone(),
                value,
            )),
            notification_ingress,
        )
        .await
        .map_err(|error| {
            acp_agent_not_delivered_error(
                "acp_control_rejected",
                "session/set_config_option",
                &error,
            )
        })?;
        *config_options = response.config_options;
        emit_runtime_event(
            stream,
            json!({
                "type": "acp_peer_config_option_set",
                "session_id": local_session_id,
                "source": "acp_peer",
                "protocol_version": "1",
                "config_id": config_id,
                "value": selection.requested,
            }),
        );
    }
    Ok(())
}

fn matching_acp_config_option<'a>(
    config_options: &'a [SessionConfigOption],
    selection: &AcpPeerConfigSelection,
) -> Option<&'a SessionConfigOption> {
    config_options
        .iter()
        .find(|option| option.id.to_string() == selection.config_id)
        .or_else(|| {
            let category = selection.category.as_ref()?;
            config_options
                .iter()
                .find(|option| option.category.as_ref() == Some(category))
        })
}

fn acp_config_option_value(
    option: &SessionConfigOption,
    requested: &str,
) -> psychevo_runtime::Result<SessionConfigOptionValue> {
    match &option.kind {
        SessionConfigKind::Select(select) => {
            let found = match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => options
                    .iter()
                    .any(|option| option.value.to_string() == requested),
                SessionConfigSelectOptions::Grouped(groups) => groups
                    .iter()
                    .flat_map(|group| group.options.iter())
                    .any(|option| option.value.to_string() == requested),
                #[allow(unreachable_patterns)]
                _ => false,
            };
            if !found {
                return Err(Error::Message(format!(
                    "ACP config option `{}` does not expose requested value `{requested}`",
                    option.id
                )));
            }
            Ok(SessionConfigOptionValue::value_id(requested.to_string()))
        }
        SessionConfigKind::Boolean(_) => match requested.trim().to_ascii_lowercase().as_str() {
            "true" | "on" | "1" => Ok(SessionConfigOptionValue::boolean(true)),
            "false" | "off" | "0" => Ok(SessionConfigOptionValue::boolean(false)),
            _ => Err(Error::Message(format!(
                "ACP boolean config option `{}` requires true or false, got `{requested}`",
                option.id
            ))),
        },
        #[allow(unreachable_patterns)]
        _ => Err(Error::Message(format!(
            "ACP config option `{}` has an unsupported value type",
            option.id
        ))),
    }
}

pub(super) fn acp_config_option_json_value(
    option: &SessionConfigOption,
    value: Value,
) -> psychevo_runtime::Result<SessionConfigOptionValue> {
    match (&option.kind, value) {
        (SessionConfigKind::Boolean(_), Value::Bool(value)) => {
            Ok(SessionConfigOptionValue::boolean(value))
        }
        (SessionConfigKind::Boolean(_), Value::String(value)) => {
            acp_config_option_value(option, &value)
                .map_err(|error| acp_not_delivered_error("acp_control_invalid", error.to_string()))
        }
        (SessionConfigKind::Select(_), Value::String(value)) => {
            acp_config_option_value(option, &value)
                .map_err(|error| acp_not_delivered_error("acp_control_invalid", error.to_string()))
        }
        _ => Err(acp_not_delivered_error(
            "acp_control_invalid",
            format!(
                "ACP control `{}` received a value with the wrong type",
                option.id
            ),
        )),
    }
}
