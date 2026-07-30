#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PendingInputId(u64);

impl PendingInputId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
struct BufferedInput {
    bytes: usize,
    message: Message,
}

#[derive(Debug)]
struct PendingUserInput {
    pub(crate) id: PendingInputId,
    bytes: usize,
    message: Message,
}

pub const MAX_CONTROL_INPUT_ITEMS: usize = 64;
pub const MAX_CONTROL_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ControlInputError {
    #[error("control input is closed")]
    Closed,
    #[error("control input must be a user message")]
    NotUserMessage,
    #[error("unknown pending control input {id}")]
    UnknownInput { id: u64 },
    #[error("control input count limit reached ({limit})")]
    CountLimit { limit: usize },
    #[error("control input byte limit reached ({limit})")]
    ByteLimit { limit: usize },
}

#[derive(Debug, Default)]
struct ControlInputBuffer {
    closed: bool,
    next_id: u64,
    injected: VecDeque<BufferedInput>,
    steered: VecDeque<PendingUserInput>,
    accepted_items: usize,
    accepted_bytes: usize,
}

impl ControlInputBuffer {
    fn admit(&self, items: usize, bytes: usize) -> std::result::Result<(), ControlInputError> {
        if self.closed {
            return Err(ControlInputError::Closed);
        }
        if self.accepted_items.saturating_add(items) > MAX_CONTROL_INPUT_ITEMS {
            return Err(ControlInputError::CountLimit {
                limit: MAX_CONTROL_INPUT_ITEMS,
            });
        }
        if self.accepted_bytes.saturating_add(bytes) > MAX_CONTROL_INPUT_BYTES {
            return Err(ControlInputError::ByteLimit {
                limit: MAX_CONTROL_INPUT_BYTES,
            });
        }
        Ok(())
    }

    fn accepted(&mut self, items: usize, bytes: usize) {
        self.accepted_items += items;
        self.accepted_bytes += bytes;
    }

    fn released(&mut self, items: usize, bytes: usize) {
        self.accepted_items = self.accepted_items.saturating_sub(items);
        self.accepted_bytes = self.accepted_bytes.saturating_sub(bytes);
    }
}

#[derive(Clone)]
pub struct ControlHandle {
    pub(crate) stop_tx: watch::Sender<bool>,
    pub(crate) abort_tx: watch::Sender<bool>,
    input: Arc<Mutex<ControlInputBuffer>>,
}

pub struct ControlReceivers {
    pub(crate) stop_rx: watch::Receiver<bool>,
    pub(crate) abort_rx: watch::Receiver<bool>,
    input: Arc<Mutex<ControlInputBuffer>>,
}

impl ControlHandle {
    pub fn new() -> (Self, ControlReceivers) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        let input = Arc::new(Mutex::new(ControlInputBuffer::default()));
        (
            Self {
                stop_tx,
                abort_tx,
                input: Arc::clone(&input),
            },
            ControlReceivers {
                stop_rx,
                abort_rx,
                input,
            },
        )
    }

    pub fn stop(&self) {
        self.stop_tx.send_replace(true);
    }

    pub fn abort(&self) {
        self.abort_tx.send_replace(true);
    }

    pub fn is_aborted(&self) -> bool {
        *self.abort_tx.borrow()
    }

    pub fn is_stopped(&self) -> bool {
        *self.stop_tx.borrow()
    }

    pub fn inject_user_message(
        &self,
        message: Message,
    ) -> std::result::Result<(), ControlInputError> {
        self.inject_messages(vec![message])
    }

    pub fn inject_messages(
        &self,
        messages: Vec<Message>,
    ) -> std::result::Result<(), ControlInputError> {
        for message in &messages {
            ensure_user_message(message)?;
        }
        self.inject_authoritative_messages(messages)
    }

    /// Injects a Framework-owned batch whose message roles are already
    /// authoritative. Product callers should use the user-only entrypoints.
    #[doc(hidden)]
    pub fn inject_authoritative_messages(
        &self,
        messages: Vec<Message>,
    ) -> std::result::Result<(), ControlInputError> {
        if messages.is_empty() {
            return Ok(());
        }
        let buffered = messages
            .into_iter()
            .map(|message| BufferedInput {
                bytes: message_payload_bytes(&message),
                message,
            })
            .collect::<Vec<_>>();
        let bytes = buffered
            .iter()
            .fold(0usize, |total, item| total.saturating_add(item.bytes));
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        input.admit(buffered.len(), bytes)?;
        let items = buffered.len();
        input.injected.extend(buffered);
        input.accepted(items, bytes);
        Ok(())
    }

    pub fn steer_user_message(
        &self,
        message: Message,
    ) -> std::result::Result<PendingInputId, ControlInputError> {
        let bytes = steer_message_payload_bytes(&message)?;
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        input.admit(1, bytes)?;
        input.next_id = input.next_id.saturating_add(1);
        let id = PendingInputId(input.next_id);
        input
            .steered
            .push_back(PendingUserInput { id, bytes, message });
        input.accepted(1, bytes);
        Ok(id)
    }

    pub fn update_pending_user_message(
        &self,
        id: PendingInputId,
        message: Message,
    ) -> std::result::Result<(), ControlInputError> {
        let bytes = user_message_payload_bytes(&message)?;
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if input.closed {
            return Err(ControlInputError::Closed);
        }
        let Some(index) = input.steered.iter().position(|item| item.id == id) else {
            return Err(ControlInputError::UnknownInput { id: id.as_u64() });
        };
        let previous_bytes = input.steered[index].bytes;
        let updated_bytes = input
            .accepted_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(bytes);
        if updated_bytes > MAX_CONTROL_INPUT_BYTES {
            return Err(ControlInputError::ByteLimit {
                limit: MAX_CONTROL_INPUT_BYTES,
            });
        };
        input.steered[index].message = message;
        input.steered[index].bytes = bytes;
        input.accepted_bytes = updated_bytes;
        Ok(())
    }

    pub fn cancel_pending_user_message(&self, id: PendingInputId) -> bool {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(index) = input.steered.iter().position(|input| input.id == id) else {
            return false;
        };
        let removed = input.steered.remove(index).expect("pending input index");
        input.released(1, removed.bytes);
        true
    }
}

impl ControlReceivers {
    pub(crate) fn stop_requested(&self) -> bool {
        *self.stop_rx.borrow()
    }

    pub(crate) fn abort_requested(&self) -> bool {
        *self.abort_rx.borrow()
    }

    pub fn abort_signal(&self) -> AbortSignal {
        AbortSignal::new(self.abort_rx.clone())
    }

    pub(crate) fn drain_injected_messages(&mut self) -> Vec<Message> {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let drained = input.injected.drain(..).collect::<Vec<_>>();
        let bytes = drained.iter().map(|item| item.bytes).sum();
        input.released(drained.len(), bytes);
        drained.into_iter().map(|item| item.message).collect()
    }

    /// Atomically consumes pending steer inputs. A consumer owns an input once
    /// it is returned, so later update or cancel calls for that id fail.
    pub fn drain_pending_user_messages(&mut self) -> Vec<(PendingInputId, Message)> {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let drained = input.steered.drain(..).collect::<Vec<_>>();
        let bytes = drained.iter().map(|item| item.bytes).sum();
        input.released(drained.len(), bytes);
        drained
            .into_iter()
            .map(|item| (item.id, item.message))
            .collect()
    }
}

impl Drop for ControlReceivers {
    fn drop(&mut self) {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        input.closed = true;
        input.injected.clear();
        input.steered.clear();
        input.accepted_items = 0;
        input.accepted_bytes = 0;
    }
}

fn ensure_user_message(message: &Message) -> std::result::Result<(), ControlInputError> {
    if !matches!(message, Message::User { .. }) {
        return Err(ControlInputError::NotUserMessage);
    }
    Ok(())
}

fn user_message_payload_bytes(message: &Message) -> std::result::Result<usize, ControlInputError> {
    ensure_user_message(message)?;
    Ok(message_payload_bytes(message))
}

pub fn validate_steer_message(message: &Message) -> std::result::Result<(), ControlInputError> {
    steer_message_payload_bytes(message).map(|_| ())
}

fn steer_message_payload_bytes(message: &Message) -> std::result::Result<usize, ControlInputError> {
    let bytes = user_message_payload_bytes(message)?;
    if bytes > MAX_CONTROL_INPUT_BYTES {
        return Err(ControlInputError::ByteLimit {
            limit: MAX_CONTROL_INPUT_BYTES,
        });
    }
    Ok(bytes)
}

fn message_payload_bytes(message: &Message) -> usize {
    serde_json::to_vec(message).map_or(usize::MAX, |encoded| encoded.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_input_capacity_rejects_newest_and_recovers_after_drain() {
        let (control, mut receivers) = ControlHandle::new();
        for index in 0..MAX_CONTROL_INPUT_ITEMS {
            control
                .inject_user_message(user_text_message(format!("message-{index}")))
                .expect("input below capacity");
        }

        assert_eq!(
            control.inject_user_message(user_text_message("overflow")),
            Err(ControlInputError::CountLimit {
                limit: MAX_CONTROL_INPUT_ITEMS,
            })
        );
        assert_eq!(
            receivers.drain_injected_messages().len(),
            MAX_CONTROL_INPUT_ITEMS
        );
        control
            .inject_user_message(user_text_message("accepted after drain"))
            .expect("drain reclaims capacity");
    }

    #[test]
    fn injected_batch_is_admitted_atomically() {
        let (control, mut receivers) = ControlHandle::new();
        for index in 0..(MAX_CONTROL_INPUT_ITEMS - 1) {
            control
                .inject_user_message(user_text_message(format!("message-{index}")))
                .expect("input below capacity");
        }

        assert_eq!(
            control.inject_messages(vec![
                user_text_message("batch-a"),
                user_text_message("batch-b"),
            ]),
            Err(ControlInputError::CountLimit {
                limit: MAX_CONTROL_INPUT_ITEMS,
            })
        );
        let drained = receivers.drain_injected_messages();
        assert_eq!(drained.len(), MAX_CONTROL_INPUT_ITEMS - 1);
        assert!(drained.iter().all(|message| {
            !matches!(
                message,
                Message::User { content, .. }
                    if matches!(
                        content.as_slice(),
                        [UserContentBlock::Text(text)]
                            if text.text == "batch-a" || text.text == "batch-b"
                    )
            )
        }));
    }

    #[test]
    fn injected_batch_rejects_non_user_messages_without_accepting_a_prefix() {
        let (control, mut receivers) = ControlHandle::new();

        assert_eq!(
            control.inject_messages(vec![
                user_text_message("valid prefix"),
                Message::ToolResult {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "test".to_string(),
                    content: "not user input".to_string(),
                    is_error: false,
                    timestamp_ms: 1,
                },
            ]),
            Err(ControlInputError::NotUserMessage)
        );
        assert!(receivers.drain_injected_messages().is_empty());
    }

    #[test]
    fn authoritative_batch_accepts_framework_owned_assistant_messages() {
        let (control, mut receivers) = ControlHandle::new();
        let message = Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "mailbox notification".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: None,
            outcome: Outcome::Normal,
            model: None,
            provider: None,
        };

        control
            .inject_authoritative_messages(vec![message.clone()])
            .expect("authoritative mailbox batch");

        assert_eq!(receivers.drain_injected_messages(), vec![message]);
    }

    #[test]
    fn pending_update_accounts_bytes_without_evicting_accepted_input() {
        let (control, mut receivers) = ControlHandle::new();
        let id = control
            .steer_user_message(user_text_message("small"))
            .expect("steer accepted");
        assert_eq!(
            control.update_pending_user_message(
                id,
                user_text_message("x".repeat(MAX_CONTROL_INPUT_BYTES + 1)),
            ),
            Err(ControlInputError::ByteLimit {
                limit: MAX_CONTROL_INPUT_BYTES,
            })
        );

        let drained = receivers.drain_pending_user_messages();
        assert_eq!(drained.len(), 1);
        let Message::User { content, .. } = &drained[0].1 else {
            panic!("steer must remain a user message");
        };
        assert!(matches!(
            content.as_slice(),
            [UserContentBlock::Text(text)] if text.text == "small"
        ));
    }

    #[test]
    fn dropping_receivers_closes_control_input() {
        let (control, receivers) = ControlHandle::new();
        drop(receivers);

        assert_eq!(
            control.inject_user_message(user_text_message("late")),
            Err(ControlInputError::Closed)
        );
        assert_eq!(
            control.steer_user_message(user_text_message("late")),
            Err(ControlInputError::Closed)
        );
    }
}
