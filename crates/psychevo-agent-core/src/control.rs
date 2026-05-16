#[derive(Clone)]
pub struct ControlHandle {
    stop_tx: watch::Sender<bool>,
    abort_tx: watch::Sender<bool>,
    injection_tx: mpsc::UnboundedSender<Message>,
}

pub struct ControlReceivers {
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
    injection_rx: mpsc::UnboundedReceiver<Message>,
}

impl ControlHandle {
    pub fn new() -> (Self, ControlReceivers) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        let (injection_tx, injection_rx) = mpsc::unbounded_channel();
        (
            Self {
                stop_tx,
                abort_tx,
                injection_tx,
            },
            ControlReceivers {
                stop_rx,
                abort_rx,
                injection_rx,
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
}

impl ControlReceivers {
    fn stop_requested(&self) -> bool {
        *self.stop_rx.borrow()
    }

    fn abort_requested(&self) -> bool {
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
}
