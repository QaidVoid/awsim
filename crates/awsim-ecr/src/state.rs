use std::collections::HashMap;

use dashmap::DashMap;

/// A single image stored in a repository.
#[derive(Debug, Clone)]
pub struct ContainerImage {
    pub image_digest: String,
    pub image_tag: Option<String>,
    pub image_manifest: String,
    pub pushed_at: String,
    pub image_size_in_bytes: u64,
}

/// A layer upload session.
#[derive(Debug, Clone)]
pub struct LayerUpload {
    #[allow(dead_code)]
    pub upload_id: String,
    #[allow(dead_code)]
    pub repository_name: String,
    pub part_data: Vec<u8>,
}

/// An ECR repository.
#[derive(Debug)]
pub struct Repository {
    pub name: String,
    pub arn: String,
    pub registry_id: String,
    pub repository_uri: String,
    pub images: Vec<ContainerImage>,
    pub created_at: String,
    pub image_tag_mutability: String,
    pub tags: HashMap<String, String>,
    pub lifecycle_policy: Option<String>,
    pub lifecycle_policy_preview: Option<String>,
    pub repository_policy: Option<String>,
    pub scan_on_push: bool,
}

#[derive(Debug, Clone)]
pub struct PullThroughCacheRule {
    pub ecr_repository_prefix: String,
    pub upstream_registry_url: String,
    pub upstream_registry: Option<String>,
    pub credential_arn: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Default, Clone)]
pub struct RegistryScanningConfiguration {
    pub scan_type: String,
    pub rules: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Clone)]
pub struct ReplicationConfiguration {
    pub rules: Vec<serde_json::Value>,
}

/// Per-account/region ECR state.
#[derive(Debug, Default)]
pub struct EcrState {
    /// repositoryName → Repository
    pub repositories: DashMap<String, Repository>,
    /// uploadId → LayerUpload (in-progress layer uploads)
    pub layer_uploads: DashMap<String, LayerUpload>,
    pub pull_through_cache_rules: DashMap<String, PullThroughCacheRule>,
    pub registry_policy: dashmap::DashMap<String, String>,
    pub registry_scanning_config: std::sync::RwLock<RegistryScanningConfiguration>,
    pub replication_config: std::sync::RwLock<ReplicationConfiguration>,
    pub account_settings: DashMap<String, String>,
}
