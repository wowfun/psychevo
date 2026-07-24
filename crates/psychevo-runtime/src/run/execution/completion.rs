
pub(crate) fn prompt_prefix_invalidation_reason(
    record: &PromptPrefixRecord,
    provider: &str,
    model: &str,
    mode: crate::types::RunMode,
    selected_agent: Option<&SelectedAgent>,
    tool_declarations_hash: &str,
    expected_metadata: &serde_json::Value,
) -> Option<String> {
    if record.provider != provider
        || record.model != model
        || record.tool_declarations_hash != tool_declarations_hash
    {
        return Some("runtime_context_changed".to_string());
    }
    let Some(metadata) = record.metadata.as_ref() else {
        return Some("prefix_metadata_missing".to_string());
    };
    if metadata.get("mode").and_then(serde_json::Value::as_str) != Some(mode.as_str()) {
        return Some("runtime_context_changed".to_string());
    }
    let expected_agent = serde_json::to_value(selected_agent).unwrap_or(serde_json::Value::Null);
    if metadata
        .get("selected_agent")
        .unwrap_or(&serde_json::Value::Null)
        != &expected_agent
    {
        return Some("main_agent_changed".to_string());
    }
    for key in [
        "effective_tools",
        "agent_catalog_visible",
        "visible_agents",
        "skill_catalog_visible",
        "project_instructions_visible",
        "project_instructions_role",
        "project_context",
        "cwd",
    ] {
        if metadata.get(key).unwrap_or(&serde_json::Value::Null)
            != expected_metadata
                .get(key)
                .unwrap_or(&serde_json::Value::Null)
        {
            return Some("runtime_context_changed".to_string());
        }
    }
    None
}

pub(crate) fn take_prompt_prefix_notice(
    store: &StateRuntime,
    session_id: &str,
) -> Result<Option<String>> {
    let notice = store
        .session_metadata(session_id)?
        .and_then(|metadata| metadata.get(PROMPT_PREFIX_NOTICE_METADATA_KEY).cloned())
        .and_then(|value| value.as_str().map(str::to_string));
    if notice.is_some() {
        store.set_session_metadata_field(session_id, PROMPT_PREFIX_NOTICE_METADATA_KEY, None)?;
    }
    Ok(notice)
}

pub(crate) fn required_agent_mentions(prompt: &str, agents: &[AgentDefinition]) -> Vec<String> {
    let known = agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut found = std::collections::BTreeSet::new();
    for raw in prompt.split_whitespace() {
        let Some(rest) = raw.strip_prefix('@') else {
            continue;
        };
        let name = rest.trim_matches(|ch: char| {
            !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        });
        if known.contains(name) {
            found.insert(name.to_string());
        }
    }
    found.into_iter().collect()
}

pub(crate) fn smart_approval_handler(
    provider: Arc<dyn GenerationProvider>,
    resolved: &ResolvedRunProvider,
    config: &PermissionConfig,
    metadata: Value,
) -> Option<Arc<dyn ApprovalHandler>> {
    if config.approvals_reviewer != crate::types::ApprovalsReviewer::Smart {
        return None;
    }
    let model = config
        .auto_review
        .model
        .as_deref()
        .and_then(parse_provider_model)
        .unwrap_or_else(|| ModelTarget {
            provider: resolved.provider.clone(),
            model: resolved.model.clone(),
        });
    Some(Arc::new(SmartReviewerApprovalHandler {
        provider,
        model,
        metadata,
        timeout_secs: config.auto_review.timeout_secs,
    }))
}

pub(crate) fn parse_provider_model(value: &str) -> Option<ModelTarget> {
    let (provider, model) = value.trim().split_once('/')?;
    let provider = provider.trim();
    let model = model.trim();
    (!provider.is_empty() && !model.is_empty()).then(|| ModelTarget {
        provider: provider.to_string(),
        model: model.to_string(),
    })
}

#[derive(Clone)]
pub(crate) struct SmartReviewerApprovalHandler {
    pub(crate) provider: Arc<dyn GenerationProvider>,
    pub(crate) model: ModelTarget,
    pub(crate) metadata: Value,
    pub(crate) timeout_secs: u64,
}

impl std::fmt::Debug for SmartReviewerApprovalHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmartReviewerApprovalHandler")
            .field(
                "model",
                &format!("{}/{}", self.model.provider, self.model.model),
            )
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for SmartReviewerApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> futures::future::BoxFuture<'static, PermissionApprovalDecision> {
        let provider = Arc::clone(&self.provider);
        let model = self.model.clone();
        let metadata = self.metadata.clone();
        Box::pin(async move {
            smart_review_permission(provider, model, metadata, request)
                .await
                .unwrap_or_else(|_| PermissionApprovalDecision::deny())
        })
    }
}

pub(crate) async fn smart_review_permission(
    provider: Arc<dyn GenerationProvider>,
    model: ModelTarget,
    metadata: Value,
    request: PermissionApprovalRequest,
) -> Result<PermissionApprovalDecision> {
    let prompt = json!({
        "instruction": "Review this tool permission request. Return strict JSON only with decision allow or deny, risk, and rationale.",
        "request": {
            "tool": request.tool_name,
            "summary": request.summary,
            "reason": request.reason,
            "matched_rule": request.matched_rule,
            "suggested_rule": request.suggested_rule,
            "filesystem": request.filesystem,
        }
    });
    let generation = GenerationRequest {
        model,
        messages: vec![json!({
            "role": "user",
            "content": prompt.to_string(),
        })],
        tools: Vec::new(),
        metadata,
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let mut stream = provider
        .stream(generation, AbortSignal::new(abort_rx))
        .await
        .map_err(|err| Error::Message(err.to_string()))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|err| Error::Message(err.to_string()))? {
            StreamEvent::TextDelta { text: delta } => text.push_str(&delta),
            StreamEvent::Done { .. } => break,
            _ => {}
        }
    }
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|err| Error::Message(err.to_string()))?;
    match value.get("decision").and_then(Value::as_str) {
        Some("allow") => Ok(PermissionApprovalDecision::allow_once()),
        Some("deny") => Ok(PermissionApprovalDecision::deny()),
        _ => Err(Error::Message(
            "smart reviewer JSON must include decision allow or deny".to_string(),
        )),
    }
}

pub(crate) fn record_missed_required_agents(
    store: &StateRuntime,
    session_id: &str,
    messages: &[Message],
    required: &[String],
) -> Result<()> {
    if required.is_empty() {
        return Ok(());
    }
    let called = called_agent_names(messages, required);
    let missed = required
        .iter()
        .filter(|name| !called.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if missed.is_empty() {
        return Ok(());
    }
    let text = format!(
        "Required agent delegation was not performed: {}",
        missed.join(", ")
    );
    store.append_message_with_metrics(
        session_id,
        &user_text_message(text),
        None,
        Some(json!({
            "agent_notification": {
                "type": "missing_required_agent_call",
                "agents": missed,
                "hidden": true
            }
        })),
    )
}

#[cfg(test)]
mod main_agent_input_tests {
    use super::*;

    #[test]
    fn explicit_agent_wins_over_session_agent() {
        let metadata = json!({
            "main_agent": {
                "name": "session-agent"
            }
        });

        assert_eq!(
            main_agent_input_from_sources(false, Some("cli-agent"), Some(&metadata)).as_deref(),
            Some("cli-agent")
        );
    }

    #[test]
    fn session_agent_is_used_without_explicit_agent() {
        let metadata = json!({
            "main_agent": {
                "input": "session-agent"
            }
        });

        assert_eq!(
            main_agent_input_from_sources(false, None, Some(&metadata)).as_deref(),
            Some("session-agent")
        );
    }

    #[test]
    fn session_explicit_default_uses_no_agent() {
        let metadata = json!({
            "main_agent": {
                "mode": "default"
            }
        });

        assert_eq!(
            main_agent_input_from_sources(false, None, Some(&metadata)),
            None
        );
    }

    #[test]
    fn missing_session_agent_uses_no_agent() {
        assert_eq!(main_agent_input_from_sources(false, None, None), None);
    }
}

#[cfg(test)]
mod smart_reviewer_tests {
    use super::*;
    use psychevo_ai::{FakeProvider, RawStreamEvent};

    fn request() -> PermissionApprovalRequest {
        PermissionApprovalRequest {
            tool_call_id: "call-1".to_string(),
            tool_name: "read".to_string(),
            summary: "/etc/hosts".to_string(),
            reason: "outside cwd".to_string(),
            matched_rule: None,
            suggested_rule: Some("filesystem:/etc/hosts".to_string()),
            allow_always: true,
            filesystem: None,
            timeout_secs: 90,
        }
    }

    #[tokio::test]
    async fn smart_reviewer_allows_once_from_json() {
        let provider: Arc<dyn GenerationProvider> = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text(
                r#"{"decision":"allow","risk":"low","rationale":"read-only"}"#.to_string(),
            ),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let decision = smart_review_permission(
            provider,
            ModelTarget {
                provider: "mock".to_string(),
                model: "reviewer".to_string(),
            },
            json!({}),
            request(),
        )
        .await
        .expect("review");
        assert_eq!(
            decision.outcome,
            crate::types::PermissionApprovalOutcome::AllowOnce
        );
    }

    #[tokio::test]
    async fn smart_reviewer_fails_closed_on_malformed_json() {
        let provider: Arc<dyn GenerationProvider> = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text("not json".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let err = smart_review_permission(
            provider,
            ModelTarget {
                provider: "mock".to_string(),
                model: "reviewer".to_string(),
            },
            json!({}),
            request(),
        )
        .await
        .expect_err("malformed JSON should fail");
        assert!(err.to_string().contains("expected ident"));
    }
}
