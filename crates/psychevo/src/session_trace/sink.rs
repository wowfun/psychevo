use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use psychevo_agent_core::AgentEvent;
use serde_json::Value;

use crate::types::MessageAccounting;

use super::drafts_compaction::{
    SessionTraceStats, run_start_trace_draft, trace_drafts_from_agent_event,
};
use super::read_write::{append_trace_record, max_valid_seq, session_trace_path, set_last_error};

const TRACE_CHANNEL_CAPACITY: usize = 512;
const TRACE_WRITER_STACK_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub(super) struct SessionTraceDraft {
    pub(super) kind: String,
    pub(super) timestamp_ms: i64,
    pub(super) monotonic_offset_ms: u64,
    pub(super) turn_index: Option<usize>,
    pub(super) correlation: Value,
    pub(super) payload: Value,
}

#[derive(Clone)]
pub(crate) struct SessionTraceSink {
    inner: Arc<SessionTraceSinkInner>,
}

struct SessionTraceSinkInner {
    sender: SyncSender<SessionTraceDraft>,
    last_error: Arc<Mutex<Option<String>>>,
    stats: Arc<Mutex<SessionTraceStats>>,
}

impl SessionTraceSink {
    pub(crate) fn open(
        db_path: &Path,
        session_id: &str,
        invocation_id: String,
    ) -> Result<Option<Self>, String> {
        let Some(path) = session_trace_path(db_path, session_id)? else {
            return Ok(None);
        };
        let (sender, receiver) = sync_channel(TRACE_CHANNEL_CAPACITY);
        let last_error = Arc::new(Mutex::new(None));
        let stats = Arc::new(Mutex::new(SessionTraceStats::default()));
        let writer_error = Arc::clone(&last_error);
        let session_id = session_id.to_string();
        thread::Builder::new()
            .name("psychevo-session-trace".to_string())
            .stack_size(TRACE_WRITER_STACK_BYTES)
            .spawn(move || {
                let mut next_seq = match max_valid_seq(&path) {
                    Ok(seq) => seq.saturating_add(1),
                    Err(err) => {
                        set_last_error(&writer_error, err);
                        1
                    }
                };
                for draft in receiver {
                    match append_trace_record(&path, &session_id, &invocation_id, next_seq, draft) {
                        Ok(()) => next_seq = next_seq.saturating_add(1),
                        Err(err) => set_last_error(&writer_error, err),
                    }
                }
            })
            .map_err(|err| format!("failed to start session trace writer: {err}"))?;
        Ok(Some(Self {
            inner: Arc::new(SessionTraceSinkInner {
                sender,
                last_error,
                stats,
            }),
        }))
    }

    fn enqueue(&self, draft: SessionTraceDraft) -> Option<String> {
        let kind = draft.kind.clone();
        match self.inner.sender.try_send(draft) {
            Ok(()) => None,
            Err(TrySendError::Full(_draft)) => {
                let message =
                    "session trace queue is full; dropping observability event".to_string();
                self.record_dropped("queue_full", &kind);
                set_last_error(&self.inner.last_error, message.clone());
                Some(message)
            }
            Err(TrySendError::Disconnected(_draft)) => {
                let message =
                    "session trace writer is unavailable; dropping observability event".to_string();
                self.record_dropped("writer_disconnected", &kind);
                set_last_error(&self.inner.last_error, message.clone());
                Some(message)
            }
        }
    }

    pub(crate) fn enqueue_run_start(&self, payload: &Value) -> Option<String> {
        self.enqueue(run_start_trace_draft(payload))
    }

    pub(crate) fn observe_agent_event(
        &self,
        event: &AgentEvent,
        accounting: Option<&MessageAccounting>,
        monotonic_offset_ms: u64,
        turn_index: Option<usize>,
    ) -> Option<String> {
        let drafts = match self.inner.stats.lock() {
            Ok(mut stats) => trace_drafts_from_agent_event(
                event,
                accounting,
                monotonic_offset_ms,
                turn_index,
                &mut stats,
            ),
            Err(_) => {
                let message = "session trace stats are unavailable".to_string();
                set_last_error(&self.inner.last_error, message.clone());
                return Some(message);
            }
        };
        let mut warning = None;
        for draft in drafts {
            if let Some(message) = self.enqueue(draft)
                && warning.is_none()
            {
                warning = Some(message);
            }
        }
        warning
    }

    pub(crate) fn take_error(&self) -> Option<String> {
        self.inner.last_error.lock().ok()?.take()
    }

    fn record_dropped(&self, reason: &'static str, kind: &str) {
        if let Ok(mut stats) = self.inner.stats.lock() {
            stats.drop_kind(reason, kind);
        }
    }
}
