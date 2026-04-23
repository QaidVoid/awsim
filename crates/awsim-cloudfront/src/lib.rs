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

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
