use serde::{Deserialize, Serialize};

/// Match a request against a service signing name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", tag = "kind", content = "value")]
pub enum ServiceMatch {
    /// Matches every service. Serialised as `{"kind":"any"}`.
    Any,
    /// Matches one signing name exactly (e.g. `"s3"`).
    Exact(String),
}

impl ServiceMatch {
    pub fn matches(&self, service: &str) -> bool {
        match self {
            ServiceMatch::Any => true,
            ServiceMatch::Exact(s) => s.eq_ignore_ascii_case(service),
        }
    }
}

/// Match a request against an operation name. Operations are
/// optional on `RequestEvent`s (raw / unparseable requests have
/// `None`); rules with [`OperationMatch::Any`] still fire on those.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", tag = "kind", content = "value")]
pub enum OperationMatch {
    Any,
    Exact(String),
}

impl OperationMatch {
    pub fn matches(&self, operation: Option<&str>) -> bool {
        match self {
            OperationMatch::Any => true,
            OperationMatch::Exact(target) => {
                operation.is_some_and(|op| op.eq_ignore_ascii_case(target))
            }
        }
    }
}

/// Synthetic AWS error to return when a rule fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorEffect {
    /// HTTP status code to return (e.g. 503).
    pub status: u16,
    /// AWS error code (e.g. `"SlowDown"`, `"Throttling"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional `Retry-After` seconds — set to encourage SDK retry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

/// Latency to inject before continuing (or before the error fires).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyEffect {
    pub min_ms: u64,
    pub max_ms: u64,
}

impl LatencyEffect {
    /// Pick a millisecond delay uniformly within `[min_ms, max_ms]`.
    pub fn sample(&self, rng: &mut impl rand::Rng) -> u64 {
        if self.max_ms <= self.min_ms {
            return self.min_ms;
        }
        rng.gen_range(self.min_ms..=self.max_ms)
    }
}

/// What happens when a rule fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChaosEffect {
    /// Skip the handler and return a synthetic AWS error.
    Error(ErrorEffect),
    /// Delay the request before passing it through to the handler.
    Latency(LatencyEffect),
    /// Delay then error.
    Both {
        latency: LatencyEffect,
        error: ErrorEffect,
    },
}

/// Optional fixed window of unix-second timestamps. Either bound is
/// optional — `start_ts: None` means "from forever", `end_ts: None`
/// means "until forever". A rule outside its window is treated as
/// disabled even when `enabled = true`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ts: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ts: Option<u64>,
}

/// Periodic on/off cycle: active for `active_secs` out of every
/// `period_secs`, with phase anchored at `anchor_ts`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Flap {
    pub period_secs: u64,
    pub active_secs: u64,
    pub anchor_ts: u64,
}

/// Composable schedule — the rule fires only if every populated
/// component says it's active. `window` and `flap` are independent
/// and combine with AND.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChaosSchedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<TimeWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flap: Option<Flap>,
}

impl ChaosSchedule {
    /// Returns `true` when the schedule allows the rule to fire at
    /// `now`. Empty schedule (no window, no flap) always allows.
    pub fn is_active_at(&self, now: u64) -> bool {
        if let Some(w) = &self.window {
            if let Some(start) = w.start_ts
                && now < start
            {
                return false;
            }
            if let Some(end) = w.end_ts
                && now >= end
            {
                return false;
            }
        }
        if let Some(f) = &self.flap {
            if f.period_secs == 0 || f.active_secs == 0 {
                return false;
            }
            // Phase = how far into the current period we are.
            let phase = now.saturating_sub(f.anchor_ts) % f.period_secs;
            if phase >= f.active_secs {
                return false;
            }
        }
        true
    }
}

/// One chaos rule — a match predicate plus an effect to inject when
/// `probability` rolls true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosRule {
    pub id: String,
    pub service: ServiceMatch,
    pub operation: OperationMatch,
    /// Probability in `[0.0, 1.0]`. `1.0` always fires; `0.0` never.
    pub probability: f64,
    pub effect: ChaosEffect,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional human label for the dashboard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Unix-second timestamp the rule was created.
    #[serde(default)]
    pub created_at: u64,
    /// How many times this rule has fired since creation. Bumped by
    /// the engine; persisted across restarts.
    #[serde(default)]
    pub injection_count: u64,
    /// Optional time-based gating — windows + flap cycles. Rules
    /// without a schedule are always active when `enabled = true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ChaosSchedule>,
}

fn default_enabled() -> bool {
    true
}

impl ChaosRule {
    /// Returns `true` when the rule's match predicate matches and
    /// the rule is enabled. Probability is *not* evaluated here —
    /// the engine handles the dice roll.
    pub fn matches(&self, service: &str, operation: Option<&str>) -> bool {
        self.enabled && self.service.matches(service) && self.operation.matches(operation)
    }
}
