mod operations;
mod state;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::EksState;

pub struct EksService {
    store: AccountRegionStore<EksState>,
}

impl EksService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for EksService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for EksService {
    fn service_name(&self) -> &str {
        "eks"
    }

    fn signing_name(&self) -> &str {
        "eks"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters",
                operation: "CreateCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters",
                operation: "ListClusters",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{name}",
                operation: "DescribeCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{name}",
                operation: "DeleteCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{name}/update-config",
                operation: "UpdateClusterConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/node-groups",
                operation: "CreateNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/node-groups",
                operation: "ListNodegroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/node-groups/{nodegroupName}",
                operation: "DescribeNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{clusterName}/node-groups/{nodegroupName}",
                operation: "DeleteNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/fargate-profiles",
                operation: "CreateFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/fargate-profiles",
                operation: "ListFargateProfiles",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/fargate-profiles/{fargateProfileName}",
                operation: "DescribeFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{clusterName}/fargate-profiles/{fargateProfileName}",
                operation: "DeleteFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{resourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{resourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{resourceArn}",
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
        debug!(operation = %operation, "EKS operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateCluster" => operations::clusters::create_cluster(&state, &input, ctx),
            "DescribeCluster" => operations::clusters::describe_cluster(&state, &input, ctx),
            "DeleteCluster" => operations::clusters::delete_cluster(&state, &input, ctx),
            "ListClusters" => operations::clusters::list_clusters(&state, &input, ctx),
            "UpdateClusterConfig" => operations::clusters::update_cluster_config(&state, &input, ctx),
            "CreateNodegroup" => operations::nodegroups::create_nodegroup(&state, &input, ctx),
            "DescribeNodegroup" => operations::nodegroups::describe_nodegroup(&state, &input, ctx),
            "DeleteNodegroup" => operations::nodegroups::delete_nodegroup(&state, &input, ctx),
            "ListNodegroups" => operations::nodegroups::list_nodegroups(&state, &input, ctx),
            "CreateFargateProfile" => operations::fargate_profiles::create_fargate_profile(&state, &input, ctx),
            "DescribeFargateProfile" => operations::fargate_profiles::describe_fargate_profile(&state, &input, ctx),
            "DeleteFargateProfile" => operations::fargate_profiles::delete_fargate_profile(&state, &input, ctx),
            "ListFargateProfiles" => operations::fargate_profiles::list_fargate_profiles(&state, &input, ctx),
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
