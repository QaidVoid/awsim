use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestEvent, RequestEventBus};
use tokio::sync::broadcast::error::RecvError;

use crate::pricing::PricingCatalog;
use crate::state::BillingStateStore;

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
        if self.pricing.get(&event.service).is_none() {
            return;
        }
        let state = self.store.get(&event.account_id, &event.region);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        state.ensure_started(now);
        state.record(
            &event.service,
            operation,
            event.request_size,
            event.response_size,
            event.error_code.is_some(),
        );
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
