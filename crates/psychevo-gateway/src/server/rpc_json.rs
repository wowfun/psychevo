use std::path::Path;

use psychevo::Error;
use psychevo::application::PermissionApprovalDecision;
use psychevo_gateway_protocol as wire;
use serde::Deserialize;
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::PermissionDecision;
use psychevo_gateway_protocol::source::{GatewaySource, GatewaySourceLifetime};

use super::auth_input::source_from_input;

#[derive(Debug, Deserialize)]
pub(super) struct RpcRequest {
    pub(super) jsonrpc: String,
    pub(super) id: Option<Value>,
    pub(super) method: String,
    #[serde(default)]
    pub(super) params: Option<Value>,
}

impl RpcRequest {
    pub(super) fn params<T>(&self) -> psychevo::Result<T>
    where
        T: Default + for<'de> Deserialize<'de>,
    {
        match &self.params {
            Some(params) => Ok(serde_json::from_value(params.clone())?),
            None => Ok(T::default()),
        }
    }

    pub(super) fn required_params<T: for<'de> Deserialize<'de>>(&self) -> psychevo::Result<T> {
        let params = self
            .params
            .clone()
            .ok_or_else(|| Error::Message(format!("{} requires params", self.method)))?;
        Ok(serde_json::from_value(params)?)
    }
}

pub(super) fn permission_decision(
    decision: PermissionDecision,
    directory: Option<String>,
) -> PermissionApprovalDecision {
    match decision {
        PermissionDecision::AllowOnce => PermissionApprovalDecision::allow_once(),
        PermissionDecision::AllowTurn => directory
            .map(PermissionApprovalDecision::allow_filesystem_turn)
            .unwrap_or_else(PermissionApprovalDecision::deny),
        PermissionDecision::AllowSession => directory
            .map(PermissionApprovalDecision::allow_filesystem_session)
            .unwrap_or_else(PermissionApprovalDecision::allow_session),
        PermissionDecision::AllowAlways => PermissionApprovalDecision::allow_always(),
        PermissionDecision::Deny => PermissionApprovalDecision::deny(),
    }
}

pub(super) fn rpc_result(id: Value, result: Value) -> String {
    serde_json::to_string(
        &json!({"jsonrpc": wire::source::JSONRPC_VERSION, "id": id, "result": result}),
    )
    .expect("json rpc result serializes")
}

pub(super) fn rpc_error(id: Value, code: i64, message: String) -> String {
    rpc_error_with_data(id, code, message, None)
}

pub(super) fn rpc_error_with_data(
    id: Value,
    code: i64,
    message: String,
    data: Option<Value>,
) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": wire::source::JSONRPC_VERSION,
        "id": id,
        "error": {"code": code, "message": message, "data": data}
    }))
    .expect("json rpc error serializes")
}

pub(super) fn rpc_notification(method: &str, params: Value) -> String {
    serde_json::to_string(
        &json!({"jsonrpc": wire::source::JSONRPC_VERSION, "method": method, "params": params}),
    )
    .expect("json rpc notification serializes")
}

pub(super) fn cwd_source(cwd: &Path) -> GatewaySource {
    source_from_input(None, cwd, GatewaySourceLifetime::Persistent)
}

pub(super) fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}
