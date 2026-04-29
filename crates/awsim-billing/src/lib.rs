//! Usage metering + pricing for AWSim.
//!
//! Subscribes to the gateway's `RequestEvent` stream and tallies per-service,
//! per-operation request counts and bytes-out. Multiplies the tallies by an
//! embedded snapshot of canonical AWS pricing (us-east-1) to produce an
//! estimated monthly cost surfaced via `/_awsim/billing`.
//!
//! This is intentionally a coarse approximation:
//!   * Operations not listed in a service's pricing dimensions fall into a
//!     bucket priced at the service's default request rate (or zero if none).
//!   * Storage / GB-month dimensions aren't sampled in the request stream;
//!     they need a separate poll over service state and aren't yet wired.
//!   * Region is locked to us-east-1 — AWSim doesn't model multi-region
//!     billing.

pub mod meter;
pub mod pricing;
pub mod report;
pub mod state;

pub use meter::{BillingMeter, spawn_meter};
pub use pricing::{PricingCatalog, RequestDimension, ServicePricing};
pub use report::{BillingReport, ServiceCost, compute_report};
pub use state::{BillingState, BillingStateStore, OpCounter};

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::RequestEvent;

    fn evt(service: &str, op: &str, size_out: u64) -> RequestEvent {
        RequestEvent {
            id: "r".into(),
            ts: 0.0,
            method: "POST".into(),
            path: "/".into(),
            service: service.into(),
            operation: Some(op.into()),
            account_id: "000000000000".into(),
            region: "us-east-1".into(),
            principal_arn: None,
            status_code: 200,
            duration_ms: 1.0,
            request_size: 0,
            response_size: size_out,
            error_code: None,
            memory_mb: None,
            state_transitions: None,
        }
    }

    #[test]
    fn s3_put_then_get_costs_match_pricing() {
        let meter = BillingMeter::new();
        // 10,000 PutObject + 100,000 GetObject + 1 GiB outbound on the GET path.
        for _ in 0..10_000 {
            meter.record(&evt("s3", "PutObject", 0));
        }
        for _ in 0..99_999 {
            meter.record(&evt("s3", "GetObject", 0));
        }
        meter.record(&evt("s3", "GetObject", 1_073_741_824));

        let report = compute_report(&meter.store, &meter.pricing);
        let s3 = report
            .services
            .iter()
            .find(|s| s.service == "s3")
            .expect("s3 service in report");

        // 10k PUTs * $5e-6  = $0.05
        // 100k GETs * $4e-7 = $0.04
        // 1 GiB out * $0.09 = $0.09
        // total            = $0.18
        let expected = 0.05 + 0.04 + 0.09;
        let diff = (s3.total_cost_usd - expected).abs();
        assert!(
            diff < 1e-6,
            "expected ${expected}, got ${}",
            s3.total_cost_usd
        );
    }

    #[test]
    fn unmetered_service_is_not_recorded() {
        // IAM has no embedded pricing (and won't, until we wire control-plane
        // services). Switch to whatever's missing if we ever add it.
        let meter = BillingMeter::new();
        meter.record(&evt("iam", "GetUser", 0));
        let report = compute_report(&meter.store, &meter.pricing);
        assert!(report.services.iter().all(|s| s.service != "iam"));
    }
}
