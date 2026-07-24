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
pub use pricing::{MeteredUnits, PricingCatalog, RequestDimension, ServicePricing};
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
            character_count: None,
            read_units: None,
            write_units: None,
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

    /// DynamoDB bills per consumed request unit, not per API call: a
    /// Scan reporting 50 RRU costs 50x the read rate, an eventually
    /// consistent GetItem reporting 0.5 RRU costs half a unit, and a
    /// call that reported nothing - an error, an idempotent replay, a
    /// PROVISIONED-mode table - costs nothing at all.
    #[test]
    fn dynamodb_bills_reported_request_units() {
        let meter = BillingMeter::new();
        let mut scan = evt("dynamodb", "Scan", 0);
        scan.read_units = Some(50.0);
        meter.record(&scan);
        let mut get = evt("dynamodb", "GetItem", 0);
        get.read_units = Some(0.5);
        meter.record(&get);
        // No units reported (throttled request, provisioned table):
        // costs nothing.
        meter.record(&evt("dynamodb", "Query", 0));

        let report = compute_report(&meter.store, &meter.pricing);
        let ddb = report
            .services
            .iter()
            .find(|s| s.service == "dynamodb")
            .expect("dynamodb service in report");

        // (50 + 0.5) read units * $1.25e-7.
        let expected = 50.5 * 1.25e-7;
        let diff = (ddb.total_cost_usd - expected).abs();
        assert!(
            diff < 1e-12,
            "expected ${expected}, got ${}",
            ddb.total_cost_usd
        );
        // request_count still reflects API calls, not units.
        assert_eq!(ddb.request_count, 3);
    }

    /// A PartiQL transaction consumes both axes in one call; each
    /// metered dimension bills only its own axis, so the cost is the
    /// sum of read units at the read rate and write units at the
    /// write rate.
    #[test]
    fn dynamodb_partiql_bills_both_axes() {
        let meter = BillingMeter::new();
        let mut tx = evt("dynamodb", "ExecuteTransaction", 0);
        tx.read_units = Some(4.0);
        tx.write_units = Some(6.0);
        meter.record(&tx);

        let report = compute_report(&meter.store, &meter.pricing);
        let ddb = report
            .services
            .iter()
            .find(|s| s.service == "dynamodb")
            .expect("dynamodb service in report");
        let expected = 4.0 * 1.25e-7 + 6.0 * 6.25e-7;
        let diff = (ddb.total_cost_usd - expected).abs();
        assert!(
            diff < 1e-12,
            "expected ${expected}, got ${}",
            ddb.total_cost_usd
        );
        // The one call is counted once, not once per dimension.
        let counted: u64 = ddb.dimensions.iter().map(|d| d.request_count).sum();
        assert_eq!(counted, 1);
    }

    /// DynamoDB Streams reads bill per GetRecords call at the AWS
    /// streams rate - the one count-based data-plane dimension.
    #[test]
    fn dynamodb_streams_reads_bill_per_call() {
        let meter = BillingMeter::new();
        for _ in 0..100_000 {
            meter.record(&evt("dynamodb", "GetRecords", 0));
        }
        let report = compute_report(&meter.store, &meter.pricing);
        let ddb = report
            .services
            .iter()
            .find(|s| s.service == "dynamodb")
            .expect("dynamodb service in report");
        // $0.02 per 100k streams read requests.
        let diff = (ddb.total_cost_usd - 0.02).abs();
        assert!(diff < 1e-9, "expected $0.02, got ${}", ddb.total_cost_usd);
    }

    /// PROVISIONED tables accrue capacity-hour cost from sampled
    /// RCU/WCU totals, independent of request traffic.
    #[test]
    fn dynamodb_provisioned_capacity_accrues_hourly_cost() {
        use crate::state::CapacityMetering;
        let m = CapacityMetering::default();
        // Rates straight from the embedded catalog.
        let meter = BillingMeter::new();
        let p = meter.pricing.get("dynamodb").expect("dynamodb pricing");
        let rcu_rate = p.provisioned_rcu_per_hour.expect("rcu rate") / 3600.0;
        let wcu_rate = p.provisioned_wcu_per_hour.expect("wcu rate") / 3600.0;
        // 100 RCU + 10 WCU held steady for one hour.
        m.record_sample(100, 10, 1_000, rcu_rate, wcu_rate);
        m.record_sample(100, 10, 4_600, rcu_rate, wcu_rate);
        let cost = m.accumulated_cost_picos.load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1e12;
        // 100 * $0.00013 + 10 * $0.00065 = $0.0195 per hour.
        let expected = 100.0 * 0.00013 + 10.0 * 0.00065;
        let diff = (cost - expected).abs();
        assert!(diff < 1e-9, "expected ${expected}, got ${cost}");
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
