#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

impl RpcRequest {
    fn params<T>(&self) -> psychevo_runtime::Result<T>
    where
        T: Default + for<'de> Deserialize<'de>,
    {
        match &self.params {
            Some(params) => Ok(serde_json::from_value(params.clone())?),
            None => Ok(T::default()),
        }
    }

    fn required_params<T: for<'de> Deserialize<'de>>(&self) -> psychevo_runtime::Result<T> {
        let params = self
            .params
            .clone()
            .ok_or_else(|| Error::Message(format!("{} requires params", self.method)))?;
        Ok(serde_json::from_value(params)?)
    }
}

fn permission_decision(decision: PermissionDecision) -> PermissionApprovalDecision {
    PermissionApprovalDecision {
        outcome: match decision {
            PermissionDecision::AllowOnce => PermissionApprovalOutcome::AllowOnce,
            PermissionDecision::AllowSession => PermissionApprovalOutcome::AllowSession,
            PermissionDecision::AllowAlways => PermissionApprovalOutcome::AllowAlways,
            PermissionDecision::Deny => PermissionApprovalOutcome::Deny,
        },
    }
}

fn rpc_result(id: Value, result: Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": wire::JSONRPC_VERSION, "id": id, "result": result}))
        .expect("json rpc result serializes")
}

fn rpc_error(id: Value, code: i64, message: String) -> String {
    rpc_error_with_data(id, code, message, None)
}

fn rpc_error_with_data(id: Value, code: i64, message: String, data: Option<Value>) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": wire::JSONRPC_VERSION,
        "id": id,
        "error": {"code": code, "message": message, "data": data}
    }))
    .expect("json rpc error serializes")
}

fn rpc_notification(method: &str, params: Value) -> String {
    serde_json::to_string(
        &json!({"jsonrpc": wire::JSONRPC_VERSION, "method": method, "params": params}),
    )
    .expect("json rpc notification serializes")
}

fn cwd_source(cwd: &Path) -> GatewaySource {
    source_from_input(None, cwd, GatewaySourceLifetime::Persistent)
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn content_type_for_path(path: &Path) -> &'static str {
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

#[allow(dead_code)]
fn _source_key_value(source_key: SourceKey) -> Value {
    json!({"sourceKey": source_key.0})
}
