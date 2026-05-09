#[derive(Clone)]
pub struct ControlHandle {
    stop_tx: watch::Sender<bool>,
    abort_tx: watch::Sender<bool>,
}

pub struct ControlReceivers {
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
}

impl ControlHandle {
    pub fn new() -> (Self, ControlReceivers) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        (
            Self { stop_tx, abort_tx },
            ControlReceivers { stop_rx, abort_rx },
        )
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }

    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
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
}

