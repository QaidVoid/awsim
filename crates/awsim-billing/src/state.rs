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

/// Per-service point-in-time storage tracker.
///
/// The poll loop calls `record_sample` periodically; each sample
/// accrues `(avg_bytes_since_last_sample) × (elapsed_seconds) × rate`
/// into the cost accumulator. Cost is stored as integer pico-USD
/// (1e-12 USD) to keep tiny per-sample accruals from truncating to
/// zero — at S3's $0.023/GB-month, 1 MB × 30 s amounts to ~0.27
/// micro-USD, which would round away under coarser units. u64 of
/// pico-USD still gives ~$18M total headroom before overflow.
#[derive(Debug, Default)]
pub struct StorageMetering {
    /// Most recent sampled byte count — also used by the dashboard
    /// so the UI can show "currently storing X GB".
    pub last_sample_bytes: AtomicU64,
    /// Unix timestamp of the most recent sample (seconds).
    pub last_sample_ts: AtomicU64,
    /// Accumulated storage cost in pico-USD (1e-12 USD).
    pub accumulated_cost_picos: AtomicU64,
}

impl StorageMetering {
    fn snapshot(&self) -> StorageMeteringSnapshot {
        StorageMeteringSnapshot {
            last_sample_bytes: self.last_sample_bytes.load(Ordering::Relaxed),
            last_sample_ts: self.last_sample_ts.load(Ordering::Relaxed),
            accumulated_cost_picos: self.accumulated_cost_picos.load(Ordering::Relaxed),
        }
    }

    fn from_snapshot(s: StorageMeteringSnapshot) -> Self {
        Self {
            last_sample_bytes: AtomicU64::new(s.last_sample_bytes),
            last_sample_ts: AtomicU64::new(s.last_sample_ts),
            accumulated_cost_picos: AtomicU64::new(s.accumulated_cost_picos),
        }
    }

    /// Accrue cost for the interval between the previous sample and
    /// `current_bytes` (taken at `now_secs`). Trapezoidal integration:
    /// the average of the two samples × elapsed time × rate.
    pub fn record_sample(&self, current_bytes: u64, now_secs: u64, per_byte_per_sec_usd: f64) {
        let last_ts = self.last_sample_ts.swap(now_secs, Ordering::Relaxed);
        let last_bytes = self
            .last_sample_bytes
            .swap(current_bytes, Ordering::Relaxed);
        if last_ts == 0 || now_secs <= last_ts {
            // First sample (or clock skew) — nothing to accrue yet.
            return;
        }
        let elapsed = (now_secs - last_ts) as f64;
        let avg_bytes = (last_bytes as f64 + current_bytes as f64) / 2.0;
        let cost_usd = avg_bytes * elapsed * per_byte_per_sec_usd;
        if cost_usd > 0.0 {
            let picos = (cost_usd * 1e12) as u64;
            self.accumulated_cost_picos
                .fetch_add(picos, Ordering::Relaxed);
        }
    }

    pub fn cost_usd(&self) -> f64 {
        self.accumulated_cost_picos.load(Ordering::Relaxed) as f64 / 1e12
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StorageMeteringSnapshot {
    pub last_sample_bytes: u64,
    pub last_sample_ts: u64,
    pub accumulated_cost_picos: u64,
}

/// Per-service compute tracker — Lambda's GB-second billing axis.
///
/// Each invocation contributes `duration_ms × assumed_memory_gb × rate`
/// to the accumulated cost; we also keep the running GB-microsecond
/// total so the dashboard can show "X GB-seconds consumed" alongside
/// the dollar amount.
#[derive(Debug, Default)]
pub struct ComputeMetering {
    /// Total GB-microseconds of compute consumed (1 GB-s = 1e6).
    /// u64 fits ~1.8e13 GB-seconds — Lambda would have to run a
    /// 128 MB function flat-out for ~45,000 years to overflow this.
    pub gb_microseconds: AtomicU64,
    /// Accumulated compute cost in pico-USD (1e-12 USD).
    pub accumulated_cost_picos: AtomicU64,
}

impl ComputeMetering {
    fn snapshot(&self) -> ComputeMeteringSnapshot {
        ComputeMeteringSnapshot {
            gb_microseconds: self.gb_microseconds.load(Ordering::Relaxed),
            accumulated_cost_picos: self.accumulated_cost_picos.load(Ordering::Relaxed),
        }
    }

    fn from_snapshot(s: ComputeMeteringSnapshot) -> Self {
        Self {
            gb_microseconds: AtomicU64::new(s.gb_microseconds),
            accumulated_cost_picos: AtomicU64::new(s.accumulated_cost_picos),
        }
    }

    pub fn record(&self, gb_seconds: f64, per_gb_second_usd: f64) {
        if gb_seconds <= 0.0 {
            return;
        }
        let cost = gb_seconds * per_gb_second_usd;
        let picos = (cost * 1e12) as u64;
        let micros = (gb_seconds * 1e6) as u64;
        self.gb_microseconds.fetch_add(micros, Ordering::Relaxed);
        self.accumulated_cost_picos
            .fetch_add(picos, Ordering::Relaxed);
    }

    pub fn cost_usd(&self) -> f64 {
        self.accumulated_cost_picos.load(Ordering::Relaxed) as f64 / 1e12
    }

    pub fn gb_seconds(&self) -> f64 {
        self.gb_microseconds.load(Ordering::Relaxed) as f64 / 1e6
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ComputeMeteringSnapshot {
    pub gb_microseconds: u64,
    pub accumulated_cost_picos: u64,
}

/// Per-(account, region) usage bucket.
///
/// Outer DashMap keyed by service signing name (e.g. `s3`), inner DashMap
/// keyed by operation name (e.g. `PutObject`).
#[derive(Debug, Default)]
pub struct BillingState {
    pub started_at: AtomicU64,
    services: DashMap<String, DashMap<String, OpCounter>>,
    /// Per-service point-in-time storage trackers. Separate from the
    /// per-operation counters because storage is sampled at intervals
    /// rather than incremented per request.
    storage: DashMap<String, StorageMetering>,
    /// Per-service compute trackers (Lambda GB-seconds).
    compute: DashMap<String, ComputeMetering>,
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

    /// Snapshot the storage trackers, keyed by service signing name.
    pub fn snapshot_storage(&self) -> HashMap<String, StorageMeteringSnapshot> {
        self.storage
            .iter()
            .map(|s| (s.key().clone(), s.value().snapshot()))
            .collect()
    }

    /// Look up (creating if absent) the storage tracker for `service`.
    pub fn storage_for(
        &self,
        service: &str,
    ) -> dashmap::mapref::one::RefMut<'_, String, StorageMetering> {
        self.storage.entry(service.to_string()).or_default()
    }

    /// Iterate the storage trackers — useful for the report builder.
    pub fn iter_storage(&self) -> Vec<(String, StorageMeteringSnapshot)> {
        self.storage
            .iter()
            .map(|s| (s.key().clone(), s.value().snapshot()))
            .collect()
    }

    /// Snapshot the compute trackers, keyed by service signing name.
    pub fn snapshot_compute(&self) -> HashMap<String, ComputeMeteringSnapshot> {
        self.compute
            .iter()
            .map(|c| (c.key().clone(), c.value().snapshot()))
            .collect()
    }

    /// Look up (creating if absent) the compute tracker for `service`.
    pub fn compute_for(
        &self,
        service: &str,
    ) -> dashmap::mapref::one::RefMut<'_, String, ComputeMetering> {
        self.compute.entry(service.to_string()).or_default()
    }

    pub fn iter_compute(&self) -> Vec<(String, ComputeMeteringSnapshot)> {
        self.compute
            .iter()
            .map(|c| (c.key().clone(), c.value().snapshot()))
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
    #[serde(default)]
    pub storage: HashMap<String, StorageMeteringSnapshot>,
    #[serde(default)]
    pub compute: HashMap<String, ComputeMeteringSnapshot>,
}

impl Snapshottable for BillingState {
    type Snapshot = BillingSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        BillingSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            started_at: self.started_at.load(Ordering::Relaxed),
            services: self.snapshot_services(),
            storage: self.snapshot_storage(),
            compute: self.snapshot_compute(),
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
        let storage: DashMap<String, StorageMetering> = DashMap::new();
        for (svc_name, st) in snap.storage {
            storage.insert(svc_name, StorageMetering::from_snapshot(st));
        }
        let compute: DashMap<String, ComputeMetering> = DashMap::new();
        for (svc_name, c) in snap.compute {
            compute.insert(svc_name, ComputeMetering::from_snapshot(c));
        }
        (
            snap.account_id,
            snap.region,
            BillingState {
                started_at: AtomicU64::new(snap.started_at),
                services,
                storage,
                compute,
            },
        )
    }
}

pub type BillingStateStore = awsim_core::AccountRegionStore<BillingState>;
