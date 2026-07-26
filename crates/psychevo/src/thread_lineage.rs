use serde_json::Value;

pub const SIDE_CONVERSATION_METADATA_KEY: &str = "side_conversation";
pub const SIDE_INHERITED_METADATA_KEY: &str = "side_inherited";
pub const TUI_SIDE_CONVERSATION_SESSION_SOURCE: &str = "tui-side-conversation";
pub const WEB_SIDE_CONVERSATION_SESSION_SOURCE: &str = "web-side-conversation";
pub const SIDE_CONVERSATION_SESSION_SOURCES: &[&str] = &[
    TUI_SIDE_CONVERSATION_SESSION_SOURCE,
    WEB_SIDE_CONVERSATION_SESSION_SOURCE,
];

pub fn side_inherited_metadata_hidden(metadata: Option<&Value>) -> bool {
    metadata
        .and_then(|metadata| metadata.get(SIDE_INHERITED_METADATA_KEY))
        .and_then(|value| value.get("hidden"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn side_conversation_session_source(source: &str) -> bool {
    SIDE_CONVERSATION_SESSION_SOURCES.contains(&source)
}
