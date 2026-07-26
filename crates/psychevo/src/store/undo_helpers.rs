use psychevo_agent_core::Message;
use serde_json::Value;

use super::store_message_fields::user_content_text;
use super::store_metadata::metadata_object;
use super::{MESSAGE_PRE_SNAPSHOT_KEY, MESSAGE_UNDO_METADATA_KEY};

pub(crate) fn undo_snapshot_from_metadata(value: Option<&str>) -> Option<String> {
    let metadata = metadata_object(value).ok()?;
    metadata
        .get(MESSAGE_UNDO_METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|undo| undo.get(MESSAGE_PRE_SNAPSHOT_KEY))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn user_prompt_from_message_json(value: &str) -> Option<String> {
    let message = serde_json::from_str::<Message>(value).ok()?;
    let Message::User { content, .. } = message else {
        return None;
    };
    Some(user_content_text(&content))
}
