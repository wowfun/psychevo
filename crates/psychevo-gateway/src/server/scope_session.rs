use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use psychevo::Error;
use psychevo::application::{ThreadAgentBinding, ThreadListQuery};
use psychevo::host_paths::normalized_native_path;
use psychevo::paths::canonicalize_cwd;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::gateway::agent_session_binding::runtime_session_handle;
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewaySource, SourceKey,
};

use super::auth_input::{current_browser_session, source_from_input};
use super::binding::{AuthContext, BrowserSession, WebState};
use super::rpc_json::cwd_source;
use super::session_view::thread_snapshot;

pub(super) fn gateway_profile_value(state: &WebState) -> Value {
    let name = state
        .inner
        .inherited_env
        .get("PSYCHEVO_PROFILE")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    json!({
        "name": name,
        "home": state.inner.home.display().to_string(),
        "default": name == "default",
    })
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedScope {
    pub(super) cwd: PathBuf,
    pub(super) source: GatewaySource,
}

impl ResolvedScope {
    pub(super) fn to_wire_scope(&self) -> wire::source::GatewayRequestScope {
        let cwd = psychevo::host_paths::normalized_native_path(&self.cwd);
        wire::source::GatewayRequestScope {
            cwd: cwd.display().to_string(),
            source: wire::source::GatewaySourceInput {
                kind: self.source.kind.clone(),
                raw_id: Some(self.source.raw_id.clone()),
                lifetime: Some(self.source.lifetime),
                raw_identity: self.source.raw_identity.clone(),
                visible_name: self.source.visible_name.clone(),
            },
        }
    }
}

pub(super) fn detached_draft_scope(scope: &ResolvedScope, auth: &AuthContext) -> ResolvedScope {
    if !matches!(auth, AuthContext::Browser { .. }) {
        return scope.clone();
    }
    let cwd = psychevo::host_paths::normalized_native_path(&scope.cwd);
    let mut source = scope.source.clone();
    let canonical_raw_id = source
        .raw_identity
        .as_ref()
        .and_then(|identity| identity.get("canonicalRawId"))
        .and_then(Value::as_str)
        .unwrap_or(source.raw_id.as_str())
        .to_string();
    source.raw_id = format!("{}:draft:{}", source.raw_id, Uuid::now_v7());
    source.visible_name = source
        .visible_name
        .clone()
        .or_else(|| Some("Web draft".to_string()));
    source.raw_identity = Some(json!({
        "kind": source.kind.clone(),
        "rawId": source.raw_id.clone(),
        "canonicalRawId": canonical_raw_id,
        "cwd": cwd.display().to_string(),
        "draft": true,
    }));
    ResolvedScope {
        cwd: scope.cwd.clone(),
        source,
    }
}

pub(super) fn canonical_source_mutation_key(source: &GatewaySource) -> SourceKey {
    let raw_id = source
        .raw_identity
        .as_ref()
        .filter(|identity| identity.get("draft").and_then(Value::as_bool) == Some(true))
        .and_then(|identity| identity.get("canonicalRawId"))
        .and_then(Value::as_str)
        .unwrap_or(source.raw_id.as_str());
    SourceKey(format!("{}:{raw_id}", source.kind))
}

#[cfg(test)]
pub(super) async fn start_empty_source(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<Value> {
    state
        .inner
        .gateway
        .clear_source_binding(&scope.source)
        .await?;
    thread_snapshot(state, scope, None).await
}

pub(super) async fn reset_source_to_empty(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<Value> {
    state
        .inner
        .gateway
        .reset_source_to_empty(&scope.source)
        .await?;
    thread_snapshot(state, scope, None).await
}

pub(super) async fn bind_source_to_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
) -> psychevo::Result<()> {
    if let Some(bound) = state
        .inner
        .gateway
        .resolve_source_thread(&scope.source)
        .await?
        && bound == thread_id
    {
        return Ok(());
    }
    state
        .inner
        .gateway
        .bind_source_thread(
            &scope.source,
            thread_id,
            &gateway_backend_info_for_thread(state, thread_id).await?,
            Some(json!({"reason": "thread_resume"})),
        )
        .await?;
    Ok(())
}

pub(super) async fn ensure_turn_start_thread(
    state: &WebState,
    scope: &ResolvedScope,
    requested_thread_id: Option<String>,
) -> psychevo::Result<(Option<String>, bool)> {
    if let Some(thread_id) = requested_thread_id {
        bind_source_to_thread(state, scope, &thread_id).await?;
        return Ok((Some(thread_id), false));
    }
    if let Some(thread_id) = state
        .inner
        .gateway
        .resolve_source_thread(&scope.source)
        .await?
    {
        return Ok((Some(thread_id), false));
    }
    Ok((Some(Uuid::now_v7().to_string()), true))
}

pub(super) fn shell_execution_intent(
    state: &WebState,
    scope: &ResolvedScope,
) -> crate::gateway::activity::ShellExecutionIntent {
    crate::gateway::activity::ShellExecutionIntent::new(scope.source.kind.clone())
        .inherited_environment(state.inner.inherited_env.clone())
}

pub(super) async fn gateway_backend_info_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo::Result<GatewayBackendInfo> {
    let thread = state.inner.framework.resume_thread(thread_id).await?;
    gateway_backend_info_for_thread_handle(&thread).await
}

pub(super) async fn gateway_backend_info_for_thread_handle(
    thread: &psychevo::Thread,
) -> psychevo::Result<GatewayBackendInfo> {
    let thread_id = thread.id();
    if let Some(binding) = thread.agent_binding().await? {
        let binding = match binding {
            ThreadAgentBinding::Resolved { binding, .. } => *binding,
            ThreadAgentBinding::Unresolved { reason, .. } => {
                let message = reason.unwrap_or_else(|| {
                    format!("Thread `{thread_id}` has an unresolved runtime binding.")
                });
                return Err(Error::structured(
                    message.clone(),
                    json!({
                        "code": "unresolved_binding",
                        "stage": "binding",
                        "retryClass": "user_action",
                        "message": message,
                        "diagnosticRef": Value::Null,
                    }),
                ));
            }
        };
        let runtime_ref = binding.runtime_ref;
        let kind = match binding.backend_kind.as_str() {
            "native" => BackendKind::Native,
            "acp" => BackendKind::Acp,
            other => {
                let message =
                    format!("Thread `{thread_id}` has unsupported runtime backend kind `{other}`.");
                return Err(Error::structured(
                    message.clone(),
                    json!({
                        "code": "unsupported",
                        "stage": "binding",
                        "retryClass": "user_action",
                        "message": message,
                        "diagnosticRef": Value::Null,
                    }),
                ));
            }
        };
        let session_handle = binding.native_session_id.as_deref().map(|native_id| {
            runtime_session_handle(&runtime_ref, Path::new(&binding.cwd), native_id)
        });
        return Ok(GatewayBackendInfo {
            kind,
            runtime_ref: Some(runtime_ref),
            native_id: session_handle,
        });
    }
    Ok(GatewayBackendInfo {
        kind: BackendKind::Native,
        runtime_ref: Some("native".to_string()),
        native_id: None,
    })
}

pub(super) fn default_resolved_scope(
    state: &WebState,
    auth: &AuthContext,
) -> psychevo::Result<ResolvedScope> {
    match auth {
        AuthContext::Bearer => Ok(ResolvedScope {
            cwd: state.inner.cwd.clone(),
            source: state.inner.source.clone(),
        }),
        AuthContext::Browser { .. } => {
            let session = current_browser_session(state, auth)?;
            Ok(ResolvedScope {
                cwd: session.cwd.clone(),
                source: session.source.clone(),
            })
        }
    }
}

pub(super) fn resolve_optional_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::source::GatewayRequestScope>,
) -> psychevo::Result<ResolvedScope> {
    match scope {
        Some(scope) => resolve_required_scope(state, auth, scope),
        None => default_resolved_scope(state, auth),
    }
}

pub(super) fn resolve_required_scope(
    _state: &WebState,
    _auth: &AuthContext,
    scope: wire::source::GatewayRequestScope,
) -> psychevo::Result<ResolvedScope> {
    let cwd = canonicalize_cwd(Path::new(&scope.cwd))?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &cwd,
            wire::source::GatewaySourceLifetime::Persistent,
        ),
        cwd,
    })
}

pub(super) fn resolve_external_file_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: wire::source::GatewayRequestScope,
) -> psychevo::Result<ResolvedScope> {
    let resolved = resolve_required_scope(state, auth, scope)?;
    if matches!(auth, AuthContext::Browser { .. }) {
        let session = current_browser_session(state, auth)?;
        let authorized_cwd = canonicalize_cwd(&session.cwd)?;
        if resolved.cwd != authorized_cwd {
            return Err(Error::Message(
                "browser session is not authorized for external actions in this workspace"
                    .to_string(),
            ));
        }
        if !session
            .external_action_grants
            .contains(&normalized_native_path(&authorized_cwd))
        {
            return Err(Error::Message(
                "browser session has no external-action grant for this workspace".to_string(),
            ));
        }
    }
    Ok(resolved)
}

pub(super) fn resolve_workspace_preview_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: wire::source::GatewayRequestScope,
) -> psychevo::Result<ResolvedScope> {
    let resolved = resolve_required_scope(state, auth, scope)?;
    if matches!(auth, AuthContext::Browser { .. }) {
        let session = current_browser_session(state, auth)?;
        let authorized_cwd = canonicalize_cwd(&session.cwd)?;
        if resolved.cwd != authorized_cwd {
            return Err(Error::Message(
                "browser session is not authorized for file previews in this workspace".to_string(),
            ));
        }
    }
    Ok(resolved)
}

pub(super) fn resolve_start_scope(
    _state: &WebState,
    _auth: &AuthContext,
    scope: wire::source::GatewayRequestScope,
) -> psychevo::Result<ResolvedScope> {
    let cwd = canonicalize_cwd(Path::new(&scope.cwd))?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &cwd,
            wire::source::GatewaySourceLifetime::Persistent,
        ),
        cwd,
    })
}

pub(super) fn resolve_cwd_filter(
    state: &WebState,
    auth: &AuthContext,
    cwd: Option<String>,
) -> psychevo::Result<PathBuf> {
    let cwd = match cwd {
        Some(cwd) => canonicalize_cwd(Path::new(&cwd))?,
        None => default_resolved_scope(state, auth)?.cwd,
    };
    Ok(cwd)
}

pub(super) fn resolve_session_cwd_filter(
    _state: &WebState,
    _auth: &AuthContext,
    cwd: Option<String>,
) -> psychevo::Result<Option<PathBuf>> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    let cwd = canonicalize_cwd(Path::new(&cwd))?;
    Ok(Some(cwd))
}

pub(super) async fn resolved_scope_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo::Result<ResolvedScope> {
    let summary = state
        .inner
        .framework
        .thread_summary(thread_id)
        .await?
        .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?;
    let cwd = PathBuf::from(summary.cwd);
    Ok(ResolvedScope {
        source: cwd_source(&cwd),
        cwd,
    })
}

fn update_browser_session_scope(state: &WebState, auth: &AuthContext, scope: &ResolvedScope) {
    let AuthContext::Browser { session_id, .. } = auth else {
        return;
    };
    let mut sessions = state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned");
    if let Some(session) = sessions.get_mut(session_id) {
        session.cwd = scope.cwd.clone();
        session.source = scope.source.clone();
    } else {
        sessions.insert(
            session_id.clone(),
            BrowserSession {
                cwd: scope.cwd.clone(),
                source: scope.source.clone(),
                external_action_grants: BTreeSet::new(),
            },
        );
    }
}

pub(super) fn grant_browser_session_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: &ResolvedScope,
) {
    let AuthContext::Browser { session_id, .. } = auth else {
        return;
    };
    let mut sessions = state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned");
    if let Some(session) = sessions.get_mut(session_id) {
        session.cwd = scope.cwd.clone();
        session.source = scope.source.clone();
        session
            .external_action_grants
            .insert(normalized_native_path(&scope.cwd));
    }
}

pub(super) async fn update_browser_session_for_draft_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: &ResolvedScope,
) -> psychevo::Result<()> {
    if !matches!(auth, AuthContext::Browser { .. }) {
        return Ok(());
    }
    let session = current_browser_session(state, auth)?;
    let cwd = normalized_native_path(&scope.cwd);
    let already_granted = session.external_action_grants.contains(&cwd);
    let adopts_stored_session = !state
        .inner
        .framework
        .list_threads(ThreadListQuery {
            cwd: Some(scope.cwd.clone()),
            archived: false,
            sources: Vec::new(),
            cursor: None,
            limit: 1,
        })
        .await?
        .threads
        .is_empty();
    if already_granted || adopts_stored_session {
        grant_browser_session_scope(state, auth, scope);
    } else {
        update_browser_session_scope(state, auth, scope);
    }
    Ok(())
}
