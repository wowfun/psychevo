#[allow(unused_imports)]
pub(crate) use super::*;
pub async fn run_agent_loop(
    provider: Arc<dyn GenerationProvider>,
    request: AgentLoopRequest,
    sink: Arc<dyn EventSink>,
    mut control: ControlReceivers,
) -> Result<AgentCompletion> {
    emit(&sink, AgentEvent::AgentStart).await?;

    if control.abort_requested() {
        let completion = AgentCompletion {
            outcome: Outcome::Aborted,
            messages: Vec::new(),
            terminal_reason: None,
        };
        emit(
            &sink,
            AgentEvent::AgentEnd {
                outcome: completion.outcome,
                messages: completion.messages.clone(),
                terminal_reason: completion.terminal_reason,
            },
        )
        .await?;
        return Ok(completion);
    }

    let mut context = request.previous_messages.clone();
    context.extend(request.context_messages.iter().cloned());
    let mut new_messages = Vec::new();
    let mut turn_index = 0usize;
    let mut tool_router =
        ToolRouter::from_tools(request.tools.clone()).with_tool_search(request.tool_search);

    emit(&sink, AgentEvent::TurnStart { turn_index }).await?;
    for message in request.prompt_messages.iter().cloned() {
        context.push(message.clone());
        new_messages.push(message.clone());
        emit(
            &sink,
            AgentEvent::MessageStart {
                message: message.clone(),
            },
        )
        .await?;
        emit(
            &sink,
            AgentEvent::MessageEnd {
                message,
                usage: None,
                metadata: None,
            },
        )
        .await?;
    }
    drain_external_user_messages(&mut control, &mut context, &mut new_messages, &sink, true)
        .await?;

    loop {
        if turn_index >= request.max_turns {
            let outcome = Outcome::Failed;
            let terminal_reason = Some(TerminalReason::MaxTurnsExceeded {
                max_turns: request.max_turns,
            });
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                    terminal_reason,
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
                terminal_reason,
            });
        }

        if control.abort_requested() {
            let outcome = Outcome::Aborted;
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                    terminal_reason: None,
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
                terminal_reason: None,
            });
        }

        drain_external_user_messages(&mut control, &mut context, &mut new_messages, &sink, true)
            .await?;

        let assistant = stream_assistant(
            Arc::clone(&provider),
            &request,
            &tool_router,
            &context,
            Arc::clone(&sink),
            control.abort_signal(),
        )
        .await?;

        let assistant_outcome = assistant_outcome(&assistant);
        context.push(assistant.clone());
        new_messages.push(assistant.clone());
        let injected_after_generation = drain_external_user_messages(
            &mut control,
            &mut context,
            &mut new_messages,
            &sink,
            false,
        )
        .await?;

        if assistant_outcome != Outcome::Normal {
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome: assistant_outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome: assistant_outcome,
                    messages: new_messages.clone(),
                    terminal_reason: None,
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome: assistant_outcome,
                messages: new_messages,
                terminal_reason: None,
            });
        }

        let tool_calls = assistant_tool_calls(&assistant);
        if !tool_calls.is_empty() {
            let tool_results = execute_tool_batch(
                &mut tool_router,
                &tool_calls,
                Arc::clone(&sink),
                control.abort_signal(),
            )
            .await?;
            for result in tool_results {
                context.push(result.clone());
                new_messages.push(result.clone());
                emit(
                    &sink,
                    AgentEvent::MessageStart {
                        message: result.clone(),
                    },
                )
                .await?;
                emit(
                    &sink,
                    AgentEvent::MessageEnd {
                        message: result,
                        usage: None,
                        metadata: None,
                    },
                )
                .await?;
            }
        }
        let injected_after_tools = injected_after_generation
            || drain_external_user_messages(
                &mut control,
                &mut context,
                &mut new_messages,
                &sink,
                true,
            )
            .await?;

        let terminal = if control.abort_requested() {
            Some(Outcome::Aborted)
        } else if control.stop_requested() {
            Some(Outcome::Stopped)
        } else if tool_calls.is_empty() && !injected_after_tools {
            Some(Outcome::Normal)
        } else {
            None
        };

        if let Some(outcome) = terminal {
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                    terminal_reason: None,
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
                terminal_reason: None,
            });
        }

        emit(
            &sink,
            AgentEvent::TurnEnd {
                turn_index,
                outcome: Outcome::Normal,
            },
        )
        .await?;
        turn_index += 1;
        emit(&sink, AgentEvent::TurnStart { turn_index }).await?;
    }
}

pub(crate) async fn drain_external_user_messages(
    control: &mut ControlReceivers,
    context: &mut Vec<Message>,
    new_messages: &mut Vec<Message>,
    sink: &Arc<dyn EventSink>,
    include_pending: bool,
) -> Result<bool> {
    let messages = control.drain_injected_messages();
    let mut had_messages = !messages.is_empty();
    context.extend(messages);
    if include_pending {
        for (id, message) in control.drain_pending_user_messages() {
            had_messages = true;
            context.push(message.clone());
            new_messages.push(message.clone());
            emit(
                sink,
                AgentEvent::MessageStart {
                    message: message.clone(),
                },
            )
            .await?;
            emit(
                sink,
                AgentEvent::MessageEnd {
                    message,
                    usage: None,
                    metadata: Some(json!({
                        "pending_input": {
                            "id": id.as_u64(),
                            "kind": "steer",
                        }
                    })),
                },
            )
            .await?;
        }
    }
    Ok(had_messages)
}
