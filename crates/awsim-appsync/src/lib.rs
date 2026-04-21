mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::AppSyncState;

pub struct AppSyncService {
    store: AccountRegionStore<AppSyncState>,
}

impl AppSyncService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<AppSyncState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for AppSyncService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for AppSyncService {
    fn service_name(&self) -> &str {
        "appsync"
    }

    fn signing_name(&self) -> &str {
        "appsync"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis",
                operation: "CreateGraphqlApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis",
                operation: "ListGraphqlApis",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}",
                operation: "GetGraphqlApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}",
                operation: "DeleteGraphqlApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}",
                operation: "UpdateGraphqlApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/schemacreation",
                operation: "StartSchemaCreation",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/schemacreation",
                operation: "GetSchemaCreationStatus",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/apikeys",
                operation: "CreateApiKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/apikeys",
                operation: "ListApiKeys",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/apikeys/{id}",
                operation: "DeleteApiKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/datasources",
                operation: "CreateDataSource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/datasources",
                operation: "ListDataSources",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/datasources/{name}",
                operation: "DeleteDataSource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}/resolvers",
                operation: "CreateResolver",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}/resolvers",
                operation: "ListResolvers",
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
        debug!(operation, "AppSync request");
        let state = self.get_state(ctx);

        match operation {
            "CreateGraphqlApi" => operations::create_graphql_api(&state, &input, ctx),
            "GetGraphqlApi" => operations::get_graphql_api(&state, &input),
            "ListGraphqlApis" => operations::list_graphql_apis(&state),
            "DeleteGraphqlApi" => operations::delete_graphql_api(&state, &input),
            "UpdateGraphqlApi" => operations::update_graphql_api(&state, &input),
            "StartSchemaCreation" => operations::start_schema_creation(&state, &input),
            "GetSchemaCreationStatus" => operations::get_schema_creation_status(&state, &input),
            "CreateApiKey" => operations::create_api_key(&state, &input),
            "ListApiKeys" => operations::list_api_keys(&state, &input),
            "DeleteApiKey" => operations::delete_api_key(&state, &input),
            "CreateDataSource" => operations::create_data_source(&state, &input),
            "ListDataSources" => operations::list_data_sources(&state, &input),
            "DeleteDataSource" => operations::delete_data_source(&state, &input),
            "CreateResolver" => operations::create_resolver(&state, &input),
            "ListResolvers" => operations::list_resolvers(&state, &input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
