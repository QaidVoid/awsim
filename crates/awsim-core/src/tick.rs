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
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::ServiceHandler;

/// Drain timeout used by [`WorkerPool::shutdown`] if no explicit
/// deadline is supplied.
pub const DEFAULT_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);

/// Default cap on concurrent worker-pool jobs. Bounded at 8 because
/// real I/O on these handlers is supposed to be light; if a service
/// genuinely needs more parallelism, construct the pool explicitly
/// with [`WorkerPool::with_capacity`].
const DEFAULT_MAX_CONCURRENCY: usize = 8;

/// Shared worker pool. Cloneable so handlers can keep their own
/// handle without each one owning a separate set of tasks.
#[derive(Clone)]
pub struct WorkerPool {
    inner: Arc<Mutex<JoinSet<()>>>,
    /// Bounds the number of jobs running at once so a pathological
    /// burst of `spawn` calls can't starve the tokio runtime.
    permits: Arc<Semaphore>,
}

impl WorkerPool {
    /// Construct a pool sized to `min(available_parallelism, 8)`.
    pub fn new() -> Self {
        let parallelism = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1);
        Self::with_capacity(parallelism.min(DEFAULT_MAX_CONCURRENCY))
    }

    /// Construct a pool with an explicit concurrency cap.
    pub fn with_capacity(max_concurrency: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JoinSet::new())),
            permits: Arc::new(Semaphore::new(max_concurrency.max(1))),
        }
    }

    /// Submit a future to run in the background. Panics in the
    /// future are caught and logged so one bad job does not poison
    /// the rest of the pool.
    pub async fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let permits = self.permits.clone();
        let mut inner = self.inner.lock().await;
        inner.spawn(async move {
            // The unwrap is safe: the semaphore is owned by the
            // pool and never closed while jobs are submitted.
            let _permit = permits
                .acquire_owned()
                .await
                .expect("pool semaphore closed");
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

/// Test harness for the central tick loop.
///
/// `TestDriver` lets unit tests register one or more
/// [`ServiceHandler`]s and invoke their [`ServiceHandler::tick`] hooks
/// deterministically, without spawning a real interval task or
/// sleeping. Each [`Self::advance`] call runs one tick per registered
/// service in registration order, the same sequence the production
/// driver would use.
///
/// `TestDriver` does not patch wall-clock-reading services; callers
/// that depend on `SystemTime::now()` inside `tick` should either
/// inject a clock or accept that real time still advances. The
/// virtual elapsed time accumulator is exposed via
/// [`Self::elapsed`] for tests that want to assert "the harness
/// thinks N seconds have passed".
pub struct TestDriver {
    services: Vec<Arc<dyn ServiceHandler>>,
    elapsed: std::sync::Mutex<Duration>,
}

impl TestDriver {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            elapsed: std::sync::Mutex::new(Duration::ZERO),
        }
    }

    /// Register a service so it receives a tick on every
    /// [`Self::advance`] call. Services tick in registration order.
    pub fn register(&mut self, service: Arc<dyn ServiceHandler>) {
        self.services.push(service);
    }

    /// Advance the virtual clock by `dur` and invoke each registered
    /// service's `tick` exactly once. Tests that need multiple ticks
    /// (e.g. a 10s scheduler test) should call `advance` once per
    /// desired tick.
    pub async fn advance(&self, dur: Duration) {
        *self.elapsed.lock().expect("elapsed mutex poisoned") += dur;
        for svc in &self.services {
            svc.tick().await;
        }
    }

    /// Total time the harness has been advanced.
    pub fn elapsed(&self) -> Duration {
        *self.elapsed.lock().expect("elapsed mutex poisoned")
    }
}

impl Default for TestDriver {
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

    #[tokio::test]
    async fn pool_caps_concurrency_at_capacity() {
        // With capacity=2 and four 50ms jobs, the second pair can only
        // start after the first pair finishes; total wall time should
        // be at least 2 quanta.
        let pool = WorkerPool::with_capacity(2);
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            let r = running.clone();
            let p = peak.clone();
            pool.spawn(async move {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                r.fetch_sub(1, Ordering::SeqCst);
            })
            .await;
        }
        pool.shutdown(Duration::from_secs(2)).await;
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "peak concurrency exceeded cap of 2"
        );
    }

    use crate::error::AwsError;
    use crate::{Protocol, RequestContext, ServiceHandler};
    use async_trait::async_trait;
    use serde_json::Value;

    struct CountingHandler {
        name: &'static str,
        ticks: AtomicUsize,
    }

    #[async_trait]
    impl ServiceHandler for CountingHandler {
        fn service_name(&self) -> &str {
            self.name
        }
        fn protocol(&self) -> Protocol {
            Protocol::AwsJson1_1
        }
        async fn handle(
            &self,
            _operation: &str,
            _input: Value,
            _ctx: &RequestContext,
        ) -> Result<Value, AwsError> {
            Ok(Value::Null)
        }
        async fn tick(&self) {
            self.ticks.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_driver_invokes_each_handler_on_advance() {
        let a = Arc::new(CountingHandler {
            name: "a",
            ticks: AtomicUsize::new(0),
        });
        let b = Arc::new(CountingHandler {
            name: "b",
            ticks: AtomicUsize::new(0),
        });
        let mut driver = TestDriver::new();
        driver.register(a.clone());
        driver.register(b.clone());

        driver.advance(Duration::from_secs(1)).await;
        driver.advance(Duration::from_secs(1)).await;
        driver.advance(Duration::from_secs(1)).await;

        assert_eq!(a.ticks.load(Ordering::SeqCst), 3);
        assert_eq!(b.ticks.load(Ordering::SeqCst), 3);
        assert_eq!(driver.elapsed(), Duration::from_secs(3));
    }
}
