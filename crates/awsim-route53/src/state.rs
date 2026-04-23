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

/// A Route53 query logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLoggingConfig {
    pub id: String,
    pub hosted_zone_id: String,
    pub cloud_watch_logs_log_group_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficPolicy {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub document: String,
    pub comment: Option<String>,
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationSet {
    pub id: String,
    pub caller_reference: String,
    pub name_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcAssociation {
    pub vpc_id: String,
    pub vpc_region: String,
    pub hosted_zone_id: String,
}

#[derive(Debug, Default)]
pub struct Route53State {
    pub hosted_zones: DashMap<String, HostedZone>,
    pub health_checks: DashMap<String, HealthCheck>,
    pub query_logging_configs: DashMap<String, QueryLoggingConfig>,
    pub traffic_policies: DashMap<String, TrafficPolicy>,
    pub delegation_sets: DashMap<String, DelegationSet>,
    pub vpc_associations: DashMap<String, Vec<VpcAssociation>>,
}
