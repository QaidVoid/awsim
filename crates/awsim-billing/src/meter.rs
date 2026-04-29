use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestEvent, RequestEventBus};
use tokio::sync::broadcast::error::RecvError;

use crate::pricing::PricingCatalog;
use crate::state::BillingStateStore;

const SECONDS_PER_MONTH: f64 = 30.0 * 24.0 * 60.0 * 60.0;
const BYTES_PER_GB: f64 = 1_073_741_824.0;

/// Fallback Lambda function memory (128 MB) — AWS's lowest tier
/// and the default for new functions. Used when the responding
/// service didn't attach an `X-Awsim-Memory-MB` header (older
/// services / non-Lambda compute paths).
const LAMBDA_DEFAULT_MEMORY_MB: u64 = 128;
const MB_PER_GB: f64 = 1024.0;

/// Operations whose request_event.duration_ms should be billed as
/// compute time when the service has a `compute_per_gb_second` rate.
/// Currently Lambda-only; if we ever add Fargate-style compute
/// pricing this list grows.
const COMPUTE_BILLED_OPS: &[&str] = &["Invoke", "InvokeAsync", "InvokeWithResponseStream"];

/// Holds the in-memory billing store + the pricing catalog. Cheaply
/// cloned (Arcs inside).
#[derive(Clone)]
pub struct BillingMeter {
    pub store: BillingStateStore,
    pub pricing: Arc<PricingCatalog>,
}

impl BillingMeter {
    pub fn new() -> Self {
        Self {
            store: BillingStateStore::new(),
            pricing: Arc::new(PricingCatalog::embedded()),
        }
    }

    /// Record a point-in-time storage sample for a metered service.
    /// Cost since the last sample accrues into the per-service
    /// `StorageMetering` bucket; nothing happens if the service has
    /// no `storage_per_gb_month` rate in the pricing catalogue.
    pub fn record_storage_sample(
        &self,
        service: &str,
        account_id: &str,
        region: &str,
        current_bytes: u64,
    ) {
        let Some(pricing) = self.pricing.get(service) else {
            return;
        };
        let Some(per_gb_month) = pricing.storage_per_gb_month else {
            return;
        };
        let per_byte_per_sec = per_gb_month / BYTES_PER_GB / SECONDS_PER_MONTH;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let state = self.store.get(account_id, region);
        state.ensure_started(now);
        let storage = state.storage_for(service);
        storage.record_sample(current_bytes, now, per_byte_per_sec);
    }

    /// Apply a single request event to the relevant per-(account, region)
    /// bucket. Events without an operation name (raw / unparseable
    /// requests) are skipped — they're useless for cost attribution.
    pub fn record(&self, event: &RequestEvent) {
        let Some(operation) = event.operation.as_deref() else {
            return;
        };
        // Only meter services we have pricing for; otherwise the bucket
        // would grow unbounded with services nobody can attribute cost
        // to anyway.
        let Some(pricing) = self.pricing.get(&event.service) else {
            return;
        };
        let state = self.store.get(&event.account_id, &event.region);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        state.ensure_started(now);
        // Step Functions bills per state transition; the SFN service
        // emits the transition count via X-Awsim-State-Transitions on
        // the response, which the gateway pulls into event.state_transitions.
        // For everything else, one request = one billable unit.
        let units = event.state_transitions.map(|n| n as u64).unwrap_or(1);
        state.record(
            &event.service,
            operation,
            units,
            event.request_size,
            event.response_size,
            event.error_code.is_some(),
        );

        // Compute billing — Lambda's GB-second axis. We only accrue
        // for compute-billed ops on services with a published rate.
        // Memory comes from the responder's X-Awsim-Memory-MB header
        // (Lambda populates this with the function's configured
        // memory); falls back to 128 MB when absent.
        if let Some(rate) = pricing.compute_per_gb_second
            && COMPUTE_BILLED_OPS.contains(&operation)
            && event.duration_ms > 0.0
        {
            let memory_mb = event
                .memory_mb
                .map(|m| m as u64)
                .unwrap_or(LAMBDA_DEFAULT_MEMORY_MB);
            let memory_gb = memory_mb as f64 / MB_PER_GB;
            let gb_seconds = (event.duration_ms / 1000.0) * memory_gb;
            state.compute_for(&event.service).record(gb_seconds, rate);
        }
    }
}

impl Default for BillingMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn the background task that drains `RequestEvent`s into the meter.
///
/// The receiver is a tokio broadcast channel — if the meter falls behind
/// (256-deep buffer per the gateway constructor) we'll see `Lagged`
/// errors. We log + skip rather than block, so a slow billing path can
/// never throttle the request gateway.
pub fn spawn_meter(meter: BillingMeter, events: &RequestEventBus) {
    let mut rx = events.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => meter.record(&event),
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Billing meter lagged behind event stream");
                }
                Err(RecvError::Closed) => {
                    tracing::info!("Request event bus closed; billing meter exiting");
                    break;
                }
            }
        }
    });
}
