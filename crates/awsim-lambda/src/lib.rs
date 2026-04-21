mod error;
mod operations;
mod state;
mod util;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::LambdaState;

pub struct LambdaService {
    store: AccountRegionStore<LambdaState>,
}

impl LambdaService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<LambdaState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for LambdaService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for LambdaService {
    fn service_name(&self) -> &str {
        "lambda"
    }

    fn signing_name(&self) -> &str {
        "lambda"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Functions
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions",
                operation: "CreateFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions",
                operation: "ListFunctions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}",
                operation: "GetFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-03-31/functions/{FunctionName}",
                operation: "DeleteFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}/configuration",
                operation: "GetFunctionConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-03-31/functions/{FunctionName}/code",
                operation: "UpdateFunctionCode",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-03-31/functions/{FunctionName}/configuration",
                operation: "UpdateFunctionConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions/{FunctionName}/invocations",
                operation: "Invoke",
                required_query_param: None,
            },
            // Versions
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions/{FunctionName}/versions",
                operation: "PublishVersion",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}/versions",
                operation: "ListVersionsByFunction",
                required_query_param: None,
            },
            // Aliases
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions/{FunctionName}/aliases",
                operation: "CreateAlias",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}/aliases",
                operation: "ListAliases",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}/aliases/{Name}",
                operation: "GetAlias",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-03-31/functions/{FunctionName}/aliases/{Name}",
                operation: "DeleteAlias",
                required_query_param: None,
            },
            // Event Source Mappings
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/event-source-mappings",
                operation: "CreateEventSourceMapping",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/event-source-mappings",
                operation: "ListEventSourceMappings",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/event-source-mappings/{UUID}",
                operation: "GetEventSourceMapping",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-03-31/event-source-mappings/{UUID}",
                operation: "DeleteEventSourceMapping",
                required_query_param: None,
            },
            // Layers
            RouteDefinition {
                method: "POST",
                path_pattern: "/2018-10-31/layers/{LayerName}/versions",
                operation: "PublishLayerVersion",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2018-10-31/layers",
                operation: "ListLayers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2018-10-31/layers/{LayerName}/versions",
                operation: "ListLayerVersions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2018-10-31/layers/{LayerName}/versions/{VersionNumber}",
                operation: "DeleteLayerVersion",
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
        debug!(operation, "Lambda request");
        let state = self.get_state(ctx);

        match operation {
            // Functions
            "CreateFunction" => {
                operations::functions::create_function(&state, &input, ctx)
            }
            "GetFunction" => {
                operations::functions::get_function(&state, &input, ctx)
            }
            "GetFunctionConfiguration" => {
                operations::functions::get_function_configuration(&state, &input, ctx)
            }
            "DeleteFunction" => operations::functions::delete_function(&state, &input),
            "ListFunctions" => {
                operations::functions::list_functions(&state, &input, ctx)
            }
            "UpdateFunctionCode" => {
                operations::functions::update_function_code(&state, &input, ctx)
            }
            "UpdateFunctionConfiguration" => {
                operations::functions::update_function_configuration(&state, &input, ctx)
            }

            // Invocations
            "Invoke" => operations::invocations::invoke(&state, &input, ctx),

            // Versions
            "PublishVersion" => {
                operations::versions::publish_version(&state, &input, ctx)
            }
            "ListVersionsByFunction" => {
                operations::versions::list_versions_by_function(&state, &input, ctx)
            }

            // Aliases
            "CreateAlias" => operations::aliases::create_alias(&state, &input, ctx),
            "GetAlias" => operations::aliases::get_alias(&state, &input, ctx),
            "DeleteAlias" => operations::aliases::delete_alias(&state, &input, ctx),
            "ListAliases" => operations::aliases::list_aliases(&state, &input, ctx),

            // Event Source Mappings
            "CreateEventSourceMapping" => {
                operations::event_source_mappings::create_event_source_mapping(
                    &state, &input, ctx,
                )
            }
            "GetEventSourceMapping" => {
                operations::event_source_mappings::get_event_source_mapping(&state, &input, ctx)
            }
            "DeleteEventSourceMapping" => {
                operations::event_source_mappings::delete_event_source_mapping(
                    &state, &input, ctx,
                )
            }
            "ListEventSourceMappings" => {
                operations::event_source_mappings::list_event_source_mappings(
                    &state, &input, ctx,
                )
            }

            // Layers
            "PublishLayerVersion" => {
                operations::layers::publish_layer_version(&state, &input, ctx)
            }
            "ListLayers" => operations::layers::list_layers(&state, &input, ctx),
            "ListLayerVersions" => {
                operations::layers::list_layer_versions(&state, &input, ctx)
            }
            "DeleteLayerVersion" => {
                operations::layers::delete_layer_version(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
