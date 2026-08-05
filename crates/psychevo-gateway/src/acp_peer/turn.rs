use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use psychevo::application::{
    ImageInput, Message, Outcome, ResolvedMcpServerInput, RunStreamSink, SelectedAgent,
    UserContentBlock, WorkspaceMutationSink, suggested_thread_title,
};
use psychevo::skills::resolve_skills_home;
use serde_json::{Value, json};

use crate::gateway::agent_session::{AgentErrorStage, agent_session_error};
use crate::gateway::peer_runtime::ResolvedPeerTurn;
use crate::{ACP_PEER_METADATA_KEY, gateway_now_ms};
use psychevo_gateway_protocol as wire;

use super::metadata_permissions::{emit_runtime_event, peer_session_metadata, prompt_history_text};
use super::process_pool::{AcpProcessPool, AcpSessionReadyCallback};
use super::stdio_turn::{
    AcpBeforePromptCallback, AcpPeerTurnContext, is_acp_peer_abort_error, run_acp_stdio_turn,
};
use super::stream_state::{
    AcpHistoryReplayEntry, AcpHistoryReplayProjection, AcpPeerPlanProjection,
    persisted_assistant_content, persisted_tool_result_messages_at,
    replay_entry_delivery_message_ids, replay_entry_identity,
};

pub(super) const ACP_PEER_ABORT_MESSAGE: &str = "ACP peer turn aborted";

#[derive(Debug)]
pub(crate) struct AcpPeerTurnResult {
    pub(crate) turn: psychevo::TurnResult,
}

pub(crate) struct AcpPeerTurnRequest {
    pub(crate) thread: psychevo::ThreadExecutionContext,
    pub(crate) history: psychevo::HistoryReader,
    pub(crate) turn_id: String,
    pub(crate) native_session_id: Option<String>,
    pub(crate) input: Vec<wire::source::GatewayInputPart>,
    pub(crate) prompt: String,
    pub(crate) images: Vec<ImageInput>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) runtime_options: BTreeMap<String, String>,
    pub(crate) mcp_servers: Vec<ResolvedMcpServerInput>,
    pub(crate) stream: Option<RunStreamSink>,
    pub(crate) workspace_mutations: Option<WorkspaceMutationSink>,
    pub(crate) approval_handler: Option<Arc<dyn psychevo::ApprovalHandler>>,
    pub(crate) control: psychevo::TurnControl,
    pub(crate) persistence: Arc<dyn psychevo::AgentTurnPersistence>,
}

#[derive(Clone)]
pub(super) struct AcpClientContext {
    pub(super) cwd: PathBuf,
    pub(super) fs_read: bool,
    pub(super) fs_write: bool,
    pub(super) approval_handler: Option<Arc<dyn psychevo::ApprovalHandler>>,
    pub(super) turn_control: Option<psychevo::TurnControl>,
    pub(super) terminal: bool,
    pub(super) terminal_env: BTreeMap<String, String>,
}

fn acp_message_ids(metadata: Option<&Value>) -> Vec<String> {
    metadata
        .and_then(|metadata| metadata.pointer("/acp/messageIds"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn acp_message_turn_id(metadata: Option<&Value>) -> Option<&str> {
    metadata
        .and_then(|metadata| metadata.pointer("/acp/turnId"))
        .and_then(Value::as_str)
}

fn acp_replay_id(metadata: Option<&Value>) -> Option<&str> {
    metadata
        .and_then(|metadata| metadata.pointer("/acp/replayId"))
        .and_then(Value::as_str)
}

fn acp_message_metadata(
    message_ids: &[String],
    origin: &str,
    turn_id: Option<&str>,
    plan: Option<&AcpPeerPlanProjection>,
) -> Option<Value> {
    if message_ids.is_empty() && plan.is_none() {
        return None;
    }
    let mut acp = json!({
        "messageIds": message_ids,
        "origin": origin,
    });
    if let Some(turn_id) = turn_id
        && let Some(object) = acp.as_object_mut()
    {
        object.insert("turnId".to_string(), Value::String(turn_id.to_string()));
    }
    if let Some(plan) = plan
        && let Some(object) = acp.as_object_mut()
    {
        object.insert(
            "plan".to_string(),
            json!({
                "body": plan.body,
                "update": plan.update,
            }),
        );
    }
    Some(json!({ "acp": acp }))
}

const ACP_PROMPT_USAGE_FIELDS: &[&str] = &[
    "total_tokens",
    "input_tokens",
    "output_tokens",
    "reasoning_tokens",
    "cached_tokens",
    "cache_write_tokens",
];

struct PreviousAcpPromptUsage {
    cumulative: Value,
    native_session_id: Option<String>,
}

#[derive(Clone)]
struct AcpHistoryJournal {
    history: psychevo::HistoryReader,
    persistence: Arc<dyn psychevo::AgentTurnPersistence>,
}

impl AcpHistoryJournal {
    async fn for_each_message(
        &self,
        mut visit: impl FnMut(&psychevo::application::Message, Option<&Value>),
    ) -> psychevo::Result<()> {
        let mut after = None;
        loop {
            let page = self.history.replay_after(after, Some(200)).await?;
            if let Some(warning) = page.warnings.first() {
                return Err(psychevo::Error::Message(format!(
                    "persisted ACP history is invalid at message {} ({:?})",
                    warning.session_seq, warning.kind
                )));
            }
            for item in page.items {
                if let psychevo::application::HistoryReplayItem::Available { item } = item {
                    visit(&item.message, item.metadata.as_ref());
                }
            }
            let Some(next_after) = page.next_after else {
                return Ok(());
            };
            after = Some(next_after);
        }
    }

    async fn latest_prompt_usage(&self) -> psychevo::Result<Option<PreviousAcpPromptUsage>> {
        let mut before = None;
        loop {
            let page = self.history.before(before, Some(200)).await?;
            let previous = page.items.into_iter().rev().find_map(|item| {
                if !matches!(
                    item.message,
                    psychevo::application::Message::Assistant { .. }
                ) {
                    return None;
                }
                let metadata = item.metadata?;
                let cumulative = metadata.pointer("/acp/promptUsageCumulative")?.clone();
                let native_session_id = metadata
                    .pointer("/acp/promptUsageNativeSessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Some(PreviousAcpPromptUsage {
                    cumulative,
                    native_session_id,
                })
            });
            if previous.is_some() {
                return Ok(previous);
            }
            let Some(next_before) = page.next_before else {
                return Ok(None);
            };
            before = Some(next_before);
        }
    }

    async fn prior_unknown_delivery(&self) -> psychevo::Result<Option<String>> {
        Ok(self
            .persistence
            .prior_unknown_delivery()
            .await?
            .map(|delivery| delivery.turn_id))
    }

    async fn append_message(&self, message: Message) -> psychevo::Result<()> {
        self.persistence.append_message(message).await
    }

    async fn append_message_with_metrics(
        &self,
        message: Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> psychevo::Result<()> {
        self.persistence
            .append_message_with_metrics(message, usage, metadata)
            .await
    }

    async fn reconcile_unknown_delivery(
        &self,
        turn_id: &str,
        metadata: Value,
    ) -> psychevo::Result<bool> {
        self.persistence
            .reconcile_unknown_delivery(turn_id.to_string(), metadata)
            .await
    }
}

async fn acp_prompt_usage_delta(
    journal: &AcpHistoryJournal,
    native_session_id: &str,
    cumulative: &Value,
) -> psychevo::Result<(Value, bool)> {
    let previous = journal.latest_prompt_usage().await?;
    let Some(previous) = previous else {
        return Ok((cumulative.clone(), false));
    };
    if previous.native_session_id.as_deref() != Some(native_session_id) {
        return Ok((cumulative.clone(), true));
    }
    Ok(cumulative_usage_delta(cumulative, &previous.cumulative))
}

fn cumulative_usage_delta(current: &Value, previous: &Value) -> (Value, bool) {
    let mut delta = serde_json::Map::new();
    let mut reset = false;
    for key in ACP_PROMPT_USAGE_FIELDS {
        let Some(current) = current.get(*key).and_then(Value::as_u64) else {
            continue;
        };
        let previous = previous.get(*key).and_then(Value::as_u64);
        let value = match previous {
            Some(previous) if current >= previous => current - previous,
            Some(_) => {
                reset = true;
                current
            }
            None => current,
        };
        delta.insert((*key).to_string(), Value::from(value));
    }
    (Value::Object(delta), reset)
}

fn acp_live_message_metadata(
    message_ids: &[String],
    turn_id: &str,
    plan: Option<&AcpPeerPlanProjection>,
    prompt_usage_cumulative: Option<&Value>,
    native_session_id: &str,
    usage_counter_reset: bool,
) -> Option<Value> {
    let mut metadata = acp_message_metadata(message_ids, "live", Some(turn_id), plan);
    let Some(prompt_usage_cumulative) = prompt_usage_cumulative else {
        return metadata;
    };
    let metadata =
        metadata.get_or_insert_with(|| json!({ "acp": { "messageIds": [], "origin": "live" } }));
    let acp = metadata
        .get_mut("acp")
        .and_then(Value::as_object_mut)
        .expect("ACP live message metadata must contain an object");
    acp.insert(
        "promptUsageCumulative".to_string(),
        prompt_usage_cumulative.clone(),
    );
    acp.insert(
        "promptUsageNativeSessionId".to_string(),
        Value::String(native_session_id.to_string()),
    );
    acp.insert(
        "usageScope".to_string(),
        Value::String("acp_session_cumulative".to_string()),
    );
    if usage_counter_reset {
        acp.insert("usageCounterReset".to_string(), Value::Bool(true));
    }
    Some(metadata.clone())
}

fn acp_history_message_metadata(
    replay_id: &str,
    message_ids: &[String],
    turn_id: Option<&str>,
    plan: Option<&AcpPeerPlanProjection>,
) -> Value {
    let mut metadata = acp_message_metadata(message_ids, "history", turn_id, plan)
        .unwrap_or_else(|| json!({"acp": {"messageIds": [], "origin": "history"}}));
    if let Some(acp) = metadata.get_mut("acp").and_then(Value::as_object_mut) {
        acp.insert("replayId".to_string(), Value::String(replay_id.to_string()));
    }
    metadata
}

#[derive(Debug, Clone)]
struct ProjectedAcpHistoryEntry {
    replay_id: String,
    delivery_message_ids: Vec<String>,
    reconciles_delivery: bool,
    messages: Vec<psychevo::AgentImportedMessage>,
}

fn project_acp_history_replay(
    peer: &ResolvedPeerTurn,
    replay: &AcpHistoryReplayProjection,
    timestamp_ms: i64,
    turn_id: Option<&str>,
) -> Vec<ProjectedAcpHistoryEntry> {
    replay
        .entries
        .iter()
        .filter_map(|entry| {
            let replay_id = replay_entry_identity(entry).to_string();
            let delivery_message_ids = replay_entry_delivery_message_ids(entry);
            let replay_turn_id = turn_id.or(Some(replay_id.as_str()));
            let (reconciles_delivery, messages) = match entry {
                AcpHistoryReplayEntry::User { text, .. } => {
                    if text.trim().is_empty() {
                        return None;
                    }
                    (
                        false,
                        vec![psychevo::AgentImportedMessage {
                            message: Message::User {
                                content: vec![UserContentBlock::text(text.clone())],
                                timestamp_ms,
                            },
                            usage: None,
                            metadata: Some(acp_history_message_metadata(
                                &replay_id,
                                &delivery_message_ids,
                                replay_turn_id,
                                None,
                            )),
                        }],
                    )
                }
                AcpHistoryReplayEntry::Assistant {
                    content_slots,
                    tools,
                    plan,
                    ..
                } => {
                    let content = persisted_assistant_content(content_slots, tools);
                    if content.is_empty() && plan.is_none() {
                        return None;
                    }
                    let mut messages = vec![psychevo::AgentImportedMessage {
                        message: Message::Assistant {
                            content,
                            timestamp_ms,
                            finish_reason: Some("end_turn".to_string()),
                            outcome: Outcome::Normal,
                            model: Some(peer.agent.name.clone()),
                            provider: Some(format!("acp:{}", peer.backend.id)),
                        },
                        usage: None,
                        metadata: Some(acp_history_message_metadata(
                            &replay_id,
                            &delivery_message_ids,
                            replay_turn_id,
                            plan.as_ref(),
                        )),
                    }];
                    messages.extend(
                        persisted_tool_result_messages_at(content_slots, tools, timestamp_ms)
                            .into_iter()
                            .map(|message| psychevo::AgentImportedMessage {
                                message,
                                usage: None,
                                metadata: None,
                            }),
                    );
                    (true, messages)
                }
            };
            Some(ProjectedAcpHistoryEntry {
                replay_id,
                delivery_message_ids,
                reconciles_delivery,
                messages,
            })
        })
        .collect()
}

pub(crate) fn project_imported_acp_replay(
    peer: &ResolvedPeerTurn,
    replay: &AcpHistoryReplayProjection,
) -> Vec<psychevo::AgentImportedMessage> {
    project_acp_history_replay(peer, replay, gateway_now_ms(), None)
        .into_iter()
        .flat_map(|entry| entry.messages)
        .collect()
}

async fn commit_acp_replay_and_current_input(
    journal: &AcpHistoryJournal,
    peer: &ResolvedPeerTurn,
    replay: &AcpHistoryReplayProjection,
    current_user_text: &str,
) -> psychevo::Result<()> {
    commit_acp_replay(journal, peer, replay).await?;
    journal
        .append_message(Message::User {
            content: vec![UserContentBlock::text(current_user_text.to_string())],
            timestamp_ms: gateway_now_ms(),
        })
        .await
}

async fn commit_acp_replay(
    journal: &AcpHistoryJournal,
    peer: &ResolvedPeerTurn,
    replay: &AcpHistoryReplayProjection,
) -> psychevo::Result<()> {
    let prior_unknown = journal.prior_unknown_delivery().await?;
    let projection =
        project_acp_history_replay(peer, replay, gateway_now_ms(), prior_unknown.as_deref());
    let replay_message_ids = projection
        .iter()
        .filter(|entry| entry.reconciles_delivery)
        .flat_map(|entry| entry.delivery_message_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let projected_replay_ids = projection
        .iter()
        .map(|entry| entry.replay_id.clone())
        .collect::<BTreeSet<_>>();
    let mut existing_replay_ids = BTreeSet::new();
    let mut reconciliation_evidence = BTreeSet::new();
    journal
        .for_each_message(|_, metadata| {
            let ids = acp_message_ids(metadata);
            if let Some(replay_id) = acp_replay_id(metadata) {
                if projected_replay_ids.contains(replay_id) {
                    existing_replay_ids.insert(replay_id.to_string());
                }
            } else {
                existing_replay_ids.extend(
                    ids.iter()
                        .filter(|message_id| projected_replay_ids.contains(*message_id))
                        .cloned(),
                );
            }
            if prior_unknown
                .as_ref()
                .is_some_and(|unknown| acp_message_turn_id(metadata) == Some(unknown.as_str()))
            {
                reconciliation_evidence.extend(
                    ids.into_iter()
                        .filter(|message_id| replay_message_ids.contains(message_id)),
                );
            }
        })
        .await?;

    for entry in projection {
        if existing_replay_ids.contains(&entry.replay_id) {
            continue;
        }
        for message in entry.messages {
            journal
                .append_message_with_metrics(message.message, message.usage, message.metadata)
                .await?;
        }
        if prior_unknown.is_some() && entry.reconciles_delivery {
            reconciliation_evidence.extend(entry.delivery_message_ids.iter().cloned());
        }
        existing_replay_ids.insert(entry.replay_id);
    }

    if let Some(prior_unknown) = prior_unknown
        && !reconciliation_evidence.is_empty()
    {
        let evidence_ids = reconciliation_evidence.into_iter().collect::<Vec<_>>();
        let metadata = json!({
            "reconciledFrom": "agent_history",
            "replayMessageIds": evidence_ids,
        });
        if !journal
            .reconcile_unknown_delivery(&prior_unknown, metadata)
            .await?
        {
            return Err(agent_session_error(
                "unknown_delivery_reconciliation_race",
                AgentErrorStage::History,
                "safe_retry",
                "not_delivered",
                "The prior unknown delivery changed while Agent history was being committed.",
                Some(format!("turn:{prior_unknown}")),
            ));
        }
    }

    Ok(())
}

pub(crate) async fn run_acp_peer_turn(
    pool: &AcpProcessPool,
    peer: ResolvedPeerTurn,
    profile: &psychevo::config::RuntimeProfileConfig,
    request: AcpPeerTurnRequest,
    session_ready: AcpSessionReadyCallback,
) -> psychevo::Result<AcpPeerTurnResult> {
    let session_id = request.thread.id.clone();
    let auto_title_new_session = request.history.latest(Some(1)).await?.items.is_empty();
    let existing_native_id = request.native_session_id.clone();
    let is_new_native_session = existing_native_id.is_none();
    let is_first_gateway_turn = !request.persistence.has_prior_terminal().await?;
    let (peer_model, peer_reasoning_effort, peer_runtime_options) = acp_peer_turn_controls(
        request.model.clone(),
        request.reasoning_effort.clone(),
        request.runtime_options.clone(),
        profile,
        is_new_native_session,
    );
    let native_session_slot = Arc::new(std::sync::Mutex::new(existing_native_id.clone()));
    let prompt_for_history = prompt_history_text(&request.prompt, &request.images);
    let journal = AcpHistoryJournal {
        history: request.history.clone(),
        persistence: request.persistence.clone(),
    };
    let before_prompt_journal = journal.clone();
    let before_prompt_peer = peer.clone();
    let before_prompt_user_text = prompt_for_history.clone();
    let before_prompt: AcpBeforePromptCallback = Arc::new(move |replay| {
        let journal = before_prompt_journal.clone();
        let peer = before_prompt_peer.clone();
        let user_text = before_prompt_user_text.clone();
        Box::pin(async move {
            commit_acp_replay_and_current_input(&journal, &peer, &replay, &user_text).await
        })
    });
    let cwd = PathBuf::from(&request.thread.cwd);
    let home = resolve_skills_home(&peer.env, &cwd)?;
    let acp_context = AcpPeerTurnContext {
        cwd,
        home,
        local_session_id: session_id.clone(),
        native_session_id: existing_native_id,
        native_session_slot: Arc::clone(&native_session_slot),
        input: request.input,
        prompt: request.prompt.clone(),
        images: request.images,
        instructions: (is_new_native_session || is_first_gateway_turn)
            .then(|| peer.agent.instructions.clone())
            .filter(|instructions| !instructions.trim().is_empty()),
        peer_model,
        peer_reasoning_effort,
        peer_runtime_options,
        mcp_servers: request.mcp_servers,
        stream: request.stream.clone(),
        workspace_mutations: request.workspace_mutations,
        approval_handler: request.approval_handler,
        turn_control: request.control,
        before_prompt,
        persistence: request.persistence.clone(),
    };

    emit_runtime_event(
        &request.stream,
        json!({
            "type": "turn_started",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "agent_name": peer.agent.name.clone(),
            "backend_id": peer.backend.id.clone(),
        }),
    );
    let acp = run_acp_stdio_turn(pool, &peer, &acp_context, session_ready).await;
    let acp = match acp {
        Ok(acp) => acp,
        Err(err) if is_acp_peer_abort_error(&err) => {
            emit_runtime_event(
                &acp_context.stream,
                json!({
                    "type": "turn_complete",
                    "session_id": session_id.clone(),
                    "source": "peer_agent",
                    "outcome": "aborted",
                }),
            );
            let turn = psychevo::TurnResult {
                thread_id: session_id.clone(),
                outcome: psychevo::TurnOutcome::Interrupted,
                terminal_reason: None,
                final_answer: String::new(),
                provider: format!("acp:{}", peer.backend.id),
                model: peer.agent.name.clone(),
                reasoning_effort: request.reasoning_effort,
                context_limit: None,
                tool_failures: 0,
                selected_agent: Some(SelectedAgent {
                    name: peer.agent.name.clone(),
                    source: peer.agent.source.as_str().to_string(),
                    path: peer.agent.file_path.clone(),
                }),
                selected_skills: Vec::new(),
                context_snapshot: None,
                terminal_error: None,
                warnings: Vec::new(),
            };
            return Ok(AcpPeerTurnResult { turn });
        }
        Err(err) => {
            emit_runtime_event(
                &acp_context.stream,
                json!({
                    "type": "turn_complete",
                    "session_id": session_id.clone(),
                    "source": "peer_agent",
                    "outcome": "failed",
                    "error": err.to_string(),
                }),
            );
            return Err(err);
        }
    };

    request
        .persistence
        .set_metadata_field(
            ACP_PEER_METADATA_KEY.to_string(),
            Some(peer_session_metadata(
                &peer,
                Some(&acp.native_session_id),
                acp.usage_update.as_ref(),
                &request.runtime_options,
                Some(&acp.session_snapshot),
            )),
        )
        .await?;
    if let Some(title) = acp.session_title.as_deref() {
        let _ = request
            .persistence
            .set_visible_title_if_empty(title.to_string())
            .await;
    } else if auto_title_new_session {
        let title = suggested_thread_title(&prompt_for_history);
        let _ = request.persistence.set_visible_title_if_empty(title).await;
    }
    let assistant_content = acp.persisted_assistant_content();
    let prompt_usage_cumulative = acp.prompt_usage.clone();
    let (prompt_usage, usage_counter_reset) = match prompt_usage_cumulative.as_ref() {
        Some(cumulative) => {
            let (delta, reset) =
                acp_prompt_usage_delta(&journal, &acp.native_session_id, cumulative).await?;
            (Some(delta), reset)
        }
        None => (None, false),
    };
    if !assistant_content.is_empty() || acp.latest_plan.is_some() || acp.prompt_usage.is_some() {
        let message_ids = acp.persisted_assistant_message_ids();
        journal
            .append_message_with_metrics(
                Message::Assistant {
                    content: assistant_content,
                    timestamp_ms: gateway_now_ms(),
                    finish_reason: Some("end_turn".to_string()),
                    outcome: Outcome::Normal,
                    model: Some(peer.agent.name.clone()),
                    provider: Some(format!("acp:{}", peer.backend.id)),
                },
                prompt_usage.clone(),
                acp_live_message_metadata(
                    &message_ids,
                    &request.turn_id,
                    acp.latest_plan.as_ref(),
                    prompt_usage_cumulative.as_ref(),
                    &acp.native_session_id,
                    usage_counter_reset,
                ),
            )
            .await?;
    }
    for message in acp.persisted_tool_result_messages() {
        journal.append_message(message).await?;
    }
    emit_runtime_event(
        &acp_context.stream,
        json!({
            "type": "message_end",
            "session_id": session_id.clone(),
            "message": {
                "role": "assistant",
                "content": acp.final_message_content(),
            },
            "usage": prompt_usage,
        }),
    );
    emit_runtime_event(
        &acp_context.stream,
        json!({
            "type": "turn_complete",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "outcome": "normal",
        }),
    );

    let turn = psychevo::TurnResult {
        thread_id: session_id.clone(),
        outcome: psychevo::TurnOutcome::Completed,
        terminal_reason: None,
        final_answer: acp.final_answer,
        provider: format!("acp:{}", peer.backend.id),
        model: peer.agent.name.clone(),
        reasoning_effort: request.reasoning_effort,
        context_limit: None,
        tool_failures: 0,
        selected_agent: Some(SelectedAgent {
            name: peer.agent.name.clone(),
            source: peer.agent.source.as_str().to_string(),
            path: peer.agent.file_path.clone(),
        }),
        selected_skills: Vec::new(),
        context_snapshot: None,
        terminal_error: None,
        warnings: Vec::new(),
    };
    Ok(AcpPeerTurnResult { turn })
}

fn acp_peer_turn_controls(
    model: Option<String>,
    reasoning_effort: Option<String>,
    mut runtime_options: BTreeMap<String, String>,
    profile: &psychevo::config::RuntimeProfileConfig,
    is_new_native_session: bool,
) -> (Option<String>, Option<String>, BTreeMap<String, String>) {
    let peer_model = runtime_options.remove("model").or(model).or_else(|| {
        is_new_native_session
            .then(|| profile.default_model.clone())
            .flatten()
    });
    let peer_reasoning_effort = runtime_options
        .remove("effort")
        .or_else(|| runtime_options.remove("reasoning"))
        .or(reasoning_effort);
    if is_new_native_session
        && !runtime_options.contains_key("mode")
        && let Some(default_mode) = profile.default_mode.clone()
    {
        runtime_options.insert("mode".to_string(), default_mode);
    }
    (peer_model, peer_reasoning_effort, runtime_options)
}

#[cfg(test)]
mod prompt_usage_tests {
    use serde_json::json;

    use super::cumulative_usage_delta;

    #[test]
    fn cumulative_prompt_usage_becomes_a_non_double_counted_turn_delta() {
        let (delta, reset) = cumulative_usage_delta(
            &json!({
                "total_tokens": 200,
                "input_tokens": 140,
                "output_tokens": 60,
                "reasoning_tokens": 8,
                "cached_tokens": 50
            }),
            &json!({
                "total_tokens": 144,
                "input_tokens": 100,
                "output_tokens": 44,
                "reasoning_tokens": 4,
                "cached_tokens": 30
            }),
        );

        assert_eq!(
            delta,
            json!({
                "total_tokens": 56,
                "input_tokens": 40,
                "output_tokens": 16,
                "reasoning_tokens": 4,
                "cached_tokens": 20
            })
        );
        assert!(!reset);
    }

    #[test]
    fn decreasing_cumulative_fields_start_a_new_counter_delta() {
        let (delta, reset) = cumulative_usage_delta(
            &json!({ "total_tokens": 20, "input_tokens": 15, "output_tokens": 5 }),
            &json!({ "total_tokens": 144, "input_tokens": 100, "output_tokens": 44 }),
        );

        assert_eq!(
            delta,
            json!({ "total_tokens": 20, "input_tokens": 15, "output_tokens": 5 })
        );
        assert!(reset);
    }
}
