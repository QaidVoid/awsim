mod operations;
pub mod proxy;
mod state;
mod util;
pub mod v1;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::ApiGatewayState;

pub use proxy::{ProxyResponse, proxy_request};
pub use state::ApiGatewayState as State;
pub use v1::{ApiGatewayV1Service, ApiGatewayV1State};

pub struct ApiGatewayService {
    store: AccountRegionStore<ApiGatewayState>,
}

impl ApiGatewayService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<ApiGatewayState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Expose the underlying state store for proxy routing in main.rs.
    pub fn store(&self) -> &AccountRegionStore<ApiGatewayState> {
        &self.store
    }
}

impl Default for ApiGatewayService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for ApiGatewayService {
    fn service_name(&self) -> &str {
        "apigateway"
    }

    fn signing_name(&self) -> &str {
        "execute-api"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // APIs
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/apis",
                operation: "CreateApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis",
                operation: "GetApis",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}",
                operation: "GetApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/apis/{ApiId}",
                operation: "DeleteApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/v2/apis/{ApiId}",
                operation: "UpdateApi",
                required_query_param: None,
            },
            // Routes
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/apis/{ApiId}/routes",
                operation: "CreateRoute",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/routes",
                operation: "GetRoutes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/routes/{RouteId}",
                operation: "GetRoute",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/apis/{ApiId}/routes/{RouteId}",
                operation: "DeleteRoute",
                required_query_param: None,
            },
            // Integrations
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/apis/{ApiId}/integrations",
                operation: "CreateIntegration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/integrations/{IntegrationId}",
                operation: "GetIntegration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/apis/{ApiId}/integrations/{IntegrationId}",
                operation: "DeleteIntegration",
                required_query_param: None,
            },
            // Stages
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/apis/{ApiId}/stages",
                operation: "CreateStage",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/stages",
                operation: "GetStages",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/stages/{StageName}",
                operation: "GetStage",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/apis/{ApiId}/stages/{StageName}",
                operation: "DeleteStage",
                required_query_param: None,
            },
            // Deployments
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/apis/{ApiId}/deployments",
                operation: "CreateDeployment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/apis/{ApiId}/deployments/{DeploymentId}",
                operation: "GetDeployment",
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
        debug!(operation, "API Gateway request");
        let state = self.get_state(ctx);

        match operation {
            // APIs
            "CreateApi" => operations::apis::create_api(&state, &input, ctx),
            "GetApi" => operations::apis::get_api(&state, &input, ctx),
            "GetApis" => operations::apis::get_apis(&state, &input, ctx),
            "DeleteApi" => operations::apis::delete_api(&state, &input, ctx),
            "UpdateApi" => operations::apis::update_api(&state, &input, ctx),

            // Routes
            "CreateRoute" => operations::routes::create_route(&state, &input, ctx),
            "GetRoute" => operations::routes::get_route(&state, &input, ctx),
            "GetRoutes" => operations::routes::get_routes(&state, &input, ctx),
            "DeleteRoute" => operations::routes::delete_route(&state, &input, ctx),

            // Integrations
            "CreateIntegration" => {
                operations::integrations::create_integration(&state, &input, ctx)
            }
            "GetIntegration" => operations::integrations::get_integration(&state, &input, ctx),
            "DeleteIntegration" => {
                operations::integrations::delete_integration(&state, &input, ctx)
            }

            // Stages
            "CreateStage" => operations::stages::create_stage(&state, &input, ctx),
            "GetStage" => operations::stages::get_stage(&state, &input, ctx),
            "GetStages" => operations::stages::get_stages(&state, &input, ctx),
            "DeleteStage" => operations::stages::delete_stage(&state, &input, ctx),

            // Deployments
            "CreateDeployment" => operations::deployments::create_deployment(&state, &input, ctx),
            "GetDeployment" => operations::deployments::get_deployment(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
