use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::pricing::PricingCatalog;
use crate::state::{BillingStateStore, OpCounterSnapshot};

const SECONDS_PER_MONTH: f64 = 30.0 * 24.0 * 60.0 * 60.0;
const BYTES_PER_GB: f64 = 1_073_741_824.0;

#[derive(Debug, Serialize)]
pub struct BillingReport {
    pub currency: String,
    /// Wall-clock seconds the meter has been running (max across all
    /// per-account buckets). Used to project a monthly rate.
    pub elapsed_secs: u64,
    /// Cost incurred in `elapsed_secs`.
    pub running_cost_usd: f64,
    /// Linear projection: running_cost_usd / elapsed_secs * SECONDS_PER_MONTH.
    pub projected_monthly_cost_usd: f64,
    /// Per-service breakdown.
    pub services: Vec<ServiceCost>,
}

#[derive(Debug, Serialize)]
pub struct ServiceCost {
    pub service: String,
    pub display_name: String,
    pub region: String,
    pub total_cost_usd: f64,
    pub request_count: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub error_count: u64,
    pub data_transfer_out_cost_usd: f64,
    /// Cost from ingest-billed services charging $/GB on bytes_in
    /// (Firehose, CloudWatch Logs etc.). Zero for services that bill
    /// per-request only.
    pub data_ingest_cost_usd: f64,
    /// Accumulated point-in-time storage cost (S3 / DDB / Lambda code).
    /// Sampled by a periodic poll loop; zero for services that don't
    /// track at-rest storage.
    pub storage_cost_usd: f64,
    /// Most recent sampled storage size in bytes — surfaced for the
    /// dashboard's "currently storing X" line.
    pub storage_bytes: u64,
    /// Accumulated compute cost (Lambda GB-seconds × rate).
    pub compute_cost_usd: f64,
    /// Total GB-seconds of compute consumed.
    pub compute_gb_seconds: f64,
    pub dimensions: Vec<DimensionCost>,
}

#[derive(Debug, Serialize)]
pub struct DimensionCost {
    pub description: String,
    pub price_per_request: f64,
    pub request_count: u64,
    pub cost_usd: f64,
}

pub fn compute_report(store: &BillingStateStore, catalog: &PricingCatalog) -> BillingReport {
    // Aggregate (service -> (op -> counter)) across every (account,
    // region) bucket. Multi-account/region accumulation is fine for the
    // dashboard total — attribution is a follow-up.
    let mut aggregate: BTreeMap<String, BTreeMap<String, OpCounterSnapshot>> = BTreeMap::new();
    // Storage aggregation: sum cost (micros) + take the max sampled
    // bytes across buckets (so the displayed "currently storing X"
    // matches the largest current footprint, not a stale snapshot).
    let mut storage_agg: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    // Same shape as storage: (cost picos, gb_microseconds).
    let mut compute_agg: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut earliest_start: u64 = 0;

    for ((_acct, _region), state) in store.iter_all() {
        let started = state.started_at.load(std::sync::atomic::Ordering::Relaxed);
        if started > 0 && (earliest_start == 0 || started < earliest_start) {
            earliest_start = started;
        }
        for (svc, ops) in state.snapshot_services() {
            let svc_bucket = aggregate.entry(svc).or_default();
            for (op, snap) in ops {
                let entry = svc_bucket.entry(op).or_default();
                entry.count += snap.count;
                entry.bytes_in += snap.bytes_in;
                entry.bytes_out += snap.bytes_out;
                entry.error_count += snap.error_count;
            }
        }
        for (svc, st) in state.iter_storage() {
            let entry = storage_agg.entry(svc).or_default();
            entry.0 += st.accumulated_cost_picos;
            entry.1 = entry.1.max(st.last_sample_bytes);
        }
        for (svc, c) in state.iter_compute() {
            let entry = compute_agg.entry(svc).or_default();
            entry.0 += c.accumulated_cost_picos;
            entry.1 += c.gb_microseconds;
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let elapsed_secs = if earliest_start == 0 || now < earliest_start {
        0
    } else {
        now - earliest_start
    };

    let mut services_out: Vec<ServiceCost> = Vec::new();
    let mut total_cost = 0.0;

    for (svc_name, ops) in aggregate {
        let pricing = catalog.get(&svc_name);
        let display_name = pricing
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| svc_name.clone());
        let region = pricing
            .map(|p| p.region.clone())
            .unwrap_or_else(|| "us-east-1".to_string());

        // Bucket op counts under the dimension they belong to. A
        // dimension shows up in the report even if zero requests hit
        // it, so dashboards stay stable as new ops trickle in.
        let mut dim_buckets: Vec<DimensionCost> = pricing
            .map(|p| {
                p.request_dimensions
                    .iter()
                    .map(|d| DimensionCost {
                        description: d.description.clone(),
                        price_per_request: d.price_per_request,
                        request_count: 0,
                        cost_usd: 0.0,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut other = DimensionCost {
            description: "Other / unmatched operations".to_string(),
            price_per_request: pricing.and_then(|p| p.default_request_rate).unwrap_or(0.0),
            request_count: 0,
            cost_usd: 0.0,
        };

        let mut svc_request_count = 0;
        let mut svc_bytes_in = 0;
        let mut svc_bytes_out = 0;
        let mut svc_error_count = 0;
        let mut svc_request_cost = 0.0;

        for (op_name, snap) in ops {
            svc_request_count += snap.count;
            svc_bytes_in += snap.bytes_in;
            svc_bytes_out += snap.bytes_out;
            svc_error_count += snap.error_count;

            let mut matched = false;
            if let Some(p) = pricing {
                for (idx, dim) in p.request_dimensions.iter().enumerate() {
                    if dim.operations.iter().any(|o| o == &op_name) {
                        let cost = snap.count as f64 * dim.price_per_request;
                        dim_buckets[idx].request_count += snap.count;
                        dim_buckets[idx].cost_usd += cost;
                        svc_request_cost += cost;
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                let cost = snap.count as f64 * other.price_per_request;
                other.request_count += snap.count;
                other.cost_usd += cost;
                svc_request_cost += cost;
            }
        }

        // Only surface the "unmatched" bucket when something actually
        // landed in it — otherwise it's distracting noise on a fresh
        // dashboard.
        if other.request_count > 0 {
            dim_buckets.push(other);
        }

        let transfer_rate = pricing
            .and_then(|p| p.data_transfer_out_per_gb)
            .unwrap_or(0.0);
        let transfer_cost = (svc_bytes_out as f64 / BYTES_PER_GB) * transfer_rate;
        let ingest_rate = pricing.and_then(|p| p.data_ingest_per_gb).unwrap_or(0.0);
        let ingest_cost = (svc_bytes_in as f64 / BYTES_PER_GB) * ingest_rate;
        let (storage_cost_usd, storage_bytes) = storage_agg
            .remove(&svc_name)
            .map(|(picos, bytes)| (picos as f64 / 1e12, bytes))
            .unwrap_or((0.0, 0));
        let (compute_cost_usd, compute_gb_seconds) = compute_agg
            .remove(&svc_name)
            .map(|(picos, gb_us)| (picos as f64 / 1e12, gb_us as f64 / 1e6))
            .unwrap_or((0.0, 0.0));
        let svc_total =
            svc_request_cost + transfer_cost + ingest_cost + storage_cost_usd + compute_cost_usd;
        total_cost += svc_total;

        services_out.push(ServiceCost {
            service: svc_name,
            display_name,
            region,
            total_cost_usd: svc_total,
            request_count: svc_request_count,
            bytes_in: svc_bytes_in,
            bytes_out: svc_bytes_out,
            error_count: svc_error_count,
            data_transfer_out_cost_usd: transfer_cost,
            data_ingest_cost_usd: ingest_cost,
            storage_cost_usd,
            storage_bytes,
            compute_cost_usd,
            compute_gb_seconds,
            dimensions: dim_buckets,
        });
    }

    // Storage- or compute-only services — anything left in storage_agg
    // or compute_agg that didn't match a service in the request
    // aggregate. A bucket that's been sitting idle but storing data
    // still costs money; a Lambda that ran once a long time ago and
    // hasn't been invoked recently still has accumulated GB-seconds.
    //
    // Skip services whose tracker is fully zero — the storage poll
    // creates an entry on first call even when bytes are 0 (e.g. an
    // empty ECR registry), which would otherwise surface as a
    // ghost row on the dashboard.
    let leftover_keys: std::collections::BTreeSet<String> = storage_agg
        .iter()
        .filter(|(_, (picos, bytes))| *picos > 0 || *bytes > 0)
        .map(|(k, _)| k.clone())
        .chain(
            compute_agg
                .iter()
                .filter(|(_, (picos, gb_us))| *picos > 0 || *gb_us > 0)
                .map(|(k, _)| k.clone()),
        )
        .collect();
    for svc_name in leftover_keys {
        let pricing = catalog.get(&svc_name);
        let display_name = pricing
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| svc_name.clone());
        let region = pricing
            .map(|p| p.region.clone())
            .unwrap_or_else(|| "us-east-1".to_string());
        let (storage_cost_usd, storage_bytes) = storage_agg
            .remove(&svc_name)
            .map(|(picos, bytes)| (picos as f64 / 1e12, bytes))
            .unwrap_or((0.0, 0));
        let (compute_cost_usd, compute_gb_seconds) = compute_agg
            .remove(&svc_name)
            .map(|(picos, gb_us)| (picos as f64 / 1e12, gb_us as f64 / 1e6))
            .unwrap_or((0.0, 0.0));
        let dim_buckets: Vec<DimensionCost> = pricing
            .map(|p| {
                p.request_dimensions
                    .iter()
                    .map(|d| DimensionCost {
                        description: d.description.clone(),
                        price_per_request: d.price_per_request,
                        request_count: 0,
                        cost_usd: 0.0,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let svc_total = storage_cost_usd + compute_cost_usd;
        total_cost += svc_total;
        services_out.push(ServiceCost {
            service: svc_name,
            display_name,
            region,
            total_cost_usd: svc_total,
            request_count: 0,
            bytes_in: 0,
            bytes_out: 0,
            error_count: 0,
            data_transfer_out_cost_usd: 0.0,
            data_ingest_cost_usd: 0.0,
            storage_cost_usd,
            storage_bytes,
            compute_cost_usd,
            compute_gb_seconds,
            dimensions: dim_buckets,
        });
    }

    services_out.sort_by(|a, b| {
        b.total_cost_usd
            .partial_cmp(&a.total_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let projected = if elapsed_secs > 0 {
        total_cost / elapsed_secs as f64 * SECONDS_PER_MONTH
    } else {
        0.0
    };

    BillingReport {
        currency: "USD".to_string(),
        elapsed_secs,
        running_cost_usd: total_cost,
        projected_monthly_cost_usd: projected,
        services: services_out,
    }
}
