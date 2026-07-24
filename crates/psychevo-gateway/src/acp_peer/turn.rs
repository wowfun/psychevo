const ACP_PEER_ABORT_MESSAGE: &str = "ACP peer turn aborted";

#[derive(Debug)]
pub(crate) struct AcpPeerTurnResult {
    pub(crate) run: RunResult,
    pub(crate) native_session_id: String,
}

#[derive(Clone)]
struct AcpClientContext {
    cwd: PathBuf,
    fs_read: bool,
    fs_write: bool,
    approval_handler: Option<Arc<dyn psychevo_runtime::types::ApprovalHandler>>,
    clarify_control: Option<RunControlHandle>,
    terminal: bool,
    terminal_env: BTreeMap<String, String>,
    stream: Option<RunStreamSink>,
    abort: Option<AbortSignal>,
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

fn acp_prompt_usage_delta(
    store: &psychevo_runtime::state::StateRuntime,
    session_id: &str,
    native_session_id: &str,
    cumulative: &Value,
) -> psychevo_runtime::Result<(Value, bool)> {
    let previous = store
        .load_tui_message_summaries(session_id)?
        .into_iter()
        .rev()
        .filter(|summary| {
            matches!(summary.message, psychevo_agent_core::Message::Assistant { .. })
        })
        .find_map(|summary| {
            let metadata = summary.metadata?;
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
    let metadata = metadata
        .get_or_insert_with(|| json!({ "acp": { "messageIds": [], "origin": "live" } }));
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
        acp.insert(
            "replayId".to_string(),
            Value::String(replay_id.to_string()),
        );
    }
    metadata
}

fn commit_acp_replay_and_current_input(
    state: &psychevo_runtime::state::StateRuntime,
    peer: &ResolvedPeerTurn,
    session_id: &str,
    current_turn_id: &str,
    replay: &AcpHistoryReplayProjection,
    current_user_text: &str,
) -> psychevo_runtime::Result<()> {
    commit_acp_replay(
        state,
        peer,
        session_id,
        Some(current_turn_id),
        replay,
    )?;
    state.append_message(
        session_id,
        &Message::User {
            content: vec![UserContentBlock::text(current_user_text.to_string())],
            timestamp_ms: gateway_now_ms(),
        },
    )
}

pub(crate) fn commit_imported_acp_replay(
    state: &psychevo_runtime::state::StateRuntime,
    peer: &ResolvedPeerTurn,
    session_id: &str,
    replay: &AcpHistoryReplayProjection,
) -> psychevo_runtime::Result<()> {
    commit_acp_replay(state, peer, session_id, None, replay)
}

fn commit_acp_replay(
    state: &psychevo_runtime::state::StateRuntime,
    peer: &ResolvedPeerTurn,
    session_id: &str,
    current_turn_id: Option<&str>,
    replay: &AcpHistoryReplayProjection,
) -> psychevo_runtime::Result<()> {
    let store = state.clone();
    let unknown = match current_turn_id {
        Some(current_turn_id) => {
            store.unknown_gateway_turn_deliveries_for_thread(session_id, current_turn_id)?
        }
        None => Vec::new(),
    };
    if unknown.len() > 1 {
        return Err(crate::agent_session_error(
            "multiple_unknown_deliveries",
            crate::AgentErrorStage::History,
            "never",
            "not_delivered",
            "A Thread has multiple unresolved unknown deliveries; Agent history cannot be assigned unambiguously.",
            Some(format!("thread:{session_id}")),
        ));
    }
    let prior_unknown = unknown.first();
    let replay_message_ids = replay
        .entries
        .iter()
        .filter(|entry| matches!(entry, AcpHistoryReplayEntry::Assistant { .. }))
        .flat_map(replay_entry_delivery_message_ids)
        .collect::<BTreeSet<_>>();
    let existing = store.load_tui_message_summaries(session_id)?;
    let mut existing_replay_ids = BTreeSet::new();
    let mut reconciliation_evidence = BTreeSet::new();
    for summary in &existing {
        let ids = acp_message_ids(summary.metadata.as_ref());
        if let Some(replay_id) = acp_replay_id(summary.metadata.as_ref()) {
            existing_replay_ids.insert(replay_id.to_string());
        } else {
            existing_replay_ids.extend(ids.iter().cloned());
        }
        if prior_unknown.is_some_and(|unknown| {
            acp_message_turn_id(summary.metadata.as_ref()) == Some(unknown.turn_id.as_str())
        }) {
            reconciliation_evidence.extend(
                ids.into_iter()
                    .filter(|message_id| replay_message_ids.contains(message_id)),
            );
        }
    }

    for entry in &replay.entries {
        let replay_id = replay_entry_identity(entry);
        if existing_replay_ids.contains(replay_id) {
            continue;
        }
        let delivery_message_ids = replay_entry_delivery_message_ids(entry);
        let turn_id = prior_unknown.map(|unknown| unknown.turn_id.as_str());
        let replay_turn_id = turn_id.or(Some(replay_id));
        match entry {
            AcpHistoryReplayEntry::User { text, .. } => {
                if text.trim().is_empty() {
                    continue;
                }
                store.append_message_with_metrics(
                    session_id,
                    &Message::User {
                        content: vec![UserContentBlock::text(text.clone())],
                        timestamp_ms: gateway_now_ms(),
                    },
                    None,
                    Some(acp_history_message_metadata(
                        replay_id,
                        &delivery_message_ids,
                        replay_turn_id,
                        None,
                    )),
                )?;
            }
            AcpHistoryReplayEntry::Assistant {
                content_slots,
                tools,
                plan,
                ..
            } => {
                let content = persisted_assistant_content(content_slots, tools);
                if content.is_empty() && plan.is_none() {
                    continue;
                }
                store.append_message_with_metrics(
                    session_id,
                    &Message::Assistant {
                        content,
                        timestamp_ms: gateway_now_ms(),
                        finish_reason: Some("end_turn".to_string()),
                        outcome: Outcome::Normal,
                        model: Some(peer.agent.name.clone()),
                        provider: Some(format!("acp:{}", peer.backend.id)),
                    },
                    None,
                    Some(acp_history_message_metadata(
                        replay_id,
                        &delivery_message_ids,
                        replay_turn_id,
                        plan.as_ref(),
                    )),
                )?;
                for message in persisted_tool_result_messages(content_slots, tools) {
                    store.append_message(session_id, &message)?;
                }
                if prior_unknown.is_some() {
                    reconciliation_evidence.extend(delivery_message_ids.iter().cloned());
                }
            }
        }
        existing_replay_ids.insert(replay_id.to_string());
    }

    if let Some(prior_unknown) = prior_unknown
        && !reconciliation_evidence.is_empty()
    {
        let evidence_ids = reconciliation_evidence.into_iter().collect::<Vec<_>>();
        let metadata = json!({
            "reconciledFrom": "agent_history",
            "replayMessageIds": evidence_ids,
        });
        if !store.reconcile_unknown_gateway_turn_delivery(
            &prior_unknown.turn_id,
            session_id,
            Some(&metadata),
        )? {
            return Err(crate::agent_session_error(
                "unknown_delivery_reconciliation_race",
                crate::AgentErrorStage::History,
                "safe_retry",
                "not_delivered",
                "The prior unknown delivery changed while Agent history was being committed.",
                Some(format!("turn:{}", prior_unknown.turn_id)),
            ));
        }
    }

    Ok(())
}

pub(crate) async fn run_acp_peer_turn(
    pool: &AcpProcessPool,
    peer: ResolvedPeerTurn,
    profile: &psychevo_runtime::config::RuntimeProfileConfig,
    request: BackendTurnRequest,
    turn_id: String,
    session_ready: AcpSessionReadyCallback,
    delivery_observer: crate::AgentDeliveryObserver,
) -> psychevo_runtime::Result<AcpPeerTurnResult> {
    let clarify_control = request.control.as_ref().map(|control| control.handle());
    let abort = request
        .control
        .as_ref()
        .map(|control| control.abort_signal());
    let input = request.input;
    let options = request.options;
    let state = options.state.clone();
    let store = state.clone();
    let local_session = ensure_local_session(&peer, &options)?;
    let session_id = local_session.session_id;
    let auto_title_new_session = local_session.created;
    let existing_native_id = local_session
        .native_session_id
        .or(options.runtime_session_id.clone());
    let is_new_native_session = existing_native_id.is_none();
    let is_first_gateway_turn = store
        .list_gateway_turn_terminals_for_thread(&session_id)?
        .is_empty();
    let mcp_servers = resolve_peer_mcp_server_handoffs(&peer, &options)?;
    let (peer_model, peer_reasoning_effort, peer_runtime_options) =
        acp_peer_turn_controls(&options, profile, is_new_native_session);
    let native_session_slot = Arc::new(std::sync::Mutex::new(existing_native_id.clone()));
    let prompt_for_history = prompt_history_text(&options.prompt, &options.image_inputs);
    let before_prompt_state = state.clone();
    let before_prompt_peer = peer.clone();
    let before_prompt_session_id = session_id.clone();
    let before_prompt_turn_id = turn_id.clone();
    let before_prompt_user_text = prompt_for_history.clone();
    let before_prompt: AcpBeforePromptCallback = Arc::new(move |replay| {
        commit_acp_replay_and_current_input(
            &before_prompt_state,
            &before_prompt_peer,
            &before_prompt_session_id,
            &before_prompt_turn_id,
            replay,
            &before_prompt_user_text,
        )
    });
    let home = resolve_skills_home(&peer.env, &options.cwd)?;
    let acp_context = AcpPeerTurnContext {
        cwd: options.cwd.clone(),
        home,
        local_session_id: session_id.clone(),
        native_session_id: existing_native_id,
        native_session_slot: Arc::clone(&native_session_slot),
        input,
        prompt: options.prompt.clone(),
        images: options.image_inputs.clone(),
        instructions: (is_new_native_session || is_first_gateway_turn)
            .then(|| peer.agent.instructions.clone())
            .filter(|instructions| !instructions.trim().is_empty()),
        peer_model,
        peer_reasoning_effort,
        peer_runtime_options,
        mcp_servers,
        stream: request.stream.clone(),
        workspace_mutations: options.workspace_mutations.clone(),
        approval_handler: options.approval_handler.clone(),
        clarify_control,
        abort,
        before_prompt,
        delivery_observer,
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
                &request.stream,
                json!({
                    "type": "turn_complete",
                    "session_id": session_id.clone(),
                    "source": "peer_agent",
                    "outcome": "aborted",
                }),
            );
            let run = RunResult {
                session_id: session_id.clone(),
                outcome: Outcome::Aborted,
                terminal_reason: None,
                final_answer: String::new(),
                db_path: state.db_path().to_path_buf(),
                cwd: options.cwd,
                provider: format!("acp:{}", peer.backend.id),
                model: peer.agent.name.clone(),
                base_url: String::new(),
                api_key_env: None,
                reasoning_effort: options.reasoning_effort,
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
                events: Vec::new(),
                warnings: Vec::new(),
            };
            return Ok(AcpPeerTurnResult {
                run,
                native_session_id: native_session_slot
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone())
                    .unwrap_or_default(),
            });
        }
        Err(err) => {
            emit_runtime_event(
                &request.stream,
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

    store.set_session_metadata_field(
        &session_id,
        ACP_PEER_METADATA_KEY,
        Some(peer_session_metadata(
            &peer,
            Some(&acp.native_session_id),
            acp.usage_update.as_ref(),
            &options.runtime_options,
            Some(&acp.session_snapshot),
        )),
    )?;
    if let Some(title) = acp.session_title.as_deref() {
        set_session_title_if_empty(&store, &session_id, title);
    } else if auto_title_new_session {
        let title = fallback_visible_session_title(&prompt_for_history);
        set_session_title_if_empty(&store, &session_id, &title);
    }
    let assistant_content = acp.persisted_assistant_content();
    let prompt_usage_cumulative = acp.prompt_usage.clone();
    let (prompt_usage, usage_counter_reset) = match prompt_usage_cumulative.as_ref() {
        Some(cumulative) => {
            let (delta, reset) = acp_prompt_usage_delta(
                &store,
                &session_id,
                &acp.native_session_id,
                cumulative,
            )?;
            (Some(delta), reset)
        }
        None => (None, false),
    };
    if !assistant_content.is_empty() || acp.latest_plan.is_some() || acp.prompt_usage.is_some() {
        let message_ids = acp.persisted_assistant_message_ids();
        store.append_message_with_metrics(
            &session_id,
            &Message::Assistant {
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
                &turn_id,
                acp.latest_plan.as_ref(),
                prompt_usage_cumulative.as_ref(),
                &acp.native_session_id,
                usage_counter_reset,
            ),
        )?;
    }
    for message in acp.persisted_tool_result_messages() {
        store.append_message(&session_id, &message)?;
    }
    emit_runtime_event(
        &request.stream,
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
        &request.stream,
        json!({
            "type": "turn_complete",
            "session_id": session_id.clone(),
            "source": "peer_agent",
            "outcome": "normal",
        }),
    );

    let run = RunResult {
        session_id: session_id.clone(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: acp.final_answer,
        db_path: state.db_path().to_path_buf(),
        cwd: options.cwd,
        provider: format!("acp:{}", peer.backend.id),
        model: peer.agent.name.clone(),
        base_url: String::new(),
        api_key_env: None,
        reasoning_effort: options.reasoning_effort,
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
        events: acp.events,
        warnings: Vec::new(),
    };
    Ok(AcpPeerTurnResult {
        run,
        native_session_id: acp.native_session_id,
    })
}

fn acp_peer_turn_controls(
    options: &psychevo_runtime::types::RunOptions,
    profile: &psychevo_runtime::config::RuntimeProfileConfig,
    is_new_native_session: bool,
) -> (Option<String>, Option<String>, BTreeMap<String, String>) {
    let mut runtime_options = options.runtime_options.clone();
    let peer_model = runtime_options
        .remove("model")
        .or_else(|| options.model.clone())
        .or_else(|| is_new_native_session.then(|| profile.default_model.clone()).flatten());
    let peer_reasoning_effort = runtime_options
        .remove("effort")
        .or_else(|| runtime_options.remove("reasoning"))
        .or_else(|| options.reasoning_effort.clone());
    if is_new_native_session
        && !runtime_options.contains_key("mode")
        && let Some(default_mode) = profile.default_mode.clone()
    {
        runtime_options.insert("mode".to_string(), default_mode);
    }
    (peer_model, peer_reasoning_effort, runtime_options)
}

fn set_session_title_if_empty(
    store: &psychevo_runtime::state::StateRuntime,
    session_id: &str,
    title: &str,
) {
    if store
        .session_summary(session_id)
        .ok()
        .flatten()
        .and_then(|summary| summary.title)
        .is_some_and(|title| !title.trim().is_empty())
    {
        return;
    }
    let _ = store.set_session_title(session_id, title);
}

#[cfg(test)]
mod prompt_usage_tests {
    use super::*;

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
