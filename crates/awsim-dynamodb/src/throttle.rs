//! Per-table read / write throughput enforcement for `BillingMode =
//! PROVISIONED`.
//!
//! Modelled as a token bucket per (table, kind). The refill rate
//! matches the table's `ReadCapacityUnits` / `WriteCapacityUnits`,
//! and the bucket's max capacity is `BURST_SECONDS * refill` so a
//! burst-then-quiet workload can spend up to five minutes of unused
//! capacity at full rate (matching real DynamoDB's burst-credit
//! window).
//!
//! Enforcement is post-op: the operation handler runs as normal,
//! computes the actual capacity it consumed (using the standard
//! AWS rounding rules in `operations::{read,write}_capacity_units`),
//! then asks the registry to charge the bucket. Buckets that would
//! go below the requested cost short-circuit with
//! `ProvisionedThroughputExceededException`. Tables on
//! `PAY_PER_REQUEST` are bypassed entirely - that is the supported
//! "no throttling" path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use awsim_core::AwsError;
use dashmap::DashMap;

/// How many seconds of unused capacity a table can accumulate as
/// burst credits. Real DynamoDB documents 300 s; matches that so
/// cost projections taken from a local run line up with prod.
const BURST_SECONDS: f64 = 300.0;

/// Whether a charge is against the read or the write capacity bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BucketKind {
    Read,
    Write,
}

impl BucketKind {
    fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Token-bucket state for a single (table, kind). Snapshots the
/// configured refill rate so a stale entry can be detected when the
/// table's RCU / WCU is updated via `UpdateTable` and rebuilt with
/// the new rate.
struct TokenBucket {
    /// Current credit balance. Capped at `capacity`, may briefly
    /// dip toward zero during a burst.
    tokens: f64,
    /// Maximum credit the bucket can hold (5-minute burst window).
    capacity: f64,
    /// Tokens added per second. Equal to the table's configured
    /// RCU / WCU.
    refill_per_sec: f64,
    /// Last instant tokens were refilled. `consume` advances this.
    last_refill: Instant,
}

impl TokenBucket {
    fn new(refill_per_sec: f64) -> Self {
        let capacity = (refill_per_sec * BURST_SECONDS).max(0.0);
        Self {
            // Start with a full burst window so freshly-created
            // tables can absorb the SDK's bootstrap traffic without
            // throttling on the very first call.
            tokens: capacity,
            capacity,
            refill_per_sec,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    /// Try to consume `units`. Returns the post-deduction balance on
    /// success, or `None` when the bucket can't cover the cost.
    fn try_consume(&mut self, units: f64) -> Option<f64> {
        self.refill();
        if self.tokens + f64::EPSILON >= units {
            self.tokens -= units;
            Some(self.tokens)
        } else {
            None
        }
    }
}

/// Per-table read + write buckets, keyed inside [`ThrottleRegistry`]
/// by table name. Held under one `Mutex` per table so cross-table
/// throttling decisions don't contend, and each bucket's quick
/// `try_consume` lock holds for microseconds.
struct TableBuckets {
    read: Mutex<TokenBucket>,
    write: Mutex<TokenBucket>,
    /// Refill rates the buckets were built with. Any divergence
    /// from the live `Table::read_capacity_units` /
    /// `write_capacity_units` triggers a rebuild on the next
    /// charge so an `UpdateTable` change takes effect immediately.
    configured_read: f64,
    configured_write: f64,
}

/// Registry of every PROVISIONED table's token buckets. Lookups
/// are lock-free via `DashMap`; each bucket has its own per-kind
/// lock for the brief refill + decrement step.
#[derive(Default)]
pub struct ThrottleRegistry {
    inner: DashMap<String, Arc<TableBuckets>>,
}

impl ThrottleRegistry {
    /// Drop the bucket entry for `table_name`. Called from
    /// `DeleteTable` so a freshly-recreated table starts with a
    /// fresh burst window.
    pub fn forget(&self, table_name: &str) {
        self.inner.remove(table_name);
    }

    /// Try to charge `units` against the table's bucket. Returns
    /// `Ok(())` when the request is within the budget,
    /// `Err(ProvisionedThroughputExceededException)` when the
    /// bucket can't cover the cost. Caller is expected to have
    /// already short-circuited PAY_PER_REQUEST tables - this entry
    /// point only sees the PROVISIONED path.
    pub fn enforce(
        &self,
        table_name: &str,
        kind: BucketKind,
        units: f64,
        configured_read: f64,
        configured_write: f64,
    ) -> Result<(), AwsError> {
        let entry = self.get_or_build(table_name, configured_read, configured_write);
        let bucket = match kind {
            BucketKind::Read => &entry.read,
            BucketKind::Write => &entry.write,
        };
        let mut guard = bucket.lock().expect("token bucket mutex poisoned");
        match guard.try_consume(units) {
            Some(_remaining) => Ok(()),
            None => Err(AwsError::bad_request(
                "ProvisionedThroughputExceededException",
                format!(
                    "The level of configured provisioned throughput for the table was exceeded. \
                     Consider increasing your provisioning level with the UpdateTable API. \
                     (table: {table_name}, kind: {})",
                    kind.label()
                ),
            )),
        }
    }

    fn get_or_build(
        &self,
        table_name: &str,
        configured_read: f64,
        configured_write: f64,
    ) -> Arc<TableBuckets> {
        // Fast path: existing entry whose configured rates still
        // match the table spec - reuse it.
        if let Some(existing) = self.inner.get(table_name)
            && rates_match(existing.configured_read, configured_read)
            && rates_match(existing.configured_write, configured_write)
        {
            return Arc::clone(&existing);
        }

        // Either missing or rates changed (UpdateTable). Rebuild.
        // `entry().or_insert_with` would race with the existing
        // entry's old rates, so do a remove-then-insert under the
        // dashmap shard lock by going through `insert`.
        let buckets = Arc::new(TableBuckets {
            read: Mutex::new(TokenBucket::new(configured_read)),
            write: Mutex::new(TokenBucket::new(configured_write)),
            configured_read,
            configured_write,
        });
        self.inner
            .insert(table_name.to_string(), Arc::clone(&buckets));
        buckets
    }
}

fn rates_match(a: f64, b: f64) -> bool {
    (a - b).abs() < f64::EPSILON
}

/// Expose the snapshot of (read, write) configured rates per table.
/// Used by `DescribeTable` admin / debug surfaces; not on the hot
/// path.
#[allow(dead_code)]
pub fn collect_configured_rates(registry: &ThrottleRegistry) -> HashMap<String, (f64, f64)> {
    registry
        .inner
        .iter()
        .map(|kv| {
            (
                kv.key().clone(),
                (kv.value().configured_read, kv.value().configured_write),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_registry() -> ThrottleRegistry {
        ThrottleRegistry::default()
    }

    #[test]
    fn enforce_passes_within_burst_capacity() {
        let r = small_registry();
        // 10 RCU / 10 WCU -> 3000 RCU / 3000 WCU burst capacity.
        for _ in 0..100 {
            r.enforce("t", BucketKind::Read, 1.0, 10.0, 10.0).unwrap();
        }
    }

    #[test]
    fn enforce_rejects_when_burst_exhausted() {
        let r = small_registry();
        // 1 RCU -> 300 burst tokens. Spend them all in one request.
        r.enforce("t", BucketKind::Read, 300.0, 1.0, 1.0).unwrap();
        let err = r.enforce("t", BucketKind::Read, 1.0, 1.0, 1.0).unwrap_err();
        assert_eq!(err.code, "ProvisionedThroughputExceededException");
    }

    #[test]
    fn enforce_rebuilds_on_capacity_change() {
        let r = small_registry();
        // Start at 1 RCU, drain it.
        r.enforce("t", BucketKind::Read, 300.0, 1.0, 1.0).unwrap();
        assert!(r.enforce("t", BucketKind::Read, 1.0, 1.0, 1.0).is_err());
        // UpdateTable raises to 100 RCU -> bucket should be rebuilt
        // with a fresh 30000-token burst window.
        r.enforce("t", BucketKind::Read, 100.0, 100.0, 100.0)
            .unwrap();
    }

    #[test]
    fn forget_resets_state() {
        let r = small_registry();
        r.enforce("t", BucketKind::Read, 300.0, 1.0, 1.0).unwrap();
        assert!(r.enforce("t", BucketKind::Read, 1.0, 1.0, 1.0).is_err());
        r.forget("t");
        // Recreated table starts with a full burst window.
        r.enforce("t", BucketKind::Read, 1.0, 1.0, 1.0).unwrap();
    }

    #[test]
    fn read_and_write_buckets_track_independently() {
        let r = small_registry();
        // Drain the read bucket but leave write alone.
        r.enforce("t", BucketKind::Read, 300.0, 1.0, 1.0).unwrap();
        assert!(r.enforce("t", BucketKind::Read, 1.0, 1.0, 1.0).is_err());
        // Write side untouched.
        r.enforce("t", BucketKind::Write, 1.0, 1.0, 1.0).unwrap();
    }
}
