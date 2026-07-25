use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::error::Result;

use super::{StateRuntime, StateRuntimeInner};

const WAL_CHECKPOINT_EVERY_WRITES: usize = 50;

pub(crate) struct StateOperation {
    inner: Arc<StateRuntimeInner>,
    started: Instant,
    finished: bool,
}

impl StateOperation {
    pub(crate) fn finish<T>(&mut self, result: &Result<T>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.inner
            .in_flight_operations
            .fetch_sub(1, Ordering::Relaxed);
        self.inner
            .execute_latency_micros
            .fetch_add(elapsed_micros(self.started), Ordering::Relaxed);
        match result {
            Ok(_) => {
                self.inner
                    .completed_operations
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                self.inner.failed_operations.fetch_add(1, Ordering::Relaxed);
                if is_sqlx_busy_error(error) {
                    self.inner.busy_operations.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

impl Drop for StateOperation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.inner
            .in_flight_operations
            .fetch_sub(1, Ordering::Relaxed);
        self.inner.failed_operations.fetch_add(1, Ordering::Relaxed);
        self.inner
            .execute_latency_micros
            .fetch_add(elapsed_micros(self.started), Ordering::Relaxed);
    }
}

impl StateRuntime {
    pub(crate) async fn observe_sqlx<T>(
        &self,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut operation = self.begin_sqlx_operation();
        let result = future.await;
        operation.finish(&result);
        result
    }

    pub(crate) fn begin_sqlx_operation(&self) -> StateOperation {
        self.inner
            .in_flight_operations
            .fetch_add(1, Ordering::Relaxed);
        StateOperation {
            inner: self.inner.clone(),
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) async fn acquire_sqlx(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
        let started = Instant::now();
        let result = self.inner.pool.acquire().await.map_err(Into::into);
        self.inner
            .acquire_latency_micros
            .fetch_add(elapsed_micros(started), Ordering::Relaxed);
        result
    }

    pub(crate) async fn begin_sqlx_write(
        &self,
    ) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        let started = Instant::now();
        let result = self
            .inner
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(Into::into);
        self.inner
            .acquire_latency_micros
            .fetch_add(elapsed_micros(started), Ordering::Relaxed);
        result
    }

    pub(crate) async fn finish_sqlx_write(&self) {
        let successful_writes = self.inner.successful_writes.fetch_add(1, Ordering::Relaxed) + 1;
        if should_checkpoint(successful_writes) {
            if sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
                .execute(&self.inner.pool)
                .await
                .is_ok()
            {
                self.inner.checkpoint_count.fetch_add(1, Ordering::Relaxed);
            } else {
                self.inner.failed_operations.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub async fn close(&self) {
        self.inner.pool.close().await;
    }
}

fn elapsed_micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

fn is_sqlx_busy_error(error: &crate::error::Error) -> bool {
    let crate::error::Error::Sqlx(sqlx::Error::Database(database)) = error else {
        return false;
    };
    database
        .code()
        .as_deref()
        .is_some_and(|code| code == "5" || code == "6" || code == "261" || code == "262")
}

fn should_checkpoint(successful_writes: usize) -> bool {
    successful_writes != 0 && successful_writes.is_multiple_of(WAL_CHECKPOINT_EVERY_WRITES)
}

#[cfg(test)]
mod tests {
    use super::should_checkpoint;

    #[test]
    fn checkpoint_cadence_is_every_50_successful_writes() {
        assert!(!should_checkpoint(0));
        assert!(!should_checkpoint(1));
        assert!(!should_checkpoint(49));
        assert!(should_checkpoint(50));
        assert!(!should_checkpoint(51));
        assert!(should_checkpoint(100));
    }
}
