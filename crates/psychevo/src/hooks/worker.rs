use serde_json::{Value, json};

use super::types::{HookMetadata, HookWorkerAdapter};

pub(crate) async fn call_worker_hook(
    worker: &HookWorkerAdapter,
    metadata: &HookMetadata,
    payload: Value,
) -> std::result::Result<Value, String> {
    let runtime = worker
        .runtime
        .as_ref()
        .ok_or_else(|| "plugin worker runtime unavailable".to_string())?;
    runtime
        .call(
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
        .await
}
