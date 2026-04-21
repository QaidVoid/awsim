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
            // Cache Policies
            RouteDefinition {
                method: "GET",
                path_pattern: "/2020-05-31/cache-policy",
                operation: "ListCachePolicies",
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

            // Cache Policies — return a fixed list of managed policies
            "ListCachePolicies" => Ok(serde_json::json!({
                "CachePolicyList": {
                    "MaxItems": 100,
                    "Quantity": 1,
                    "Items": {
                        "CachePolicySummary": [{
                            "Type": "managed",
                            "CachePolicy": {
                                "Id": "658327ea-f89d-4fab-a63d-7e88639e58f6",
                                "LastModifiedTime": "2021-05-10T00:00:00Z",
                                "CachePolicyConfig": {
                                    "Name": "CachingOptimized",
                                    "DefaultTTL": 86400,
                                    "MaxTTL": 31536000,
                                    "MinTTL": 1,
                                    "Comment": "Optimized for caching",
                                }
                            }
                        }]
                    }
                }
            })),

            // Tags
            "TagResource" => operations::tags::tag_resource(&state, &input),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
