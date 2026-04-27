use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, Body, BodyStore, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{auth, extras, images, registry, repositories, tags};
use crate::state::{EcrState, EcrStateSnapshot};

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
}
