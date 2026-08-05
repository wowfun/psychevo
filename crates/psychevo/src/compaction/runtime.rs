use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use psychevo_agent_core::{
    AssistantBlock, Message, ToolCallBlock, UserContentBlock, user_text_message,
};
use psychevo_ai::{
    FinishReason, FinishReasonKind, GenerationEvent, GenerationOutcome, LanguageModel,
    LanguageRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{load_run_config, resolve_compression_config, resolve_run_provider};
use crate::context::prune_context;
use crate::context_usage::ContextSnapshot;
use crate::error::{Error, Result};
use crate::paths::canonical_cwd;
use crate::prompt_templates;
use crate::state::StateRuntime;
use crate::store::{SessionCompactionInput, SessionCompactionRecord, SessionMessageRecord};
use crate::thread_lineage::side_conversation_session_source;
use crate::types::{ImageInput, RunMode, RunOptions};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) const SUMMARY_TOOL_TEXT_LIMIT: usize = 4_000;
pub(crate) const SUMMARY_MESSAGE_TEXT_LIMIT: usize = 12_000;
pub(crate) const MIN_SUMMARIZED_MESSAGES: usize = 2;
pub(crate) const DEFAULT_COMPACTION_INPUT_TOKENS: u64 = 32_768;
pub(crate) const HARD_COMPACTION_INPUT_TOKENS: u64 = 65_536;
pub(crate) const MAX_COMPACTION_OUTPUT_TOKENS: u64 = 4_096;
pub(crate) const MIN_COMPACTION_OUTPUT_TOKENS: u64 = 512;
pub(crate) const COMPACTION_INPUT_RESERVE_TOKENS: u64 = 1_024;

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
    pub cwd: PathBuf,
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
    pub cwd: PathBuf,
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
    let cwd = canonical_cwd(&options.cwd)?;
    let store = options.state.clone();
    let summary = store
        .session_summary(&options.session)
        .await?
        .ok_or_else(|| Error::Message(format!("session not found: {}", options.session)))?;
    if side_conversation_session_source(&summary.source) {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "compaction is unavailable for side chats",
        ));
    }

    let previous = store.latest_valid_session_compaction(&summary.id).await?;
    let records = store
        .load_message_records_from(
            &summary.id,
            previous
                .as_ref()
                .map(|record| record.first_kept_session_seq),
        )
        .await?;
    if records.len() < MIN_SUMMARIZED_MESSAGES + 1 {
        return Ok(skipped_result(
            &summary.id,
            options.reason,
            "not enough messages to compact",
        ));
    }

    let run_options = compaction_run_options(&options, &summary.provider, &summary.model, &cwd);
    let loaded = load_run_config(&run_options, &cwd)?;
    let hook_runtime = crate::hooks::hook_runtime_config_from_options(&run_options, &cwd)
        .map(|config| crate::agents::build_hook_runtime(None, Vec::new(), config, &cwd))?;
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
    if let Some(runtime) = &hook_runtime {
        let pre = runtime
            .run_pre_compact(&json!({
                "session_id": summary.id.clone(),
                "reason": options.reason.as_str(),
                "trigger": options.reason.as_str(),
                "cwd": cwd.clone(),
                "first_kept_session_seq": first_kept_session_seq,
                "tokens_before": preparation.tokens_before,
                "tokens_after_without_summary": preparation.tokens_after_without_summary,
            }))
            .await;
        if let Some(reason) = pre.stop_reason {
            return Ok(skipped_result(&summary.id, options.reason, &reason));
        }
    }

    let compression = resolve_compression_config(&run_options, &loaded, &current)?;

    let provider = crate::run::generation_provider(
        compression.provider.base_url.clone(),
        compression.provider.api_key.clone(),
        compression.provider.provider.clone(),
        compression.provider.inference_idle_timeout_secs,
    )?
    .language_model(compression.provider.model.clone())
    .map_err(|error| Error::Config(error.to_string()))?;
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
    let record = store
        .append_session_compaction(SessionCompactionInput {
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
        })
        .await?;
    let post_compact_warning = if let Some(runtime) = &hook_runtime {
        let post = runtime
            .run_post_compact(&json!({
                "session_id": summary.id.clone(),
                "reason": options.reason.as_str(),
                "trigger": options.reason.as_str(),
                "cwd": cwd.clone(),
                "checkpoint_id": record.id,
                "first_kept_session_seq": record.first_kept_session_seq,
                "tokens_before": record.tokens_before,
                "tokens_after": record.tokens_after,
            }))
            .await;
        (!post.response.diagnostics.is_empty()).then(|| {
            format!(
                "PostCompact warnings: {}",
                post.response.diagnostics.join("; ")
            )
        })
    } else {
        None
    };
    Ok(CompactionResult {
        session_id: summary.id,
        compacted: true,
        reason: options.reason.as_str().to_string(),
        message: post_compact_warning
            .map(|warning| format!("context compacted; {warning}"))
            .unwrap_or_else(|| "context compacted".to_string()),
        checkpoint_id: Some(record.id),
        first_kept_session_seq: Some(record.first_kept_session_seq),
        tokens_before: record.tokens_before,
        tokens_after: record.tokens_after,
        summary: Some(record.summary_text),
        summary_provider: Some(record.summary_provider),
        summary_model: Some(record.summary_model),
    })
}

pub(crate) async fn load_projected_messages(
    store: &StateRuntime,
    session_id: &str,
    max_context_messages: Option<usize>,
) -> Result<Vec<Message>> {
    let Some(compaction) = store.latest_valid_session_compaction(session_id).await? else {
        let records = store.load_message_records(session_id).await?;
        return Ok(prune_context(
            records.into_iter().map(|record| record.message).collect(),
            max_context_messages,
        ));
    };
    let records = store
        .load_message_records_from(session_id, Some(compaction.first_kept_session_seq))
        .await?;
    let mut messages = vec![compaction_summary_message(&compaction)];
    messages.extend(records.into_iter().map(|record| record.message));
    Ok(prune_context(messages, max_context_messages))
}

pub fn auto_compaction_due_for_snapshot(
    options: &AutoCompactionCheckOptions,
    snapshot: &ContextSnapshot,
) -> Result<bool> {
    if snapshot.context_limit.unwrap_or_default() == 0 {
        return Ok(false);
    }
    let cwd = canonical_cwd(&options.cwd)?;
    let run_options = auto_compaction_check_run_options(options, snapshot, &cwd);
    let loaded = load_run_config(&run_options, &cwd)?;
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
    provider: LanguageModel,
    resolved: &crate::config::ResolvedRunProvider,
    previous: Option<&SessionCompactionRecord>,
    messages: &[SessionMessageRecord],
    instructions: Option<&str>,
) -> Result<String> {
    let budget = compaction_generation_budget(resolved)?;
    let units = rendered_summary_units(messages);
    let mut previous_summary = previous.map(|record| record.summary_text.clone());
    let mut unit_index = 0usize;
    let mut generated_any = false;
    while unit_index < units.len() || !generated_any {
        let fixed_user_prompt =
            summary_user_prompt_from_rendered(previous_summary.as_deref(), "", instructions);
        let system_tokens = estimate_text_tokens(summary_system_prompt());
        let fixed_user_chars = fixed_user_prompt.chars().count() as u64;
        let fixed_tokens =
            system_tokens.saturating_add(estimate_char_count_tokens(fixed_user_chars));
        validate_fixed_summary_input(fixed_tokens, budget.input_tokens)?;
        let chunk_start = unit_index;
        let mut chunk_chars = 0u64;
        while unit_index < units.len() {
            let candidate_chars = fixed_user_chars
                .saturating_add(chunk_chars)
                .saturating_add(units[unit_index].chars);
            let candidate_tokens =
                system_tokens.saturating_add(estimate_char_count_tokens(candidate_chars));
            if candidate_tokens <= budget.input_tokens {
                chunk_chars = chunk_chars.saturating_add(units[unit_index].chars);
                unit_index += 1;
                continue;
            }
            if unit_index == chunk_start {
                return Err(Error::Message(format!(
                    "compaction atomic message unit at session_seq {} exceeds the {} token input budget",
                    units[unit_index].first_session_seq, budget.input_tokens
                )));
            }
            break;
        }
        let mut chunk = String::with_capacity(chunk_chars.min(usize::MAX as u64) as usize);
        for unit in &units[chunk_start..unit_index] {
            chunk.push_str(&unit.text);
        }
        previous_summary = Some(
            generate_summary_chunk(
                &provider,
                resolved,
                previous_summary.as_deref(),
                &chunk,
                instructions,
                budget.output_tokens,
            )
            .await?,
        );
        generated_any = true;
    }
    previous_summary
        .ok_or_else(|| Error::Message("compaction did not produce a rolling summary".to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactionGenerationBudget {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}

pub(crate) fn compaction_generation_budget(
    resolved: &crate::config::ResolvedRunProvider,
) -> Result<CompactionGenerationBudget> {
    let context_tokens = resolved
        .context_limit
        .or(resolved.metadata.limits.context)
        .unwrap_or(DEFAULT_COMPACTION_INPUT_TOKENS)
        .min(HARD_COMPACTION_INPUT_TOKENS);
    let model_input_tokens = resolved
        .metadata
        .limits
        .input
        .unwrap_or(context_tokens)
        .min(context_tokens)
        .min(HARD_COMPACTION_INPUT_TOKENS);
    let output_tokens = resolved
        .metadata
        .limits
        .output
        .unwrap_or(MAX_COMPACTION_OUTPUT_TOKENS)
        .min(MAX_COMPACTION_OUTPUT_TOKENS)
        .min(context_tokens / 4);
    if output_tokens < MIN_COMPACTION_OUTPUT_TOKENS {
        return Err(Error::Message(format!(
            "compaction output budget is {output_tokens} tokens; at least {MIN_COMPACTION_OUTPUT_TOKENS} are required"
        )));
    }
    let input_tokens = model_input_tokens
        .checked_sub(output_tokens.saturating_add(COMPACTION_INPUT_RESERVE_TOKENS))
        .filter(|budget| *budget > 0)
        .ok_or_else(|| {
            Error::Message(format!(
                "compaction input limit {model_input_tokens} cannot reserve {output_tokens} output tokens plus {COMPACTION_INPUT_RESERVE_TOKENS} safety tokens"
            ))
        })?;
    Ok(CompactionGenerationBudget {
        input_tokens,
        output_tokens,
    })
}

fn validate_fixed_summary_input(fixed_tokens: u64, input_tokens: u64) -> Result<()> {
    if fixed_tokens <= input_tokens {
        return Ok(());
    }
    Err(Error::Message(format!(
        "compaction fixed prompt, manual instructions, and rolling summary require {fixed_tokens} tokens but the input budget is {input_tokens}"
    )))
}

fn estimate_char_count_tokens(chars: u64) -> u64 {
    (chars.saturating_add(3) / 4).max(1)
}

pub(crate) fn atomic_summary_units(
    messages: &[SessionMessageRecord],
) -> Vec<Vec<SessionMessageRecord>> {
    let mut ordered = messages.to_vec();
    ordered.sort_by_key(|record| record.session_seq);
    let mut result_indices = BTreeMap::<String, usize>::new();
    for (index, record) in ordered.iter().enumerate() {
        if let Message::ToolResult { tool_call_id, .. } = &record.message {
            result_indices.insert(tool_call_id.clone(), index);
        }
    }
    let mut units = Vec::new();
    let mut start = 0usize;
    while start < ordered.len() {
        let mut end = start;
        let mut scan = start;
        while scan <= end {
            for tool_call_id in assistant_tool_call_ids(&ordered[scan].message) {
                if let Some(result_index) = result_indices.get(&tool_call_id) {
                    end = end.max(*result_index);
                }
            }
            scan += 1;
        }
        units.push(ordered[start..=end].to_vec());
        start = end + 1;
    }
    units
}

#[derive(Debug)]
struct RenderedSummaryUnit {
    first_session_seq: i64,
    text: String,
    chars: u64,
}

#[cfg(test)]
thread_local! {
    static SUMMARY_RECORD_RENDER_COUNT: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_summary_record_render_count() {
    SUMMARY_RECORD_RENDER_COUNT.set(0);
}

#[cfg(test)]
fn summary_record_render_count() -> usize {
    SUMMARY_RECORD_RENDER_COUNT.get()
}

fn rendered_summary_units(messages: &[SessionMessageRecord]) -> Vec<RenderedSummaryUnit> {
    atomic_summary_units(messages)
        .into_iter()
        .map(|unit| {
            let first_session_seq = unit
                .first()
                .map(|record| record.session_seq)
                .unwrap_or_default();
            let mut text = String::new();
            for record in unit {
                text.push_str(&render_summary_record(&record));
            }
            RenderedSummaryUnit {
                first_session_seq,
                chars: text.chars().count() as u64,
                text,
            }
        })
        .collect()
}

fn render_summary_record(record: &SessionMessageRecord) -> String {
    #[cfg(test)]
    SUMMARY_RECORD_RENDER_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    format!(
        "\n[session_seq={} role={}]\n{}\n",
        record.session_seq,
        record.message.role(),
        message_summary_text(&record.message)
    )
}

async fn generate_summary_chunk(
    provider: &LanguageModel,
    resolved: &crate::config::ResolvedRunProvider,
    previous_summary: Option<&str>,
    messages_text: &str,
    instructions: Option<&str>,
    output_tokens: u64,
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
    let request = LanguageRequest {
        messages: vec![
            psychevo_ai::Message::system(summary_system_prompt()),
            psychevo_ai::Message::user(summary_user_prompt_from_rendered(
                previous_summary,
                messages_text,
                instructions,
            )),
        ],
        tools: Vec::new(),
        extensions: BTreeMap::from([("psychevo".to_string(), metadata)]),
        settings: psychevo_ai::LanguageSettings {
            max_output_tokens: Some(output_tokens),
            ..Default::default()
        },
        ..LanguageRequest::default()
    };
    let mut stream = provider.stream(request);
    let mut text = String::new();
    while let Some(event) = stream.next_event().await {
        match event.map_err(|err| Error::Message(format!("summary provider failed: {err}")))? {
            GenerationEvent::TextDelta { delta, .. } => text.push_str(&delta),
            GenerationEvent::Resync { snapshot, .. } => {
                text = snapshot
                    .assistant
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        psychevo_ai::AssistantContent::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
            }
            _ => {}
        }
    }
    let output = stream
        .finish()
        .await
        .map_err(|err| Error::Message(format!("summary provider failed: {err}")))?;
    validate_summary_completion(output.outcome, output.finish_reason.as_ref())?;
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Message(
            "summary provider returned an empty compaction summary".to_string(),
        ));
    }
    Ok(text.to_string())
}

fn validate_summary_completion(
    outcome: GenerationOutcome,
    finish_reason: Option<&FinishReason>,
) -> Result<()> {
    if outcome == GenerationOutcome::Completed
        && finish_reason.is_some_and(|reason| reason.kind == FinishReasonKind::Stop)
    {
        return Ok(());
    }
    Err(Error::Message(format!(
        "summary provider did not complete normally: outcome={outcome:?}, finish_reason={:?}",
        finish_reason.map(|reason| reason.kind)
    )))
}

pub(crate) fn summary_system_prompt() -> &'static str {
    prompt_templates::compaction_summary_system()
}

#[cfg(test)]
pub(crate) fn summary_user_prompt_text(
    previous_summary: Option<&str>,
    messages: &[SessionMessageRecord],
    instructions: Option<&str>,
) -> String {
    let mut messages_text = String::new();
    for record in messages {
        messages_text.push_str(&render_summary_record(record));
    }
    summary_user_prompt_from_rendered(previous_summary, &messages_text, instructions)
}

fn summary_user_prompt_from_rendered(
    previous_summary: Option<&str>,
    messages_text: &str,
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
    let previous_summary_section = if let Some(previous_summary) = previous_summary {
        prompt_templates::compaction_summary_previous_section(&redact_secrets(previous_summary))
    } else {
        String::new()
    };
    prompt_templates::compaction_summary_user(
        &manual_focus_section,
        &previous_summary_section,
        messages_text,
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
                AssistantBlock::ProviderTool(call) => {
                    Some(format!("Hosted {} ({})", call.name, call.status))
                }
                AssistantBlock::Source {
                    source: psychevo_ai::AssistantSource::UrlCitation(source),
                } => Some(format!("Source: {}", source.url)),
                AssistantBlock::Source {
                    source: psychevo_ai::AssistantSource::Image(source),
                } => Some(format!("Image source: {}", source.source_website_url)),
                AssistantBlock::Source {
                    source: psychevo_ai::AssistantSource::Provider { .. },
                } => None,
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
    cwd: &std::path::Path,
) -> RunOptions {
    RunOptions {
        state: options.state.clone(),
        cwd: cwd.to_path_buf(),
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
        sandbox_override: None,
        model: options
            .model
            .clone()
            .or_else(|| Some(format!("{}/{}", snapshot.provider, snapshot.model))),
        reasoning_effort: options.reasoning_effort.clone(),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_runtime: None,
        workspace_mutations: None,
        runtime_tools: Vec::new(),
    }
}

pub(crate) fn compaction_run_options(
    options: &CompactSessionOptions,
    session_provider: &str,
    session_model: &str,
    cwd: &std::path::Path,
) -> RunOptions {
    RunOptions {
        state: options.state.clone(),
        cwd: cwd.to_path_buf(),
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
        sandbox_override: None,
        model: options
            .model
            .clone()
            .or_else(|| Some(format!("{session_provider}/{session_model}"))),
        reasoning_effort: options.reasoning_effort.clone(),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_runtime: None,
        workspace_mutations: None,
        runtime_tools: Vec::new(),
    }
}
