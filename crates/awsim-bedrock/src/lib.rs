mod aliases;
mod backend;
mod catalog;
mod config;
mod health;
mod management;
mod metrics;
mod model_map;
mod models;
mod runtime;
mod state;

pub use aliases::{AliasKind, AliasSpec, AliasStrategy, AliasTarget};
pub use backend::{BedrockBackend, BedrockBackends, ResolvedTarget, single_default};
pub use catalog::{AuthKind, CatalogModel, Provider, ProviderCatalog, ProviderKind, catalog};
pub use config::{
    BackendSpec, BedrockConfigError, BedrockSpec, CredentialSpec, ModelPricing, build_from_spec,
    load_from_file,
};
pub use health::{BackendHealth, BackendStatus, CheckRecord, HealthRegistry, probe, run_poller};
pub use metrics::{
    AttemptRecord, InvocationRecord, MetricsRegistry, OpKind, Outcome, RecentInvocations,
};
pub use model_map::{ModelEntry, ModelMap, ModelMapError};
pub use runtime::converse as run_converse;

use std::sync::Arc;

use arc_swap::ArcSwap;
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
            "StopModelCustomizationJob" => management::stop_model_customization_job(&state, &input),
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

/// Hot-swappable handle to a Bedrock proxy registry. Cloning the
/// handle is cheap; the underlying `Option<BedrockBackends>` can be
/// replaced atomically via [`BedrockBackendsSwap::store`] without
/// blocking request-path readers.
pub type BedrockBackendsSwap = Arc<ArcSwap<Option<BedrockBackends>>>;

/// Build an empty (canned-response) swap handle.
pub fn empty_backends_swap() -> BedrockBackendsSwap {
    Arc::new(ArcSwap::from_pointee(None))
}

/// Build a swap handle pre-populated with the given backends.
pub fn backends_swap(backends: Option<BedrockBackends>) -> BedrockBackendsSwap {
    Arc::new(ArcSwap::from_pointee(backends))
}

/// Bedrock runtime handler. Holds a hot-swappable backends registry —
/// invocations read the live registry on each call. When the swap
/// holds `None`, the service returns deterministic canned responses
/// so SDK code that just wires up the calls keeps working in CI.
pub struct BedrockRuntimeService {
    backends: BedrockBackendsSwap,
}

impl BedrockRuntimeService {
    pub fn new() -> Self {
        Self {
            backends: empty_backends_swap(),
        }
    }

    /// Construct from a hot-swappable handle. The runtime reads the
    /// live value on each request, so callers can swap backends out
    /// without touching the service.
    pub fn with_swap(backends: BedrockBackendsSwap) -> Self {
        Self { backends }
    }

    pub fn with_backends(backends: BedrockBackends) -> Self {
        Self::with_swap(backends_swap(Some(backends)))
    }

    /// Convenience for callers that have just one endpoint.
    pub fn with_backend(backend: BedrockBackend, model_map: ModelMap) -> Self {
        Self::with_backends(BedrockBackends::single(backend, model_map))
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

        // Snapshot the live registry once per request. The Guard keeps
        // the inner Arc alive for the duration of the call even if the
        // admin swaps a new one in mid-flight.
        let guard = self.backends.load();
        let snapshot = guard.as_ref().as_ref();

        match operation {
            "InvokeModel" => runtime::invoke_model(snapshot, &input).await,
            "InvokeModelWithResponseStream" => {
                runtime::invoke_model_with_response_stream(snapshot, &input).await
            }
            "Converse" => runtime::converse(snapshot, &input).await,
            "ConverseStream" => runtime::converse_stream(snapshot, &input).await,
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    async fn handle_streaming(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<awsim_core::HandlerResult, AwsError> {
        // Only the two event-stream operations override; everything
        // else delegates to the buffered `handle` path. We resolve a
        // snapshot here too so the request sees a consistent backend
        // even if the admin hot-swaps mid-stream.
        match operation {
            "ConverseStream" | "InvokeModelWithResponseStream" => {
                let backends = Arc::clone(&self.backends);
                debug!(operation, "Bedrock runtime streaming");
                runtime::stream_response(backends, operation, input).await
            }
            _ => self.handle(operation, input, ctx).await.map(Into::into),
        }
    }
}
