use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use psychevo::{
    Error,
    config::{RuntimeProfileConfig, RuntimeProfileKind},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::agent_session_binding::{PreparedGatewayAgentTurn, runtime_profile_config_fingerprint};
use super::peer_runtime::ResolvedPeerTurn;
use crate::acp_peer;
use psychevo_gateway_protocol::source::{AgentDeliveryStatusView, AgentErrorView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentErrorStage {
    Configuration,
    Binding,
    Control,
    Delivery,
    History,
    Interaction,
}
impl AgentErrorStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Binding => "binding",
            Self::Control => "control",
            Self::Delivery => "delivery",
            Self::History => "history",
            Self::Interaction => "interaction",
        }
    }
}

pub(crate) fn agent_session_error(
    code: &str,
    stage: AgentErrorStage,
    retry_class: &str,
    delivery: &str,
    message: impl Into<String>,
    diagnostic_ref: Option<String>,
) -> Error {
    let message = message.into();
    Error::structured(
        message.clone(),
        json!({
            "code": code,
            "stage": stage.as_str(),
            "retryClass": retry_class,
            "delivery": delivery,
            "message": message,
            "diagnosticRef": diagnostic_ref,
        }),
    )
}

pub(crate) fn agent_error_view(message: impl Into<String>, data: Option<&Value>) -> AgentErrorView {
    let message = message.into();
    let nested_error = data.and_then(|value| value.get("error"));
    let field = |name: &str| {
        data.and_then(|value| value.get(name))
            .or_else(|| nested_error.and_then(|value| value.get(name)))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let delivery = match field("delivery").as_deref() {
        Some("not_delivered" | "notDelivered") => AgentDeliveryStatusView::NotDelivered,
        Some("delivered") => AgentDeliveryStatusView::Delivered,
        Some("unknown") | Some(_) | None => AgentDeliveryStatusView::Unknown,
    };
    AgentErrorView {
        message,
        code: field("code"),
        stage: field("stage"),
        retry_class: field("retryClass"),
        delivery,
        recovery_action: field("recoveryAction"),
        diagnostic_ref: field("diagnosticRef"),
    }
}

pub(crate) fn agent_session_configuration_error(message: impl Into<String>) -> Error {
    agent_session_error(
        "configuration",
        AgentErrorStage::Configuration,
        "user_action",
        "not_delivered",
        message,
        None,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BoundAgentSessionIdentity {
    thread_id: String,
    binding_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundAgentSessionCapture {
    identity: BoundAgentSessionIdentity,
    runtime_ref: String,
    profile_fingerprint: String,
    agent_fingerprint: String,
    adapter_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentSessionAttachmentIdentity {
    Bound(BoundAgentSessionCapture),
    Invocation { invocation_id: String },
}

pub(super) struct CapturedAgentSessionTarget {
    identity: AgentSessionAttachmentIdentity,
    profile: RuntimeProfileConfig,
    peer: Option<ResolvedPeerTurn>,
}

impl CapturedAgentSessionTarget {
    pub(super) fn application_bound(
        binding: &psychevo::AgentBindingSnapshot,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
    ) -> psychevo::Result<Self> {
        Self::captured_bound(
            BoundAgentSessionCapture {
                identity: BoundAgentSessionIdentity {
                    thread_id: binding.thread_id.clone(),
                    binding_revision: binding.binding_revision,
                },
                runtime_ref: binding.runtime_ref.clone(),
                profile_fingerprint: binding.profile_fingerprint.clone(),
                agent_fingerprint: binding.agent_fingerprint.clone(),
                adapter_kind: binding.adapter_kind.clone(),
            },
            profile,
            peer,
        )
    }

    fn captured_bound(
        capture: BoundAgentSessionCapture,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
    ) -> psychevo::Result<Self> {
        if capture.runtime_ref != profile.id {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{}` captured Runtime Profile `{}`, not `{}`.",
                capture.identity.thread_id, capture.runtime_ref, profile.id
            )));
        }
        if capture.profile_fingerprint != runtime_profile_config_fingerprint(&profile) {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{}` no longer matches its captured Runtime Profile.",
                capture.identity.thread_id
            )));
        }
        Ok(Self {
            identity: AgentSessionAttachmentIdentity::Bound(capture),
            profile,
            peer,
        })
    }

    pub(super) fn invocation(
        invocation_id: impl Into<String>,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
    ) -> Self {
        Self {
            identity: AgentSessionAttachmentIdentity::Invocation {
                invocation_id: invocation_id.into(),
            },
            profile,
            peer,
        }
    }
}

enum AgentSessionTarget {
    Native {
        profile: RuntimeProfileConfig,
    },
    Acp {
        peer: Box<ResolvedPeerTurn>,
        profile: RuntimeProfileConfig,
    },
}

#[derive(Clone)]
pub(super) struct AgentSessionRef {
    pub(super) cwd: PathBuf,
    pub(super) local_session_id: String,
    pub(super) native_session_id: String,
    pub(super) mcp_servers: Vec<psychevo::application::ResolvedMcpServerInput>,
}

pub(super) struct AgentSessionDiscoveryQuery {
    pub(super) cwd_filter: Option<PathBuf>,
    pub(super) cursor: Option<String>,
}

#[derive(Debug)]
pub(super) enum AgentSessionSnapshot {
    #[cfg(test)]
    Native {
        profile_id: String,
    },
    Acp(Box<acp_peer::session_projection::AcpSessionSnapshot>),
}

impl AgentSessionSnapshot {
    pub(super) fn into_acp(
        self,
    ) -> psychevo::Result<acp_peer::session_projection::AcpSessionSnapshot> {
        match self {
            Self::Acp(snapshot) => Ok(*snapshot),
            #[cfg(test)]
            Self::Native { profile_id } => Err(agent_session_error(
                "agent_session_snapshot_mismatch",
                AgentErrorStage::Configuration,
                "never",
                "not_delivered",
                format!(
                    "Agent Session inspection for Native profile `{profile_id}` cannot be decoded as an ACP session."
                ),
                Some("agent-session:snapshot-kind".to_string()),
            )),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AgentSessionHost {
    acp: acp_peer::process_pool::AcpProcessPool,
    /// Captured attachment identity, not a second command mailbox. Native
    /// ordering remains in the Framework Application and ACP ordering remains in the
    /// resident process/session actor.
    bound_attachments: Arc<Mutex<HashMap<BoundAgentSessionIdentity, BoundAgentSessionCapture>>>,
    prepared_sessions: Arc<Mutex<HashMap<String, PreparedAgentSession>>>,
    prepared_imports: Arc<Mutex<HashMap<String, CapturedFrameworkAgentImport>>>,
}

#[derive(Clone)]
pub(crate) struct CapturedFrameworkAgentImport {
    pub(crate) target: PreparedGatewayAgentTurn,
    pub(crate) context: CapturedAgentImportContext,
    pub(crate) native_session_id: String,
    pub(crate) title: Option<String>,
    pub(crate) target_label: String,
}

#[derive(Clone)]
pub(crate) struct CapturedAgentImportContext {
    pub(crate) cwd: PathBuf,
    pub(crate) runtime_options: BTreeMap<String, String>,
}

pub(crate) struct CapturedFrameworkAgentImportReservation {
    host: AgentSessionHost,
    token: psychevo::AgentSessionImportToken,
}

impl CapturedFrameworkAgentImportReservation {
    pub(crate) fn token(&self) -> psychevo::AgentSessionImportToken {
        self.token.clone()
    }
}

impl Drop for CapturedFrameworkAgentImportReservation {
    fn drop(&mut self) {
        self.host.discard_framework_import(&self.token);
    }
}

#[derive(Clone)]
struct PreparedAgentSession {
    target_id: String,
    agent_ref: Option<String>,
    runtime_ref: String,
    profile_fingerprint: String,
    cwd: PathBuf,
    local_session_id: String,
    native_session_id: String,
    mcp_servers: Vec<psychevo::application::ResolvedMcpServerInput>,
    peer: ResolvedPeerTurn,
}

impl fmt::Debug for AgentSessionHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSessionHost")
            .finish_non_exhaustive()
    }
}

impl AgentSessionHost {
    pub(crate) fn new() -> Self {
        Self {
            acp: acp_peer::process_pool::AcpProcessPool::default(),
            bound_attachments: Arc::new(Mutex::new(HashMap::new())),
            prepared_sessions: Arc::new(Mutex::new(HashMap::new())),
            prepared_imports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) async fn prepare(
        &self,
        captured: CapturedAgentSessionTarget,
        source_key: String,
        target_id: String,
        agent_ref: Option<String>,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo::application::ResolvedMcpServerInput>,
    ) -> psychevo::Result<acp_peer::session_projection::AcpSessionSnapshot> {
        let attached = self.attach(captured)?;
        let (peer, profile) = match &attached.target {
            AgentSessionTarget::Native { profile } => {
                return Err(agent_session_configuration_error(format!(
                    "Native profile `{}` does not create a prepared Agent session.",
                    profile.id
                )));
            }
            AgentSessionTarget::Acp { peer, profile } => (peer.as_ref().clone(), profile.clone()),
        };
        let profile_fingerprint = runtime_profile_config_fingerprint(&profile);
        let existing = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(&source_key)
            .filter(|prepared| {
                prepared.target_id == target_id
                    && prepared.cwd == cwd
                    && prepared.profile_fingerprint == profile_fingerprint
            })
            .cloned();
        if let Some(existing) = existing
            && let Some(snapshot) = self
                .acp
                .inspect_cached(
                    existing.local_session_id.clone(),
                    existing.native_session_id.clone(),
                )
                .await?
        {
            return Ok(snapshot);
        }
        self.release_prepared(&source_key).await?;

        let digest = Sha256::digest(
            format!(
                "{source_key}\0{target_id}\0{}\0{profile_fingerprint}",
                cwd.display()
            )
            .as_bytes(),
        );
        let local_session_id = format!("draft:{:x}", digest);
        let snapshot = self
            .acp
            .prepare_session(
                peer.clone(),
                cwd.clone(),
                local_session_id.clone(),
                mcp_servers.clone(),
            )
            .await?;
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .insert(
                source_key,
                PreparedAgentSession {
                    target_id,
                    agent_ref,
                    runtime_ref: profile.id,
                    profile_fingerprint,
                    cwd,
                    local_session_id,
                    native_session_id: snapshot.native_session_id.clone(),
                    mcp_servers,
                    peer,
                },
            );
        Ok(snapshot)
    }

    pub(super) async fn release_prepared(&self, source_key: &str) -> psychevo::Result<bool> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .remove(source_key);
        let Some(prepared) = prepared else {
            return Ok(false);
        };
        let session = acp_peer::lifecycle::AcpResidentSessionRef {
            local_session_id: prepared.local_session_id,
            native_session_id: prepared.native_session_id,
        };
        if self
            .acp
            .close_session(prepared.peer, prepared.cwd, session.clone())
            .await
            .is_err()
        {
            self.acp.release_session(session).await?;
        }
        Ok(true)
    }

    pub(super) async fn inspect_prepared(
        &self,
        source_key: &str,
        target_id: &str,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| prepared.target_id == target_id)
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .inspect_cached(prepared.local_session_id, prepared.native_session_id)
            .await
    }

    pub(super) async fn set_prepared_control(
        &self,
        source_key: &str,
        target_id: &str,
        control_id: String,
        value: Value,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| prepared.target_id == target_id)
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .set_control(acp_peer::process_pool::AcpSetControlInput {
                peer: prepared.peer,
                cwd: prepared.cwd,
                local_session_id: prepared.local_session_id,
                native_session_id: prepared.native_session_id,
                mcp_servers: prepared.mcp_servers,
                control_id,
                value,
            })
            .await
            .map(Some)
    }

    pub(super) async fn promote_prepared(
        &self,
        source_key: &str,
        agent_ref: Option<&str>,
        runtime_ref: &str,
        profile_fingerprint: &str,
        thread_id: &str,
    ) -> psychevo::Result<Option<String>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| {
                prepared.agent_ref.as_deref() == agent_ref
                    && prepared.runtime_ref == runtime_ref
                    && prepared.profile_fingerprint == profile_fingerprint
            })
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .promote_session(
                prepared.local_session_id,
                thread_id.to_string(),
                prepared.native_session_id.clone(),
            )
            .await?;
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .remove(source_key);
        Ok(Some(prepared.native_session_id))
    }

    pub(super) fn attach(
        &self,
        captured: CapturedAgentSessionTarget,
    ) -> psychevo::Result<AttachedAgent> {
        let CapturedAgentSessionTarget {
            identity,
            profile,
            mut peer,
        } = captured;
        if let Some(peer) = peer.as_mut() {
            peer.process_scope_fingerprint = Some(runtime_profile_config_fingerprint(&profile));
        }
        let target = match profile.runtime {
            RuntimeProfileKind::Native => {
                if peer.is_some() {
                    return Err(agent_session_configuration_error(
                        "Native Runtime Profile resolved an ACP Agent backend.",
                    ));
                }
                AgentSessionTarget::Native { profile }
            }
            RuntimeProfileKind::Acp => AgentSessionTarget::Acp {
                peer: Box::new(peer.ok_or_else(|| {
                    agent_session_configuration_error(format!(
                        "ACP Runtime Profile `{}` references an unavailable backend.",
                        profile.id
                    ))
                })?),
                profile,
            },
        };
        if let AgentSessionAttachmentIdentity::Bound(capture) = &identity {
            let mut attachments = self
                .bound_attachments
                .lock()
                .expect("Agent Session attachment registry poisoned");
            match attachments.get(&capture.identity) {
                Some(existing) if existing != capture => {
                    return Err(agent_session_error(
                        "agent_session_attachment_conflict",
                        AgentErrorStage::Binding,
                        "never",
                        "not_delivered",
                        format!(
                            "Thread `{}` binding revision {} was attached with a different immutable Agent target.",
                            capture.identity.thread_id, capture.identity.binding_revision
                        ),
                        Some(format!(
                            "agent-binding:{}:{}",
                            capture.identity.thread_id, capture.identity.binding_revision
                        )),
                    ));
                }
                Some(_) => {}
                None => {
                    attachments.insert(capture.identity.clone(), capture.clone());
                }
            }
        }
        Ok(AttachedAgent {
            host: self.clone(),
            _identity: identity,
            target,
        })
    }

    pub(super) async fn discover(
        &self,
        captured: CapturedAgentSessionTarget,
        query: AgentSessionDiscoveryQuery,
    ) -> psychevo::Result<acp_peer::lifecycle::AcpSessionListPage> {
        let invocation_cwd = query.cwd_filter.clone().ok_or_else(|| {
            agent_session_configuration_error("Agent session discovery requires a workspace cwd.")
        })?;
        let attached = self.attach(captured)?;
        match &attached.target {
            AgentSessionTarget::Native { profile } => Err(agent_session_error(
                "agent_session_discovery_unsupported",
                AgentErrorStage::History,
                "user_action",
                "not_delivered",
                format!(
                    "Native profile `{}` does not own external Agent sessions.",
                    profile.id
                ),
                None,
            )),
            AgentSessionTarget::Acp { peer, .. } => {
                self.acp
                    .list_sessions(
                        peer.as_ref().clone(),
                        invocation_cwd,
                        query.cwd_filter,
                        query.cursor,
                    )
                    .await
            }
        }
    }

    pub(super) async fn shutdown(&self, force: bool) -> psychevo::Result<()> {
        let result = self.acp.shutdown(force).await;
        self.bound_attachments
            .lock()
            .expect("Agent Session attachment registry poisoned")
            .clear();
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .clear();
        self.prepared_imports
            .lock()
            .expect("prepared Agent import registry poisoned")
            .clear();
        result
    }

    pub(super) fn reserve_framework_import(
        &self,
        captured: CapturedFrameworkAgentImport,
    ) -> CapturedFrameworkAgentImportReservation {
        let token = psychevo::AgentSessionImportToken::unique();
        self.prepared_imports
            .lock()
            .expect("prepared Agent import registry poisoned")
            .insert(token.as_str().to_string(), captured);
        CapturedFrameworkAgentImportReservation {
            host: self.clone(),
            token,
        }
    }

    pub(super) fn consume_framework_import(
        &self,
        token: &psychevo::AgentSessionImportToken,
    ) -> psychevo::Result<CapturedFrameworkAgentImport> {
        self.prepared_imports
            .lock()
            .expect("prepared Agent import registry poisoned")
            .remove(token.as_str())
            .ok_or_else(|| {
                agent_session_configuration_error(
                    "The captured Agent import preparation is missing or was already consumed.",
                )
            })
    }

    fn discard_framework_import(&self, token: &psychevo::AgentSessionImportToken) {
        self.prepared_imports
            .lock()
            .expect("prepared Agent import registry poisoned")
            .remove(token.as_str());
    }

    #[cfg(test)]
    pub(super) fn bound_attachment_count(&self) -> usize {
        self.bound_attachments
            .lock()
            .expect("attachment registry poisoned")
            .len()
    }

    pub(super) async fn run_framework_acp_turn(
        &self,
        peer: ResolvedPeerTurn,
        profile: RuntimeProfileConfig,
        request: acp_peer::turn::AcpPeerTurnRequest,
        session_ready: acp_peer::process_pool::AcpSessionReadyCallback,
    ) -> psychevo::Result<acp_peer::turn::AcpPeerTurnResult> {
        acp_peer::turn::run_acp_peer_turn(&self.acp, peer, &profile, request, session_ready).await
    }

    pub(super) async fn inspect_cached_acp_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        self.acp
            .inspect_cached(local_session_id, native_session_id)
            .await
    }

    pub(super) async fn release_acp_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo::Result<()> {
        self.acp
            .release_session(acp_peer::lifecycle::AcpResidentSessionRef {
                local_session_id,
                native_session_id,
            })
            .await
    }

    // Protocol and authentication diagnosis belong to backend administration,
    // not to an attached public Thread session, so they deliberately stay
    // outside the typed attached-session operation family.
    pub(super) async fn probe_acp_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo::Result<acp_peer::process_pool::AcpProtocolDoctorStatus> {
        self.acp.probe_protocol_compatibility(peer, cwd).await
    }

    pub(super) async fn probe_acp_authentication(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo::Result<acp_peer::process_pool::AcpAuthDoctorStatus> {
        self.acp.probe_authentication(peer, cwd).await
    }
}

pub(super) struct AttachedAgent {
    host: AgentSessionHost,
    _identity: AgentSessionAttachmentIdentity,
    target: AgentSessionTarget,
}

impl AttachedAgent {
    fn unsupported_lifecycle<T>(
        &self,
        profile: &RuntimeProfileConfig,
        operation: &str,
    ) -> psychevo::Result<T> {
        Err(agent_session_error(
            "agent_session_lifecycle_unsupported",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            format!(
                "Native profile `{}` does not expose Agent session/{operation}.",
                profile.id
            ),
            None,
        ))
    }

    pub(super) async fn resume_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo::Result<AgentSessionSnapshot> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "resume"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .resume_session(
                    peer.as_ref().clone(),
                    session.cwd,
                    acp_peer::lifecycle::AcpResidentSessionRef {
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id,
                    },
                    session.mcp_servers,
                )
                .await
                .map(|snapshot| AgentSessionSnapshot::Acp(Box::new(snapshot))),
        }
    }

    pub(super) async fn load_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo::Result<acp_peer::stream_state::AcpSessionLoadOutput> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "load"),
            AgentSessionTarget::Acp { peer, .. } => {
                self.host
                    .acp
                    .load_session(
                        peer.as_ref().clone(),
                        session.cwd,
                        session.local_session_id,
                        session.native_session_id,
                        session.mcp_servers,
                    )
                    .await
            }
        }
    }

    pub(super) async fn fork_session(
        &self,
        source: AgentSessionRef,
        fork_local_session_id: String,
    ) -> psychevo::Result<AgentSessionSnapshot> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "fork"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .fork_session(
                    peer.as_ref().clone(),
                    source.cwd,
                    acp_peer::lifecycle::AcpResidentSessionRef {
                        local_session_id: source.local_session_id,
                        native_session_id: source.native_session_id,
                    },
                    fork_local_session_id,
                )
                .await
                .map(|snapshot| AgentSessionSnapshot::Acp(Box::new(snapshot))),
        }
    }

    pub(super) async fn close_session(&self, session: AgentSessionRef) -> psychevo::Result<()> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "close"),
            AgentSessionTarget::Acp { peer, .. } => {
                self.host
                    .acp
                    .close_session(
                        peer.as_ref().clone(),
                        session.cwd,
                        acp_peer::lifecycle::AcpResidentSessionRef {
                            local_session_id: session.local_session_id,
                            native_session_id: session.native_session_id,
                        },
                    )
                    .await
            }
        }
    }

    pub(super) async fn delete_session(&self, session: AgentSessionRef) -> psychevo::Result<()> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "delete"),
            AgentSessionTarget::Acp { peer, .. } => {
                let resident = self
                    .host
                    .acp
                    .inspect_cached(
                        session.local_session_id.clone(),
                        session.native_session_id.clone(),
                    )
                    .await?
                    .map(|_| acp_peer::lifecycle::AcpResidentSessionRef {
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id.clone(),
                    });
                self.host
                    .acp
                    .delete_session(
                        peer.as_ref().clone(),
                        session.cwd,
                        session.native_session_id,
                        resident,
                    )
                    .await
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn inspect(
        &self,
        session: AgentSessionRef,
    ) -> psychevo::Result<AgentSessionSnapshot> {
        match &self.target {
            AgentSessionTarget::Native { profile } => Ok(AgentSessionSnapshot::Native {
                profile_id: profile.id.clone(),
            }),
            AgentSessionTarget::Acp { peer, .. } => {
                let snapshot = self
                    .host
                    .acp
                    .inspect(
                        peer.as_ref().clone(),
                        session.cwd,
                        session.local_session_id,
                        session.native_session_id,
                        session.mcp_servers,
                    )
                    .await?;
                Ok(AgentSessionSnapshot::Acp(Box::new(snapshot)))
            }
        }
    }

    pub(super) async fn set_control(
        &self,
        session: AgentSessionRef,
        control_id: String,
        value: Value,
    ) -> psychevo::Result<AgentSessionSnapshot> {
        match &self.target {
            AgentSessionTarget::Native { profile } => Err(agent_session_error(
                "unsupported_control",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                format!(
                    "Native profile `{}` applies controls when a turn is submitted and does not expose live Agent-session control mutation.",
                    profile.id
                ),
                Some(format!("agent-session:{}", session.local_session_id)),
            )),
            AgentSessionTarget::Acp { peer, .. } => {
                let snapshot = self
                    .host
                    .acp
                    .set_control(acp_peer::process_pool::AcpSetControlInput {
                        peer: peer.as_ref().clone(),
                        cwd: session.cwd,
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id,
                        mcp_servers: session.mcp_servers,
                        control_id,
                        value,
                    })
                    .await?;
                Ok(AgentSessionSnapshot::Acp(Box::new(snapshot)))
            }
        }
    }
}
