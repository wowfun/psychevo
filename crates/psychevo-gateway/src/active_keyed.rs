use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Default)]
pub(crate) struct ActiveKeyedMutex {
    registry: Arc<Mutex<HashMap<String, Arc<MutexEntry>>>>,
}

struct MutexEntry {
    mutex: Arc<AsyncMutex<()>>,
    reservations: AtomicUsize,
}

pub(crate) struct ActiveKeyedMutexGuard {
    _guard: OwnedMutexGuard<()>,
    _reservation: ActiveKeyedMutexReservation,
}

pub(crate) struct ActiveKeyedMutexReservation {
    registry: Arc<Mutex<HashMap<String, Arc<MutexEntry>>>>,
    key: String,
    entry: Arc<MutexEntry>,
}

impl ActiveKeyedMutex {
    pub(crate) async fn lock(&self, key: &str) -> ActiveKeyedMutexGuard {
        self.reserve(key).lock().await
    }

    pub(crate) fn reserve(&self, key: &str) -> ActiveKeyedMutexReservation {
        let key = key.to_string();
        let mut registry = recover_lock(&self.registry);
        let entry = Arc::clone(registry.entry(key.clone()).or_insert_with(|| {
            Arc::new(MutexEntry {
                mutex: Arc::new(AsyncMutex::new(())),
                reservations: AtomicUsize::new(0),
            })
        }));
        entry.reservations.fetch_add(1, Ordering::Relaxed);
        drop(registry);
        ActiveKeyedMutexReservation {
            registry: Arc::clone(&self.registry),
            key,
            entry,
        }
    }

    #[cfg(test)]
    pub(crate) fn active_keys(&self) -> usize {
        recover_lock(&self.registry).len()
    }
}

impl ActiveKeyedMutexReservation {
    pub(crate) async fn lock(self) -> ActiveKeyedMutexGuard {
        let guard = Arc::clone(&self.entry.mutex).lock_owned().await;
        ActiveKeyedMutexGuard {
            _guard: guard,
            _reservation: self,
        }
    }
}

impl Drop for ActiveKeyedMutexReservation {
    fn drop(&mut self) {
        if self.entry.reservations.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut registry = recover_lock(&self.registry);
        if self.entry.reservations.load(Ordering::Acquire) == 0
            && registry
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(entry, &self.entry))
        {
            registry.remove(&self.key);
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct SourceEpochs {
    registry: Arc<Mutex<SourceEpochRegistry>>,
}

#[derive(Default)]
struct SourceEpochRegistry {
    next_epoch: u64,
    entries: HashMap<String, Arc<SourceEpochEntry>>,
}

struct SourceEpochEntry {
    epoch: AtomicU64,
    leases: AtomicUsize,
}

pub(crate) struct SourceEpochLease {
    registry: Arc<Mutex<SourceEpochRegistry>>,
    key: String,
    entry: Arc<SourceEpochEntry>,
    captured_epoch: u64,
}

impl SourceEpochs {
    pub(crate) fn capture(&self, key: &str) -> SourceEpochLease {
        let key = key.to_string();
        let mut registry = recover_lock(&self.registry);
        let entry = if let Some(entry) = registry.entries.get(&key) {
            Arc::clone(entry)
        } else {
            let epoch = allocate_epoch(&mut registry);
            let entry = Arc::new(SourceEpochEntry {
                epoch: AtomicU64::new(epoch),
                leases: AtomicUsize::new(0),
            });
            registry.entries.insert(key.clone(), Arc::clone(&entry));
            entry
        };
        entry.leases.fetch_add(1, Ordering::Relaxed);
        let captured_epoch = entry.epoch.load(Ordering::Relaxed);
        drop(registry);
        SourceEpochLease {
            registry: Arc::clone(&self.registry),
            key,
            entry,
            captured_epoch,
        }
    }

    pub(crate) fn invalidate(&self, key: &str) {
        let mut registry = recover_lock(&self.registry);
        let Some(entry) = registry.entries.get(key).cloned() else {
            return;
        };
        let epoch = allocate_epoch(&mut registry);
        entry.epoch.store(epoch, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn active_keys(&self) -> usize {
        recover_lock(&self.registry).entries.len()
    }
}

impl SourceEpochLease {
    pub(crate) fn is_current(&self) -> bool {
        self.entry.epoch.load(Ordering::Acquire) == self.captured_epoch
    }
}

impl Drop for SourceEpochLease {
    fn drop(&mut self) {
        if self.entry.leases.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut registry = recover_lock(&self.registry);
        if self.entry.leases.load(Ordering::Acquire) == 0
            && registry
                .entries
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(entry, &self.entry))
        {
            registry.entries.remove(&self.key);
        }
    }
}

fn allocate_epoch(registry: &mut SourceEpochRegistry) -> u64 {
    registry.next_epoch = registry
        .next_epoch
        .checked_add(1)
        .expect("source epoch space exhausted");
    registry.next_epoch
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_waiter_reserves_the_existing_mutex_until_it_finishes() {
        let lanes = ActiveKeyedMutex::default();
        let first = lanes.lock("same").await;
        let waiting_lanes = lanes.clone();
        let waiting = tokio::spawn(async move { waiting_lanes.lock("same").await });
        tokio::task::yield_now().await;
        assert_eq!(lanes.active_keys(), 1);

        drop(first);
        let second = waiting.await.expect("waiter task");
        let later_lanes = lanes.clone();
        let later = tokio::spawn(async move { later_lanes.lock("same").await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), later)
                .await
                .is_err(),
            "a later command must remain behind the queued command"
        );
        drop(second);
    }

    #[tokio::test]
    async fn cancelled_waiters_and_completed_keys_are_reclaimed() {
        let lanes = ActiveKeyedMutex::default();
        let held = lanes.lock("held").await;
        let waiting_lanes = lanes.clone();
        let waiting = tokio::spawn(async move { waiting_lanes.lock("held").await });
        tokio::task::yield_now().await;
        waiting.abort();
        let _ = waiting.await;
        drop(held);

        for index in 0..100_000 {
            drop(lanes.lock(&format!("transient-{index}")).await);
        }
        assert_eq!(lanes.active_keys(), 0);
    }

    #[tokio::test]
    async fn unrelated_keys_do_not_serialize() {
        let lanes = ActiveKeyedMutex::default();
        let _first = lanes.lock("first").await;
        tokio::time::timeout(Duration::from_millis(20), lanes.lock("second"))
            .await
            .expect("unrelated key should not wait");
    }

    #[test]
    fn source_epoch_invalidation_is_scoped_and_reclaimed() {
        let epochs = SourceEpochs::default();
        let first = epochs.capture("first");
        let second = epochs.capture("second");
        epochs.invalidate("first");
        epochs.invalidate("idle");
        assert!(!first.is_current());
        assert!(second.is_current());
        assert_eq!(epochs.active_keys(), 2);
        drop(first);
        drop(second);
        assert_eq!(epochs.active_keys(), 0);

        for index in 0..100_000 {
            drop(epochs.capture(&format!("transient-{index}")));
        }
        assert_eq!(epochs.active_keys(), 0);
    }
}
