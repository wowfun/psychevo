#[cfg(feature = "wechat")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(feature = "telegram", feature = "wechat"))]
use serde_json::Value;

#[cfg(any(feature = "telegram", feature = "wechat"))]
pub(super) fn value_id_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(feature = "wechat")]
pub(super) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
