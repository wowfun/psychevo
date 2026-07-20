impl GatewayLiveProjector {
    fn upsert_block(&mut self, segment: usize, block: TranscriptBlock) {
        self.entry_state_mut(segment).upsert_block(block);
    }

    fn replace_blocks(&mut self, segment: usize, blocks: BTreeMap<String, TranscriptBlock>) {
        self.entry_state_mut(segment).replace_blocks(blocks);
    }

    fn entry_state_mut(&mut self, segment: usize) -> &mut LiveEntryState {
        self.entries
            .entry(segment)
            .or_insert_with(|| LiveEntryState::new(segment))
    }

    fn emit_entry_event(
        &mut self,
        turn_id: &str,
        segment: usize,
        completed: bool,
        authoritative_blocks: bool,
    ) -> GatewayEvent {
        self.stream_seq += 1;
        let stream_seq = self.stream_seq;
        let state = self.entry_state_mut(segment);
        let was_started = state.started;
        state.started = true;
        state.updated_at_ms = crate::gateway_now_ms();
        let entry = state.to_entry(turn_id, stream_seq, authoritative_blocks);
        if completed {
            GatewayEvent::EntryCompleted {
                turn_id: turn_id.to_string(),
                entry,
            }
        } else if !was_started {
            GatewayEvent::EntryStarted {
                turn_id: turn_id.to_string(),
                entry,
            }
        } else {
            GatewayEvent::EntryUpdated {
                turn_id: turn_id.to_string(),
                entry,
            }
        }
    }

    fn advance_assistant_segment(&mut self) {
        self.assistant_segment += 1;
    }

    fn prepare_turn(&mut self, turn_id: &str) {
        if self.active_turn_id.as_deref() == Some(turn_id) {
            return;
        }
        if self.active_turn_id.is_some() {
            self.reset_turn_state();
        }
        self.active_turn_id = Some(turn_id.to_string());
    }

    fn reset_turn_state(&mut self) {
        self.active_turn_id = None;
        self.assistant_segment = 0;
        self.entries.clear();
        self.tool_owners.clear();
        self.tool_aliases.clear();
        self.tool_positions.clear();
        self.tool_args.clear();
        self.write_previews.clear();
        self.exec_sessions.clear();
    }

    fn attach_thread_id(&mut self, event: &mut GatewayEvent) {
        if self.thread_id.is_none()
            && let Some(thread_id) = event_thread_id(event)
        {
            self.thread_id = Some(thread_id);
        }
        let Some(thread_id) = self.thread_id.as_deref() else {
            return;
        };
        match event {
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => {
                if entry.thread_id.is_empty() {
                    entry.thread_id = thread_id.to_string();
                }
            }
            GatewayEvent::TurnStarted {
                thread_id: event_thread_id,
                ..
            }
            | GatewayEvent::TurnQueued {
                thread_id: event_thread_id,
                ..
            }
            | GatewayEvent::ActivityChanged {
                thread_id: event_thread_id,
                ..
            } => {
                if event_thread_id.is_none() {
                    *event_thread_id = Some(thread_id.to_string());
                }
            }
            GatewayEvent::TurnCompleted {
                thread_id: event_thread_id,
                turn,
                committed_entries,
                ..
            } => {
                if event_thread_id.is_none() {
                    *event_thread_id = Some(thread_id.to_string());
                }
                if turn.thread_id.is_none() {
                    turn.thread_id = Some(thread_id.to_string());
                }
                for entry in committed_entries {
                    if entry.thread_id.is_empty() {
                        entry.thread_id = thread_id.to_string();
                    }
                }
            }
            _ => {}
        }
    }
}

fn force_event_thread_id(event: &mut GatewayEvent, thread_id: &str) {
    match event {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            entry.thread_id = thread_id.to_string();
        }
        GatewayEvent::TurnStarted {
            thread_id: event_thread_id,
            ..
        }
        | GatewayEvent::TurnQueued {
            thread_id: event_thread_id,
            ..
        }
        | GatewayEvent::ActivityChanged {
            thread_id: event_thread_id,
            ..
        } => {
            *event_thread_id = Some(thread_id.to_string());
        }
        GatewayEvent::TurnCompleted {
            thread_id: event_thread_id,
            turn,
            committed_entries,
            ..
        } => {
            *event_thread_id = Some(thread_id.to_string());
            turn.thread_id = Some(thread_id.to_string());
            for entry in committed_entries {
                entry.thread_id = thread_id.to_string();
            }
        }
        _ => {}
    }
}
