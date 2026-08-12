use std::collections::BTreeMap;
use std::path::Path;

use psychevo_agent_core::Message;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRuntimeBindingStatus {
    Resolved,
    Unresolved,
}

impl GatewayRuntimeBindingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "resolved" => Some(Self::Resolved),
            "unresolved" => Some(Self::Unresolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRuntimeBindingOwnership {
    ReadWrite,
    ReadOnly,
}

impl GatewayRuntimeBindingOwnership {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadWrite => "read_write",
            Self::ReadOnly => "read_only",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "read_write" => Some(Self::ReadWrite),
            "read_only" => Some(Self::ReadOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeBindingInput<'a> {
    pub thread_id: &'a str,
    pub agent_ref: Option<&'a str>,
    pub agent_fingerprint: &'a str,
    pub agent_definition_json: &'a str,
    pub runtime_ref: &'a str,
    pub backend_kind: &'a str,
    pub native_kind: &'a str,
    pub native_session_id: Option<&'a str>,
    pub cwd: &'a str,
    pub profile_fingerprint: &'a str,
    pub profile_revision: &'a str,
    pub profile_config_json: &'a str,
    pub adapter_kind: &'a str,
    pub adapter_revision: &'a str,
    pub ownership: GatewayRuntimeBindingOwnership,
    pub parent_thread_id: Option<&'a str>,
}

pub(crate) struct AgentThreadImportCommitInput<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) parent_thread_id: Option<&'a str>,
    pub(crate) cwd: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) binding: GatewayRuntimeBindingInput<'a>,
    pub(crate) messages: &'a [AgentThreadImportMessageInput<'a>],
    pub(crate) metadata: &'a BTreeMap<String, Value>,
    pub(crate) title: Option<&'a str>,
}

pub(crate) struct AgentThreadImportMessageInput<'a> {
    pub(crate) message: &'a Message,
    pub(crate) usage: &'a Option<Value>,
    pub(crate) metadata: &'a Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentThreadImportCommit {
    Published,
    Existing { thread_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeBindingRecord {
    pub thread_id: String,
    pub status: GatewayRuntimeBindingStatus,
    pub agent_ref: Option<String>,
    pub agent_fingerprint: Option<String>,
    pub agent_definition_json: Option<String>,
    pub runtime_ref: Option<String>,
    pub backend_kind: Option<String>,
    pub native_kind: Option<String>,
    pub native_session_id: Option<String>,
    pub cwd: String,
    pub profile_fingerprint: Option<String>,
    pub profile_revision: Option<String>,
    pub profile_config_json: Option<String>,
    pub adapter_kind: Option<String>,
    pub adapter_revision: Option<String>,
    pub ownership: GatewayRuntimeBindingOwnership,
    pub parent_thread_id: Option<String>,
    pub binding_revision: i64,
    pub thread_preferences: BTreeMap<String, Value>,
    pub runtime_observed: BTreeMap<String, Value>,
    pub control_revision: i64,
    pub unresolved_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayRuntimeControlStatePatch<'a> {
    /// Replaces the complete stored Thread preference map when present.
    pub thread_preferences: Option<&'a BTreeMap<String, Value>>,
    /// Replaces the complete Adapter-observed map when present.
    pub runtime_observed: Option<&'a BTreeMap<String, Value>>,
}
