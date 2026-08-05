use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AuthMethod, McpServer, SessionConfigOption, SessionConfigOptionCategory, SessionModeState,
    SessionNotification, SessionUpdate,
};
use agent_client_protocol::{Agent, ConnectionTo, UntypedMessage};
use agent_client_protocol_schema::v1::InitializeResponse;
use futures::channel::mpsc;
use psychevo::Error;
use serde_json::{Value, json};
use sha2::Digest as _;
use tokio::sync::mpsc as tokio_mpsc;

use psychevo_gateway_protocol as wire;

use super::runtime_options::project_acp_runtime_options;
use super::stream_state::AcpPeerStreamState;

const ACP_MAX_AVAILABLE_COMMANDS: usize = 128;
const ACP_MAX_COMMAND_NAME_CHARS: usize = 128;
const ACP_MAX_COMMAND_DESCRIPTION_CHARS: usize = 1_024;
const ACP_MAX_COMMAND_HINT_CHARS: usize = 512;
pub(super) const ACP_MAX_AGENT_NAME_CHARS: usize = 256;
const ACP_MAX_AGENT_TITLE_CHARS: usize = 512;
const ACP_MAX_AGENT_VERSION_CHARS: usize = 128;
const ACP_MAX_AVAILABLE_MODES: usize = 64;
const ACP_MAX_MODE_ID_CHARS: usize = 128;
const ACP_MAX_MODE_NAME_CHARS: usize = 256;
const ACP_MAX_MODE_DESCRIPTION_CHARS: usize = 1_024;
pub(super) const ACP_MAX_SESSION_TITLE_CHARS: usize = 1_024;
pub(super) const ACP_MAX_UPDATED_AT_CHARS: usize = 128;
const ACP_MAX_CURRENCY_CHARS: usize = 16;
const ACP_MAX_AUTH_METHODS: usize = 32;
const ACP_MAX_AUTH_METHOD_ID_CHARS: usize = 128;
const ACP_MAX_AUTH_METHOD_NAME_CHARS: usize = 256;
const ACP_MAX_AUTH_METHOD_DESCRIPTION_CHARS: usize = 1_024;
const ACP_MAX_LEGACY_MODELS: usize = 512;
const ACP_MAX_LEGACY_MODEL_ID_CHARS: usize = 1_024;
const ACP_MAX_LEGACY_MODEL_NAME_CHARS: usize = 256;
const ACP_MAX_LEGACY_MODEL_DESCRIPTION_CHARS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpAgentIdentitySnapshot {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    /// Preserved without semantic normalization (within the product bound) so
    /// a reviewed capability pack can apply its own version range.
    pub(crate) version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpPromptInputCapabilitiesSnapshot {
    pub(crate) text: bool,
    pub(crate) image: bool,
    pub(crate) audio: bool,
    pub(crate) resource: bool,
    pub(crate) resource_link: bool,
    pub(crate) embedded_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpSessionLifecycleCapabilitiesSnapshot {
    pub(crate) load: bool,
    pub(crate) list: bool,
    pub(crate) delete: bool,
    pub(crate) fork: bool,
    pub(crate) resume: bool,
    pub(crate) close: bool,
    pub(crate) additional_directories: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AcpAuthMethodKindSnapshot {
    Agent,
    EnvVar,
    Terminal,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpAuthMethodSnapshot {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) kind: AcpAuthMethodKindSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpNegotiatedCapabilitiesSnapshot {
    pub(crate) prompt_input: AcpPromptInputCapabilitiesSnapshot,
    pub(crate) session: AcpSessionLifecycleCapabilitiesSnapshot,
    pub(crate) auth_logout: bool,
    pub(crate) auth_methods: Vec<AcpAuthMethodSnapshot>,
    pub(crate) providers: bool,
    pub(crate) mcp_http: bool,
    pub(crate) mcp_sse: bool,
    pub(crate) mcp_acp: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpAvailableCommandSnapshot {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpSessionModeSnapshot {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpLegacyModelSnapshot {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpLegacyModelState {
    pub(crate) current_model_id: String,
    pub(crate) available_models: Vec<AcpLegacyModelSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AcpHistoryOwnerSnapshot {
    Agent,
    Process,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpHistorySnapshot {
    pub(crate) owner: AcpHistoryOwnerSnapshot,
    pub(crate) resumable: bool,
    pub(crate) load_supported: bool,
    pub(crate) resume_supported: bool,
    pub(crate) loaded_from_agent: bool,
    pub(crate) replay_complete: bool,
    pub(crate) replay_update_count: u64,
    pub(crate) live_update_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpSessionInfoSnapshot {
    pub(crate) title: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) usage: Option<Value>,
}

/// The single product-safe read interface for a resident outbound ACP session.
/// Raw ACP handles, metadata, and connection objects do not cross this seam.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpSessionSnapshot {
    pub(crate) native_session_id: String,
    pub(crate) agent: Option<AcpAgentIdentitySnapshot>,
    pub(crate) capabilities: AcpNegotiatedCapabilitiesSnapshot,
    pub(crate) options: Vec<wire::thread_command_turn::RuntimeConfigOptionView>,
    pub(crate) available_commands: Vec<AcpAvailableCommandSnapshot>,
    pub(crate) available_modes: Vec<AcpSessionModeSnapshot>,
    pub(crate) current_mode_id: Option<String>,
    #[serde(default)]
    pub(crate) legacy_models: Option<AcpLegacyModelState>,
    pub(crate) history: AcpHistorySnapshot,
    pub(crate) session_info: AcpSessionInfoSnapshot,
    pub(crate) generation: u64,
    pub(crate) session_epoch: u64,
    pub(crate) control_revision: String,
    pub(crate) projection_revision: String,
}

impl AcpSessionSnapshot {
    /// Admission freshness excludes display-only session projection such as
    /// commands, history counters, title, and usage. Controls carry their own
    /// revision, so this token is limited to identity and negotiated facts
    /// that can change input admission or sendability.
    pub(crate) fn admission_revision(&self) -> String {
        let mut revision = sha2::Sha256::new();
        revision.update(self.generation.to_be_bytes());
        revision.update(self.session_epoch.to_be_bytes());
        revision.update(
            serde_json::to_vec(&json!({
                "agent": &self.agent,
                "capabilities": &self.capabilities,
            }))
            .unwrap_or_default(),
        );
        format!("{:x}", revision.finalize())
    }

    /// Keeps the ThreadContext input mapping exhaustive and independent from
    /// ACP wire names. Unknown product input kinds remain unknown instead of
    /// silently becoming unsupported.
    pub(crate) fn supports_input_kind(&self, kind: &str) -> Option<bool> {
        match kind {
            "text" => Some(self.capabilities.prompt_input.text),
            "image" => Some(self.capabilities.prompt_input.image),
            "audio" => Some(self.capabilities.prompt_input.audio),
            "resource" => Some(self.capabilities.prompt_input.resource),
            "resourceLink" => Some(self.capabilities.prompt_input.resource_link),
            "embeddedContext" => Some(self.capabilities.prompt_input.embedded_context),
            _ => None,
        }
    }

    /// Pack matching is deliberately downstream of the common adapter. The
    /// adapter only exposes the bounded initialize identity and version.
    pub(crate) fn agent_pack_identity(&self) -> Option<(&str, &str)> {
        self.agent
            .as_ref()
            .map(|agent| (agent.name.as_str(), agent.version.as_str()))
    }
}

#[derive(Clone)]
pub(super) struct AcpResidentSession {
    pub(super) native_session_id: String,
    pub(super) agent: Option<AcpAgentIdentitySnapshot>,
    pub(super) capabilities: AcpNegotiatedCapabilitiesSnapshot,
    pub(super) config_options: Vec<SessionConfigOption>,
    pub(super) available_commands: Vec<AcpAvailableCommandSnapshot>,
    pub(super) available_modes: Vec<AcpSessionModeSnapshot>,
    pub(super) current_mode_id: Option<String>,
    pub(super) legacy_models: Option<AcpLegacyModelState>,
    pub(super) history: AcpHistorySnapshot,
    pub(super) session_info: AcpSessionInfoSnapshot,
    pub(super) session_epoch: u64,
    pub(super) last_notification_sequence: u64,
    pub(super) unknown_notification_count: u64,
    /// Exact wire declarations captured at session creation. Resume/fork reuse
    /// these values instead of re-resolving mutable MCP configuration.
    pub(super) mcp_servers: Vec<McpServer>,
    pub(super) mcp_declaration_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcpFactOrigin {
    History,
    Live,
}

impl AcpFactOrigin {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum AcpPeerInboundPayload {
    Session(Box<SessionNotification>),
    Unknown { method: String, params: Value },
    Barrier,
}

pub(super) struct AcpResidentSessionInput {
    pub(super) native_session_id: String,
    pub(super) modes: Option<SessionModeState>,
    pub(super) config_options: Vec<SessionConfigOption>,
    pub(super) legacy_models: Option<AcpLegacyModelState>,
    pub(super) session_epoch: u64,
    pub(super) loaded_from_agent: bool,
    pub(super) mcp_servers: Vec<McpServer>,
    pub(super) mcp_declaration_fingerprint: String,
}

#[derive(Debug, Clone)]
pub(super) struct AcpPeerInboundNotification {
    pub(super) sequence: u64,
    pub(super) payload: AcpPeerInboundPayload,
}

impl AcpPeerInboundNotification {
    fn explicit_session_id(&self) -> Option<String> {
        match &self.payload {
            AcpPeerInboundPayload::Session(notification) => {
                Some(notification.session_id.to_string())
            }
            AcpPeerInboundPayload::Unknown { params, .. } => params
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            AcpPeerInboundPayload::Barrier => None,
        }
    }
}

#[derive(Clone)]
pub(super) struct AcpNotificationIngress {
    state: Arc<Mutex<AcpNotificationIngressState>>,
}

struct AcpNotificationIngressState {
    tx: mpsc::UnboundedSender<AcpPeerInboundNotification>,
    next_sequence: u64,
    response_barriers: BTreeMap<String, u64>,
}

pub(super) type AcpResidentSessions = Arc<tokio::sync::Mutex<BTreeMap<String, AcpResidentSession>>>;

#[derive(Clone, Default)]
pub(super) struct AcpNotificationRouter {
    state: Arc<Mutex<AcpNotificationRouterState>>,
}

#[derive(Default)]
struct AcpNotificationRouterState {
    next_subscription_id: u64,
    subscribers: BTreeMap<u64, AcpNotificationSubscriber>,
}

struct AcpNotificationSubscriber {
    native_session_id: Option<String>,
    tx: tokio_mpsc::UnboundedSender<AcpPeerInboundNotification>,
}

pub(super) struct AcpNotificationSubscription {
    id: u64,
    router: AcpNotificationRouter,
    rx: tokio_mpsc::UnboundedReceiver<AcpPeerInboundNotification>,
}

impl AcpNotificationRouter {
    pub(super) fn subscribe(
        &self,
        native_session_id: Option<String>,
    ) -> psychevo::Result<AcpNotificationSubscription> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Message("ACP notification router lock poisoned".to_string()))?;
        let id = state.next_subscription_id;
        state.next_subscription_id =
            state.next_subscription_id.checked_add(1).ok_or_else(|| {
                Error::Message("ACP notification subscription id exhausted".to_string())
            })?;
        state.subscribers.insert(
            id,
            AcpNotificationSubscriber {
                native_session_id,
                tx,
            },
        );
        Ok(AcpNotificationSubscription {
            id,
            router: self.clone(),
            rx,
        })
    }

    /// Publishes one immutable ingress envelope to every active session task.
    /// The return value tells the generation actor whether a task currently
    /// owns reduction for the envelope's explicit native session.
    pub(super) fn publish(&self, envelope: AcpPeerInboundNotification) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let native_session_id = envelope.explicit_session_id();
        let mut owned = false;
        state.subscribers.retain(|_, subscriber| {
            let sent = subscriber.tx.send(envelope.clone()).is_ok();
            owned |= sent
                && native_session_id
                    .as_deref()
                    .is_some_and(|native_session_id| {
                        subscriber.native_session_id.as_deref() == Some(native_session_id)
                    });
            sent
        });
        owned
    }

    pub(super) fn set_native_session_id(
        &self,
        subscription_id: u64,
        native_session_id: Option<String>,
    ) -> psychevo::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Message("ACP notification router lock poisoned".to_string()))?;
        let subscriber = state.subscribers.get_mut(&subscription_id).ok_or_else(|| {
            Error::Message("ACP notification subscription is no longer active".to_string())
        })?;
        subscriber.native_session_id = native_session_id;
        Ok(())
    }

    pub(super) fn unsubscribe(&self, subscription_id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.subscribers.remove(&subscription_id);
        }
    }
}

impl AcpNotificationSubscription {
    pub(super) fn set_native_session_id(
        &self,
        native_session_id: impl Into<String>,
    ) -> psychevo::Result<()> {
        self.router
            .set_native_session_id(self.id, Some(native_session_id.into()))
    }

    pub(super) fn deactivate(&self) -> psychevo::Result<()> {
        self.router.set_native_session_id(self.id, None)
    }

    pub(super) async fn recv(&mut self) -> Option<AcpPeerInboundNotification> {
        self.rx.recv().await
    }

    pub(super) fn try_recv(&mut self) -> Option<AcpPeerInboundNotification> {
        self.rx.try_recv().ok()
    }
}

impl Drop for AcpNotificationSubscription {
    fn drop(&mut self) {
        self.router.unsubscribe(self.id);
    }
}

impl AcpNotificationIngress {
    pub(super) fn channel() -> (Self, mpsc::UnboundedReceiver<AcpPeerInboundNotification>) {
        let (tx, rx) = mpsc::unbounded();
        (
            Self {
                state: Arc::new(Mutex::new(AcpNotificationIngressState {
                    tx,
                    next_sequence: 1,
                    response_barriers: BTreeMap::new(),
                })),
            },
            rx,
        )
    }

    pub(super) fn notification(&self, payload: AcpPeerInboundPayload) -> psychevo::Result<u64> {
        self.send(payload)
    }

    /// Appends a deterministic ordering fence to the same ingress used by ACP
    /// notifications. Callers reduce through the returned sequence instead of
    /// sleeping or observing a momentarily empty channel.
    pub(super) fn barrier(&self) -> psychevo::Result<u64> {
        self.send(AcpPeerInboundPayload::Barrier)
    }

    /// Called by the response dispatch interceptor while the SDK's central
    /// dispatch loop is blocked. The map lets the request waiter retrieve the
    /// exact ingress sequence after the typed response is forwarded.
    pub(super) fn response_barrier(&self, request_id: Value) -> psychevo::Result<u64> {
        let request_id = serde_json::to_string(&request_id)?;
        let mut state = self.state.lock().map_err(|_| {
            Error::Message("ACP notification projection ingress lock poisoned".to_string())
        })?;
        let sequence = Self::send_locked(&mut state, AcpPeerInboundPayload::Barrier)?;
        state.response_barriers.insert(request_id, sequence);
        Ok(sequence)
    }

    fn take_response_barrier(&self, request_id: &Value) -> psychevo::Result<Option<u64>> {
        let request_id = serde_json::to_string(request_id)?;
        let mut state = self.state.lock().map_err(|_| {
            Error::Message("ACP notification projection ingress lock poisoned".to_string())
        })?;
        Ok(state.response_barriers.remove(&request_id))
    }

    fn send(&self, payload: AcpPeerInboundPayload) -> psychevo::Result<u64> {
        // Sequence allocation and queue insertion are one operation. Using an
        // atomic counter alone would allow two callback tasks to enqueue in the
        // opposite order and make the later reducer discard the lower number.
        let mut state = self.state.lock().map_err(|_| {
            Error::Message("ACP notification projection ingress lock poisoned".to_string())
        })?;
        Self::send_locked(&mut state, payload)
    }

    fn send_locked(
        state: &mut AcpNotificationIngressState,
        payload: AcpPeerInboundPayload,
    ) -> psychevo::Result<u64> {
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.checked_add(1).ok_or_else(|| {
            Error::Message("ACP notification projection sequence exhausted".to_string())
        })?;
        state
            .tx
            .unbounded_send(AcpPeerInboundNotification { sequence, payload })
            .map_err(|_| {
                Error::Message("ACP notification projection ingress is closed".to_string())
            })?;
        Ok(sequence)
    }
}

/// Receives an ACP response whose exact ingress fence was inserted by the
/// connection's response dispatch interceptor before forwarding the response
/// to this waiter.
pub(super) async fn acp_response_with_projection_barrier<
    T: agent_client_protocol::JsonRpcResponse,
>(
    request: agent_client_protocol::SentRequest<T>,
    notification_ingress: &AcpNotificationIngress,
) -> Result<(T, u64), agent_client_protocol::Error> {
    let request_id = request.id();
    let response = request.block_task().await;
    let barrier = notification_ingress
        .take_response_barrier(&request_id)
        .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))?;
    let response = response?;
    let barrier = barrier.ok_or_else(|| {
        agent_client_protocol::Error::internal_error()
            .data("ACP response arrived without an ordered projection barrier")
    })?;
    Ok((response, barrier))
}

pub(super) async fn acp_session_response_with_legacy_models<T, P>(
    cx: &ConnectionTo<Agent>,
    method: &str,
    params: P,
    notification_ingress: &AcpNotificationIngress,
) -> Result<(T, Option<AcpLegacyModelState>, u64), agent_client_protocol::Error>
where
    T: serde::de::DeserializeOwned,
    P: serde::Serialize,
{
    let request = UntypedMessage::new(method, params)?;
    let (raw, barrier) =
        acp_response_with_projection_barrier(cx.send_request(request), notification_ingress)
            .await?;
    let legacy_models = project_legacy_model_state(raw.get("models"));
    let response = serde_json::from_value(raw).map_err(|error| {
        agent_client_protocol::Error::internal_error().data(format!(
            "ACP {method} response failed stable schema decoding: {error}"
        ))
    })?;
    Ok((response, legacy_models, barrier))
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct AcpInboundReduction {
    pub(super) barrier: Option<u64>,
    pub(super) active_session_observed: bool,
}

fn acp_agent_identity(initialized: &InitializeResponse) -> Option<AcpAgentIdentitySnapshot> {
    initialized
        .agent_info
        .as_ref()
        .map(|agent| AcpAgentIdentitySnapshot {
            name: bounded_acp_text(&agent.name, ACP_MAX_AGENT_NAME_CHARS),
            title: agent
                .title
                .as_deref()
                .map(|title| bounded_acp_text(title, ACP_MAX_AGENT_TITLE_CHARS)),
            version: bounded_acp_text(&agent.version, ACP_MAX_AGENT_VERSION_CHARS),
        })
}

fn acp_negotiated_capabilities(
    initialized: &InitializeResponse,
) -> AcpNegotiatedCapabilitiesSnapshot {
    let capabilities = &initialized.agent_capabilities;
    let serialized = serde_json::to_value(capabilities).unwrap_or(Value::Null);
    let session = &capabilities.session_capabilities;
    AcpNegotiatedCapabilitiesSnapshot {
        prompt_input: AcpPromptInputCapabilitiesSnapshot {
            // Text and resource links are ACP v1 baseline prompt blocks.
            text: true,
            image: capabilities.prompt_capabilities.image,
            audio: capabilities.prompt_capabilities.audio,
            resource: capabilities.prompt_capabilities.embedded_context,
            resource_link: true,
            embedded_context: capabilities.prompt_capabilities.embedded_context,
        },
        session: AcpSessionLifecycleCapabilitiesSnapshot {
            load: capabilities.load_session,
            list: session.list.is_some(),
            delete: session.delete.is_some(),
            fork: serialized
                .pointer("/sessionCapabilities/fork")
                .is_some_and(|value| !value.is_null()),
            resume: session.resume.is_some(),
            close: session.close.is_some(),
            additional_directories: session.additional_directories.is_some(),
        },
        auth_logout: capabilities.auth.logout.is_some(),
        auth_methods: initialized
            .auth_methods
            .iter()
            .take(ACP_MAX_AUTH_METHODS)
            .map(|method| AcpAuthMethodSnapshot {
                id: bounded_acp_text(&method.id().to_string(), ACP_MAX_AUTH_METHOD_ID_CHARS),
                name: bounded_acp_text(method.name(), ACP_MAX_AUTH_METHOD_NAME_CHARS),
                description: method.description().map(|description| {
                    bounded_acp_text(description, ACP_MAX_AUTH_METHOD_DESCRIPTION_CHARS)
                }),
                kind: match method {
                    AuthMethod::Agent(_) => AcpAuthMethodKindSnapshot::Agent,
                    AuthMethod::EnvVar(_) => AcpAuthMethodKindSnapshot::EnvVar,
                    AuthMethod::Terminal(_) => AcpAuthMethodKindSnapshot::Terminal,
                    _ => AcpAuthMethodKindSnapshot::Unknown,
                },
            })
            .collect(),
        providers: serialized
            .get("providers")
            .is_some_and(|value| !value.is_null()),
        mcp_http: capabilities.mcp_capabilities.http,
        mcp_sse: capabilities.mcp_capabilities.sse,
        mcp_acp: serialized
            .pointer("/mcpCapabilities/acp")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

pub(super) fn new_acp_resident_session(
    initialized: &InitializeResponse,
    input: AcpResidentSessionInput,
) -> AcpResidentSession {
    let capabilities = acp_negotiated_capabilities(initialized);
    let load_supported = capabilities.session.load;
    let resume_supported = capabilities.session.resume;
    let (current_mode_id, available_modes) = project_session_modes(input.modes);
    AcpResidentSession {
        native_session_id: input.native_session_id,
        agent: acp_agent_identity(initialized),
        config_options: input.config_options,
        available_commands: Vec::new(),
        available_modes,
        current_mode_id,
        legacy_models: input.legacy_models,
        history: AcpHistorySnapshot {
            owner: if load_supported || resume_supported {
                AcpHistoryOwnerSnapshot::Agent
            } else {
                AcpHistoryOwnerSnapshot::Process
            },
            resumable: load_supported || resume_supported,
            load_supported,
            resume_supported,
            loaded_from_agent: input.loaded_from_agent,
            replay_complete: !input.loaded_from_agent,
            replay_update_count: 0,
            live_update_count: 0,
        },
        session_info: AcpSessionInfoSnapshot::default(),
        capabilities,
        session_epoch: input.session_epoch,
        last_notification_sequence: 0,
        unknown_notification_count: 0,
        mcp_servers: input.mcp_servers,
        mcp_declaration_fingerprint: input.mcp_declaration_fingerprint,
    }
}

pub(super) fn next_acp_session_epoch(next_session_epoch: &AtomicU64) -> psychevo::Result<u64> {
    next_session_epoch
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |epoch| {
            epoch.checked_add(1)
        })
        .map_err(|_| Error::Message("ACP session epoch exhausted".to_string()))
}

pub(super) fn bounded_acp_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(super) fn project_legacy_model_state(value: Option<&Value>) -> Option<AcpLegacyModelState> {
    let value = value?.as_object()?;
    let current_model_id = value
        .get("currentModelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| bounded_acp_text(id, ACP_MAX_LEGACY_MODEL_ID_CHARS))?;
    let mut seen = BTreeSet::new();
    let available_models = value
        .get("availableModels")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|model| {
            let id = model
                .get("modelId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| bounded_acp_text(id, ACP_MAX_LEGACY_MODEL_ID_CHARS))?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| bounded_acp_text(name, ACP_MAX_LEGACY_MODEL_NAME_CHARS))?;
            Some(AcpLegacyModelSnapshot {
                id,
                name,
                description: model
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|description| !description.is_empty())
                    .map(|description| {
                        bounded_acp_text(description, ACP_MAX_LEGACY_MODEL_DESCRIPTION_CHARS)
                    }),
            })
        })
        .take(ACP_MAX_LEGACY_MODELS)
        .collect::<Vec<_>>();
    if available_models.is_empty()
        || !available_models
            .iter()
            .any(|model| model.id == current_model_id)
    {
        return None;
    }
    Some(AcpLegacyModelState {
        current_model_id,
        available_models,
    })
}

fn has_stable_model_config_option(config_options: &[SessionConfigOption]) -> bool {
    config_options.iter().any(|option| {
        option.id.to_string() == "model"
            || option.category.as_ref() == Some(&SessionConfigOptionCategory::Model)
    })
}

pub(super) fn effective_legacy_models<'a>(
    config_options: &[SessionConfigOption],
    legacy_models: Option<&'a AcpLegacyModelState>,
) -> Option<&'a AcpLegacyModelState> {
    (!has_stable_model_config_option(config_options))
        .then_some(legacy_models)
        .flatten()
}

fn legacy_model_runtime_option(
    legacy_models: &AcpLegacyModelState,
) -> wire::thread_command_turn::RuntimeConfigOptionView {
    wire::thread_command_turn::RuntimeConfigOptionView {
        id: "model".to_string(),
        name: "Model".to_string(),
        description: Some("Reported by the ACP Agent's legacy model selector.".to_string()),
        category: Some("model".to_string()),
        option_type: "select".to_string(),
        current_value: Some(legacy_models.current_model_id.clone()),
        values: legacy_models
            .available_models
            .iter()
            .map(
                |model| wire::thread_command_turn::RuntimeConfigOptionValueView {
                    value: model.id.clone(),
                    name: model.name.clone(),
                    description: model.description.clone(),
                    group: None,
                },
            )
            .collect(),
    }
}

fn project_session_modes(
    modes: Option<SessionModeState>,
) -> (Option<String>, Vec<AcpSessionModeSnapshot>) {
    let Some(modes) = modes else {
        return (None, Vec::new());
    };
    let current_mode_id = Some(bounded_acp_text(
        &modes.current_mode_id.to_string(),
        ACP_MAX_MODE_ID_CHARS,
    ));
    let available_modes = modes
        .available_modes
        .into_iter()
        .take(ACP_MAX_AVAILABLE_MODES)
        .map(|mode| AcpSessionModeSnapshot {
            id: bounded_acp_text(&mode.id.to_string(), ACP_MAX_MODE_ID_CHARS),
            name: bounded_acp_text(&mode.name, ACP_MAX_MODE_NAME_CHARS),
            description: mode
                .description
                .as_deref()
                .map(|description| bounded_acp_text(description, ACP_MAX_MODE_DESCRIPTION_CHARS)),
        })
        .collect();
    (current_mode_id, available_modes)
}

fn project_available_commands(
    update: &agent_client_protocol::schema::v1::AvailableCommandsUpdate,
) -> Vec<AcpAvailableCommandSnapshot> {
    update
        .available_commands
        .iter()
        .take(ACP_MAX_AVAILABLE_COMMANDS)
        .map(|command| AcpAvailableCommandSnapshot {
            name: bounded_acp_text(&command.name, ACP_MAX_COMMAND_NAME_CHARS),
            description: bounded_acp_text(&command.description, ACP_MAX_COMMAND_DESCRIPTION_CHARS),
            input: command.input.as_ref().and_then(project_command_input),
        })
        .collect()
}

fn project_command_input(
    input: &agent_client_protocol::schema::v1::AvailableCommandInput,
) -> Option<Value> {
    match input {
        agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(input) => {
            Some(json!({
                "hint": bounded_acp_text(&input.hint, ACP_MAX_COMMAND_HINT_CHARS),
            }))
        }
        _ => None,
    }
}

impl AcpResidentSession {
    pub(super) fn reduce_notification(
        &mut self,
        envelope: &AcpPeerInboundNotification,
        origin: AcpFactOrigin,
    ) -> bool {
        // Ingress sequence is the ordering identity within one process
        // generation. Session epoch scopes it to this resident attachment.
        if envelope.sequence <= self.last_notification_sequence {
            return false;
        }
        self.last_notification_sequence = envelope.sequence;
        match origin {
            AcpFactOrigin::History => {
                self.history.replay_update_count =
                    self.history.replay_update_count.saturating_add(1);
            }
            AcpFactOrigin::Live => {
                self.history.live_update_count = self.history.live_update_count.saturating_add(1);
            }
        }

        match &envelope.payload {
            AcpPeerInboundPayload::Session(notification) => match &notification.update {
                SessionUpdate::AvailableCommandsUpdate(update) => {
                    self.available_commands = project_available_commands(update);
                }
                SessionUpdate::CurrentModeUpdate(update) => {
                    self.current_mode_id = Some(bounded_acp_text(
                        &update.current_mode_id.to_string(),
                        ACP_MAX_MODE_ID_CHARS,
                    ));
                }
                SessionUpdate::ConfigOptionUpdate(update) => {
                    self.config_options = update.config_options.clone();
                }
                SessionUpdate::SessionInfoUpdate(update) => {
                    let value = serde_json::to_value(update).unwrap_or(Value::Null);
                    if let Some(title) = value.get("title") {
                        self.session_info.title = title
                            .as_str()
                            .map(str::trim)
                            .filter(|title| !title.is_empty())
                            .map(|title| bounded_acp_text(title, ACP_MAX_SESSION_TITLE_CHARS));
                    }
                    if let Some(updated_at) = value.get("updatedAt") {
                        self.session_info.updated_at = updated_at
                            .as_str()
                            .map(str::trim)
                            .filter(|updated_at| !updated_at.is_empty())
                            .map(|updated_at| {
                                bounded_acp_text(updated_at, ACP_MAX_UPDATED_AT_CHARS)
                            });
                    }
                }
                SessionUpdate::UsageUpdate(update) => {
                    self.session_info.usage = Some(json!({
                        "used": update.used,
                        "size": update.size,
                        "cost": update.cost.as_ref().map(|cost| json!({
                            "amount": cost.amount,
                            "currency": bounded_acp_text(&cost.currency, ACP_MAX_CURRENCY_CHARS),
                        })),
                    }));
                }
                _ => {}
            },
            AcpPeerInboundPayload::Unknown { .. } => {
                self.unknown_notification_count = self.unknown_notification_count.saturating_add(1);
            }
            AcpPeerInboundPayload::Barrier => return false,
        }
        true
    }
}

pub(super) fn reduce_acp_inbound_notification(
    sessions: &mut BTreeMap<String, AcpResidentSession>,
    generation: u64,
    envelope: AcpPeerInboundNotification,
    replay_native_session_id: Option<&str>,
    active_native_session_id: Option<&str>,
    active_state: Option<&mut AcpPeerStreamState>,
) -> AcpInboundReduction {
    if matches!(&envelope.payload, AcpPeerInboundPayload::Barrier) {
        return AcpInboundReduction {
            barrier: Some(envelope.sequence),
            active_session_observed: false,
        };
    }

    let explicit_session_id = envelope.explicit_session_id();
    let native_session_id = explicit_session_id.as_deref().or(active_native_session_id);
    let Some(native_session_id) = native_session_id else {
        return AcpInboundReduction::default();
    };
    let active_session_observed = active_native_session_id == Some(native_session_id);
    let origin = if replay_native_session_id == Some(native_session_id) {
        AcpFactOrigin::History
    } else {
        AcpFactOrigin::Live
    };
    let Some(session) = sessions
        .values_mut()
        .find(|session| session.native_session_id == native_session_id)
    else {
        return AcpInboundReduction {
            barrier: None,
            active_session_observed,
        };
    };
    let session_epoch = session.session_epoch;
    let applied = session.reduce_notification(&envelope, origin);
    if applied
        && active_session_observed
        && let Some(state) = active_state
    {
        state.reduce_notification(envelope, origin, generation, session_epoch);
    }
    AcpInboundReduction {
        barrier: None,
        active_session_observed,
    }
}

pub(super) struct AcpBarrierProjection<'a> {
    pub(super) sessions: &'a AcpResidentSessions,
    pub(super) generation: u64,
    pub(super) barrier_sequence: u64,
    pub(super) replay_native_session_id: Option<&'a str>,
    pub(super) active_native_session_id: Option<&'a str>,
    pub(super) active_state: Option<&'a mut AcpPeerStreamState>,
}

pub(super) async fn reduce_acp_notifications_through_barrier(
    notification_rx: &mut AcpNotificationSubscription,
    projection: AcpBarrierProjection<'_>,
) -> psychevo::Result<bool> {
    let AcpBarrierProjection {
        sessions,
        generation,
        barrier_sequence,
        replay_native_session_id,
        active_native_session_id,
        mut active_state,
    } = projection;
    let mut active_session_observed = false;
    loop {
        let envelope = notification_rx.recv().await.ok_or_else(|| {
            Error::Message(
                "ACP notification projection ingress closed before its barrier".to_string(),
            )
        })?;
        if !acp_notification_is_for_session_or_barrier(&envelope, active_native_session_id) {
            continue;
        }
        let reduction = {
            let mut sessions = sessions.lock().await;
            reduce_acp_inbound_notification(
                &mut sessions,
                generation,
                envelope,
                replay_native_session_id,
                active_native_session_id,
                active_state.as_deref_mut(),
            )
        };
        active_session_observed |= reduction.active_session_observed;
        if reduction.barrier == Some(barrier_sequence) {
            return Ok(active_session_observed);
        }
    }
}

pub(super) fn acp_notification_is_for_session_or_barrier(
    envelope: &AcpPeerInboundNotification,
    active_native_session_id: Option<&str>,
) -> bool {
    if matches!(&envelope.payload, AcpPeerInboundPayload::Barrier) {
        return true;
    }
    envelope
        .explicit_session_id()
        .as_deref()
        .zip(active_native_session_id)
        .is_some_and(|(notification_session_id, active_session_id)| {
            notification_session_id == active_session_id
        })
}

pub(super) async fn reduce_idle_acp_notification(
    sessions: &AcpResidentSessions,
    generation: u64,
    envelope: AcpPeerInboundNotification,
) {
    if matches!(&envelope.payload, AcpPeerInboundPayload::Barrier) {
        return;
    }
    let mut sessions = sessions.lock().await;
    let _ = reduce_acp_inbound_notification(&mut sessions, generation, envelope, None, None, None);
}

pub(super) async fn drain_acp_notification_subscription(
    subscription: &mut AcpNotificationSubscription,
    sessions: &AcpResidentSessions,
    generation: u64,
) {
    let _ = subscription.deactivate();
    while let Some(envelope) = subscription.try_recv() {
        reduce_idle_acp_notification(sessions, generation, envelope).await;
    }
}

pub(super) fn acp_session_snapshot(
    session: &AcpResidentSession,
    generation: u64,
) -> AcpSessionSnapshot {
    let encoded_options = serde_json::to_vec(&session.config_options).unwrap_or_default();
    let effective_legacy_models =
        effective_legacy_models(&session.config_options, session.legacy_models.as_ref());
    let encoded_legacy_models = serde_json::to_vec(&effective_legacy_models).unwrap_or_default();
    let encoded_modes = serde_json::to_vec(
        &session
            .available_modes
            .iter()
            .map(|mode| {
                json!({
                    "id": mode.id,
                    "name": mode.name,
                    "description": mode.description,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let mut control_revision = sha2::Sha256::new();
    control_revision.update(generation.to_be_bytes());
    control_revision.update(session.session_epoch.to_be_bytes());
    control_revision.update(&encoded_options);
    control_revision.update(&encoded_legacy_models);
    control_revision.update(&encoded_modes);
    control_revision.update(
        session
            .current_mode_id
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );

    let commands = session
        .available_commands
        .iter()
        .map(|command| {
            json!({
                "name": command.name,
                "description": command.description,
                "input": command.input,
            })
        })
        .collect::<Vec<_>>();
    let mut projection_revision = sha2::Sha256::new();
    projection_revision.update(generation.to_be_bytes());
    projection_revision.update(session.session_epoch.to_be_bytes());
    projection_revision.update(
        serde_json::to_vec(&json!({
            "agent": session.agent.as_ref().map(|agent| json!({
                "name": agent.name,
                "title": agent.title,
                "version": agent.version,
            })),
            "capabilities": {
                "prompt": {
                    "text": session.capabilities.prompt_input.text,
                    "image": session.capabilities.prompt_input.image,
                    "audio": session.capabilities.prompt_input.audio,
                    "resource": session.capabilities.prompt_input.resource,
                    "resourceLink": session.capabilities.prompt_input.resource_link,
                    "embeddedContext": session.capabilities.prompt_input.embedded_context,
                },
                "session": {
                    "load": session.capabilities.session.load,
                    "list": session.capabilities.session.list,
                    "delete": session.capabilities.session.delete,
                    "fork": session.capabilities.session.fork,
                    "resume": session.capabilities.session.resume,
                    "close": session.capabilities.session.close,
                    "additionalDirectories": session.capabilities.session.additional_directories,
                },
                "authLogout": session.capabilities.auth_logout,
                "mcpHttp": session.capabilities.mcp_http,
                "mcpSse": session.capabilities.mcp_sse,
                "mcpAcp": session.capabilities.mcp_acp,
            },
            "configOptions": serde_json::from_slice::<Value>(&encoded_options).unwrap_or(Value::Null),
            "legacyModels": serde_json::from_slice::<Value>(&encoded_legacy_models).unwrap_or(Value::Null),
            "commands": commands,
            "availableModes": serde_json::from_slice::<Value>(&encoded_modes).unwrap_or(Value::Null),
            "currentModeId": session.current_mode_id,
            "history": {
                "owner": match session.history.owner {
                    AcpHistoryOwnerSnapshot::Agent => "agent",
                    AcpHistoryOwnerSnapshot::Process => "process",
                },
                "resumable": session.history.resumable,
                "loadSupported": session.history.load_supported,
                "resumeSupported": session.history.resume_supported,
                "loadedFromAgent": session.history.loaded_from_agent,
                "replayComplete": session.history.replay_complete,
                "replayUpdateCount": session.history.replay_update_count,
                "liveUpdateCount": session.history.live_update_count,
            },
            "sessionInfo": {
                "title": session.session_info.title,
                "updatedAt": session.session_info.updated_at,
                "usage": session.session_info.usage,
            },
        }))
        .unwrap_or_default(),
    );

    let mut options = project_acp_runtime_options(
        serde_json::to_value(&session.config_options).unwrap_or(Value::Null),
    );
    if let Some(legacy_models) = effective_legacy_models {
        options.push(legacy_model_runtime_option(legacy_models));
    }

    AcpSessionSnapshot {
        native_session_id: session.native_session_id.clone(),
        agent: session.agent.clone(),
        capabilities: session.capabilities.clone(),
        options,
        available_commands: session.available_commands.clone(),
        available_modes: session.available_modes.clone(),
        current_mode_id: session.current_mode_id.clone(),
        legacy_models: effective_legacy_models.cloned(),
        history: session.history.clone(),
        session_info: session.session_info.clone(),
        generation,
        session_epoch: session.session_epoch,
        control_revision: format!("{:x}", control_revision.finalize()),
        projection_revision: format!("{:x}", projection_revision.finalize()),
    }
}
