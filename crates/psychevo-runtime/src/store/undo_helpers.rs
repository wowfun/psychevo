use psychevo_agent_core::Message;
use rusqlite::{Connection, params};
use serde_json::Value;

use super::store_message_fields::user_content_text;
use super::store_metadata::metadata_object;
use super::{MESSAGE_PRE_SNAPSHOT_KEY, MESSAGE_UNDO_METADATA_KEY, UndoTarget};

pub(crate) fn undo_target_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UndoTarget> {
    let seq = row.get::<_, i64>(0)?;
    let message_json = row.get::<_, String>(1)?;
    let content_text = row.get::<_, Option<String>>(2)?;
    let metadata_json = row.get::<_, Option<String>>(3)?;
    let prompt = content_text
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| user_prompt_from_message_json(&message_json).unwrap_or_default());
    Ok(UndoTarget {
        seq,
        prompt,
        snapshot: undo_snapshot_from_metadata(metadata_json.as_deref()),
    })
}

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

pub(crate) fn session_tool_call_count(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<i64> {
    let mut stmt =
        conn.prepare("SELECT tool_calls_json FROM messages WHERE session_id = ?1 AND tool_calls_json IS NOT NULL")?;
    let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
    let mut count = 0i64;
    for row in rows {
        let value = row?;
        if let Ok(tool_calls) = serde_json::from_str::<Vec<Value>>(&value) {
            count += tool_calls.len() as i64;
        }
    }
    Ok(count)
}
