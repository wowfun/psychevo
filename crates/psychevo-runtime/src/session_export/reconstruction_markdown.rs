#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn prompt_prefix_hash(metadata: &Option<Value>) -> Option<&str> {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.get("prompt_prefix"))
        .and_then(|prefix| prefix.get("hash"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn prefix_prompt_instruction_values(prefix: &PromptPrefixRecord) -> Vec<Value> {
    let mut slots = prefix
        .slots
        .iter()
        .filter(|slot| slot.provider_role != "user" && !slot.content.trim().is_empty())
        .collect::<Vec<_>>();
    slots.sort_by_key(|slot| slot.order);
    slots
        .into_iter()
        .map(|slot| {
            PromptInstruction {
                slot: slot.slot.clone(),
                tier: slot.tier.clone(),
                semantic_role: slot.semantic_role.clone(),
                provider_role: slot.provider_role.clone(),
                order: slot.order,
                content: slot.content.clone(),
                content_hash: slot.content_hash.clone(),
                source_kind: slot.source_kind.clone(),
                source_name: slot.source_name.clone(),
                source_path: slot.source_path.clone(),
            }
            .to_provider_value()
        })
        .collect()
}

pub(crate) fn prefix_contextual_user_messages(
    prefix: &PromptPrefixRecord,
) -> Vec<ContextualUserMessage> {
    let mut slots = prefix
        .slots
        .iter()
        .filter(|slot| {
            slot.provider_role == "user"
                && slot.semantic_role == "project_context"
                && !slot.content.trim().is_empty()
        })
        .collect::<Vec<_>>();
    slots.sort_by_key(|slot| slot.order);
    let blocks = slots
        .into_iter()
        .map(|slot| {
            ContextualUserBlock::new(
                slot.source_kind
                    .clone()
                    .unwrap_or_else(|| "project_instruction".to_string()),
                slot.source_name.clone(),
                slot.source_path.clone(),
                slot.content.clone(),
            )
        })
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        Vec::new()
    } else {
        vec![ContextualUserMessage::new_with_category(
            "project_instructions",
            "project_context",
            blocks,
        )]
    }
}

pub(crate) fn turn_prompt_instruction_values_from_evidence(
    evidence: &[ContextEvidenceRecord],
) -> Vec<Value> {
    prompt_instruction_values_from_evidence(evidence, "turn_prompt_instructions")
}

pub(crate) fn prompt_instruction_values_from_evidence(
    evidence: &[ContextEvidenceRecord],
    provider_group: &str,
) -> Vec<Value> {
    let mut items = evidence
        .iter()
        .filter(|item| {
            item.provider_group.as_deref() == Some(provider_group)
                && !item.content_text.trim().is_empty()
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| {
        (
            item.provider_block_index.unwrap_or(i64::MAX),
            item.context_seq,
        )
    });
    items
        .into_iter()
        .map(|item| {
            let metadata = item.metadata.as_ref();
            let slot = metadata
                .and_then(|metadata| metadata.get("slot"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| item.source_name.clone());
            let tier = metadata
                .and_then(|metadata| metadata.get("tier"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    if provider_group.starts_with("prefix_") {
                        "prefix"
                    } else {
                        "turn"
                    }
                });
            let semantic_role = metadata
                .and_then(|metadata| metadata.get("semantic_role"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| item.context_kind.clone());
            let content_hash = metadata
                .and_then(|metadata| metadata.get("content_hash"))
                .and_then(Value::as_str);
            let order = metadata
                .and_then(|metadata| metadata.get("order"))
                .and_then(Value::as_u64);
            serde_json::json!({
                "role": item.role,
                "content": item.content_text,
                "metadata": {
                    "prompt_slot": slot,
                    "prompt_slot_tier": tier,
                    "prompt_semantic_role": semantic_role,
                    "prompt_content_hash": content_hash,
                    "prompt_order": order,
                    "source_kind": item.source_kind,
                    "source_name": item.source_name,
                    "source_path": item.source_path,
                }
            })
        })
        .collect()
}

pub(crate) fn turn_contextual_user_messages_from_evidence(
    evidence: &[ContextEvidenceRecord],
) -> Vec<ContextualUserMessage> {
    contextual_user_messages_from_evidence_for_kinds(evidence, &["selected_skill"])
}

pub(crate) fn contextual_user_messages_from_evidence(
    evidence: &[ContextEvidenceRecord],
) -> Vec<ContextualUserMessage> {
    contextual_user_messages_from_evidence_for_kinds(
        evidence,
        &["project_instruction", "selected_skill"],
    )
}

pub(crate) fn contextual_user_messages_from_evidence_for_kinds(
    evidence: &[ContextEvidenceRecord],
    source_kinds: &[&str],
) -> Vec<ContextualUserMessage> {
    #[derive(Debug)]
    struct Group {
        name: String,
        first_context_seq: i64,
        timestamp_ms: i64,
        blocks: Vec<(Option<i64>, i64, ContextualUserBlock)>,
    }

    let mut groups = Vec::<Group>::new();
    for item in evidence.iter().filter(|item| {
        item.role == "user"
            && source_kinds.contains(&item.source_kind.as_str())
            && !item.content_text.trim().is_empty()
    }) {
        let group_name = item
            .provider_group
            .clone()
            .unwrap_or_else(|| format!("legacy_context:{}", item.context_seq));
        let block = ContextualUserBlock::new(
            item.context_kind
                .clone()
                .unwrap_or_else(|| item.source_kind.clone()),
            item.source_name.clone(),
            item.source_path.clone(),
            item.content_text.clone(),
        );
        if let Some(group) = groups.iter_mut().find(|group| group.name == group_name) {
            group
                .blocks
                .push((item.provider_block_index, item.context_seq, block));
        } else {
            groups.push(Group {
                name: group_name,
                first_context_seq: item.context_seq,
                timestamp_ms: item.timestamp_ms,
                blocks: vec![(item.provider_block_index, item.context_seq, block)],
            });
        }
    }

    groups.sort_by_key(|group| group.first_context_seq);
    groups
        .into_iter()
        .map(|mut group| {
            group.blocks.sort_by_key(|(block_index, context_seq, _)| {
                (block_index.unwrap_or(i64::MAX), *context_seq)
            });
            ContextualUserMessage {
                provider_group: group.name,
                context_category: "turn_context".to_string(),
                blocks: group
                    .blocks
                    .into_iter()
                    .map(|(_, _, block)| block)
                    .collect(),
                hidden: true,
                timestamp_ms: group.timestamp_ms,
            }
        })
        .collect()
}

pub(crate) fn message_to_value(message: &Message) -> Result<Value> {
    Ok(serde_json::to_value(message)?)
}

pub(crate) fn push_mailbox_events_delivered_after_message(
    provider_messages: &mut Vec<Value>,
    mailbox_events: &[AgentMailboxEventRecord],
    session_seq: i64,
) -> Result<()> {
    for event in mailbox_events
        .iter()
        .filter(|event| event.delivered_after_session_seq == Some(session_seq))
    {
        provider_messages.push(message_to_value(&agent_mailbox_event_message(event))?);
    }
    Ok(())
}

pub(crate) fn push_mailbox_events_delivered_for_prompt(
    provider_messages: &mut Vec<Value>,
    mailbox_events: &[AgentMailboxEventRecord],
    prompt_session_seq: i64,
) -> Result<()> {
    for event in mailbox_events.iter().filter(|event| {
        event
            .delivered_prompt_session_seq
            .is_some_and(|seq| seq <= prompt_session_seq)
    }) {
        provider_messages.push(message_to_value(&agent_mailbox_event_message(event))?);
    }
    Ok(())
}

pub(crate) fn base_reconstruction_warnings(metadata: &Value) -> Vec<String> {
    let mut warnings = vec![
        "request body is reconstructed from persisted session data, not captured from the original HTTP request".to_string(),
        "reconstruction uses the current provider adapter and current tool schemas; old sessions may differ".to_string(),
        "local image references are re-read during reconstruction and may differ if files changed".to_string(),
        "context pruning may differ for sessions that used unstored pruning options".to_string(),
        "HTTP headers and API keys are never included".to_string(),
        "skill/no-skill runtime options are not persisted; default skill tool declarations were reconstructed".to_string(),
    ];
    if metadata
        .get("base_url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        warnings.push("session metadata does not include base_url; endpoint was reconstructed from an empty base URL".to_string());
    }
    if metadata.get("model_metadata").is_none() {
        warnings.push(
            "session metadata does not include model_metadata; capability-specific request shaping may differ"
                .to_string(),
        );
    }
    warnings
}

pub(crate) fn session_mode_from_metadata(metadata: &Value, warnings: &mut Vec<String>) -> RunMode {
    match metadata
        .get("mode")
        .and_then(Value::as_str)
        .and_then(RunMode::parse)
    {
        Some(mode) => mode,
        None => {
            warnings.push(
                "session metadata does not include a valid mode; default mode tool declarations were reconstructed"
                    .to_string(),
            );
            RunMode::default()
        }
    }
}

pub(crate) fn generation_metadata_from_session_metadata(
    metadata: &Value,
    warnings: &mut Vec<String>,
) -> Value {
    let mut object = Map::new();
    if let Some(model_metadata) = metadata.get("model_metadata") {
        object.insert("model_metadata".to_string(), model_metadata.clone());
    }
    if let Some(reasoning_effort) = metadata
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert(
            "reasoning_effort".to_string(),
            Value::String(reasoning_effort.to_string()),
        );
    } else {
        warnings.push(
            "session metadata does not include reasoning_effort; no explicit reasoning_effort field was reconstructed"
                .to_string(),
        );
    }
    Value::Object(object)
}

pub(crate) fn reconstructed_tool_declarations(
    store: &SqliteStore,
    summary: &SessionSummary,
    metadata: &Value,
    workdir: &Path,
    mode: RunMode,
) -> Vec<ToolDeclaration> {
    let base_url = metadata
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        base_url.clone(),
        String::new(),
        summary.provider.clone(),
    ));
    let tools = assemble_tool_surface(ToolSurfaceAssembly {
        workdir: workdir.to_path_buf(),
        task_id: summary.id.clone(),
        mode,
        lsp: Default::default(),
        allow_login_shell: false,
        stream_events: None,
        path_prefixes: Vec::new(),
        tool_selection: Default::default(),
        custom_toolsets: BTreeMap::new(),
        clarify: ClarifyToolSurface::declaration_only(),
        skills: Some(SkillDiscoveryOptions {
            home: workdir.join(".psychevo"),
            workdir: workdir.to_path_buf(),
            config_path: None,
            env: BTreeMap::new(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        }),
        extension_tools: Vec::new(),
        agents: Some(AgentToolContext {
            provider,
            model_provider: summary.provider.clone(),
            model: summary.model.clone(),
            provider_label: metadata
                .get("provider_label")
                .and_then(Value::as_str)
                .unwrap_or(summary.provider.as_str())
                .to_string(),
            base_url,
            api_key_env: metadata
                .get("api_key_env")
                .and_then(Value::as_str)
                .map(str::to_string),
            reasoning_effort: metadata
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::to_string),
            context_limit: metadata.get("context_limit").and_then(Value::as_u64),
            generation_metadata: json_value_object_with_model_metadata(metadata),
            workdir: workdir.to_path_buf(),
            mode,
            permission_config: Default::default(),
            lsp: Default::default(),
            permission_mode: Default::default(),
            approval_mode: Default::default(),
            approval_handler: None,
            state: StateRuntime::from_store(PathBuf::new(), store.clone()),
            config_path: None,
            parent_session_id: summary.id.clone(),
            parent_context_snapshot: Vec::new(),
            catalog: AgentCatalog::default(),
            control_handle: None,
            stream_events: None,
            model_metadata: ModelMetadata::default(),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            tool_selection: Default::default(),
            custom_toolsets: BTreeMap::new(),
            allowed_agent_names: None,
            denied_agent_names: BTreeSet::new(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
        }),
    });
    tool_declarations(&tools)
}

pub(crate) fn json_value_object_with_model_metadata(metadata: &Value) -> Value {
    let mut object = Map::new();
    if let Some(model_metadata) = metadata.get("model_metadata") {
        object.insert("model_metadata".to_string(), model_metadata.clone());
    }
    Value::Object(object)
}

pub(crate) fn effective_tool_names_from_prefix_metadata(
    prompt_metadata: &Option<Value>,
    assistant_metadata: &Option<Value>,
    prompt_prefix: Option<&PromptPrefixRecord>,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    if let Some(names) = effective_tool_names_from_message_metadata(prompt_metadata) {
        return names;
    }
    if let Some(names) = effective_tool_names_from_message_metadata(assistant_metadata) {
        return names;
    }
    if let Some(names) =
        prompt_prefix.and_then(|prefix| effective_tool_names_from_value(prefix.metadata.as_ref()?))
    {
        return names;
    }
    warnings.push(
        "prompt prefix metadata does not include effective_tools; no tool declarations were reconstructed"
            .to_string(),
    );
    Vec::new()
}

pub(crate) fn effective_tool_names_from_message_metadata(
    metadata: &Option<Value>,
) -> Option<Vec<String>> {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.get("prompt_prefix"))
        .and_then(effective_tool_names_from_value)
}

pub(crate) fn effective_tool_names_from_value(value: &Value) -> Option<Vec<String>> {
    value
        .get("effective_tools")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
}

pub(crate) fn filter_tool_declarations(
    declarations: &[ToolDeclaration],
    effective_names: &[String],
) -> Vec<ToolDeclaration> {
    let effective = effective_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    declarations
        .iter()
        .filter(|tool| effective.contains(tool.name.as_str()))
        .cloned()
        .collect()
}

pub(crate) fn export_document<'a>(
    summary: &'a SessionSummary,
    sections: ExportSections,
    options: SessionExportOptions,
) -> ExportDocument<'a> {
    let ExportSections {
        prompt_prefix,
        messages,
        mailbox_events,
        evidence,
        last_request,
        last_response,
    } = sections;
    let header = options
        .include
        .contains(SessionExportInclude::Header)
        .then(|| ExportHeaderValue {
            session: ExportSessionValue {
                id: &summary.id,
                source: &summary.source,
                workdir: &summary.workdir,
                model: &summary.model,
                provider: &summary.provider,
                started_at_ms: summary.started_at_ms,
                updated_at_ms: summary.updated_at_ms,
                ended_at_ms: summary.ended_at_ms,
                end_reason: summary.end_reason.as_deref(),
                archived_at_ms: summary.archived_at_ms,
                message_count: summary.message_count,
                tool_call_count: summary.tool_call_count,
                title: summary.title.as_deref(),
            },
            options: ExportOptionsValue {
                format: options.format,
                artifact_kind: options.artifact_kind,
                include: options.include.values().collect(),
            },
            prompt_prefix,
        });
    ExportDocument {
        header,
        messages: messages.map(|messages| {
            messages
                .iter()
                .map(|record| ExportMessageValue {
                    session_seq: record.session_seq,
                    message: record.message.clone(),
                })
                .collect()
        }),
        mailbox_events,
        provider_input_evidence: evidence.filter(|items| !items.is_empty()),
        last_provider_request: last_request,
        last_provider_response: last_response,
    }
}

pub(crate) fn render_markdown(
    summary: &SessionSummary,
    sections: &ExportSections,
    options: &SessionExportOptions,
) -> String {
    let mut out = String::new();
    if options.include.contains(SessionExportInclude::Header) {
        let title = match options.artifact_kind {
            SessionArtifactKind::Export => "# Psychevo Session Export",
            SessionArtifactKind::Share => "# Psychevo Session Share",
        };
        push_line(&mut out, title);
        push_line(&mut out, "");
        if let Some(title) = summary.title.as_deref().filter(|value| !value.is_empty()) {
            push_line(&mut out, &format!("Title: {}", markdown_inline(title)));
        }
        push_line(&mut out, &format!("Session: `{}`", summary.id));
        push_line(&mut out, &format!("Source: `{}`", summary.source));
        push_line(&mut out, &format!("Workdir: `{}`", summary.workdir));
        push_line(
            &mut out,
            &format!("Model: `{}/{}`", summary.provider, summary.model),
        );
        push_line(&mut out, &format!("Started: `{}`", summary.started_at_ms));
        push_line(&mut out, &format!("Updated: `{}`", summary.updated_at_ms));
        push_line(&mut out, "");
        push_line(&mut out, "Options:");
        push_line(
            &mut out,
            &format!("- artifact: `{}`", options.artifact_kind.as_str()),
        );
        push_line(
            &mut out,
            &format!("- include: `{}`", options.include.tokens().join(",")),
        );
        if let Some(prefix) = sections.prompt_prefix.as_ref() {
            push_line(&mut out, "");
            render_markdown_prompt_prefix(&mut out, prefix);
        }
    }
    if let Some(messages) = sections.messages.as_deref() {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Transcript");
        for record in messages {
            push_line(&mut out, "");
            render_markdown_message(&mut out, record);
        }
    }
    if let Some(mailbox_events) = sections
        .mailbox_events
        .as_deref()
        .filter(|items| !items.is_empty())
    {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Mailbox Events");
        for event in mailbox_events {
            push_line(&mut out, "");
            push_line(&mut out, &format!("### Mailbox event #{}", event.id));
            push_line(&mut out, &format!("- agent: `{}`", event.agent_name));
            push_line(&mut out, &format!("- agent_id: `{}`", event.agent_id));
            if let Some(seq) = event.delivered_prompt_session_seq {
                push_line(
                    &mut out,
                    &format!("- delivered_prompt_session_seq: `{seq}`"),
                );
            }
            if let Some(seq) = event.delivered_after_session_seq {
                push_line(&mut out, &format!("- delivered_after_session_seq: `{seq}`"));
            }
            push_fenced_json(&mut out, &event.payload);
        }
    }
    if let Some(evidence) = sections.evidence.as_ref().filter(|items| !items.is_empty()) {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Provider Input Evidence");
        for prompt in evidence {
            push_line(&mut out, "");
            push_line(
                &mut out,
                &format!("### Prompt message #{}", prompt.prompt_session_seq),
            );
            for item in &prompt.items {
                push_line(&mut out, "");
                push_line(
                    &mut out,
                    &format!(
                        "#### {} / {}",
                        markdown_inline(&item.role),
                        markdown_inline(&item.source_kind)
                    ),
                );
                if let Some(name) = &item.source_name {
                    push_line(&mut out, &format!("- source: `{}`", markdown_inline(name)));
                }
                if let Some(path) = &item.source_path {
                    push_line(&mut out, &format!("- path: `{}`", markdown_inline(path)));
                }
                if let Some(group) = &item.provider_group {
                    push_line(
                        &mut out,
                        &format!("- provider_group: `{}`", markdown_inline(group)),
                    );
                }
                if let Some(index) = item.provider_block_index {
                    push_line(&mut out, &format!("- provider_block_index: `{index}`"));
                }
                if let Some(kind) = &item.context_kind {
                    push_line(
                        &mut out,
                        &format!("- context_kind: `{}`", markdown_inline(kind)),
                    );
                }
                if let Some(metadata) = &item.metadata {
                    push_line(&mut out, "- metadata:");
                    push_fenced_json(&mut out, metadata);
                }
                push_fenced_text(&mut out, &item.content_text);
            }
        }
    }
    if options
        .include
        .contains(SessionExportInclude::LastProviderRequest)
    {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Reconstructed Last Provider Request");
        if let Some(request) = sections.last_request.as_ref() {
            push_line(&mut out, "");
            push_line(
                &mut out,
                &format!(
                    "### Prompt #{} -> assistant #{}",
                    request.prompt_session_seq, request.assistant_session_seq
                ),
            );
            push_line(
                &mut out,
                &format!("- provider: `{}`", markdown_inline(&request.provider)),
            );
            push_line(
                &mut out,
                &format!("- model: `{}`", markdown_inline(&request.model)),
            );
            push_line(
                &mut out,
                &format!("- base_url: `{}`", markdown_inline(&request.base_url)),
            );
            push_line(
                &mut out,
                &format!("- endpoint: `{}`", markdown_inline(&request.endpoint)),
            );
            push_line(&mut out, "- reconstructed: `true`");
            if !request.warnings.is_empty() {
                push_line(&mut out, "- warnings:");
                for warning in &request.warnings {
                    push_line(&mut out, &format!("  - {}", markdown_inline(warning)));
                }
            }
            push_fenced_json(&mut out, &request.body);
        } else {
            push_line(&mut out, "");
            push_line(&mut out, "_No reconstructed provider request available._");
        }
    }
    if options
        .include
        .contains(SessionExportInclude::LastProviderResponse)
    {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Normalized Last Provider Response");
        if let Some(response) = sections.last_response.as_ref() {
            push_line(&mut out, "");
            push_fenced_json(
                &mut out,
                &serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({})),
            );
        } else {
            push_line(&mut out, "");
            push_line(&mut out, "_No persisted assistant response available._");
        }
    }
    out
}

pub(crate) fn render_markdown_prompt_prefix(out: &mut String, prefix: &ExportPromptPrefixValue) {
    push_line(out, "### Prompt Prefix");
    push_line(out, "");
    push_line(
        out,
        &format!("- hash: `{}`", markdown_inline(&prefix.prefix_hash)),
    );
    push_line(out, &format!("- version: `{}`", prefix.version));
    push_line(
        out,
        &format!(
            "- model: `{}/{}`",
            markdown_inline(&prefix.provider),
            markdown_inline(&prefix.model)
        ),
    );
    push_line(
        out,
        &format!(
            "- tool_declarations_hash: `{}`",
            markdown_inline(&prefix.tool_declarations_hash)
        ),
    );
    if let Some(reason) = &prefix.invalidation_reason {
        push_line(
            out,
            &format!("- invalidation_reason: `{}`", markdown_inline(reason)),
        );
    }
    if let Some(metadata) = &prefix.metadata {
        push_line(out, "- metadata:");
        push_fenced_json(out, metadata);
    }
    push_line(out, "");
    push_line(out, "| slot | role | tier | hash | source |");
    push_line(out, "| --- | --- | --- | --- | --- |");
    for slot in &prefix.slots {
        let source = slot
            .source_path
            .as_deref()
            .or(slot.source_name.as_deref())
            .or(slot.source_kind.as_deref())
            .unwrap_or("");
        push_line(
            out,
            &format!(
                "| `{}` | `{}/{}` | `{}` | `{}` | `{}` |",
                markdown_inline(&slot.slot),
                markdown_inline(&slot.semantic_role),
                markdown_inline(&slot.provider_role),
                markdown_inline(&slot.tier),
                markdown_inline(&slot.content_hash),
                markdown_inline(source),
            ),
        );
    }
}

pub(crate) fn render_markdown_message(out: &mut String, record: &ExportMessageRecord) {
    match &record.message {
        Message::User { content, .. } => {
            push_line(out, &format!("### {}. User", record.session_seq));
            let text = user_content_markdown(content);
            if text.trim().is_empty() {
                push_line(out, "_No text content._");
            } else {
                push_line(out, "");
                push_line(out, &text);
            }
        }
        Message::Assistant { content, .. } => {
            push_line(out, &format!("### {}. Assistant", record.session_seq));
            for block in content {
                match block {
                    AssistantBlock::Text { text } => {
                        if !text.trim().is_empty() {
                            push_line(out, "");
                            push_line(out, text);
                        }
                    }
                    AssistantBlock::Reasoning { text, .. } => {
                        if !text.trim().is_empty() {
                            push_line(out, "");
                            push_line(out, "#### Reasoning");
                            push_fenced_text(out, text);
                        }
                    }
                    AssistantBlock::ToolCall(call) => {
                        push_line(out, "");
                        push_line(
                            out,
                            &format!("#### Tool call: `{}` (`{}`)", call.name, call.id),
                        );
                        push_fenced_json(out, &call.arguments);
                    }
                }
            }
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => {
            push_line(
                out,
                &format!(
                    "### {}. Tool result: `{}` (`{}`)",
                    record.session_seq, tool_name, tool_call_id
                ),
            );
            push_line(out, &format!("- error: `{is_error}`"));
            push_fenced_text(out, content);
        }
    }
}

pub(crate) fn sanitize_message_without_reasoning(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => Message::Assistant {
            content: content
                .iter()
                .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
                .cloned()
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}
