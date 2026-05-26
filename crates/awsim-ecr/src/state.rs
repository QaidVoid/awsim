use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use awsim_core::{Body, BodyStore, Snapshottable};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A single image stored in a repository.
#[derive(Debug, Clone)]
pub struct ContainerImage {
    pub image_digest: String,
    pub image_tag: Option<String>,
    pub image_manifest: String,
    pub pushed_at: String,
    pub image_size_in_bytes: u64,
    /// Canonical media type detected from manifest content (Docker schema
    /// 1/2, OCI image manifest, OCI image index). Surfaced via PutImage
    /// and BatchGetImage so clients can disambiguate single-arch from
    /// multi-arch indexes.
    pub image_manifest_media_type: Option<String>,
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
pub struct Layer {
    #[allow(dead_code)]
    pub digest: String,
    pub body: Body,
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
    /// `AES256` (default, AWS-managed) or `KMS` (customer-managed,
    /// requires `kms_key`). Persisted at CreateRepository.
    pub encryption_type: String,
    /// KMS key ARN when `encryption_type == "KMS"`.
    pub kms_key: Option<String>,
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
#[derive(Debug)]
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
    pub port: std::sync::atomic::AtomicU16,
}

impl Default for EcrState {
    fn default() -> Self {
        Self {
            repositories: DashMap::new(),
            layer_uploads: DashMap::new(),
            pull_through_cache_rules: DashMap::new(),
            registry_policy: DashMap::new(),
            registry_scanning_config: std::sync::RwLock::new(
                RegistryScanningConfiguration::default(),
            ),
            replication_config: std::sync::RwLock::new(ReplicationConfiguration::default()),
            account_settings: DashMap::new(),
            body_store: OnceLock::new(),
            port: std::sync::atomic::AtomicU16::new(4566),
        }
    }
}

impl EcrState {
    #[allow(dead_code)]
    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.get()
    }

    pub fn set_body_store(&self, store: Arc<BodyStore>) {
        let _ = self.body_store.set(store);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EcrStateSnapshot {
    pub repositories: Vec<RepositorySnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EcrRegionSnapshot {
    pub account_id: String,
    pub region: String,
    pub repositories: Vec<RepositorySnapshot>,
}

impl Snapshottable for EcrState {
    type Snapshot = EcrRegionSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        let repositories = self
            .repositories
            .iter()
            .map(|entry| {
                let r = entry.value();
                RepositorySnapshot {
                    account_id: account_id.to_string(),
                    region: region.to_string(),
                    name: r.name.clone(),
                    arn: r.arn.clone(),
                    registry_id: r.registry_id.clone(),
                    repository_uri: r.repository_uri.clone(),
                    created_at: r.created_at.clone(),
                    image_tag_mutability: r.image_tag_mutability.clone(),
                    tags: r.tags.clone(),
                    lifecycle_policy: r.lifecycle_policy.clone(),
                    repository_policy: r.repository_policy.clone(),
                    scan_on_push: r.scan_on_push,
                    images: r
                        .images
                        .iter()
                        .map(|i| ImageSnapshot {
                            image_digest: i.image_digest.clone(),
                            image_tag: i.image_tag.clone(),
                            image_manifest: i.image_manifest.clone(),
                            pushed_at: i.pushed_at.clone(),
                            image_size_in_bytes: i.image_size_in_bytes,
                            image_manifest_media_type: i.image_manifest_media_type.clone(),
                        })
                        .collect(),
                    encryption_type: Some(r.encryption_type.clone()),
                    kms_key: r.kms_key.clone(),
                    layers: r
                        .layers
                        .iter()
                        .map(|le| {
                            let l = le.value();
                            LayerSnapshot {
                                digest: l.digest.clone(),
                                size: l.size,
                                media_type: l.media_type.clone(),
                            }
                        })
                        .collect(),
                }
            })
            .collect();

        EcrRegionSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            repositories,
        }
    }

    fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self) {
        let state = EcrState::default();
        for rs in snapshot.repositories {
            let images: Vec<ContainerImage> = rs
                .images
                .into_iter()
                .map(|i| ContainerImage {
                    image_digest: i.image_digest,
                    image_tag: i.image_tag,
                    image_manifest: i.image_manifest,
                    pushed_at: i.pushed_at,
                    image_size_in_bytes: i.image_size_in_bytes,
                    image_manifest_media_type: i.image_manifest_media_type,
                })
                .collect();

            let layers = DashMap::new();
            for ls in rs.layers {
                layers.insert(
                    ls.digest.clone(),
                    Layer {
                        digest: ls.digest,
                        body: Body::InMemory(Vec::new()),
                        size: ls.size,
                        media_type: ls.media_type,
                    },
                );
            }

            let repo = Repository {
                name: rs.name.clone(),
                arn: rs.arn,
                registry_id: rs.registry_id,
                repository_uri: rs.repository_uri,
                images,
                layers,
                created_at: rs.created_at,
                image_tag_mutability: rs.image_tag_mutability,
                tags: rs.tags,
                lifecycle_policy: rs.lifecycle_policy,
                lifecycle_policy_preview: None,
                repository_policy: rs.repository_policy,
                scan_on_push: rs.scan_on_push,
                encryption_type: rs.encryption_type.unwrap_or_else(|| "AES256".to_string()),
                kms_key: rs.kms_key,
            };

            state.repositories.insert(rs.name, repo);
        }
        (snapshot.account_id, snapshot.region, state)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepositorySnapshot {
    pub account_id: String,
    pub region: String,
    pub name: String,
    pub arn: String,
    pub registry_id: String,
    pub repository_uri: String,
    pub created_at: String,
    pub image_tag_mutability: String,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub lifecycle_policy: Option<String>,
    #[serde(default)]
    pub repository_policy: Option<String>,
    #[serde(default)]
    pub scan_on_push: bool,
    #[serde(default)]
    pub images: Vec<ImageSnapshot>,
    #[serde(default)]
    pub layers: Vec<LayerSnapshot>,
    #[serde(default)]
    pub encryption_type: Option<String>,
    #[serde(default)]
    pub kms_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageSnapshot {
    pub image_digest: String,
    pub image_tag: Option<String>,
    pub image_manifest: String,
    pub pushed_at: String,
    pub image_size_in_bytes: u64,
    #[serde(default)]
    pub image_manifest_media_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LayerSnapshot {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
}
