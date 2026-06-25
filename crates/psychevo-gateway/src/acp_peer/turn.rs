const ACP_PEER_ABORT_MESSAGE: &str = "ACP peer turn aborted";

#[derive(Debug)]
pub(crate) struct AcpPeerTurnResult {
    pub(crate) run: RunResult,
    pub(crate) native_session_id: String,
}

#[derive(Debug)]
pub(crate) struct AcpPeerRuntimeOptions {
    pub(crate) native_session_id: Option<String>,
    pub(crate) options: Vec<wire::RuntimeConfigOptionView>,
}

#[derive(Clone)]
struct AcpClientContext {
    workdir: PathBuf,
    fs_read: bool,
    fs_write: bool,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
}

pub(crate) async fn run_acp_peer_turn(
    peer: ResolvedPeerTurn,
    request: BackendTurnRequest,
    _turn_id: String,
) -> psychevo_runtime::Result<AcpPeerTurnResult> {
    let abort = request
        .control
        .as_ref()
        .map(|control| control.abort_signal());
    let options = request.options;
    let state = options.state.clone();
    let store = state.store();
    let local_session = ensure_local_session(&peer, &options)?;
    let session_id = local_session.session_id;
    let auto_title_new_session = local_session.created;
    let existing_native_id = local_session
        .native_session_id
        .or(options.runtime_session_id.clone());
    let is_new_native_session = existing_native_id.is_none();
    let prompt = peer_prompt_text(
        &peer.agent,
        &options.prompt,
        &options.image_inputs,
        is_new_native_session,
    );
    let prompt_for_history = prompt_history_text(&options.prompt, &options.image_inputs);
    let acp_context = AcpPeerTurnContext {
        workdir: options.workdir.clone(),
        local_session_id: session_id.clone(),
        native_session_id: existing_native_id,
        prompt,
        peer_model: options.model.clone(),
        peer_reasoning_effort: options.reasoning_effort.clone(),
        peer_runtime_mode: options.runtime_options.get("mode").cloned(),
        stream: request.stream.clone(),
        approval_handler: options.approval_handler.clone(),
        abort,
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
    store.append_message(
        &session_id,
        &Message::User {
            content: vec![UserContentBlock::text(prompt_for_history.clone())],
            timestamp_ms: gateway_now_ms(),
        },
    )?;

    let acp = run_acp_stdio_turn(&peer, &acp_context).await;
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
                workdir: options.workdir,
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
                events: Vec::new(),
                warnings: Vec::new(),
            };
            return Ok(AcpPeerTurnResult {
                run,
                native_session_id: acp_context.native_session_id.unwrap_or_default(),
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
            store.append_message(
                &session_id,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: err.to_string(),
                    }],
                    timestamp_ms: gateway_now_ms(),
                    finish_reason: Some("error".to_string()),
                    outcome: Outcome::Failed,
                    model: Some(peer.agent.name.clone()),
                    provider: Some(format!("acp:{}", peer.backend.id)),
                },
            )?;
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
        )),
    )?;
    if let Some(title) = acp.session_title.as_deref() {
        set_session_title_if_empty(store, &session_id, title);
    } else if auto_title_new_session {
        let title = fallback_visible_session_title(&prompt_for_history);
        set_session_title_if_empty(store, &session_id, &title);
    }
    let assistant_content = acp.persisted_assistant_content();
    if !assistant_content.is_empty() {
        store.append_message(
            &session_id,
            &Message::Assistant {
                content: assistant_content,
                timestamp_ms: gateway_now_ms(),
                finish_reason: Some("end_turn".to_string()),
                outcome: Outcome::Normal,
                model: Some(peer.agent.name.clone()),
                provider: Some(format!("acp:{}", peer.backend.id)),
            },
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
        workdir: options.workdir,
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
        events: acp.events,
        warnings: Vec::new(),
    };
    Ok(AcpPeerTurnResult {
        run,
        native_session_id: acp.native_session_id,
    })
}

fn set_session_title_if_empty(
    store: &psychevo_runtime::SqliteStore,
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
