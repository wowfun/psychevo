use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, mpsc, oneshot};

use super::{
    DurableGatewayActivity, Gateway, GatewayEvent, GatewayEventEmitError, GatewaySupervisor,
};

const GATEWAY_EVENT_INGRESS_CAPACITY: usize = 512;

#[derive(Clone)]
pub(crate) struct GatewayEventIngress {
    inner: Arc<GatewayEventIngressInner>,
}

struct GatewayEventIngressInner {
    accepted: AtomicU64,
    closed: AtomicBool,
    completed: AtomicU64,
    completion: Notify,
    rejected: AtomicU64,
    sender: Mutex<Option<mpsc::Sender<GatewayEventEnvelope>>>,
    supervisor: GatewaySupervisor,
}

pub(crate) struct GatewayEventEnvelope {
    pub(crate) activity: DurableGatewayActivity,
    pub(crate) completion: Option<oneshot::Sender<()>>,
    pub(crate) default_turn_id: Option<String>,
    pub(crate) event: GatewayEvent,
    pub(crate) queue_key: Option<String>,
    pub(crate) root_activity_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GatewayEventIngressDiagnostics {
    pub(crate) accepted: u64,
    pub(crate) completed: u64,
    pub(crate) rejected: u64,
}

impl GatewayEventIngress {
    pub(crate) fn new(supervisor: GatewaySupervisor) -> Self {
        Self {
            inner: Arc::new(GatewayEventIngressInner {
                accepted: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                completed: AtomicU64::new(0),
                completion: Notify::new(),
                rejected: AtomicU64::new(0),
                sender: Mutex::new(None),
                supervisor,
            }),
        }
    }

    pub(crate) fn submit(
        &self,
        gateway: Gateway,
        envelope: GatewayEventEnvelope,
    ) -> Result<(), GatewayEventEmitError> {
        let sender = match self.sender(gateway) {
            Ok(sender) => sender,
            Err(error) => {
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        match sender.try_send(envelope) {
            Ok(()) => {
                self.inner.accepted.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                Err(GatewayEventEmitError::new(
                    "Gateway event durability ingress is full; local delivery succeeded but durable relay admission failed.",
                ))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                Err(GatewayEventEmitError::new(
                    "Gateway event durability ingress is closed.",
                ))
            }
        }
    }

    pub(crate) async fn submit_wait(
        &self,
        gateway: Gateway,
        mut envelope: GatewayEventEnvelope,
    ) -> Result<(), GatewayEventEmitError> {
        let sender = match self.sender(gateway) {
            Ok(sender) => sender,
            Err(error) => {
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        if self.inner.closed.load(Ordering::Acquire) {
            self.inner.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        let (completion_tx, completion_rx) = oneshot::channel();
        envelope.completion = Some(completion_tx);
        if sender.send(envelope).await.is_err() {
            self.inner.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        self.inner.accepted.fetch_add(1, Ordering::Relaxed);
        completion_rx.await.map_err(|_| {
            GatewayEventEmitError::new(
                "Gateway event durability ingress closed before the event was processed.",
            )
        })
    }

    pub(crate) fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.inner
            .sender
            .lock()
            .expect("gateway event ingress sender poisoned")
            .take();
        self.inner.completion.notify_waiters();
    }

    pub(crate) fn diagnostics(&self) -> GatewayEventIngressDiagnostics {
        GatewayEventIngressDiagnostics {
            accepted: self.inner.accepted.load(Ordering::Relaxed),
            completed: self.inner.completed.load(Ordering::Relaxed),
            rejected: self.inner.rejected.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_until_drained(&self) {
        let target = self.inner.accepted.load(Ordering::Acquire);
        loop {
            let completed = self.inner.completed.load(Ordering::Acquire);
            if completed >= target {
                return;
            }
            let notified = self.inner.completion.notified();
            if self.inner.completed.load(Ordering::Acquire) >= target {
                return;
            }
            notified.await;
        }
    }

    fn sender(
        &self,
        gateway: Gateway,
    ) -> Result<mpsc::Sender<GatewayEventEnvelope>, GatewayEventEmitError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        let mut slot = self
            .inner
            .sender
            .lock()
            .expect("gateway event ingress sender poisoned");
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        if let Some(sender) = slot.as_ref() {
            return Ok(sender.clone());
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            GatewayEventEmitError::new(
                "Gateway event durability ingress requires an active Tokio runtime.",
            )
        })?;
        let (sender, mut receiver) =
            mpsc::channel::<GatewayEventEnvelope>(GATEWAY_EVENT_INGRESS_CAPACITY);
        let ingress = self.clone();
        self.inner
            .supervisor
            .spawn_infrastructure("gateway-event-ingress", async move {
                while let Some(envelope) = receiver.recv().await {
                    let completion = envelope.completion;
                    let envelope = GatewayEventEnvelope {
                        completion: None,
                        ..envelope
                    };
                    gateway.persist_gateway_event_envelope(envelope).await;
                    ingress.inner.completed.fetch_add(1, Ordering::Release);
                    ingress.inner.completion.notify_waiters();
                    if let Some(completion) = completion {
                        let _ = completion.send(());
                    }
                }
            });
        *slot = Some(sender.clone());
        Ok(sender)
    }
}

impl fmt::Debug for GatewayEventIngress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayEventIngress")
            .field("diagnostics", &self.diagnostics())
            .finish()
    }
}
