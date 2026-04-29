use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct XrayState {
    /// trace_id → segments (raw JSON segment documents)
    pub traces: DashMap<String, Trace>,
    /// Per-rule sampling state for GetSamplingRules.
    pub sampling_rules: DashMap<String, serde_json::Value>,
    pub groups: DashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub trace_id: String,
    pub segments: Vec<TraceSegment>,
    pub start_time: f64,
    pub end_time: f64,
    pub duration: f64,
    pub services: Vec<String>,
    pub has_error: bool,
    pub has_fault: bool,
    pub has_throttle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSegment {
    pub id: String,
    pub document: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct XrayStateSnapshot {
    pub traces: Vec<Trace>,
    pub sampling_rules: HashMap<String, serde_json::Value>,
    pub groups: HashMap<String, serde_json::Value>,
}

impl XrayState {
    pub fn to_snapshot(&self) -> XrayStateSnapshot {
        XrayStateSnapshot {
            traces: self.traces.iter().map(|e| e.value().clone()).collect(),
            sampling_rules: self
                .sampling_rules
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
            groups: self
                .groups
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: XrayStateSnapshot) {
        self.traces.clear();
        self.sampling_rules.clear();
        self.groups.clear();
        for t in snap.traces {
            self.traces.insert(t.trace_id.clone(), t);
        }
        for (k, v) in snap.sampling_rules {
            self.sampling_rules.insert(k, v);
        }
        for (k, v) in snap.groups {
            self.groups.insert(k, v);
        }
    }
}
