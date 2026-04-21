use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// An alias target for a DNS record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasTarget {
    pub dns_name: String,
    pub evaluate_target_health: bool,
    pub hosted_zone_id: String,
}

/// A single DNS resource record set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRecordSet {
    pub name: String,
    pub r#type: String,
    pub ttl: Option<u64>,
    pub resource_records: Vec<String>,
    pub alias_target: Option<AliasTarget>,
}

/// A Route53 hosted zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedZone {
    /// Full ID: `/hostedzone/{uuid}`
    pub id: String,
    /// Zone name always ends with `.`
    pub name: String,
    pub caller_reference: String,
    pub record_sets: Vec<ResourceRecordSet>,
    pub tags: HashMap<String, String>,
    pub created_at: String,
}

/// A Route53 health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub id: String,
    pub config: serde_json::Value,
    pub health_check_version: u64,
}

/// Per-account Route53 state (global — Route53 is not region-scoped).
#[derive(Debug, Default)]
pub struct Route53State {
    /// Hosted zone ID → HostedZone
    pub hosted_zones: DashMap<String, HostedZone>,
    /// Health check ID → HealthCheck
    pub health_checks: DashMap<String, HealthCheck>,
}
