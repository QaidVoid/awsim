//! In-process metrics + recent-invocations ring for the gateway.
//!
//! Two registries living behind cloneable handles, both reset on
//! process restart:
//!
//! - [`MetricsRegistry`] aggregates per `(bedrock_id, backend)`
//!   counters (success / retriable / fatal) and a fixed-bucket
//!   latency histogram. Used by the UI to render call counts and
//!   p50/p95 latency chips next to each alias mapping.
//! - [`RecentInvocations`] is a 200-entry ring of one record per
//!   outer-call (one user `InvokeModel` -> N candidate attempts).
//!   Used by the Activity tab to show "why did this fall through
//!   to canned?" detail.
//!
//! Both are intentionally cheap: no atomics, brief per-shard locks
//! via `DashMap`, and a tiny `Mutex<VecDeque>` for the ring.
//! Volume is bounded by the LLM call rate (typically <10 RPS for
//! a dev box), so contention is irrelevant.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Value, json};

/// Final outcome of an attempt or a whole call. Mirrors the
/// `runtime::Attempt` variants so the metrics view can attribute
/// errors to retriable / fatal causes without each call site
/// having to translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Outcome {
    #[serde(rename = "success")]
    Success,
    /// Retriable upstream failure (5xx / 408 / 429 / network).
    /// The runtime rolled forward to the next alias candidate.
    #[serde(rename = "retriable")]
    RetriableError,
    /// Non-retriable upstream failure (translator-build error,
    /// 4xx that won't change). The runtime aborted the call.
    #[serde(rename = "fatal")]
    FatalError,
}

/// Logical operation kind. Lets the UI render a chip telling the
/// reader which path served the call (chat / chat-stream / embed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpKind {
    Chat,
    ChatStream,
    Embed,
}

/// Fixed bucket boundaries in milliseconds. Twelve buckets cover
/// ~10ms (cache-hit-like) through ~60s (large generation calls);
/// anything slower lands in the +inf overflow bucket. Picked so
/// that p50/p95 for both local Ollama (~hundreds of ms) and slow
/// hosted backends (~seconds) lands somewhere informative.
const BUCKET_BOUNDS_MS: &[u64] = &[
    10, 25, 50, 100, 250, 500, 1_000, 2_000, 5_000, 10_000, 30_000, 60_000,
];

#[derive(Debug, Default, Clone)]
struct MetricEntry {
    success: u64,
    retriable: u64,
    fatal: u64,
    /// One slot per BUCKET_BOUNDS_MS entry plus an overflow slot
    /// at the end for samples above the last boundary.
    histogram: [u64; BUCKET_BOUNDS_MS.len() + 1],
    /// Most recent error message; reset on the next success.
    /// Surfaces in the UI so users see what's currently failing
    /// without having to trawl the ring buffer.
    last_error: Option<String>,
}

impl MetricEntry {
    fn record(&mut self, outcome: Outcome, latency_ms: u64, err: Option<&str>) {
        match outcome {
            Outcome::Success => {
                self.success += 1;
                self.last_error = None;
            }
            Outcome::RetriableError => {
                self.retriable += 1;
                if let Some(e) = err {
                    self.last_error = Some(truncate(e, 240));
                }
            }
            Outcome::FatalError => {
                self.fatal += 1;
                if let Some(e) = err {
                    self.last_error = Some(truncate(e, 240));
                }
            }
        }
        let idx = bucket_for(latency_ms);
        self.histogram[idx] += 1;
    }

    fn percentile(&self, target: f64) -> Option<u64> {
        let total: u64 = self.histogram.iter().sum();
        if total == 0 {
            return None;
        }
        let goal = ((total as f64) * target).ceil() as u64;
        let mut cum: u64 = 0;
        for (i, &count) in self.histogram.iter().enumerate() {
            cum += count;
            if cum >= goal {
                return Some(bound_upper(i));
            }
        }
        Some(bound_upper(self.histogram.len() - 1))
    }
}

fn bucket_for(latency_ms: u64) -> usize {
    BUCKET_BOUNDS_MS
        .iter()
        .position(|&b| latency_ms <= b)
        .unwrap_or(BUCKET_BOUNDS_MS.len())
}

fn bound_upper(idx: usize) -> u64 {
    if idx < BUCKET_BOUNDS_MS.len() {
        BUCKET_BOUNDS_MS[idx]
    } else {
        // overflow bucket; report the last bound as a floor
        BUCKET_BOUNDS_MS[BUCKET_BOUNDS_MS.len() - 1]
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut out = s.chars().take(max).collect::<String>();
        out.push_str("...");
        out
    }
}

/// Process-lifetime metrics registry. Cheaply cloneable; the
/// underlying DashMap is shared across clones so the runtime
/// writes and the admin handler reads see the same data.
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    inner: Arc<DashMap<MetricKey, MetricEntry>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MetricKey {
    pub bedrock_id: String,
    pub backend: String,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one attempt. Called per candidate by the runtime
    /// (so a multi-target alias with one retriable + one success
    /// books TWO records, one per backend).
    pub fn record(
        &self,
        bedrock_id: &str,
        backend: &str,
        outcome: Outcome,
        latency_ms: u64,
        err: Option<&str>,
    ) {
        let key = MetricKey {
            bedrock_id: bedrock_id.to_string(),
            backend: backend.to_string(),
        };
        self.inner
            .entry(key)
            .or_default()
            .record(outcome, latency_ms, err);
    }

    /// Snapshot for the admin endpoint. Walks the map, computes
    /// p50/p95 from each entry's histogram, returns a JSON shape
    /// the UI can render without further client-side aggregation.
    pub fn snapshot_json(&self) -> Value {
        let mut by_mapping: Vec<Value> = self
            .inner
            .iter()
            .map(|kv| {
                let key = kv.key();
                let v = kv.value();
                json!({
                    "bedrockId": key.bedrock_id,
                    "backend": key.backend,
                    "success": v.success,
                    "retriable": v.retriable,
                    "fatal": v.fatal,
                    "total": v.success + v.retriable + v.fatal,
                    "p50Ms": v.percentile(0.5),
                    "p95Ms": v.percentile(0.95),
                    "lastError": v.last_error,
                })
            })
            .collect();
        by_mapping.sort_by(|a, b| {
            let ka = (
                a["bedrockId"].as_str().unwrap_or(""),
                a["backend"].as_str().unwrap_or(""),
            );
            let kb = (
                b["bedrockId"].as_str().unwrap_or(""),
                b["backend"].as_str().unwrap_or(""),
            );
            ka.cmp(&kb)
        });
        let totals = self.inner.iter().fold((0u64, 0u64, 0u64), |(s, r, f), kv| {
            (
                s + kv.value().success,
                r + kv.value().retriable,
                f + kv.value().fatal,
            )
        });
        json!({
            "mappings": by_mapping,
            "totals": {
                "success": totals.0,
                "retriable": totals.1,
                "fatal": totals.2,
                "total": totals.0 + totals.1 + totals.2,
            }
        })
    }
}

const RECENT_CAP: usize = 200;

/// Process-lifetime ring buffer of recent outer-call records.
/// Cheaply cloneable. One push per `InvokeModel` / `Converse` /
/// embed call, regardless of how many candidates were tried.
#[derive(Debug, Clone, Default)]
pub struct RecentInvocations {
    inner: Arc<Mutex<VecDeque<InvocationRecord>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvocationRecord {
    pub at: DateTime<Utc>,
    pub bedrock_id: String,
    pub op: OpKind,
    pub attempts: Vec<AttemptRecord>,
    pub outcome: Outcome,
    pub total_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub backend: String,
    pub tag: String,
    pub outcome: Outcome,
    pub latency_ms: u64,
    pub error: Option<String>,
}

impl RecentInvocations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, record: InvocationRecord) {
        let mut buf = self.inner.lock().expect("RecentInvocations mutex poisoned");
        if buf.len() == RECENT_CAP {
            buf.pop_front();
        }
        buf.push_back(record);
    }

    /// Snapshot newest-first. Bounded length so callers can hand
    /// it straight to the UI without paging.
    pub fn snapshot_json(&self) -> Value {
        let buf = self.inner.lock().expect("RecentInvocations mutex poisoned");
        let arr: Vec<Value> = buf
            .iter()
            .rev()
            .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
            .collect();
        json!({ "invocations": arr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_bumps_outcome_counters_and_histogram() {
        let m = MetricsRegistry::new();
        m.record("anthropic.x", "ollama", Outcome::Success, 30, None);
        m.record("anthropic.x", "ollama", Outcome::Success, 150, None);
        m.record(
            "anthropic.x",
            "ollama",
            Outcome::RetriableError,
            500,
            Some("upstream 503"),
        );
        let snap = m.snapshot_json();
        let row = &snap["mappings"][0];
        assert_eq!(row["success"], 2);
        assert_eq!(row["retriable"], 1);
        assert_eq!(row["fatal"], 0);
        assert_eq!(row["total"], 3);
        // p50 of {30, 150, 500} should land on the 250ms bucket
        // (which holds 150) or higher; p95 reaches the 500ms bucket.
        assert!(row["p50Ms"].as_u64().unwrap() >= 250);
        assert_eq!(row["p95Ms"], 500);
        assert_eq!(row["lastError"], "upstream 503");
    }

    #[test]
    fn last_error_resets_on_success() {
        let m = MetricsRegistry::new();
        m.record(
            "x",
            "b",
            Outcome::FatalError,
            10,
            Some("translator rejected"),
        );
        m.record("x", "b", Outcome::Success, 20, None);
        assert!(m.snapshot_json()["mappings"][0]["lastError"].is_null());
    }

    #[test]
    fn recent_ring_bounded() {
        let r = RecentInvocations::new();
        for i in 0..(RECENT_CAP + 5) {
            r.push(InvocationRecord {
                at: Utc::now(),
                bedrock_id: format!("m-{i}"),
                op: OpKind::Chat,
                attempts: vec![],
                outcome: Outcome::Success,
                total_latency_ms: 1,
            });
        }
        let snap = r.snapshot_json();
        assert_eq!(snap["invocations"].as_array().unwrap().len(), RECENT_CAP);
        // Newest-first: the most recent push should be first.
        let head = &snap["invocations"][0];
        assert_eq!(head["bedrockId"], format!("m-{}", RECENT_CAP + 4));
    }
}
