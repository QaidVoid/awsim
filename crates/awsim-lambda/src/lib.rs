pub mod authz;
pub mod error;
mod executor;
mod operations;
pub mod state;
mod util;

pub use authz::LambdaResourcePolicyLookup;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, BlobInventory, Body, BodyStore, Protocol, RequestContext,
    RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::{LambdaState, LambdaStateSnapshot};

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

    pub fn with_max_blob_bytes(mut self, bytes: u64) -> Self {
        if let Some(bs) = self.body_store.take() {
            let root = bs.root().to_path_buf();
            self.body_store = Some(Arc::new(BodyStore::new(root).with_max_size(bytes)));
        }
        self
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

    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.as_ref()
    }

    pub const GROUPS: &'static [&'static str] = &["lambda"];

    fn rebind_bodies(&self) {
        let Some(bs) = &self.body_store else {
            return;
        };
        for (_, state) in self.store.iter_all() {
            state.set_body_store(Arc::clone(bs));
            for mut entry in state.functions.iter_mut() {
                let name = entry.key().clone();
                let func = entry.value_mut();
                if let Ok(path) = bs.blob_path("lambda", &name, "$LATEST") {
                    func.code = Some(Body::OnDisk(path));
                }
                for v in func.versions.iter_mut() {
                    if let Ok(path) = bs.blob_path("lambda", &name, &v.version) {
                        v.code = Some(Body::OnDisk(path));
                    }
                }
            }
        }
    }
}

impl Default for LambdaService {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-service invoker that lets other services (Secrets Manager
/// rotation, Cognito triggers, etc.) call a Lambda function
/// synchronously without depending on the full `LambdaService`
/// handler. Implements [`awsim_core::LambdaInvoker`].
///
/// `function_name` accepts either the bare function name or an
/// `arn:aws:lambda:region:account:function:name(:qualifier)?` ARN;
/// the invoker normalises to the bare name before dispatching.
pub struct LambdaServiceInvoker {
    store: AccountRegionStore<LambdaState>,
}

impl LambdaServiceInvoker {
    pub fn new(store: AccountRegionStore<LambdaState>) -> Self {
        Self { store }
    }

    fn normalise_function_name(reference: &str) -> &str {
        if let Some(rest) = reference.strip_prefix("arn:")
            && let Some(idx) = rest.rfind(':')
        {
            let tail = &rest[idx + 1..];
            if let Some((name, _qualifier)) = tail.split_once(':') {
                return name;
            }
            return tail;
        }
        reference
    }
}

impl awsim_core::LambdaInvoker for LambdaServiceInvoker {
    fn invoke(
        &self,
        function_name: &str,
        payload: &Value,
        account: &str,
        region: &str,
    ) -> Result<Value, AwsError> {
        let state = self.store.get(account, region);
        let name = Self::normalise_function_name(function_name);
        let ctx = RequestContext::new("lambda", region);
        let input = serde_json::json!({
            "FunctionName": name,
            "InvocationType": "RequestResponse",
            "Payload": payload,
        });
        let response = operations::invocations::invoke(&state, &input, &ctx)?;
        // The `invoke` operation surfaces a `FunctionError` field when
        // the Lambda runtime reported an error. AWS clients treat that
        // as a logical failure even though the HTTP status is 200, so
        // forward it as an `AwsError` so the caller can react.
        if let Some(err) = response.get("FunctionError").and_then(|v| v.as_str()) {
            let payload = response
                .get("Payload")
                .cloned()
                .unwrap_or(Value::Null)
                .to_string();
            return Err(AwsError::bad_request(
                "LambdaInvocationError",
                format!("Lambda {name} returned {err}: {payload}"),
            ));
        }
        Ok(response.get("Payload").cloned().unwrap_or(Value::Null))
    }
}

impl BlobInventory for LambdaService {
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (_, state) in self.store.iter_all() {
            for func_entry in state.functions.iter() {
                let name = func_entry.key().clone();
                out.push(("lambda".to_string(), name.clone(), "$LATEST".to_string()));
                for v in func_entry.value().versions.iter() {
                    out.push(("lambda".to_string(), name.clone(), v.version.clone()));
                }
            }
        }
        out
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
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-03-31/event-source-mappings/{UUID}",
                operation: "UpdateEventSourceMapping",
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
            // Reserved concurrency
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2017-10-31/functions/{FunctionName}/concurrency",
                operation: "PutFunctionConcurrency",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2019-09-30/functions/{FunctionName}/concurrency",
                operation: "GetFunctionConcurrency",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2017-10-31/functions/{FunctionName}/concurrency",
                operation: "DeleteFunctionConcurrency",
                required_query_param: None,
            },
            // Recursion (self-invoke) control.
            RouteDefinition {
                method: "GET",
                path_pattern: "/2024-08-31/functions/{FunctionName}/recursion-config",
                operation: "GetFunctionRecursionConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2024-08-31/functions/{FunctionName}/recursion-config",
                operation: "PutFunctionRecursionConfig",
                required_query_param: None,
            },
            // Provisioned concurrency — single config (Get/Put/Delete) keys
            // off the Qualifier query param; List omits it.
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2019-09-30/functions/{FunctionName}/provisioned-concurrency",
                operation: "PutProvisionedConcurrencyConfig",
                required_query_param: Some("Qualifier"),
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2019-09-30/functions/{FunctionName}/provisioned-concurrency",
                operation: "GetProvisionedConcurrencyConfig",
                required_query_param: Some("Qualifier"),
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2019-09-30/functions/{FunctionName}/provisioned-concurrency",
                operation: "DeleteProvisionedConcurrencyConfig",
                required_query_param: Some("Qualifier"),
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2019-09-30/functions/{FunctionName}/provisioned-concurrency",
                operation: "ListProvisionedConcurrencyConfigs",
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
            "UpdateAlias" => operations::aliases::update_alias(&state, &input, ctx),
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
            "UpdateEventSourceMapping" => {
                operations::event_source_mappings::update_event_source_mapping(&state, &input, ctx)
            }

            // Layers
            "PublishLayerVersion" => operations::layers::publish_layer_version(&state, &input, ctx),
            "ListLayers" => operations::layers::list_layers(&state, &input, ctx),
            "ListLayerVersions" => operations::layers::list_layer_versions(&state, &input, ctx),
            "GetLayerVersion" => operations::layers::get_layer_version(&state, &input, ctx),
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

            // Reserved + provisioned concurrency
            "PutFunctionConcurrency" => {
                operations::concurrency::put_function_concurrency(&state, &input, ctx)
            }
            "GetFunctionConcurrency" => {
                operations::concurrency::get_function_concurrency(&state, &input, ctx)
            }
            "DeleteFunctionConcurrency" => {
                operations::concurrency::delete_function_concurrency(&state, &input, ctx)
            }
            "GetFunctionRecursionConfig" => {
                operations::functions::get_function_recursion_config(&state, &input, ctx)
            }
            "PutFunctionRecursionConfig" => {
                operations::functions::put_function_recursion_config(&state, &input, ctx)
            }
            "PutProvisionedConcurrencyConfig" => {
                operations::concurrency::put_provisioned_concurrency_config(&state, &input, ctx)
            }
            "GetProvisionedConcurrencyConfig" => {
                operations::concurrency::get_provisioned_concurrency_config(&state, &input, ctx)
            }
            "DeleteProvisionedConcurrencyConfig" => {
                operations::concurrency::delete_provisioned_concurrency_config(&state, &input, ctx)
            }
            "ListProvisionedConcurrencyConfigs" => {
                operations::concurrency::list_provisioned_concurrency_configs(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.store.snapshot_to_bytes()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        use awsim_core::Snapshottable;
        use state::LambdaRegionSnapshot;

        if let Ok(()) = self.store.restore_from_bytes(data) {
            self.rebind_bodies();
            return Ok(());
        }

        let legacy: LambdaStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let mut by_region: std::collections::HashMap<(String, String), Vec<_>> =
            std::collections::HashMap::new();
        for fs in legacy.functions {
            by_region
                .entry((fs.account_id.clone(), fs.region.clone()))
                .or_default()
                .push(fs);
        }
        self.store.clear();
        for ((account_id, region), functions) in by_region {
            let snap = LambdaRegionSnapshot {
                account_id: account_id.clone(),
                region: region.clone(),
                functions,
            };
            let (acct, reg, state) = LambdaState::from_snapshot(snap);
            self.store.set(&acct, &reg, state);
        }
        self.rebind_bodies();
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
            | "ListFunctionEventInvokeConfigs"
            | "PutFunctionConcurrency"
            | "GetFunctionConcurrency"
            | "DeleteFunctionConcurrency"
            | "PutProvisionedConcurrencyConfig"
            | "GetProvisionedConcurrencyConfig"
            | "DeleteProvisionedConcurrencyConfig"
            | "ListProvisionedConcurrencyConfigs" => Some(format!("lambda:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        let prefix = format!(
            "arn:{}:lambda:{}:{}",
            ctx.partition, ctx.region, ctx.account_id
        );
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
