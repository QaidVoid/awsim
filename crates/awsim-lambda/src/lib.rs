pub mod authz;
mod error;
mod executor;
mod operations;
pub mod state;
mod util;

pub use authz::LambdaResourcePolicyLookup;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Body, BodyStore, Protocol, RequestContext, RouteDefinition,
    ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::{
    Alias, AliasSnapshot, FunctionSnapshot, FunctionVersion, FunctionVersionSnapshot,
    LambdaFunction, LambdaState, LambdaStateSnapshot,
};

pub struct LambdaService {
    store: AccountRegionStore<LambdaState>,
    body_store: Option<Arc<BodyStore>>,
}

impl LambdaService {
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

    fn get_state(&self, ctx: &RequestContext) -> Arc<LambdaState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
    }

    pub fn store(&self) -> AccountRegionStore<LambdaState> {
        self.store.clone()
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
            // Function URL Configs
            RouteDefinition {
                method: "POST",
                path_pattern: "/2021-10-31/functions/{FunctionName}/url",
                operation: "CreateFunctionUrlConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2021-10-31/functions/{FunctionName}/url",
                operation: "GetFunctionUrlConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2021-10-31/functions/{FunctionName}/url",
                operation: "DeleteFunctionUrlConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2021-10-31/functions/{FunctionName}/urls",
                operation: "ListFunctionUrlConfigs",
                required_query_param: None,
            },
            // Tags
            RouteDefinition {
                method: "GET",
                path_pattern: "/2017-03-31/tags/{Resource}",
                operation: "ListTags",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2017-03-31/tags/{Resource}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2017-03-31/tags/{Resource}",
                operation: "UntagResource",
                required_query_param: None,
            },
            // Policy / Permissions
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}/policy",
                operation: "GetPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions/{FunctionName}/policy",
                operation: "AddPermission",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-03-31/functions/{FunctionName}/policy/{StatementId}",
                operation: "RemovePermission",
                required_query_param: None,
            },
            // Account Settings
            RouteDefinition {
                method: "GET",
                path_pattern: "/2016-08-19/account-settings",
                operation: "GetAccountSettings",
                required_query_param: None,
            },
            // Event Invoke Configs
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2019-09-25/functions/{FunctionName}/event-invoke-config",
                operation: "PutFunctionEventInvokeConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2019-09-25/functions/{FunctionName}/event-invoke-config",
                operation: "GetFunctionEventInvokeConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2019-09-25/functions/{FunctionName}/event-invoke-config",
                operation: "DeleteFunctionEventInvokeConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2019-09-25/functions/{FunctionName}/event-invoke-config",
                operation: "UpdateFunctionEventInvokeConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2019-09-25/functions/{FunctionName}/event-invoke-config/list",
                operation: "ListFunctionEventInvokeConfigs",
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
            "CreateFunction" => operations::functions::create_function(&state, &input, ctx),
            "GetFunction" => operations::functions::get_function(&state, &input, ctx),
            "GetFunctionConfiguration" => {
                operations::functions::get_function_configuration(&state, &input, ctx)
            }
            "DeleteFunction" => operations::functions::delete_function(&state, &input),
            "ListFunctions" => operations::functions::list_functions(&state, &input, ctx),
            "UpdateFunctionCode" => {
                operations::functions::update_function_code(&state, &input, ctx)
            }
            "UpdateFunctionConfiguration" => {
                operations::functions::update_function_configuration(&state, &input, ctx)
            }

            // Invocations
            "Invoke" => operations::invocations::invoke(&state, &input, ctx),

            // Versions
            "PublishVersion" => operations::versions::publish_version(&state, &input, ctx),
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
                operations::event_source_mappings::create_event_source_mapping(&state, &input, ctx)
            }
            "GetEventSourceMapping" => {
                operations::event_source_mappings::get_event_source_mapping(&state, &input, ctx)
            }
            "DeleteEventSourceMapping" => {
                operations::event_source_mappings::delete_event_source_mapping(&state, &input, ctx)
            }
            "ListEventSourceMappings" => {
                operations::event_source_mappings::list_event_source_mappings(&state, &input, ctx)
            }

            // Layers
            "PublishLayerVersion" => operations::layers::publish_layer_version(&state, &input, ctx),
            "ListLayers" => operations::layers::list_layers(&state, &input, ctx),
            "ListLayerVersions" => operations::layers::list_layer_versions(&state, &input, ctx),
            "DeleteLayerVersion" => operations::layers::delete_layer_version(&state, &input, ctx),

            // Function URL Configs
            "CreateFunctionUrlConfig" => {
                operations::url_configs::create_function_url_config(&state, &input, ctx)
            }
            "GetFunctionUrlConfig" => {
                operations::url_configs::get_function_url_config(&state, &input, ctx)
            }
            "DeleteFunctionUrlConfig" => {
                operations::url_configs::delete_function_url_config(&state, &input, ctx)
            }
            "ListFunctionUrlConfigs" => {
                operations::url_configs::list_function_url_configs(&state, &input, ctx)
            }

            // Tags
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTags" => operations::tags::list_tags(&state, &input, ctx),

            // Policy / Permissions
            "GetPolicy" => operations::permissions::get_policy(&state, &input, ctx),
            "AddPermission" => operations::permissions::add_permission(&state, &input, ctx),
            "RemovePermission" => operations::permissions::remove_permission(&state, &input, ctx),

            // Account Settings
            "GetAccountSettings" => {
                operations::permissions::get_account_settings(&state, &input, ctx)
            }

            // Event Invoke Configs
            "PutFunctionEventInvokeConfig" => {
                operations::event_invoke_configs::put_function_event_invoke_config(
                    &state, &input, ctx,
                )
            }
            "GetFunctionEventInvokeConfig" => {
                operations::event_invoke_configs::get_function_event_invoke_config(
                    &state, &input, ctx,
                )
            }
            "UpdateFunctionEventInvokeConfig" => {
                operations::event_invoke_configs::update_function_event_invoke_config(
                    &state, &input, ctx,
                )
            }
            "DeleteFunctionEventInvokeConfig" => {
                operations::event_invoke_configs::delete_function_event_invoke_config(
                    &state, &input, ctx,
                )
            }
            "ListFunctionEventInvokeConfigs" => {
                operations::event_invoke_configs::list_function_event_invoke_configs(
                    &state, &input, ctx,
                )
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let functions: Vec<FunctionSnapshot> = self
            .store
            .iter_all()
            .into_iter()
            .flat_map(|((account_id, region), state)| {
                state
                    .functions
                    .iter()
                    .map(|entry| {
                        let f = entry.value();
                        FunctionSnapshot {
                            account_id: account_id.clone(),
                            region: region.clone(),
                            name: f.name.clone(),
                            arn: f.arn.clone(),
                            runtime: f.runtime.clone(),
                            role: f.role.clone(),
                            handler: f.handler.clone(),
                            description: f.description.clone(),
                            timeout: f.timeout,
                            memory_size: f.memory_size,
                            code_sha256: f.code_sha256.clone(),
                            code_size: f.code_size,
                            environment: f.environment.clone(),
                            version: f.version.clone(),
                            versions: f
                                .versions
                                .iter()
                                .map(|v| FunctionVersionSnapshot {
                                    version: v.version.clone(),
                                    description: v.description.clone(),
                                    code_sha256: v.code_sha256.clone(),
                                    code_size: v.code_size,
                                    last_modified: v.last_modified.clone(),
                                })
                                .collect(),
                            aliases: f
                                .aliases
                                .iter()
                                .map(|(k, a)| {
                                    (
                                        k.clone(),
                                        AliasSnapshot {
                                            name: a.name.clone(),
                                            arn: a.arn.clone(),
                                            function_version: a.function_version.clone(),
                                            description: a.description.clone(),
                                        },
                                    )
                                })
                                .collect(),
                            last_modified: f.last_modified.clone(),
                            state: f.state.clone(),
                            policy_statements: f.policy_statements.clone(),
                            tags: f.tags.clone(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        serde_json::to_vec(&LambdaStateSnapshot { functions }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: LambdaStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        for fs in snapshot.functions {
            let state = self.store.get(&fs.account_id, &fs.region);
            if let Some(bs) = &self.body_store {
                state.set_body_store(Arc::clone(bs));
            }

            let code = self
                .body_store
                .as_ref()
                .and_then(|bs| bs.blob_path("lambda", &fs.name, "$LATEST").ok())
                .map(Body::OnDisk);

            let versions: Vec<FunctionVersion> = fs
                .versions
                .into_iter()
                .map(|v| {
                    let version_code = self
                        .body_store
                        .as_ref()
                        .and_then(|bs| bs.blob_path("lambda", &fs.name, &v.version).ok())
                        .map(Body::OnDisk);
                    FunctionVersion {
                        version: v.version,
                        description: v.description,
                        code_sha256: v.code_sha256,
                        code_size: v.code_size,
                        code: version_code,
                        last_modified: v.last_modified,
                    }
                })
                .collect();

            let aliases: std::collections::HashMap<String, Alias> = fs
                .aliases
                .into_iter()
                .map(|(k, a)| {
                    (
                        k,
                        Alias {
                            name: a.name,
                            arn: a.arn,
                            function_version: a.function_version,
                            description: a.description,
                        },
                    )
                })
                .collect();

            let func = LambdaFunction {
                name: fs.name.clone(),
                arn: fs.arn,
                runtime: fs.runtime,
                role: fs.role,
                handler: fs.handler,
                description: fs.description,
                timeout: fs.timeout,
                memory_size: fs.memory_size,
                code_sha256: fs.code_sha256,
                code_size: fs.code_size,
                code,
                environment: fs.environment,
                version: fs.version,
                versions,
                aliases,
                last_modified: fs.last_modified,
                state: fs.state,
                invocations: Vec::new(),
                policy_statements: fs.policy_statements,
                tags: fs.tags,
            };

            state.functions.insert(fs.name, func);
        }

        Ok(())
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "CreateFunction"
            | "GetFunction"
            | "GetFunctionConfiguration"
            | "DeleteFunction"
            | "ListFunctions"
            | "UpdateFunctionCode"
            | "UpdateFunctionConfiguration"
            | "Invoke"
            | "InvokeFunction"
            | "InvokeAsync"
            | "PublishVersion"
            | "ListVersionsByFunction"
            | "CreateAlias"
            | "GetAlias"
            | "DeleteAlias"
            | "ListAliases"
            | "UpdateAlias"
            | "CreateEventSourceMapping"
            | "GetEventSourceMapping"
            | "DeleteEventSourceMapping"
            | "ListEventSourceMappings"
            | "UpdateEventSourceMapping"
            | "PublishLayerVersion"
            | "ListLayers"
            | "ListLayerVersions"
            | "DeleteLayerVersion"
            | "GetLayerVersion"
            | "CreateFunctionUrlConfig"
            | "GetFunctionUrlConfig"
            | "DeleteFunctionUrlConfig"
            | "ListFunctionUrlConfigs"
            | "UpdateFunctionUrlConfig"
            | "TagResource"
            | "UntagResource"
            | "ListTags"
            | "GetPolicy"
            | "AddPermission"
            | "RemovePermission"
            | "GetAccountSettings"
            | "PutFunctionEventInvokeConfig"
            | "GetFunctionEventInvokeConfig"
            | "UpdateFunctionEventInvokeConfig"
            | "DeleteFunctionEventInvokeConfig"
            | "ListFunctionEventInvokeConfigs" => Some(format!("lambda:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        let prefix = format!("arn:aws:lambda:{}:{}", ctx.region, ctx.account_id);
        match operation {
            "ListFunctions"
            | "ListEventSourceMappings"
            | "ListLayers"
            | "GetAccountSettings"
            | "CreateFunction"
            | "CreateEventSourceMapping" => Some("*".to_string()),
            "TagResource" | "UntagResource" | "ListTags" => input
                .get("Resource")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "GetEventSourceMapping" | "DeleteEventSourceMapping" | "UpdateEventSourceMapping" => {
                input
                    .get("UUID")
                    .and_then(|v| v.as_str())
                    .map(|uuid| format!("{prefix}:event-source-mapping:{uuid}"))
            }
            "PublishLayerVersion" | "ListLayerVersions" => input
                .get("LayerName")
                .and_then(|v| v.as_str())
                .map(|name| format!("{prefix}:layer:{name}")),
            "GetLayerVersion" | "DeleteLayerVersion" => {
                let name = input.get("LayerName").and_then(|v| v.as_str())?;
                let version = input
                    .get("VersionNumber")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                Some(format!("{prefix}:layer:{name}:{version}"))
            }
            _ => {
                let name = input.get("FunctionName").and_then(|v| v.as_str())?;
                if name.starts_with("arn:") {
                    Some(name.to_string())
                } else {
                    Some(format!("{prefix}:function:{name}"))
                }
            }
        }
    }
}
