use std::collections::VecDeque;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::rule::{ChaosEffect, ChaosRule, ErrorEffect};

/// What the engine wants the gateway to do for this request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChaosOutcome {
    /// Rule that fired. Used for logging + injection-count bookkeeping.
    pub rule_id: String,
    /// Wait this long before continuing (or before erroring).
    pub latency: Option<Duration>,
    /// Skip the handler and return this synthetic error.
    pub error: Option<ErrorEffect>,
}

const RECENT_INJECTIONS_CAP: usize = 256;

/// One entry in the recent-injections ring buffer surfaced via
/// `/_awsim/chaos/stats` — used by the dashboard's sparkline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentInjection {
    pub ts: u64,
    pub rule_id: String,
    pub service: String,
    pub operation: Option<String>,
}

/// Persisted snapshot shape. Wraps the rule list so the schema can
/// grow new fields without forcing a bump.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChaosSnapshot {
    pub rules: Vec<ChaosRule>,
}

/// In-memory chaos rules + recent-injection ring buffer. Cheaply
/// cloneable behind an `Arc`; share one across the gateway and the
/// admin handlers.
#[derive(Debug, Default)]
pub struct ChaosEngine {
    rules: RwLock<Vec<ChaosRule>>,
    recent: RwLock<VecDeque<RecentInjection>>,
    /// Total injections fired across all rules. Drives the dashboard
    /// "X injections in the last 5 min" summary.
    pub total_injections: AtomicU64,
}

impl ChaosEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate the rule list against a request. Returns the first
    /// rule that matches *and* whose probability roll succeeds —
    /// later rules don't fire even if they'd also match.
    pub fn evaluate(&self, service: &str, operation: Option<&str>) -> Option<ChaosOutcome> {
        let mut rng = rand::thread_rng();
        self.evaluate_with_rng(service, operation, &mut rng)
    }

    /// Same as [`evaluate`] but uses the caller's RNG — useful for
    /// deterministic tests.
    pub fn evaluate_with_rng(
        &self,
        service: &str,
        operation: Option<&str>,
        rng: &mut impl rand::Rng,
    ) -> Option<ChaosOutcome> {
        let rules = self.rules.read().ok()?;
        for rule in rules.iter() {
            if !rule.matches(service, operation) {
                continue;
            }
            if rule.probability <= 0.0 {
                continue;
            }
            // Probability ≥ 1.0 always fires; otherwise roll.
            if rule.probability < 1.0 && rng.gen_range(0.0..1.0) >= rule.probability {
                continue;
            }
            return Some(build_outcome(rule, rng));
        }
        None
    }

    /// Bookkeep an injection — bump the rule's count, push into the
    /// ring buffer. Called by the gateway after `evaluate` returns.
    pub fn record_injection(&self, rule_id: &str, service: &str, operation: Option<&str>) {
        if let Ok(mut rules) = self.rules.write()
            && let Some(rule) = rules.iter_mut().find(|r| r.id == rule_id)
        {
            rule.injection_count = rule.injection_count.saturating_add(1);
        }
        self.total_injections.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut buf) = self.recent.write() {
            if buf.len() >= RECENT_INJECTIONS_CAP {
                buf.pop_front();
            }
            buf.push_back(RecentInjection {
                ts: now,
                rule_id: rule_id.to_string(),
                service: service.to_string(),
                operation: operation.map(|s| s.to_string()),
            });
        }
    }

    pub fn rules(&self) -> Vec<ChaosRule> {
        self.rules.read().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn add_rule(&self, rule: ChaosRule) {
        if let Ok(mut rules) = self.rules.write() {
            rules.push(rule);
        }
    }

    /// Returns `true` when a rule with that id was found + removed.
    pub fn remove_rule(&self, id: &str) -> bool {
        if let Ok(mut rules) = self.rules.write() {
            let before = rules.len();
            rules.retain(|r| r.id != id);
            return rules.len() != before;
        }
        false
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> bool {
        if let Ok(mut rules) = self.rules.write()
            && let Some(rule) = rules.iter_mut().find(|r| r.id == id)
        {
            rule.enabled = enabled;
            return true;
        }
        false
    }

    pub fn clear(&self) {
        if let Ok(mut rules) = self.rules.write() {
            rules.clear();
        }
        if let Ok(mut buf) = self.recent.write() {
            buf.clear();
        }
        self.total_injections.store(0, Ordering::Relaxed);
    }

    pub fn recent_injections(&self) -> Vec<RecentInjection> {
        self.recent
            .read()
            .map(|r| r.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Snapshot just the rules (recent buffer + counters are
    /// transient and not worth persisting across restarts).
    pub fn snapshot(&self) -> ChaosSnapshot {
        ChaosSnapshot {
            rules: self.rules(),
        }
    }

    pub fn restore(&self, snap: ChaosSnapshot) {
        if let Ok(mut rules) = self.rules.write() {
            *rules = snap.rules;
        }
    }

    /// JSON-serialise the current rule list. Returns `None` only on
    /// the (effectively unreachable) serde failure path so callers
    /// can ignore the error rather than handle it.
    pub fn snapshot_to_bytes(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(&self.snapshot()).ok()
    }

    pub fn restore_from_bytes(&self, bytes: &[u8]) -> Result<(), serde_json::Error> {
        let snap: ChaosSnapshot = serde_json::from_slice(bytes)?;
        self.restore(snap);
        Ok(())
    }
}

fn build_outcome(rule: &ChaosRule, rng: &mut impl rand::Rng) -> ChaosOutcome {
    let (latency, error) = match &rule.effect {
        ChaosEffect::Error(e) => (None, Some(e.clone())),
        ChaosEffect::Latency(l) => (Some(Duration::from_millis(l.sample(rng))), None),
        ChaosEffect::Both { latency, error } => (
            Some(Duration::from_millis(latency.sample(rng))),
            Some(error.clone()),
        ),
    };
    ChaosOutcome {
        rule_id: rule.id.clone(),
        latency,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::{LatencyEffect, OperationMatch, ServiceMatch};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn rule(
        id: &str,
        svc: ServiceMatch,
        op: OperationMatch,
        p: f64,
        eff: ChaosEffect,
    ) -> ChaosRule {
        ChaosRule {
            id: id.to_string(),
            service: svc,
            operation: op,
            probability: p,
            effect: eff,
            enabled: true,
            label: None,
            created_at: 0,
            injection_count: 0,
        }
    }

    fn err(code: &str) -> ErrorEffect {
        ErrorEffect {
            status: 503,
            code: code.to_string(),
            message: format!("synthetic {code}"),
            retry_after_secs: None,
        }
    }

    #[test]
    fn empty_engine_returns_none() {
        let e = ChaosEngine::new();
        let mut rng = StdRng::seed_from_u64(0);
        assert!(
            e.evaluate_with_rng("s3", Some("PutObject"), &mut rng)
                .is_none()
        );
    }

    #[test]
    fn matching_rule_fires_at_probability_one() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Exact("s3".into()),
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("SlowDown")),
        ));
        let mut rng = StdRng::seed_from_u64(0);
        let outcome = e.evaluate_with_rng("s3", Some("PutObject"), &mut rng);
        assert!(outcome.is_some());
        assert_eq!(outcome.unwrap().rule_id, "r1");
    }

    #[test]
    fn non_matching_service_skips_rule() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Exact("s3".into()),
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("SlowDown")),
        ));
        let mut rng = StdRng::seed_from_u64(0);
        assert!(
            e.evaluate_with_rng("dynamodb", Some("PutItem"), &mut rng)
                .is_none()
        );
    }

    #[test]
    fn disabled_rule_is_inert() {
        let e = ChaosEngine::new();
        let mut r = rule(
            "r1",
            ServiceMatch::Any,
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("SlowDown")),
        );
        r.enabled = false;
        e.add_rule(r);
        let mut rng = StdRng::seed_from_u64(0);
        assert!(
            e.evaluate_with_rng("s3", Some("PutObject"), &mut rng)
                .is_none()
        );
    }

    #[test]
    fn first_matching_rule_wins() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "first",
            ServiceMatch::Any,
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("First")),
        ));
        e.add_rule(rule(
            "second",
            ServiceMatch::Any,
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("Second")),
        ));
        let mut rng = StdRng::seed_from_u64(0);
        let outcome = e
            .evaluate_with_rng("s3", Some("PutObject"), &mut rng)
            .unwrap();
        assert_eq!(outcome.rule_id, "first");
    }

    #[test]
    fn probability_zero_never_fires() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Any,
            OperationMatch::Any,
            0.0,
            ChaosEffect::Error(err("X")),
        ));
        let mut rng = StdRng::seed_from_u64(0);
        for _ in 0..100 {
            assert!(e.evaluate_with_rng("s3", Some("Op"), &mut rng).is_none());
        }
    }

    #[test]
    fn record_injection_bumps_counter() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Any,
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(err("X")),
        ));
        e.record_injection("r1", "s3", Some("PutObject"));
        e.record_injection("r1", "s3", Some("GetObject"));
        let rules = e.rules();
        assert_eq!(rules[0].injection_count, 2);
        assert_eq!(e.total_injections.load(Ordering::Relaxed), 2);
        assert_eq!(e.recent_injections().len(), 2);
    }

    #[test]
    fn snapshot_to_bytes_round_trip() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Exact("s3".into()),
            OperationMatch::Any,
            0.25,
            ChaosEffect::Error(err("SlowDown")),
        ));
        let bytes = e.snapshot_to_bytes().expect("serialise");

        let other = ChaosEngine::new();
        other.restore_from_bytes(&bytes).expect("deserialise");
        let rules = other.rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "r1");
        assert_eq!(rules[0].probability, 0.25);
    }

    #[test]
    fn snapshot_round_trip_preserves_rules() {
        let e = ChaosEngine::new();
        e.add_rule(rule(
            "r1",
            ServiceMatch::Exact("s3".into()),
            OperationMatch::Exact("PutObject".into()),
            0.5,
            ChaosEffect::Latency(LatencyEffect {
                min_ms: 100,
                max_ms: 300,
            }),
        ));
        let snap = e.snapshot();
        let bytes = serde_json::to_vec(&snap).unwrap();
        let restored: ChaosSnapshot = serde_json::from_slice(&bytes).unwrap();

        let other = ChaosEngine::new();
        other.restore(restored);
        let rules = other.rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "r1");
        assert_eq!(rules[0].probability, 0.5);
    }
}
