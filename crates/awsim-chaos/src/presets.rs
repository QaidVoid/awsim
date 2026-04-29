//! Built-in chaos rule bundles. Each preset returns a fresh
//! `Vec<ChaosRule>` (with new UUIDs) that can be appended to the
//! engine to simulate a common AWS failure mode.

use crate::rule::{
    ChaosEffect, ChaosRule, ErrorEffect, LatencyEffect, OperationMatch, ServiceMatch,
};

/// Metadata describing one preset — name + description for the UI
/// and CLI listing. The `build` fn produces the rules.
#[derive(Debug, Clone, Copy)]
pub struct PresetInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// All known presets. Add new entries here and the admin API +
/// CLI listing pick them up automatically.
pub const PRESETS: &[PresetInfo] = &[
    PresetInfo {
        name: "flaky-s3",
        description: "5% of S3 requests return 503 SlowDown — exercises retry/backoff.",
    },
    PresetInfo {
        name: "ddb-throttle",
        description: "10% of DynamoDB requests return ProvisionedThroughputExceededException (400).",
    },
    PresetInfo {
        name: "slow-lambda",
        description: "All Lambda Invoke calls get +500-2000ms latency — models cold-start spikes.",
    },
    PresetInfo {
        name: "kms-outage",
        description: "Every KMS call returns 503 KMSInternalException — total KMS outage.",
    },
    PresetInfo {
        name: "regional-failover",
        description: "50% of all calls return 503 ServiceUnavailable — partial regional outage.",
    },
    PresetInfo {
        name: "network-jitter",
        description: "Every call gets +50-300ms latency — slow link / cross-region call.",
    },
];

/// Build the rules for a named preset. Returns `None` for unknown
/// names so callers can return 404 / a friendly CLI error.
pub fn build(name: &str) -> Option<Vec<ChaosRule>> {
    let rules = match name {
        "flaky-s3" => vec![rule(
            ServiceMatch::Exact("s3".into()),
            OperationMatch::Any,
            0.05,
            ChaosEffect::Error(error(503, "SlowDown", "Please reduce your request rate.")),
            "preset: flaky-s3",
        )],
        "ddb-throttle" => vec![rule(
            ServiceMatch::Exact("dynamodb".into()),
            OperationMatch::Any,
            0.10,
            ChaosEffect::Error(error(
                400,
                "ProvisionedThroughputExceededException",
                "The level of configured provisioned throughput for the table was exceeded.",
            )),
            "preset: ddb-throttle",
        )],
        "slow-lambda" => vec![rule(
            ServiceMatch::Exact("lambda".into()),
            OperationMatch::Exact("Invoke".into()),
            1.0,
            ChaosEffect::Latency(LatencyEffect {
                min_ms: 500,
                max_ms: 2000,
            }),
            "preset: slow-lambda",
        )],
        "kms-outage" => vec![rule(
            ServiceMatch::Exact("kms".into()),
            OperationMatch::Any,
            1.0,
            ChaosEffect::Error(error(
                503,
                "KMSInternalException",
                "The system timed out while trying to fulfill the request.",
            )),
            "preset: kms-outage",
        )],
        "regional-failover" => vec![rule(
            ServiceMatch::Any,
            OperationMatch::Any,
            0.5,
            ChaosEffect::Error(error(
                503,
                "ServiceUnavailable",
                "Service is unable to handle request — failover suggested.",
            )),
            "preset: regional-failover",
        )],
        "network-jitter" => vec![rule(
            ServiceMatch::Any,
            OperationMatch::Any,
            1.0,
            ChaosEffect::Latency(LatencyEffect {
                min_ms: 50,
                max_ms: 300,
            }),
            "preset: network-jitter",
        )],
        _ => return None,
    };
    Some(rules)
}

fn rule(
    service: ServiceMatch,
    operation: OperationMatch,
    probability: f64,
    effect: ChaosEffect,
    label: &str,
) -> ChaosRule {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ChaosRule {
        id: uuid::Uuid::new_v4().to_string(),
        service,
        operation,
        probability,
        effect,
        enabled: true,
        label: Some(label.to_string()),
        created_at: now,
        injection_count: 0,
    }
}

fn error(status: u16, code: &str, message: &str) -> ErrorEffect {
    ErrorEffect {
        status,
        code: code.to_string(),
        message: message.to_string(),
        retry_after_secs: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_builds() {
        for info in PRESETS {
            let rules = build(info.name).expect("preset should build");
            assert!(!rules.is_empty(), "{} produced no rules", info.name);
            for r in &rules {
                assert!(!r.id.is_empty());
                assert!(r.probability > 0.0);
                assert!(r.label.is_some());
            }
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(build("does-not-exist").is_none());
    }

    #[test]
    fn each_preset_has_unique_rule_ids() {
        let a = build("flaky-s3").unwrap();
        let b = build("flaky-s3").unwrap();
        assert_ne!(a[0].id, b[0].id, "ids should be freshly generated");
    }
}
