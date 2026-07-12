const ACP_MAX_RUNTIME_OPTIONS: usize = 128;
const ACP_MAX_RUNTIME_OPTION_VALUES: usize = 512;
const ACP_MAX_RUNTIME_OPTION_ID_CHARS: usize = 128;
const ACP_MAX_RUNTIME_OPTION_NAME_CHARS: usize = 256;
const ACP_MAX_RUNTIME_OPTION_DESCRIPTION_CHARS: usize = 1_024;
const ACP_MAX_RUNTIME_OPTION_CATEGORY_CHARS: usize = 128;
const ACP_MAX_RUNTIME_OPTION_TYPE_CHARS: usize = 64;
const ACP_MAX_RUNTIME_OPTION_VALUE_CHARS: usize = 1_024;
const ACP_MAX_RUNTIME_OPTION_GROUP_CHARS: usize = 256;

enum AcpV1Initialization {
    Compatible(Box<InitializeResponse>),
    Incompatible {
        expected: ProtocolVersion,
        actual: ProtocolVersion,
    },
}

async fn initialize_acp_v1(
    cx: &ConnectionTo<Agent>,
    peer: &ResolvedPeerTurn,
    client_name: &str,
) -> Result<AcpV1Initialization, agent_client_protocol::Error> {
    let initialized = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(client_capabilities(peer))
                .client_info(
                    Implementation::new(client_name, env!("CARGO_PKG_VERSION"))
                        .title("Psychevo Gateway"),
                ),
        )
        .block_task()
        .await?;
    if initialized.protocol_version != ProtocolVersion::V1 {
        return Ok(AcpV1Initialization::Incompatible {
            expected: ProtocolVersion::V1,
            actual: initialized.protocol_version,
        });
    }
    Ok(AcpV1Initialization::Compatible(Box::new(initialized)))
}

fn project_acp_runtime_options(value: Value) -> Vec<wire::RuntimeConfigOptionView> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(project_acp_runtime_option)
        .take(ACP_MAX_RUNTIME_OPTIONS)
        .collect()
}

fn project_acp_runtime_option(option: &Value) -> Option<wire::RuntimeConfigOptionView> {
    let id = bounded_string_field(option, "id", ACP_MAX_RUNTIME_OPTION_ID_CHARS)?;
    let name = bounded_string_field(option, "name", ACP_MAX_RUNTIME_OPTION_NAME_CHARS)
        .unwrap_or_else(|| id.clone());
    Some(wire::RuntimeConfigOptionView {
        id,
        name,
        description: bounded_string_field(
            option,
            "description",
            ACP_MAX_RUNTIME_OPTION_DESCRIPTION_CHARS,
        ),
        category: bounded_string_field(option, "category", ACP_MAX_RUNTIME_OPTION_CATEGORY_CHARS),
        option_type: bounded_string_field(option, "type", ACP_MAX_RUNTIME_OPTION_TYPE_CHARS)
            .unwrap_or_else(|| "unknown".to_string()),
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
                    bounded_string_field(group, "name", ACP_MAX_RUNTIME_OPTION_GROUP_CHARS)
                        .or_else(|| {
                            bounded_string_field(group, "group", ACP_MAX_RUNTIME_OPTION_GROUP_CHARS)
                        });
                group
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(move |value| {
                        project_acp_runtime_option_value(value, group_name.clone())
                    })
            })
            .take(ACP_MAX_RUNTIME_OPTION_VALUES)
            .collect();
    }
    values
        .iter()
        .filter_map(|value| project_acp_runtime_option_value(value, None))
        .take(ACP_MAX_RUNTIME_OPTION_VALUES)
        .collect()
}

fn project_acp_runtime_option_value(
    value: &Value,
    group: Option<String>,
) -> Option<wire::RuntimeConfigOptionValueView> {
    let id = bounded_string_field(value, "value", ACP_MAX_RUNTIME_OPTION_VALUE_CHARS)?;
    Some(wire::RuntimeConfigOptionValueView {
        value: id.clone(),
        name: bounded_string_field(value, "name", ACP_MAX_RUNTIME_OPTION_NAME_CHARS).unwrap_or(id),
        description: bounded_string_field(
            value,
            "description",
            ACP_MAX_RUNTIME_OPTION_DESCRIPTION_CHARS,
        ),
        group,
    })
}

fn current_value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(bounded_acp_runtime_option_text(
            value,
            ACP_MAX_RUNTIME_OPTION_VALUE_CHARS,
        )),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        value => bounded_string_field(value, "value", ACP_MAX_RUNTIME_OPTION_VALUE_CHARS),
    }
}

fn bounded_string_field(value: &Value, field: &str, max_chars: usize) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(|value| bounded_acp_runtime_option_text(value, max_chars))
        .filter(|value| !value.is_empty())
}

fn bounded_acp_runtime_option_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
