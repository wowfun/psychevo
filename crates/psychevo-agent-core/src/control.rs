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
pub(crate) struct PendingUserInput {
    pub(crate) id: PendingInputId,
    pub(crate) message: Message,
}

#[derive(Debug, Default)]
pub(crate) struct PendingUserInputs {
    pub(crate) next_id: u64,
    pub(crate) inputs: VecDeque<PendingUserInput>,
}

#[derive(Clone)]
pub struct ControlHandle {
    pub(crate) stop_tx: watch::Sender<bool>,
    pub(crate) abort_tx: watch::Sender<bool>,
    pub(crate) injection_tx: mpsc::UnboundedSender<Message>,
    pub(crate) pending_user_inputs: Arc<Mutex<PendingUserInputs>>,
}

pub struct ControlReceivers {
    pub(crate) stop_rx: watch::Receiver<bool>,
    pub(crate) abort_rx: watch::Receiver<bool>,
    pub(crate) injection_rx: mpsc::UnboundedReceiver<Message>,
    pub(crate) pending_user_inputs: Arc<Mutex<PendingUserInputs>>,
}

impl ControlHandle {
    pub fn new() -> (Self, ControlReceivers) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        let (injection_tx, injection_rx) = mpsc::unbounded_channel();
        let pending_user_inputs = Arc::new(Mutex::new(PendingUserInputs::default()));
        (
            Self {
                stop_tx,
                abort_tx,
                injection_tx,
                pending_user_inputs: Arc::clone(&pending_user_inputs),
            },
            ControlReceivers {
                stop_rx,
                abort_rx,
                injection_rx,
                pending_user_inputs,
            },
        )
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }

    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
    }

    pub fn inject_user_message(&self, message: Message) -> bool {
        self.injection_tx.send(message).is_ok()
    }

    pub fn steer_user_message(&self, message: Message) -> Option<PendingInputId> {
        if !matches!(message, Message::User { .. }) {
            return None;
        }
        let mut state = self.pending_user_inputs.lock().ok()?;
        state.next_id = state.next_id.saturating_add(1);
        let id = PendingInputId(state.next_id);
        state.inputs.push_back(PendingUserInput { id, message });
        Some(id)
    }

    pub fn update_pending_user_message(&self, id: PendingInputId, message: Message) -> bool {
        if !matches!(message, Message::User { .. }) {
            return false;
        }
        let Ok(mut state) = self.pending_user_inputs.lock() else {
            return false;
        };
        let Some(input) = state.inputs.iter_mut().find(|input| input.id == id) else {
            return false;
        };
        input.message = message;
        true
    }

    pub fn cancel_pending_user_message(&self, id: PendingInputId) -> bool {
        let Ok(mut state) = self.pending_user_inputs.lock() else {
            return false;
        };
        let Some(index) = state.inputs.iter().position(|input| input.id == id) else {
            return false;
        };
        state.inputs.remove(index);
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
        let mut messages = Vec::new();
        while let Ok(message) = self.injection_rx.try_recv() {
            messages.push(message);
        }
        messages
    }

    pub(crate) fn drain_pending_user_messages(&mut self) -> Vec<(PendingInputId, Message)> {
        let Ok(mut state) = self.pending_user_inputs.lock() else {
            return Vec::new();
        };
        state
            .inputs
            .drain(..)
            .map(|input| (input.id, input.message))
            .collect()
    }
}
