use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{auth, extras, images, repositories, tags};
use crate::state::EcrState;

/// The ECR service handler.
pub struct EcrService {
    store: AccountRegionStore<EcrState>,
}

impl EcrService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
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

        let state = self.store.get(&ctx.account_id, &ctx.region);

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

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
