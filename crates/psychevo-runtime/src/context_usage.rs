use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use psychevo_agent_core::{Message, PromptInstruction};
use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, GenerationStream, ModelTarget,
    OpenAiChatTokenCount, count_openai_chat_request,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::compaction::load_projected_messages;
use crate::config::selected_configured_model;
use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::project_instructions::load_project_instructions;
use crate::skills::{
    SkillDiscoveryOptions, discover_skills, format_skills_for_prompt, resolve_skills_home,
};
use crate::store::SqliteStore;
use crate::tool_surface::tool_declarations;
use crate::tools::{coding_core_tools_for_mode, mode_instruction, skill_tools_for_mode};
use crate::types::RunMode;

const CONTEXT_SNAPSHOT_TYPE: &str = "context_snapshot";
const TOTAL_WARNING_PERCENT: f64 = 70.0;
const TOTAL_CRITICAL_PERCENT: f64 = 90.0;
const CATEGORY_ADVICE_PERCENT: f64 = 20.0;
const ADVICE_LIMIT: usize = 3;
pub const CONTEXT_BAR_MIN_CELLS: usize = 50;
pub const CONTEXT_BAR_MAX_CELLS: usize = 100;

#[derive(Debug, Clone)]
pub struct ContextOptions {
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub session: String,
    pub config_path: Option<PathBuf>,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextFormatOptions {
    pub heading: bool,
    pub bar_width: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSnapshot {
    #[serde(rename = "type")]
    pub event_type: String,
    pub scope: ContextScope,
    pub status: String,
    pub session_id: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    pub tokenizer: ContextTokenizer,
    pub total: ContextTotal,
    pub categories: BTreeMap<String, ContextCategory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advice: Vec<ContextAdvice>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextScope {
    LastProviderRequest,
    SessionEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextTokenizer {
    pub encoding: String,
    pub source: String,
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextTotal {
    pub tokens: u64,
    pub estimated_tokens: u64,
    pub estimated: bool,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextCategory {
    pub label: String,
    pub tokens: u64,
    pub estimated: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextAdvice {
    pub category: String,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub(crate) struct ContextRecorder {
    state: Arc<Mutex<ContextRecorderState>>,
}

#[derive(Debug, Default)]
struct ContextRecorderState {
    latest_started_sequence: u64,
    latest_snapshot: Option<(u64, ContextSnapshot)>,
    latest_provider_input_tokens: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveContextProfile {
    pub(crate) session_id: String,
    pub(crate) base_url: String,
    pub(crate) context_limit: Option<u64>,
    pub(crate) mode: RunMode,
}

pub(crate) struct ContextRecordingProvider {
    inner: Arc<dyn GenerationProvider>,
    recorder: ContextRecorder,
    profile: LiveContextProfile,
}

impl ContextRecordingProvider {
    pub(crate) fn new(
        inner: Arc<dyn GenerationProvider>,
        recorder: ContextRecorder,
        profile: LiveContextProfile,
    ) -> Self {
        Self {
            inner,
            recorder,
            profile,
        }
    }
}

impl GenerationProvider for ContextRecordingProvider {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, psychevo_ai::Result<GenerationStream>> {
        self.recorder
            .record_live_request(request.clone(), self.profile.clone());
        self.inner.stream(request, abort)
    }
}

impl ContextRecorder {
    pub(crate) fn record_live_request(
        &self,
        request: GenerationRequest,
        profile: LiveContextProfile,
    ) {
        let sequence = {
            let mut state = self.state.lock().expect("context recorder lock poisoned");
            state.latest_started_sequence = state.latest_started_sequence.saturating_add(1);
            state.latest_started_sequence
        };
        let recorder = self.clone();
        tokio::task::spawn_blocking(move || {
            let count = count_openai_chat_request(&request, &profile.base_url);
            let snapshot = snapshot_from_count(
                ContextScope::LastProviderRequest,
                Some(profile.session_id),
                request.model.provider.clone(),
                request.model.model.clone(),
                Some(profile.mode.as_str().to_string()),
                profile.context_limit,
                count,
            );
            recorder.finish_count(sequence, snapshot);
        });
    }

    pub(crate) fn record_provider_usage(&self, usage: Option<&Value>) {
        let Some(tokens) = usage.and_then(provider_input_tokens) else {
            return;
        };
        let mut state = self.state.lock().expect("context recorder lock poisoned");
        let sequence = state.latest_started_sequence;
        state.latest_provider_input_tokens = Some((sequence, tokens));
        if let Some((snapshot_sequence, snapshot)) = state.latest_snapshot.as_mut()
            && *snapshot_sequence == sequence
        {
            snapshot.apply_provider_input_tokens(tokens);
        }
    }

    pub(crate) fn latest_snapshot(&self) -> Option<ContextSnapshot> {
        self.state
            .lock()
            .expect("context recorder lock poisoned")
            .latest_snapshot
            .as_ref()
            .map(|(_, snapshot)| snapshot.clone())
    }

    fn finish_count(&self, sequence: u64, mut snapshot: ContextSnapshot) {
        let mut state = self.state.lock().expect("context recorder lock poisoned");
        if sequence != state.latest_started_sequence {
            return;
        }
        if let Some((usage_sequence, tokens)) = state.latest_provider_input_tokens
            && usage_sequence == sequence
        {
            snapshot.apply_provider_input_tokens(tokens);
        }
        state.latest_snapshot = Some((sequence, snapshot));
    }
}

impl ContextSnapshot {
    pub(crate) fn apply_provider_input_tokens(&mut self, tokens: u64) {
        self.total.tokens = tokens;
        self.total.estimated = false;
        self.total.source = "provider_usage".to_string();
        self.total.percent = percent(tokens, self.context_limit);
        self.status = "provider_usage".to_string();
        rebuild_free_space(self);
        self.advice = context_advice(self);
    }
}

pub fn context_snapshot(options: ContextOptions) -> Result<ContextSnapshot> {
    let store = SqliteStore::open(&options.db_path)?;
    let selector = options.session.trim();
    if selector.is_empty() {
        return Err(Error::Message(
            "pevo context requires --session <id|latest>".to_string(),
        ));
    }
    let summary = if selector == "latest" {
        let workdir = canonical_workdir(&options.workdir)?;
        let Some(session_id) =
            store.latest_session_for_workdir_with_sources(&workdir, &["run", "tui"])?
        else {
            return Err(Error::Message(format!(
                "no active run or tui session for {}",
                workdir.display()
            )));
        };
        store
            .session_summary(&session_id)?
            .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?
    } else {
        store
            .session_summary(selector)?
            .ok_or_else(|| Error::Message(format!("session not found: {selector}")))?
    };
    let session_metadata = store.session_metadata(&summary.id)?.unwrap_or(Value::Null);
    let workdir = PathBuf::from(&summary.workdir);
    let mode = session_metadata
        .get("mode")
        .and_then(Value::as_str)
        .and_then(RunMode::parse)
        .unwrap_or_default();
    let context_limit = session_metadata
        .get("context_limit")
        .and_then(Value::as_u64)
        .or_else(|| {
            configured_context_limit(&options, &summary.provider, &summary.model, &workdir)
        });
    let message_summaries = store
        .load_tui_message_summaries(&summary.id)?
        .into_iter()
        .collect::<Vec<_>>();
    let has_compaction = store
        .latest_valid_session_compaction(&summary.id)?
        .is_some();
    let latest_input_tokens = if has_compaction {
        None
    } else {
        latest_assistant_usage_input_tokens(&message_summaries)
    };
    let messages = load_projected_messages(&store, &summary.id, None)?;
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let skills_home = resolve_skills_home(&env, &workdir)?;
    let skill_options = SkillDiscoveryOptions {
        home: skills_home,
        workdir: workdir.clone(),
        config_path: options.config_path.clone(),
        env,
        explicit_inputs: Vec::new(),
        no_skills: false,
    };
    let catalog = discover_skills(&skill_options)?;
    let skills_prompt = format_skills_for_prompt(&catalog.skills);
    let mut request_messages = vec![json!({
        "role": "system",
        "content": mode_instruction(mode),
        "metadata": {
            "prompt_slot": "base/mode",
            "prompt_semantic_role": "base_policy",
        },
    })];
    if !skills_prompt.trim().is_empty() {
        request_messages.push(json!({
            "role": "system",
            "content": skills_prompt,
            "metadata": {
                "prompt_slot": "skill_index",
                "prompt_semantic_role": "developer_prompt",
            },
        }));
    }
    let project_instructions = load_project_instructions(&workdir)?;
    for (index, fragment) in project_instructions.fragments.iter().enumerate() {
        request_messages.push(json!({
            "role": "system",
            "content": format!(
                "Project instructions below are policy context, not user task content.\n\n{}",
                fragment.content
            ),
            "metadata": {
                "prompt_slot": format!("project_context:{index}"),
                "prompt_semantic_role": "developer_prompt",
            },
        }));
    }
    for message in &messages {
        request_messages.push(serde_json::to_value(message)?);
    }
    let mut tools = coding_core_tools_for_mode(&workdir, mode);
    tools.extend(skill_tools_for_mode(skill_options, mode));
    let request = GenerationRequest {
        model: ModelTarget {
            provider: summary.provider.clone(),
            model: summary.model.clone(),
        },
        messages: request_messages,
        tools: tool_declarations(&tools),
        metadata: json!({
            "context_counting": {
                "system_prompt_message_count": 1,
                "skill_index_message_count": if catalog.skills.is_empty() { 0 } else { 1 },
                "previous_message_count": messages.len(),
                "project_instruction_context_message_count": 0,
                "selected_skill_context_message_count": 0,
                "skill_names": catalog.skills.iter().map(|skill| skill.name.clone()).collect::<Vec<_>>(),
            }
        }),
    };
    let count = count_openai_chat_request(&request, "");
    let mut snapshot = snapshot_from_count(
        ContextScope::SessionEstimate,
        Some(summary.id),
        summary.provider,
        summary.model,
        Some(mode.as_str().to_string()),
        context_limit,
        count,
    );
    if let Some(tokens) = latest_input_tokens {
        snapshot.apply_provider_input_tokens(tokens);
    }
    Ok(snapshot)
}

pub(crate) fn context_counting_metadata(
    prompt_instructions: &[PromptInstruction],
    turn_prompt_message_count: usize,
    previous_message_count: usize,
    project_instruction_context_message_count: usize,
    selected_skill_context_message_count: usize,
    skill_names: Vec<String>,
) -> Value {
    let base_policy_message_count = prompt_instructions
        .iter()
        .filter(|instruction| instruction.semantic_role == "base_policy")
        .count();
    let developer_prompt_message_count = prompt_instructions
        .iter()
        .filter(|instruction| instruction.semantic_role == "developer_prompt")
        .count();
    let skill_index_message_count = prompt_instructions
        .iter()
        .filter(|instruction| instruction.slot == "skill_index")
        .count();
    json!({
        "system_prompt_message_count": prompt_instructions.len(),
        "base_policy_message_count": base_policy_message_count,
        "developer_prompt_message_count": developer_prompt_message_count,
        "turn_prompt_message_count": turn_prompt_message_count,
        "skill_index_message_count": skill_index_message_count,
        "previous_message_count": previous_message_count,
        "project_instruction_context_message_count": project_instruction_context_message_count,
        "selected_skill_context_message_count": selected_skill_context_message_count,
        "skill_names": skill_names,
    })
}

pub fn format_context_snapshot_text(snapshot: &ContextSnapshot, include_bar: bool) -> String {
    format_context_snapshot_text_with_options(
        snapshot,
        ContextFormatOptions {
            heading: true,
            bar_width: include_bar.then_some(CONTEXT_BAR_MIN_CELLS),
        },
    )
}

pub fn format_context_snapshot_text_with_options(
    snapshot: &ContextSnapshot,
    options: ContextFormatOptions,
) -> String {
    let mut lines = Vec::new();
    if options.heading {
        lines.push("Context Usage".to_string());
    }
    if let Some(width) = options.bar_width
        && let Some(bar) = context_bar(snapshot, width)
    {
        lines.push(bar);
        lines.push(
            "B base  D developer  P project  H history  C turn  U prompt  T tools  . free"
                .to_string(),
        );
        lines.push(String::new());
    }
    lines.push(format_total_line(snapshot));
    for key in [
        "base_policy",
        "developer_prompt",
        "project_context",
        "history",
        "turn_context",
        "current_prompt",
        "system_tools",
        "free_space",
    ] {
        let Some(category) = snapshot.categories.get(key) else {
            continue;
        };
        let percent = category
            .percent
            .map(|value| format!(" ({value:.1}%)"))
            .unwrap_or_default();
        lines.push(format!(
            "{}: {}{}",
            context_category_text_key(key),
            format_token_count(category.tokens, category.estimated),
            percent
        ));
        if key == "system_tools" {
            if let Some(count) = category.details.get("tool_count").and_then(Value::as_u64)
                && count > 0
            {
                lines.push(format!("  tools: {count}"));
            }
        } else if key == "developer_prompt" {
            for entry in sorted_skill_entries(category) {
                lines.push(format!(
                    "  {}: {}",
                    entry.name,
                    format_token_count(entry.tokens, true)
                ));
            }
        } else if key == "history" {
            if let Some(roles) = category.details.get("roles").and_then(Value::as_object) {
                for (role, value) in roles {
                    let count = value.get("count").and_then(Value::as_u64).unwrap_or(0);
                    let tokens = value.get("tokens").and_then(Value::as_u64).unwrap_or(0);
                    lines.push(format!(
                        "  {role}: {count} {}, {}",
                        input_message_unit(count),
                        format_token_count(tokens, true)
                    ));
                }
            }
        } else if key == "project_context" {
            let count = category
                .details
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if count > 0 {
                lines.push(format!(
                    "  project_context: {count} {}",
                    input_message_unit(count)
                ));
            }
        } else if key == "turn_context" {
            let count = category
                .details
                .get("selected_skill_context_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tokens = category
                .details
                .get("selected_skill_context_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if count > 0 || tokens > 0 {
                lines.push(format!(
                    "  selected_skill_context: {count} {}, {}",
                    input_message_unit(count),
                    format_token_count(tokens, true)
                ));
            }
        }
    }
    if !snapshot.advice.is_empty() {
        lines.push("advice:".to_string());
        for advice in &snapshot.advice {
            lines.push(format!("  - {}", advice.message));
        }
    }
    lines.push(String::new());
    lines.push(format!("scope: {}", scope_label(snapshot.scope)));
    lines.push(format!("model: {}/{}", snapshot.provider, snapshot.model));
    lines.join("\n")
}

fn context_category_text_key(key: &str) -> &str {
    match key {
        "history" => "input_history",
        "current_prompt" => "input_prompt",
        other => other,
    }
}

fn input_message_unit(count: u64) -> &'static str {
    if count == 1 {
        "input msg"
    } else {
        "input msgs"
    }
}

pub fn format_context_total_value(snapshot: &ContextSnapshot) -> String {
    format_context_total_value_parts(
        snapshot.total.tokens,
        snapshot.total.estimated,
        snapshot.context_limit,
        snapshot.total.percent,
    )
}

pub fn format_context_total_value_parts(
    tokens: u64,
    estimated: bool,
    context_limit: Option<u64>,
    percent: Option<f64>,
) -> String {
    let suffix = if estimated { " estimated" } else { "" };
    if let Some(limit) = context_limit {
        let percent = percent
            .map(|value| format!(" ({value:.1}%)"))
            .unwrap_or_default();
        format!(
            "{}/{}{}{}",
            format_compact_count(tokens, estimated),
            format_compact_count(limit, false),
            percent,
            suffix
        )
    } else {
        format!("{}{}", format_token_count(tokens, estimated), suffix)
    }
}

fn format_total_line(snapshot: &ContextSnapshot) -> String {
    format!("tokens: {}", format_context_total_value(snapshot))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillEntryTokens {
    name: String,
    tokens: u64,
}

fn sorted_skill_entries(category: &ContextCategory) -> Vec<SkillEntryTokens> {
    let mut entries = category
        .details
        .get("skill_entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?;
            let tokens = entry.get("tokens").and_then(Value::as_u64)?;
            Some(SkillEntryTokens {
                name: name.to_string(),
                tokens,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .tokens
            .cmp(&left.tokens)
            .then_with(|| left.name.cmp(&right.name))
    });
    entries
}

fn configured_context_limit(
    options: &ContextOptions,
    provider: &str,
    model: &str,
    workdir: &std::path::Path,
) -> Option<u64> {
    let run_options = crate::types::RunOptions {
        db_path: options.db_path.clone(),
        workdir: workdir.to_path_buf(),
        snapshot_root: None,
        session: None,
        continue_latest: false,
        prompt: "context estimate".to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        model: Some(format!("{provider}/{model}")),
        reasoning_effort: None,
        include_reasoning: false,
        mode: RunMode::Build,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: None,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
    };
    selected_configured_model(&run_options)
        .ok()
        .flatten()
        .and_then(|model| model.context_limit)
}

fn latest_assistant_usage_input_tokens(
    messages: &[crate::types::TuiMessageSummary],
) -> Option<u64> {
    messages.iter().rev().find_map(|summary| {
        matches!(summary.message, Message::Assistant { .. })
            .then(|| summary.usage.as_ref().and_then(provider_input_tokens))
            .flatten()
    })
}

fn snapshot_from_count(
    scope: ContextScope,
    session_id: Option<String>,
    provider: String,
    model: String,
    mode: Option<String>,
    context_limit: Option<u64>,
    count: OpenAiChatTokenCount,
) -> ContextSnapshot {
    let mut categories = BTreeMap::new();
    insert_category(
        &mut categories,
        "base_policy",
        "Base policy",
        count.base_policy_tokens,
        context_limit,
        json!({}),
    );
    insert_category(
        &mut categories,
        "developer_prompt",
        "Developer prompt",
        count.developer_prompt_tokens,
        context_limit,
        json!({
            "skill_count": count.skill_names.len(),
            "skill_names": count.skill_names,
            "skill_entries": count.skill_entries
                .into_iter()
                .map(|entry| json!({ "name": entry.name, "tokens": entry.tokens }))
                .collect::<Vec<_>>(),
        }),
    );
    insert_category(
        &mut categories,
        "project_context",
        "Project context",
        count.project_context_tokens,
        context_limit,
        json!({ "count": count.project_instruction_context_count }),
    );
    let roles = count
        .role_counts
        .into_iter()
        .map(|(role, value)| {
            (
                role,
                json!({
                    "count": value.count,
                    "tokens": value.tokens,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    insert_category(
        &mut categories,
        "history",
        "History",
        count.history_tokens,
        context_limit,
        json!({ "roles": roles }),
    );
    insert_category(
        &mut categories,
        "turn_context",
        "Turn context",
        count.turn_context_tokens,
        context_limit,
        json!({
            "selected_skill_context_count": count.selected_skill_context_count,
            "selected_skill_context_tokens": count.selected_skill_context_tokens,
        }),
    );
    insert_category(
        &mut categories,
        "current_prompt",
        "Current prompt",
        count.current_prompt_tokens,
        context_limit,
        json!({}),
    );
    insert_category(
        &mut categories,
        "system_tools",
        "System tools",
        count.system_tools_tokens,
        context_limit,
        json!({ "tool_count": count.tool_count }),
    );
    let mut snapshot = ContextSnapshot {
        event_type: CONTEXT_SNAPSHOT_TYPE.to_string(),
        scope,
        status: "estimated".to_string(),
        session_id,
        provider,
        model,
        mode,
        context_limit,
        tokenizer: ContextTokenizer {
            encoding: count.encoding,
            source: count.encoding_source,
            fallback: count.encoding_fallback,
        },
        total: ContextTotal {
            tokens: count.total_estimated_tokens,
            estimated_tokens: count.total_estimated_tokens,
            estimated: true,
            source: "estimate".to_string(),
            percent: percent(count.total_estimated_tokens, context_limit),
        },
        categories,
        advice: Vec::new(),
    };
    rebuild_free_space(&mut snapshot);
    snapshot.advice = context_advice(&snapshot);
    snapshot
}

fn insert_category(
    categories: &mut BTreeMap<String, ContextCategory>,
    key: &str,
    label: &str,
    tokens: u64,
    context_limit: Option<u64>,
    details: Value,
) {
    categories.insert(
        key.to_string(),
        ContextCategory {
            label: label.to_string(),
            tokens,
            estimated: true,
            status: "estimated".to_string(),
            percent: percent(tokens, context_limit),
            details,
        },
    );
}

fn rebuild_free_space(snapshot: &mut ContextSnapshot) {
    let Some(limit) = snapshot.context_limit else {
        snapshot.categories.remove("free_space");
        return;
    };
    let free = limit.saturating_sub(snapshot.total.tokens);
    snapshot.categories.insert(
        "free_space".to_string(),
        ContextCategory {
            label: "Free space".to_string(),
            tokens: free,
            estimated: snapshot.total.estimated,
            status: "derived".to_string(),
            percent: percent(free, Some(limit)),
            details: json!({}),
        },
    );
}

fn context_advice(snapshot: &ContextSnapshot) -> Vec<ContextAdvice> {
    let mut advice = Vec::new();
    let Some(limit) = snapshot.context_limit else {
        advice.push(ContextAdvice {
            category: "context_limit".to_string(),
            severity: "info".to_string(),
            message: "Context limit unknown; configure or refresh model metadata to show remaining space.".to_string(),
        });
        return advice;
    };
    if limit > 0 {
        let total_percent = snapshot.total.tokens as f64 / limit as f64 * 100.0;
        if total_percent >= TOTAL_CRITICAL_PERCENT {
            advice.push(ContextAdvice {
                category: "total".to_string(),
                severity: "critical".to_string(),
                message: "Context usage is above 90%; reduce message history or tool/skill surface before continuing.".to_string(),
            });
        } else if total_percent >= TOTAL_WARNING_PERCENT {
            advice.push(ContextAdvice {
                category: "total".to_string(),
                severity: "warning".to_string(),
                message: "Context usage is above 70%; consider reducing history, tools, or enabled skills.".to_string(),
            });
        }
    }
    for (key, message) in [
        (
            "system_tools",
            "System tools are a large share; switch to plan mode or reduce the tool surface when possible.",
        ),
        (
            "developer_prompt",
            "Developer prompt is a large share; prune enabled skills or narrow configured agent and skill paths.",
        ),
        (
            "history",
            "History dominates context; shorten the conversation or start a fresh session when practical.",
        ),
        (
            "turn_context",
            "Turn context is a large share; reduce selected skill bodies or prompt-scoped attachments.",
        ),
        (
            "current_prompt",
            "Current prompt is a large share; shorten prompt-scoped input where practical.",
        ),
    ] {
        if advice.len() >= ADVICE_LIMIT {
            break;
        }
        let Some(category) = snapshot.categories.get(key) else {
            continue;
        };
        let percent = category.tokens as f64 / limit as f64 * 100.0;
        if percent > CATEGORY_ADVICE_PERCENT {
            advice.push(ContextAdvice {
                category: key.to_string(),
                severity: "warning".to_string(),
                message: message.to_string(),
            });
        }
    }
    advice.truncate(ADVICE_LIMIT);
    advice
}

fn provider_input_tokens(usage: &Value) -> Option<u64> {
    usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .or_else(|| usage.get("context_input_tokens"))
        .and_then(Value::as_u64)
}

fn percent(tokens: u64, limit: Option<u64>) -> Option<f64> {
    let limit = limit?;
    (limit > 0).then(|| tokens as f64 / limit as f64 * 100.0)
}

fn format_token_count(tokens: u64, estimated: bool) -> String {
    format!("{} tokens", format_compact_count(tokens, estimated))
}

fn format_compact_count(tokens: u64, estimated: bool) -> String {
    let prefix = if estimated { "~" } else { "" };
    if tokens < 1_000 {
        format!("{prefix}{tokens}")
    } else if tokens < 1_000_000 {
        let value = tokens as f64 / 1_000.0;
        format!("{prefix}{value:.1}k")
    } else {
        let value = tokens as f64 / 1_000_000.0;
        format!("{prefix}{value:.1}M")
    }
}

fn context_bar(snapshot: &ContextSnapshot, requested_width: usize) -> Option<String> {
    let limit = snapshot.context_limit?;
    let bar_cells = normalize_context_bar_width(requested_width);
    let order = [
        ("base_policy", 'B'),
        ("developer_prompt", 'D'),
        ("project_context", 'P'),
        ("history", 'H'),
        ("turn_context", 'C'),
        ("current_prompt", 'U'),
        ("system_tools", 'T'),
        ("free_space", '.'),
    ];
    let total = order
        .iter()
        .map(|(key, _)| {
            if *key == "free_space" {
                limit.saturating_sub(snapshot.total.estimated_tokens)
            } else {
                snapshot
                    .categories
                    .get(*key)
                    .map(|category| category.tokens)
                    .unwrap_or(0)
            }
        })
        .sum::<u64>();
    if total == 0 {
        return None;
    }
    let mut cells = String::new();
    let mut used = 0usize;
    for (index, (key, marker)) in order.iter().enumerate() {
        let tokens = if *key == "free_space" {
            limit.saturating_sub(snapshot.total.estimated_tokens)
        } else {
            snapshot
                .categories
                .get(*key)
                .map(|category| category.tokens)
                .unwrap_or(0)
        };
        let remaining = bar_cells.saturating_sub(used);
        let width = if index + 1 == order.len() {
            remaining
        } else {
            ((tokens as f64 / total as f64) * bar_cells as f64).round() as usize
        }
        .min(remaining);
        cells.extend(std::iter::repeat_n(*marker, width));
        used = used.saturating_add(width);
    }
    while cells.len() < bar_cells {
        cells.push('.');
    }
    Some(format!("[{cells}]"))
}

pub fn normalize_context_bar_width(requested_width: usize) -> usize {
    let clamped = requested_width.clamp(CONTEXT_BAR_MIN_CELLS, CONTEXT_BAR_MAX_CELLS);
    (clamped / 5 * 5).max(CONTEXT_BAR_MIN_CELLS)
}

fn scope_label(scope: ContextScope) -> &'static str {
    match scope {
        ContextScope::LastProviderRequest => "last provider request",
        ContextScope::SessionEstimate => "session estimate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_ai::{OpenAiChatRoleTokenCount, OpenAiChatSkillTokenCount};

    fn count(system: u64, tools: u64, skills: u64, messages: u64) -> OpenAiChatTokenCount {
        let mut role_counts = BTreeMap::new();
        role_counts.insert(
            "user".to_string(),
            OpenAiChatRoleTokenCount {
                count: 2,
                tokens: messages,
            },
        );
        OpenAiChatTokenCount {
            encoding: "o200k_base".to_string(),
            encoding_source: "fallback".to_string(),
            encoding_fallback: true,
            base_policy_tokens: system,
            developer_prompt_tokens: skills,
            project_context_tokens: 0,
            history_tokens: messages,
            turn_context_tokens: 0,
            current_prompt_tokens: 0,
            system_prompt_tokens: system + skills,
            system_tools_tokens: tools,
            skills_tokens: 0,
            messages_tokens: messages,
            total_estimated_tokens: system + tools + skills + messages,
            tool_count: 4,
            role_counts,
            project_instruction_context_tokens: 0,
            project_instruction_context_count: 0,
            selected_skill_context_tokens: 0,
            selected_skill_context_count: 0,
            skill_names: vec!["alpha".to_string(), "beta".to_string()],
            skill_entries: vec![
                OpenAiChatSkillTokenCount {
                    name: "beta".to_string(),
                    tokens: 8,
                },
                OpenAiChatSkillTokenCount {
                    name: "alpha".to_string(),
                    tokens: 13,
                },
            ],
        }
    }

    #[test]
    fn snapshot_categories_use_estimates_and_provider_usage_overrides_headline() {
        let mut snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(200),
            count(10, 20, 5, 45),
        );

        assert_eq!(snapshot.total.tokens, 80);
        assert!(snapshot.total.estimated);
        assert_eq!(snapshot.categories["system_tools"].tokens, 20);

        snapshot.apply_provider_input_tokens(90);

        assert_eq!(snapshot.total.tokens, 90);
        assert!(!snapshot.total.estimated);
        assert_eq!(snapshot.total.source, "provider_usage");
        assert_eq!(snapshot.categories["free_space"].tokens, 110);
        assert_eq!(
            snapshot.categories["history"].details["roles"]["user"]["count"],
            2
        );
    }

    #[test]
    fn snapshot_text_reports_project_context_bucket() {
        let mut count = count(10, 20, 5, 45);
        count.project_instruction_context_count = 2;
        count.project_instruction_context_tokens = 17;
        count.project_context_tokens = 17;
        let snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(200),
            count,
        );

        let text = format_context_snapshot_text(&snapshot, false);

        assert!(text.contains("project_context: ~17 tokens"));
        assert!(text.contains("project_context: 2 input msgs"));
        assert!(!text.contains("\nmessages:"));
    }

    #[test]
    fn context_text_uses_compact_layout_and_skill_entry_order() {
        let mut snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(1_000_000),
            count(350, 674, 341, 31_000),
        );
        snapshot.apply_provider_input_tokens(34_000);

        let text = format_context_snapshot_text_with_options(
            &snapshot,
            ContextFormatOptions {
                heading: false,
                bar_width: Some(55),
            },
        );

        assert!(text.starts_with("["));
        assert!(text.contains(
            "\nB base  D developer  P project  H history  C turn  U prompt  T tools  . free\n\n"
        ));
        assert!(text.contains("tokens: 34.0k/1.0M (3.4%)\n"));
        let token_line = text
            .lines()
            .find(|line| line.starts_with("tokens:"))
            .expect("token line");
        assert_eq!(
            format_context_total_value(&snapshot),
            token_line.strip_prefix("tokens: ").expect("token value")
        );
        assert_eq!(
            format_context_total_value_parts(34_000, false, Some(1_000_000), Some(3.4)),
            "34.0k/1.0M (3.4%)"
        );
        assert!(!token_line.contains("provider"));
        assert!(text.contains("\n  alpha: ~13 tokens\n  beta: ~8 tokens\n"));
        assert!(text.contains("input_history: ~31.0k tokens"));
        assert!(!text.contains("\nmessages:"));
        assert!(text.contains("user: 2 input msgs, ~31.0k tokens"));
        assert!(text.contains("free_space: 966.0k tokens (96.6%)\n\nscope: last provider request\nmodel: openai/gpt-4o"));

        let value = serde_json::to_value(&snapshot).expect("snapshot json");
        assert!(value["categories"].get("history").is_some());
        assert!(value["categories"].get("input_messages").is_none());
    }

    #[test]
    fn context_text_uses_singular_input_msg_count() {
        assert_eq!(input_message_unit(1), "input msg");
        assert_eq!(input_message_unit(2), "input msgs");
    }

    #[test]
    fn estimated_context_text_marks_only_estimated_headline() {
        let snapshot = snapshot_from_count(
            ContextScope::SessionEstimate,
            Some("session".to_string()),
            "mock".to_string(),
            "model".to_string(),
            None,
            Some(1_000_000),
            count(1_000, 2_000, 3_000, 28_000),
        );
        let text = format_context_snapshot_text(&snapshot, false);

        assert!(text.contains("tokens: ~34.0k/1.0M (3.4%) estimated"));
    }

    #[test]
    fn unknown_context_limit_omits_free_space_and_reports_metadata_advice() {
        let snapshot = snapshot_from_count(
            ContextScope::SessionEstimate,
            Some("session".to_string()),
            "mock".to_string(),
            "model".to_string(),
            None,
            None,
            count(1, 2, 3, 4),
        );

        assert!(!snapshot.categories.contains_key("free_space"));
        assert_eq!(snapshot.total.percent, None);
        assert_eq!(snapshot.advice[0].category, "context_limit");

        let text = format_context_snapshot_text(&snapshot, false);
        let token_line = text
            .lines()
            .find(|line| line.starts_with("tokens:"))
            .expect("token line");
        assert_eq!(
            format_context_total_value(&snapshot),
            token_line.strip_prefix("tokens: ").expect("token value")
        );
    }

    #[test]
    fn advice_is_thresholded_and_bounded() {
        let snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "model".to_string(),
            Some("default".to_string()),
            Some(100),
            count(5, 30, 25, 35),
        );

        assert_eq!(snapshot.advice.len(), 3);
        assert_eq!(snapshot.advice[0].category, "total");
        assert!(
            snapshot
                .advice
                .iter()
                .any(|advice| advice.category == "system_tools")
        );
        assert!(
            snapshot
                .advice
                .iter()
                .any(|advice| advice.category == "developer_prompt")
        );
    }

    #[test]
    fn recorder_keeps_latest_started_request() {
        let recorder = ContextRecorder::default();
        {
            let mut state = recorder.state.lock().expect("state");
            state.latest_started_sequence = 2;
        }
        let old = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "old".to_string(),
            None,
            Some(100),
            count(1, 1, 1, 1),
        );
        let latest = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "latest".to_string(),
            None,
            Some(100),
            count(2, 2, 2, 2),
        );

        recorder.finish_count(1, old);
        assert!(recorder.latest_snapshot().is_none());

        recorder.finish_count(2, latest);
        assert_eq!(
            recorder.latest_snapshot().expect("snapshot").model,
            "latest"
        );
    }
}
