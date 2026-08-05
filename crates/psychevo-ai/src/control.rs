#[cfg(feature = "openai")]
use futures::future::BoxFuture;
use tokio::sync::watch;

#[cfg(feature = "openai")]
use crate::types::GenerationRequest;
#[cfg(feature = "openai")]
use crate::{GenerationStream, Result};

#[derive(Clone, Debug)]
pub struct AbortSignal {
    pub(crate) rx: watch::Receiver<bool>,
}

#[derive(Clone, Debug)]
pub struct AbortHandle {
    tx: watch::Sender<bool>,
}

pub(crate) fn abort_pair() -> (AbortHandle, AbortSignal) {
    let (tx, rx) = watch::channel(false);
    (AbortHandle { tx }, AbortSignal { rx })
}

impl AbortHandle {
    pub fn abort(&self) -> bool {
        self.tx.send(true).is_ok()
    }

    pub fn is_aborted(&self) -> bool {
        *self.tx.borrow()
    }
}

impl AbortSignal {
    pub fn new(rx: watch::Receiver<bool>) -> Self {
        Self { rx }
    }

    pub fn aborted(&self) -> bool {
        *self.rx.borrow()
    }

    pub async fn wait_for_abort(&mut self) {
        if self.aborted() {
            return;
        }
        while self.rx.changed().await.is_ok() {
            if self.aborted() {
                return;
            }
        }
        std::future::pending::<()>().await;
    }
}

#[cfg(feature = "openai")]
pub(crate) trait GenerationProvider: Send + Sync {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>>;
}
