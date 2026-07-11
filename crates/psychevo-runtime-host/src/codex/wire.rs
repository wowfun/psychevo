use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{RetryClass, RuntimeError, RuntimeErrorStage};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum RequestId {
    Integer(i64),
    String(String),
}

impl RequestId {
    pub(super) fn key(&self) -> String {
        match self {
            Self::Integer(value) => format!("i:{value}"),
            Self::String(value) => format!("s:{value}"),
        }
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::String(value) => formatter.write_str(value),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum IncomingMessage {
    Response {
        id: RequestId,
        result: Value,
    },
    Error {
        id: RequestId,
        error: RpcError,
    },
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

pub(super) fn parse_incoming(line: &str) -> Result<IncomingMessage, RuntimeError> {
    let value: Value = serde_json::from_str(line).map_err(|error| {
        RuntimeError::new(
            "codex_protocol_invalid_json",
            RuntimeErrorStage::Transport,
            RetryClass::Never,
            format!("Codex app-server emitted invalid JSON: {error}"),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        RuntimeError::new(
            "codex_protocol_invalid_message",
            RuntimeErrorStage::Transport,
            RetryClass::Never,
            "Codex app-server emitted a non-object message",
        )
    })?;
    let id = object
        .get("id")
        .cloned()
        .map(serde_json::from_value::<RequestId>)
        .transpose()
        .map_err(|error| {
            RuntimeError::new(
                "codex_protocol_invalid_id",
                RuntimeErrorStage::Transport,
                RetryClass::Never,
                format!("Codex app-server emitted an invalid request id: {error}"),
            )
        })?;
    let method = object.get("method").and_then(Value::as_str);
    match (id, method, object.get("result"), object.get("error")) {
        (Some(id), Some(method), _, _) => Ok(IncomingMessage::Request {
            id,
            method: method.to_string(),
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        (None, Some(method), _, _) => Ok(IncomingMessage::Notification {
            method: method.to_string(),
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        (Some(id), None, Some(result), _) => Ok(IncomingMessage::Response {
            id,
            result: result.clone(),
        }),
        (Some(id), None, _, Some(error)) => {
            let error = serde_json::from_value(error.clone()).map_err(|decode_error| {
                RuntimeError::new(
                    "codex_protocol_invalid_error",
                    RuntimeErrorStage::Transport,
                    RetryClass::Never,
                    format!("Codex app-server emitted an invalid error response: {decode_error}"),
                )
            })?;
            Ok(IncomingMessage::Error { id, error })
        }
        _ => Err(RuntimeError::new(
            "codex_protocol_invalid_message",
            RuntimeErrorStage::Transport,
            RetryClass::Never,
            "Codex app-server emitted an unrecognized message envelope",
        )),
    }
}

pub(super) fn request(id: RequestId, method: &str, params: Value) -> Value {
    let mut object = Map::new();
    object.insert(
        "id".to_string(),
        serde_json::to_value(id).expect("request id"),
    );
    object.insert("method".to_string(), Value::String(method.to_string()));
    if !params.is_null() {
        object.insert("params".to_string(), params);
    }
    Value::Object(object)
}

pub(super) fn notification(method: &str, params: Value) -> Value {
    let mut object = Map::new();
    object.insert("method".to_string(), Value::String(method.to_string()));
    if !params.is_null() {
        object.insert("params".to_string(), params);
    }
    Value::Object(object)
}

pub(super) fn response(id: RequestId, result: Value) -> Value {
    json!({"id": id, "result": result})
}

pub(super) fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(super) fn rpc_error(error: RpcError, method: &str) -> RuntimeError {
    let retry_class = if error.code == -32001 {
        RetryClass::SafeRetry
    } else {
        RetryClass::Never
    };
    let diagnostic_ref = error
        .data
        .as_ref()
        .and_then(|data| data.get("diagnosticRef"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut runtime_error = RuntimeError::new(
        if error.code == -32001 {
            "codex_server_overloaded"
        } else {
            "codex_rpc_error"
        },
        RuntimeErrorStage::Transport,
        retry_class,
        format!("Codex app-server rejected {method}: {}", error.message),
    );
    runtime_error.diagnostic_ref = diagnostic_ref;
    runtime_error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wire_without_jsonrpc_header() {
        assert!(matches!(
            parse_incoming(r#"{"id":1,"result":{"ok":true}}"#).expect("response"),
            IncomingMessage::Response {
                id: RequestId::Integer(1),
                ..
            }
        ));
        assert!(matches!(
            parse_incoming(r#"{"method":"turn/completed","params":{}}"#)
                .expect("notification"),
            IncomingMessage::Notification { method, .. } if method == "turn/completed"
        ));
    }
}
