use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, broadcast, mpsc};

use crate::GatewayEvent;

use super::rpc_notification;

pub(super) const EVENT_HUB_CAPACITY: usize = 512;
pub(super) const CONNECTION_OUTBOX_FRAMES: usize = 128;
pub(super) const CONNECTION_OUTBOX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryUpdateKey {
    thread_id: String,
    turn_id: String,
    entry_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct GatewayBroadcastFrame {
    text: Arc<str>,
    replace_key: Option<EntryUpdateKey>,
}

impl GatewayBroadcastFrame {
    fn from_event(event: &GatewayEvent) -> serde_json::Result<Self> {
        let replace_key = match event {
            GatewayEvent::EntryUpdated { turn_id, entry } => Some(EntryUpdateKey {
                thread_id: entry.thread_id.clone(),
                turn_id: turn_id.clone(),
                entry_id: entry.id.clone(),
            }),
            _ => None,
        };
        Ok(Self {
            text: rpc_notification("gateway/event", serde_json::to_value(event)?).into(),
            replace_key,
        })
    }
}

#[derive(Clone)]
pub(super) struct GatewayEventHub {
    sender: broadcast::Sender<GatewayBroadcastFrame>,
}

impl Default for GatewayEventHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(EVENT_HUB_CAPACITY);
        Self { sender }
    }
}

impl GatewayEventHub {
    pub(super) fn subscribe(&self) -> broadcast::Receiver<GatewayBroadcastFrame> {
        self.sender.subscribe()
    }

    pub(super) fn publish(&self, event: &GatewayEvent) {
        let Ok(frame) = GatewayBroadcastFrame::from_event(event) else {
            return;
        };
        let _ = self.sender.send(frame);
    }
}

#[derive(Debug)]
struct QueuedFrame {
    text: Arc<str>,
    replace_key: Option<EntryUpdateKey>,
}

#[derive(Debug, Default)]
struct OutboxState {
    queue: VecDeque<QueuedFrame>,
    queued_bytes: usize,
    close_reason: Option<String>,
}

#[derive(Debug)]
pub(super) struct ConnectionOutboxInner {
    state: Mutex<OutboxState>,
    ready: Notify,
    closed: Notify,
}

impl ConnectionOutboxInner {
    fn close(&self, reason: impl Into<String>) {
        let mut state = self.state.lock().expect("connection outbox poisoned");
        if state.close_reason.is_some() {
            return;
        }
        state.close_reason = Some(reason.into());
        state.queue.clear();
        state.queued_bytes = 0;
        drop(state);
        self.ready.notify_waiters();
        self.closed.notify_waiters();
    }
}

#[derive(Debug, Clone)]
pub(super) struct ConnectionSendError;

#[derive(Clone)]
pub(super) enum ConnectionSender {
    Bounded(Arc<ConnectionOutboxInner>),
    InternalUnbounded(mpsc::UnboundedSender<String>),
}

impl ConnectionSender {
    pub(super) fn is_internal_adapter(&self) -> bool {
        matches!(self, Self::InternalUnbounded(_))
    }

    pub(super) fn send(&self, text: String) -> Result<(), ConnectionSendError> {
        self.enqueue(text.into(), None)
    }

    pub(super) fn send_gateway_event(
        &self,
        frame: GatewayBroadcastFrame,
    ) -> Result<(), ConnectionSendError> {
        self.enqueue(frame.text, frame.replace_key)
    }

    fn enqueue(
        &self,
        text: Arc<str>,
        replace_key: Option<EntryUpdateKey>,
    ) -> Result<(), ConnectionSendError> {
        match self {
            Self::Bounded(inner) => {
                let text_len = text.len();
                let mut state = inner.state.lock().expect("connection outbox poisoned");
                if state.close_reason.is_some() {
                    return Err(ConnectionSendError);
                }
                if let Some(key) = replace_key.as_ref()
                    && let Some(index) = state
                        .queue
                        .iter()
                        .position(|queued| queued.replace_key.as_ref() == Some(key))
                {
                    let replaced_len = state.queue[index].text.len();
                    let next_bytes = state
                        .queued_bytes
                        .saturating_sub(replaced_len)
                        .saturating_add(text_len);
                    if next_bytes > CONNECTION_OUTBOX_BYTES {
                        drop(state);
                        inner.close("connection outbox byte budget exceeded");
                        return Err(ConnectionSendError);
                    }
                    state.queue[index].text = text;
                    state.queued_bytes = next_bytes;
                    return Ok(());
                }

                let oversized_required = replace_key.is_none()
                    && state.queue.is_empty()
                    && text_len > CONNECTION_OUTBOX_BYTES;
                if state.queue.len() >= CONNECTION_OUTBOX_FRAMES
                    || (!oversized_required
                        && state.queued_bytes.saturating_add(text_len) > CONNECTION_OUTBOX_BYTES)
                {
                    drop(state);
                    inner.close("connection outbox capacity exceeded");
                    return Err(ConnectionSendError);
                }
                state.queued_bytes = state.queued_bytes.saturating_add(text_len);
                state.queue.push_back(QueuedFrame { text, replace_key });
                drop(state);
                inner.ready.notify_one();
                Ok(())
            }
            Self::InternalUnbounded(sender) => sender
                .send(text.to_string())
                .map_err(|_| ConnectionSendError),
        }
    }

    pub(super) fn close(&self, reason: impl Into<String>) {
        if let Self::Bounded(inner) = self {
            inner.close(reason);
        }
    }

    pub(super) async fn closed(&self) {
        let Self::Bounded(inner) = self else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            let notified = inner.closed.notified();
            if inner
                .state
                .lock()
                .expect("connection outbox poisoned")
                .close_reason
                .is_some()
            {
                return;
            }
            notified.await;
        }
    }
}

impl From<mpsc::UnboundedSender<String>> for ConnectionSender {
    fn from(value: mpsc::UnboundedSender<String>) -> Self {
        Self::InternalUnbounded(value)
    }
}

pub(super) enum OutboxReceive {
    Frame(Arc<str>),
    Closed(String),
}

pub(super) struct ConnectionOutboxReceiver {
    inner: Arc<ConnectionOutboxInner>,
}

impl ConnectionOutboxReceiver {
    pub(super) async fn recv(&mut self) -> OutboxReceive {
        loop {
            let notified = self.inner.ready.notified();
            {
                let mut state = self.inner.state.lock().expect("connection outbox poisoned");
                if let Some(frame) = state.queue.pop_front() {
                    state.queued_bytes = state.queued_bytes.saturating_sub(frame.text.len());
                    return OutboxReceive::Frame(frame.text);
                }
                if let Some(reason) = state.close_reason.clone() {
                    return OutboxReceive::Closed(reason);
                }
            }
            notified.await;
        }
    }
}

pub(super) fn connection_outbox() -> (ConnectionSender, ConnectionOutboxReceiver) {
    let inner = Arc::new(ConnectionOutboxInner {
        state: Mutex::new(OutboxState::default()),
        ready: Notify::new(),
        closed: Notify::new(),
    });
    (
        ConnectionSender::Bounded(inner.clone()),
        ConnectionOutboxReceiver { inner },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GatewayActivityView, TranscriptBlockStatus, TranscriptEntry, TranscriptEntryRole};

    fn entry_update(body: &str) -> GatewayEvent {
        GatewayEvent::EntryUpdated {
            turn_id: "turn".to_string(),
            entry: TranscriptEntry {
                id: "entry".to_string(),
                thread_id: "thread".to_string(),
                turn_id: Some("turn".to_string()),
                message_seq: None,
                role: TranscriptEntryRole::Assistant,
                status: TranscriptBlockStatus::Running,
                source: body.to_string(),
                blocks: Vec::new(),
                metadata: None,
                usage: None,
                accounting: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
        }
    }

    #[tokio::test]
    async fn entry_updates_replace_in_place_without_reordering_required_frames() {
        let (sender, mut receiver) = connection_outbox();
        sender.send("before".to_string()).expect("required frame");
        sender
            .send_gateway_event(GatewayBroadcastFrame::from_event(&entry_update("first")).unwrap())
            .expect("first update");
        sender
            .send_gateway_event(GatewayBroadcastFrame::from_event(&entry_update("latest")).unwrap())
            .expect("replacement update");
        sender.send("after".to_string()).expect("required frame");

        assert!(
            matches!(receiver.recv().await, OutboxReceive::Frame(text) if text.as_ref() == "before")
        );
        assert!(
            matches!(receiver.recv().await, OutboxReceive::Frame(text) if text.contains("latest"))
        );
        assert!(
            matches!(receiver.recv().await, OutboxReceive::Frame(text) if text.as_ref() == "after")
        );
    }

    #[tokio::test]
    async fn capacity_failure_closes_only_the_slow_connection() {
        let (sender, mut receiver) = connection_outbox();
        for index in 0..CONNECTION_OUTBOX_FRAMES {
            sender
                .send(format!("required-{index}"))
                .expect("within frame budget");
        }
        assert!(sender.send("overflow".to_string()).is_err());
        assert!(
            matches!(receiver.recv().await, OutboxReceive::Closed(reason) if reason.contains("capacity"))
        );
    }

    #[tokio::test]
    async fn hub_fans_out_one_publication_to_each_subscriber() {
        let hub = GatewayEventHub::default();
        let mut first = hub.subscribe();
        let mut second = hub.subscribe();
        hub.publish(&GatewayEvent::ActivityChanged {
            thread_id: Some("thread".to_string()),
            activity: GatewayActivityView::default(),
        });
        let first = first.recv().await.expect("first subscriber");
        let second = second.recv().await.expect("second subscriber");
        assert_eq!(first.text, second.text);
    }
}
