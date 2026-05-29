use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use psychevo_agent_core::{
    AssistantBlock, ControlHandle, Message, ToolCallBlock, UserContentBlock, user_text_message,
};
use psychevo_ai::{
    GenerationProvider, GenerationRequest, ModelTarget, OpenAiChatProvider, Outcome, StreamEvent,
    ToolDeclaration,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{load_run_config, resolve_compression_config, resolve_run_provider};
use crate::context::prune_context;
use crate::context_usage::ContextSnapshot;
use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::prompt_templates;
use crate::state_runtime::StateRuntime;
use crate::store::{
    SessionCompactionInput, SessionCompactionRecord, SessionMessageRecord, SqliteStore,
};
use crate::types::{ImageInput, RunMode, RunOptions};

pub(crate) const SUMMARY_TOOL_TEXT_LIMIT: usize = 4_000;
pub(crate) const SUMMARY_MESSAGE_TEXT_LIMIT: usize = 12_000;
pub(crate) const MIN_SUMMARIZED_MESSAGES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionReason {
    Manual,
    AutoThreshold,
    Overflow,
}

impl CompactionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AutoThreshold => "auto_threshold",
            Self::Overflow => "overflow",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactSessionOptions {
    pub state: StateRuntime,
    pub workdir: PathBuf,
    pub session: String,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub reason: CompactionReason,
    pub instructions: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct AutoCompactionCheckOptions {
    pub state: StateRuntime,
    pub workdir: PathBuf,
    pub session: String,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionResult {
    pub session_id: String,
    pub compacted: bool,
    pub reason: String,
    pub message: String,
    pub checkpoint_id: Option<i64>,
    pub first_kept_session_seq: Option<i64>,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary: Option<String>,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
}

pub async fn compact_session(options: CompactSessionOptions) -> Result<CompactionResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    let store = options.state.store().clone();
    let summary = store
        .session_summary(&options.session)?
        .ok_or_else(|| Error::Message(format!("session not found: {}", options.session)))?;
    if summary.source == "tui-side" {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "compaction is unavailable for side sessions",
        ));
    }

    let run_options = compaction_run_options(&options, &summary.provider, &summary.model, &workdir);
    let loaded = load_run_config(&run_options, &workdir)?;
    let compression_config = loaded.config.compression.clone();
    if !compression_config.enabled {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "compaction is disabled",
        ));
    }
    if options.reason != CompactionReason::Manual && !compression_config.auto {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "automatic compaction is disabled",
        ));
    }
    let current = resolve_run_provider(&run_options, &loaded)?;

    let records = store.load_message_records(&summary.id)?;
    if records.len() < MIN_SUMMARIZED_MESSAGES + 1 {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "not enough messages to compact",
        ));
    }
    let previous = store.latest_valid_session_compaction(&summary.id)?;
    let preparation = prepare_compaction(
        &records,
        previous.as_ref(),
        compression_config.keep_recent_tokens,
    )?;
    if !options.force
        && !compaction_due(
            preparation.tokens_before,
            current.context_limit,
            compression_config.threshold_percent,
            compression_config.reserve_tokens,
        )
    {
        return Ok(CompactionResult {
            session_id: summary.id,
            compacted: false,
            reason: options.reason.as_str().to_string(),
            message: "context is below compaction threshold".to_string(),
            checkpoint_id: None,
            first_kept_session_seq: preparation.first_kept_session_seq,
            tokens_before: Some(preparation.tokens_before),
            tokens_after: Some(preparation.tokens_after_without_summary),
            summary: None,
            summary_provider: None,
            summary_model: None,
        });
    }
    let Some(first_kept_session_seq) = preparation.first_kept_session_seq else {
        return Ok(CompactionResult {
            session_id: summary.id,
            compacted: false,
            reason: options.reason.as_str().to_string(),
            message: "no safe compaction boundary found".to_string(),
            checkpoint_id: None,
            first_kept_session_seq: None,
            tokens_before: Some(preparation.tokens_before),
            tokens_after: Some(preparation.tokens_after_without_summary),
            summary: None,
            summary_provider: None,
            summary_model: None,
        });
    };

    let compression = resolve_compression_config(&run_options, &loaded, &current)?;

    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        compression.provider.base_url.clone(),
        compression.provider.api_key.clone(),
        compression.provider.provider.clone(),
    ));
    let summary_text = generate_summary(
        provider,
        &compression.provider,
        previous.as_ref(),
        &preparation.messages_to_summarize,
        options.instructions.as_deref(),
    )
    .await?;
    let summary_tokens = estimate_text_tokens(&summary_text);
    let tokens_after = preparation
        .tokens_after_without_summary
        .saturating_add(summary_tokens);
    let record = store.append_session_compaction(SessionCompactionInput {
        session_id: summary.id.clone(),
        reason: options.reason.as_str().to_string(),
        summary_text: summary_text.clone(),
        first_kept_session_seq,
        created_after_session_seq: records
            .last()
            .map(|record| record.session_seq)
            .unwrap_or(first_kept_session_seq),
        tokens_before: Some(preparation.tokens_before),
        tokens_after: Some(tokens_after),
        summary_provider: compression.provider.provider.clone(),
        summary_model: compression.provider.model.clone(),
        instructions: options.instructions.clone(),
        metadata: Some(json!({
            "model_configured": compression.model_configured,
            "threshold_percent": compression_config.threshold_percent,
            "reserve_tokens": compression_config.reserve_tokens,
            "keep_recent_tokens": compression_config.keep_recent_tokens,
            "previous_compaction_id": previous.as_ref().map(|record| record.id),
        })),
    })?;
    Ok(CompactionResult {
        session_id: summary.id,
        compacted: true,
        reason: options.reason.as_str().to_string(),
        message: "context compacted".to_string(),
        checkpoint_id: Some(record.id),
        first_kept_session_seq: Some(record.first_kept_session_seq),
        tokens_before: record.tokens_before,
        tokens_after: record.tokens_after,
        summary: Some(record.summary_text),
        summary_provider: Some(record.summary_provider),
        summary_model: Some(record.summary_model),
    })
}

pub(crate) fn load_projected_messages(
    store: &SqliteStore,
    session_id: &str,
    max_context_messages: Option<usize>,
) -> Result<Vec<Message>> {
    let records = store.load_message_records(session_id)?;
    let Some(compaction) = store.latest_valid_session_compaction(session_id)? else {
        return Ok(prune_context(
            records.into_iter().map(|record| record.message).collect(),
            max_context_messages,
        ));
    };
    let mut messages = vec![compaction_summary_message(&compaction)];
    messages.extend(
        records
            .into_iter()
            .filter(|record| record.session_seq >= compaction.first_kept_session_seq)
            .map(|record| record.message),
    );
    Ok(prune_context(messages, max_context_messages))
}

pub fn auto_compaction_due_for_snapshot(
    options: &AutoCompactionCheckOptions,
    snapshot: &ContextSnapshot,
) -> Result<bool> {
    if snapshot.context_limit.unwrap_or_default() == 0 {
        return Ok(false);
    }
    let workdir = canonical_workdir(&options.workdir)?;
    let run_options = auto_compaction_check_run_options(options, snapshot, &workdir);
    let loaded = load_run_config(&run_options, &workdir)?;
    let compression_config = loaded.config.compression;
    if !compression_config.enabled || !compression_config.auto {
        return Ok(false);
    }
    Ok(compaction_due(
        snapshot.total.tokens,
        snapshot.context_limit,
        compression_config.threshold_percent,
        compression_config.reserve_tokens,
    ))
}

pub(crate) fn compaction_due(
    tokens: u64,
    context_limit: Option<u64>,
    threshold_percent: f64,
    reserve_tokens: u64,
) -> bool {
    let Some(limit) = context_limit else {
        return false;
    };
    if limit == 0 {
        return false;
    }
    let percent = tokens as f64 / limit as f64 * 100.0;
    percent >= threshold_percent || limit.saturating_sub(tokens) <= reserve_tokens
}

pub(crate) fn is_context_overflow_error(error: &Error) -> bool {
    let text = error.to_string().to_lowercase();
    [
        "context length",
        "context_length",
        "maximum context",
        "context window",
        "too many tokens",
        "input is too long",
        "token limit",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[derive(Debug, Clone)]
pub(crate) struct CompactionPreparation {
    pub(crate) first_kept_session_seq: Option<i64>,
    pub(crate) messages_to_summarize: Vec<SessionMessageRecord>,
    pub(crate) tokens_before: u64,
    pub(crate) tokens_after_without_summary: u64,
}

pub(crate) fn prepare_compaction(
    records: &[SessionMessageRecord],
    previous: Option<&SessionCompactionRecord>,
    keep_recent_tokens: u64,
) -> Result<CompactionPreparation> {
    let tokens_before = projection_tokens(previous, records);
    let Some(first_kept_index) = choose_first_kept_index(records, keep_recent_tokens) else {
        return Ok(CompactionPreparation {
            first_kept_session_seq: None,
            messages_to_summarize: Vec::new(),
            tokens_before,
            tokens_after_without_summary: tokens_before,
        });
    };
    if first_kept_index == 0 {
        return Ok(CompactionPreparation {
            first_kept_session_seq: None,
            messages_to_summarize: Vec::new(),
            tokens_before,
            tokens_after_without_summary: tokens_before,
        });
    }
    let first_kept_session_seq = records[first_kept_index].session_seq;
    let summarize_start_seq = previous
        .map(|record| record.first_kept_session_seq)
        .unwrap_or(i64::MIN);
    let messages_to_summarize = records
        .iter()
        .filter(|record| {
            record.session_seq >= summarize_start_seq && record.session_seq < first_kept_session_seq
        })
        .cloned()
        .collect::<Vec<_>>();
    if messages_to_summarize.len() < MIN_SUMMARIZED_MESSAGES {
        return Ok(CompactionPreparation {
            first_kept_session_seq: None,
            messages_to_summarize: Vec::new(),
            tokens_before,
            tokens_after_without_summary: tokens_before,
        });
    }
    let tokens_after_without_summary = records
        .iter()
        .filter(|record| record.session_seq >= first_kept_session_seq)
        .map(|record| estimate_message_tokens(&record.message))
        .sum();
    Ok(CompactionPreparation {
        first_kept_session_seq: Some(first_kept_session_seq),
        messages_to_summarize,
        tokens_before,
        tokens_after_without_summary,
    })
}

pub(crate) fn choose_first_kept_index(
    records: &[SessionMessageRecord],
    keep_recent_tokens: u64,
) -> Option<usize> {
    if records.is_empty() {
        return None;
    }
    let mut tokens = 0u64;
    let mut first = records.len();
    for (index, record) in records.iter().enumerate().rev() {
        let message_tokens = estimate_message_tokens(&record.message);
        if first < records.len() && tokens.saturating_add(message_tokens) > keep_recent_tokens {
            break;
        }
        tokens = tokens.saturating_add(message_tokens);
        first = index;
    }
    if first == records.len() {
        first = records.len().saturating_sub(1);
    }
    if let Some(latest_user) = records
        .iter()
        .rposition(|record| matches!(record.message, Message::User { .. }))
    {
        first = first.min(latest_user);
    }
    while first > 0 && !matches!(records[first].message, Message::User { .. }) {
        first -= 1;
    }
    Some(adjust_for_tool_pairs(records, first))
}

pub(crate) fn adjust_for_tool_pairs(records: &[SessionMessageRecord], mut first: usize) -> usize {
    let mut tool_call_index = BTreeMap::<String, usize>::new();
    for (index, record) in records.iter().enumerate() {
        for id in assistant_tool_call_ids(&record.message) {
            tool_call_index.insert(id, index);
        }
    }
    loop {
        let mut changed = false;
        let retained_tool_results = records[first..]
            .iter()
            .filter_map(|record| match &record.message {
                Message::ToolResult { tool_call_id, .. } => Some(tool_call_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for tool_call_id in retained_tool_results {
            if let Some(index) = tool_call_index.get(&tool_call_id)
                && *index < first
            {
                first = *index;
                changed = true;
            }
        }
        while first > 0 && matches!(records[first].message, Message::ToolResult { .. }) {
            first -= 1;
            changed = true;
        }
        if !changed {
            return first;
        }
    }
}

pub(crate) fn assistant_tool_call_ids(message: &Message) -> Vec<String> {
    let Message::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::ToolCall(call) => Some(call.id.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) async fn generate_summary(
    provider: Arc<dyn GenerationProvider>,
    resolved: &crate::config::ResolvedRunProvider,
    previous: Option<&SessionCompactionRecord>,
    messages: &[SessionMessageRecord],
    instructions: Option<&str>,
) -> Result<String> {
    let mut metadata = json!({
        "model_metadata": resolved.metadata.public_json(),
    });
    if let Some(effort) = &resolved.reasoning_effort
        && let Some(object) = metadata.as_object_mut()
    {
        object.insert(
            "reasoning_effort".to_string(),
            Value::String(effort.clone()),
        );
    }
    let request = GenerationRequest {
        model: ModelTarget {
            provider: resolved.provider.clone(),
            model: resolved.model.clone(),
        },
        messages: vec![
            json!({
                "role": "system",
                "content": summary_system_prompt(),
            }),
            json!({
                "role": "user",
                "content": summary_user_prompt(previous, messages, instructions),
            }),
        ],
        tools: Vec::<ToolDeclaration>::new(),
        metadata,
    };
    let (_handle, receivers) = ControlHandle::new();
    let mut stream = provider
        .stream(request, receivers.abort_signal())
        .await
        .map_err(|err| Error::Message(format!("summary provider failed: {err}")))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|err| Error::Message(format!("summary provider failed: {err}")))? {
            StreamEvent::TextDelta { text: delta } => text.push_str(&delta),
            StreamEvent::Done { outcome, .. } if outcome != Outcome::Normal => {
                return Err(Error::Message(format!(
                    "summary provider ended with {}",
                    outcome.as_str()
                )));
            }
            _ => {}
        }
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Message(
            "summary provider returned an empty compaction summary".to_string(),
        ));
    }
    Ok(text.to_string())
}

pub(crate) fn summary_system_prompt() -> &'static str {
    prompt_templates::compaction_summary_system()
}

pub(crate) fn summary_user_prompt(
    previous: Option<&SessionCompactionRecord>,
    messages: &[SessionMessageRecord],
    instructions: Option<&str>,
) -> String {
    let manual_focus_section = if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt_templates::compaction_summary_manual_focus_section(instructions)
    } else {
        String::new()
    };
    let previous_summary_section = if let Some(previous) = previous {
        prompt_templates::compaction_summary_previous_section(&redact_secrets(
            &previous.summary_text,
        ))
    } else {
        String::new()
    };
    let mut messages_text = String::new();
    for record in messages {
        messages_text.push_str(&format!(
            "\n[session_seq={} role={}]\n{}\n",
            record.session_seq,
            record.message.role(),
            message_summary_text(&record.message)
        ));
    }
    prompt_templates::compaction_summary_user(
        &manual_focus_section,
        &previous_summary_section,
        &messages_text,
    )
}

pub(crate) fn message_summary_text(message: &Message) -> String {
    let raw = match message {
        Message::User { content, .. } => content
            .iter()
            .map(user_block_summary)
            .collect::<Vec<_>>()
            .join("\n"),
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|block| match block {
                AssistantBlock::Text { text } => Some(text.clone()),
                AssistantBlock::Reasoning { .. } => None,
                AssistantBlock::ToolCall(call) => Some(tool_call_summary(call)),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Message::ToolResult {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let status = if *is_error { "error" } else { "ok" };
            format!(
                "tool_result {tool_name} ({status}):\n{}",
                truncate_text(content, SUMMARY_TOOL_TEXT_LIMIT)
            )
        }
    };
    truncate_text(&redact_secrets(&raw), SUMMARY_MESSAGE_TEXT_LIMIT)
}

pub(crate) fn user_block_summary(block: &UserContentBlock) -> String {
    match block {
        UserContentBlock::Text(text) => text.text.clone(),
        UserContentBlock::LocalImage(image) => {
            format!("[local image: {}]", image.path.display())
        }
        UserContentBlock::ImageUrl(image) => format!("[image url: {}]", image.url),
    }
}

pub(crate) fn tool_call_summary(call: &ToolCallBlock) -> String {
    format!(
        "tool_call {} {}",
        call.name,
        truncate_text(&call.arguments_json, SUMMARY_TOOL_TEXT_LIMIT)
    )
}

pub(crate) fn compaction_summary_message(record: &SessionCompactionRecord) -> Message {
    user_text_message(format!(
        "{}\n\n{}",
        prompt_templates::compaction_summary_prefix(),
        record.summary_text
    ))
}

pub(crate) fn projection_tokens(
    previous: Option<&SessionCompactionRecord>,
    records: &[SessionMessageRecord],
) -> u64 {
    match previous {
        Some(previous) => {
            estimate_text_tokens(&previous.summary_text)
                + records
                    .iter()
                    .filter(|record| record.session_seq >= previous.first_kept_session_seq)
                    .map(|record| estimate_message_tokens(&record.message))
                    .sum::<u64>()
        }
        None => records
            .iter()
            .map(|record| estimate_message_tokens(&record.message))
            .sum(),
    }
}

pub(crate) fn estimate_message_tokens(message: &Message) -> u64 {
    serde_json::to_string(message)
        .map(|value| estimate_text_tokens(&value))
        .unwrap_or(0)
}

pub(crate) fn estimate_text_tokens(text: &str) -> u64 {
    ((text.chars().count() as u64).saturating_add(3) / 4).max(1)
}

pub(crate) fn truncate_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut output = text.chars().take(limit).collect::<String>();
    output.push_str("\n[truncated]");
    output
}

pub(crate) fn redact_secrets(text: &str) -> String {
    text.lines()
        .map(|line| {
            let lower = line.to_lowercase();
            if [
                "api_key",
                "apikey",
                "authorization:",
                "bearer ",
                "secret",
                "password",
                "token=",
                "access_token",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            {
                "[redacted secret-like line]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn skipped_result(
    session_id: &str,
    reason: CompactionReason,
    message: &str,
) -> CompactionResult {
    CompactionResult {
        session_id: session_id.to_string(),
        compacted: false,
        reason: reason.as_str().to_string(),
        message: message.to_string(),
        checkpoint_id: None,
        first_kept_session_seq: None,
        tokens_before: None,
        tokens_after: None,
        summary: None,
        summary_provider: None,
        summary_model: None,
    }
}

pub(crate) fn auto_compaction_check_run_options(
    options: &AutoCompactionCheckOptions,
    snapshot: &ContextSnapshot,
    workdir: &std::path::Path,
) -> RunOptions {
    RunOptions {
        state: options.state.clone(),
        workdir: workdir.to_path_buf(),
        snapshot_root: None,
        session: Some(options.session.clone()),
        continue_latest: false,
        prompt: "check automatic context compaction".to_string(),
        image_inputs: Vec::<ImageInput>::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        model: options
            .model
            .clone()
            .or_else(|| Some(format!("{}/{}", snapshot.provider, snapshot.model))),
        reasoning_effort: options.reasoning_effort.clone(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: None,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
    }
}

pub(crate) fn compaction_run_options(
    options: &CompactSessionOptions,
    session_provider: &str,
    session_model: &str,
    workdir: &std::path::Path,
) -> RunOptions {
    RunOptions {
        state: options.state.clone(),
        workdir: workdir.to_path_buf(),
        snapshot_root: None,
        session: Some(options.session.clone()),
        continue_latest: false,
        prompt: "compact session context".to_string(),
        image_inputs: Vec::<ImageInput>::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        model: options
            .model
            .clone()
            .or_else(|| Some(format!("{session_provider}/{session_model}"))),
        reasoning_effort: options.reasoning_effort.clone(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: None,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use crate::context_usage::{ContextScope, ContextTokenizer, ContextTotal};
    use psychevo_agent_core::{AssistantBlock, ToolCallBlock, now_ms};
    use std::fs;
    use std::path::PathBuf;

    fn record(session_seq: i64, message: Message) -> SessionMessageRecord {
        SessionMessageRecord {
            session_seq,
            message,
        }
    }

    fn previous_compaction(first_kept_session_seq: i64) -> SessionCompactionRecord {
        SessionCompactionRecord {
            id: 1,
            session_id: "session".to_string(),
            created_at_ms: now_ms(),
            reason: "manual".to_string(),
            summary_text: "previous summary".to_string(),
            first_kept_session_seq,
            created_after_session_seq: first_kept_session_seq,
            tokens_before: Some(100),
            tokens_after: Some(50),
            summary_provider: "mock".to_string(),
            summary_model: "mock-model".to_string(),
            instructions: None,
            metadata: None,
        }
    }

    fn snapshot(tokens: u64, context_limit: Option<u64>) -> ContextSnapshot {
        ContextSnapshot {
            event_type: "context_snapshot".to_string(),
            scope: ContextScope::LastProviderRequest,
            status: "estimated".to_string(),
            session_id: Some("session".to_string()),
            provider: "mock".to_string(),
            model: "model".to_string(),
            mode: Some("default".to_string()),
            context_limit,
            tokenizer: ContextTokenizer {
                encoding: "o200k_base".to_string(),
                source: "fallback".to_string(),
                fallback: true,
            },
            total: ContextTotal {
                tokens,
                estimated_tokens: tokens,
                estimated: true,
                source: "estimate".to_string(),
                percent: context_limit.map(|limit| tokens as f64 / limit as f64 * 100.0),
            },
            categories: BTreeMap::new(),
            advice: Vec::new(),
        }
    }

    fn auto_check_options(
        db_path: PathBuf,
        workdir: PathBuf,
        psychevo_home: PathBuf,
    ) -> AutoCompactionCheckOptions {
        AutoCompactionCheckOptions {
            state: StateRuntime::open(&db_path).expect("state runtime"),
            workdir,
            session: "session".to_string(),
            config_path: None,
            model: Some("mock/model".to_string()),
            reasoning_effort: None,
            inherited_env: Some(BTreeMap::from([(
                "PSYCHEVO_HOME".to_string(),
                psychevo_home.display().to_string(),
            )])),
        }
    }

    #[test]
    fn auto_compaction_check_uses_configured_usage_threshold() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let workdir = temp.path().join("work");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::write(
            home.join("config.toml"),
            r#"[compression]
threshold_percent = 70
reserve_tokens = 5000
"#,
        )
        .expect("config");
        let options = auto_check_options(home.join("state.db"), workdir, home);

        assert!(
            !auto_compaction_due_for_snapshot(&options, &snapshot(69_000, Some(100_000)))
                .expect("below threshold")
        );
        assert!(
            auto_compaction_due_for_snapshot(&options, &snapshot(70_000, Some(100_000)))
                .expect("at threshold")
        );
        assert!(
            !auto_compaction_due_for_snapshot(&options, &snapshot(90_000, None))
                .expect("unbounded")
        );
    }

    #[test]
    fn cutpoint_preserves_latest_user() {
        let records = vec![
            record(1, user_text_message("old user")),
            record(2, user_text_message("old assistant context")),
            record(3, user_text_message("latest user task")),
        ];
        let prep = prepare_compaction(&records, None, 1).expect("prepare");
        assert_eq!(prep.first_kept_session_seq, Some(3));
    }

    #[test]
    fn cutpoint_keeps_tool_call_parent_for_retained_tool_result() {
        let call = ToolCallBlock {
            id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: json!({}),
            arguments_json: "{}".to_string(),
            arguments_error: None,
            content_index: 0,
            call_index: 0,
        };
        let records = vec![
            record(1, user_text_message("old user")),
            record(
                2,
                Message::Assistant {
                    content: vec![AssistantBlock::ToolCall(call)],
                    timestamp_ms: now_ms(),
                    finish_reason: None,
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            record(
                3,
                Message::ToolResult {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "read".to_string(),
                    content: "large result".to_string(),
                    is_error: false,
                    timestamp_ms: now_ms(),
                },
            ),
            record(4, user_text_message("latest user")),
        ];
        let first = adjust_for_tool_pairs(&records, 2);
        assert_eq!(records[first].session_seq, 2);
    }

    #[test]
    fn repeated_compaction_summarizes_from_previous_kept_boundary() {
        let records = vec![
            record(1, user_text_message("already summarized one")),
            record(2, user_text_message("already summarized two")),
            record(3, user_text_message("previously retained one")),
            record(4, user_text_message("previously retained two")),
            record(5, user_text_message("latest user task")),
        ];
        let previous = previous_compaction(3);
        let prep = prepare_compaction(&records, Some(&previous), 1).expect("prepare");

        assert_eq!(prep.first_kept_session_seq, Some(5));
        assert_eq!(
            prep.messages_to_summarize
                .iter()
                .map(|record| record.session_seq)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[test]
    fn compacted_context_projection_uses_checkpoint_without_deleting_transcript() {
        let store = SqliteStore::open(std::path::Path::new(":memory:")).expect("store");
        let session = store
            .create_session(std::path::Path::new("."))
            .expect("session");
        store
            .append_message(&session, &user_text_message("old one"))
            .expect("append");
        store
            .append_message(&session, &user_text_message("old two"))
            .expect("append");
        store
            .append_message(&session, &user_text_message("latest task"))
            .expect("append");
        store
            .append_session_compaction(SessionCompactionInput {
                session_id: session.clone(),
                reason: "manual".to_string(),
                summary_text: "summary text".to_string(),
                first_kept_session_seq: 3,
                created_after_session_seq: 3,
                tokens_before: Some(30),
                tokens_after: Some(10),
                summary_provider: "mock".to_string(),
                summary_model: "mock-model".to_string(),
                instructions: None,
                metadata: None,
            })
            .expect("checkpoint");

        let projected = load_projected_messages(&store, &session, None).expect("projected");
        assert_eq!(projected.len(), 2);
        assert!(
            serde_json::to_string(&projected[0])
                .expect("summary json")
                .contains("summary text")
        );
        assert!(
            serde_json::to_string(&projected[1])
                .expect("latest json")
                .contains("latest task")
        );
        assert_eq!(
            store.load_message_records(&session).expect("records").len(),
            3
        );
    }
}
