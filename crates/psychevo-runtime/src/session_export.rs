use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use psychevo_agent_core::{
    AssistantBlock, ContextualUserBlock, ContextualUserMessage, Message, PromptInstruction,
    UserContentBlock,
};
use psychevo_ai::{
    GenerationProvider, GenerationRequest, ModelTarget, OpenAiChatProvider, ToolDeclaration,
    openai_chat_completions_endpoint, openai_chat_request_body,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::agents::{AgentCatalog, AgentToolContext, agent_mailbox_event_message, agent_tools};
use crate::error::{Error, Result};
use crate::skills::SkillDiscoveryOptions;
use crate::store::{
    AgentMailboxEventRecord, ContextEvidenceRecord, PromptPrefixRecord, SqliteStore,
};
use crate::tools::{coding_core_tools_for_mode, mode_instruction, skill_tools_for_mode};
use crate::types::{ModelMetadata, RunMode, SessionSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionExportFormat {
    Markdown,
    Json,
}

impl SessionExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionArtifactKind {
    Export,
    Share,
}

impl SessionArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Share => "share",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionExportInclude {
    Header,
    Messages,
    Reasoning,
    ProviderInputEvidence,
    LastProviderRequest,
}

impl SessionExportInclude {
    pub fn parse_token(value: &str) -> Option<Self> {
        match value {
            "header" | "h" => Some(Self::Header),
            "messages" | "m" => Some(Self::Messages),
            "reasoning" | "r" => Some(Self::Reasoning),
            "provider-input-evidence" | "pie" => Some(Self::ProviderInputEvidence),
            "last-provider-request" | "lpr" => Some(Self::LastProviderRequest),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Messages => "messages",
            Self::Reasoning => "reasoning",
            Self::ProviderInputEvidence => "provider-input-evidence",
            Self::LastProviderRequest => "last-provider-request",
        }
    }
}

impl Serialize for SessionExportInclude {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExportIncludeSet {
    values: BTreeSet<SessionExportInclude>,
}

impl SessionExportIncludeSet {
    pub fn default_for(_artifact_kind: SessionArtifactKind) -> Self {
        Self::from_values([SessionExportInclude::Messages])
    }

    pub fn parse(value: &str, artifact_kind: SessionArtifactKind) -> Result<Self> {
        let mut values = Vec::new();
        for token in value
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            let include = SessionExportInclude::parse_token(token).ok_or_else(|| {
                Error::Message(format!(
                    "unknown export include `{token}`; expected comma-separated values from {}",
                    include_usage_for_artifact(artifact_kind)
                ))
            })?;
            values.push(include);
        }
        if values.is_empty() {
            return Err(Error::Message(format!(
                "empty export include list; expected comma-separated values from {}",
                include_usage_for_artifact(artifact_kind)
            )));
        }
        Self::new(values, artifact_kind)
    }

    pub fn new(
        values: impl IntoIterator<Item = SessionExportInclude>,
        artifact_kind: SessionArtifactKind,
    ) -> Result<Self> {
        let mut set = Self::from_values(values);
        set.expand_dependencies();
        set.validate_for_artifact(artifact_kind)?;
        Ok(set)
    }

    pub fn contains(&self, include: SessionExportInclude) -> bool {
        self.values.contains(&include)
    }

    pub fn values(&self) -> impl Iterator<Item = SessionExportInclude> + '_ {
        self.values.iter().copied()
    }

    pub fn tokens(&self) -> Vec<&'static str> {
        self.values().map(SessionExportInclude::as_str).collect()
    }

    fn from_values(values: impl IntoIterator<Item = SessionExportInclude>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }

    fn expand_dependencies(&mut self) {
        if self.contains(SessionExportInclude::Reasoning) {
            self.values.insert(SessionExportInclude::Messages);
        }
    }

    fn validate_for_artifact(&self, artifact_kind: SessionArtifactKind) -> Result<()> {
        if artifact_kind == SessionArtifactKind::Share
            && self.contains(SessionExportInclude::LastProviderRequest)
        {
            return Err(Error::Message(
                "share artifacts do not support include value `last-provider-request`".to_string(),
            ));
        }
        Ok(())
    }
}

fn include_usage_for_artifact(artifact_kind: SessionArtifactKind) -> &'static str {
    match artifact_kind {
        SessionArtifactKind::Export => {
            "header,messages,reasoning,provider-input-evidence,last-provider-request"
        }
        SessionArtifactKind::Share => "header,messages,reasoning,provider-input-evidence",
    }
}

#[derive(Debug, Clone)]
pub struct SessionExportOptions {
    pub format: SessionExportFormat,
    pub include: SessionExportIncludeSet,
    pub artifact_kind: SessionArtifactKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionExportArtifact {
    pub content: String,
    pub format: SessionExportFormat,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionExportWriteResult {
    pub path: PathBuf,
    pub bytes: usize,
    pub format: SessionExportFormat,
    pub session_id: String,
}

#[derive(Debug, Clone)]
struct ExportMessageRecord {
    session_seq: i64,
    message: Message,
    metadata: Option<Value>,
}

#[derive(Serialize)]
struct ExportDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<ExportHeaderValue<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<ExportMessageValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mailbox_events: Option<Vec<ExportMailboxEventValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_input_evidence: Option<Vec<ExportPromptEvidence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_provider_request: Option<ProviderRequestExport>,
}

#[derive(Serialize)]
struct ExportHeaderValue<'a> {
    session: ExportSessionValue<'a>,
    options: ExportOptionsValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_prefix: Option<ExportPromptPrefixValue>,
}

#[derive(Serialize)]
struct ExportSessionValue<'a> {
    id: &'a str,
    source: &'a str,
    workdir: &'a str,
    model: &'a str,
    provider: &'a str,
    started_at_ms: i64,
    updated_at_ms: i64,
    ended_at_ms: Option<i64>,
    end_reason: Option<&'a str>,
    archived_at_ms: Option<i64>,
    message_count: i64,
    tool_call_count: i64,
    title: Option<&'a str>,
}

#[derive(Serialize)]
struct ExportOptionsValue {
    format: SessionExportFormat,
    artifact_kind: SessionArtifactKind,
    include: Vec<SessionExportInclude>,
}

#[derive(Serialize)]
struct ExportMessageValue {
    session_seq: i64,
    message: Message,
}

#[derive(Serialize)]
struct ExportMailboxEventValue {
    id: i64,
    parent_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_session_id: Option<String>,
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_name: Option<String>,
    agent_name: String,
    created_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivered_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivered_prompt_session_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivered_after_session_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivered_tool_call_id: Option<String>,
    content_text: String,
    payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

#[derive(Serialize)]
struct ExportPromptPrefixValue {
    version: i64,
    created_at_ms: i64,
    provider: String,
    model: String,
    prefix_hash: String,
    tool_declarations_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    invalidation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
    slots: Vec<ExportPromptPrefixSlotValue>,
}

#[derive(Serialize)]
struct ExportPromptPrefixSlotValue {
    slot: String,
    tier: String,
    semantic_role: String,
    provider_role: String,
    order: usize,
    content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
}

#[derive(Serialize)]
struct ExportPromptEvidence {
    prompt_session_seq: i64,
    items: Vec<ExportEvidenceItem>,
}

#[derive(Serialize)]
struct ExportEvidenceItem {
    context_seq: i64,
    role: String,
    source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_block_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_kind: Option<String>,
    content_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderRequestExport {
    prompt_session_seq: i64,
    assistant_session_seq: i64,
    provider: String,
    model: String,
    base_url: String,
    endpoint: String,
    reconstructed: bool,
    warnings: Vec<String>,
    body: Value,
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
    let prompt_prefix_record = store.load_session_prompt_prefix(session_id)?;
    let last_request = if options
        .include
        .contains(SessionExportInclude::LastProviderRequest)
    {
        let unfiltered_messages = load_unfiltered_export_messages(store, session_id)?;
        reconstruct_last_provider_request(
            store,
            session_id,
            &summary,
            &unfiltered_messages,
            prompt_prefix_record.as_ref(),
        )?
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
    let format = options.format;
    let content = match format {
        SessionExportFormat::Markdown => render_markdown(
            &summary,
            prompt_prefix.as_ref(),
            messages.as_deref(),
            mailbox_events.as_deref(),
            evidence.as_ref(),
            last_request.as_ref(),
            &options,
        ),
        SessionExportFormat::Json => {
            let document = export_document(
                &summary,
                prompt_prefix,
                &messages,
                mailbox_events,
                evidence,
                last_request,
                options,
            );
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

fn load_export_messages(
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
                metadata: record.metadata,
            })
        })
        .collect()
}

fn load_unfiltered_export_messages(
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
                metadata: record.metadata,
            })
        })
        .collect()
}

fn export_prompt_prefix_value(record: PromptPrefixRecord) -> ExportPromptPrefixValue {
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

fn load_provider_input_evidence(
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

fn export_evidence_item(record: ContextEvidenceRecord) -> ExportEvidenceItem {
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

fn export_mailbox_event_value(record: AgentMailboxEventRecord) -> ExportMailboxEventValue {
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

fn reconstruct_last_provider_request(
    store: &SqliteStore,
    session_id: &str,
    summary: &SessionSummary,
    messages: &[ExportMessageRecord],
    prompt_prefix: Option<&PromptPrefixRecord>,
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
    let workdir = PathBuf::from(&summary.workdir);
    let all_tools = reconstructed_tool_declarations(store, summary, &metadata, &workdir, mode);
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
        let effective_tool_names = effective_tool_names_from_prefix_metadata(
            prompt_metadata,
            &record.metadata,
            prompt_prefix,
            &mut request_warnings,
        );
        let tools = filter_tool_declarations(&all_tools, &effective_tool_names);
        let context = ProviderMessageReconstruction {
            store,
            session_id,
            messages,
            mode,
            prompt_prefix,
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

struct ProviderMessageReconstruction<'a> {
    store: &'a SqliteStore,
    session_id: &'a str,
    messages: &'a [ExportMessageRecord],
    mode: RunMode,
    prompt_prefix: Option<&'a PromptPrefixRecord>,
}

fn reconstructed_provider_messages(
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
    if let Some(prefix) = matching_prompt_prefix(
        context.prompt_prefix,
        prompt_metadata,
        assistant_metadata,
        warnings,
    ) {
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

fn reconstructed_provider_messages_from_prefix(
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

fn matching_prompt_prefix<'a>(
    prompt_prefix: Option<&'a PromptPrefixRecord>,
    prompt_metadata: &Option<Value>,
    assistant_metadata: &Option<Value>,
    warnings: &mut Vec<String>,
) -> Option<&'a PromptPrefixRecord> {
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
        return None;
    };
    let Some(prefix) = prompt_prefix else {
        warnings.push(format!(
            "prompt prefix snapshot `{recorded_hash}` from {source} is unavailable; hidden prefix text cannot be reconstructed and the request is approximate"
        ));
        return None;
    };
    if prefix.prefix_hash != recorded_hash {
        warnings.push(format!(
            "latest prompt prefix snapshot `{}` does not match {source} prompt prefix `{recorded_hash}`; hidden prefix text is stale or unavailable and the request is approximate",
            prefix.prefix_hash
        ));
        return None;
    }
    Some(prefix)
}

fn prompt_prefix_hash(metadata: &Option<Value>) -> Option<&str> {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.get("prompt_prefix"))
        .and_then(|prefix| prefix.get("hash"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn prefix_prompt_instruction_values(prefix: &PromptPrefixRecord) -> Vec<Value> {
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

fn prefix_contextual_user_messages(prefix: &PromptPrefixRecord) -> Vec<ContextualUserMessage> {
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

fn turn_prompt_instruction_values_from_evidence(evidence: &[ContextEvidenceRecord]) -> Vec<Value> {
    prompt_instruction_values_from_evidence(evidence, "turn_prompt_instructions")
}

fn prompt_instruction_values_from_evidence(
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

fn turn_contextual_user_messages_from_evidence(
    evidence: &[ContextEvidenceRecord],
) -> Vec<ContextualUserMessage> {
    contextual_user_messages_from_evidence_for_kinds(evidence, &["selected_skill"])
}

fn contextual_user_messages_from_evidence(
    evidence: &[ContextEvidenceRecord],
) -> Vec<ContextualUserMessage> {
    contextual_user_messages_from_evidence_for_kinds(
        evidence,
        &["project_instruction", "selected_skill"],
    )
}

fn contextual_user_messages_from_evidence_for_kinds(
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

fn message_to_value(message: &Message) -> Result<Value> {
    Ok(serde_json::to_value(message)?)
}

fn push_mailbox_events_delivered_after_message(
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

fn push_mailbox_events_delivered_for_prompt(
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

fn base_reconstruction_warnings(metadata: &Value) -> Vec<String> {
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

fn session_mode_from_metadata(metadata: &Value, warnings: &mut Vec<String>) -> RunMode {
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

fn generation_metadata_from_session_metadata(
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

fn reconstructed_tool_declarations(
    store: &SqliteStore,
    summary: &SessionSummary,
    metadata: &Value,
    workdir: &Path,
    mode: RunMode,
) -> Vec<ToolDeclaration> {
    let mut tools = coding_core_tools_for_mode(workdir, mode);
    tools.extend(skill_tools_for_mode(
        SkillDiscoveryOptions {
            home: workdir.join(".psychevo"),
            workdir: workdir.to_path_buf(),
            config_path: None,
            env: BTreeMap::new(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        },
        mode,
    ));
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
    tools.extend(agent_tools(AgentToolContext {
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
        permission_mode: Default::default(),
        approval_mode: Default::default(),
        approval_handler: None,
        store: store.clone(),
        parent_session_id: summary.id.clone(),
        parent_context_snapshot: Vec::new(),
        catalog: AgentCatalog::default(),
        control_handle: None,
        stream_events: None,
        model_metadata: ModelMetadata::default(),
        env: BTreeMap::new(),
        allowed_agent_names: None,
        denied_agent_names: BTreeSet::new(),
        required_agent_names: Vec::new(),
        spawn_depth_remaining: None,
    }));
    tools
        .iter()
        .map(|tool| ToolDeclaration {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters(),
        })
        .collect()
}

fn json_value_object_with_model_metadata(metadata: &Value) -> Value {
    let mut object = Map::new();
    if let Some(model_metadata) = metadata.get("model_metadata") {
        object.insert("model_metadata".to_string(), model_metadata.clone());
    }
    Value::Object(object)
}

fn effective_tool_names_from_prefix_metadata(
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

fn effective_tool_names_from_message_metadata(metadata: &Option<Value>) -> Option<Vec<String>> {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.get("prompt_prefix"))
        .and_then(effective_tool_names_from_value)
}

fn effective_tool_names_from_value(value: &Value) -> Option<Vec<String>> {
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

fn filter_tool_declarations(
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

fn export_document<'a>(
    summary: &'a SessionSummary,
    prompt_prefix: Option<ExportPromptPrefixValue>,
    messages: &Option<Vec<ExportMessageRecord>>,
    mailbox_events: Option<Vec<ExportMailboxEventValue>>,
    evidence: Option<Vec<ExportPromptEvidence>>,
    last_request: Option<ProviderRequestExport>,
    options: SessionExportOptions,
) -> ExportDocument<'a> {
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
        messages: messages.as_ref().map(|messages| {
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
    }
}

fn render_markdown(
    summary: &SessionSummary,
    prompt_prefix: Option<&ExportPromptPrefixValue>,
    messages: Option<&[ExportMessageRecord]>,
    mailbox_events: Option<&[ExportMailboxEventValue]>,
    evidence: Option<&Vec<ExportPromptEvidence>>,
    last_request: Option<&ProviderRequestExport>,
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
        if let Some(prefix) = prompt_prefix {
            push_line(&mut out, "");
            render_markdown_prompt_prefix(&mut out, prefix);
        }
    }
    if let Some(messages) = messages {
        if !out.is_empty() {
            push_line(&mut out, "");
        }
        push_line(&mut out, "## Transcript");
        for record in messages {
            push_line(&mut out, "");
            render_markdown_message(&mut out, record);
        }
    }
    if let Some(mailbox_events) = mailbox_events.filter(|items| !items.is_empty()) {
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
    if let Some(evidence) = evidence.filter(|items| !items.is_empty()) {
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
        if let Some(request) = last_request {
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
    out
}

fn render_markdown_prompt_prefix(out: &mut String, prefix: &ExportPromptPrefixValue) {
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

fn render_markdown_message(out: &mut String, record: &ExportMessageRecord) {
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

fn sanitize_message_without_reasoning(message: &Message) -> Message {
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

fn sanitize_reasoning_for_export(message: &Message) -> Message {
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
                .filter_map(|block| match block {
                    AssistantBlock::Reasoning { text, .. } if !text.trim().is_empty() => {
                        Some(AssistantBlock::Reasoning {
                            text: text.clone(),
                            provider_evidence: None,
                        })
                    }
                    AssistantBlock::Reasoning { .. } => None,
                    other => Some(other.clone()),
                })
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

fn user_content_markdown(content: &[UserContentBlock]) -> String {
    let mut image_index = 0usize;
    content
        .iter()
        .map(|block| match block {
            UserContentBlock::Text(block) => block.text.clone(),
            UserContentBlock::LocalImage(block) => {
                image_index += 1;
                format!("[Image {image_index}: {}]", block.path.display())
            }
            UserContentBlock::ImageUrl(block) => {
                image_index += 1;
                format!("[Image {image_index}: {}]", block.url)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_fenced_json(out: &mut String, value: &Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    push_line(out, "```json");
    push_line(out, &text);
    push_line(out, "```");
}

fn push_fenced_text(out: &mut String, text: &str) {
    push_line(out, "```text");
    push_line(out, text);
    push_line(out, "```");
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn markdown_inline(value: &str) -> String {
    value.replace('`', "\\`")
}

fn short_session_id(session_id: &str) -> &str {
    session_id.get(..13).unwrap_or(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_agent_core::{ToolCallBlock, user_text_message};
    use psychevo_ai::Outcome;
    use tempfile::TempDir;

    use crate::store::{AgentMailboxEventInput, PromptPrefixSlotRecord};

    #[test]
    fn default_export_filename_distinguishes_sibling_uuidv7_sessions() {
        let parent = default_session_export_filename(
            "019e3716-eeb0-7102-9e7b-7a66ac5dc0a1",
            SessionExportFormat::Json,
            SessionArtifactKind::Export,
        );
        let child = default_session_export_filename(
            "019e3716-fa89-7240-9397-1c4a74d8cebf",
            SessionExportFormat::Json,
            SessionArtifactKind::Export,
        );
        assert_ne!(parent, child);
        assert_eq!(parent, "psychevo-session-019e3716-eeb0.json");
        assert_eq!(child, "psychevo-session-019e3716-fa89.json");
    }

    #[test]
    fn export_last_provider_request_omits_tools_for_empty_effective_policy() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "run",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "default",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        let prefix_hash = "empty-tools-prefix";
        let prompt_prefix_metadata = serde_json::json!({
            "prompt_prefix": {
                "hash": prefix_hash,
                "version": 1,
                "created_at_ms": 1,
                "provider": "provider",
                "model": "model",
                "tool_declarations_hash": "empty-tools-hash",
                "invalidation_reason": "new_session",
                "effective_tools": [],
                "agent_catalog_visible": false,
                "visible_agents": [],
                "skill_catalog_visible": false,
                "project_instructions_visible": false,
                "project_instructions_role": null
            }
        });
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("translate this"),
                Some(prompt_prefix_metadata),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "translated".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: prefix_hash.to_string(),
                tool_declarations_hash: "empty-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "Runtime mode: default. No callable tools are available.".to_string(),
                    content_hash: "base".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "selected_agent": null,
                    "agents_enabled": true,
                    "effective_tools": [],
                    "agent_catalog_visible": false,
                    "visible_agents": [],
                    "skill_catalog_visible": false,
                    "project_instructions_visible": false,
                    "project_instructions_role": null
                })),
            })
            .expect("prefix");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        assert_eq!(
            value["header"]["prompt_prefix"]["metadata"]["effective_tools"],
            serde_json::json!([])
        );
        assert!(
            value["last_provider_request"]["body"]
                .as_object()
                .expect("body")
                .get("tools")
                .is_none()
        );
    }

    #[test]
    fn export_last_provider_request_includes_mailbox_result_once_after_wait() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "run",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "default",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        let prefix_hash = "mailbox-prefix";
        let prompt_prefix_metadata = serde_json::json!({
            "prompt_prefix": {
                "hash": prefix_hash,
                "version": 1,
                "created_at_ms": 1,
                "provider": "provider",
                "model": "model",
                "tool_declarations_hash": "mailbox-tools-hash",
                "invalidation_reason": "new_session",
                "effective_tools": ["wait_agent"],
                "agent_catalog_visible": false,
                "visible_agents": [],
                "skill_catalog_visible": false,
                "project_instructions_visible": false,
                "project_instructions_role": null
            }
        });
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("wait for workers"),
                Some(prompt_prefix_metadata),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::ToolCall(ToolCallBlock {
                        id: "call-wait".to_string(),
                        name: "wait_agent".to_string(),
                        arguments: serde_json::json!({"timeout_ms": 1000}),
                        arguments_json: "{\"timeout_ms\":1000}".to_string(),
                        arguments_error: None,
                        content_index: 0,
                        call_index: 0,
                    })],
                    timestamp_ms: 2,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant tool call");
        store
            .append_message(
                &session,
                &Message::ToolResult {
                    tool_call_id: "call-wait".to_string(),
                    tool_name: "wait_agent".to_string(),
                    content: "{\"message\":\"Wait completed.\",\"timed_out\":false}".to_string(),
                    is_error: false,
                    timestamp_ms: 3,
                },
            )
            .expect("append wait result");
        let final_answer = "unique mailbox final answer";
        let payload = serde_json::json!({
            "author": "/root/worker",
            "recipient": "/root",
            "other_recipients": [],
            "content": format!(
                "<subagent_notification>\n{}\n</subagent_notification>",
                serde_json::json!({
                    "agent_id": "agent-1",
                    "agent_name": "worker",
                    "status": "completed",
                    "outcome": "normal",
                    "final_answer": final_answer
                })
            ),
            "trigger_turn": false
        });
        store
            .append_agent_mailbox_event(AgentMailboxEventInput {
                parent_session_id: session.clone(),
                child_session_id: None,
                agent_id: "agent-1".to_string(),
                task_name: Some("worker".to_string()),
                agent_name: "worker".to_string(),
                content_text: serde_json::to_string(&payload).expect("payload text"),
                payload,
                metadata: None,
            })
            .expect("mailbox event");
        store
            .deliver_pending_agent_mailbox_events_for_tool(&session, "call-wait", 3)
            .expect("deliver");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "synthesized".to_string(),
                    }],
                    timestamp_ms: 4,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append final assistant");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: prefix_hash.to_string(),
                tool_declarations_hash: "mailbox-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "Runtime mode: default.".to_string(),
                    content_hash: "base".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "selected_agent": null,
                    "agents_enabled": true,
                    "effective_tools": ["wait_agent"],
                    "agent_catalog_visible": false,
                    "visible_agents": [],
                    "skill_catalog_visible": false,
                    "project_instructions_visible": false,
                    "project_instructions_role": null
                })),
            })
            .expect("prefix");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        let body = &value["last_provider_request"]["body"];
        let body_text = serde_json::to_string(body).expect("body text");
        assert_eq!(body_text.matches(final_answer).count(), 1);
        let wait_tool_result = body["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| {
                message.get("role").and_then(Value::as_str) == Some("tool")
                    && message.get("tool_call_id").and_then(Value::as_str) == Some("call-wait")
            })
            .expect("wait tool result");
        assert!(
            !wait_tool_result
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains(final_answer)
        );
    }
}
