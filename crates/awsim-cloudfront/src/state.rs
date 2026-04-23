use dashmap::DashMap;
use std::collections::HashMap;
use serde_json::Value;

/// CloudFront state — global per account (region-independent).
#[derive(Debug, Default)]
pub struct CloudFrontState {
    pub distributions: DashMap<String, Distribution>,
    pub origin_access_controls: DashMap<String, OriginAccessControl>,
    /// Legacy OAIs (CloudFront Origin Access Identities)
    pub oais: DashMap<String, OriginAccessIdentity>,
    /// Invalidation ID → Invalidation
    pub invalidations: DashMap<String, Invalidation>,
    /// Cache policy ID → CachePolicy
    pub cache_policies: DashMap<String, CachePolicy>,
}

#[derive(Debug, Clone)]
pub struct Distribution {
    pub id: String,
    pub arn: String,
    pub domain_name: String,
    pub status: String,
    pub config: DistributionConfig,
    pub created_at: String,
    pub tags: HashMap<String, String>,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct DistributionConfig {
    pub origins: Vec<Origin>,
    pub default_cache_behavior: Value,
    pub comment: String,
    pub enabled: bool,
    pub price_class: String,
    pub http_version: String,
    pub is_ipv6_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Origin {
    pub id: String,
    pub domain_name: String,
    pub s3_origin_config: Option<Value>,
    pub custom_origin_config: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct OriginAccessControl {
    pub id: String,
    pub name: String,
    pub description: String,
    pub signing_protocol: String,
    pub signing_behavior: String,
    pub origin_access_control_origin_type: String,
    pub created_at: String,
}

/// Legacy CloudFront Origin Access Identity (OAI).
#[derive(Debug, Clone)]
pub struct OriginAccessIdentity {
    pub id: String,
    pub s3_canonical_user_id: String,
    pub comment: String,
    pub caller_reference: String,
    pub created_at: String,
}

/// A CloudFront invalidation.
#[derive(Debug, Clone)]
pub struct Invalidation {
    pub id: String,
    pub distribution_id: String,
    pub status: String,
    pub create_time: String,
    pub paths: Vec<String>,
    pub caller_reference: String,
}

/// A CloudFront cache policy.
#[derive(Debug, Clone)]
pub struct CachePolicy {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub default_ttl: u64,
    pub max_ttl: u64,
    pub min_ttl: u64,
    pub created_at: String,
    pub etag: String,
}
