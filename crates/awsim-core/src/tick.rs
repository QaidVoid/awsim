//! Worker pool for slow async work that doesn't belong on the
//! per-service tick path.
//!
//! Services that need to do real I/O on a schedule (Lambda
//! event-source-mapping polling, secrets rotation, scheduler target
//! dispatch, EventBridge replay, etc.) can submit a future here
//! instead of spawning their own free-running task. The pool gives
//! us a single place to bound concurrency and to drain in flight
//! futures on shutdown.
//!
//! Submissions are fire-and-forget: the future runs to completion or
//! gets dropped on shutdown. If the future panics, the panic is
//! caught and logged so it doesn't tear down the runtime.
//!
//! The [`crate::ServiceHandler::tick`] contract is unchanged: tick
//! still must return in under ~10 ms, but it can enqueue any amount
//! of work via the pool.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::{debug, warn};

/// Drain timeout used by [`WorkerPool::shutdown`] if no explicit
/// deadline is supplied.
pub const DEFAULT_SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// Shared worker pool. Cloneable so handlers can keep their own
/// handle without each one owning a separate set of tasks.
#[derive(Clone)]
pub struct WorkerPool {
    inner: Arc<Mutex<JoinSet<()>>>,
}

impl WorkerPool {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(JoinSet::new())),
        }
    }

    /// Submit a future to run in the background. Panics in the
    /// future are caught and logged so one bad job does not poison
    /// the rest of the pool.
    pub async fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        inner.spawn(async move {
            if let Err(panic) = AssertUnwindSafe(future).catch_unwind().await {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&'static str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                warn!(panic = %msg, "worker pool job panicked");
            }
        });
    }

    /// Number of in-flight jobs. Useful for tests and metrics.
    pub async fn in_flight(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Drain in-flight jobs, waiting up to `deadline` for them to
    /// finish. Anything still running past the deadline is aborted.
    pub async fn shutdown(&self, deadline: std::time::Duration) {
        let drain = async {
            let mut inner = self.inner.lock().await;
            while inner.join_next().await.is_some() {}
        };
        match tokio::time::timeout(deadline, drain).await {
            Ok(()) => debug!("worker pool drained cleanly"),
            Err(_) => {
                let mut inner = self.inner.lock().await;
                let aborted = inner.len();
                inner.abort_all();
                warn!(
                    aborted,
                    "worker pool drain deadline exceeded; remaining jobs aborted"
                );
            }
        }
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::Duration;

    #[tokio::test]
    async fn spawned_future_runs_to_completion() {
        let pool = WorkerPool::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        pool.spawn(async move {
            c.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        pool.shutdown(Duration::from_secs(1)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn many_concurrent_jobs_all_complete() {
        let pool = WorkerPool::new();
        let counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..32 {
            let c = counter.clone();
            pool.spawn(async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                c.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        }
        pool.shutdown(Duration::from_secs(1)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 32);
    }

    #[tokio::test]
    async fn panic_in_job_is_caught_and_does_not_kill_pool() {
        let pool = WorkerPool::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = counter.clone();
        pool.spawn(async move {
            c1.fetch_add(1, Ordering::SeqCst);
            panic!("intentional test panic");
        })
        .await;
        let c2 = counter.clone();
        pool.spawn(async move {
            c2.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        pool.shutdown(Duration::from_secs(1)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn shutdown_aborts_jobs_past_deadline() {
        let pool = WorkerPool::new();
        let started = Arc::new(AtomicUsize::new(0));
        let s = started.clone();
        pool.spawn(async move {
            s.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(10)).await;
        })
        .await;
        // Yield so the spawned task definitely starts.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(started.load(Ordering::SeqCst), 1);
        pool.shutdown(Duration::from_millis(50)).await;
    }
}
