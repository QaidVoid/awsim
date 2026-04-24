use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;

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
    /// Origin request policy ID → OriginRequestPolicy
    pub origin_request_policies: DashMap<String, OriginRequestPolicy>,
    /// Key group ID → KeyGroup
    pub key_groups: DashMap<String, KeyGroup>,
    /// Public key ID → PublicKey
    pub public_keys: DashMap<String, PublicKey>,
    /// Field level encryption config ID → FieldLevelEncryptionConfig
    pub field_level_encryption_configs: DashMap<String, FieldLevelEncryptionConfig>,
    /// Real-time log config ARN → RealtimeLogConfig
    pub realtime_log_configs: DashMap<String, RealtimeLogConfig>,
    /// Function name → CloudFrontFunction
    pub functions: DashMap<String, CloudFrontFunction>,
}

#[derive(Debug, Clone)]
pub struct OriginRequestPolicy {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub created_at: String,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct KeyGroup {
    pub id: String,
    pub name: String,
    pub items: Vec<String>,
    pub comment: String,
    pub created_at: String,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct PublicKey {
    pub id: String,
    pub name: String,
    pub encoded_key: String,
    pub caller_reference: String,
    pub comment: String,
    pub created_at: String,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct FieldLevelEncryptionConfig {
    pub id: String,
    pub comment: String,
    pub caller_reference: String,
    pub created_at: String,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct RealtimeLogConfig {
    pub arn: String,
    pub name: String,
    pub sampling_rate: i64,
    pub fields: Vec<String>,
    pub end_points: Value,
}

#[derive(Debug, Clone)]
pub struct CloudFrontFunction {
    pub name: String,
    pub stage: String,
    pub comment: String,
    pub runtime: String,
    pub created_at: String,
    pub etag: String,
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
