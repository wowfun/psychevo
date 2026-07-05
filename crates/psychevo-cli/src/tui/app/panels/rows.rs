#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) fn variant_line(&self) -> String {
        format!("variant: {}", self.variant_display_value())
    }

    pub(crate) fn model_display_value(&self) -> String {
        self.current_model
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .map(|model| format!("{}/{}", model.provider, model.model))
            })
            .unwrap_or_else(|| "config".to_string())
    }

    pub(crate) fn variant_display_value(&self) -> String {
        self.current_variant
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .and_then(|model| model.reasoning_effort.clone())
            })
            .unwrap_or_else(|| "default".to_string())
    }
}

pub(crate) fn model_capability_tags(model: &ConfiguredModel) -> Vec<String> {
    let caps = &model.metadata.capabilities;
    let mut tags = Vec::new();
    match caps.reasoning {
        Some(true) => tags.push("reasoning".to_string()),
        Some(false) => tags.push("no reasoning".to_string()),
        None => {}
    }
    match caps.tool_call {
        Some(true) => tags.push("tools".to_string()),
        Some(false) => tags.push("no tools".to_string()),
        None => {}
    }
    match caps.developer_role {
        Some(true) => tags.push("developer".to_string()),
        Some(false) => tags.push("no developer".to_string()),
        None => {}
    }
    if caps.attachment == Some(true) || caps.input_modalities.iter().any(|value| value != "text") {
        tags.push("multi-modal".to_string());
    }
    if caps.structured_output == Some(true) {
        tags.push("structured".to_string());
    }
    tags
}

pub(crate) fn model_pricing_label(model: &ConfiguredModel) -> Option<String> {
    let cost = model.metadata.cost.as_ref()?;
    let input = cost.input?;
    let output = cost.output?;
    if input == 0.0 && output == 0.0 {
        return Some("free".to_string());
    }
    Some(format!("${input:.3}/${output:.3} /1M"))
}

pub(crate) fn stats_row(
    key: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
    detail: Option<String>,
    group: Option<String>,
) -> BottomSelectionRow {
    let label = label.into();
    let description = description.into();
    BottomSelectionRow {
        label: label.clone(),
        description: Some(description.clone()),
        detail,
        group,
        search_text: format!("{label} {description}"),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::StatsRow(key.into()),
    }
}

pub(crate) fn string_values(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

pub(crate) fn json_array_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|values| string_values(values))
        .unwrap_or_default()
}

pub(crate) fn agent_definition_row(
    agent: psychevo_runtime::AgentDefinition,
    shadowed: bool,
    current_agent: Option<&str>,
) -> BottomSelectionRow {
    let source = agent.source;
    let path = agent.file_path.clone();
    let state = if shadowed { "Shadowed" } else { "Active" };
    let editable = agent_definition_editable(source, path.as_ref());
    let source_label = source.display_label();
    let entrypoints = agent.entrypoints.clone();
    let entrypoint_label = agent_entrypoint_label(&entrypoints);
    let current_main = current_agent.is_some_and(|current| {
        current == agent.name.as_str()
            || agent
                .file_path
                .as_ref()
                .is_some_and(|path| current == path.display().to_string())
    });
    let definition_detail = if editable {
        format!(
            "{state} {source_label} editable  {entrypoint_label}  depth {}",
            agent.max_spawn_depth,
        )
    } else {
        format!(
            "{state} {source_label} read-only  {entrypoint_label}  depth {}",
            agent.max_spawn_depth,
        )
    };
    let detail = if current_main && !shadowed {
        format!("Current main  {definition_detail}")
    } else {
        definition_detail
    };
    BottomSelectionRow {
        label: agent.name.clone(),
        description: Some(agent.description.clone()),
        detail: Some(detail),
        group: Some(if shadowed {
            "Shadowed duplicates".to_string()
        } else {
            "Available definitions".to_string()
        }),
        search_text: format!(
            "{} {} {} {} {} {} {}",
            agent.name,
            agent.description,
            source.as_str(),
            entrypoint_label,
            path.as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            state,
            agent.max_spawn_depth
        ),
        is_current: current_main && !shadowed,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: Some("Enter actions  R run  V view  Esc close".to_string()),
        value: BottomSelectionValue::AgentAvailable {
            name: agent.name,
            source,
            path,
            entrypoints,
            shadowed,
        },
    }
}

pub(crate) fn agent_diagnostic_row(
    diagnostic: psychevo_runtime::AgentDiagnostic,
) -> BottomSelectionRow {
    let path = diagnostic
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    BottomSelectionRow {
        label: "Definition error".to_string(),
        description: Some(diagnostic.message.clone()),
        detail: (!path.is_empty()).then_some(path.clone()),
        group: Some("Diagnostics".to_string()),
        search_text: format!("{} {} {}", diagnostic.kind, diagnostic.message, path),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: Some("Read-only diagnostic  Esc close  Tab running".to_string()),
        value: BottomSelectionValue::AgentDiagnostic(diagnostic.message),
    }
}

pub(crate) fn agent_action_row(
    name: &str,
    source: AgentSource,
    path: Option<PathBuf>,
    shadowed: bool,
    action: AgentAction,
) -> BottomSelectionRow {
    let description = match action {
        AgentAction::UseAsMain => "Use this definition for future turns in the current session",
        AgentAction::Run => "Start a background fresh-context child run",
        AgentAction::View => "Show definition details",
        AgentAction::Update => "Edit the .psychevo Markdown definition",
        AgentAction::Delete => "Delete the .psychevo Markdown definition",
    };
    BottomSelectionRow {
        label: action.label().to_string(),
        description: Some(description.to_string()),
        detail: None,
        group: None,
        search_text: format!("{name} {} {description}", action.label()),
        is_current: false,
        is_default: false,
        style: if matches!(action, AgentAction::UseAsMain | AgentAction::Run) {
            BottomRowStyle::Action
        } else {
            BottomRowStyle::Normal
        },
        footer: Some("Enter select  Esc back".to_string()),
        value: BottomSelectionValue::AgentAction {
            name: name.to_string(),
            source,
            path,
            shadowed,
            action,
        },
    }
}

pub(crate) fn agent_definition_editable(source: AgentSource, path: Option<&PathBuf>) -> bool {
    matches!(source, AgentSource::Project | AgentSource::Global) && path.is_some()
}

pub(crate) fn agent_entrypoint_label(entrypoints: &BTreeSet<AgentEntrypoint>) -> String {
    if entrypoints.is_empty() {
        return "no-entrypoint".to_string();
    }
    entrypoints
        .iter()
        .map(|entrypoint| entrypoint.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn json_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

pub(crate) fn pluralize_count(count: i64, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}
