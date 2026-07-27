use serde_json::{Value, json};

use super::types::{HookMetadata, HookWorkerAdapter};

pub(crate) fn call_worker_hook(
    worker: &HookWorkerAdapter,
    metadata: &HookMetadata,
    payload: Value,
) -> std::result::Result<Value, String> {
    let session = worker
        .session
        .as_ref()
        .ok_or_else(|| "plugin worker session unavailable".to_string())?;
    session.call(
        "hooks/call",
        json!({
            "hook": {
                "key": metadata.key,
                "event": metadata.event,
                "matcher": metadata.matcher,
                "handler_type": metadata.handler_type.as_str(),
            },
            "payload": payload,
        }),
    )
}
