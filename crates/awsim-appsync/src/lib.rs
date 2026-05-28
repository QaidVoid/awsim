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
            // GraphQL APIs
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
            // Schema
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
            // API Keys
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
                path_pattern: "/v1/apis/{apiId}/apikeys/{id}",
                operation: "UpdateApiKey",
                required_query_param: None,
            },
            // Data Sources
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
            // Resolvers
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
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}/resolvers/{fieldName}",
                operation: "UpdateResolver",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}/resolvers/{fieldName}",
                operation: "DeleteResolver",
                required_query_param: None,
            },
            // GraphQL Types
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/types",
                operation: "CreateType",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}",
                operation: "GetType",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/types",
                operation: "ListTypes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}",
                operation: "DeleteType",
                required_query_param: None,
            },
            // AppSync Functions
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/functions",
                operation: "CreateFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/functions/{functionId}",
                operation: "GetFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/functions",
                operation: "ListFunctions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/functions/{functionId}",
                operation: "DeleteFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/functions/{functionId}",
                operation: "UpdateFunction",
                required_query_param: None,
            },
            // API Cache
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apis/{apiId}/FlushCache",
                operation: "FlushApiCache",
                required_query_param: None,
            },
            // Data source extras
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/datasources/{name}",
                operation: "GetDataSource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apis/{apiId}/datasources/{name}",
                operation: "UpdateDataSource",
                required_query_param: None,
            },
            // Resolver get
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}/resolvers/{fieldName}",
                operation: "GetResolver",
                required_query_param: None,
            },
            // Type update
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/apis/{apiId}/types/{typeName}",
                operation: "UpdateType",
                required_query_param: None,
            },
            // Schema introspection
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/schema",
                operation: "GetIntrospectionSchema",
                required_query_param: None,
            },
            // Tags
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
            // Source API associations
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/mergedApis/{mergedApiIdentifier}/sourceApiAssociations",
                operation: "AssociateMergedGraphqlApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/mergedApis/{mergedApiIdentifier}/sourceApiAssociations/{associationId}",
                operation: "GetSourceApiAssociation",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apis/{apiId}/sourceApiAssociations",
                operation: "ListSourceApiAssociations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/mergedApis/{mergedApiIdentifier}/sourceApiAssociations/{associationId}",
                operation: "DisassociateMergedGraphqlApi",
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
            // GraphQL APIs
            "CreateGraphqlApi" => operations::create_graphql_api(&state, &input, ctx),
            "GetGraphqlApi" => operations::get_graphql_api(&state, &input),
            "ListGraphqlApis" => operations::list_graphql_apis(&state),
            "DeleteGraphqlApi" => operations::delete_graphql_api(&state, &input),
            "UpdateGraphqlApi" => operations::update_graphql_api(&state, &input),
            // Schema
            "StartSchemaCreation" => operations::start_schema_creation(&state, &input),
            "GetSchemaCreationStatus" => operations::get_schema_creation_status(&state, &input),
            // API Keys
            "CreateApiKey" => operations::create_api_key(&state, &input),
            "ListApiKeys" => operations::list_api_keys(&state, &input),
            "DeleteApiKey" => operations::delete_api_key(&state, &input),
            "UpdateApiKey" => operations::update_api_key(&state, &input),
            // Data Sources
            "CreateDataSource" => operations::create_data_source(&state, &input),
            "ListDataSources" => operations::list_data_sources(&state, &input),
            "DeleteDataSource" => operations::delete_data_source(&state, &input),
            // Resolvers
            "CreateResolver" => operations::create_resolver(&state, &input),
            "ListResolvers" => operations::list_resolvers(&state, &input),
            "UpdateResolver" => operations::update_resolver(&state, &input),
            "DeleteResolver" => operations::delete_resolver(&state, &input),
            // GraphQL Types
            "CreateType" => operations::create_type(&state, &input, ctx),
            "GetType" => operations::get_type(&state, &input),
            "ListTypes" => operations::list_types(&state, &input),
            "DeleteType" => operations::delete_type(&state, &input),
            // AppSync Functions
            "CreateFunction" => operations::create_function(&state, &input, ctx),
            "GetFunction" => operations::get_function(&state, &input),
            "ListFunctions" => operations::list_functions(&state, &input),
            "DeleteFunction" => operations::delete_function(&state, &input),
            "UpdateFunction" => operations::update_function(&state, &input),
            // API Cache
            "FlushApiCache" => operations::flush_api_cache(&state, &input),
            // Data source extras
            "GetDataSource" => operations::get_data_source(&state, &input),
            "UpdateDataSource" => operations::update_data_source(&state, &input),
            // Resolver get
            "GetResolver" => operations::get_resolver(&state, &input),
            // Type update
            "UpdateType" => operations::update_type(&state, &input),
            // Schema introspection
            "GetIntrospectionSchema" => operations::get_introspection_schema(&state, &input),
            // Tags
            "TagResource" => operations::tag_resource(&state, &input),
            "UntagResource" => operations::untag_resource(&state, &input),
            "ListTagsForResource" => operations::list_tags_for_resource(&state, &input),
            // Source API associations
            "AssociateMergedGraphqlApi" => {
                operations::associate_merged_graphql_api(&state, &input, ctx)
            }
            "GetSourceApiAssociation" => operations::get_source_api_association(&state, &input),
            "ListSourceApiAssociations" => operations::list_source_api_associations(&state, &input),
            "DisassociateMergedGraphqlApi" => {
                operations::disassociate_merged_graphql_api(&state, &input)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let entries: Vec<(String, String, state::AppSyncStateSnapshot)> = self
            .store
            .iter_all()
            .into_iter()
            .map(|((account, region), state)| (account, region, state.to_snapshot()))
            .collect();
        serde_json::to_vec(&entries).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let entries: Vec<(String, String, state::AppSyncStateSnapshot)> =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for (account, region, snap) in entries {
            self.store
                .get(&account, &region)
                .restore_from_snapshot(snap);
        }
        Ok(())
    }
}
