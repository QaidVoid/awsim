use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use awsim_core::BodyStore;
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

#[derive(Debug, Clone)]
pub enum LayerBody {
    InMemory(Vec<u8>),
    OnDisk(PathBuf),
}

#[allow(dead_code)]
impl LayerBody {
    pub fn read_all(&self) -> std::io::Result<Vec<u8>> {
        match self {
            LayerBody::InMemory(b) => Ok(b.clone()),
            LayerBody::OnDisk(p) => std::fs::read(p),
        }
    }

    pub fn len_hint(&self) -> Option<u64> {
        match self {
            LayerBody::InMemory(b) => Some(b.len() as u64),
            LayerBody::OnDisk(p) => std::fs::metadata(p).ok().map(|m| m.len()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    #[allow(dead_code)]
    pub digest: String,
    pub body: LayerBody,
    pub size: u64,
    pub media_type: String,
}

/// An ECR repository.
#[derive(Debug)]
pub struct Repository {
    pub name: String,
    pub arn: String,
    pub registry_id: String,
    pub repository_uri: String,
    pub images: Vec<ContainerImage>,
    pub layers: DashMap<String, Layer>,
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
    pub body_store: OnceLock<Arc<BodyStore>>,
}

impl EcrState {
    #[allow(dead_code)]
    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.get()
    }

    #[allow(dead_code)]
    pub fn set_body_store(&self, store: Arc<BodyStore>) {
        let _ = self.body_store.set(store);
    }
}
