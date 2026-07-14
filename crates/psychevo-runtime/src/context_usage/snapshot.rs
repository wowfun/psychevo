use super::presentation::{
    context_advice, context_bar, format_compact_count, format_token_count, percent,
    provider_input_tokens, scope_label,
};
use super::{
    AbortSignal, Arc, BTreeMap, BoxFuture, Deserialize, Error, GenerationProvider,
    GenerationRequest, GenerationStream, Message, ModelTarget, Mutex, OpenAiChatTokenCount,
    PathBuf, PromptInstruction, Result, RunMode, RunOptions, Serialize, SkillDiscoveryOptions,
    StateRuntime, Value, canonical_cwd, coding_core_tools_for_mode, count_openai_chat_request,
    discover_skills, format_skills_for_prompt, json, load_project_context_instruction_mode,
    load_project_instructions, load_projected_messages, mode_instruction, resolve_skills_home,
    runtime_environment_prompt, selected_configured_model, skill_tools_for_mode,
    skills_visible_for_prompt_with_tools, tool_declarations,
};
use crate::prompt_templates;

pub(crate) const CONTEXT_SNAPSHOT_TYPE: &str = "context_snapshot";
pub(crate) const TOTAL_WARNING_PERCENT: f64 = 70.0;
pub(crate) const TOTAL_CRITICAL_PERCENT: f64 = 90.0;
pub(crate) const CATEGORY_ADVICE_PERCENT: f64 = 20.0;
pub(crate) const ADVICE_LIMIT: usize = 3;
pub const CONTEXT_BAR_MIN_CELLS: usize = 50;
pub const CONTEXT_BAR_MAX_CELLS: usize = 100;

#[derive(Debug, Clone)]
pub struct ContextOptions {
    pub state: StateRuntime,
    pub cwd: PathBuf,
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
    pub(crate) state: Arc<Mutex<ContextRecorderState>>,
}

#[derive(Debug, Default)]
pub(crate) struct ContextRecorderState {
    pub(crate) latest_started_sequence: u64,
    pub(crate) latest_snapshot: Option<(u64, ContextSnapshot)>,
    pub(crate) latest_provider_input_tokens: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveContextProfile {
    pub(crate) session_id: String,
    pub(crate) base_url: String,
    pub(crate) context_limit: Option<u64>,
    pub(crate) mode: RunMode,
}

pub(crate) struct ContextRecordingProvider {
    pub(crate) inner: Arc<dyn GenerationProvider>,
    pub(crate) recorder: ContextRecorder,
    pub(crate) profile: LiveContextProfile,
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

    pub(crate) fn finish_count(&self, sequence: u64, mut snapshot: ContextSnapshot) {
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
    let store = options.state.store().clone();
    let selector = options.session.trim();
    if selector.is_empty() {
        return Err(Error::Message(
            "pevo context requires --session <id|latest>".to_string(),
        ));
    }
    let summary = if selector == "latest" {
        let cwd = canonical_cwd(&options.cwd)?;
        let Some(session_id) = store.latest_session_for_cwd_with_sources(&cwd, &["run", "tui"])?
        else {
            return Err(Error::Message(format!(
                "no active run or tui session for {}",
                cwd.display()
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
    let cwd = PathBuf::from(&summary.cwd);
    let mode = session_metadata
        .get("mode")
        .and_then(Value::as_str)
        .and_then(RunMode::parse)
        .unwrap_or_default();
    let context_limit = session_metadata
        .get("context_limit")
        .and_then(Value::as_u64)
        .or_else(|| configured_context_limit(&options, &summary.provider, &summary.model, &cwd));
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
    let project_context_options = RunOptions {
        state: options.state.clone(),
        cwd: cwd.clone(),
        snapshot_root: None,
        session: Some(summary.id.clone()),
        continue_latest: false,
        prompt: "context estimate".to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        sandbox_override: None,
        model: Some(format!("{}/{}", summary.provider, summary.model)),
        reasoning_effort: session_metadata
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_string),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        include_reasoning: false,
        mode,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(env.clone()),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        runtime_tools: Vec::new(),
    };
    let project_context_mode =
        load_project_context_instruction_mode(&project_context_options, &cwd)?;
    let skills_home = resolve_skills_home(&env, &cwd)?;
    let skill_options = SkillDiscoveryOptions {
        home: skills_home,
        cwd: cwd.clone(),
        config_path: options.config_path.clone(),
        env,
        explicit_inputs: Vec::new(),
        additional_roots: Vec::new(),
        no_skills: false,
    };
    let catalog = discover_skills(&skill_options)?;
    let mut tools = coding_core_tools_for_mode(&cwd, mode);
    tools.extend(skill_tools_for_mode(skill_options, mode));
    let effective_tool_names = tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    let prompt_skills = skills_visible_for_prompt_with_tools(
        &catalog.skills,
        effective_tool_names.iter().map(String::as_str),
    );
    let skills_prompt = format_skills_for_prompt(&prompt_skills);
    let mut request_messages = vec![json!({
        "role": "system",
        "content": mode_instruction(mode),
        "metadata": {
            "prompt_slot": "base/mode",
            "prompt_semantic_role": "base_policy",
        },
    })];
    request_messages.push(json!({
        "role": "system",
        "content": runtime_environment_prompt(&cwd),
        "metadata": {
            "prompt_slot": "runtime_environment",
            "prompt_semantic_role": "base_policy",
        },
    }));
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
    let project_instructions = load_project_instructions(&cwd, project_context_mode)?;
    for (index, fragment) in project_instructions.fragments.iter().enumerate() {
        request_messages.push(json!({
            "role": "system",
            "content": prompt_templates::project_context(&fragment.content),
            "metadata": {
                "prompt_slot": format!("project_context:{index}"),
                "prompt_semantic_role": "developer_prompt",
            },
        }));
    }
    for message in &messages {
        request_messages.push(serde_json::to_value(message)?);
    }
    let request = GenerationRequest {
        model: ModelTarget {
            provider: summary.provider.clone(),
            model: summary.model.clone(),
        },
        messages: request_messages,
        tools: tool_declarations(&tools)
            .into_iter()
            .map(Into::into)
            .collect(),
        metadata: json!({
            "context_counting": {
                "system_prompt_message_count": 2,
                "base_policy_message_count": 2,
                "skill_index_message_count": if prompt_skills.is_empty() { 0 } else { 1 },
                "previous_message_count": messages.len(),
                "project_instruction_context_message_count": 0,
                "selected_skill_context_message_count": 0,
                "skill_names": prompt_skills.iter().map(|skill| skill.name.clone()).collect::<Vec<_>>(),
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

pub(crate) fn context_category_text_key(key: &str) -> &str {
    match key {
        "history" => "input_history",
        "current_prompt" => "input_prompt",
        other => other,
    }
}

pub(crate) fn input_message_unit(count: u64) -> &'static str {
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

pub(crate) fn format_total_line(snapshot: &ContextSnapshot) -> String {
    format!("tokens: {}", format_context_total_value(snapshot))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillEntryTokens {
    pub(crate) name: String,
    pub(crate) tokens: u64,
}

pub(crate) fn sorted_skill_entries(category: &ContextCategory) -> Vec<SkillEntryTokens> {
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

pub(crate) fn configured_context_limit(
    options: &ContextOptions,
    provider: &str,
    model: &str,
    cwd: &std::path::Path,
) -> Option<u64> {
    let run_options = crate::types::RunOptions {
        state: options.state.clone(),
        cwd: cwd.to_path_buf(),
        snapshot_root: None,
        session: None,
        continue_latest: false,
        prompt: "context estimate".to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        sandbox_override: None,
        model: Some(format!("{provider}/{model}")),
        reasoning_effort: None,
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
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
        runtime_tools: Vec::new(),
    };
    selected_configured_model(&run_options)
        .ok()
        .flatten()
        .and_then(|model| model.context_limit)
}

pub(crate) fn latest_assistant_usage_input_tokens(
    messages: &[crate::types::TuiMessageSummary],
) -> Option<u64> {
    messages.iter().rev().find_map(|summary| {
        matches!(summary.message, Message::Assistant { .. })
            .then(|| summary.usage.as_ref().and_then(provider_input_tokens))
            .flatten()
    })
}

pub(crate) fn snapshot_from_count(
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

pub(crate) fn insert_category(
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

pub(crate) fn rebuild_free_space(snapshot: &mut ContextSnapshot) {
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
