use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceBindingInput<'a> {
    pub source_key: &'a str,
    pub source_kind: &'a str,
    pub raw_identity: Value,
    pub visible_name: Option<&'a str>,
    pub thread_id: &'a str,
    pub backend_kind: &'a str,
    pub backend_native_id: Option<&'a str>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceBindingRecord {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub thread_id: String,
    pub backend_kind: String,
    pub backend_native_id: Option<String>,
    pub draft_agent_ref: Option<String>,
    pub draft_profile_ref: Option<String>,
    pub draft_control_values: BTreeMap<String, String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceLaneInput<'a> {
    pub source_key: &'a str,
    pub source_kind: &'a str,
    pub raw_identity: Value,
    pub visible_name: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub draft_agent_ref: Option<&'a str>,
    pub draft_profile_ref: Option<&'a str>,
    pub draft_control_values: &'a BTreeMap<String, String>,
    pub lineage: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewaySourceLaneRecord {
    pub source_key: String,
    pub source_kind: String,
    pub raw_identity: Value,
    pub visible_name: Option<String>,
    pub thread_id: Option<String>,
    pub draft_agent_ref: Option<String>,
    pub draft_profile_ref: Option<String>,
    pub draft_control_values: BTreeMap<String, String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub lineage: Option<Value>,
}
