use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BlobInventory, Body, BodyStore, Protocol, RequestContext,
    ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::repositories::now_epoch_str;
use crate::operations::{auth, extras, images, registry, repositories, tags};
use crate::state::{EcrState, EcrStateSnapshot};

/// Milliseconds a replication task spends PENDING before the tick
/// advances it to IN_PROGRESS.
const REPLICATION_IN_PROGRESS_AFTER_MS: u64 = 100;
/// Milliseconds a replication task spends in-flight before the tick
/// marks it COMPLETE and materializes the image at the destination.
const REPLICATION_COMPLETE_AFTER_MS: u64 = 400;

/// The ECR service handler.
pub struct EcrService {
    store: AccountRegionStore<EcrState>,
    body_store: Option<Arc<BodyStore>>,
    port: u16,
}

impl EcrService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: None,
            port: 4566,
        }
    }

    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: Some(Arc::new(BodyStore::new(dir.as_ref().to_path_buf()))),
            port: 4566,
        }
    }

    pub fn with_max_blob_bytes(mut self, bytes: u64) -> Self {
        if let Some(bs) = self.body_store.take() {
            let root = bs.root().to_path_buf();
            self.body_store = Some(Arc::new(BodyStore::new(root).with_max_size(bytes)));
        }
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<EcrState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
            .port
            .store(self.port, std::sync::atomic::Ordering::Relaxed);
        state
    }

    pub fn store(&self) -> AccountRegionStore<EcrState> {
        self.store.clone()
    }

    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.as_ref()
    }

    pub const GROUPS: &'static [&'static str] = &["ecr"];

    /// Copy a completed replication task's image manifest and its layer
    /// bindings into the destination account/region state.
    ///
    /// The destination repository is created on demand. Layers are
    /// rebound to the *same* on-disk blob (`blob_path("ecr", repo,
    /// digest)`) the source serves from - the body store is shared
    /// across every account/region state, and the repo name is preserved
    /// across the copy, so both registries resolve the identical file.
    /// When no body store is configured (pure in-memory) the layer body
    /// is copied by value so the destination still resolves.
    fn replicate_to_destination(&self, task: &crate::state::ReplicationTask) {
        use crate::state::{ContainerImage, Layer, Repository};

        // The task does not record the source account/region, so scan
        // every source state for the repo + digest and snapshot the
        // image plus the source layer entries under a short read.
        let mut found: Option<(ContainerImage, Vec<Layer>)> = None;
        for ((_, _), state) in self.store.iter_all() {
            if let Some(repo) = state.repositories.get(&task.source_repo)
                && let Some(img) = repo
                    .images
                    .iter()
                    .find(|i| i.image_digest == task.image_digest)
            {
                let layers: Vec<Layer> = repo.layers.iter().map(|e| e.value().clone()).collect();
                found = Some((img.clone(), layers));
                break;
            }
        }
        let Some((image, src_layers)) = found else {
            return;
        };

        let dest = self.store.get(&task.dest_account, &task.dest_region);
        if let Some(bs) = &self.body_store {
            dest.set_body_store(Arc::clone(bs));
        }

        // Ensure a destination repository exists with this name.
        if !dest.repositories.contains_key(&task.source_repo) {
            let arn = format!(
                "arn:aws:ecr:{}:{}:repository/{}",
                task.dest_region, task.dest_account, task.source_repo
            );
            let repository_uri = format!(
                "{}.dkr.ecr.{}.localhost/{}",
                task.dest_account, task.dest_region, task.source_repo
            );
            dest.repositories.insert(
                task.source_repo.clone(),
                Repository {
                    name: task.source_repo.clone(),
                    arn,
                    registry_id: task.dest_account.clone(),
                    repository_uri,
                    images: Vec::new(),
                    layers: dashmap::DashMap::new(),
                    created_at: now_epoch_str(),
                    image_tag_mutability: "MUTABLE".to_string(),
                    tags: std::collections::HashMap::new(),
                    lifecycle_policy: None,
                    lifecycle_policy_preview: None,
                    repository_policy: None,
                    scan_on_push: false,
                    encryption_type: "AES256".to_string(),
                    kms_key: None,
                },
            );
        }

        let Some(mut dest_repo) = dest.repositories.get_mut(&task.source_repo) else {
            return;
        };

        // Copy the image (idempotent on digest).
        if !dest_repo
            .images
            .iter()
            .any(|i| i.image_digest == image.image_digest)
        {
            dest_repo.images.push(image.clone());
        }

        // Rebind each layer to the shared on-disk blob (or copy the body
        // in-memory when no store is configured). The repo name is
        // preserved, so blob_path resolves to the identical source file.
        for src_layer in &src_layers {
            let digest = &src_layer.digest;
            if dest_repo.layers.contains_key(digest) {
                continue;
            }
            let body = match &self.body_store {
                Some(bs) => match bs.blob_path("ecr", &task.source_repo, digest) {
                    Ok(path) => Body::OnDisk(path),
                    Err(_) => continue,
                },
                None => src_layer.body.clone(),
            };
            dest_repo.layers.insert(
                digest.clone(),
                Layer {
                    digest: digest.clone(),
                    body,
                    size: src_layer.size,
                    media_type: src_layer.media_type.clone(),
                },
            );
        }
    }

    fn rebind_layer_bodies(&self) {
        for (_, state) in self.store.iter_all() {
            if let Some(bs) = &self.body_store {
                state.set_body_store(Arc::clone(bs));
                for repo_entry in state.repositories.iter() {
                    let name = repo_entry.key().clone();
                    let repo = repo_entry.value();
                    let mut to_remove: Vec<String> = Vec::new();
                    for mut layer_entry in repo.layers.iter_mut() {
                        let digest = layer_entry.key().clone();
                        match bs.blob_path("ecr", &name, &digest) {
                            Ok(path) => layer_entry.value_mut().body = Body::OnDisk(path),
                            Err(_) => to_remove.push(digest),
                        }
                    }
                    for d in to_remove {
                        repo.layers.remove(&d);
                    }
                }
            } else {
                for repo_entry in state.repositories.iter() {
                    repo_entry.value().layers.clear();
                }
            }
        }
    }
}

impl Default for EcrService {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobInventory for EcrService {
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (_, state) in self.store.iter_all() {
            for repo_entry in state.repositories.iter() {
                let name = repo_entry.key().clone();
                for layer_entry in repo_entry.value().layers.iter() {
                    out.push(("ecr".to_string(), name.clone(), layer_entry.key().clone()));
                }
            }
        }
        out
    }
}

#[async_trait::async_trait]
impl ServiceHandler for EcrService {
    fn service_name(&self) -> &str {
        "ecr"
    }

    fn signing_name(&self) -> &str {
        "ecr"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "ECR operation");

        let state = self.get_state(ctx);

        match operation {
            // Repositories
            "CreateRepository" => repositories::create_repository(&state, &input, ctx),
            "DeleteRepository" => repositories::delete_repository(&state, &input, ctx),
            "DescribeRepositories" => repositories::describe_repositories(&state, &input, ctx),

            // Authorization
            "GetAuthorizationToken" => auth::get_authorization_token(&input, ctx),

            // Images
            "PutImage" => images::put_image(&state, &input, ctx),
            "BatchGetImage" => images::batch_get_image(&state, &input, ctx),
            "BatchDeleteImage" => images::batch_delete_image(&state, &input, ctx),
            "ListImages" => images::list_images(&state, &input, ctx),
            "DescribeImages" => images::describe_images(&state, &input, ctx),
            "DescribeImageReplicationStatus" => {
                images::describe_image_replication_status(&state, &input, ctx)
            }

            // Tags
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),

            // Lifecycle Policies
            "PutLifecyclePolicy" => extras::put_lifecycle_policy(&state, &input, ctx),
            "GetLifecyclePolicy" => extras::get_lifecycle_policy(&state, &input, ctx),
            "DeleteLifecyclePolicy" => extras::delete_lifecycle_policy(&state, &input, ctx),

            // Repository Policies
            "SetRepositoryPolicy" => extras::set_repository_policy(&state, &input, ctx),
            "GetRepositoryPolicy" => extras::get_repository_policy(&state, &input, ctx),
            "DeleteRepositoryPolicy" => extras::delete_repository_policy(&state, &input, ctx),

            // Image Scanning
            "StartImageScan" => extras::start_image_scan(&state, &input, ctx),
            "DescribeImageScanFindings" => {
                extras::describe_image_scan_findings(&state, &input, ctx)
            }

            // Layer Operations
            "GetDownloadUrlForLayer" => extras::get_download_url_for_layer(&state, &input, ctx),
            "BatchCheckLayerAvailability" => {
                extras::batch_check_layer_availability(&state, &input, ctx)
            }
            "InitiateLayerUpload" => extras::initiate_layer_upload(&state, &input, ctx),
            "UploadLayerPart" => extras::upload_layer_part(&state, &input, ctx),
            "CompleteLayerUpload" => extras::complete_layer_upload(&state, &input, ctx),

            "PutImageTagMutability" => registry::put_image_tag_mutability(&state, &input, ctx),
            "PutImageScanningConfiguration" => {
                registry::put_image_scanning_configuration(&state, &input, ctx)
            }
            "StartLifecyclePolicyPreview" => {
                registry::start_lifecycle_policy_preview(&state, &input, ctx)
            }
            "GetLifecyclePolicyPreview" => {
                registry::get_lifecycle_policy_preview(&state, &input, ctx)
            }
            "GetRegistryPolicy" => registry::get_registry_policy(&state, &input, ctx),
            "PutRegistryPolicy" => registry::put_registry_policy(&state, &input, ctx),
            "DeleteRegistryPolicy" => registry::delete_registry_policy(&state, &input, ctx),
            "DescribeRegistry" => registry::describe_registry(&state, &input, ctx),
            "GetRegistryScanningConfiguration" => {
                registry::get_registry_scanning_configuration(&state, &input, ctx)
            }
            "PutRegistryScanningConfiguration" => {
                registry::put_registry_scanning_configuration(&state, &input, ctx)
            }
            "PutReplicationConfiguration" => {
                registry::put_replication_configuration(&state, &input, ctx)
            }
            "BatchGetRepositoryScanningConfiguration" => {
                registry::batch_get_repository_scanning_configuration(&state, &input, ctx)
            }
            "CreatePullThroughCacheRule" => {
                registry::create_pull_through_cache_rule(&state, &input, ctx)
            }
            "DeletePullThroughCacheRule" => {
                registry::delete_pull_through_cache_rule(&state, &input, ctx)
            }
            "DescribePullThroughCacheRules" => {
                registry::describe_pull_through_cache_rules(&state, &input, ctx)
            }
            "GetAccountSetting" => registry::get_account_setting(&state, &input, ctx),
            "PutAccountSetting" => registry::put_account_setting(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.store.snapshot_to_bytes()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        use crate::state::EcrRegionSnapshot;
        use awsim_core::Snapshottable;

        if let Ok(()) = self.store.restore_from_bytes(data) {
            self.rebind_layer_bodies();
            return Ok(());
        }

        let legacy: EcrStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let mut by_region: std::collections::HashMap<(String, String), Vec<_>> =
            std::collections::HashMap::new();
        for rs in legacy.repositories {
            by_region
                .entry((rs.account_id.clone(), rs.region.clone()))
                .or_default()
                .push(rs);
        }
        self.store.clear();
        for ((account_id, region), repositories) in by_region {
            let snap = EcrRegionSnapshot {
                account_id: account_id.clone(),
                region: region.clone(),
                repositories,
            };
            let (acct, reg, state) = EcrState::from_snapshot(snap);
            self.store.set(&acct, &reg, state);
        }
        self.rebind_layer_bodies();
        Ok(())
    }

    /// Advance the image replication state machine.
    ///
    /// For every in-flight [`crate::state::ReplicationTask`] this maps
    /// absolute elapsed time since `enqueued_at` onto
    /// PENDING -> IN_PROGRESS -> COMPLETE. The mapping is pure time, so
    /// repeated ticks at the same instant are idempotent. The moment a
    /// task first reaches COMPLETE the source image + its layer bindings
    /// are copied into the destination account/region state (reusing the
    /// shared on-disk blobs). The work is bounded: a fixed walk over the
    /// task set with no blocking I/O beyond a layer rebind.
    async fn tick(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        for (_, state) in self.store.iter_all() {
            for mut entry in state.replication_tasks.iter_mut() {
                let task = entry.value_mut();
                let elapsed = now.saturating_sub(task.enqueued_at);
                let next = if elapsed >= REPLICATION_COMPLETE_AFTER_MS {
                    "COMPLETE"
                } else if elapsed >= REPLICATION_IN_PROGRESS_AFTER_MS {
                    "IN_PROGRESS"
                } else {
                    "PENDING"
                };
                // Materialize at the destination exactly on the
                // PENDING/IN_PROGRESS -> COMPLETE transition.
                if next == "COMPLETE" && task.status != "COMPLETE" {
                    self.replicate_to_destination(task);
                }
                if task.status != next {
                    task.status = next.to_string();
                }
            }
        }
    }
}
