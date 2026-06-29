fn current_browser_session(
    state: &WebState,
    auth: &AuthContext,
) -> psychevo_runtime::Result<BrowserSession> {
    let AuthContext::Browser { session_id } = auth else {
        return Err(Error::Message(
            "browser session is required for this operation".to_string(),
        ));
    };
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::Message("browser session is no longer active".to_string()))
}

fn authorize_thread(
    state: &WebState,
    auth: &AuthContext,
    thread_id: &str,
) -> psychevo_runtime::Result<()> {
    if matches!(auth, AuthContext::Bearer) {
        return Ok(());
    }
    if state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .is_none()
    {
        return Err(Error::Message(format!("session not found: {thread_id}")));
    }
    Ok(())
}

fn selector_from_thread_or_default(
    state: &WebState,
    auth: &AuthContext,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<GatewayThreadSelector> {
    if let Some(thread_id) = thread_id {
        return Ok(GatewayThreadSelector::thread_id(thread_id));
    }
    let scope = default_resolved_scope(state, auth)?;
    Ok(state.selector(&scope.source))
}

fn selector_from_interaction_context(
    state: &WebState,
    auth: &AuthContext,
    thread_id: Option<String>,
    source_key: Option<String>,
    activity_id: Option<String>,
) -> psychevo_runtime::Result<GatewayThreadSelector> {
    if let Some(thread_id) = thread_id {
        authorize_thread(state, auth, &thread_id)?;
        return Ok(GatewayThreadSelector::thread_id(thread_id));
    }
    if let Some(source_key) = source_key.filter(|value| !value.trim().is_empty()) {
        return Ok(GatewayThreadSelector::source(wire::SourceKey(source_key)));
    }
    if let Some(activity_id) = activity_id.filter(|value| !value.trim().is_empty())
        && let Some(activity) = state.inner.state.store().gateway_activity(&activity_id)?
    {
        if let Some(thread_id) = activity.thread_id {
            authorize_thread(state, auth, &thread_id)?;
            return Ok(GatewayThreadSelector::thread_id(thread_id));
        }
        if let Some(source_key) = activity.source_key {
            return Ok(GatewayThreadSelector::source(wire::SourceKey(source_key)));
        }
    }
    let scope = default_resolved_scope(state, auth)?;
    Ok(state.selector(&scope.source))
}

fn source_from_input(
    input: Option<wire::GatewaySourceInput>,
    cwd: &Path,
    default_lifetime: wire::GatewaySourceLifetime,
) -> GatewaySource {
    let canonical = cwd.to_string_lossy().to_string();
    let hash = stable_hash_hex(&canonical);
    let display = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cwd")
        .to_string();
    let input = input.unwrap_or(wire::GatewaySourceInput {
        kind: "web".to_string(),
        raw_id: None,
        lifetime: Some(default_lifetime),
        raw_identity: None,
        visible_name: None,
    });
    let raw_id = input.raw_id.unwrap_or_else(|| format!("cwd:{hash}"));
    let mut source = GatewaySource::new(input.kind, raw_id);
    source.lifetime = input.lifetime.unwrap_or(default_lifetime);
    source.visible_name = input.visible_name.or(Some(display.clone()));
    let source_kind = source.kind.clone();
    let source_raw_id = source.raw_id.clone();
    let source_lifetime = source.lifetime;
    source.raw_identity = Some(input.raw_identity.unwrap_or_else(|| {
        json!({
            "kind": source_kind,
            "rawId": source_raw_id,
            "cwdHash": hash,
            "displayName": display,
            "lifetime": source_lifetime,
        })
    }));
    source
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn session_cookie_value(cookie_header: &str) -> Option<&str> {
    cookie_header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == "psychevo_gateway_session").then_some(value)
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as i64
}

fn apply_mentions_to_run_options(
    options: &mut RunOptions,
    mentions: &[wire::GatewayMention],
) -> psychevo_runtime::Result<()> {
    let peer_runtime = options
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "native");
    for mention in mentions {
        match &mention.target {
            wire::GatewayMentionTarget::Skill { name, path } => {
                let input = path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                    .unwrap_or(name)
                    .to_string();
                if !options
                    .skill_inputs
                    .iter()
                    .any(|existing| existing == &input)
                {
                    options.skill_inputs.push(input);
                }
            }
            wire::GatewayMentionTarget::Agent {
                name, backend_ref, ..
            } => {
                if let (Some(runtime), Some(backend_ref)) = (peer_runtime, backend_ref.as_deref())
                    && runtime == backend_ref
                {
                    return Err(Error::Message(format!(
                        "{} is already the current runtime; remove @{name} or switch back to Native to delegate to {backend_ref}",
                        backend_ref
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

trait TurnStartInputExt {
    fn input_parts(&self) -> psychevo_runtime::Result<Vec<GatewayInputPart>>;
}

impl TurnStartInputExt for wire::TurnStartParams {
    fn input_parts(&self) -> psychevo_runtime::Result<Vec<GatewayInputPart>> {
        let mut input = self.input.clone();
        if let Some(text) = &self.text
            && !text.trim().is_empty()
        {
            input.push(GatewayInputPart::Text { text: text.clone() });
        }
        if input.is_empty() {
            return Err(Error::Message("turn/start requires input".to_string()));
        }
        Ok(input)
    }
}
