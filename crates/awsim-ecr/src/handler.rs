use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BodyStore, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{auth, extras, images, registry, repositories, tags};
use crate::state::EcrState;

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
}
