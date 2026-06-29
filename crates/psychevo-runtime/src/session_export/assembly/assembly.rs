
#[derive(Serialize)]
pub(crate) struct ExportDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) header: Option<ExportHeaderValue<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) messages: Option<Vec<ExportMessageValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mailbox_events: Option<Vec<ExportMailboxEventValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_input_evidence: Option<Vec<ExportPromptEvidence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_provider_request: Option<ProviderRequestExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_provider_response: Option<ProviderResponseExport>,
}

pub(crate) struct ExportSections {
    pub(crate) prompt_prefix: Option<ExportPromptPrefixValue>,
    pub(crate) messages: Option<Vec<ExportMessageRecord>>,
    pub(crate) mailbox_events: Option<Vec<ExportMailboxEventValue>>,
    pub(crate) evidence: Option<Vec<ExportPromptEvidence>>,
    pub(crate) last_request: Option<ProviderRequestExport>,
    pub(crate) last_response: Option<ProviderResponseExport>,
}

#[derive(Serialize)]
pub(crate) struct ExportHeaderValue<'a> {
    pub(crate) session: ExportSessionValue<'a>,
    pub(crate) options: ExportOptionsValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_prefix: Option<ExportPromptPrefixValue>,
}

#[derive(Serialize)]
pub(crate) struct ExportSessionValue<'a> {
    pub(crate) id: &'a str,
    pub(crate) source: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) model: &'a str,
    pub(crate) provider: &'a str,
    pub(crate) started_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) ended_at_ms: Option<i64>,
    pub(crate) end_reason: Option<&'a str>,
    pub(crate) archived_at_ms: Option<i64>,
    pub(crate) message_count: i64,
    pub(crate) tool_call_count: i64,
    pub(crate) title: Option<&'a str>,
}

#[derive(Serialize)]
pub(crate) struct ExportOptionsValue {
    pub(crate) format: SessionExportFormat,
    pub(crate) artifact_kind: SessionArtifactKind,
    pub(crate) include: Vec<SessionExportInclude>,
}

#[derive(Serialize)]
pub(crate) struct ExportMessageValue {
    pub(crate) session_seq: i64,
    pub(crate) message: Message,
}

#[derive(Serialize)]
pub(crate) struct ExportMailboxEventValue {
    pub(crate) id: i64,
    pub(crate) parent_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) child_session_id: Option<String>,
    pub(crate) agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) task_name: Option<String>,
    pub(crate) agent_name: String,
    pub(crate) created_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delivered_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delivered_prompt_session_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delivered_after_session_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delivered_tool_call_id: Option<String>,
    pub(crate) content_text: String,
    pub(crate) payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<Value>,
}

#[derive(Serialize)]
pub(crate) struct ExportPromptPrefixValue {
    pub(crate) version: i64,
    pub(crate) created_at_ms: i64,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) prefix_hash: String,
    pub(crate) tool_declarations_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) invalidation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<Value>,
    pub(crate) slots: Vec<ExportPromptPrefixSlotValue>,
}

#[derive(Serialize)]
pub(crate) struct ExportPromptPrefixSlotValue {
    pub(crate) slot: String,
    pub(crate) tier: String,
    pub(crate) semantic_role: String,
    pub(crate) provider_role: String,
    pub(crate) order: usize,
    pub(crate) content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_path: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ExportPromptEvidence {
    pub(crate) prompt_session_seq: i64,
    pub(crate) items: Vec<ExportEvidenceItem>,
}

#[derive(Serialize)]
pub(crate) struct ExportEvidenceItem {
    pub(crate) context_seq: i64,
    pub(crate) role: String,
    pub(crate) source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_block_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context_kind: Option<String>,
    pub(crate) content_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProviderRequestExport {
    pub(crate) prompt_session_seq: i64,
    pub(crate) assistant_session_seq: i64,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) base_url: String,
    pub(crate) endpoint: String,
    pub(crate) reconstructed: bool,
    pub(crate) warnings: Vec<String>,
    pub(crate) body: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProviderResponseExport {
    pub(crate) assistant_session_seq: i64,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) raw: bool,
    pub(crate) reconstructed: bool,
    pub(crate) source: String,
    pub(crate) warnings: Vec<String>,
    pub(crate) message: Message,
    pub(crate) usage: Value,
    pub(crate) metadata: Value,
}

pub fn default_session_export_filename(
    session_id: &str,
    format: SessionExportFormat,
    artifact_kind: SessionArtifactKind,
) -> String {
    let short = short_session_id(session_id);
    match artifact_kind {
        SessionArtifactKind::Export => format!("psychevo-session-{short}.{}", format.extension()),
        SessionArtifactKind::Share => format!("psychevo-share-{short}.md"),
    }
}

pub fn render_session_export(
    store: &SqliteStore,
    session_id: &str,
    options: SessionExportOptions,
) -> Result<SessionExportArtifact> {
    let summary = store
        .session_summary(session_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
    let include_messages = options.include.contains(SessionExportInclude::Messages);
    let include_reasoning = options.include.contains(SessionExportInclude::Reasoning);
    let messages = if include_messages {
        Some(load_export_messages(store, session_id, include_reasoning)?)
    } else {
        None
    };
    let last_response = if options
        .include
        .contains(SessionExportInclude::LastProviderResponse)
    {
        if let Some(messages) = messages.as_ref() {
            latest_provider_response_from_messages(&summary, messages)
        } else {
            let response_messages = load_export_messages(store, session_id, include_reasoning)?;
            latest_provider_response_from_messages(&summary, &response_messages)
        }
    } else {
        None
    };
    let prompt_prefix_record = store.load_session_prompt_prefix(session_id)?;
    let last_request = if options
        .include
        .contains(SessionExportInclude::LastProviderRequest)
    {
        let unfiltered_messages = load_unfiltered_export_messages(store, session_id)?;
        reconstruct_last_provider_request(store, session_id, &summary, &unfiltered_messages)?
    } else {
        None
    };
    let evidence = if options
        .include
        .contains(SessionExportInclude::ProviderInputEvidence)
    {
        Some(load_provider_input_evidence(store, session_id)?)
    } else {
        None
    };
    let mailbox_events = store
        .load_agent_mailbox_events(session_id)?
        .into_iter()
        .map(export_mailbox_event_value)
        .collect::<Vec<_>>();
    let mailbox_events = (!mailbox_events.is_empty()).then_some(mailbox_events);
    let prompt_prefix = prompt_prefix_record.map(export_prompt_prefix_value);
    let sections = ExportSections {
        prompt_prefix,
        messages,
        mailbox_events,
        evidence,
        last_request,
        last_response,
    };
    let format = options.format;
    let content = match format {
        SessionExportFormat::Markdown => render_markdown(&summary, &sections, &options),
        SessionExportFormat::Json => {
            let document = export_document(&summary, sections, options);
            serde_json::to_string_pretty(&document)?
        }
    };
    Ok(SessionExportArtifact {
        content,
        format,
        session_id: summary.id,
    })
}

pub fn write_session_export(
    store: &SqliteStore,
    session_id: &str,
    output_path: &Path,
    options: SessionExportOptions,
) -> Result<SessionExportWriteResult> {
    let artifact = render_session_export(store, session_id, options)?;
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, artifact.content.as_bytes())?;
    Ok(SessionExportWriteResult {
        path: output_path.to_path_buf(),
        bytes: artifact.content.len(),
        format: artifact.format,
        session_id: artifact.session_id,
    })
}

pub(crate) fn load_export_messages(
    store: &SqliteStore,
    session_id: &str,
    include_reasoning: bool,
) -> Result<Vec<ExportMessageRecord>> {
    store
        .load_export_message_summaries(session_id)?
        .into_iter()
        .map(|record| {
            let message = if include_reasoning {
                sanitize_reasoning_for_export(&record.message)
            } else {
                sanitize_message_without_reasoning(&record.message)
            };
            Ok(ExportMessageRecord {
                session_seq: record.session_seq,
                message,
                usage: record.usage,
                metadata: record.metadata,
            })
        })
        .collect()
}

pub(crate) fn load_unfiltered_export_messages(
    store: &SqliteStore,
    session_id: &str,
) -> Result<Vec<ExportMessageRecord>> {
    store
        .load_export_message_summaries(session_id)?
        .into_iter()
        .map(|record| {
            Ok(ExportMessageRecord {
                session_seq: record.session_seq,
                message: record.message,
                usage: record.usage,
                metadata: record.metadata,
            })
        })
        .collect()
}

pub(crate) fn latest_provider_response_from_messages(
    summary: &SessionSummary,
    messages: &[ExportMessageRecord],
) -> Option<ProviderResponseExport> {
    messages.iter().rev().find_map(|record| {
        let Message::Assistant {
            model, provider, ..
        } = &record.message
        else {
            return None;
        };
        Some(ProviderResponseExport {
            assistant_session_seq: record.session_seq,
            provider: provider.clone().unwrap_or_else(|| summary.provider.clone()),
            model: model.clone().unwrap_or_else(|| summary.model.clone()),
            raw: false,
            reconstructed: true,
            source: "persisted_assistant_message".to_string(),
            warnings: vec!["Original provider response chunks are not persisted.".to_string()],
            message: record.message.clone(),
            usage: record
                .usage
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            metadata: record
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        })
    })
}

pub(crate) fn export_prompt_prefix_value(record: PromptPrefixRecord) -> ExportPromptPrefixValue {
    ExportPromptPrefixValue {
        version: record.version,
        created_at_ms: record.created_at_ms,
        provider: record.provider,
        model: record.model,
        prefix_hash: record.prefix_hash,
        tool_declarations_hash: record.tool_declarations_hash,
        invalidation_reason: record.invalidation_reason,
        metadata: record.metadata,
        slots: record
            .slots
            .into_iter()
            .map(|slot| ExportPromptPrefixSlotValue {
                slot: slot.slot,
                tier: slot.tier,
                semantic_role: slot.semantic_role,
                provider_role: slot.provider_role,
                order: slot.order,
                content_hash: slot.content_hash,
                source_kind: slot.source_kind,
                source_name: slot.source_name,
                source_path: slot.source_path,
            })
            .collect(),
    }
}

pub(crate) fn load_provider_input_evidence(
    store: &SqliteStore,
    session_id: &str,
) -> Result<Vec<ExportPromptEvidence>> {
    let mut prompts = Vec::new();
    for record in store.load_export_message_summaries(session_id)? {
        if !matches!(record.message, Message::User { .. }) {
            continue;
        }
        let items = store.load_context_evidence(session_id, record.session_seq)?;
        if items.is_empty() {
            continue;
        }
        prompts.push(ExportPromptEvidence {
            prompt_session_seq: record.session_seq,
            items: items.into_iter().map(export_evidence_item).collect(),
        });
    }
    Ok(prompts)
}

pub(crate) fn export_evidence_item(record: ContextEvidenceRecord) -> ExportEvidenceItem {
    ExportEvidenceItem {
        context_seq: record.context_seq,
        role: record.role,
        source_kind: record.source_kind,
        source_name: record.source_name,
        source_path: record.source_path,
        provider_group: record.provider_group,
        provider_block_index: record.provider_block_index,
        context_kind: record.context_kind,
        content_text: record.content_text,
        metadata: record.metadata,
    }
}

pub(crate) fn export_mailbox_event_value(
    record: AgentMailboxEventRecord,
) -> ExportMailboxEventValue {
    ExportMailboxEventValue {
        id: record.id,
        parent_session_id: record.parent_session_id,
        child_session_id: record.child_session_id,
        agent_id: record.agent_id,
        task_name: record.task_name,
        agent_name: record.agent_name,
        created_at_ms: record.created_at_ms,
        delivered_at_ms: record.delivered_at_ms,
        delivered_prompt_session_seq: record.delivered_prompt_session_seq,
        delivered_after_session_seq: record.delivered_after_session_seq,
        delivered_tool_call_id: record.delivered_tool_call_id,
        content_text: record.content_text,
        payload: record.payload,
        metadata: record.metadata,
    }
}

pub(crate) fn reconstruct_last_provider_request(
    store: &SqliteStore,
    session_id: &str,
    summary: &SessionSummary,
    messages: &[ExportMessageRecord],
) -> Result<Option<ProviderRequestExport>> {
    let metadata = store.session_metadata(session_id)?.unwrap_or(Value::Null);
    let base_url = metadata
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let endpoint = openai_chat_completions_endpoint(&base_url);
    let mut warnings = base_reconstruction_warnings(&metadata);
    let mode = session_mode_from_metadata(&metadata, &mut warnings);
    let generation_metadata = generation_metadata_from_session_metadata(&metadata, &mut warnings);
    let cwd = PathBuf::from(&summary.cwd);
    let all_tools = reconstructed_tool_declarations(store, summary, &metadata, &cwd, mode);
    let reconstructed_tool_declarations_hash = tool_declarations_hash_from_declarations(&all_tools);
    let mut current_prompt = None;
    let mut last_request = None;

    for (index, record) in messages.iter().enumerate() {
        if matches!(record.message, Message::User { .. }) {
            current_prompt = Some((record.session_seq, record.metadata.clone()));
        }
        if !matches!(record.message, Message::Assistant { .. }) {
            continue;
        }
        let Some((prompt_session_seq, ref prompt_metadata)) = current_prompt else {
            continue;
        };
        let mut request_warnings = warnings.clone();
        let prompt_prefix_record = matching_prompt_prefix(
            store,
            session_id,
            prompt_metadata,
            &record.metadata,
            &mut request_warnings,
        )?;
        if let Some(prefix) = prompt_prefix_record.as_ref()
            && prefix.tool_declarations_hash != reconstructed_tool_declarations_hash
        {
            request_warnings.push(format!(
                "current registry tool declarations hash `{}` does not match recorded prompt prefix tool declarations hash `{}`; tool schema reconstruction is approximate",
                reconstructed_tool_declarations_hash, prefix.tool_declarations_hash
            ));
        }
        let effective_tool_names = effective_tool_names_from_prefix_metadata(
            prompt_metadata,
            &record.metadata,
            prompt_prefix_record.as_ref(),
            &mut request_warnings,
        );
        let tools = filter_tool_declarations(&all_tools, &effective_tool_names);
        let context = ProviderMessageReconstruction {
            store,
            session_id,
            messages,
            mode,
            prompt_prefix: prompt_prefix_record.as_ref(),
        };
        let provider_messages = reconstructed_provider_messages(
            &context,
            index,
            prompt_session_seq,
            prompt_metadata,
            &record.metadata,
            &mut request_warnings,
        )?;
        let request = GenerationRequest {
            model: ModelTarget {
                provider: summary.provider.clone(),
                model: summary.model.clone(),
            },
            messages: provider_messages,
            tools,
            metadata: generation_metadata.clone(),
        };
        let body = openai_chat_request_body(&request, &base_url);
        last_request = Some(ProviderRequestExport {
            prompt_session_seq,
            assistant_session_seq: record.session_seq,
            provider: summary.provider.clone(),
            model: summary.model.clone(),
            base_url: base_url.clone(),
            endpoint: endpoint.clone(),
            reconstructed: true,
            warnings: request_warnings,
            body,
        });
    }

    Ok(last_request)
}

pub(crate) struct ProviderMessageReconstruction<'a> {
    pub(crate) store: &'a SqliteStore,
    pub(crate) session_id: &'a str,
    pub(crate) messages: &'a [ExportMessageRecord],
    pub(crate) mode: RunMode,
    pub(crate) prompt_prefix: Option<&'a PromptPrefixRecord>,
}

pub(crate) fn reconstructed_provider_messages(
    context: &ProviderMessageReconstruction<'_>,
    assistant_index: usize,
    prompt_session_seq: i64,
    prompt_metadata: &Option<Value>,
    assistant_metadata: &Option<Value>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>> {
    let evidence = context
        .store
        .load_context_evidence(context.session_id, prompt_session_seq)?;
    let mailbox_events = context
        .store
        .load_agent_mailbox_events(context.session_id)?;
    let _ = (prompt_metadata, assistant_metadata);
    if let Some(prefix) = context.prompt_prefix {
        return reconstructed_provider_messages_from_prefix(
            &evidence,
            &mailbox_events,
            prefix,
            context.messages,
            assistant_index,
            prompt_session_seq,
        );
    }

    let mut provider_messages =
        prompt_instruction_values_from_evidence(&evidence, "prefix_prompt_instructions");
    if provider_messages.is_empty() {
        provider_messages.push(serde_json::json!({
            "role": "system",
            "content": mode_instruction(context.mode),
        }));
        warnings.push(
            "prompt-scoped system instruction evidence was unavailable; default mode instruction was reconstructed"
                .to_string(),
        );
    }

    for record in context
        .messages
        .iter()
        .take_while(|record| record.session_seq < prompt_session_seq)
    {
        provider_messages.push(message_to_value(&record.message)?);
        push_mailbox_events_delivered_after_message(
            &mut provider_messages,
            &mailbox_events,
            record.session_seq,
        )?;
    }
    push_mailbox_events_delivered_for_prompt(
        &mut provider_messages,
        &mailbox_events,
        prompt_session_seq,
    )?;

    if evidence.is_empty() {
        warnings.push(
            "prompt-scoped context evidence was unavailable; hidden project or selected-skill inputs may be missing"
                .to_string(),
        );
    } else {
        for message in contextual_user_messages_from_evidence(&evidence) {
            provider_messages.push(message.to_provider_value());
        }
    }

    for record in context
        .messages
        .iter()
        .take(assistant_index)
        .filter(|record| record.session_seq >= prompt_session_seq)
    {
        provider_messages.push(message_to_value(&record.message)?);
        push_mailbox_events_delivered_after_message(
            &mut provider_messages,
            &mailbox_events,
            record.session_seq,
        )?;
    }

    Ok(provider_messages)
}

pub(crate) fn reconstructed_provider_messages_from_prefix(
    evidence: &[ContextEvidenceRecord],
    mailbox_events: &[AgentMailboxEventRecord],
    prefix: &PromptPrefixRecord,
    messages: &[ExportMessageRecord],
    assistant_index: usize,
    prompt_session_seq: i64,
) -> Result<Vec<Value>> {
    let mut provider_messages = Vec::new();
    provider_messages.extend(prefix_prompt_instruction_values(prefix));
    for message in prefix_contextual_user_messages(prefix) {
        provider_messages.push(message.to_provider_value());
    }

    for record in messages
        .iter()
        .take_while(|record| record.session_seq < prompt_session_seq)
    {
        provider_messages.push(message_to_value(&record.message)?);
        push_mailbox_events_delivered_after_message(
            &mut provider_messages,
            mailbox_events,
            record.session_seq,
        )?;
    }
    push_mailbox_events_delivered_for_prompt(
        &mut provider_messages,
        mailbox_events,
        prompt_session_seq,
    )?;

    provider_messages.extend(turn_prompt_instruction_values_from_evidence(evidence));
    for message in turn_contextual_user_messages_from_evidence(evidence) {
        provider_messages.push(message.to_provider_value());
    }

    for record in messages
        .iter()
        .take(assistant_index)
        .filter(|record| record.session_seq >= prompt_session_seq)
    {
        provider_messages.push(message_to_value(&record.message)?);
        push_mailbox_events_delivered_after_message(
            &mut provider_messages,
            mailbox_events,
            record.session_seq,
        )?;
    }

    Ok(provider_messages)
}

pub(crate) fn matching_prompt_prefix(
    store: &SqliteStore,
    session_id: &str,
    prompt_metadata: &Option<Value>,
    assistant_metadata: &Option<Value>,
    warnings: &mut Vec<String>,
) -> Result<Option<PromptPrefixRecord>> {
    let prompt_hash = prompt_prefix_hash(prompt_metadata);
    let assistant_hash = prompt_prefix_hash(assistant_metadata);
    if let (Some(prompt_hash), Some(assistant_hash)) = (prompt_hash, assistant_hash)
        && prompt_hash != assistant_hash
    {
        warnings.push(format!(
            "user prompt prefix hash `{prompt_hash}` differs from assistant prompt prefix hash `{assistant_hash}`; using the user prompt hash for reconstruction"
        ));
    }
    let (recorded_hash, source) = if let Some(prompt_hash) = prompt_hash {
        (prompt_hash, "user prompt")
    } else if let Some(assistant_hash) = assistant_hash {
        (assistant_hash, "assistant message")
    } else {
        warnings.push(
            "neither the user prompt nor assistant message includes a prompt prefix hash; hidden prefix snapshot cannot be verified and the reconstructed request is approximate"
                .to_string(),
        );
        return Ok(None);
    };
    let prompt_version = prompt_prefix_version(prompt_metadata);
    let assistant_version = prompt_prefix_version(assistant_metadata);
    if let (Some(prompt_version), Some(assistant_version)) = (prompt_version, assistant_version)
        && prompt_version != assistant_version
    {
        warnings.push(format!(
            "user prompt prefix version `{prompt_version}` differs from assistant prompt prefix version `{assistant_version}`; using the user prompt version for reconstruction"
        ));
    }
    let prefix = if let Some(version) = prompt_version.or(assistant_version) {
        let Some(prefix) = store.load_session_prompt_prefix_version(session_id, version)? else {
            warnings.push(format!(
                "prompt prefix snapshot version `{version}` from {source} is unavailable; hidden prefix text cannot be reconstructed and the request is approximate"
            ));
            return Ok(None);
        };
        prefix
    } else {
        warnings.push(format!(
            "{source} prompt prefix metadata does not include a version; using latest stored prompt prefix as an approximate fallback"
        ));
        let Some(prefix) = store.load_session_prompt_prefix(session_id)? else {
            warnings.push(format!(
                "prompt prefix snapshot `{recorded_hash}` from {source} is unavailable; hidden prefix text cannot be reconstructed and the request is approximate"
            ));
            return Ok(None);
        };
        prefix
    };
    if prefix.prefix_hash != recorded_hash {
        warnings.push(format!(
            "prompt prefix snapshot version `{}` hash `{}` does not match {source} prompt prefix `{recorded_hash}`; hidden prefix text is stale or unavailable and the request is approximate",
            prefix.version, prefix.prefix_hash
        ));
        return Ok(None);
    }
    Ok(Some(prefix))
}
