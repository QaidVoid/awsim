mod management;
mod models;
mod runtime;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::BedrockState;

// ── Management service (signing name: bedrock) ────────────────────────────────

pub struct BedrockService {
    store: AccountRegionStore<BedrockState>,
}

impl BedrockService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<BedrockState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for BedrockService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for BedrockService {
    fn service_name(&self) -> &str {
        "bedrock"
    }

    fn signing_name(&self) -> &str {
        "bedrock"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "GET",
                path_pattern: "/foundation-models",
                operation: "ListFoundationModels",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/foundation-models/{modelIdentifier}",
                operation: "GetFoundationModel",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model-customization-jobs",
                operation: "CreateModelCustomizationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/model-customization-jobs",
                operation: "ListModelCustomizationJobs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/guardrails",
                operation: "CreateGuardrail",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/guardrails/{guardrailIdentifier}",
                operation: "GetGuardrail",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/guardrails",
                operation: "ListGuardrails",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/guardrails/{guardrailIdentifier}",
                operation: "DeleteGuardrail",
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
        debug!(operation, "Bedrock management request");
        let state = self.get_state(ctx);

        match operation {
            "ListFoundationModels" => management::list_foundation_models(&state, &input),
            "GetFoundationModel" => management::get_foundation_model(&state, &input),
            "CreateModelCustomizationJob" => {
                management::create_model_customization_job(&state, &input, ctx)
            }
            "ListModelCustomizationJobs" => {
                management::list_model_customization_jobs(&state, &input)
            }
            "CreateGuardrail" => management::create_guardrail(&state, &input, ctx),
            "GetGuardrail" => management::get_guardrail(&state, &input),
            "ListGuardrails" => management::list_guardrails(&state, &input),
            "DeleteGuardrail" => management::delete_guardrail(&state, &input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

// ── Runtime service (signing name: bedrock-runtime) ───────────────────────────

pub struct BedrockRuntimeService;

impl BedrockRuntimeService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BedrockRuntimeService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for BedrockRuntimeService {
    fn service_name(&self) -> &str {
        "bedrock-runtime"
    }

    fn signing_name(&self) -> &str {
        "bedrock-runtime"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/model/{modelId}/invoke",
                operation: "InvokeModel",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model/{modelId}/converse",
                operation: "Converse",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        _ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "Bedrock runtime request");

        match operation {
            "InvokeModel" => runtime::invoke_model(&input),
            "Converse" => runtime::converse(&input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
