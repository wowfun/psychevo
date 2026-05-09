#[derive(Clone)]
pub struct AbortSignal {
    rx: watch::Receiver<bool>,
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

pub trait GenerationProvider: Send + Sync {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>>;
}
