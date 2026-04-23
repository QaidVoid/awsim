mod ids;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::CloudFrontState;

/// The AWSim CloudFront service handler.
pub struct CloudFrontService {
    store: AccountRegionStore<CloudFrontState>,
}

impl CloudFrontService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    /// CloudFront state is global per account — region is not used.
    fn get_state(&self, ctx: &RequestContext) -> Arc<CloudFrontState> {
        self.store.get(&ctx.account_id, "global")
    }
}

impl Default for CloudFrontService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudFrontService {
    fn service_name(&self) -> &str {
        "cloudfront"
    }

    fn signing_name(&self) -> &str {
        "cloudfront"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestXml
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Distributions
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/distribution",
                operation: "CreateDistribution",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distribution",
                operation: "ListDistributions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distribution/{Id}",
                operation: "GetDistribution",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/distribution/{Id}",
                operation: "DeleteDistribution",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2020-05-31/distribution/{Id}/config",
                operation: "UpdateDistribution",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distribution/{Id}/config",
                operation: "GetDistributionConfig",
                required_query_param: None,
            },
            // Invalidations
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/distribution/{DistributionId}/invalidation",
                operation: "CreateInvalidation",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distribution/{DistributionId}/invalidation",
                operation: "ListInvalidations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distribution/{DistributionId}/invalidation/{Id}",
                operation: "GetInvalidation",
                required_query_param: None,
            },
            // Origin Access Controls
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/origin-access-control",
                operation: "CreateOriginAccessControl",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/origin-access-control",
                operation: "ListOriginAccessControls",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/origin-access-control/{Id}",
                operation: "DeleteOriginAccessControl",
                required_query_param: None,
            },
            // Legacy OAIs
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/origin-access-identity/cloudfront",
                operation: "CreateCloudFrontOriginAccessIdentity",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/origin-access-identity/cloudfront/{Id}",
                operation: "GetCloudFrontOriginAccessIdentity",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/origin-access-identity/cloudfront",
                operation: "ListCloudFrontOriginAccessIdentities",
                required_query_param: None,
            },
            // Cache Policies
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/cache-policy",
                operation: "CreateCachePolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/cache-policy",
                operation: "ListCachePolicies",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/cache-policy/{Id}",
                operation: "GetCachePolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/cache-policy/{Id}",
                operation: "DeleteCachePolicy",
                required_query_param: None,
            },
            // Response Headers Policies
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/response-headers-policy",
                operation: "ListResponseHeadersPolicies",
                required_query_param: None,
            },
            // Tags
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/tagging",
                operation: "TagResource",
                required_query_param: Some("Operation=Tag"),
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/tagging",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
            // Origin Request Policies
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/origin-request-policy",
                operation: "CreateOriginRequestPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/origin-request-policy",
                operation: "ListOriginRequestPolicies",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/origin-request-policy/{Id}",
                operation: "GetOriginRequestPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/origin-request-policy/{Id}",
                operation: "DeleteOriginRequestPolicy",
                required_query_param: None,
            },
            // Key Groups
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/key-group",
                operation: "CreateKeyGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/key-group",
                operation: "ListKeyGroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/key-group/{Id}",
                operation: "GetKeyGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/key-group/{Id}",
                operation: "DeleteKeyGroup",
                required_query_param: None,
            },
            // Public Keys
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/public-key",
                operation: "CreatePublicKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/public-key",
                operation: "ListPublicKeys",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/public-key/{Id}",
                operation: "GetPublicKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/public-key/{Id}",
                operation: "DeletePublicKey",
                required_query_param: None,
            },
            // Field Level Encryption
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/field-level-encryption",
                operation: "CreateFieldLevelEncryptionConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/field-level-encryption",
                operation: "ListFieldLevelEncryptionConfigs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/field-level-encryption/{Id}",
                operation: "GetFieldLevelEncryptionConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/field-level-encryption/{Id}",
                operation: "DeleteFieldLevelEncryptionConfig",
                required_query_param: None,
            },
            // Real-time Log Configs
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/realtime-log-config",
                operation: "CreateRealtimeLogConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/realtime-log-config",
                operation: "ListRealtimeLogConfigs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2020-05-31/realtime-log-config",
                operation: "GetRealtimeLogConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/delete-realtime-log-config",
                operation: "DeleteRealtimeLogConfig",
                required_query_param: None,
            },
            // CloudFront Functions
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/function",
                operation: "CreateFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/function",
                operation: "ListFunctions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/function/{Name}/describe",
                operation: "DescribeFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2020-05-31/function/{Name}",
                operation: "DeleteFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/function/{Name}/publish",
                operation: "PublishFunction",
                required_query_param: None,
            },
            // List by web ACL / realtime log config
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/distributionsByWebACLId/{WebACLId}",
                operation: "ListDistributionsByWebACLId",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2020-05-31/distributionsByRealtimeLogConfig",
                operation: "ListDistributionsByRealtimeLogConfig",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "CloudFront request");
        let state = self.get_state(ctx);

        match operation {
            // Distributions
            "CreateDistribution" => {
                operations::distributions::create_distribution(&state, &input, ctx)
            }
            "GetDistribution" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::distributions::get_distribution(&state, id)
            }
            "GetDistributionConfig" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::distributions::get_distribution_config(&state, id)
            }
            "ListDistributions" => operations::distributions::list_distributions(&state),
            "DeleteDistribution" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::distributions::delete_distribution(&state, id)
            }
            "UpdateDistribution" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                operations::distributions::update_distribution(&state, &id, &input)
            }

            // Invalidations
            "CreateInvalidation" => {
                let dist_id = input
                    .get("DistributionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::invalidations::create_invalidation(&state, dist_id, &input)
            }
            "GetInvalidation" => {
                let dist_id = input
                    .get("DistributionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let inv_id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::invalidations::get_invalidation(&state, dist_id, inv_id)
            }
            "ListInvalidations" => {
                let dist_id = input
                    .get("DistributionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::invalidations::list_invalidations(&state, dist_id)
            }

            // Origin Access Controls
            "CreateOriginAccessControl" => {
                operations::origin_access::create_origin_access_control(&state, &input)
            }
            "ListOriginAccessControls" => {
                operations::origin_access::list_origin_access_controls(&state)
            }
            "DeleteOriginAccessControl" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::origin_access::delete_origin_access_control(&state, id)
            }

            // Legacy OAIs
            "CreateCloudFrontOriginAccessIdentity" => {
                operations::oai::create_oai(&state, &input)
            }
            "GetCloudFrontOriginAccessIdentity" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::oai::get_oai(&state, id)
            }
            "ListCloudFrontOriginAccessIdentities" => {
                operations::oai::list_oais(&state)
            }

            // Cache Policies
            "CreateCachePolicy" => operations::cache_policies::create_cache_policy(&state, &input),
            "GetCachePolicy" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::cache_policies::get_cache_policy(&state, id)
            }
            "DeleteCachePolicy" => {
                let id = input
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                operations::cache_policies::delete_cache_policy(&state, id)
            }
            "ListCachePolicies" => operations::cache_policies::list_cache_policies(&state),

            // Response Headers Policies
            "ListResponseHeadersPolicies" => {
                operations::response_headers_policies::list_response_headers_policies(&state)
            }

            // Tags
            "TagResource" => operations::tags::tag_resource(&state, &input),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input),

            // Origin Request Policies
            "CreateOriginRequestPolicy" => {
                operations::origin_request_policies::create_origin_request_policy(&state, &input)
            }
            "GetOriginRequestPolicy" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::origin_request_policies::get_origin_request_policy(&state, id)
            }
            "DeleteOriginRequestPolicy" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::origin_request_policies::delete_origin_request_policy(&state, id)
            }
            "ListOriginRequestPolicies" => {
                operations::origin_request_policies::list_origin_request_policies(&state)
            }

            // Key Groups
            "CreateKeyGroup" => operations::key_groups::create_key_group(&state, &input),
            "GetKeyGroup" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::key_groups::get_key_group(&state, id)
            }
            "DeleteKeyGroup" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::key_groups::delete_key_group(&state, id)
            }
            "ListKeyGroups" => operations::key_groups::list_key_groups(&state),

            // Public Keys
            "CreatePublicKey" => operations::public_keys::create_public_key(&state, &input),
            "GetPublicKey" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::public_keys::get_public_key(&state, id)
            }
            "DeletePublicKey" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::public_keys::delete_public_key(&state, id)
            }
            "ListPublicKeys" => operations::public_keys::list_public_keys(&state),

            // Field Level Encryption
            "CreateFieldLevelEncryptionConfig" => {
                operations::field_level_encryption::create_field_level_encryption_config(
                    &state, &input,
                )
            }
            "GetFieldLevelEncryptionConfig" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::field_level_encryption::get_field_level_encryption_config(&state, id)
            }
            "DeleteFieldLevelEncryptionConfig" => {
                let id = input.get("Id").and_then(|v| v.as_str()).unwrap_or("");
                operations::field_level_encryption::delete_field_level_encryption_config(
                    &state, id,
                )
            }
            "ListFieldLevelEncryptionConfigs" => {
                operations::field_level_encryption::list_field_level_encryption_configs(&state)
            }

            // Real-time Log Configs
            "CreateRealtimeLogConfig" => {
                operations::realtime_logs::create_realtime_log_config(&state, &input)
            }
            "GetRealtimeLogConfig" => {
                operations::realtime_logs::get_realtime_log_config(&state, &input)
            }
            "DeleteRealtimeLogConfig" => {
                operations::realtime_logs::delete_realtime_log_config(&state, &input)
            }
            "ListRealtimeLogConfigs" => {
                operations::realtime_logs::list_realtime_log_configs(&state)
            }

            // CloudFront Functions
            "CreateFunction" => operations::functions::create_function(&state, &input),
            "DescribeFunction" => {
                let name = input.get("Name").and_then(|v| v.as_str()).unwrap_or("");
                operations::functions::describe_function(&state, name)
            }
            "DeleteFunction" => {
                let name = input.get("Name").and_then(|v| v.as_str()).unwrap_or("");
                operations::functions::delete_function(&state, name)
            }
            "ListFunctions" => operations::functions::list_functions(&state),
            "PublishFunction" => {
                let name = input.get("Name").and_then(|v| v.as_str()).unwrap_or("");
                operations::functions::publish_function(&state, name)
            }

            // Distribution listings
            "ListDistributionsByWebACLId" => {
                let acl = input.get("WebACLId").and_then(|v| v.as_str()).unwrap_or("");
                operations::extras::list_distributions_by_web_acl_id(&state, acl)
            }
            "ListDistributionsByRealtimeLogConfig" => {
                operations::extras::list_distributions_by_realtime_log_config(&state, &input)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
