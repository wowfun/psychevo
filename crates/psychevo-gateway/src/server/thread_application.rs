use std::collections::BTreeMap;
use std::path::PathBuf;

use futures::future::BoxFuture;
use psychevo::Error;
use psychevo::application::{ClarifyAnswer, ClarifyResponse, ClarifyResult, WorkspaceMutationSink};
use psychevo::application::{Message as RuntimeMessage, UserContentBlock};
use psychevo::thread_lineage::side_conversation_session_source;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::gateway::activity::{ThreadCallerContext, ThreadSurface, ThreadTurnIntent};
use crate::gateway::agent_session::{AgentErrorStage, agent_session_error};
use crate::gateway::public_api::BoundedTranscriptPage;
use crate::gateway::results::GatewayTurnResult;
use crate::history_editing::HistoryEditingSurface;
use crate::journey_profile::{GatewayProfileFields, gateway_profile_mark};
use crate::{GatewayEventEmitter, gateway_now_ms, transcript};
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, PendingActionView, PermissionDecision,
};
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewayInputPart, GatewaySource, GatewaySourceLifetime,
    GatewayThread, GatewayThreadSelector, GatewayTurnStatus,
};

use super::auth_input::{TurnStartInputExt, apply_mentions_to_turn_policy, authorize_thread};
use super::binding::{AuthContext, WebState};
use super::event_delivery::ConnectionSender;
use super::rpc_dispatch::{
    enqueue_thread_compact_result_for_thread, thread_compact_result_for_thread,
};
use super::rpc_json::{cwd_source, permission_decision};
use super::runtime_profiles::{
    RunnableTargetCatalog, ThreadDraftPrepareWork, apply_thread_control_precedence,
    cached_thread_history_descriptor, ensure_turn_runtime_profile_supported,
    prepare_draft_source_lane, runnable_target_input, runtime_backend_kind,
    selected_context_target_id, thread_context_read_result_live,
    thread_context_read_result_live_with_catalog_and_configured,
    thread_control_override_string_value, thread_control_set_result, thread_draft_prepare_result,
    thread_draft_prepare_result_with_work, validate_turn_runnable_target,
};
use super::scope_session::{
    ResolvedScope, canonical_source_mutation_key, detached_draft_scope, ensure_turn_start_thread,
    resolve_optional_scope, resolve_required_scope, resolve_start_scope, resolved_scope_for_thread,
    update_browser_session_for_draft_scope,
};
use super::session_import_application::{
    fork_acp_thread, fork_native_thread, typed_thread_snapshot,
};
use super::session_view::{
    active_turn_projection_window, prune_pending_actions, replay_running_live_transcript_overlay,
    snapshot_activity, thread_snapshot, thread_snapshot_live,
};

pub(super) fn prewarm_codex_runtime_inventory(state: &WebState, cwd: PathBuf) {
    let warm_state = state.clone();
    state
        .inner
        .gateway
        .spawn_background("codex-runtime-inventory-prewarm", async move {
            let _ = warm_state
                .inner
                .codex_capability_broker
                .refresh_runtime_inventory(&cwd)
                .await;
        });
}

pub(super) async fn inspect_thread(
    state: &WebState,
    auth: &AuthContext,
    params: wire::agents_backend_rpc::ThreadContextReadParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadContextReadResult> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    if let Some(thread_id) = params.thread_id.as_deref() {
        authorize_thread(state, auth, thread_id).await?;
    }
    thread_context_read_result_live(state, &scope, params).await
}

pub(super) async fn prepare_thread_draft(
    state: &WebState,
    auth: &AuthContext,
    params: wire::agents_backend_rpc::ThreadDraftPrepareParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadDraftPrepareResult> {
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    let _source_mutation = state
        .inner
        .gateway
        .lock_source_mutation(&canonical_source_mutation_key(&scope.source))
        .await;
    thread_draft_prepare_result(state, &scope, params).await
}

pub(super) async fn set_thread_control(
    state: &WebState,
    auth: &AuthContext,
    params: wire::agents_backend_rpc::ThreadControlSetParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadControlSetResult> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    if let Some(thread_id) = params.thread_id.as_deref() {
        authorize_thread(state, auth, thread_id).await?;
    }
    thread_control_set_result(state, &scope, params).await
}

pub(super) async fn run_thread_action(
    state: &WebState,
    auth: &AuthContext,
    out_tx: ConnectionSender,
    params: wire::thread_command_turn::ThreadActionRunParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadActionRunResult> {
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    run_action(state, auth, &scope, params, out_tx).await
}

pub(super) async fn respond_to_thread_interaction(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadInteractionRespondParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadInteractionRespondResult> {
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    respond_to_interaction(state, auth, &scope, params).await
}

pub(super) async fn read_thread_history(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadHistoryReadParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryReadResult> {
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    read_history(state, auth, &scope, params).await
}

pub(super) async fn read_thread_history_draft(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadHistoryDraftReadParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryDraftReadResult> {
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    read_history_draft(state, auth, &scope, params).await
}

pub(super) async fn open_thread_draft(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadDraftOpenParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadDraftOpenResult> {
    let scope = resolve_start_scope(state, auth, params.origin.clone())?;
    gateway_profile_mark(
        "thread_draft_open_received",
        None,
        None,
        GatewayProfileFields {
            request_method: Some("thread/draft/open"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    let _source_mutation = state
        .inner
        .gateway
        .lock_source_mutation(&canonical_source_mutation_key(&scope.source))
        .await;
    state
        .inner
        .gateway
        .release_prepared_agent_session(&scope.source.source_key().0)
        .await?;
    state
        .inner
        .gateway
        .clear_source_binding(&scope.source)
        .await?;
    prewarm_codex_runtime_inventory(state, scope.cwd.clone());
    let snapshot_scope = detached_draft_scope(&scope, auth);
    update_browser_session_for_draft_scope(state, auth, &snapshot_scope).await?;
    let snapshot = serde_json::from_value(thread_snapshot(state, &snapshot_scope, None).await?)?;
    let target_catalog = RunnableTargetCatalog::load(state, &snapshot_scope)?;
    gateway_profile_mark(
        "thread_draft_catalog_loaded",
        None,
        None,
        GatewayProfileFields {
            request_method: Some("thread/draft/open"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    let target = match params.target_intent {
        wire::thread_command_turn::ThreadDraftTargetIntent::Default => {
            gateway_profile_mark(
                "thread_draft_target_discovery_started",
                None,
                None,
                GatewayProfileFields {
                    request_method: Some("thread/draft/open"),
                    runtime_source: Some("web"),
                    ..GatewayProfileFields::default()
                },
            );
            let target = target_catalog
                .default_draft_target(state, &snapshot_scope)
                .await?;
            gateway_profile_mark(
                "thread_draft_target_discovery_completed",
                None,
                None,
                GatewayProfileFields {
                    request_method: Some("thread/draft/open"),
                    runtime_source: Some("web"),
                    ..GatewayProfileFields::default()
                },
            );
            target
        }
        wire::thread_command_turn::ThreadDraftTargetIntent::Exact { target_id } => {
            gateway_profile_mark(
                "thread_draft_target_discovery_skipped",
                None,
                None,
                GatewayProfileFields {
                    request_method: Some("thread/draft/open"),
                    runtime_source: Some("web"),
                    ..GatewayProfileFields::default()
                },
            );
            target_catalog.by_id(&target_id).cloned().ok_or_else(|| {
                agent_session_error(
                    "target_not_found",
                    AgentErrorStage::Binding,
                    "user_action",
                    "not_delivered",
                    "The selected Agent target is no longer present in this workspace catalog. Refresh Thread Context and select another target.",
                    None,
                )
            })?
        }
    };
    let source_lane_prepared = if target.ready {
        prepare_draft_source_lane(state, &snapshot_scope, &target).await?;
        true
    } else {
        false
    };
    let (context, configured) = thread_context_read_result_live_with_catalog_and_configured(
        state,
        &snapshot_scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: None,
            target: Some(runnable_target_input(&target)),
            scope: Some(snapshot_scope.to_wire_scope()),
        },
        target_catalog.clone(),
    )
    .await?;
    gateway_profile_mark(
        "thread_draft_prepare_started",
        None,
        None,
        GatewayProfileFields {
            request_method: Some("thread/draft/open"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    let prepared = thread_draft_prepare_result_with_work(
        state,
        &snapshot_scope,
        wire::agents_backend_rpc::ThreadDraftPrepareParams {
            scope: snapshot_scope.to_wire_scope(),
            target_id: target.target_id.clone(),
        },
        ThreadDraftPrepareWork {
            target_catalog,
            target,
            context,
            configured,
            source_lane_prepared,
        },
    )
    .await?;
    gateway_profile_mark(
        "thread_draft_prepare_completed",
        None,
        None,
        GatewayProfileFields {
            request_method: Some("thread/draft/open"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    gateway_profile_mark(
        "thread_draft_open_completed",
        None,
        None,
        GatewayProfileFields {
            request_method: Some("thread/draft/open"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    Ok(wire::agents_backend_rpc::ThreadDraftOpenResult {
        snapshot,
        context: prepared.context,
        problem: prepared.problem,
    })
}

pub(super) async fn start_thread_turn(
    state: &WebState,
    auth: &AuthContext,
    out_tx: ConnectionSender,
    params: wire::thread_command_turn::TurnStartParams,
) -> psychevo::Result<wire::thread_command_turn::TurnStartResult> {
    gateway_profile_mark(
        "turn_start_received",
        None,
        params.thread_id.as_deref(),
        GatewayProfileFields {
            request_method: Some("turn/start"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    let scope = resolve_required_scope(state, auth, params.scope.clone())?;
    if params.client_turn_id.trim().is_empty() {
        return Err(Error::Message(
            "turn/start requires a non-empty `clientTurnId`".to_string(),
        ));
    }
    let input = params.input_parts()?;
    let requested_thread_id = match params.thread_id.clone() {
        Some(thread_id) => {
            authorize_thread(state, auth, &thread_id).await?;
            Some(thread_id)
        }
        None => None,
    };
    let validated_target = params
        .target
        .as_ref()
        .map(|target| validate_turn_runnable_target(state, &scope, target))
        .transpose()?;
    let turn_context = validate_turn_revisions(
        state,
        &scope,
        requested_thread_id.clone(),
        params.target.clone(),
        params.expected_context_revision.as_deref(),
        params.expected_control_revision.as_deref(),
    )
    .await?;
    let existing_binding = turn_context.binding.as_ref();
    if let (Some(binding), Some(target)) = (existing_binding.as_ref(), validated_target.as_ref()) {
        if binding.runtime_ref != target.runtime_profile_ref {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread is bound to Runtime Profile `{bound}`; start a new thread to use `{}`.",
                    target.runtime_profile_ref,
                    bound = binding.runtime_ref,
                ),
                requested_thread_id
                    .as_ref()
                    .map(|thread_id| format!("agent-binding:{thread_id}")),
            ));
        }
        if binding.agent_ref != target.agent_ref {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread is bound to Agent target `{}`; start a new thread to use `{}`.",
                    binding.agent_ref.as_deref().unwrap_or("Default Agent"),
                    target.agent_ref.as_deref().unwrap_or("Default Agent"),
                ),
                requested_thread_id
                    .as_ref()
                    .map(|thread_id| format!("agent-binding:{thread_id}")),
            ));
        }
    }
    let runtime_profile_ref = match (
        existing_binding.map(|binding| binding.runtime_ref.as_str()),
        validated_target.as_ref(),
    ) {
        (Some(bound), _) => bound.to_string(),
        (None, Some(target)) => target.runtime_profile_ref.clone(),
        (None, _) => {
            return Err(agent_session_error(
                "target_required",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "An unbound turn requires `target.runtimeProfileRef`.",
                None,
            ));
        }
    };
    if existing_binding.is_none() {
        ensure_turn_runtime_profile_supported(state, &scope, Some(runtime_profile_ref.as_str()))?;
    }
    validate_turn_admission(
        &turn_context,
        &input,
        &params.mentions,
        &params.turn_overrides,
    )?;
    let mut control_values = BTreeMap::new();
    apply_thread_control_precedence(
        state,
        &scope,
        requested_thread_id.as_deref(),
        &mut control_values,
    )
    .await?;
    let initial_thread_preferences = source_draft_control_values(&turn_context)?;
    control_values.extend(initial_thread_preferences.clone());
    let response_backend_kind = validated_target
        .as_ref()
        .map(|target| target.backend_kind)
        .map(Ok)
        .unwrap_or_else(|| {
            turn_context
                .binding
                .as_ref()
                .map(|binding| match binding.backend_kind.as_str() {
                    "native" => Ok(wire::source::BackendKind::Native),
                    "acp" => Ok(wire::source::BackendKind::Acp),
                    _ => Err(agent_session_error(
                        "bound_backend_kind_invalid",
                        AgentErrorStage::Binding,
                        "never",
                        "not_delivered",
                        "The captured Thread binding has an invalid backend kind.",
                        Some(format!("agent-binding:{}", binding.thread_id)),
                    )),
                })
                .unwrap_or_else(|| runtime_backend_kind(state, &scope, &runtime_profile_ref))
        })?;
    let requested_side_conversation_thread = if let Some(thread_id) = requested_thread_id.as_deref()
    {
        state
            .inner
            .framework
            .resume_thread(thread_id)
            .await?
            .summary()
            .await
            .map(|summary| side_conversation_session_source(&summary.source))?
    } else {
        false
    };
    let (thread_id, creates_thread) = if requested_side_conversation_thread {
        (requested_thread_id, false)
    } else {
        ensure_turn_start_thread(state, &scope, requested_thread_id).await?
    };
    let source = (!requested_side_conversation_thread).then(|| scope.source.clone());
    let event_selector = thread_id
        .as_ref()
        .map(GatewayThreadSelector::thread_id)
        .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
    let event_thread_id = thread_id.clone();
    let event_state = state.clone();
    let review_cwd = scope.cwd.clone();
    let event_tx = out_tx.clone();
    let event_sink = GatewayEventEmitter::new(move |event| {
        let context =
            event_state.pending_context_for_selector(&event_selector, event_thread_id.as_deref());
        event_state.publish_gateway_event_for_connection(
            event,
            context,
            Some(&review_cwd),
            Some(&event_tx),
        );
    });
    let bind_source = (!requested_side_conversation_thread).then(|| cwd_source(&scope.cwd));
    thread_id.as_ref().ok_or_else(|| {
        agent_session_error(
            "thread_creation_failed",
            AgentErrorStage::Binding,
            "retry",
            "not_delivered",
            "Gateway accepted turn preparation without creating a public Thread.",
            None,
        )
    })?;
    let requested_turn_id = Uuid::now_v7().to_string();
    let mutation_turn_id = requested_turn_id.clone();
    let mutation_cwd = scope.cwd.clone();
    let review = state.inner.review.clone();
    let workspace_mutations = WorkspaceMutationSink::new(move |mutation| {
        review.observe_mutation(&mutation_turn_id, &mutation_cwd, mutation);
    });
    let mut prepared = prepare_routed_turn(
        state,
        &scope,
        RoutedThreadTurn {
            thread_id,
            context: turn_context,
            control_values,
            initial_thread_preferences,
            input,
            mentions: params.mentions,
            turn_overrides: params.turn_overrides,
            runtime_source: "web".to_string(),
            continue_sources: vec!["run".to_string(), "tui".to_string(), "web".to_string()],
            event_sink: Some(event_sink),
            workspace_mutations: Some(workspace_mutations),
            lineage: None,
            source,
            bind_source,
            turn_id: Some(requested_turn_id),
        },
    )
    .await?;
    prepared.intent.client_turn_id = Some(params.client_turn_id);
    let initial_preferences = prepared.intent.policy.initial_thread_preferences.clone();
    let thread_id = prepared
        .intent
        .thread_id
        .clone()
        .expect("prepared routed Turn has a Thread identity");
    let submission = prepared.intent.into_framework_request(prepared.caller)?;
    let observers = submission.observers;
    let accepted = if creates_thread {
        let source = prepared.initial_source.and_then(|source| {
            (source.lifetime == GatewaySourceLifetime::Persistent).then(|| {
                psychevo::InitialThreadSourceAssociation {
                    source_key: source.source_key().0,
                    source_kind: source.kind,
                    raw_identity: source.raw_identity.unwrap_or(Value::Null),
                    visible_name: source.visible_name,
                    lineage: prepared.lineage.clone(),
                }
            })
        });
        let preferences = initial_preferences
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect();
        let mut start = psychevo::StartThreadRequest::new(&scope.cwd);
        start.source = "web".to_string();
        start.metadata = prepared.lineage;
        state
            .inner
            .framework
            .start_thread_with_turn(
                start
                    .with_execution_source_key(prepared.execution_source_key)
                    .with_initial_context(thread_id, source, preferences),
                submission.request,
            )
            .await?
    } else {
        state
            .inner
            .framework
            .resume_thread(&submission.thread_id)
            .await?
            .start_turn(submission.request)
            .await?
    };
    observers.attach(&state.inner.gateway, accepted.clone());
    let response_thread_id = accepted.receipt().thread_id.clone();
    let response_turn_id = accepted.receipt().turn_id.clone();
    gateway_profile_mark(
        "turn_start_admitted",
        Some(&response_turn_id),
        Some(&response_thread_id),
        GatewayProfileFields {
            request_method: Some("turn/start"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    if let Some(lease_id) = prepared.codex_lease_id {
        let lease_state = state.clone();
        let accepted = accepted.clone();
        state.inner.gateway.spawn_background(
            format!("web-turn-completion:{response_turn_id}"),
            async move {
                let _ = accepted.wait().await;
                lease_state
                    .inner
                    .codex_capability_broker
                    .release_turn_lease(&lease_id)
                    .await;
            },
        );
    }
    gateway_profile_mark(
        "turn_start_accepted",
        Some(&response_turn_id),
        Some(&response_thread_id),
        GatewayProfileFields {
            request_method: Some("turn/start"),
            runtime_source: Some("web"),
            ..GatewayProfileFields::default()
        },
    );
    Ok(wire::thread_command_turn::TurnStartResult {
        accepted: true,
        thread_id: response_thread_id.clone(),
        turn_id: response_turn_id,
        thread: wire::source::GatewayThread {
            id: response_thread_id,
            backend: wire::source::GatewayBackendInfo {
                kind: response_backend_kind,
                runtime_ref: Some(runtime_profile_ref),
                native_id: None,
            },
            source_key: Some(scope.source.source_key()),
            forked_from_thread_id: None,
        },
    })
}

pub(super) struct RoutedThreadTurn {
    pub(super) thread_id: Option<String>,
    pub(super) context: wire::agents_backend_rpc::ThreadContextReadResult,
    pub(super) control_values: BTreeMap<String, String>,
    pub(super) initial_thread_preferences: BTreeMap<String, String>,
    pub(super) input: Vec<GatewayInputPart>,
    pub(super) mentions: Vec<wire::source::GatewayMention>,
    pub(super) turn_overrides: BTreeMap<String, Value>,
    pub(super) runtime_source: String,
    pub(super) continue_sources: Vec<String>,
    pub(super) event_sink: Option<GatewayEventEmitter>,
    pub(super) workspace_mutations: Option<WorkspaceMutationSink>,
    pub(super) lineage: Option<Value>,
    pub(super) source: Option<GatewaySource>,
    pub(super) bind_source: Option<GatewaySource>,
    pub(super) turn_id: Option<String>,
}

pub(super) fn source_draft_control_values(
    context: &wire::agents_backend_rpc::ThreadContextReadResult,
) -> psychevo::Result<BTreeMap<String, String>> {
    context
        .controls
        .iter()
        .filter(|control| {
            control.effective_source
                == wire::agents_backend_rpc::ThreadControlEffectiveSourceView::SourceDraft
        })
        .filter_map(|control| {
            control
                .effective_value
                .as_ref()
                .map(|value| (control.id.clone(), value))
        })
        .map(|(control_id, value)| {
            thread_control_override_string_value(value).map(|value| (control_id, value))
        })
        .collect()
}

/// Delivers one turn for an internal source broker through the same target,
/// descriptor, control-precedence, and Adapter boundary as public turn/start.
pub(super) async fn run_routed_turn(
    state: &WebState,
    scope: &ResolvedScope,
    mut request: RoutedThreadTurn,
) -> psychevo::Result<GatewayTurnResult> {
    let creates_thread = request.thread_id.is_none();
    if creates_thread {
        request.thread_id = Some(Uuid::now_v7().to_string());
    }
    let prepared = prepare_routed_turn(state, scope, request).await?;
    let thread_id = prepared
        .intent
        .thread_id
        .clone()
        .expect("prepared routed Turn has a Thread identity");
    let initial_preferences = prepared.intent.policy.initial_thread_preferences.clone();
    let initial_source = prepared.initial_source.clone();
    let captured_runtime_ref = prepared.intent.policy.runtime_profile_ref.clone();
    let submission = prepared.intent.into_framework_request(prepared.caller)?;
    let observers = submission.observers;
    let handle = if creates_thread {
        let durable_source = initial_source.as_ref().and_then(|source| {
            (source.lifetime == GatewaySourceLifetime::Persistent).then(|| {
                psychevo::InitialThreadSourceAssociation {
                    source_key: source.source_key().0,
                    source_kind: source.kind.clone(),
                    raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                    visible_name: source.visible_name.clone(),
                    lineage: prepared.lineage.clone(),
                }
            })
        });
        let preferences = initial_preferences
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect();
        let mut start = psychevo::StartThreadRequest::new(&scope.cwd);
        start.source = submission.request.source().to_string();
        start.metadata = prepared.lineage.clone();
        state
            .inner
            .framework
            .start_thread_with_turn(
                start
                    .with_execution_source_key(prepared.execution_source_key.clone())
                    .with_initial_context(thread_id.clone(), durable_source, preferences),
                submission.request,
            )
            .await?
    } else {
        state
            .inner
            .framework
            .resume_thread(&submission.thread_id)
            .await?
            .start_turn(submission.request)
            .await?
    };
    observers.attach(&state.inner.gateway, handle.clone());
    if creates_thread
        && let Some(source) = initial_source
        && source.lifetime != GatewaySourceLifetime::Persistent
        && source.lifetime != GatewaySourceLifetime::Invocation
    {
        state
            .inner
            .gateway
            .bind_source_thread(
                &source,
                &thread_id,
                &GatewayBackendInfo {
                    kind: BackendKind::Native,
                    runtime_ref: captured_runtime_ref,
                    native_id: None,
                },
                prepared.lineage.clone(),
            )
            .await?;
    }
    let receipt = handle.receipt().clone();
    let result = handle
        .wait()
        .await
        .map(|result| framework_gateway_turn_result(receipt, result));
    if let Some(lease_id) = prepared.codex_lease_id.as_deref() {
        state
            .inner
            .codex_capability_broker
            .release_turn_lease(lease_id)
            .await;
    }
    result
}

pub(super) fn framework_gateway_turn_result(
    receipt: psychevo::TurnReceipt,
    result: psychevo::TurnResult,
) -> GatewayTurnResult {
    let (outcome, status) = match result.outcome {
        psychevo::TurnOutcome::Completed => (
            psychevo::application::Outcome::Normal,
            GatewayTurnStatus::Completed,
        ),
        psychevo::TurnOutcome::Stopped => (
            psychevo::application::Outcome::Stopped,
            GatewayTurnStatus::Interrupted,
        ),
        psychevo::TurnOutcome::Failed => (
            psychevo::application::Outcome::Failed,
            GatewayTurnStatus::Failed,
        ),
        psychevo::TurnOutcome::Interrupted => (
            psychevo::application::Outcome::Aborted,
            GatewayTurnStatus::Interrupted,
        ),
    };
    GatewayTurnResult {
        thread: GatewayThread {
            id: receipt.thread_id.clone(),
            backend: GatewayBackendInfo {
                kind: BackendKind::Native,
                runtime_ref: None,
                native_id: None,
            },
            source_key: None,
            forked_from_thread_id: None,
        },
        turn: psychevo_gateway_protocol::source::GatewayTurn {
            id: receipt.turn_id,
            thread_id: Some(receipt.thread_id),
            status,
            outcome: Some(outcome.as_str().to_string()),
            error: None,
            started_at_ms: None,
            completed_at_ms: Some(gateway_now_ms()),
        },
        result,
        committed_entries: Vec::new(),
    }
}

struct PreparedRoutedTurn {
    caller: ThreadCallerContext,
    intent: ThreadTurnIntent,
    codex_lease_id: Option<String>,
    initial_source: Option<GatewaySource>,
    execution_source_key: Option<String>,
    lineage: Option<Value>,
}

async fn prepare_routed_turn(
    state: &WebState,
    scope: &ResolvedScope,
    request: RoutedThreadTurn,
) -> psychevo::Result<PreparedRoutedTurn> {
    let initial_source = request
        .bind_source
        .clone()
        .or_else(|| request.source.clone());
    let execution_source_key = request
        .source
        .as_ref()
        .map(|source| source.source_key().0)
        .filter(|source_key| {
            initial_source
                .as_ref()
                .map(|source| source.source_key().0.as_str() != source_key)
                .unwrap_or(true)
        });
    let lineage = request.lineage.clone();
    let context = request.context;
    let selected_target_id = selected_context_target_id(&context)?.to_string();
    let target = context
        .compatible_targets
        .iter()
        .find(|target| target.target_id == selected_target_id)
        .cloned()
        .or_else(|| {
            context
                .binding
                .as_ref()
                .map(|binding| wire::agents_backend_rpc::RunnableTargetView {
                    target_id: selected_target_id,
                    agent_ref: binding.agent_ref.clone(),
                    runtime_profile_ref: context.runtime_profile_ref.clone(),
                    agent_label: binding
                        .agent_ref
                        .clone()
                        .unwrap_or_else(|| "Psychevo".to_string()),
                    profile_label: context.runtime_profile_ref.clone(),
                    label: context.runtime_profile_ref.clone(),
                    ready: context.sendability.allowed,
                    unavailable_reason: context.sendability.reason.clone(),
                })
        })
        .ok_or_else(|| {
            agent_session_error(
                "target_not_found",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "The selected Agent target is no longer present in Thread Context.",
                None,
            )
        })?;
    validate_turn_admission(
        &context,
        &request.input,
        &request.mentions,
        &request.turn_overrides,
    )?;
    if context.binding.is_none() {
        ensure_turn_runtime_profile_supported(state, scope, Some(&target.runtime_profile_ref))?;
    }
    let (mut caller, mut intent) =
        state.thread_turn_request(scope.cwd.clone(), request.thread_id.clone(), request.input);
    intent.policy.runtime_profile_ref = Some(target.runtime_profile_ref);
    intent.policy.agent_ref = target.agent_ref;
    intent.policy.control_values = request.control_values;
    intent.policy.initial_thread_preferences = request.initial_thread_preferences;
    for (control_id, value) in &request.turn_overrides {
        intent.policy.control_values.insert(
            control_id.clone(),
            thread_control_override_string_value(value)?,
        );
    }
    apply_mentions_to_turn_policy(&mut intent.policy, &request.mentions)?;
    intent.source = request.source;
    caller.surface = if request.runtime_source == "web" {
        ThreadSurface::Web
    } else if request.runtime_source.starts_with("channel/") {
        ThreadSurface::Channel
    } else if request.runtime_source == "automation" {
        ThreadSurface::Automation
    } else {
        ThreadSurface::Other(request.runtime_source.clone())
    };
    caller.runtime_source = request.runtime_source;
    caller.continue_sources = request.continue_sources;
    let event_sink = request.event_sink.clone();
    if let Some(event_sink) = event_sink.clone() {
        caller.set_event_observer(event_sink);
    }
    if let Some(workspace_mutations) = request.workspace_mutations {
        caller.set_workspace_mutations(workspace_mutations);
    }
    intent.turn_id = Some(
        request
            .turn_id
            .unwrap_or_else(|| Uuid::now_v7().to_string()),
    );
    let mut codex_lease_id = None;
    if let Some(thread_id) = intent.thread_id.clone() {
        match state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &scope.cwd,
                &thread_id,
                intent.turn_id.clone(),
                event_sink.clone(),
            )
            .await
        {
            Ok(contributions) => {
                codex_lease_id = contributions.lease_id;
                intent
                    .policy
                    .selected_capability_roots
                    .extend(contributions.capability_roots);
                caller.extend_runtime_tools(contributions.runtime_tools);
            }
            Err(err) => {
                eprintln!(
                    "{}",
                    json!({
                        "target": "psychevo.codex_plugins",
                        "event": "turn_snapshot_failed",
                        "cwd": scope.cwd,
                        "reason": err.to_string(),
                    })
                );
            }
        }
    }
    Ok(PreparedRoutedTurn {
        caller,
        intent,
        codex_lease_id,
        initial_source,
        execution_source_key,
        lineage,
    })
}

pub(super) async fn action_descriptors(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    supported_actions: &[wire::thread_command_turn::ThreadActionKind],
    selected_ready: bool,
    stability: Option<wire::agents_backend_rpc::RuntimeStabilityView>,
) -> psychevo::Result<Vec<wire::agents_backend_rpc::ThreadActionDescriptorView>> {
    let Some(thread_id) = thread_id else {
        return Ok(Vec::new());
    };
    let activity = snapshot_activity(state, &scope.source, Some(thread_id)).await?;
    let active = activity.running || activity.queued_turns > 0;
    let binding = state
        .inner
        .gateway
        .framework_agent_binding(thread_id)
        .await?;
    let acp = binding
        .as_ref()
        .is_some_and(|binding| binding.backend_kind == "acp");
    let native_history = state
        .inner
        .gateway
        .native_history_actions(thread_id, HistoryEditingSurface::Workbench)
        .await?;
    let staged = native_history.staged.is_some();
    let native_history_reason = native_history.unavailable_reason;
    let stability = stability.unwrap_or(wire::agents_backend_rpc::RuntimeStabilityView::Stable);
    let descriptor =
        |id, label: &str, enabled: bool, channel_safe: bool, unavailable_reason: Option<String>| {
            wire::agents_backend_rpc::ThreadActionDescriptorView {
                id,
                label: label.to_string(),
                enabled,
                stability,
                channel_safe,
                unavailable_reason,
            }
        };
    let inactive_reason = || Some("No turn is currently running on this Thread.".to_string());
    let actions = supported_actions
        .iter()
        .map(|action| match action {
            wire::thread_command_turn::ThreadActionKind::Interrupt => descriptor(
                *action,
                "Interrupt",
                active,
                true,
                (!active).then(inactive_reason).flatten(),
            ),
            wire::thread_command_turn::ThreadActionKind::Steer => {
                let enabled = activity.activities.iter().any(|activity| {
                    matches!(
                        activity,
                        wire::events_transcript::ThreadActivityView::FrameworkTurn { .. }
                            | wire::events_transcript::ThreadActivityView::Foreign { .. }
                    )
                });
                descriptor(
                    *action,
                    "Steer",
                    enabled,
                    true,
                    (!enabled).then(inactive_reason).flatten(),
                )
            }
            wire::thread_command_turn::ThreadActionKind::Compact => descriptor(
                *action,
                "Compact context",
                selected_ready,
                true,
                (!selected_ready)
                    .then(|| "This Agent target is currently unavailable.".to_string()),
            ),
            wire::thread_command_turn::ThreadActionKind::Fork => {
                let unavailable_reason = (!selected_ready)
                    .then(|| "This Agent target is currently unavailable.".to_string())
                    .or_else(|| active.then(|| "A running Thread cannot be forked.".to_string()))
                    .or_else(|| {
                        staged.then(|| {
                            "Run, restore, or redo the staged history state before forking."
                                .to_string()
                        })
                    })
                    .or_else(|| (!acp).then(|| native_history_reason.clone()).flatten());
                descriptor(
                    *action,
                    "Fork session",
                    unavailable_reason.is_none(),
                    false,
                    unavailable_reason,
                )
            }
            wire::thread_command_turn::ThreadActionKind::ForkBefore => {
                let unavailable_reason = (!selected_ready)
                    .then(|| "This Agent target is currently unavailable.".to_string())
                    .or_else(|| active.then(|| "A running Thread cannot be forked.".to_string()))
                    .or_else(|| {
                        staged.then(|| {
                            "Run, restore, or redo the staged history state before forking."
                                .to_string()
                        })
                    })
                    .or_else(|| native_history_reason.clone());
                descriptor(
                    *action,
                    "Fork before message",
                    unavailable_reason.is_none(),
                    false,
                    unavailable_reason,
                )
            }
            wire::thread_command_turn::ThreadActionKind::RevertConversation => {
                let unavailable_reason = (!selected_ready)
                    .then(|| "This Agent target is currently unavailable.".to_string())
                    .or_else(|| active.then(|| "A running Thread cannot be edited.".to_string()))
                    .or_else(|| native_history_reason.clone());
                descriptor(
                    *action,
                    "Edit message",
                    unavailable_reason.is_none(),
                    false,
                    unavailable_reason,
                )
            }
            wire::thread_command_turn::ThreadActionKind::UnrevertConversation => {
                let enabled = !active
                    && native_history.staged.as_ref().is_some_and(|staged| {
                        staged.kind
                            == wire::events_transcript::ThreadHistoryEditingKind::ConversationEdit
                    });
                descriptor(
                    *action,
                    "Restore history",
                    enabled,
                    false,
                    (!enabled).then(|| {
                        if active {
                            "A running Thread cannot restore history.".to_string()
                        } else {
                            "No conversation edit is staged.".to_string()
                        }
                    }),
                )
            }
        })
        .collect();
    Ok(actions)
}

pub(super) async fn pending_interactions(
    state: &WebState,
    _scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo::Result<Vec<PendingActionView>> {
    let Some(thread_id) = thread_id else {
        return Ok(Vec::new());
    };
    let selector = GatewayThreadSelector::thread_id(thread_id);
    Ok(prune_pending_actions(state, &selector, Some(thread_id))
        .await?
        .into_iter()
        .filter(|action| {
            matches!(
                action.kind,
                GatewayActionKind::Permission | GatewayActionKind::Clarify
            )
        })
        .collect())
}

pub(super) async fn authoritative_history_view(
    state: &WebState,
    thread_id: Option<&str>,
) -> psychevo::Result<wire::events_transcript::ThreadHistoryView> {
    cached_thread_history_descriptor(state, thread_id).await
}

pub(super) async fn authoritative_history_projection(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
) -> psychevo::Result<BoundedTranscriptPage> {
    let activity = snapshot_activity(state, &scope.source, Some(thread_id)).await?;
    let mut page = state
        .inner
        .gateway
        .thread_transcript_page(thread_id, None, 100)
        .await?;
    if let Some((turn_id, first_committed_seq)) =
        active_turn_projection_window(state, thread_id, &activity).await?
    {
        transcript::stamp_committed_entries_for_turn_window(
            &mut page.entries,
            transcript::TurnProjectionWindow {
                turn_id: &turn_id,
                first_committed_seq,
            },
        );
    }
    replay_running_live_transcript_overlay(state, thread_id, &activity, &mut page.entries).await?;
    Ok(page)
}

pub(super) async fn read_history(
    state: &WebState,
    auth: &AuthContext,
    requested_scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadHistoryReadParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryReadResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolved_scope_for_thread(state, &params.thread_id).await?;
    if scope.cwd != requested_scope.cwd {
        return Err(agent_session_error(
            "thread_scope_mismatch",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "The requested Thread does not belong to this workspace scope.",
            Some(format!("thread:{}", params.thread_id)),
        ));
    }
    let context = thread_context_read_result_live(
        state,
        &scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: Some(params.thread_id.clone()),
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    let limit = params.limit.unwrap_or(100).clamp(1, 200);
    let before = params.cursor.as_deref();
    let mut page = state
        .inner
        .gateway
        .thread_transcript_page(&params.thread_id, before, limit)
        .await?;
    if before.is_none() {
        let activity = snapshot_activity(state, &scope.source, Some(&params.thread_id)).await?;
        if let Some((turn_id, first_committed_seq)) =
            active_turn_projection_window(state, &params.thread_id, &activity).await?
        {
            transcript::stamp_committed_entries_for_turn_window(
                &mut page.entries,
                transcript::TurnProjectionWindow {
                    turn_id: &turn_id,
                    first_committed_seq,
                },
            );
        }
        replay_running_live_transcript_overlay(
            state,
            &params.thread_id,
            &activity,
            &mut page.entries,
        )
        .await?;
    }
    let next_cursor = page.next_cursor;
    let mut history = context.history;
    history.cursor = next_cursor.clone();
    Ok(wire::thread_command_turn::ThreadHistoryReadResult {
        thread_id: params.thread_id,
        history,
        entries: page.entries,
        next_cursor,
    })
}

pub(super) async fn read_history_draft(
    state: &WebState,
    auth: &AuthContext,
    requested_scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadHistoryDraftReadParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryDraftReadResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolved_scope_for_thread(state, &params.thread_id).await?;
    if scope.cwd != requested_scope.cwd {
        return Err(agent_session_error(
            "thread_scope_mismatch",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "The requested Thread does not belong to this workspace scope.",
            Some(format!("thread:{}", params.thread_id)),
        ));
    }
    read_history_draft_for_scope(state, &scope, params).await
}

async fn read_history_draft_for_scope(
    state: &WebState,
    _scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadHistoryDraftReadParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryDraftReadResult> {
    state
        .inner
        .gateway
        .read_native_editable_draft(
            &params.thread_id,
            &params.message_id,
            HistoryEditingSurface::Workbench,
        )
        .await
}

pub(super) async fn run_action(
    state: &WebState,
    auth: &AuthContext,
    requested_scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadActionRunParams,
    out_tx: ConnectionSender,
) -> psychevo::Result<wire::thread_command_turn::ThreadActionRunResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolved_scope_for_thread(state, &params.thread_id).await?;
    if scope.cwd != requested_scope.cwd {
        return Err(agent_session_error(
            "thread_scope_mismatch",
            AgentErrorStage::Control,
            "user_action",
            "not_delivered",
            "The requested Thread does not belong to this workspace scope.",
            Some(format!("thread:{}", params.thread_id)),
        ));
    }
    run_routed_action(state, &scope, params, out_tx).await
}

/// Runs an action already authorized by an internal source broker. Public RPC
/// callers must use `run_action`; Channels use this seam only after resolving
/// their source lane to its authoritative public Thread.
pub(super) async fn run_routed_action(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadActionRunParams,
    out_tx: ConnectionSender,
) -> psychevo::Result<wire::thread_command_turn::ThreadActionRunResult> {
    let context = thread_context_read_result_live(
        state,
        scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: Some(params.thread_id.clone()),
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    let action_kind = params.action.kind();
    let descriptor = context
        .actions
        .iter()
        .find(|descriptor| descriptor.id == action_kind)
        .ok_or_else(|| {
            agent_session_error(
                "action_unsupported",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                "This Thread runtime does not support the requested action.",
                Some(format!("thread:{}", params.thread_id)),
            )
        })?;
    // A repeated conversation-edit request is an idempotent recovery path.
    // Keep the action disabled in presentation while letting the Gateway
    // mutation distinguish that exact retry from every conflicting mutation.
    if !descriptor.enabled
        && !matches!(
            &params.action,
            wire::thread_command_turn::ThreadActionInput::RevertConversation { .. }
        )
    {
        return Err(agent_session_error(
            "action_unavailable",
            AgentErrorStage::Control,
            "retry",
            "not_delivered",
            descriptor
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| "The requested action is temporarily unavailable.".to_string()),
            Some(format!("thread:{}", params.thread_id)),
        ));
    }
    let activity = snapshot_activity(state, &scope.source, Some(&params.thread_id)).await?;
    let control_owner = activity.activities.first().cloned();
    match params.action {
        wire::thread_command_turn::ThreadActionInput::Interrupt => {
            let selector = GatewayThreadSelector::thread_id(&params.thread_id);
            let (interrupted, cleared) = match control_owner {
                Some(wire::events_transcript::ThreadActivityView::FrameworkTurn { .. }) => {
                    let thread = state
                        .inner
                        .framework
                        .resume_thread(&params.thread_id)
                        .await?;
                    thread.interrupt()
                }
                Some(wire::events_transcript::ThreadActivityView::GatewayLocal {
                    activity_id,
                    ..
                }) => (
                    state.inner.gateway.interrupt_local_activity(&activity_id),
                    state.inner.gateway.clear_queue(selector),
                ),
                Some(wire::events_transcript::ThreadActivityView::Foreign { .. }) => {
                    (state.inner.gateway.interrupt_turn(selector).await, 0)
                }
                None => (false, 0),
            };
            Ok(
                wire::thread_command_turn::ThreadActionRunResult::Interrupt {
                    thread_id: params.thread_id,
                    interrupted,
                    cleared,
                },
            )
        }
        wire::thread_command_turn::ThreadActionInput::Steer {
            expected_turn_id,
            text,
        } => {
            if text.trim().is_empty() {
                return Err(agent_session_error(
                    "invalid_action",
                    AgentErrorStage::Control,
                    "user_action",
                    "not_delivered",
                    "Steer text must be non-empty.",
                    Some(format!("thread:{}", params.thread_id)),
                ));
            }
            let message = RuntimeMessage::User {
                content: vec![UserContentBlock::text(text.clone())],
                timestamp_ms: gateway_now_ms(),
            };
            let accepted = match control_owner {
                Some(wire::events_transcript::ThreadActivityView::FrameworkTurn {
                    turn_id,
                    ..
                }) if turn_id == expected_turn_id => {
                    match state
                        .inner
                        .framework
                        .resume_thread(&params.thread_id)
                        .await?
                        .steer(&expected_turn_id, text)
                    {
                        Ok(accepted) => accepted,
                        Err(psychevo::ControlInputError::Closed) => false,
                        Err(error) => return Err(control_input_application_error(error)),
                    }
                }
                Some(wire::events_transcript::ThreadActivityView::Foreign { .. }) => {
                    state
                        .inner
                        .gateway
                        .steer_turn(
                            GatewayThreadSelector::thread_id(&params.thread_id),
                            Some(&expected_turn_id),
                            message,
                        )
                        .await
                }
                _ => false,
            };
            Ok(wire::thread_command_turn::ThreadActionRunResult::Steer {
                thread_id: params.thread_id,
                accepted,
            })
        }
        wire::thread_command_turn::ThreadActionInput::Compact { instructions } => {
            let result = thread_compact_result_for_thread(
                state,
                scope,
                params.thread_id.clone(),
                instructions,
                context.runtime_profile_ref,
                out_tx,
            )
            .await?;
            Ok(wire::thread_command_turn::ThreadActionRunResult::Compact {
                thread_id: params.thread_id,
                result: Box::new(result),
            })
        }
        wire::thread_command_turn::ThreadActionInput::Fork => {
            let native = state
                .inner
                .gateway
                .framework_agent_binding(&params.thread_id)
                .await?
                .is_some_and(|binding| binding.backend_kind == "native");
            if native {
                fork_native_thread(state, scope, &params.thread_id, None).await
            } else {
                Box::pin(fork_acp_thread(state, scope, &params.thread_id)).await
            }
        }
        wire::thread_command_turn::ThreadActionInput::ForkBefore { message_id } => {
            let draft = read_history_draft_for_scope(
                state,
                scope,
                wire::thread_command_turn::ThreadHistoryDraftReadParams {
                    scope: scope.to_wire_scope(),
                    thread_id: params.thread_id.clone(),
                    message_id,
                },
            )
            .await?;
            let message_seq = editable_message_seq(&draft)?;
            fork_native_thread(state, scope, &params.thread_id, Some(message_seq)).await
        }
        wire::thread_command_turn::ThreadActionInput::RevertConversation { message_id, draft } => {
            let staged = state
                .inner
                .gateway
                .stage_native_conversation_edit(
                    &params.thread_id,
                    &message_id,
                    &draft,
                    HistoryEditingSurface::Workbench,
                )
                .await?;
            let no_op = !staged;
            Ok(
                wire::thread_command_turn::ThreadActionRunResult::RevertConversation {
                    thread_id: params.thread_id.clone(),
                    staged,
                    no_op,
                    snapshot: Box::new(typed_thread_snapshot(
                        thread_snapshot_live(state, scope, Some(&params.thread_id)).await?,
                    )?),
                },
            )
        }
        wire::thread_command_turn::ThreadActionInput::UnrevertConversation => {
            let draft = state
                .inner
                .gateway
                .restore_native_conversation_edit(
                    &params.thread_id,
                    HistoryEditingSurface::Workbench,
                )
                .await?;
            Ok(
                wire::thread_command_turn::ThreadActionRunResult::UnrevertConversation {
                    thread_id: params.thread_id.clone(),
                    draft,
                    snapshot: Box::new(typed_thread_snapshot(
                        thread_snapshot_live(state, scope, Some(&params.thread_id)).await?,
                    )?),
                },
            )
        }
    }
}

fn control_input_application_error(error: psychevo::ControlInputError) -> psychevo::Error {
    let data = match &error {
        psychevo::ControlInputError::CountLimit { limit } => json!({
            "kind": "control_input_overload",
            "resource": "count",
            "limit": limit,
        }),
        psychevo::ControlInputError::ByteLimit { limit } => json!({
            "kind": "control_input_overload",
            "resource": "bytes",
            "limit": limit,
        }),
        _ => json!({
            "kind": "control_input_rejected",
        }),
    };
    psychevo::Error::structured(error.to_string(), data)
}

/// Accepts a routed compaction at the Thread Application boundary and returns
/// a completion future only after the operation has entered the authoritative
/// per-Thread queue.
pub(super) async fn enqueue_routed_compact_action(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: String,
    instructions: Option<String>,
    out_tx: ConnectionSender,
) -> psychevo::Result<
    BoxFuture<'static, psychevo::Result<wire::thread_command_turn::ThreadActionRunResult>>,
> {
    let context = thread_context_read_result_live(
        state,
        scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: Some(thread_id.clone()),
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    let descriptor = context
        .actions
        .iter()
        .find(|descriptor| descriptor.id == wire::thread_command_turn::ThreadActionKind::Compact)
        .ok_or_else(|| {
            agent_session_error(
                "action_unsupported",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                "This Thread runtime does not support the requested action.",
                Some(format!("thread:{thread_id}")),
            )
        })?;
    if !descriptor.enabled {
        return Err(agent_session_error(
            "action_unavailable",
            AgentErrorStage::Control,
            "retry",
            "not_delivered",
            descriptor
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| "The requested action is temporarily unavailable.".to_string()),
            Some(format!("thread:{thread_id}")),
        ));
    }
    let completion = enqueue_thread_compact_result_for_thread(
        state,
        scope,
        thread_id.clone(),
        instructions,
        context.runtime_profile_ref,
        out_tx,
    )
    .await?;
    Ok(Box::pin(async move {
        Ok(wire::thread_command_turn::ThreadActionRunResult::Compact {
            thread_id,
            result: Box::new(completion.await?),
        })
    }))
}

fn editable_message_seq(
    draft: &wire::thread_command_turn::ThreadHistoryDraftReadResult,
) -> psychevo::Result<i64> {
    if let Some(reason) = &draft.unavailable_reason {
        return Err(agent_session_error(
            "history_message_unavailable",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            reason.clone(),
            Some(format!("thread:{}", draft.thread_id)),
        ));
    }
    draft.message_seq.ok_or_else(|| {
        agent_session_error(
            "history_message_unavailable",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "The selected message does not have a durable sequence.",
            Some(format!("thread:{}", draft.thread_id)),
        )
    })
}

pub(super) async fn respond_to_interaction(
    state: &WebState,
    auth: &AuthContext,
    requested_scope: &ResolvedScope,
    params: wire::thread_command_turn::ThreadInteractionRespondParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadInteractionRespondResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolved_scope_for_thread(state, &params.thread_id).await?;
    if scope.cwd != requested_scope.cwd {
        return Err(agent_session_error(
            "thread_scope_mismatch",
            AgentErrorStage::Interaction,
            "user_action",
            "not_delivered",
            "The requested interaction does not belong to this workspace scope.",
            Some(format!("thread:{}", params.thread_id)),
        ));
    }
    let pending = pending_interactions(state, &scope, Some(&params.thread_id)).await?;
    let action = pending
        .iter()
        .find(|action| action.action_id == params.interaction_id)
        .ok_or_else(|| {
            agent_session_error(
                "interaction_stale",
                AgentErrorStage::Interaction,
                "user_action",
                "not_delivered",
                "The interaction was already resolved, expired, or is not visible to this Thread.",
                Some(format!("interaction:{}", params.interaction_id)),
            )
        })?;
    respond_to_routed_interaction(
        state,
        &params.thread_id,
        &params.interaction_id,
        action.kind,
        params.response,
    )
    .await
}

/// Resolves a typed interaction already authorized by an internal broker such
/// as the Channel token router. Public RPC callers must go through
/// `respond_to_interaction`, which first proves projection visibility.
pub(super) async fn respond_to_routed_interaction(
    state: &WebState,
    thread_id: &str,
    interaction_id: &str,
    expected_kind: GatewayActionKind,
    response: wire::thread_command_turn::ThreadInteractionResponse,
) -> psychevo::Result<wire::thread_command_turn::ThreadInteractionRespondResult> {
    respond_to_routed_interaction_for_selector(
        state,
        GatewayThreadSelector::thread_id(thread_id),
        interaction_id,
        expected_kind,
        response,
    )
    .await
}

/// Resolves an interaction through an internal broker-owned selector. Channel
/// tokens are source-scoped, so their authoritative queue selector may remain
/// the source alias while the public action already carries its bound Thread.
pub(super) async fn respond_to_routed_interaction_for_selector(
    state: &WebState,
    selector: GatewayThreadSelector,
    interaction_id: &str,
    expected_kind: GatewayActionKind,
    response: wire::thread_command_turn::ThreadInteractionResponse,
) -> psychevo::Result<wire::thread_command_turn::ThreadInteractionRespondResult> {
    if expected_kind == GatewayActionKind::Clarify
        && let Some(result) = super::codex_capability_broker::respond_to_elicitation(
            state,
            interaction_id,
            response.clone(),
        )?
    {
        state.remove_pending_permission(interaction_id);
        return Ok(result);
    }
    let framework_thread_id = match &selector {
        GatewayThreadSelector::ThreadId { thread_id } => Some(thread_id.clone()),
        GatewayThreadSelector::Source { .. } => state
            .inner
            .pending_actions
            .lock()
            .expect("web pending actions poisoned")
            .get(interaction_id)
            .and_then(|action| action.thread_id.clone()),
    };
    if let Some(thread_id) = framework_thread_id
        && let Ok(thread) = state.inner.framework.resume_thread(&thread_id).await
    {
        let framework_response = match &response {
            wire::thread_command_turn::ThreadInteractionResponse::Permission {
                decision,
                directory,
            } => psychevo::InteractionResponse::Permission(permission_decision(
                *decision,
                directory.clone(),
            )),
            wire::thread_command_turn::ThreadInteractionResponse::Clarify { answers } => {
                psychevo::InteractionResponse::Clarify(answers.clone())
            }
            wire::thread_command_turn::ThreadInteractionResponse::CancelClarify => {
                psychevo::InteractionResponse::Cancel
            }
        };
        if thread
            .respond(interaction_id, framework_response)
            .await?
            .accepted
        {
            state.remove_pending_permission(interaction_id);
            return Ok(wire::thread_command_turn::ThreadInteractionRespondResult {
                accepted: true,
                interaction_id: interaction_id.to_string(),
                outcome: interaction_response_outcome(expected_kind, &response),
            });
        }
    }
    let (accepted, outcome) = match (expected_kind, response) {
        (
            GatewayActionKind::Permission,
            wire::thread_command_turn::ThreadInteractionResponse::Permission {
                decision,
                directory,
            },
        ) => {
            let outcome = if decision == PermissionDecision::Deny {
                GatewayActionOutcome::Rejected
            } else {
                GatewayActionOutcome::Accepted
            };
            (
                state
                    .inner
                    .gateway
                    .submit_permission(
                        selector,
                        interaction_id,
                        permission_decision(decision, directory),
                    )
                    .await,
                outcome,
            )
        }
        (
            GatewayActionKind::Clarify,
            wire::thread_command_turn::ThreadInteractionResponse::Clarify { answers },
        ) => (
            state
                .inner
                .gateway
                .submit_clarify(
                    selector,
                    interaction_id,
                    ClarifyResult::Answered(ClarifyResponse {
                        answers: answers
                            .into_iter()
                            .map(|answers| ClarifyAnswer { answers })
                            .collect(),
                    }),
                )
                .await,
            GatewayActionOutcome::Accepted,
        ),
        (
            GatewayActionKind::Clarify,
            wire::thread_command_turn::ThreadInteractionResponse::CancelClarify,
        ) => (
            state
                .inner
                .gateway
                .submit_clarify(selector, interaction_id, ClarifyResult::Cancelled)
                .await,
            GatewayActionOutcome::Cancelled,
        ),
        _ => {
            return Err(agent_session_error(
                "interaction_kind_mismatch",
                AgentErrorStage::Interaction,
                "user_action",
                "not_delivered",
                "The interaction response kind does not match the pending request.",
                Some(format!("interaction:{interaction_id}")),
            ));
        }
    };
    if !accepted {
        state.remove_pending_permission(interaction_id);
        return Err(agent_session_error(
            "interaction_stale",
            AgentErrorStage::Interaction,
            "user_action",
            "not_delivered",
            "The interaction was already resolved or expired.",
            Some(format!("interaction:{interaction_id}")),
        ));
    }
    // A successful response is accepted exactly once. Removing the public
    // projection only after the underlying responder accepts makes retries
    // fail closed instead of acknowledging the same interaction twice.
    state.remove_pending_permission(interaction_id);
    Ok(wire::thread_command_turn::ThreadInteractionRespondResult {
        accepted: true,
        interaction_id: interaction_id.to_string(),
        outcome,
    })
}

fn interaction_response_outcome(
    kind: GatewayActionKind,
    response: &wire::thread_command_turn::ThreadInteractionResponse,
) -> GatewayActionOutcome {
    match (kind, response) {
        (
            GatewayActionKind::Permission,
            wire::thread_command_turn::ThreadInteractionResponse::Permission {
                decision: PermissionDecision::Deny,
                ..
            },
        ) => GatewayActionOutcome::Rejected,
        (_, wire::thread_command_turn::ThreadInteractionResponse::CancelClarify) => {
            GatewayActionOutcome::Cancelled
        }
        _ => GatewayActionOutcome::Accepted,
    }
}

pub(super) async fn validate_turn_revisions(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    target: Option<wire::thread_command_turn::RunnableTargetInput>,
    expected_context_revision: Option<&str>,
    expected_control_revision: Option<&str>,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadContextReadResult> {
    let require = |value: Option<&str>, name: &str| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                agent_session_error(
                    "revision_required",
                    AgentErrorStage::Control,
                    "user_action",
                    "not_delivered",
                    format!("turn/start requires a non-empty `{name}` from Thread Context."),
                    thread_id
                        .as_ref()
                        .map(|thread_id| format!("thread:{thread_id}")),
                )
            })
    };
    let expected_context_revision = require(expected_context_revision, "expectedContextRevision")?;
    let expected_control_revision = require(expected_control_revision, "expectedControlRevision")?;
    // Compare against the same negotiated live Thread Context returned to the
    // caller. A base-only revision would make every bound ACP thread stale
    // because its public revision also includes the resident session snapshot.
    let context = thread_context_read_result_live(
        state,
        scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: thread_id.clone(),
            target,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    if context.context_revision != expected_context_revision
        || context.control_revision != expected_control_revision
    {
        return Err(agent_session_error(
            "stale_revision",
            AgentErrorStage::Control,
            "user_action",
            "not_delivered",
            "Thread Context changed; refresh it before starting the turn.",
            thread_id.map(|thread_id| format!("thread:{thread_id}")),
        ));
    }
    Ok(context)
}

pub(super) fn validate_turn_admission(
    context: &wire::agents_backend_rpc::ThreadContextReadResult,
    input: &[wire::source::GatewayInputPart],
    mentions: &[wire::source::GatewayMention],
    turn_overrides: &BTreeMap<String, Value>,
) -> psychevo::Result<()> {
    let required_controls_satisfied_by_turn = context
        .controls
        .iter()
        .filter(|control| control.required)
        .all(|control| {
            control.enabled
                && (control.effective_value.is_some() || turn_overrides.contains_key(&control.id))
        });
    let recoverable_required_control_draft =
        context.sendability.recovery_action.is_none() && required_controls_satisfied_by_turn;
    if !context.sendability.allowed && !recoverable_required_control_draft {
        return Err(agent_session_error(
            "target_not_sendable",
            AgentErrorStage::Delivery,
            "user_action",
            "not_delivered",
            context
                .sendability
                .reason
                .clone()
                .unwrap_or_else(|| "This Agent target cannot accept a turn.".to_string()),
            None,
        ));
    }
    for part in input {
        let kind = match part {
            wire::source::GatewayInputPart::Text { .. } => "text",
            wire::source::GatewayInputPart::Image { .. } => "image",
            wire::source::GatewayInputPart::Resource { .. } => "resource",
            wire::source::GatewayInputPart::ResourceLink { .. } => "resourceLink",
            wire::source::GatewayInputPart::Context { .. } => "embeddedContext",
        };
        require_input_capability(context, kind)?;
    }
    if mentions.iter().any(|mention| {
        matches!(
            mention.target,
            wire::source::GatewayMentionTarget::Agent { .. }
        )
    }) {
        require_input_capability(context, "agentMention")?;
    }
    for (control_id, value) in turn_overrides {
        let control = context
            .controls
            .iter()
            .find(|control| control.id == *control_id)
            .ok_or_else(|| {
                agent_session_error(
                    "control_not_found",
                    AgentErrorStage::Control,
                    "user_action",
                    "not_delivered",
                    format!("This Agent target does not expose control `{control_id}`."),
                    None,
                )
            })?;
        if !control.enabled {
            return Err(agent_session_error(
                "control_unavailable",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                control
                    .unavailable_reason
                    .clone()
                    .unwrap_or_else(|| format!("Control `{control_id}` is unavailable.")),
                None,
            ));
        }
        if !control.choices.is_empty()
            && !control.choices.iter().any(|choice| choice.value == *value)
        {
            return Err(agent_session_error(
                "invalid_control",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                format!("Control `{control_id}` does not accept the requested value."),
                None,
            ));
        }
    }
    for control in context.controls.iter().filter(|control| control.required) {
        if !control.enabled {
            return Err(agent_session_error(
                "required_control_unavailable",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                control.unavailable_reason.clone().unwrap_or_else(|| {
                    format!("Required control `{}` is unavailable.", control.id)
                }),
                None,
            ));
        }
        if turn_overrides.get(&control.id).is_none() && control.effective_value.is_none() {
            return Err(agent_session_error(
                "required_control_missing",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                format!("{} is required before starting a turn.", control.label),
                None,
            ));
        }
    }
    Ok(())
}

fn require_input_capability(
    context: &wire::agents_backend_rpc::ThreadContextReadResult,
    kind: &str,
) -> psychevo::Result<()> {
    let capability = context
        .input_capabilities
        .iter()
        .find(|capability| capability.kind == kind);
    if capability.is_some_and(|capability| capability.enabled) {
        return Ok(());
    }
    Err(agent_session_error(
        "unsupported_input",
        AgentErrorStage::Delivery,
        "user_action",
        "not_delivered",
        capability
            .and_then(|capability| capability.unavailable_reason.clone())
            .unwrap_or_else(|| {
                format!("Input capability `{kind}` is unavailable for this Agent target.")
            }),
        None,
    ))
}
