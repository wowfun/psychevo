#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn session_metadata_json(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT metadata_json FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get::<_, Option<String>>(0),
    )
}

pub(crate) fn metadata_object(value: Option<&str>) -> Result<Map<String, Value>> {
    let Some(value) = value else {
        return Ok(Map::new());
    };
    let parsed = serde_json::from_str::<Value>(value)?;
    Ok(parsed.as_object().cloned().unwrap_or_default())
}

pub(crate) fn metadata_object_sql(value: Option<&str>) -> rusqlite::Result<Map<String, Value>> {
    let Some(value) = value else {
        return Ok(Map::new());
    };
    let parsed = serde_json::from_str::<Value>(value).map_err(json_to_sql)?;
    Ok(parsed.as_object().cloned().unwrap_or_default())
}

pub(crate) fn metadata_json_sql(metadata: Map<String, Value>) -> rusqlite::Result<Option<String>> {
    (!metadata.is_empty())
        .then(|| serde_json::to_string(&Value::Object(metadata)).map_err(json_to_sql))
        .transpose()
}

pub(crate) fn json_to_sql(err: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}

pub(crate) fn parse_session_revert(value: Option<&str>) -> Result<Option<SessionRevertState>> {
    let metadata = metadata_object(value)?;
    parse_session_revert_from_metadata(&metadata).map_err(Into::into)
}

pub(crate) fn parse_session_revert_sql(
    value: Option<&str>,
) -> rusqlite::Result<Option<SessionRevertState>> {
    let metadata = metadata_object_sql(value)?;
    parse_session_revert_from_metadata(&metadata)
}

pub(crate) fn parse_session_revert_from_metadata(
    metadata: &Map<String, Value>,
) -> rusqlite::Result<Option<SessionRevertState>> {
    let Some(revert) = metadata
        .get(SESSION_REVERT_METADATA_KEY)
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let Some(start_seq) = revert.get("start_seq").and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(original_snapshot) = revert
        .get("original_snapshot")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(SessionRevertState {
        start_seq,
        original_snapshot,
    }))
}
