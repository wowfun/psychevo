use super::{AgentSource, PathBuf, Value, json};

pub const SESSION_MAIN_AGENT_METADATA_KEY: &str = "main_agent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedMainAgent {
    Missing,
    Default,
    Agent(String),
}

pub fn main_agent_default_metadata() -> Value {
    json!({"mode": "default"})
}

pub fn main_agent_metadata(
    input: &str,
    name: &str,
    source: AgentSource,
    path: Option<&PathBuf>,
) -> Value {
    json!({
        "mode": "agent",
        "input": input,
        "name": name,
        "source": source.as_str(),
        "path": path,
    })
}

pub fn main_agent_from_session_metadata(metadata: Option<&Value>) -> LoadedMainAgent {
    let Some(metadata) = metadata else {
        return LoadedMainAgent::Missing;
    };
    let Some(main_agent) = metadata.get(SESSION_MAIN_AGENT_METADATA_KEY) else {
        return LoadedMainAgent::Missing;
    };
    if main_agent
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode == "default")
        || main_agent.is_null()
    {
        return LoadedMainAgent::Default;
    }
    if let Some(input) = main_agent
        .get("input")
        .and_then(Value::as_str)
        .or_else(|| main_agent.get("name").and_then(Value::as_str))
        .or_else(|| main_agent.get("path").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return LoadedMainAgent::Agent(input.to_string());
    }
    LoadedMainAgent::Missing
}

pub fn session_agent_input_from_metadata(metadata: &Value) -> Option<String> {
    match main_agent_from_session_metadata(Some(metadata)) {
        LoadedMainAgent::Agent(agent) => Some(agent),
        LoadedMainAgent::Default | LoadedMainAgent::Missing => None,
    }
}

pub fn session_main_agent_explicit_default(metadata: &Value) -> bool {
    matches!(
        main_agent_from_session_metadata(Some(metadata)),
        LoadedMainAgent::Default
    )
}

pub fn session_base_agent_name_from_metadata(metadata: Option<&Value>) -> Option<String> {
    metadata?
        .get("agent")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
