fn gateway_profile_value(state: &WebState) -> Value {
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
struct ResolvedScope {
    cwd: PathBuf,
    source: GatewaySource,
}

impl ResolvedScope {
    fn to_wire_scope(&self) -> wire::GatewayRequestScope {
        let cwd = psychevo_runtime::normalized_native_path(&self.cwd);
        wire::GatewayRequestScope {
            cwd: cwd.display().to_string(),
            source: wire::GatewaySourceInput {
                kind: self.source.kind.clone(),
                raw_id: Some(self.source.raw_id.clone()),
                lifetime: Some(self.source.lifetime),
                raw_identity: self.source.raw_identity.clone(),
                visible_name: self.source.visible_name.clone(),
            },
        }
    }
}

fn detached_draft_scope(scope: &ResolvedScope, auth: &AuthContext) -> ResolvedScope {
    if !matches!(auth, AuthContext::Browser { .. }) {
        return scope.clone();
    }
    let cwd = psychevo_runtime::normalized_native_path(&scope.cwd);
    let mut source = scope.source.clone();
    source.raw_id = format!("{}:draft:{}", source.raw_id, Uuid::now_v7());
    source.visible_name = source
        .visible_name
        .clone()
        .or_else(|| Some("Web draft".to_string()));
    source.raw_identity = Some(json!({
        "kind": source.kind.clone(),
        "rawId": source.raw_id.clone(),
        "canonicalRawId": scope.source.raw_id.clone(),
        "cwd": cwd.display().to_string(),
        "draft": true,
    }));
    ResolvedScope {
        cwd: scope.cwd.clone(),
        source,
    }
}

#[cfg(test)]
fn start_empty_source(state: &WebState, scope: &ResolvedScope) -> psychevo_runtime::Result<Value> {
    state.inner.gateway.clear_source_binding(&scope.source)?;
    thread_snapshot(state, scope, None)
}

fn reset_source_to_empty(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Value> {
    state.inner.gateway.reset_source_to_empty(&scope.source)?;
    thread_snapshot(state, scope, None)
}

fn bind_source_to_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    if let Some(bound) = state.inner.gateway.resolve_source_thread(&scope.source)?
        && bound == thread_id
    {
        return Ok(());
    }
    state.inner.gateway.bind_source_thread(
        &scope.source,
        thread_id,
        &gateway_backend_info_for_thread(state, thread_id)?,
        Some(json!({"reason": "thread_resume"})),
    )?;
    Ok(())
}

fn ensure_turn_start_thread(
    state: &WebState,
    scope: &ResolvedScope,
    requested_thread_id: Option<String>,
) -> psychevo_runtime::Result<Option<String>> {
    if let Some(thread_id) = requested_thread_id {
        bind_source_to_thread(state, scope, &thread_id)?;
        return Ok(Some(thread_id));
    }
    if let Some(thread_id) = state.inner.gateway.resolve_source_thread(&scope.source)? {
        return Ok(Some(thread_id));
    }

    let thread_id = state.inner.state.store().create_session_with_metadata(
        &scope.cwd,
        "web",
        "pending",
        "pending",
        None,
    )?;
    bind_source_to_thread(state, scope, &thread_id)?;
    Ok(Some(thread_id))
}

fn user_shell_context_options(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
) -> UserShellContextOptions {
    UserShellContextOptions {
        state: state.inner.state.clone(),
        session: thread_id,
        continue_latest: false,
        source: scope.source.kind.clone(),
        continue_sources: Vec::new(),
        config_path: state.inner.config_path.clone(),
        model: None,
        reasoning_effort: None,
        mode: RunMode::Default,
        inherited_env: Some(state.inner.inherited_env.clone()),
    }
}

fn gateway_backend_info_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<GatewayBackendInfo> {
    let store = state.inner.state.store();
    if let Some(binding) = store.gateway_runtime_binding(thread_id)? {
        if binding.status != GatewayRuntimeBindingStatus::Resolved {
            let message = binding.unresolved_reason.unwrap_or_else(|| {
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
        let runtime_ref = binding.runtime_ref.ok_or_else(|| {
            let message = format!(
                "Thread `{thread_id}` has no resolved Runtime Profile identity."
            );
            Error::structured(
                message.clone(),
                json!({
                    "code": "unresolved_binding",
                    "stage": "binding",
                    "retryClass": "user_action",
                    "message": message,
                    "diagnosticRef": Value::Null,
                }),
            )
        })?;
        let kind = match binding.backend_kind.as_deref() {
            Some("native") => BackendKind::Native,
            Some("acp") => BackendKind::Acp,
            other => {
                let message = format!(
                    "Thread `{thread_id}` has unsupported runtime backend kind `{}`.",
                    other.unwrap_or("missing")
                );
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
            crate::runtime_session_handle(&runtime_ref, Path::new(&binding.cwd), native_id)
        });
        return Ok(GatewayBackendInfo {
            kind,
            runtime_ref: Some(runtime_ref),
            native_id: session_handle,
        });
    }

    store
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    Ok(GatewayBackendInfo {
        kind: BackendKind::Native,
        runtime_ref: Some("native".to_string()),
        native_id: None,
    })
}

fn default_resolved_scope(
    state: &WebState,
    auth: &AuthContext,
) -> psychevo_runtime::Result<ResolvedScope> {
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

fn resolve_optional_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::GatewayRequestScope>,
) -> psychevo_runtime::Result<ResolvedScope> {
    match scope {
        Some(scope) => resolve_required_scope(state, auth, scope),
        None => default_resolved_scope(state, auth),
    }
}

fn resolve_required_scope(
    _state: &WebState,
    _auth: &AuthContext,
    scope: wire::GatewayRequestScope,
) -> psychevo_runtime::Result<ResolvedScope> {
    let cwd = canonicalize_cwd(Path::new(&scope.cwd))?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &cwd,
            wire::GatewaySourceLifetime::Persistent,
        ),
        cwd,
    })
}

fn resolve_start_scope(
    _state: &WebState,
    _auth: &AuthContext,
    scope: wire::GatewayRequestScope,
) -> psychevo_runtime::Result<ResolvedScope> {
    let cwd = canonicalize_cwd(Path::new(&scope.cwd))?;
    Ok(ResolvedScope {
        source: source_from_input(
            Some(scope.source),
            &cwd,
            wire::GatewaySourceLifetime::Persistent,
        ),
        cwd,
    })
}

fn resolve_cwd_filter(
    state: &WebState,
    auth: &AuthContext,
    cwd: Option<String>,
) -> psychevo_runtime::Result<PathBuf> {
    let cwd = match cwd {
        Some(cwd) => canonicalize_cwd(Path::new(&cwd))?,
        None => default_resolved_scope(state, auth)?.cwd,
    };
    Ok(cwd)
}

fn resolve_session_cwd_filter(
    _state: &WebState,
    _auth: &AuthContext,
    cwd: Option<String>,
) -> psychevo_runtime::Result<Option<PathBuf>> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    let cwd = canonicalize_cwd(Path::new(&cwd))?;
    Ok(Some(cwd))
}

fn resolved_scope_for_thread(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<ResolvedScope> {
    let summary = state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
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
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .insert(
            session_id.clone(),
            BrowserSession {
                cwd: scope.cwd.clone(),
                source: scope.source.clone(),
            },
        );
}
