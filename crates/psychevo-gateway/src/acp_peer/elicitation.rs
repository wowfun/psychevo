const ACP_MAX_ELICITATION_FIELDS: usize = 32;
const ACP_MAX_ELICITATION_OPTIONS: usize = 128;
const ACP_MAX_ELICITATION_TEXT_CHARS: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AcpElicitationChoiceValue {
    Scalar(String),
    Skip,
    EmptyArray,
}

#[derive(Debug, Clone)]
struct AcpElicitationChoice {
    label: String,
    value: AcpElicitationChoiceValue,
}

#[derive(Debug, Clone)]
struct AcpElicitationField {
    name: String,
    required: bool,
    schema: ElicitationPropertySchema,
    choices: Vec<AcpElicitationChoice>,
    custom: bool,
}

async fn create_elicitation(
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    request: CreateElicitationRequest,
) -> Result<CreateElicitationResponse, agent_client_protocol::Error> {
    let session_id = match request.scope() {
        ElicitationScope::Session(scope) => scope.session_id.to_string(),
        ElicitationScope::Request(_) => return Ok(declined_elicitation()),
        _ => return Ok(declined_elicitation()),
    };
    let context = acp_request_context(contexts, &session_id)?;
    let (Some(control), Some(stream)) =
        (context.clarify_control.clone(), context.stream.clone())
    else {
        return Ok(declined_elicitation());
    };
    let ElicitationMode::Form(form) = request.mode else {
        return Ok(declined_elicitation());
    };
    let call_id = format!("acp-elicitation-{}", uuid::Uuid::now_v7());
    let (clarify, fields) = match project_acp_elicitation_form(call_id, &request.message, form) {
        Ok(projected) => projected,
        Err(_) => return Ok(declined_elicitation()),
    };
    let response = control
        .request_clarification(clarify, stream, context.abort.clone())
        .await;
    match response {
        ClarifyInteractionOutcome::Answered(response) => {
            Ok(encode_acp_elicitation_response(&fields, response)
                .unwrap_or_else(|_| declined_elicitation()))
        }
        ClarifyInteractionOutcome::Cancelled
        | ClarifyInteractionOutcome::TimedOut
        | ClarifyInteractionOutcome::TurnFinished => Ok(cancelled_elicitation()),
    }
}

fn project_acp_elicitation_form(
    call_id: String,
    message: &str,
    form: agent_client_protocol::schema::v1::ElicitationFormMode,
) -> std::result::Result<(ClarifyRequestEvent, Vec<AcpElicitationField>), String> {
    bounded_elicitation_text(message, "message")?;
    let schema = form.requested_schema;
    if schema.properties.len() > ACP_MAX_ELICITATION_FIELDS {
        return Err(format!(
            "ACP elicitation has {} fields; maximum is {ACP_MAX_ELICITATION_FIELDS}",
            schema.properties.len()
        ));
    }
    let required = schema
        .required
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if required
        .iter()
        .any(|name| !schema.properties.contains_key(name))
    {
        return Err("ACP elicitation requires an unknown property".to_string());
    }

    if schema.properties.is_empty() {
        let question = ClarifyQuestion {
            header: schema.title.unwrap_or_else(|| "Confirmation".to_string()),
            question: non_empty_elicitation_question(message, schema.description.as_deref(), None),
            options: vec![
                ClarifyQuestionOption {
                    label: "Continue".to_string(),
                    description: "Accept this request without additional form values.".to_string(),
                },
                ClarifyQuestionOption {
                    label: "Decline".to_string(),
                    description: "Decline this request.".to_string(),
                },
            ],
            multiple: false,
            custom: false,
            secret: false,
        };
        return Ok((
            ClarifyRequestEvent {
                call_id,
                questions: vec![question],
            },
            Vec::new(),
        ));
    }

    let mut questions = Vec::with_capacity(schema.properties.len());
    let mut fields = Vec::with_capacity(schema.properties.len());
    for (name, property) in schema.properties {
        bounded_elicitation_text(&name, "property name")?;
        let is_required = required.contains(&name);
        let (title, description, mut options, mut choices, multiple, custom) =
            project_acp_elicitation_property(&name, &property)?;
        if !is_required {
            let skip_label = unique_elicitation_label("Skip this field", &choices);
            options.insert(
                0,
                ClarifyQuestionOption {
                    label: skip_label.clone(),
                    description: "Leave this optional field unset.".to_string(),
                },
            );
            choices.insert(
                0,
                AcpElicitationChoice {
                    label: skip_label,
                    value: AcpElicitationChoiceValue::Skip,
                },
            );
        }
        if options.len() > ACP_MAX_ELICITATION_OPTIONS {
            return Err(format!(
                "ACP elicitation field `{name}` has too many options"
            ));
        }
        let question = non_empty_elicitation_question(message, description.as_deref(), Some(&title));
        bounded_elicitation_text(&question, "projected question")?;
        questions.push(ClarifyQuestion {
            header: name.clone(),
            question,
            options,
            multiple,
            custom,
            secret: false,
        });
        fields.push(AcpElicitationField {
            name,
            required: is_required,
            schema: property,
            choices,
            custom,
        });
    }
    Ok((ClarifyRequestEvent { call_id, questions }, fields))
}

type ProjectedElicitationProperty = (
    String,
    Option<String>,
    Vec<ClarifyQuestionOption>,
    Vec<AcpElicitationChoice>,
    bool,
    bool,
);

fn project_acp_elicitation_property(
    name: &str,
    schema: &ElicitationPropertySchema,
) -> std::result::Result<ProjectedElicitationProperty, String> {
    match schema {
        ElicitationPropertySchema::String(schema) => {
            let mut options = Vec::new();
            let mut choices = Vec::new();
            match (&schema.enum_values, &schema.one_of) {
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "ACP elicitation string `{name}` declares both enum and oneOf"
                    ));
                }
                (Some(values), None) => {
                    for value in values {
                        push_elicitation_choice(
                            &mut options,
                            &mut choices,
                            value,
                            value,
                            "Use this value.",
                        )?;
                    }
                }
                (None, Some(values)) => {
                    for option in values {
                        push_elicitation_choice(
                            &mut options,
                            &mut choices,
                            &option.title,
                            &option.value,
                            option.description.as_deref().unwrap_or("Use this value."),
                        )?;
                    }
                }
                (None, None) => {}
            }
            Ok((
                schema.title.clone().unwrap_or_else(|| name.to_string()),
                schema.description.clone(),
                options,
                choices,
                false,
                schema.enum_values.is_none() && schema.one_of.is_none(),
            ))
        }
        ElicitationPropertySchema::Number(schema) => Ok((
            schema.title.clone().unwrap_or_else(|| name.to_string()),
            schema.description.clone(),
            Vec::new(),
            Vec::new(),
            false,
            true,
        )),
        ElicitationPropertySchema::Integer(schema) => Ok((
            schema.title.clone().unwrap_or_else(|| name.to_string()),
            schema.description.clone(),
            Vec::new(),
            Vec::new(),
            false,
            true,
        )),
        ElicitationPropertySchema::Boolean(schema) => {
            let mut options = Vec::new();
            let mut choices = Vec::new();
            push_elicitation_choice(&mut options, &mut choices, "True", "true", "Enable this value.")?;
            push_elicitation_choice(&mut options, &mut choices, "False", "false", "Disable this value.")?;
            Ok((
                schema.title.clone().unwrap_or_else(|| name.to_string()),
                schema.description.clone(),
                options,
                choices,
                false,
                false,
            ))
        }
        ElicitationPropertySchema::Array(schema) => {
            let mut options = Vec::new();
            let mut choices = Vec::new();
            match &schema.items {
                MultiSelectItems::String(items) => {
                    for value in &items.values {
                        push_elicitation_choice(
                            &mut options,
                            &mut choices,
                            value,
                            value,
                            "Include this value.",
                        )?;
                    }
                }
                MultiSelectItems::Titled(items) => {
                    for option in &items.options {
                        push_elicitation_choice(
                            &mut options,
                            &mut choices,
                            &option.title,
                            &option.value,
                            option.description.as_deref().unwrap_or("Include this value."),
                        )?;
                    }
                }
                MultiSelectItems::Other(_) => {
                    return Err(format!(
                        "ACP elicitation array `{name}` uses an unknown item schema"
                    ));
                }
                _ => {
                    return Err(format!(
                        "ACP elicitation array `{name}` uses an unsupported item schema"
                    ));
                }
            }
            if schema.min_items.unwrap_or(0) == 0 {
                let label = unique_elicitation_label("No selection", &choices);
                options.insert(
                    0,
                    ClarifyQuestionOption {
                        label: label.clone(),
                        description: "Submit an empty selection.".to_string(),
                    },
                );
                choices.insert(
                    0,
                    AcpElicitationChoice {
                        label,
                        value: AcpElicitationChoiceValue::EmptyArray,
                    },
                );
            }
            Ok((
                schema.title.clone().unwrap_or_else(|| name.to_string()),
                schema.description.clone(),
                options,
                choices,
                true,
                false,
            ))
        }
        ElicitationPropertySchema::Other(_) => Err(format!(
            "ACP elicitation property `{name}` uses an unknown schema"
        )),
        _ => Err(format!(
            "ACP elicitation property `{name}` uses an unsupported schema"
        )),
    }
}

fn push_elicitation_choice(
    options: &mut Vec<ClarifyQuestionOption>,
    choices: &mut Vec<AcpElicitationChoice>,
    proposed_label: &str,
    value: &str,
    description: &str,
) -> std::result::Result<(), String> {
    bounded_elicitation_text(value, "option value")?;
    bounded_elicitation_text(description, "option description")?;
    let label = unique_elicitation_label(proposed_label, choices);
    bounded_elicitation_text(&label, "option label")?;
    options.push(ClarifyQuestionOption {
        label: label.clone(),
        description: description.to_string(),
    });
    choices.push(AcpElicitationChoice {
        label,
        value: AcpElicitationChoiceValue::Scalar(value.to_string()),
    });
    Ok(())
}

fn unique_elicitation_label(proposed: &str, choices: &[AcpElicitationChoice]) -> String {
    let proposed = proposed.trim();
    let proposed = if proposed.is_empty() { "Value" } else { proposed };
    if !choices.iter().any(|choice| choice.label == proposed) {
        return proposed.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{proposed} ({suffix})");
        if !choices.iter().any(|choice| choice.label == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn non_empty_elicitation_question(
    message: &str,
    description: Option<&str>,
    title: Option<&str>,
) -> String {
    let question = [Some(message), title, description]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" — ")
        .trim()
        .to_string();
    if question.is_empty() {
        "Provide the requested value.".to_string()
    } else {
        question
    }
}

fn bounded_elicitation_text(value: &str, field: &str) -> std::result::Result<(), String> {
    if value.chars().count() > ACP_MAX_ELICITATION_TEXT_CHARS {
        Err(format!("ACP elicitation {field} is too long"))
    } else {
        Ok(())
    }
}

fn encode_acp_elicitation_response(
    fields: &[AcpElicitationField],
    response: psychevo_runtime::ClarifyResponse,
) -> std::result::Result<CreateElicitationResponse, String> {
    if fields.is_empty() {
        let answer = response
            .answers
            .first()
            .and_then(|answer| answer.answers.first())
            .map(String::as_str)
            .ok_or_else(|| "ACP elicitation confirmation has no answer".to_string())?;
        return Ok(if answer == "Continue" {
            CreateElicitationResponse::new(ElicitationAcceptAction::new().content(
                BTreeMap::<String, ElicitationContentValue>::new(),
            ))
        } else {
            declined_elicitation()
        });
    }
    if response.answers.len() != fields.len() {
        return Err("ACP elicitation answer count does not match the form".to_string());
    }
    let mut content = BTreeMap::new();
    for (field, answer) in fields.iter().zip(response.answers) {
        if let Some(value) = encode_acp_elicitation_field(field, answer)? {
            content.insert(field.name.clone(), value);
        }
    }
    Ok(CreateElicitationResponse::new(
        ElicitationAcceptAction::new().content(content),
    ))
}

fn encode_acp_elicitation_field(
    field: &AcpElicitationField,
    answer: ClarifyAnswer,
) -> std::result::Result<Option<ElicitationContentValue>, String> {
    let mut values = Vec::with_capacity(answer.answers.len());
    let mut skipped = false;
    let mut empty_array = false;
    for answer in answer.answers {
        if let Some(choice) = field
            .choices
            .iter()
            .find(|choice| choice.label == answer)
        {
            match &choice.value {
                AcpElicitationChoiceValue::Scalar(value) => values.push(value.clone()),
                AcpElicitationChoiceValue::Skip => skipped = true,
                AcpElicitationChoiceValue::EmptyArray => empty_array = true,
            }
        } else if field.custom {
            values.push(answer);
        } else {
            return Err(format!(
                "ACP elicitation field `{}` received an unknown option",
                field.name
            ));
        }
    }
    if skipped && values.is_empty() && !empty_array {
        if field.required {
            return Err(format!(
                "ACP elicitation required field `{}` was skipped",
                field.name
            ));
        }
        return Ok(None);
    }
    match &field.schema {
        ElicitationPropertySchema::String(schema) => {
            let value = exactly_one_elicitation_value(&field.name, values)?;
            validate_elicitation_string(&field.name, &value, schema)?;
            Ok(Some(ElicitationContentValue::String(value)))
        }
        ElicitationPropertySchema::Number(schema) => {
            let raw = exactly_one_elicitation_value(&field.name, values)?;
            let value = raw.parse::<f64>().map_err(|_| {
                format!("ACP elicitation field `{}` requires a number", field.name)
            })?;
            if !value.is_finite()
                || schema.minimum.is_some_and(|minimum| value < minimum)
                || schema.maximum.is_some_and(|maximum| value > maximum)
            {
                return Err(format!(
                    "ACP elicitation field `{}` is outside its numeric constraints",
                    field.name
                ));
            }
            Ok(Some(ElicitationContentValue::Number(value)))
        }
        ElicitationPropertySchema::Integer(schema) => {
            let raw = exactly_one_elicitation_value(&field.name, values)?;
            let value = raw.parse::<i64>().map_err(|_| {
                format!("ACP elicitation field `{}` requires an integer", field.name)
            })?;
            if schema.minimum.is_some_and(|minimum| value < minimum)
                || schema.maximum.is_some_and(|maximum| value > maximum)
            {
                return Err(format!(
                    "ACP elicitation field `{}` is outside its integer constraints",
                    field.name
                ));
            }
            Ok(Some(ElicitationContentValue::Integer(value)))
        }
        ElicitationPropertySchema::Boolean(_) => {
            let raw = exactly_one_elicitation_value(&field.name, values)?;
            let value = match raw.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(format!(
                        "ACP elicitation field `{}` requires true or false",
                        field.name
                    ));
                }
            };
            Ok(Some(ElicitationContentValue::Boolean(value)))
        }
        ElicitationPropertySchema::Array(schema) => {
            if empty_array && values.is_empty() {
                values.clear();
            }
            values.sort();
            values.dedup();
            let count = u64::try_from(values.len()).unwrap_or(u64::MAX);
            if schema.min_items.is_some_and(|minimum| count < minimum)
                || schema.max_items.is_some_and(|maximum| count > maximum)
            {
                return Err(format!(
                    "ACP elicitation field `{}` violates selection bounds",
                    field.name
                ));
            }
            Ok(Some(ElicitationContentValue::StringArray(values)))
        }
        ElicitationPropertySchema::Other(_) => Err(format!(
            "ACP elicitation field `{}` uses an unknown schema",
            field.name
        )),
        _ => Err(format!(
            "ACP elicitation field `{}` uses an unsupported schema",
            field.name
        )),
    }
}

fn exactly_one_elicitation_value(
    name: &str,
    values: Vec<String>,
) -> std::result::Result<String, String> {
    let [value]: [String; 1] = values.try_into().map_err(|_| {
        format!("ACP elicitation field `{name}` requires exactly one answer")
    })?;
    Ok(value)
}

fn validate_elicitation_string(
    name: &str,
    value: &str,
    schema: &agent_client_protocol::schema::v1::StringPropertySchema,
) -> std::result::Result<(), String> {
    let length = u32::try_from(value.chars().count()).unwrap_or(u32::MAX);
    if schema.min_length.is_some_and(|minimum| length < minimum)
        || schema.max_length.is_some_and(|maximum| length > maximum)
    {
        return Err(format!(
            "ACP elicitation field `{name}` violates string length bounds"
        ));
    }
    if let Some(pattern) = schema.pattern.as_deref() {
        let pattern = regex_lite::Regex::new(pattern)
            .map_err(|_| format!("ACP elicitation field `{name}` has an invalid pattern"))?;
        if !pattern.is_match(value) {
            return Err(format!(
                "ACP elicitation field `{name}` does not match its pattern"
            ));
        }
    }
    if let Some(format) = schema.format {
        let valid = match format {
            StringFormat::Email => {
                let Some((local, domain)) = value.split_once('@') else {
                    return Err(format!("ACP elicitation field `{name}` requires an email"));
                };
                !local.is_empty() && domain.contains('.') && !domain.ends_with('.')
            }
            StringFormat::Uri => reqwest::Url::parse(value).is_ok(),
            StringFormat::Date => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
            StringFormat::DateTime => chrono::DateTime::parse_from_rfc3339(value).is_ok(),
            _ => false,
        };
        if !valid {
            return Err(format!(
                "ACP elicitation field `{name}` does not match its format"
            ));
        }
    }
    Ok(())
}

fn declined_elicitation() -> CreateElicitationResponse {
    CreateElicitationResponse::new(ElicitationAction::Decline)
}

fn cancelled_elicitation() -> CreateElicitationResponse {
    CreateElicitationResponse::new(ElicitationAction::Cancel)
}
