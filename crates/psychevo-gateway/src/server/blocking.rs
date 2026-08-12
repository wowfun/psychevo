use std::future::Future;
use std::sync::{Arc, LazyLock};

use tokio::sync::Semaphore;

const MAX_BLOCKING_RPC_WORK: usize = 32;
const MAX_SLOW_BLOCKING_RPC_WORK: usize = 4;
static BLOCKING_RPC_ADMISSION: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_BLOCKING_RPC_WORK)));
static SLOW_BLOCKING_RPC_ADMISSION: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_SLOW_BLOCKING_RPC_WORK)));

pub(super) async fn run_blocking<T, F>(work: F) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
{
    run_blocking_with_admission(Arc::clone(&BLOCKING_RPC_ADMISSION), work).await
}

pub(super) async fn run_slow_blocking<T, F>(work: F) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
{
    run_blocking_with_admission(Arc::clone(&SLOW_BLOCKING_RPC_ADMISSION), work).await
}

async fn run_blocking_with_admission<T, F>(
    admission: Arc<Semaphore>,
    work: F,
) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
{
    let permit = admission
        .acquire_owned()
        .await
        .map_err(|_| psychevo::Error::Message("blocking RPC admission closed".to_string()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| psychevo::Error::Message(format!("blocking RPC worker failed: {error}")))?
}

pub(super) async fn run_blocking_mutation<T, F, C>(work: F, after_success: C) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
    C: FnOnce() + Send + 'static,
{
    run_blocking(move || {
        let result = work();
        if result.is_ok() {
            after_success();
        }
        result
    })
    .await
}

pub(super) async fn run_slow_blocking_mutation<T, F, C>(
    work: F,
    after_success: C,
) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
    C: FnOnce() + Send + 'static,
{
    run_slow_blocking(move || {
        let result = work();
        if result.is_ok() {
            after_success();
        }
        result
    })
    .await
}

pub(super) async fn run_blocking_mutation_async<T, F, C, Fut>(
    work: F,
    after_success: C,
) -> psychevo::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> psychevo::Result<T> + Send + 'static,
    C: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let result = run_blocking(work).await;
        if result.is_ok() {
            after_success().await;
        }
        result
    })
    .await
    .map_err(|error| {
        psychevo::Error::Message(format!("blocking RPC continuation failed: {error}"))
    })?
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn blocking_rpc_work_is_admitted_before_workers_spawn() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let tasks = (0..256)
            .map(|_| {
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                tokio::spawn(run_blocking(move || {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(2));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }))
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.await.expect("task").expect("blocking work");
        }
        let peak = peak.load(Ordering::SeqCst);
        assert!(peak > 1, "fixture must exercise concurrent blocking work");
        assert!(peak <= MAX_BLOCKING_RPC_WORK);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn canceled_blocking_mutation_still_runs_success_continuation() {
        let invalidations = Arc::new(AtomicUsize::new(0));
        let worker_invalidations = Arc::clone(&invalidations);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let task = tokio::spawn(run_blocking_mutation(
            move || {
                started_tx.send(()).expect("started receiver");
                release_rx.recv().expect("release worker");
                Ok(())
            },
            move || {
                worker_invalidations.fetch_add(1, Ordering::SeqCst);
            },
        ));
        tokio::task::spawn_blocking(move || started_rx.recv().expect("worker started"))
            .await
            .expect("started waiter");

        task.abort();
        release_tx.send(()).expect("release sender");
        tokio::time::timeout(Duration::from_secs(2), async {
            while invalidations.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("successful continuation");
    }

    #[tokio::test]
    async fn slow_blocking_work_cannot_exhaust_short_work_admission() {
        let short_admission = Arc::new(Semaphore::new(1));
        let slow_admission = Arc::new(Semaphore::new(1));
        let slow_permit = Arc::clone(&slow_admission)
            .acquire_owned()
            .await
            .expect("slow permit");
        let waiting_slow = tokio::spawn(run_blocking_with_admission(slow_admission, || Ok(())));
        tokio::task::yield_now().await;

        tokio::time::timeout(
            Duration::from_secs(1),
            run_blocking_with_admission(short_admission, || Ok(())),
        )
        .await
        .expect("short work must not wait for the saturated slow lane")
        .expect("short work");

        drop(slow_permit);
        waiting_slow.await.expect("slow task").expect("slow work");
    }

    #[tokio::test]
    async fn canceled_async_mutation_still_runs_success_continuation() {
        let invalidations = Arc::new(AtomicUsize::new(0));
        let continuation_invalidations = Arc::clone(&invalidations);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let task = tokio::spawn(run_blocking_mutation_async(
            move || {
                started_tx.send(()).expect("started receiver");
                release_rx.recv().expect("release worker");
                Ok(())
            },
            move || async move {
                tokio::task::yield_now().await;
                continuation_invalidations.fetch_add(1, Ordering::SeqCst);
            },
        ));
        tokio::task::spawn_blocking(move || started_rx.recv().expect("worker started"))
            .await
            .expect("started waiter");

        task.abort();
        release_tx.send(()).expect("release sender");
        tokio::time::timeout(Duration::from_secs(2), async {
            while invalidations.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("successful async continuation");
    }
}
