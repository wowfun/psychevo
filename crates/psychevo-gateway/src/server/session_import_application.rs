use super::*;
use crate::{
    ResolvedPeerTurn, agent_session_configuration_error, ensure_gateway_runtime_binding,
    resolve_gateway_agent_binding_snapshot, runtime_profile_config_fingerprint,
    runtime_profile_config_revision,
};
use futures::{StreamExt, stream};
use psychevo_runtime::agents::AgentEntrypoint;

const IMPORT_DISCOVERY_CONCURRENCY: usize = 4;
const IMPORT_STATE_METADATA_KEY: &str = "agentSessionImportState";
const LIFECYCLE_METADATA_KEY: &str = "agentSessionLifecycle";
const DELETE_INTENT_METADATA_KEY: &str = "agentSessionDeleteIntent";

pub(super) async fn list(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadImportListParams,
) -> psychevo_runtime::Result<wire::ThreadImportListResult> {
    let scope = resolve_optional_scope(state, auth, Some(params.scope.clone()))?;
    let requested_cursors = params.cursors;
    let jobs = runtime_profiles::importable_acp_profiles(state, &scope)?;
    let mut profiles = stream::iter(jobs)
        .map(|profile| {
            let state = state.clone();
            let scope = scope.clone();
            let public_cursor = requested_cursors.get(&profile.config.id).cloned();
            async move { discover_profile_sessions(&state, &scope, profile, public_cursor).await }
        })
        .buffer_unordered(IMPORT_DISCOVERY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    profiles.sort_by(|left, right| left.profile_label.cmp(&right.profile_label));
    Ok(wire::ThreadImportListResult { profiles })
}

async fn discover_profile_sessions(
    state: &WebState,
    scope: &ResolvedScope,
    profile: runtime_profiles::ImportableAcpProfile,
    public_cursor: Option<String>,
) -> wire::ThreadImportProfileView {
    let runtime_profile_ref = profile.config.id.clone();
    let profile_label = profile.view.label.clone();
    let targets = profile.targets.clone();
    let result = Box::pin(async {
        if !profile.view.enabled || !profile.targets.iter().any(|target| target.ready) {
            return Err(agent_session_error(
                "target_unavailable",
                AgentErrorStage::Configuration,
                "user_action",
                "not_delivered",
                profile.view.health.summary.clone(),
                None,
            ));
        }
        let agent_ref = profile
            .targets
            .iter()
            .find(|target| target.ready)
            .and_then(|target| target.agent_ref.as_deref());
        let peer = runtime_profiles::resolve_runtime_target_peer_turn(
            state,
            scope,
            &runtime_profile_ref,
            agent_ref,
        )?
        .ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Runtime Profile `{runtime_profile_ref}` does not resolve an ACP Agent."
            ))
        })?;
        let cursor = resolve_import_cursor(state, &runtime_profile_ref, public_cursor.as_deref())?;
        state
            .inner
            .gateway
            .discover_agent_sessions(profile.config.clone(), peer, scope.cwd.clone(), cursor)
            .await
    })
    .await;

    match result {
        Ok(page) => {
            let mut sessions = Vec::new();
            let mut already_imported_count = 0;
            for session in page.sessions {
                match state.inner.state.gateway_runtime_binding_by_native_session(
                    &runtime_profile_ref,
                    &session.native_session_id,
                ) {
                    Ok(Some(_)) => {
                        already_imported_count += 1;
                        continue;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return import_profile_error(
                            runtime_profile_ref,
                            profile_label,
                            targets,
                            error,
                        );
                    }
                }
                let candidate_id = state
                    .inner
                    .agent_session_imports
                    .lock()
                    .expect("Agent session import registry poisoned")
                    .insert_candidate(
                        runtime_profile_ref.clone(),
                        session.cwd.clone(),
                        session.native_session_id,
                        session.title.clone(),
                    );
                sessions.push(wire::ThreadImportCandidateView {
                    candidate_id,
                    cwd: session.cwd.display().to_string(),
                    title: session.title,
                    updated_at: session.updated_at,
                });
            }
            let next_cursor = page.next_cursor.map(|cursor| {
                state
                    .inner
                    .agent_session_imports
                    .lock()
                    .expect("Agent session import registry poisoned")
                    .insert_cursor(runtime_profile_ref.clone(), cursor)
            });
            wire::ThreadImportProfileView {
                runtime_profile_ref,
                profile_label,
                targets,
                status: "ready".to_string(),
                sessions,
                next_cursor,
                already_imported_count,
                error: None,
            }
        }
        Err(error) => import_profile_error(runtime_profile_ref, profile_label, targets, error),
    }
}

fn import_profile_error(
    runtime_profile_ref: String,
    profile_label: String,
    targets: Vec<wire::RunnableTargetView>,
    error: Error,
) -> wire::ThreadImportProfileView {
    wire::ThreadImportProfileView {
        runtime_profile_ref,
        profile_label,
        targets,
        status: "error".to_string(),
        sessions: Vec::new(),
        next_cursor: None,
        already_imported_count: 0,
        error: Some(agent_error_view(error.to_string(), error.structured_data())),
    }
}

fn resolve_import_cursor(
    state: &WebState,
    runtime_profile_ref: &str,
    public_cursor: Option<&str>,
) -> psychevo_runtime::Result<Option<String>> {
    let Some(public_cursor) = public_cursor else {
        return Ok(None);
    };
    let now_ms = gateway_now_ms();
    let mut registry = state
        .inner
        .agent_session_imports
        .lock()
        .expect("Agent session import registry poisoned");
    registry.retain_live(now_ms);
    let cursor = registry.cursors.get(public_cursor).ok_or_else(|| {
        agent_session_error(
            "agent_session_cursor_expired",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "This Agent session page expired. Refresh sessions and try again.",
            None,
        )
    })?;
    if cursor.runtime_profile_ref != runtime_profile_ref {
        return Err(agent_session_error(
            "agent_session_cursor_mismatch",
            AgentErrorStage::History,
            "never",
            "not_delivered",
            "The Agent session cursor belongs to a different Runtime Profile.",
            None,
        ));
    }
    Ok(Some(cursor.cursor.clone()))
}

pub(super) async fn import(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadImportParams,
) -> psychevo_runtime::Result<wire::ThreadImportResult> {
    let scope = resolve_optional_scope(state, auth, Some(params.scope.clone()))?;
    let import_archived = params.archived;
    let candidate = take_import_candidate(state, &params.candidate_id)?;
    if candidate.cwd != scope.cwd {
        return Err(agent_session_error(
            "agent_session_candidate_scope_mismatch",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            "The selected Agent session belongs to another workspace.",
            None,
        ));
    }
    let target = runtime_profiles::runnable_target_by_id(state, &scope, &params.target_id)?;
    if target.runtime_profile_ref != candidate.runtime_profile_ref {
        return Err(agent_session_error(
            "agent_session_candidate_target_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "The selected Agent target does not own this import candidate.",
            None,
        ));
    }
    if let Some(existing) = state
        .inner
        .state
        .gateway_runtime_binding_by_native_session(
            &candidate.runtime_profile_ref,
            &candidate.native_session_id,
        )?
    {
        if import_archived {
            archive_thread(state, &existing.thread_id).await?;
        } else {
            restore_thread(state, &existing.thread_id).await?;
        }
        bind_source_to_thread(state, &scope, &existing.thread_id)?;
        return Ok(wire::ThreadImportResult {
            snapshot: Box::new(typed_thread_snapshot(
                thread_snapshot_live(state, &scope, Some(&existing.thread_id)).await?,
            )?),
        });
    }

    let profile = runtime_profiles::importable_acp_profiles(state, &scope)?
        .into_iter()
        .find(|profile| profile.config.id == candidate.runtime_profile_ref)
        .map(|profile| profile.config)
        .ok_or_else(|| {
            agent_session_configuration_error("The import Runtime Profile is no longer available.")
        })?;
    let peer = runtime_profiles::resolve_runtime_target_peer_turn(
        state,
        &scope,
        &candidate.runtime_profile_ref,
        target.agent_ref.as_deref(),
    )?
    .ok_or_else(|| agent_session_configuration_error("The selected ACP Agent is unavailable."))?;
    let thread_id = state.inner.state.create_session_with_metadata(
        &candidate.cwd,
        "web",
        "pending",
        "pending",
        Some(json!({IMPORT_STATE_METADATA_KEY: "pending"})),
    )?;
    let imported_native_session_id = candidate.native_session_id.clone();
    let result = async {
        let imported = import_agent_session_into_thread(
            state, &scope, &target, profile, peer, candidate, &thread_id,
        )
        .await?;
        if !import_archived {
            return Ok(imported);
        }
        archive_thread(state, &thread_id).await?;
        Ok(wire::ThreadImportResult {
            snapshot: Box::new(typed_thread_snapshot(
                thread_snapshot_live(state, &scope, Some(&thread_id)).await?,
            )?),
        })
    }
    .await;
    if result.is_err() {
        let _ = state
            .inner
            .gateway
            .release_imported_agent_session(thread_id.clone(), imported_native_session_id)
            .await;
        let _ = state.inner.state.delete_session(&thread_id);
    }
    result
}

async fn import_agent_session_into_thread(
    state: &WebState,
    scope: &ResolvedScope,
    target: &wire::RunnableTargetView,
    profile: RuntimeProfileConfig,
    peer: ResolvedPeerTurn,
    candidate: AgentSessionImportCandidate,
    thread_id: &str,
) -> psychevo_runtime::Result<wire::ThreadImportResult> {
    let mut options = state.run_options(candidate.cwd.clone(), Some(thread_id.to_string()));
    options.runtime_ref = Some(profile.id.clone());
    options.agent = target.agent_ref.clone();
    let loaded = state
        .inner
        .gateway
        .load_imported_agent_session(
            profile.clone(),
            peer.clone(),
            options.clone(),
            thread_id.to_string(),
            candidate.native_session_id.clone(),
        )
        .await?;
    let snapshot = loaded.snapshot;
    if snapshot.native_session_id != candidate.native_session_id {
        return Err(agent_session_error(
            "agent_session_load_identity_mismatch",
            AgentErrorStage::Binding,
            "never",
            "unknown",
            "The ACP Agent loaded a different native session than requested.",
            None,
        ));
    }
    crate::acp_peer::commit_imported_acp_replay(
        &state.inner.state,
        &peer,
        thread_id,
        &loaded.replay,
    )?;
    let agent =
        resolve_gateway_agent_binding_snapshot(&options, &profile, None, AgentEntrypoint::Peer)?;
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    let revision = runtime_profile_config_revision(&fingerprint);
    let binding = ensure_gateway_runtime_binding(
        &state.inner.state,
        thread_id,
        &agent,
        &profile,
        revision,
        &fingerprint,
    )?;
    state.inner.state.attach_gateway_runtime_native_session(
        thread_id,
        binding.binding_revision,
        &candidate.native_session_id,
    )?;
    state.inner.state.set_session_metadata_field(
        thread_id,
        ACP_PEER_METADATA_KEY,
        Some(crate::acp_peer::peer_session_metadata(
            &peer,
            Some(&candidate.native_session_id),
            None,
            &options.runtime_options,
            Some(&snapshot),
        )),
    )?;
    persist_lifecycle_projection(state, thread_id, target, &snapshot)?;
    if let Some(title) = candidate
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    {
        state.inner.state.set_session_title(thread_id, title)?;
    }
    state
        .inner
        .state
        .set_session_metadata_field(thread_id, IMPORT_STATE_METADATA_KEY, None)?;
    bind_source_to_thread(state, scope, thread_id)?;
    Ok(wire::ThreadImportResult {
        snapshot: Box::new(typed_thread_snapshot(
            thread_snapshot_live(state, scope, Some(thread_id)).await?,
        )?),
    })
}

fn take_import_candidate(
    state: &WebState,
    candidate_id: &str,
) -> psychevo_runtime::Result<AgentSessionImportCandidate> {
    let now_ms = gateway_now_ms();
    let mut registry = state
        .inner
        .agent_session_imports
        .lock()
        .expect("Agent session import registry poisoned");
    registry.retain_live(now_ms);
    registry
        .candidates
        .get(candidate_id)
        .cloned()
        .ok_or_else(|| {
            agent_session_error(
                "agent_session_candidate_expired",
                AgentErrorStage::History,
                "user_action",
                "not_delivered",
                "This Agent session candidate expired. Refresh sessions and try again.",
                None,
            )
        })
}

fn persist_lifecycle_projection(
    state: &WebState,
    thread_id: &str,
    target: &wire::RunnableTargetView,
    snapshot: &crate::acp_peer::AcpSessionSnapshot,
) -> psychevo_runtime::Result<()> {
    state.inner.state.set_session_metadata_field(
        thread_id,
        LIFECYCLE_METADATA_KEY,
        Some(json!({
            "targetLabel": target.label,
            "fork": snapshot.capabilities.session.fork,
            "delete": snapshot.capabilities.session.delete,
            "close": snapshot.capabilities.session.close,
            "resume": snapshot.capabilities.session.resume,
        })),
    )
}

pub(super) async fn fork_acp_thread(
    state: &WebState,
    scope: &ResolvedScope,
    source_thread_id: &str,
) -> psychevo_runtime::Result<wire::ThreadActionRunResult> {
    let binding = require_acp_binding(state, source_thread_id)?;
    let bound = runtime_profiles::resolve_bound_thread_agent_target(state, &binding)?;
    let context = runtime_profiles::thread_context_read_result_live(
        state,
        scope,
        wire::ThreadContextReadParams {
            thread_id: Some(source_thread_id.to_string()),
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    let target = bound_context_target(&context, source_thread_id)?;
    let peer = bound
        .peer
        .clone()
        .ok_or_else(|| agent_session_configuration_error("The source ACP Agent is unavailable."))?;
    let thread_id = state.inner.state.create_session_with_metadata(
        &scope.cwd,
        "web",
        "pending",
        "pending",
        Some(json!({
            IMPORT_STATE_METADATA_KEY: "pending",
            "forkedFromThreadId": source_thread_id,
        })),
    )?;
    let mut options = state.run_options(scope.cwd.clone(), Some(source_thread_id.to_string()));
    options.runtime_ref = binding.runtime_ref.clone();
    options.agent = binding.agent_ref.clone();
    let result = async {
        if state
            .inner
            .gateway
            .inspect_cached_bound_agent_session(
                source_thread_id.to_string(),
                binding.native_session_id.clone().unwrap_or_default(),
            )
            .await?
            .is_none()
        {
            state
                .inner
                .gateway
                .resume_bound_agent_session(
                    binding.clone(),
                    bound.profile.clone(),
                    peer.clone(),
                    options.clone(),
                )
                .await?;
        }
        let snapshot = state
            .inner
            .gateway
            .fork_bound_agent_session(
                binding.clone(),
                bound.profile.clone(),
                peer,
                options.clone(),
                thread_id.clone(),
            )
            .await?;
        let agent = resolve_gateway_agent_binding_snapshot(
            &options,
            &bound.profile,
            Some(&binding),
            AgentEntrypoint::Peer,
        )?;
        let fork_binding = ensure_gateway_runtime_binding(
            &state.inner.state,
            &thread_id,
            &agent,
            &bound.profile,
            bound.revision,
            &bound.fingerprint,
        )?;
        state.inner.state.attach_gateway_runtime_native_session(
            &thread_id,
            fork_binding.binding_revision,
            &snapshot.native_session_id,
        )?;
        persist_lifecycle_projection(state, &thread_id, &target, &snapshot)?;
        state.inner.state.set_session_metadata_field(
            &thread_id,
            IMPORT_STATE_METADATA_KEY,
            None,
        )?;
        bind_source_to_thread(state, scope, &thread_id)?;
        Ok(wire::ThreadActionRunResult::Fork {
            source_thread_id: source_thread_id.to_string(),
            snapshot: Box::new(typed_thread_snapshot(
                thread_snapshot_live(state, scope, Some(&thread_id)).await?,
            )?),
        })
    }
    .await;
    if result.is_err() {
        let _ = state.inner.state.delete_session(&thread_id);
    }
    result
}

pub(super) async fn fork_native_thread(
    state: &WebState,
    scope: &ResolvedScope,
    source_thread_id: &str,
    before_session_seq: Option<i64>,
) -> psychevo_runtime::Result<wire::ThreadActionRunResult> {
    let thread_id = crate::history_editing::fork_native_history(
        &state.inner.state,
        source_thread_id,
        before_session_seq,
        &scope.source.kind,
    )?;
    let result = async {
        bind_source_to_thread(state, scope, &thread_id)?;
        let snapshot = Box::new(typed_thread_snapshot(
            thread_snapshot_live(state, scope, Some(&thread_id)).await?,
        )?);
        Ok(if before_session_seq.is_some() {
            wire::ThreadActionRunResult::ForkBefore {
                source_thread_id: source_thread_id.to_string(),
                snapshot,
            }
        } else {
            wire::ThreadActionRunResult::Fork {
                source_thread_id: source_thread_id.to_string(),
                snapshot,
            }
        })
    }
    .await;
    if result.is_err() {
        let _ = state.inner.state.delete_session(&thread_id);
    }
    result
}

fn bound_context_target(
    context: &wire::ThreadContextReadResult,
    thread_id: &str,
) -> psychevo_runtime::Result<wire::RunnableTargetView> {
    let selected_target_id = selected_context_target_id(context)?;
    context
        .compatible_targets
        .iter()
        .find(|target| target.target_id == selected_target_id)
        .cloned()
        .ok_or_else(|| {
            agent_session_error(
                "bound_target_missing",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The captured Agent target is missing from bound Thread Context.",
                Some(format!("agent-binding:{thread_id}")),
            )
        })
}

pub(super) async fn archive_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<Value> {
    if let Some(binding) = state.inner.state.gateway_runtime_binding(thread_id)?
        && binding.backend_kind.as_deref() == Some("acp")
        && let Some(native_session_id) = binding.native_session_id.clone()
        && state
            .inner
            .gateway
            .inspect_cached_bound_agent_session(thread_id.to_string(), native_session_id)
            .await?
            .is_some()
    {
        let scope = resolved_scope_for_thread(state, thread_id)?;
        let bound = runtime_profiles::resolve_bound_thread_agent_target(state, &binding)?;
        let peer = bound.peer.clone().ok_or_else(|| {
            agent_session_configuration_error("The bound ACP Agent is unavailable.")
        })?;
        let cached = state
            .inner
            .gateway
            .inspect_cached_bound_agent_session(
                thread_id.to_string(),
                binding.native_session_id.clone().unwrap_or_default(),
            )
            .await?;
        if cached.is_some_and(|snapshot| snapshot.capabilities.session.close) {
            state
                .inner
                .gateway
                .close_bound_agent_session(
                    binding,
                    bound.profile,
                    peer,
                    state.run_options(scope.cwd, Some(thread_id.to_string())),
                )
                .await?;
        }
    }
    state.inner.state.archive_session(thread_id)?;
    state
        .inner
        .codex_capability_broker
        .archive_ephemeral_thread(thread_id)
        .await;
    session_summary_by_id(state, thread_id)
}

pub(super) async fn restore_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<Value> {
    if let Some(binding) = state.inner.state.gateway_runtime_binding(thread_id)?
        && binding.backend_kind.as_deref() == Some("acp")
    {
        let scope = resolved_scope_for_thread(state, thread_id)?;
        let bound = runtime_profiles::resolve_bound_thread_agent_target(state, &binding)?;
        let peer = bound.peer.clone().ok_or_else(|| {
            agent_session_configuration_error("The bound ACP Agent is unavailable.")
        })?;
        let context = runtime_profiles::thread_context_read_result(
            state,
            &scope,
            wire::ThreadContextReadParams {
                thread_id: Some(thread_id.to_string()),
                target: None,
                scope: Some(scope.to_wire_scope()),
            },
        )?;
        let target = bound_context_target(&context, thread_id)?;
        let snapshot = match state
            .inner
            .gateway
            .inspect_cached_bound_agent_session(
                thread_id.to_string(),
                binding.native_session_id.clone().unwrap_or_default(),
            )
            .await?
        {
            Some(snapshot) => snapshot,
            None => {
                state
                    .inner
                    .gateway
                    .resume_bound_agent_session(
                        binding,
                        bound.profile,
                        peer,
                        state.run_options(scope.cwd, Some(thread_id.to_string())),
                    )
                    .await?
            }
        };
        persist_lifecycle_projection(state, thread_id, &target, &snapshot)?;
    }
    state.inner.state.restore_session(thread_id)?;
    session_summary_by_id(state, thread_id)
}

pub(super) async fn delete_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    let Some(binding) = state.inner.state.gateway_runtime_binding(thread_id)? else {
        state.inner.state.delete_session(thread_id)?;
        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread(thread_id)
            .await;
        return Ok(());
    };
    if binding.backend_kind.as_deref() != Some("acp") {
        state.inner.state.delete_session(thread_id)?;
        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread(thread_id)
            .await;
        return Ok(());
    }
    let scope = resolved_scope_for_thread(state, thread_id)?;
    let bound = runtime_profiles::resolve_bound_thread_agent_target(state, &binding)?;
    let peer = bound
        .peer
        .clone()
        .ok_or_else(|| agent_session_configuration_error("The bound ACP Agent is unavailable."))?;
    let options = state.run_options(scope.cwd, Some(thread_id.to_string()));
    let snapshot = match state
        .inner
        .gateway
        .inspect_cached_bound_agent_session(
            thread_id.to_string(),
            binding.native_session_id.clone().unwrap_or_default(),
        )
        .await?
    {
        Some(snapshot) => snapshot,
        None => {
            state
                .inner
                .gateway
                .resume_bound_agent_session(
                    binding.clone(),
                    bound.profile.clone(),
                    peer.clone(),
                    options.clone(),
                )
                .await?
        }
    };
    if !snapshot.capabilities.session.delete {
        return Err(agent_session_error(
            "agent_session_delete_unsupported",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "This ACP Agent does not support deleting its persistent session.",
            Some(format!("thread:{thread_id}")),
        ));
    }
    state.inner.state.set_session_metadata_field(
        thread_id,
        DELETE_INTENT_METADATA_KEY,
        Some(json!({"state": "prepared", "createdAtMs": gateway_now_ms()})),
    )?;
    state
        .inner
        .gateway
        .delete_bound_agent_session(binding, bound.profile, peer, options)
        .await?;
    state.inner.state.set_session_metadata_field(
        thread_id,
        DELETE_INTENT_METADATA_KEY,
        Some(json!({"state": "remoteAcknowledged", "updatedAtMs": gateway_now_ms()})),
    )?;
    state.inner.state.delete_session(thread_id)?;
    state
        .inner
        .codex_capability_broker
        .archive_ephemeral_thread(thread_id)
        .await;
    Ok(())
}

fn require_acp_binding(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<GatewayRuntimeBindingRecord> {
    let binding = state
        .inner
        .state
        .gateway_runtime_binding(thread_id)?
        .ok_or_else(|| agent_session_configuration_error("The Thread has no Agent binding."))?;
    if binding.backend_kind.as_deref() != Some("acp") {
        return Err(agent_session_error(
            "agent_session_fork_unsupported",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            "This Agent does not expose session fork.",
            Some(format!("thread:{thread_id}")),
        ));
    }
    Ok(binding)
}

pub(super) fn reconcile_acknowledged_session_deletes(state: &WebState) {
    let Ok(sessions) = state.inner.state.list_sessions_with_sources(&[]) else {
        return;
    };
    for session in sessions {
        let Ok(Some(metadata)) = state.inner.state.session_metadata(&session.id) else {
            continue;
        };
        if metadata
            .get(DELETE_INTENT_METADATA_KEY)
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            == Some("remoteAcknowledged")
        {
            let _ = state.inner.state.delete_session(&session.id);
        }
    }
}

pub(super) fn typed_thread_snapshot(
    value: Value,
) -> psychevo_runtime::Result<wire::ThreadSnapshot> {
    serde_json::from_value(value)
        .map_err(|error| Error::Message(format!("invalid Thread snapshot projection: {error}")))
}
