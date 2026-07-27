use std::collections::VecDeque;
use std::sync::Mutex;

use tokio::sync::Notify;

use super::TurnEvent;

pub(super) struct EventLog {
    inner: Mutex<EventLogState>,
    notify: Notify,
    capacity: usize,
}

struct EventLogState {
    first_sequence: u64,
    next_sequence: u64,
    events: VecDeque<TurnEvent>,
    closed: bool,
}

impl EventLog {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(EventLogState {
                first_sequence: 0,
                next_sequence: 0,
                events: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            notify: Notify::new(),
            capacity,
        }
    }

    pub(super) fn push(&self, event: TurnEvent) {
        let mut state = self.inner.lock().expect("turn event log poisoned");
        if state.events.len() == self.capacity {
            state.events.pop_front();
            state.first_sequence += 1;
        }
        state.events.push_back(event);
        state.next_sequence += 1;
        drop(state);
        self.notify.notify_waiters();
    }

    pub(super) fn close(&self) {
        self.inner.lock().expect("turn event log poisoned").closed = true;
        self.notify.notify_waiters();
    }

    pub(super) async fn next(&self, cursor: &mut u64) -> Option<TurnEvent> {
        loop {
            let notified = self.notify.notified();
            {
                let state = self.inner.lock().expect("turn event log poisoned");
                if *cursor < state.first_sequence {
                    let missed = state.first_sequence - *cursor;
                    *cursor = state.first_sequence;
                    return Some(TurnEvent::ResyncRequired { missed });
                }
                if *cursor < state.next_sequence {
                    let offset = (*cursor - state.first_sequence) as usize;
                    let event = state.events.get(offset).cloned();
                    *cursor += 1;
                    if event.is_some() {
                        return event;
                    }
                }
                if state.closed {
                    return None;
                }
            }
            notified.await;
        }
    }
}
