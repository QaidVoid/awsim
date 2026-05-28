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
    /// `true` for VPC-scoped private zones. Public zones default to
    /// `false`. AWS requires at least one VPC at CreateHostedZone time
    /// when this flips on.
    #[serde(default)]
    pub private_zone: bool,
    /// VPC associations for private zones. Each entry is
    /// `{ VPCId, VPCRegion }`.
    #[serde(default)]
    pub vpcs: Vec<serde_json::Value>,
    /// HostedZoneConfig.Comment (free-form description).
    #[serde(default)]
    pub comment: Option<String>,
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
    /// Change submissions — map of bare change id (no `/change/`
    /// prefix) -> unix-epoch seconds when submitted. GetChange uses
    /// the elapsed time to walk `PENDING` -> `INSYNC` after a short
    /// propagation window.
    pub change_submissions: DashMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Route53StateSnapshot {
    #[serde(default)]
    pub hosted_zones: Vec<HostedZone>,
    #[serde(default)]
    pub health_checks: Vec<HealthCheck>,
    #[serde(default)]
    pub query_logging_configs: Vec<QueryLoggingConfig>,
    #[serde(default)]
    pub traffic_policies: Vec<TrafficPolicy>,
    #[serde(default)]
    pub delegation_sets: Vec<DelegationSet>,
    #[serde(default)]
    pub vpc_associations: Vec<(String, Vec<VpcAssociation>)>,
    #[serde(default)]
    pub change_submissions: Vec<(String, u64)>,
}

impl Route53State {
    pub fn to_snapshot(&self) -> Route53StateSnapshot {
        Route53StateSnapshot {
            hosted_zones: self
                .hosted_zones
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            health_checks: self
                .health_checks
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            query_logging_configs: self
                .query_logging_configs
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            traffic_policies: self
                .traffic_policies
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            delegation_sets: self
                .delegation_sets
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            vpc_associations: self
                .vpc_associations
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
            change_submissions: self
                .change_submissions
                .iter()
                .map(|e| (e.key().clone(), *e.value()))
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: Route53StateSnapshot) {
        self.hosted_zones.clear();
        for z in snap.hosted_zones {
            self.hosted_zones.insert(z.id.clone(), z);
        }
        self.health_checks.clear();
        for h in snap.health_checks {
            self.health_checks.insert(h.id.clone(), h);
        }
        self.query_logging_configs.clear();
        for q in snap.query_logging_configs {
            self.query_logging_configs.insert(q.id.clone(), q);
        }
        self.traffic_policies.clear();
        for t in snap.traffic_policies {
            self.traffic_policies.insert(t.id.clone(), t);
        }
        self.delegation_sets.clear();
        for d in snap.delegation_sets {
            self.delegation_sets.insert(d.id.clone(), d);
        }
        self.vpc_associations.clear();
        for (k, v) in snap.vpc_associations {
            self.vpc_associations.insert(k, v);
        }
        self.change_submissions.clear();
        for (k, v) in snap.change_submissions {
            self.change_submissions.insert(k, v);
        }
    }
}
