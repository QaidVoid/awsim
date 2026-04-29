use awsim_core::Snapshottable;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counters for a single (service, operation) bucket.
#[derive(Debug, Default)]
pub struct OpCounter {
    pub count: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub error_count: AtomicU64,
}

impl OpCounter {
    fn record(&self, bytes_in: u64, bytes_out: u64, is_error: bool) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        if is_error {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> OpCounterSnapshot {
        OpCounterSnapshot {
            count: self.count.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
        }
    }

    fn from_snapshot(snap: OpCounterSnapshot) -> Self {
        Self {
            count: AtomicU64::new(snap.count),
            bytes_in: AtomicU64::new(snap.bytes_in),
            bytes_out: AtomicU64::new(snap.bytes_out),
            error_count: AtomicU64::new(snap.error_count),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OpCounterSnapshot {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub bytes_in: u64,
    #[serde(default)]
    pub bytes_out: u64,
    #[serde(default)]
    pub error_count: u64,
}

/// Per-(account, region) usage bucket.
///
/// Outer DashMap keyed by service signing name (e.g. `s3`), inner DashMap
/// keyed by operation name (e.g. `PutObject`).
#[derive(Debug, Default)]
pub struct BillingState {
    pub started_at: AtomicU64,
    services: DashMap<String, DashMap<String, OpCounter>>,
}

impl BillingState {
    pub fn record(
        &self,
        service: &str,
        operation: &str,
        bytes_in: u64,
        bytes_out: u64,
        is_error: bool,
    ) {
        let svc = self.services.entry(service.to_string()).or_default();
        svc.entry(operation.to_string())
            .or_default()
            .record(bytes_in, bytes_out, is_error);
    }

    pub fn ensure_started(&self, now_secs: u64) {
        // CAS from 0 so the first record sets the meter epoch; subsequent
        // calls are no-ops.
        let _ = self
            .started_at
            .compare_exchange(0, now_secs, Ordering::Relaxed, Ordering::Relaxed);
    }

    /// Snapshot all counters into a serialisable map.
    pub fn snapshot_services(&self) -> HashMap<String, HashMap<String, OpCounterSnapshot>> {
        self.services
            .iter()
            .map(|svc| {
                let ops: HashMap<String, OpCounterSnapshot> = svc
                    .value()
                    .iter()
                    .map(|op| (op.key().clone(), op.value().snapshot()))
                    .collect();
                (svc.key().clone(), ops)
            })
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BillingSnapshot {
    pub account_id: String,
    pub region: String,
    #[serde(default)]
    pub started_at: u64,
    pub services: HashMap<String, HashMap<String, OpCounterSnapshot>>,
}

impl Snapshottable for BillingState {
    type Snapshot = BillingSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        BillingSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            started_at: self.started_at.load(Ordering::Relaxed),
            services: self.snapshot_services(),
        }
    }

    fn from_snapshot(snap: Self::Snapshot) -> (String, String, Self) {
        let services: DashMap<String, DashMap<String, OpCounter>> = DashMap::new();
        for (svc_name, ops) in snap.services {
            let ops_map: DashMap<String, OpCounter> = DashMap::new();
            for (op_name, counter) in ops {
                ops_map.insert(op_name, OpCounter::from_snapshot(counter));
            }
            services.insert(svc_name, ops_map);
        }
        (
            snap.account_id,
            snap.region,
            BillingState {
                started_at: AtomicU64::new(snap.started_at),
                services,
            },
        )
    }
}

pub type BillingStateStore = awsim_core::AccountRegionStore<BillingState>;
