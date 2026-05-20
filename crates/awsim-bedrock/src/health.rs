//! Per-backend health tracking + background poller.
//!
//! A long-lived [`HealthRegistry`] holds the last status for every
//! backend the user has configured. The alias resolver consults it
//! before expanding a multi-target alias so a `Down` backend gets
//! routed around without the request having to fail first; the UI
//! reads the same registry to surface red/amber/green pills and a
//! short check history per backend.
//!
//! Status transitions are deliberately sticky to avoid flapping:
//!
//! - `Healthy` -> `Degraded` after a single check failure.
//! - `Degraded` -> `Down` after a second consecutive failure.
//! - any -> `Healthy` after one success.
//!
//! This means a transient blip surfaces as `Degraded` (still routed
//! to so the user gets the upstream's real error) while a sustained
//! outage surfaces as `Down` (skipped by the alias resolver).
//!
//! The registry survives across [`crate::BedrockBackends`] swaps; the
//! poller watches the swap so it picks up new backends within one
//! tick of the user adding them via the gateway UI.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Value, json};
use tracing::debug;

use crate::backend::{BedrockBackend, BedrockBackends};

const HISTORY_CAP: usize = 120; // ~1 hour at 30s intervals

/// Health status surfaced to the resolver and the UI. Translating
/// a probe outcome into one of three buckets keeps the alias
/// resolver decision-rule trivial (skip only `Down`) and gives the
/// UI a small, stable colour palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendStatus {
    /// Last probe succeeded.
    Healthy,
    /// One probe failed; might recover on the next tick. Still
    /// routed to so the upstream error reaches the caller.
    Degraded,
    /// Two or more consecutive failures. Skipped by alias
    /// resolution; the UI shows a red pill.
    Down,
    /// No probe has run yet. Treated as healthy by the resolver so
    /// freshly-added backends aren't penalised before the first
    /// tick. Surfaces as a grey pill in the UI.
    #[default]
    Unknown,
}

/// One probe result, kept in a per-backend ring buffer so the UI
/// can render a short history sparkline.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRecord {
    pub at: DateTime<Utc>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

impl CheckRecord {
    fn ok(at: DateTime<Utc>, latency_ms: u64) -> Self {
        Self {
            at,
            latency_ms: Some(latency_ms),
            error: None,
        }
    }
    fn err(at: DateTime<Utc>, error: String) -> Self {
        Self {
            at,
            latency_ms: None,
            error: Some(error),
        }
    }
    fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

/// Aggregate health for one backend. Updated in-place by the
/// poller; snapshotted by the admin endpoint and by `is_down`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BackendHealth {
    pub status: BackendStatus,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_latency_ms: Option<u64>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub history: Vec<CheckRecord>,
}

/// Process-lifetime registry of backend health. Cheaply cloneable
/// (Arc inside). Outlives any individual [`BedrockBackends`] so
/// statuses don't reset on every config hot-swap.
#[derive(Debug, Clone, Default)]
pub struct HealthRegistry {
    inner: Arc<DashMap<String, BackendHealth>>,
}

impl HealthRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a probe result. Updates status + history + counters
    /// according to the sticky transition rules documented at the
    /// module top.
    pub fn record(&self, backend: &str, record: CheckRecord) {
        let mut entry = self.inner.entry(backend.to_string()).or_default();
        let success = record.is_ok();
        if success {
            entry.consecutive_successes = entry.consecutive_successes.saturating_add(1);
            entry.consecutive_failures = 0;
            entry.status = BackendStatus::Healthy;
            entry.last_latency_ms = record.latency_ms;
            entry.last_error = None;
        } else {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            entry.consecutive_successes = 0;
            entry.status = if entry.consecutive_failures >= 2 {
                BackendStatus::Down
            } else {
                BackendStatus::Degraded
            };
            entry.last_latency_ms = None;
            entry.last_error = record.error.clone();
        }
        entry.last_checked_at = Some(record.at);
        push_history(&mut entry.history, record);
    }

    /// True when the backend's last two probes both failed. The
    /// alias resolver uses this to skip multi-target candidates;
    /// `Unknown` / `Degraded` / `Healthy` are all routed to.
    pub fn is_down(&self, backend: &str) -> bool {
        self.inner
            .get(backend)
            .map(|h| h.status == BackendStatus::Down)
            .unwrap_or(false)
    }

    /// Snapshot of one backend's health for the admin endpoint.
    /// Returns `None` if the backend has never been probed.
    pub fn get(&self, backend: &str) -> Option<BackendHealth> {
        self.inner.get(backend).map(|h| h.clone())
    }

    /// Full snapshot as JSON for the admin endpoint. Sorted by
    /// backend name so the UI doesn't reshuffle on every refresh.
    pub fn snapshot_json(&self) -> Value {
        let mut entries: Vec<(String, BackendHealth)> = self
            .inner
            .iter()
            .map(|kv| (kv.key().clone(), kv.value().clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let arr: Vec<Value> = entries
            .into_iter()
            .map(|(name, h)| {
                json!({
                    "backend": name,
                    "status": h.status,
                    "lastCheckedAt": h.last_checked_at,
                    "lastLatencyMs": h.last_latency_ms,
                    "lastError": h.last_error,
                    "consecutiveFailures": h.consecutive_failures,
                    "consecutiveSuccesses": h.consecutive_successes,
                    "history": h.history,
                })
            })
            .collect();
        json!({ "backends": arr })
    }

    /// Drop the entry for a backend the user has removed. Called
    /// by the poller when a backend is no longer in the live
    /// registry so the UI doesn't show stale red pills forever.
    pub fn forget(&self, backend: &str) {
        self.inner.remove(backend);
    }

    pub fn known_backends(&self) -> Vec<String> {
        self.inner.iter().map(|kv| kv.key().clone()).collect()
    }
}

fn push_history(history: &mut Vec<CheckRecord>, record: CheckRecord) {
    // VecDeque would be ergonomic for ring semantics, but the
    // history is serialized through serde + read short-lived from
    // the UI, so a plain Vec with manual front-trim keeps the
    // serialize side simple.
    if history.len() >= HISTORY_CAP {
        let drain_count = history.len() - HISTORY_CAP + 1;
        history.drain(0..drain_count);
    }
    history.push(record);
}

/// Probe one backend by GETting `<endpoint>/models` with a tight
/// timeout. Used by both the background poller and the on-demand
/// admin endpoint. Returns the probe outcome as a `CheckRecord`
/// rather than mutating the registry directly so the caller can
/// decide whether to record + with what timestamp.
pub async fn probe(backend: &BedrockBackend, timeout: Duration) -> CheckRecord {
    let url = format!("{}/models", backend.endpoint());
    let mut req = backend.client().get(&url).timeout(timeout);
    if let Some(key) = backend.api_key() {
        req = req.bearer_auth(key);
    }
    let started = std::time::Instant::now();
    let now = Utc::now();
    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let latency = started.elapsed().as_millis() as u64;
            if status.is_success() {
                CheckRecord::ok(now, latency)
            } else {
                CheckRecord::err(now, format!("HTTP {status}"))
            }
        }
        Err(e) => CheckRecord::err(now, e.to_string()),
    }
}

/// Long-running poller. Spawns one tokio task at boot; never
/// cancels (process-lifetime). On each tick, walks the live swap
/// snapshot, probes each configured backend in parallel, records
/// outcomes, and forgets entries whose backend is no longer
/// configured.
pub async fn run_poller(
    swap: Arc<ArcSwap<Option<BedrockBackends>>>,
    registry: HealthRegistry,
    interval: Duration,
    timeout: Duration,
) {
    // Tick on a steady cadence (drops missed ticks rather than
    // bunching them up if a probe round takes longer than the
    // interval, which would otherwise cascade into a stampede).
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Skip the immediate fire so a freshly-booted process doesn't
    // hammer backends within milliseconds of startup.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        poll_once(&swap, &registry, timeout).await;
    }
}

async fn poll_once(
    swap: &ArcSwap<Option<BedrockBackends>>,
    registry: &HealthRegistry,
    timeout: Duration,
) {
    let guard = swap.load();
    let Some(backends) = guard.as_ref().as_ref() else {
        // Canned-response mode: nothing to ping. Forget any
        // stale entries the user might be staring at.
        for name in registry.known_backends() {
            registry.forget(&name);
        }
        return;
    };
    let live: Vec<String> = backends.backend_names();
    let live_set: std::collections::HashSet<String> = live.iter().cloned().collect();
    for stale in registry.known_backends() {
        if !live_set.contains(&stale) {
            registry.forget(&stale);
        }
    }
    let probes = live.into_iter().filter_map(|name| {
        let backend = backends.get_backend(&name)?.clone();
        Some(async move {
            let record = probe(&backend, timeout).await;
            (name, record)
        })
    });
    let results = futures::future::join_all(probes).await;
    for (name, record) in results {
        debug!(
            backend = %name,
            ok = record.is_ok(),
            "bedrock health probe"
        );
        registry.record(&name, record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_default_status() {
        let h = BackendHealth::default();
        assert_eq!(h.status, BackendStatus::Unknown);
    }

    #[test]
    fn single_failure_marks_degraded_not_down() {
        let r = HealthRegistry::new();
        r.record("b", CheckRecord::err(Utc::now(), "boom".into()));
        assert_eq!(r.get("b").unwrap().status, BackendStatus::Degraded);
        assert!(!r.is_down("b"));
    }

    #[test]
    fn two_failures_mark_down_then_one_success_recovers() {
        let r = HealthRegistry::new();
        r.record("b", CheckRecord::err(Utc::now(), "boom".into()));
        r.record("b", CheckRecord::err(Utc::now(), "boom".into()));
        assert!(r.is_down("b"));
        r.record("b", CheckRecord::ok(Utc::now(), 5));
        assert_eq!(r.get("b").unwrap().status, BackendStatus::Healthy);
        assert!(!r.is_down("b"));
    }

    #[test]
    fn history_is_bounded() {
        let r = HealthRegistry::new();
        for _ in 0..(HISTORY_CAP + 10) {
            r.record("b", CheckRecord::ok(Utc::now(), 1));
        }
        assert_eq!(r.get("b").unwrap().history.len(), HISTORY_CAP);
    }

    #[test]
    fn forget_drops_entry() {
        let r = HealthRegistry::new();
        r.record("b", CheckRecord::ok(Utc::now(), 1));
        r.forget("b");
        assert!(r.get("b").is_none());
    }
}
