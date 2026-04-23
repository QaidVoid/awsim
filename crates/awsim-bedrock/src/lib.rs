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
                method: "GET",
                path_pattern: "/model-customization-jobs/{jobIdentifier}",
                operation: "GetModelCustomizationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model-customization-jobs/{jobIdentifier}/stop",
                operation: "StopModelCustomizationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/custom-models",
                operation: "ListCustomModels",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/provisioned-model-throughputs",
                operation: "ListProvisionedModelThroughputs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/provisioned-model-throughput",
                operation: "CreateProvisionedModelThroughput",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/provisioned-model-throughput/{provisionedModelId}",
                operation: "GetProvisionedModelThroughput",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/provisioned-model-throughput/{provisionedModelId}",
                operation: "DeleteProvisionedModelThroughput",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model-invocation-jobs",
                operation: "CreateModelInvocationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/model-invocation-jobs",
                operation: "ListModelInvocationJobs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/model-invocation-jobs/{jobIdentifier}",
                operation: "GetModelInvocationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model-invocation-jobs/{jobIdentifier}/stop",
                operation: "StopModelInvocationJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/custom-models/{modelIdentifier}",
                operation: "GetCustomModel",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/custom-models/{modelIdentifier}",
                operation: "DeleteCustomModel",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/knowledgebases",
                operation: "CreateKnowledgeBase",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/knowledgebases",
                operation: "ListKnowledgeBases",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/knowledgebases/{knowledgeBaseId}",
                operation: "GetKnowledgeBase",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/knowledgebases/{knowledgeBaseId}",
                operation: "DeleteKnowledgeBase",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/logging/modelinvocations",
                operation: "GetModelInvocationLoggingConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/logging/modelinvocations",
                operation: "PutModelInvocationLoggingConfiguration",
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
            RouteDefinition {
                method: "POST",
                path_pattern: "/tagResource",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/untagResource",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/listTagsForResource",
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
            "GetModelCustomizationJob" => management::get_model_customization_job(&state, &input),
            "StopModelCustomizationJob" => {
                management::stop_model_customization_job(&state, &input)
            }
            "ListCustomModels" => management::list_custom_models(&state, &input),
            "ListProvisionedModelThroughputs" => {
                management::list_provisioned_model_throughputs(&state, &input)
            }
            "CreateProvisionedModelThroughput" => {
                management::create_provisioned_model_throughput(&state, &input, ctx)
            }
            "GetProvisionedModelThroughput" => {
                management::get_provisioned_model_throughput(&state, &input)
            }
            "DeleteProvisionedModelThroughput" => {
                management::delete_provisioned_model_throughput(&state, &input)
            }
            "CreateModelInvocationJob" => {
                management::create_model_invocation_job(&state, &input, ctx)
            }
            "ListModelInvocationJobs" => management::list_model_invocation_jobs(&state, &input),
            "GetModelInvocationJob" => management::get_model_invocation_job(&state, &input),
            "StopModelInvocationJob" => management::stop_model_invocation_job(&state, &input),
            "GetCustomModel" => management::get_custom_model(&state, &input),
            "DeleteCustomModel" => management::delete_custom_model(&state, &input),
            "CreateKnowledgeBase" => management::create_knowledge_base(&state, &input, ctx),
            "GetKnowledgeBase" => management::get_knowledge_base(&state, &input),
            "ListKnowledgeBases" => management::list_knowledge_bases(&state, &input),
            "DeleteKnowledgeBase" => management::delete_knowledge_base(&state, &input),
            "GetModelInvocationLoggingConfiguration" => {
                management::get_model_invocation_logging_configuration(&state, &input)
            }
            "PutModelInvocationLoggingConfiguration" => {
                management::put_model_invocation_logging_configuration(&state, &input)
            }
            "CreateGuardrail" => management::create_guardrail(&state, &input, ctx),
            "GetGuardrail" => management::get_guardrail(&state, &input),
            "ListGuardrails" => management::list_guardrails(&state, &input),
            "DeleteGuardrail" => management::delete_guardrail(&state, &input),
            "TagResource" => management::tag_resource(&state, &input),
            "UntagResource" => management::untag_resource(&state, &input),
            "ListTagsForResource" => management::list_tags_for_resource(&state, &input),
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
                path_pattern: "/model/{modelId}/invoke-with-response-stream",
                operation: "InvokeModelWithResponseStream",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model/{modelId}/converse",
                operation: "Converse",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/model/{modelId}/converse-stream",
                operation: "ConverseStream",
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
            "InvokeModelWithResponseStream" => {
                runtime::invoke_model_with_response_stream(&input)
            }
            "Converse" => runtime::converse(&input),
            "ConverseStream" => runtime::converse_stream(&input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
