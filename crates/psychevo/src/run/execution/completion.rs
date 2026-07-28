
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

pub(crate) async fn take_prompt_prefix_notice(
    store: &StateRuntime,
    session_id: &str,
) -> Result<Option<String>> {
    let notice = store
        .session_metadata(session_id)
        .await?
        .and_then(|metadata| metadata.get(PROMPT_PREFIX_NOTICE_METADATA_KEY).cloned())
        .and_then(|value| value.as_str().map(str::to_string));
    if notice.is_some() {
        store
            .set_session_metadata_field(session_id, PROMPT_PREFIX_NOTICE_METADATA_KEY, None)
            .await?;
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
    model: Option<LanguageModel>,
    config: &PermissionConfig,
    metadata: Value,
) -> Option<Arc<dyn ApprovalHandler>> {
    if config.approvals_reviewer != crate::types::ApprovalsReviewer::Smart {
        return None;
    }
    let model = model?;
    Some(Arc::new(SmartReviewerApprovalHandler {
        model,
        metadata,
        timeout_secs: config.auto_review.timeout_secs,
    }))
}

pub(crate) fn smart_reviewer_model(
    injected_provider: Option<&Provider>,
    primary_provider: &Provider,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    resolved: &ResolvedRunProvider,
    config: &PermissionConfig,
) -> Option<LanguageModel> {
    if config.approvals_reviewer != ApprovalsReviewer::Smart {
        return None;
    }
    let Some(target) = config.auto_review.model.as_deref() else {
        return primary_provider.language_model(resolved.model.clone()).ok();
    };
    let (provider_id, model_id) = parse_provider_model_spec(target).ok()?;
    if let Some(provider) = injected_provider {
        return provider.language_model(model_id).ok();
    }
    let reviewer = resolve_one_provider(
        &provider_id,
        Some(model_id.clone()),
        None,
        options,
        loaded,
        false,
    )
    .ok()?;
    generation_provider(
        reviewer.base_url,
        reviewer.api_key,
        reviewer.provider,
        reviewer.inference_idle_timeout_secs,
    )
    .ok()?
    .language_model(model_id)
    .ok()
}

#[derive(Clone)]
pub(crate) struct SmartReviewerApprovalHandler {
    pub(crate) model: LanguageModel,
    pub(crate) metadata: Value,
    pub(crate) timeout_secs: u64,
}

impl std::fmt::Debug for SmartReviewerApprovalHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmartReviewerApprovalHandler")
            .field("model", self.model.descriptor())
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
        let model = self.model.clone();
        let metadata = self.metadata.clone();
        Box::pin(async move {
            smart_review_permission(model, metadata, request)
                .await
                .unwrap_or_else(|_| PermissionApprovalDecision::deny())
        })
    }
}

pub(crate) async fn smart_review_permission(
    model: LanguageModel,
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
    let generation = LanguageRequest {
        messages: vec![psychevo_ai::Message::user(prompt.to_string())],
        tools: Vec::new(),
        extensions: BTreeMap::from([("psychevo".to_string(), metadata)]),
        ..LanguageRequest::default()
    };
    let mut stream = model.stream(generation);
    let mut text = String::new();
    while let Some(event) = stream.next_event().await {
        match event.map_err(|err| Error::Message(err.to_string()))? {
            GenerationEvent::TextDelta { delta, .. } => text.push_str(&delta),
            GenerationEvent::Finish { .. } => break,
            _ => {}
        }
    }
    stream
        .finish()
        .await
        .map_err(|err| Error::Message(err.to_string()))?;
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

pub(crate) async fn record_missed_required_agents(
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
    .await
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
    use psychevo_ai::{Fake, FakeLanguageAdapter};

    fn fake_model(text: &str) -> LanguageModel {
        Fake::with_language(FakeLanguageAdapter::text(text))
            .expect("fake provider")
            .provider()
            .language_model("reviewer")
            .expect("fake language model")
    }

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
        let provider =
            fake_model(r#"{"decision":"allow","risk":"low","rationale":"read-only"}"#);
        let decision = smart_review_permission(
            provider,
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
        let provider = fake_model("not json");
        let err = smart_review_permission(
            provider,
            json!({}),
            request(),
        )
        .await
        .expect_err("malformed JSON should fail");
        assert!(err.to_string().contains("expected ident"));
    }

    #[tokio::test]
    async fn smart_reviewer_binds_the_independently_configured_provider_and_model() {
        let temp = tempfile::tempdir().expect("temp");
        let mut options = crate::tests::base_options(&temp).await;
        std::fs::create_dir_all(&options.cwd).expect("cwd");
        let config_path = temp.path().join("config.toml");
        crate::tests::write_config(
            &config_path,
            r#"
model = "primary/main-model"
approvals_reviewer = "smart"

[auto_review]
model = "reviewer/review-model"

[provider.primary]
api = "http://127.0.0.1:8001/v1"

[provider.primary.models.main-model]

[provider.reviewer]
api = "http://127.0.0.1:8002/v1"

[provider.reviewer.models.review-model]
"#,
        )
        .expect("config");
        options.config_path = Some(config_path);
        let cwd = canonical_cwd(&options.cwd).expect("canonical cwd");
        let loaded = load_run_config(&options, &cwd).expect("loaded config");
        let resolved = resolve_one_provider(
            "primary",
            Some("main-model".to_string()),
            None,
            &options,
            &loaded,
            false,
        )
        .expect("primary provider");
        let primary = generation_provider(
            resolved.base_url.clone(),
            resolved.api_key.clone(),
            resolved.provider.clone(),
            resolved.inference_idle_timeout_secs,
        )
        .expect("primary SDK provider");

        let reviewer = smart_reviewer_model(
            None,
            &primary,
            &options,
            &loaded,
            &resolved,
            &loaded.config.permissions,
        )
        .expect("reviewer model");

        assert_eq!(reviewer.descriptor().provider_family, "reviewer");
        assert_eq!(reviewer.descriptor().model_id, "review-model");
    }
}
