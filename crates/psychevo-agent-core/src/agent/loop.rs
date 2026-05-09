pub async fn run_agent_loop(
    provider: Arc<dyn GenerationProvider>,
    request: AgentLoopRequest,
    sink: Arc<dyn EventSink>,
    control: ControlReceivers,
) -> Result<AgentCompletion> {
    emit(&sink, AgentEvent::AgentStart).await?;

    if control.abort_requested() {
        let completion = AgentCompletion {
            outcome: Outcome::Aborted,
            messages: Vec::new(),
        };
        emit(
            &sink,
            AgentEvent::AgentEnd {
                outcome: completion.outcome,
                messages: completion.messages.clone(),
            },
        )
        .await?;
        return Ok(completion);
    }

    let mut context = request.previous_messages.clone();
    context.extend(request.context_messages.iter().cloned());
    let mut new_messages = Vec::new();
    let mut turn_index = 0usize;

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

    loop {
        if turn_index >= request.max_turns {
            let outcome = Outcome::Failed;
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
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
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
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
            });
        }

        let assistant = stream_assistant(
            Arc::clone(&provider),
            &request,
            &context,
            Arc::clone(&sink),
            control.abort_signal(),
        )
        .await?;

        let assistant_outcome = assistant_outcome(&assistant);
        context.push(assistant.clone());
        new_messages.push(assistant.clone());

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
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome: assistant_outcome,
                messages: new_messages,
            });
        }

        let tool_calls = assistant_tool_calls(&assistant);
        if !tool_calls.is_empty() {
            let tool_results = execute_tool_batch(
                &request.tools,
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

        let terminal = if control.abort_requested() {
            Some(Outcome::Aborted)
        } else if control.stop_requested() {
            Some(Outcome::Stopped)
        } else if tool_calls.is_empty() {
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
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
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
