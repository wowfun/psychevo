use futures::{StreamExt, stream};
use psychevo::Error;
use psychevo::agents::AgentEntrypoint;
use psychevo_gateway_protocol as wire;
use serde_json::Value;

use crate::gateway::agent_session::{
    AgentErrorStage, CapturedAgentImportContext, CapturedFrameworkAgentImport, agent_error_view,
    agent_session_configuration_error, agent_session_error,
};
use crate::gateway::agent_session_binding::{
    PreparedGatewayAgentTurn, resolve_gateway_agent_binding_snapshot_at,
    runtime_profile_config_fingerprint, runtime_profile_config_revision,
};
use crate::gateway_now_ms;
use crate::history_editing::HistoryEditingSurface;

use super::binding::{AgentSessionImportCandidate, AuthContext, WebState};
use super::runtime_profiles;
use super::scope_session::{ResolvedScope, bind_source_to_thread, resolve_optional_scope};
use super::session_view::{session_summary_by_id, thread_snapshot_live};

const IMPORT_DISCOVERY_CONCURRENCY: usize = 4;
pub(super) async fn list(
    state: &WebState,
    auth: &AuthContext,
    params: wire::agents_backend_rpc::ThreadImportListParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadImportListResult> {
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
    Ok(wire::agents_backend_rpc::ThreadImportListResult { profiles })
}

async fn discover_profile_sessions(
    state: &WebState,
    scope: &ResolvedScope,
    profile: runtime_profiles::ImportableAcpProfile,
    public_cursor: Option<String>,
) -> wire::agents_backend_rpc::ThreadImportProfileView {
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
                match state
                    .inner
                    .framework
                    .agent_thread_by_native_session(
                        &runtime_profile_ref,
                        &session.native_session_id,
                    )
                    .await
                {
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
                sessions.push(wire::agents_backend_rpc::ThreadImportCandidateView {
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
            wire::agents_backend_rpc::ThreadImportProfileView {
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
    targets: Vec<wire::agents_backend_rpc::RunnableTargetView>,
    error: Error,
) -> wire::agents_backend_rpc::ThreadImportProfileView {
    wire::agents_backend_rpc::ThreadImportProfileView {
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
) -> psychevo::Result<Option<String>> {
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
    params: wire::agents_backend_rpc::ThreadImportParams,
) -> psychevo::Result<wire::agents_backend_rpc::ThreadImportResult> {
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
        .framework
        .agent_thread_by_native_session(
            &candidate.runtime_profile_ref,
            &candidate.native_session_id,
        )
        .await?
    {
        let thread_id = existing.id();
        bind_source_to_thread(state, &scope, thread_id).await?;
        if import_archived {
            archive_thread(state, thread_id).await?;
        } else {
            restore_thread(state, thread_id).await?;
        }
        return Ok(wire::agents_backend_rpc::ThreadImportResult {
            snapshot: Box::new(typed_thread_snapshot(
                thread_snapshot_live(state, &scope, Some(thread_id)).await?,
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
    let import_cwd = candidate.cwd.clone();
    let mut inherited_env = state.inner.inherited_env.clone();
    inherited_env
        .entry("PSYCHEVO_HOME".to_string())
        .or_insert_with(|| state.inner.home.to_string_lossy().into_owned());
    let profile_fingerprint = runtime_profile_config_fingerprint(&profile);
    let profile_revision = runtime_profile_config_revision(&profile_fingerprint);
    let agent = resolve_gateway_agent_binding_snapshot_at(
        &import_cwd,
        &inherited_env,
        target.agent_ref.as_deref(),
        &profile,
        AgentEntrypoint::Peer,
    )?;
    let import_context = CapturedAgentImportContext {
        cwd: import_cwd.clone(),
        runtime_options: Default::default(),
    };
    let preparation =
        state
            .inner
            .gateway
            .capture_framework_agent_import(CapturedFrameworkAgentImport {
                target: PreparedGatewayAgentTurn {
                    profile,
                    profile_revision,
                    profile_fingerprint,
                    peer: Some(peer),
                    agent,
                },
                context: import_context,
                native_session_id: candidate.native_session_id,
                title: candidate.title,
                target_label: target.label,
            });
    let mut request = psychevo::ImportAgentThreadRequest::new(import_cwd, preparation.token());
    request.source = "web".to_string();
    let imported = state.inner.framework.import_agent_thread(request).await?;
    drop(preparation);
    let thread_id = imported.thread.id().to_string();
    bind_source_to_thread(state, &scope, &thread_id).await?;
    if import_archived {
        archive_thread(state, &thread_id).await?;
    } else if imported.existing {
        restore_thread(state, &thread_id).await?;
    }
    Ok(wire::agents_backend_rpc::ThreadImportResult {
        snapshot: Box::new(typed_thread_snapshot(
            thread_snapshot_live(state, &scope, Some(&thread_id)).await?,
        )?),
    })
}

fn take_import_candidate(
    state: &WebState,
    candidate_id: &str,
) -> psychevo::Result<AgentSessionImportCandidate> {
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

pub(super) async fn fork_acp_thread(
    state: &WebState,
    scope: &ResolvedScope,
    source_thread_id: &str,
) -> psychevo::Result<wire::thread_command_turn::ThreadActionRunResult> {
    let source = state
        .inner
        .framework
        .resume_thread(source_thread_id)
        .await?;
    let fork = source
        .fork_agent(psychevo::ForkAgentThreadRequest {
            source: "web".to_string(),
        })
        .await?;
    let thread_id = fork.id().to_string();
    bind_source_to_thread(state, scope, &thread_id).await?;
    Ok(wire::thread_command_turn::ThreadActionRunResult::Fork {
        source_thread_id: source_thread_id.to_string(),
        snapshot: Box::new(typed_thread_snapshot(
            thread_snapshot_live(state, scope, Some(&thread_id)).await?,
        )?),
    })
}

pub(super) async fn fork_native_thread(
    state: &WebState,
    scope: &ResolvedScope,
    source_thread_id: &str,
    before_session_seq: Option<i64>,
) -> psychevo::Result<wire::thread_command_turn::ThreadActionRunResult> {
    let thread_id = state
        .inner
        .gateway
        .fork_native_history(
            source_thread_id,
            before_session_seq,
            HistoryEditingSurface::Workbench,
        )
        .await?;
    let result = async {
        bind_source_to_thread(state, scope, &thread_id).await?;
        let snapshot = Box::new(typed_thread_snapshot(
            thread_snapshot_live(state, scope, Some(&thread_id)).await?,
        )?);
        Ok(if before_session_seq.is_some() {
            wire::thread_command_turn::ThreadActionRunResult::ForkBefore {
                source_thread_id: source_thread_id.to_string(),
                snapshot,
            }
        } else {
            wire::thread_command_turn::ThreadActionRunResult::Fork {
                source_thread_id: source_thread_id.to_string(),
                snapshot,
            }
        })
    }
    .await;
    if result.is_err()
        && let Ok(thread) = state.inner.framework.resume_thread(&thread_id).await
    {
        let _ = thread.delete().await;
    }
    result
}

pub(super) async fn archive_thread(state: &WebState, thread_id: &str) -> psychevo::Result<Value> {
    let thread = state.inner.framework.resume_thread(thread_id).await?;
    thread.archive().await?;
    state
        .inner
        .codex_capability_broker
        .archive_ephemeral_thread(thread_id)
        .await;
    session_summary_by_id(state, thread_id).await
}

pub(super) async fn restore_thread(state: &WebState, thread_id: &str) -> psychevo::Result<Value> {
    let thread = state.inner.framework.resume_thread(thread_id).await?;
    thread.restore().await?;
    session_summary_by_id(state, thread_id).await
}

pub(super) async fn delete_thread(state: &WebState, thread_id: &str) -> psychevo::Result<()> {
    let thread = state.inner.framework.resume_thread(thread_id).await?;
    thread.delete().await?;
    state
        .inner
        .codex_capability_broker
        .archive_ephemeral_thread(thread_id)
        .await;
    Ok(())
}

pub(super) async fn reconcile_acknowledged_session_deletes(state: &WebState) {
    let _ = state
        .inner
        .framework
        .reconcile_acknowledged_agent_deletes()
        .await;
}

pub(super) fn typed_thread_snapshot(
    value: Value,
) -> psychevo::Result<wire::events_transcript::ThreadSnapshot> {
    serde_json::from_value(value)
        .map_err(|error| Error::Message(format!("invalid Thread snapshot projection: {error}")))
}
