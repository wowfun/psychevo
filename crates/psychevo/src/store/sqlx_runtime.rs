use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::error::Result;

use super::{StateRuntime, StateRuntimeInner};

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
                if let crate::error::Error::Sqlx(error) = error
                    && is_sqlx_busy_error(error)
                {
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

    pub async fn close(&self) {
        #[cfg(test)]
        {
            let barrier = self
                .inner
                .state_close_barrier
                .lock()
                .expect("State close barrier poisoned")
                .take();
            if let Some((entered, release)) = barrier {
                entered.notify_one();
                release.notified().await;
            }
        }
        self.inner.pool.close().await;
    }

    #[cfg(test)]
    pub(crate) fn set_close_barrier_for_test(
        &self,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) {
        *self
            .inner
            .state_close_barrier
            .lock()
            .expect("State close barrier poisoned") = Some((entered, release));
    }
}

fn elapsed_micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(super) fn is_sqlx_busy_error(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database) = error else {
        return false;
    };
    database
        .code()
        .as_deref()
        .is_some_and(|code| code == "5" || code == "6" || code == "261" || code == "262")
}
