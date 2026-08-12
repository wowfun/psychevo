use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::{GatewayRuntimeBindingInput, GatewaySourceLaneInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayTurnDeliveryInput<'a> {
    pub turn_id: &'a str,
    pub thread_id: &'a str,
    pub runtime_ref: &'a str,
    pub input_json: &'a str,
    pub input_hash: &'a str,
}

pub(crate) struct ExistingFrameworkThreadTurnInput<'a> {
    pub delivery: GatewayTurnDeliveryInput<'a>,
    pub client_turn_id: Option<&'a str>,
    pub runtime_binding: Option<GatewayRuntimeBindingInput<'a>>,
    pub initial_thread_preferences: &'a BTreeMap<String, Value>,
    pub mission: Option<crate::application::AgentMissionRegistration>,
}

pub(crate) struct NewFrameworkThreadTurnInput<'a> {
    pub thread_id: &'a str,
    pub cwd: &'a Path,
    pub workspace_id: Option<&'a str>,
    pub workspace_roots: Option<&'a [String]>,
    pub workspace_revision: Option<i64>,
    pub source: &'a str,
    pub metadata: Option<Value>,
    pub delivery: GatewayTurnDeliveryInput<'a>,
    pub client_turn_id: Option<&'a str>,
    pub source_lane: Option<GatewaySourceLaneInput<'a>>,
    pub runtime_binding: Option<GatewayRuntimeBindingInput<'a>>,
    pub initial_thread_preferences: &'a BTreeMap<String, Value>,
    pub mission: Option<crate::application::AgentMissionRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayTurnDeliveryRecord {
    pub turn_id: String,
    pub thread_id: String,
    pub runtime_ref: String,
    pub status: GatewayTurnDeliveryStatus,
    pub input_json: Option<String>,
    pub input_hash: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub delivery_confirmed_at_ms: Option<i64>,
    pub terminal_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayTurnDeliveryStatus {
    NotDelivered,
    Delivered,
    Unknown,
    Terminal,
}

impl GatewayTurnDeliveryStatus {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "not_delivered" => Some(Self::NotDelivered),
            "delivered" => Some(Self::Delivered),
            "unknown" => Some(Self::Unknown),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayChannelOutboxInput<'a> {
    pub delivery_id: &'a str,
    pub thread_id: &'a str,
    pub turn_id: &'a str,
    pub connection_id: &'a str,
    pub source_key: &'a str,
    pub payload_text: &'a str,
    pub payload_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayChannelOutboxRecord {
    pub delivery_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub connection_id: String,
    pub source_key: String,
    pub status: GatewayChannelOutboxStatus,
    pub payload_text: Option<String>,
    pub payload_hash: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub acknowledged_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayChannelOutboxStatus {
    Pending,
    Acknowledged,
    Failed,
}

impl GatewayChannelOutboxStatus {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "acknowledged" => Some(Self::Acknowledged),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}
