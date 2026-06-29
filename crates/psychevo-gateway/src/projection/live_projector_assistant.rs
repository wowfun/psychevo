impl GatewayLiveProjector {
    fn project_reasoning_delta(&mut self, turn_id: &str, text: &str) -> Option<GatewayEvent> {
        if text.is_empty() {
            return None;
        }
        let segment = self.assistant_segment;
        let block_id = live_reasoning_block_id(turn_id, segment);
        let current = self
            .entries
            .get(&segment)
            .and_then(|state| state.blocks.get(&block_id))
            .and_then(|block| block.body.as_deref())
            .unwrap_or_default();
        let body = format!("{current}{text}");
        let block = live_block(
            block_id,
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            DEFAULT_REASONING_ORDER,
            Some("Thinking".to_string()),
            Some(body),
            Some(json!({
                "projection": "reasoning",
                "origin": "run_stream_reasoning",
                "liveOrder": DEFAULT_REASONING_ORDER,
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }

    fn project_reasoning_end(&mut self, turn_id: &str) -> Option<GatewayEvent> {
        let segment = self.assistant_segment;
        let block_id = live_reasoning_block_id(turn_id, segment);
        let body = self
            .entries
            .get(&segment)
            .and_then(|state| state.blocks.get(&block_id))
            .and_then(|block| block.body.clone())
            .filter(|body| !body.trim().is_empty())?;
        let block = live_block(
            block_id,
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            DEFAULT_REASONING_ORDER,
            Some("Thinking".to_string()),
            Some(body),
            Some(json!({
                "projection": "reasoning",
                "origin": "run_stream_reasoning",
                "liveOrder": DEFAULT_REASONING_ORDER,
            })),
        );
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }

    fn project_assistant_message_event(
        &mut self,
        turn_id: &str,
        value: &Value,
        status: TranscriptBlockStatus,
        completed: bool,
    ) -> Option<GatewayEvent> {
        let message = value.get("message")?;
        let segment = self.assistant_segment;
        let is_tool_call_turn = assistant_message_is_tool_call_turn(Some(message));
        let content = message.get("content").and_then(Value::as_array);
        let visible = content.is_some_and(|content| {
            self.replace_assistant_content_blocks(
                turn_id,
                value,
                content,
                segment,
                status,
                is_tool_call_turn,
            )
        });
        if !visible {
            return None;
        }
        Some(self.emit_entry_event(turn_id, segment, completed, true))
    }

    fn replace_assistant_content_blocks(
        &mut self,
        turn_id: &str,
        event_value: &Value,
        content: &[Value],
        segment: usize,
        status: TranscriptBlockStatus,
        is_tool_call_turn: bool,
    ) -> bool {
        let mut blocks = BTreeMap::new();
        let mut text_ordinal = 0usize;
        for (index, content_block) in content.iter().enumerate() {
            let text_ordinal_for_block =
                if content_block.get("type").and_then(Value::as_str) == Some("text") {
                    let ordinal = text_ordinal;
                    text_ordinal += 1;
                    Some(ordinal)
                } else {
                    None
                };
            let Some(block) = self.build_assistant_content_block(AssistantContentProjection {
                turn_id,
                event_value,
                content_block,
                index,
                text_ordinal: text_ordinal_for_block,
                segment,
                status,
                is_tool_call_turn,
            }) else {
                continue;
            };
            blocks.insert(block.id.clone(), block);
        }
        if !blocks.is_empty()
            && !blocks
                .values()
                .any(|block| block.kind == TranscriptBlockKind::Reasoning)
            && let Some(reasoning) = self.preserved_run_stream_reasoning_block(segment)
        {
            blocks.insert(reasoning.id.clone(), reasoning);
        }
        for block in self.preserved_acp_peer_blocks(segment) {
            blocks.entry(block.id.clone()).or_insert(block);
        }
        if blocks.is_empty() {
            return false;
        }
        self.replace_blocks(segment, blocks);
        true
    }

    fn build_assistant_content_block(
        &mut self,
        projection: AssistantContentProjection<'_>,
    ) -> Option<TranscriptBlock> {
        let AssistantContentProjection {
            turn_id,
            event_value,
            content_block,
            index,
            text_ordinal,
            segment,
            status,
            is_tool_call_turn,
        } = projection;
        match content_block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = content_block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let order = content_block_order(content_block, index, index as i64);
                let mut metadata = if is_tool_call_turn {
                    assistant_phase_metadata(event_value)
                } else {
                    assistant_message_metadata(event_value)
                };
                set_metadata_field(&mut metadata, "content_array_index", json!(index));
                set_metadata_field(&mut metadata, "liveOrder", json!(order));
                Some(live_block(
                    live_text_block_id(turn_id, segment, text_ordinal.unwrap_or(index)),
                    TranscriptBlockKind::Text,
                    status,
                    order,
                    None,
                    Some(text),
                    Some(metadata),
                ))
            }
            Some("reasoning") => {
                let text = content_block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)?;
                let order = content_block_order(content_block, index, DEFAULT_REASONING_ORDER);
                Some(live_block(
                    live_reasoning_block_id(turn_id, segment),
                    TranscriptBlockKind::Reasoning,
                    status,
                    order,
                    Some("Thinking".to_string()),
                    Some(text),
                    Some(json!({
                        "projection": "reasoning",
                        "content_array_index": index,
                        "liveOrder": order,
                    })),
                ))
            }
            Some("tool_call" | "tool_calls" | "tool_use") => {
                let (raw_tool_call_id, tool_name, mut metadata) =
                    tool_message_block_metadata(content_block, index)?;
                let tool_call_id = self.register_tool_content_identity(
                    turn_id,
                    segment,
                    &raw_tool_call_id,
                    &tool_name,
                    &mut metadata,
                );
                if let Some(args) = metadata.get("args").cloned() {
                    self.tool_args.insert(tool_call_id.clone(), args);
                }
                if tool_name == "write_stdin" {
                    return None;
                }
                self.tool_owners.insert(tool_call_id.clone(), segment);
                let order = content_block_order(content_block, index, index as i64);
                Some(self.live_tool_block_from_metadata(LiveToolBlockBuild {
                    turn_id,
                    segment,
                    tool_call_id: &tool_call_id,
                    tool_name: &tool_name,
                    status: TranscriptBlockStatus::Pending,
                    body: None,
                    metadata,
                    order: Some(order),
                }))
            }
            _ => None,
        }
    }

    fn preserved_run_stream_reasoning_block(&self, segment: usize) -> Option<TranscriptBlock> {
        self.entries
            .get(&segment)?
            .blocks
            .values()
            .find(|block| {
                block.kind == TranscriptBlockKind::Reasoning
                    && block
                        .body
                        .as_deref()
                        .or(block.detail.as_deref())
                        .or(block.preview.as_deref())
                        .is_some_and(|body| !body.trim().is_empty())
                    && block
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("origin"))
                        .and_then(Value::as_str)
                        == Some("run_stream_reasoning")
            })
            .cloned()
            .map(|mut block| {
                block.status = TranscriptBlockStatus::Completed;
                block.updated_at_ms = crate::gateway_now_ms();
                block
            })
    }

    fn preserved_acp_peer_blocks(&self, segment: usize) -> Vec<TranscriptBlock> {
        self.entries
            .get(&segment)
            .map(|state| {
                state
                    .blocks
                    .values()
                    .filter(|block| {
                        block.metadata.as_ref().is_some_and(|metadata| {
                            metadata
                                .get("source")
                                .and_then(Value::as_str)
                                .or_else(|| metadata.get("origin").and_then(Value::as_str))
                                == Some("acp_peer")
                                || metadata
                                    .get("metadata")
                                    .and_then(|metadata| metadata.get("origin"))
                                    .and_then(Value::as_str)
                                    == Some("acp_peer")
                        })
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}
