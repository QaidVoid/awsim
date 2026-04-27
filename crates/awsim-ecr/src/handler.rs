use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BodyStore, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{auth, extras, images, registry, repositories, tags};
use crate::state::{
    ContainerImage, EcrState, EcrStateSnapshot, ImageSnapshot, Layer, LayerBody, LayerSnapshot,
    Repository, RepositorySnapshot,
};

/// The ECR service handler.
pub struct EcrService {
    store: AccountRegionStore<EcrState>,
    body_store: Option<Arc<BodyStore>>,
}

impl EcrService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: None,
        }
    }

    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: Some(Arc::new(BodyStore::new(dir.as_ref().to_path_buf()))),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<EcrState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
    }

    pub fn store(&self) -> AccountRegionStore<EcrState> {
        self.store.clone()
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
        let repositories: Vec<RepositorySnapshot> = self
            .store
            .iter_all()
            .into_iter()
            .flat_map(|((account_id, region), state)| {
                state
                    .repositories
                    .iter()
                    .map(|entry| {
                        let r = entry.value();
                        RepositorySnapshot {
                            account_id: account_id.clone(),
                            region: region.clone(),
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
                                })
                                .collect(),
                            layers: r
                                .layers
                                .iter()
                                .map(|entry| {
                                    let l = entry.value();
                                    LayerSnapshot {
                                        digest: l.digest.clone(),
                                        size: l.size,
                                        media_type: l.media_type.clone(),
                                    }
                                })
                                .collect(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        serde_json::to_vec(&EcrStateSnapshot { repositories }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: EcrStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;

        for rs in snapshot.repositories {
            let state = self.store.get(&rs.account_id, &rs.region);
            if let Some(bs) = &self.body_store {
                state.set_body_store(Arc::clone(bs));
            }

            let images: Vec<ContainerImage> = rs
                .images
                .into_iter()
                .map(|i| ContainerImage {
                    image_digest: i.image_digest,
                    image_tag: i.image_tag,
                    image_manifest: i.image_manifest,
                    pushed_at: i.pushed_at,
                    image_size_in_bytes: i.image_size_in_bytes,
                })
                .collect();

            let layers = dashmap::DashMap::new();
            for ls in rs.layers {
                let body = match self.body_store.as_ref() {
                    Some(bs) => match bs.blob_path("ecr", &rs.name, &ls.digest) {
                        Ok(p) => LayerBody::OnDisk(p),
                        Err(_) => continue,
                    },
                    None => continue,
                };
                layers.insert(
                    ls.digest.clone(),
                    Layer {
                        digest: ls.digest,
                        body,
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
            };

            state.repositories.insert(rs.name, repo);
        }

        Ok(())
    }
}
