//! API Gateway v1 — REST APIs.
//!
//! AWS exposes two distinct API Gateway services:
//!   * v1 (REST APIs) — signed as `apigateway`, paths like `/restapis/...`,
//!     resource-tree model with per-resource `Method`s and `Integration`s.
//!   * v2 (HTTP / WebSocket APIs) — signed as `execute-api`, paths like
//!     `/v2/apis/...`, simpler routes-and-integrations model.
//!
//! `ApiGatewayService` (in `lib.rs`) handles v2. This module adds a
//! parallel handler for v1 so the management UI (which is built on REST
//! APIs) stops returning `UnknownService`. The two handlers share no
//! state — they really are two different services in AWS too.
//!
//! Scope is intentionally tight: every operation the UI client calls is
//! covered, plus enough surface for create/list/delete to round-trip.
//! Higher-fidelity REST APIs work (request/response models, integration
//! responses, API keys, usage plans, etc.) is out of scope for now.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use dashmap::DashMap;
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RestApi {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub created_date: u64,
    pub api_key_source: String,
    pub endpoint_types: Vec<String>,
    /// Endpoints attached to this API when `endpoint_types` contains
    /// `PRIVATE`; AWS requires interface VPC endpoint IDs to be
    /// surfaced verbatim in describe responses.
    pub vpc_endpoint_ids: Vec<String>,
    /// Content types treated as binary on the request/response path.
    /// Persisted as supplied and echoed back from GetRestApi.
    pub binary_media_types: Vec<String>,
    /// Smallest response body (in bytes) that triggers gzip; absent
    /// disables compression.
    pub minimum_compression_size: Option<u32>,
    /// Resource id -> Resource. Always contains a root `/` resource.
    pub resources: HashMap<String, Resource>,
    pub stages: HashMap<String, Stage>,
    pub deployments: Vec<Deployment>,
    pub authorizers: HashMap<String, Authorizer>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub id: String,
    pub parent_id: String,
    pub path_part: String,
    pub path: String,
    /// HTTP method -> Method config. Empty until the user attaches one.
    pub methods: HashMap<String, Method>,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub http_method: String,
    pub authorization_type: String,
    pub authorizer_id: String,
    pub api_key_required: bool,
    pub request_parameters: HashMap<String, bool>,
    pub integration: Option<Integration>,
}

#[derive(Debug, Clone)]
pub struct Integration {
    pub r#type: String,
    pub http_method: String,
    pub uri: String,
    pub connection_type: String,
    pub passthrough_behavior: String,
    pub timeout_in_millis: u32,
    pub cache_namespace: String,
    /// Request body mapping templates keyed by content-type.
    /// Used for non-proxy integrations (AWS, HTTP, MOCK).
    pub request_templates: HashMap<String, String>,
    /// Integration responses keyed by status code (`"200"`, `"default"`, ...).
    pub integration_responses: HashMap<String, IntegrationResponse>,
}

#[derive(Debug, Clone)]
pub struct IntegrationResponse {
    pub status_code: String,
    /// Regex on the integration's raw output that picks this response.
    /// Empty string means "default" — used when no other pattern matches.
    pub selection_pattern: String,
    /// Response body mapping templates keyed by content-type.
    pub response_templates: HashMap<String, String>,
    /// Response header mappings (header-name → source expression).
    pub response_parameters: HashMap<String, String>,
    /// `CONVERT_TO_BINARY` decodes a base64-encoded body before sending
    /// to the client; `CONVERT_TO_TEXT` base64-encodes a binary body.
    /// Absent means pass-through.
    pub content_handling: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub stage_name: String,
    pub deployment_id: String,
    pub description: String,
    pub cache_cluster_enabled: bool,
    pub created_date: u64,
    pub last_updated_date: u64,
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Deployment {
    pub id: String,
    pub description: String,
    pub created_date: u64,
}

#[derive(Debug, Clone)]
pub struct Authorizer {
    pub id: String,
    pub name: String,
    pub r#type: String,
    pub auth_type: String,
    pub authorizer_uri: String,
    pub identity_source: String,
    /// Seconds an Allow / Deny decision is cached for. AWS' default is
    /// 300; 0 disables caching.
    pub result_ttl_in_seconds: u32,
    /// Cognito User Pool ARNs the authorizer trusts. Populated for
    /// COGNITO_USER_POOLS authorizers; ignored for the others.
    pub provider_arns: Vec<String>,
    /// Regex applied to the resolved identity (TOKEN authorizers) before
    /// invoking the Lambda. A non-matching value yields 401 without
    /// invoking the upstream, matching AWS's pre-flight validation.
    pub identity_validation_expression: Option<String>,
}

/// API key as stored. The `value` is the bearer string the SDK sends in
/// `x-api-key`; the `id` is a short opaque handle the management API
/// uses. Keys exist at the account+region level — not under any one API.
#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: String,
    pub value: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub customer_id: String,
    pub created_date: u64,
    pub last_updated_date: u64,
    /// Stage associations recorded at creation time as `"apiId/stage"`.
    /// Real AWS uses these to seed CloudWatch dimensions; we only store
    /// them for round-trip fidelity.
    pub stage_keys: Vec<String>,
}

/// Usage plan grouping keys that share quota / throttle limits and a
/// list of API stages they're allowed to call.
#[derive(Debug, Clone)]
pub struct UsagePlan {
    pub id: String,
    pub name: String,
    pub description: String,
    pub api_stages: Vec<UsagePlanApiStage>,
    pub throttle: Option<UsageThrottle>,
    pub quota: Option<UsageQuota>,
}

#[derive(Debug, Clone)]
pub struct UsagePlanApiStage {
    pub api_id: String,
    pub stage: String,
}

#[derive(Debug, Clone, Default)]
pub struct UsageThrottle {
    pub rate_limit: f64,
    pub burst_limit: u32,
}

#[derive(Debug, Clone, Default)]
pub struct UsageQuota {
    pub limit: u32,
    pub period: String,
    pub offset: u32,
}

/// Edge linking an `ApiKey` (by id) to a `UsagePlan` (by id). One ApiKey
/// can belong to multiple plans; one plan holds many keys.
#[derive(Debug, Clone)]
pub struct UsagePlanKey {
    pub id: String,
    pub key_id: String,
    pub key_type: String,
    pub usage_plan_id: String,
}

#[derive(Default)]
pub struct ApiGatewayV1State {
    pub apis: DashMap<String, RestApi>,
    /// Cache of authorizer decisions, keyed by `(authorizer_id, identity)`.
    pub authorizer_cache: crate::authorizer::AuthorizerCache,
    /// API keys keyed by their opaque `id`.
    pub api_keys: DashMap<String, ApiKey>,
    /// Usage plans keyed by id.
    pub usage_plans: DashMap<String, UsagePlan>,
    /// Edges from a usage plan to the keys it covers, keyed by
    /// `"{usage_plan_id}/{key_id}"`.
    pub usage_plan_keys: DashMap<String, UsagePlanKey>,
}

pub struct ApiGatewayV1Service {
    store: AccountRegionStore<ApiGatewayV1State>,
}

impl ApiGatewayV1Service {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<ApiGatewayV1State> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Expose the underlying state store for proxy routing in main.rs.
    pub fn store(&self) -> &AccountRegionStore<ApiGatewayV1State> {
        &self.store
    }
}

impl Default for ApiGatewayV1Service {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for ApiGatewayV1Service {
    fn service_name(&self) -> &str {
        "apigateway"
    }

    fn signing_name(&self) -> &str {
        "apigateway"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // REST APIs
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis",
                operation: "GetRestApis",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/restapis",
                operation: "CreateRestApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}",
                operation: "GetRestApi",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}",
                operation: "DeleteRestApi",
                required_query_param: None,
            },
            // Resources
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources",
                operation: "GetResources",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/restapis/{restapi_id}/resources/{parent_id}",
                operation: "CreateResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}",
                operation: "DeleteResource",
                required_query_param: None,
            },
            // Methods + integrations (per-resource)
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}",
                operation: "GetMethod",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}",
                operation: "PutMethod",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}",
                operation: "DeleteMethod",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration",
                operation: "GetIntegration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration",
                operation: "PutIntegration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration",
                operation: "DeleteIntegration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration/responses/{status_code}",
                operation: "GetIntegrationResponse",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration/responses/{status_code}",
                operation: "PutIntegrationResponse",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration/responses/{status_code}",
                operation: "DeleteIntegrationResponse",
                required_query_param: None,
            },
            // Stages
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/stages",
                operation: "GetStages",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/restapis/{restapi_id}/stages",
                operation: "CreateStage",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/stages/{stage_name}",
                operation: "DeleteStage",
                required_query_param: None,
            },
            // Deployments
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/deployments",
                operation: "GetDeployments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/restapis/{restapi_id}/deployments",
                operation: "CreateDeployment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/deployments/{deployment_id}",
                operation: "DeleteDeployment",
                required_query_param: None,
            },
            // Authorizers
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/authorizers",
                operation: "GetAuthorizers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/restapis/{restapi_id}/authorizers",
                operation: "CreateAuthorizer",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/restapis/{restapi_id}/authorizers/{authorizer_id}",
                operation: "DeleteAuthorizer",
                required_query_param: None,
            },
            // API keys
            RouteDefinition {
                method: "POST",
                path_pattern: "/apikeys",
                operation: "CreateApiKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/apikeys",
                operation: "GetApiKeys",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/apikeys/{api_key}",
                operation: "GetApiKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/apikeys/{api_key}",
                operation: "DeleteApiKey",
                required_query_param: None,
            },
            // Usage plans
            RouteDefinition {
                method: "POST",
                path_pattern: "/usageplans",
                operation: "CreateUsagePlan",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/usageplans",
                operation: "GetUsagePlans",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/usageplans/{usageplanId}",
                operation: "GetUsagePlan",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/usageplans/{usageplanId}",
                operation: "DeleteUsagePlan",
                required_query_param: None,
            },
            // Usage plan keys
            RouteDefinition {
                method: "POST",
                path_pattern: "/usageplans/{usageplanId}/keys",
                operation: "CreateUsagePlanKey",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/usageplans/{usageplanId}/keys",
                operation: "GetUsagePlanKeys",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/usageplans/{usageplanId}/keys/{keyId}",
                operation: "DeleteUsagePlanKey",
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
        debug!(operation, "API Gateway v1 request");
        let state = self.get_state(ctx);

        match operation {
            "GetRestApis" => Ok(get_rest_apis(&state)),
            "CreateRestApi" => create_rest_api(&state, &input),
            "GetRestApi" => get_rest_api(&state, &input),
            "DeleteRestApi" => delete_rest_api(&state, &input),
            "GetResources" => get_resources(&state, &input),
            "CreateResource" => create_resource(&state, &input),
            "DeleteResource" => delete_resource(&state, &input),
            "GetMethod" => get_method(&state, &input),
            "PutMethod" => put_method(&state, &input),
            "DeleteMethod" => delete_method(&state, &input),
            "GetIntegration" => get_integration(&state, &input),
            "PutIntegration" => put_integration(&state, &input),
            "DeleteIntegration" => delete_integration(&state, &input),
            "GetIntegrationResponse" => get_integration_response(&state, &input),
            "PutIntegrationResponse" => put_integration_response(&state, &input),
            "DeleteIntegrationResponse" => delete_integration_response(&state, &input),
            "GetStages" => get_stages(&state, &input),
            "CreateStage" => create_stage(&state, &input),
            "DeleteStage" => delete_stage(&state, &input),
            "GetDeployments" => get_deployments(&state, &input),
            "CreateDeployment" => create_deployment(&state, &input),
            "DeleteDeployment" => delete_deployment(&state, &input),
            "GetAuthorizers" => get_authorizers(&state, &input),
            "CreateAuthorizer" => create_authorizer(&state, &input),
            "DeleteAuthorizer" => delete_authorizer(&state, &input),
            "CreateApiKey" => create_api_key(&state, &input),
            "GetApiKey" => get_api_key(&state, &input),
            "GetApiKeys" => get_api_keys(&state, &input),
            "DeleteApiKey" => delete_api_key(&state, &input),
            "CreateUsagePlan" => create_usage_plan(&state, &input),
            "GetUsagePlan" => get_usage_plan(&state, &input),
            "GetUsagePlans" => Ok(get_usage_plans(&state)),
            "DeleteUsagePlan" => delete_usage_plan(&state, &input),
            "CreateUsagePlanKey" => create_usage_plan_key(&state, &input),
            "GetUsagePlanKeys" => get_usage_plan_keys(&state, &input),
            "DeleteUsagePlanKey" => delete_usage_plan_key(&state, &input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rest_api_id() -> String {
    Uuid::new_v4()
        .to_string()
        .replace('-', "")
        .chars()
        .take(10)
        .collect()
}

fn root_resource() -> Resource {
    Resource {
        id: Uuid::new_v4()
            .to_string()
            .replace('-', "")
            .chars()
            .take(10)
            .collect(),
        parent_id: String::new(),
        path_part: String::new(),
        path: "/".to_string(),
        methods: HashMap::new(),
    }
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input[key]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("Missing {key}")))
}

fn rest_api_to_json(api: &RestApi) -> Value {
    let mut endpoint_cfg = json!({ "types": api.endpoint_types });
    if !api.vpc_endpoint_ids.is_empty() {
        endpoint_cfg["vpcEndpointIds"] = json!(api.vpc_endpoint_ids);
    }
    let mut obj = json!({
        "id": api.id,
        "name": api.name,
        "description": api.description,
        "createdDate": api.created_date,
        "version": api.version,
        "apiKeySource": api.api_key_source,
        "endpointConfiguration": endpoint_cfg,
        "binaryMediaTypes": api.binary_media_types,
    });
    if let Some(size) = api.minimum_compression_size {
        obj["minimumCompressionSize"] = json!(size);
    }
    obj
}

fn resource_to_json(r: &Resource) -> Value {
    let mut methods_obj = serde_json::Map::new();
    for name in r.methods.keys() {
        methods_obj.insert(name.clone(), json!({}));
    }
    json!({
        "id": r.id,
        "parentId": r.parent_id,
        "pathPart": r.path_part,
        "path": r.path,
        "resourceMethods": methods_obj,
    })
}

fn method_to_json(m: &Method) -> Value {
    let integration = m
        .integration
        .as_ref()
        .map(integration_to_json)
        .unwrap_or(Value::Null);
    json!({
        "httpMethod": m.http_method,
        "authorizationType": m.authorization_type,
        "authorizerId": m.authorizer_id,
        "apiKeyRequired": m.api_key_required,
        "requestParameters": m.request_parameters,
        "methodIntegration": integration,
    })
}

fn integration_to_json(i: &Integration) -> Value {
    let responses: serde_json::Map<String, Value> = i
        .integration_responses
        .iter()
        .map(|(k, v)| (k.clone(), integration_response_to_json(v)))
        .collect();
    json!({
        "type": i.r#type,
        "httpMethod": i.http_method,
        "uri": i.uri,
        "connectionType": i.connection_type,
        "passthroughBehavior": i.passthrough_behavior,
        "timeoutInMillis": i.timeout_in_millis,
        "cacheNamespace": i.cache_namespace,
        "requestTemplates": i.request_templates,
        "integrationResponses": Value::Object(responses),
    })
}

fn integration_response_to_json(r: &IntegrationResponse) -> Value {
    let mut obj = json!({
        "statusCode": r.status_code,
        "selectionPattern": r.selection_pattern,
        "responseTemplates": r.response_templates,
        "responseParameters": r.response_parameters,
    });
    if let Some(ref ch) = r.content_handling {
        obj["contentHandling"] = json!(ch);
    }
    obj
}

/// Apply the `contentHandling` rule to an integration response body.
/// AWS treats the bytes as opaque payload; `CONVERT_TO_BINARY` base64-
/// decodes the input string so binary clients receive raw bytes, while
/// `CONVERT_TO_TEXT` base64-encodes raw bytes for text clients. Missing
/// or unset values yield the input unchanged. On decode failure the
/// original bytes pass through, matching AWS's lenient behavior.
pub fn apply_response_content_handling(body: &[u8], content_handling: Option<&str>) -> Vec<u8> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    match content_handling {
        Some("CONVERT_TO_BINARY") => match std::str::from_utf8(body) {
            Ok(s) => STANDARD.decode(s.trim()).unwrap_or_else(|_| body.to_vec()),
            Err(_) => body.to_vec(),
        },
        Some("CONVERT_TO_TEXT") => STANDARD.encode(body).into_bytes(),
        _ => body.to_vec(),
    }
}

fn stage_to_json(s: &Stage) -> Value {
    json!({
        "stageName": s.stage_name,
        "deploymentId": s.deployment_id,
        "description": s.description,
        "cacheClusterEnabled": s.cache_cluster_enabled,
        "createdDate": s.created_date,
        "lastUpdatedDate": s.last_updated_date,
        "variables": s.variables,
    })
}

fn deployment_to_json(d: &Deployment) -> Value {
    json!({
        "id": d.id,
        "description": d.description,
        "createdDate": d.created_date,
    })
}

fn authorizer_to_json(a: &Authorizer) -> Value {
    let mut obj = json!({
        "id": a.id,
        "name": a.name,
        "type": a.r#type,
        "authType": a.auth_type,
        "authorizerUri": a.authorizer_uri,
        "identitySource": a.identity_source,
        "authorizerResultTtlInSeconds": a.result_ttl_in_seconds,
        "providerARNs": a.provider_arns,
    });
    if let Some(ref expr) = a.identity_validation_expression {
        obj["identityValidationExpression"] = json!(expr);
    }
    obj
}

fn get_rest_apis(state: &ApiGatewayV1State) -> Value {
    let mut items: Vec<Value> = state
        .apis
        .iter()
        .map(|e| rest_api_to_json(e.value()))
        .collect();
    items.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    json!({ "items": items })
}

fn create_rest_api(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "name")?.to_string();
    let description = input["description"].as_str().unwrap_or("").to_string();
    let id = rest_api_id();
    let root = root_resource();
    let mut resources = HashMap::new();
    resources.insert(root.id.clone(), root);

    let (endpoint_types, vpc_endpoint_ids) = parse_endpoint_configuration(input)?;
    let binary_media_types = input["binaryMediaTypes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let minimum_compression_size = input["minimumCompressionSize"].as_u64().map(|n| n as u32);
    let api_key_source = input["apiKeySource"]
        .as_str()
        .unwrap_or("HEADER")
        .to_string();
    if !matches!(api_key_source.as_str(), "HEADER" | "AUTHORIZER") {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("apiKeySource `{api_key_source}` must be HEADER or AUTHORIZER."),
        ));
    }

    let api = RestApi {
        id: id.clone(),
        name,
        description,
        version: "2015-07-09".to_string(),
        created_date: now_epoch(),
        api_key_source,
        endpoint_types,
        vpc_endpoint_ids,
        binary_media_types,
        minimum_compression_size,
        resources,
        stages: HashMap::new(),
        deployments: Vec::new(),
        authorizers: HashMap::new(),
    };
    state.apis.insert(id.clone(), api.clone());
    Ok(rest_api_to_json(&api))
}

/// Parse + validate `endpointConfiguration` on CreateRestApi /
/// UpdateRestApi. AWS allows zero or more `types` from
/// REGIONAL/EDGE/PRIVATE; PRIVATE is the only type that may carry
/// `vpcEndpointIds`. Defaults to `["REGIONAL"]` when omitted.
fn parse_endpoint_configuration(input: &Value) -> Result<(Vec<String>, Vec<String>), AwsError> {
    let Some(cfg) = input.get("endpointConfiguration") else {
        return Ok((vec!["REGIONAL".to_string()], Vec::new()));
    };
    if cfg.is_null() {
        return Ok((vec!["REGIONAL".to_string()], Vec::new()));
    }
    let obj = cfg.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            "endpointConfiguration must be an object.",
        )
    })?;
    let types: Vec<String> = obj
        .get("types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| vec!["REGIONAL".to_string()]);
    for ty in &types {
        if !matches!(ty.as_str(), "REGIONAL" | "EDGE" | "PRIVATE") {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!(
                    "endpointConfiguration.types entry `{ty}` must be REGIONAL, EDGE, or PRIVATE."
                ),
            ));
        }
    }
    let vpc_endpoint_ids: Vec<String> = obj
        .get("vpcEndpointIds")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if !vpc_endpoint_ids.is_empty() && !types.iter().any(|t| t == "PRIVATE") {
        return Err(AwsError::bad_request(
            "BadRequestException",
            "endpointConfiguration.vpcEndpointIds is only allowed when types includes PRIVATE.",
        ));
    }
    Ok((types, vpc_endpoint_ids))
}

fn get_rest_api(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    state
        .apis
        .get(id)
        .map(|e| rest_api_to_json(e.value()))
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("RestApi {id} not found")))
}

fn delete_rest_api(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    if state.apis.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("RestApi {id} not found"),
        ));
    }
    Ok(json!({}))
}

fn get_resources(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    let api = state.apis.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    let mut items: Vec<Value> = api.resources.values().map(resource_to_json).collect();
    items.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });
    Ok(json!({ "items": items }))
}

fn get_method(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?;
    let resource_id = require_str(input, "resource_id")?;
    let http_method = require_str(input, "http_method")?;
    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {api_id} not found"))
    })?;
    let resource = api.resources.get(resource_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Resource {resource_id} not found"),
        )
    })?;
    let method = resource.methods.get(http_method).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Method {http_method} not configured"),
        )
    })?;
    Ok(method_to_json(method))
}

fn get_integration(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let method_json = get_method(state, input)?;
    if method_json["methodIntegration"].is_null() {
        return Err(AwsError::not_found(
            "NotFoundException",
            "No integration configured for method",
        ));
    }
    Ok(method_json["methodIntegration"].clone())
}

fn get_stages(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    let api = state.apis.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    let item: Vec<Value> = api.stages.values().map(stage_to_json).collect();
    // Note: AWS returns the stages array under the key `item`, not `items`.
    Ok(json!({ "item": item }))
}

fn get_deployments(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    let api = state.apis.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    let items: Vec<Value> = api.deployments.iter().map(deployment_to_json).collect();
    Ok(json!({ "items": items }))
}

fn create_deployment(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    let stage_name = input["stageName"].as_str().map(str::to_string);
    let description = input["description"].as_str().unwrap_or("").to_string();

    let mut entry = state.apis.get_mut(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    let api = entry.value_mut();
    let deployment = Deployment {
        id: Uuid::new_v4()
            .to_string()
            .replace('-', "")
            .chars()
            .take(10)
            .collect(),
        description,
        created_date: now_epoch(),
    };
    api.deployments.push(deployment.clone());

    if let Some(name) = stage_name {
        api.stages.insert(
            name.clone(),
            Stage {
                stage_name: name,
                deployment_id: deployment.id.clone(),
                description: String::new(),
                cache_cluster_enabled: false,
                created_date: now_epoch(),
                last_updated_date: now_epoch(),
                variables: HashMap::new(),
            },
        );
    }

    Ok(deployment_to_json(&deployment))
}

fn get_authorizers(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "restapi_id")?;
    let api = state.apis.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    let items: Vec<Value> = api.authorizers.values().map(authorizer_to_json).collect();
    Ok(json!({ "items": items }))
}

// --- Mutating operations -------------------------------------------------

fn short_id() -> String {
    Uuid::new_v4()
        .to_string()
        .replace('-', "")
        .chars()
        .take(10)
        .collect()
}

fn with_api_mut<F, R>(state: &ApiGatewayV1State, id: &str, f: F) -> Result<R, AwsError>
where
    F: FnOnce(&mut RestApi) -> Result<R, AwsError>,
{
    let mut entry = state.apis.get_mut(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("RestApi {id} not found"))
    })?;
    f(entry.value_mut())
}

fn create_resource(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let parent_id = require_str(input, "parent_id")?.to_string();
    let path_part = require_str(input, "pathPart")?.to_string();
    if path_part.is_empty() {
        return Err(AwsError::bad_request(
            "BadRequestException",
            "pathPart must be non-empty",
        ));
    }
    with_api_mut(state, &api_id, |api| {
        let parent = api.resources.get(&parent_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Parent resource {parent_id} not found"),
            )
        })?;
        let parent_path = parent.path.clone();
        // Reject duplicates at the same parent + same pathPart.
        if api
            .resources
            .values()
            .any(|r| r.parent_id == parent_id && r.path_part == path_part)
        {
            return Err(AwsError::conflict(
                "ConflictException",
                format!("Resource {path_part} already exists under parent {parent_id}"),
            ));
        }
        let new_path = if parent_path == "/" {
            format!("/{path_part}")
        } else {
            format!("{parent_path}/{path_part}")
        };
        let resource = Resource {
            id: short_id(),
            parent_id,
            path_part,
            path: new_path,
            methods: HashMap::new(),
        };
        let json = resource_to_json(&resource);
        api.resources.insert(resource.id.clone(), resource);
        Ok(json)
    })
}

fn delete_resource(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    with_api_mut(state, &api_id, |api| {
        let target = api.resources.get(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        if target.path == "/" {
            return Err(AwsError::bad_request(
                "BadRequestException",
                "Cannot delete the root resource",
            ));
        }
        // Refuse if any other resource lists this one as parent — keeps the
        // tree consistent and matches the AWS behaviour.
        if api.resources.values().any(|r| r.parent_id == resource_id) {
            return Err(AwsError::conflict(
                "ConflictException",
                "Resource has children; delete those first",
            ));
        }
        api.resources.remove(&resource_id);
        Ok(json!({}))
    })
}

fn put_method(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    let auth_type = input["authorizationType"]
        .as_str()
        .unwrap_or("NONE")
        .to_string();
    let authorizer_id = input["authorizerId"].as_str().unwrap_or("").to_string();
    let api_key_required = input["apiKeyRequired"].as_bool().unwrap_or(false);
    let request_parameters = input["requestParameters"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                .collect()
        })
        .unwrap_or_default();

    with_api_mut(state, &api_id, |api| {
        let resource = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = Method {
            http_method: http_method.clone(),
            authorization_type: auth_type,
            authorizer_id,
            api_key_required,
            request_parameters,
            integration: None,
        };
        let json = method_to_json(&method);
        resource.methods.insert(http_method, method);
        Ok(json)
    })
}

fn delete_method(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    with_api_mut(state, &api_id, |api| {
        let resource = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        if resource.methods.remove(&http_method).is_none() {
            return Err(AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured"),
            ));
        }
        Ok(json!({}))
    })
}

fn put_integration(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    let integration = Integration {
        r#type: input["type"].as_str().unwrap_or("AWS_PROXY").to_string(),
        http_method: input["httpMethod"]
            .as_str()
            .unwrap_or(&http_method)
            .to_string(),
        uri: input["uri"].as_str().unwrap_or("").to_string(),
        connection_type: input["connectionType"]
            .as_str()
            .unwrap_or("INTERNET")
            .to_string(),
        passthrough_behavior: input["passthroughBehavior"]
            .as_str()
            .unwrap_or("WHEN_NO_MATCH")
            .to_string(),
        timeout_in_millis: input["timeoutInMillis"].as_u64().unwrap_or(29000) as u32,
        cache_namespace: input["cacheNamespace"].as_str().unwrap_or("").to_string(),
        request_templates: parse_template_map(&input["requestTemplates"]),
        integration_responses: HashMap::new(),
    };
    with_api_mut(state, &api_id, |api| {
        let resource = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = resource.methods.get_mut(&http_method).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured — call PutMethod first"),
            )
        })?;
        let json = integration_to_json(&integration);
        method.integration = Some(integration);
        Ok(json)
    })
}

fn parse_template_map(v: &Value) -> HashMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn get_integration_response(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    let status_code = require_str(input, "status_code")?.to_string();
    with_api_mut(state, &api_id, |api| {
        let r = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = r.methods.get_mut(&http_method).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured"),
            )
        })?;
        let integration = method
            .integration
            .as_mut()
            .ok_or_else(|| AwsError::not_found("NotFoundException", "No integration configured"))?;
        integration
            .integration_responses
            .get(&status_code)
            .map(integration_response_to_json)
            .ok_or_else(|| {
                AwsError::not_found(
                    "NotFoundException",
                    format!("IntegrationResponse {status_code} not found"),
                )
            })
    })
}

fn put_integration_response(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    let status_code = require_str(input, "status_code")?.to_string();
    let content_handling = input["contentHandling"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(ref ch) = content_handling
        && !matches!(ch.as_str(), "CONVERT_TO_BINARY" | "CONVERT_TO_TEXT")
    {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("contentHandling '{ch}' must be CONVERT_TO_BINARY or CONVERT_TO_TEXT"),
        ));
    }
    let response = IntegrationResponse {
        status_code: status_code.clone(),
        selection_pattern: input["selectionPattern"].as_str().unwrap_or("").to_string(),
        response_templates: parse_template_map(&input["responseTemplates"]),
        response_parameters: parse_template_map(&input["responseParameters"]),
        content_handling,
    };
    with_api_mut(state, &api_id, |api| {
        let r = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = r.methods.get_mut(&http_method).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured"),
            )
        })?;
        let integration = method
            .integration
            .as_mut()
            .ok_or_else(|| AwsError::not_found("NotFoundException", "No integration configured"))?;
        let json = integration_response_to_json(&response);
        integration
            .integration_responses
            .insert(status_code, response);
        Ok(json)
    })
}

fn delete_integration_response(
    state: &ApiGatewayV1State,
    input: &Value,
) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    let status_code = require_str(input, "status_code")?.to_string();
    with_api_mut(state, &api_id, |api| {
        let r = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = r.methods.get_mut(&http_method).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured"),
            )
        })?;
        let integration = method
            .integration
            .as_mut()
            .ok_or_else(|| AwsError::not_found("NotFoundException", "No integration configured"))?;
        integration
            .integration_responses
            .remove(&status_code)
            .ok_or_else(|| {
                AwsError::not_found(
                    "NotFoundException",
                    format!("IntegrationResponse {status_code} not found"),
                )
            })?;
        Ok(json!({}))
    })
}

fn delete_integration(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let resource_id = require_str(input, "resource_id")?.to_string();
    let http_method = require_str(input, "http_method")?.to_uppercase();
    with_api_mut(state, &api_id, |api| {
        let resource = api.resources.get_mut(&resource_id).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resource {resource_id} not found"),
            )
        })?;
        let method = resource.methods.get_mut(&http_method).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Method {http_method} not configured"),
            )
        })?;
        if method.integration.take().is_none() {
            return Err(AwsError::not_found(
                "NotFoundException",
                "No integration configured",
            ));
        }
        Ok(json!({}))
    })
}

fn create_stage(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let stage_name = require_str(input, "stageName")?.to_string();
    let deployment_id = require_str(input, "deploymentId")?.to_string();
    let description = input["description"].as_str().unwrap_or("").to_string();
    let cache_cluster_enabled = input["cacheClusterEnabled"].as_bool().unwrap_or(false);
    let variables = input["variables"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    with_api_mut(state, &api_id, |api| {
        if !api.deployments.iter().any(|d| d.id == deployment_id) {
            return Err(AwsError::not_found(
                "NotFoundException",
                format!("Deployment {deployment_id} not found"),
            ));
        }
        if api.stages.contains_key(&stage_name) {
            return Err(AwsError::conflict(
                "ConflictException",
                format!("Stage {stage_name} already exists"),
            ));
        }
        let stage = Stage {
            stage_name: stage_name.clone(),
            deployment_id,
            description,
            cache_cluster_enabled,
            created_date: now_epoch(),
            last_updated_date: now_epoch(),
            variables,
        };
        let json = stage_to_json(&stage);
        api.stages.insert(stage_name, stage);
        Ok(json)
    })
}

fn delete_stage(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let stage_name = require_str(input, "stage_name")?.to_string();
    with_api_mut(state, &api_id, |api| {
        if api.stages.remove(&stage_name).is_none() {
            return Err(AwsError::not_found(
                "NotFoundException",
                format!("Stage {stage_name} not found"),
            ));
        }
        Ok(json!({}))
    })
}

fn delete_deployment(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let deployment_id = require_str(input, "deployment_id")?.to_string();
    with_api_mut(state, &api_id, |api| {
        // AWS rejects deletion when any stage still points at this deployment.
        if api
            .stages
            .values()
            .any(|s| s.deployment_id == deployment_id)
        {
            return Err(AwsError::conflict(
                "ConflictException",
                "Deployment is still referenced by one or more stages",
            ));
        }
        let before = api.deployments.len();
        api.deployments.retain(|d| d.id != deployment_id);
        if api.deployments.len() == before {
            return Err(AwsError::not_found(
                "NotFoundException",
                format!("Deployment {deployment_id} not found"),
            ));
        }
        Ok(json!({}))
    })
}

fn create_authorizer(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let name = require_str(input, "name")?.to_string();
    let r#type = input["type"].as_str().unwrap_or("TOKEN").to_string();
    let auth_type = input["authType"].as_str().unwrap_or("custom").to_string();
    let authorizer_uri = input["authorizerUri"].as_str().unwrap_or("").to_string();
    let identity_source = input["identitySource"]
        .as_str()
        .unwrap_or("method.request.header.Authorization")
        .to_string();
    let result_ttl_in_seconds = input["authorizerResultTtlInSeconds"]
        .as_u64()
        .map(|v| v as u32)
        .unwrap_or(300);
    let provider_arns = input["providerARNs"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let identity_validation_expression = input["identityValidationExpression"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(ref expr) = identity_validation_expression {
        regex::Regex::new(expr).map_err(|e| {
            AwsError::bad_request(
                "BadRequestException",
                format!("identityValidationExpression `{expr}` is not a valid regex: {e}"),
            )
        })?;
    }

    with_api_mut(state, &api_id, |api| {
        let authorizer = Authorizer {
            id: short_id(),
            name,
            r#type,
            auth_type,
            authorizer_uri,
            identity_source,
            result_ttl_in_seconds,
            provider_arns,
            identity_validation_expression,
        };
        let json = authorizer_to_json(&authorizer);
        api.authorizers.insert(authorizer.id.clone(), authorizer);
        Ok(json)
    })
}

fn delete_authorizer(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let api_id = require_str(input, "restapi_id")?.to_string();
    let authorizer_id = require_str(input, "authorizer_id")?.to_string();
    with_api_mut(state, &api_id, |api| {
        if api.authorizers.remove(&authorizer_id).is_none() {
            return Err(AwsError::not_found(
                "NotFoundException",
                format!("Authorizer {authorizer_id} not found"),
            ));
        }
        Ok(json!({}))
    })
}

// --- API keys + usage plans ---------------------------------------------

fn api_key_to_json(k: &ApiKey) -> Value {
    json!({
        "id": k.id,
        "value": k.value,
        "name": k.name,
        "description": k.description,
        "enabled": k.enabled,
        "customerId": k.customer_id,
        "createdDate": k.created_date,
        "lastUpdatedDate": k.last_updated_date,
        "stageKeys": k.stage_keys,
    })
}

fn usage_plan_to_json(p: &UsagePlan) -> Value {
    let stages: Vec<Value> = p
        .api_stages
        .iter()
        .map(|s| json!({"apiId": s.api_id, "stage": s.stage}))
        .collect();
    let mut obj = json!({
        "id": p.id,
        "name": p.name,
        "description": p.description,
        "apiStages": stages,
    });
    if let Some(t) = &p.throttle
        && let Some(map) = obj.as_object_mut()
    {
        map.insert(
            "throttle".to_string(),
            json!({
                "rateLimit": t.rate_limit,
                "burstLimit": t.burst_limit,
            }),
        );
    }
    if let Some(q) = &p.quota
        && let Some(map) = obj.as_object_mut()
    {
        map.insert(
            "quota".to_string(),
            json!({
                "limit": q.limit,
                "period": q.period,
                "offset": q.offset,
            }),
        );
    }
    obj
}

fn usage_plan_key_to_json(k: &UsagePlanKey) -> Value {
    json!({
        "id": k.id,
        "type": k.key_type,
        "value": k.id, // AWS surfaces the underlying key id here
    })
}

/// Generate a 40-char alphanumeric API key value, matching AWS' format.
fn generate_api_key_value() -> String {
    let raw = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    raw.chars().take(40).collect()
}

fn create_api_key(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let name = input["name"].as_str().unwrap_or("").to_string();
    let description = input["description"].as_str().unwrap_or("").to_string();
    let enabled = input["enabled"].as_bool().unwrap_or(true);
    let customer_id = input["customerId"].as_str().unwrap_or("").to_string();
    let provided_value = input["value"].as_str().map(String::from);
    let stage_keys = input["stageKeys"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let restapi_id = v["restApiId"].as_str()?;
                    let stage = v["stageName"].as_str()?;
                    Some(format!("{restapi_id}/{stage}"))
                })
                .collect()
        })
        .unwrap_or_default();
    let id = short_id();
    let now = now_epoch();
    let key = ApiKey {
        id: id.clone(),
        value: provided_value.unwrap_or_else(generate_api_key_value),
        name,
        description,
        enabled,
        customer_id,
        created_date: now,
        last_updated_date: now,
        stage_keys,
    };
    let json = api_key_to_json(&key);
    state.api_keys.insert(id, key);
    Ok(json)
}

fn get_api_key(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "api_key")?;
    state
        .api_keys
        .get(id)
        .map(|e| api_key_to_json(e.value()))
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("ApiKey {id} not found")))
}

fn get_api_keys(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let include_values = input["includeValues"].as_bool().unwrap_or(false);
    let mut items: Vec<Value> = state
        .api_keys
        .iter()
        .map(|e| {
            let mut v = api_key_to_json(e.value());
            if !include_values && let Some(map) = v.as_object_mut() {
                map.remove("value");
            }
            v
        })
        .collect();
    items.sort_by(|a, b| {
        a["createdDate"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&b["createdDate"].as_u64().unwrap_or(0))
    });
    Ok(json!({"items": items}))
}

fn delete_api_key(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "api_key")?;
    if state.api_keys.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("ApiKey {id} not found"),
        ));
    }
    state.usage_plan_keys.retain(|_, edge| edge.key_id != id);
    Ok(json!({}))
}

fn create_usage_plan(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "name")?.to_string();
    let description = input["description"].as_str().unwrap_or("").to_string();
    let api_stages = input["apiStages"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(UsagePlanApiStage {
                        api_id: v["apiId"].as_str()?.to_string(),
                        stage: v["stage"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let throttle = input["throttle"].as_object().map(|t| UsageThrottle {
        rate_limit: t.get("rateLimit").and_then(Value::as_f64).unwrap_or(0.0),
        burst_limit: t.get("burstLimit").and_then(Value::as_u64).unwrap_or(0) as u32,
    });
    let quota = input["quota"].as_object().map(|q| UsageQuota {
        limit: q.get("limit").and_then(Value::as_u64).unwrap_or(0) as u32,
        period: q
            .get("period")
            .and_then(Value::as_str)
            .unwrap_or("MONTH")
            .to_string(),
        offset: q.get("offset").and_then(Value::as_u64).unwrap_or(0) as u32,
    });

    let id = short_id();
    let plan = UsagePlan {
        id: id.clone(),
        name,
        description,
        api_stages,
        throttle,
        quota,
    };
    let json = usage_plan_to_json(&plan);
    state.usage_plans.insert(id, plan);
    Ok(json)
}

fn get_usage_plan(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "usageplanId")?;
    state
        .usage_plans
        .get(id)
        .map(|e| usage_plan_to_json(e.value()))
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("UsagePlan {id} not found"))
        })
}

fn get_usage_plans(state: &ApiGatewayV1State) -> Value {
    let mut items: Vec<Value> = state
        .usage_plans
        .iter()
        .map(|e| usage_plan_to_json(e.value()))
        .collect();
    items.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    json!({"items": items})
}

fn delete_usage_plan(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let id = require_str(input, "usageplanId")?;
    if state.usage_plans.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("UsagePlan {id} not found"),
        ));
    }
    state
        .usage_plan_keys
        .retain(|_, edge| edge.usage_plan_id != id);
    Ok(json!({}))
}

fn create_usage_plan_key(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let usage_plan_id = require_str(input, "usageplanId")?.to_string();
    let key_id = require_str(input, "keyId")?.to_string();
    let key_type = input["keyType"].as_str().unwrap_or("API_KEY").to_string();
    if !state.usage_plans.contains_key(&usage_plan_id) {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("UsagePlan {usage_plan_id} not found"),
        ));
    }
    if !state.api_keys.contains_key(&key_id) {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("ApiKey {key_id} not found"),
        ));
    }
    let edge_id = format!("{usage_plan_id}/{key_id}");
    let edge = UsagePlanKey {
        id: key_id.clone(),
        key_id,
        key_type,
        usage_plan_id,
    };
    let json = usage_plan_key_to_json(&edge);
    state.usage_plan_keys.insert(edge_id, edge);
    Ok(json)
}

fn get_usage_plan_keys(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let usage_plan_id = require_str(input, "usageplanId")?.to_string();
    let items: Vec<Value> = state
        .usage_plan_keys
        .iter()
        .filter(|e| e.value().usage_plan_id == usage_plan_id)
        .map(|e| usage_plan_key_to_json(e.value()))
        .collect();
    Ok(json!({"items": items}))
}

fn delete_usage_plan_key(state: &ApiGatewayV1State, input: &Value) -> Result<Value, AwsError> {
    let usage_plan_id = require_str(input, "usageplanId")?.to_string();
    let key_id = require_str(input, "keyId")?.to_string();
    let edge_id = format!("{usage_plan_id}/{key_id}");
    if state.usage_plan_keys.remove(&edge_id).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("UsagePlanKey {key_id} not found in plan {usage_plan_id}"),
        ));
    }
    Ok(json!({}))
}

/// Look up an `x-api-key` header against the configured ApiKeys + their
/// linked UsagePlans. Returns Ok if the key is enabled and at least one
/// usage plan covers `(api_id, stage)`. Modeled after AWS' enforcement —
/// throttle / quota aren't tracked yet so any matching plan grants
/// access.
pub fn validate_api_key(
    state: &ApiGatewayV1State,
    api_key_value: Option<&str>,
    api_id: &str,
    stage: &str,
) -> Result<(), &'static str> {
    let value = api_key_value.ok_or("Missing x-api-key header")?;
    if value.is_empty() {
        return Err("Missing x-api-key header");
    }
    let key = state
        .api_keys
        .iter()
        .find(|e| e.value().value == value)
        .ok_or("Invalid API key")?;
    if !key.value().enabled {
        return Err("API key is disabled");
    }
    let key_id = key.value().id.clone();
    drop(key);

    let covered = state.usage_plan_keys.iter().any(|edge| {
        if edge.value().key_id != key_id {
            return false;
        }
        state
            .usage_plans
            .get(&edge.value().usage_plan_id)
            .map(|plan| {
                plan.value()
                    .api_stages
                    .iter()
                    .any(|s| s.api_id == api_id && s.stage == stage)
            })
            .unwrap_or(false)
    });
    if !covered {
        return Err("API key is not associated with a usage plan covering this API stage");
    }
    Ok(())
}

// --- Stage proxy routing -------------------------------------------------

/// Result of matching a stage-invocation request against a REST API's
/// configured resources + methods. The caller (the binary's proxy
/// handler) consumes this to dispatch by integration type.
pub struct V1ProxyMatch {
    pub integration_type: String,
    pub integration_uri: String,
    /// Lambda-style v1 proxy event (`apiGateway1.0` payload format).
    /// The caller can pass this verbatim as the Lambda Invoke `Payload`.
    pub event: Value,
    /// Path of the matched resource — used purely for diagnostics.
    pub matched_resource_path: String,
    /// Full integration record so the caller can run request/response
    /// template mapping for non-PROXY integrations.
    pub integration: Integration,
    /// Stage variables — exposed so non-PROXY templates can resolve
    /// `$stageVariables.x`.
    pub stage_variables: HashMap<String, String>,
    /// Path parameters extracted from the matched resource pattern.
    pub path_params: HashMap<String, String>,
    /// Query parameters parsed from the URL.
    pub query_params: HashMap<String, String>,
    /// Request headers as a string→string map.
    pub headers: HashMap<String, String>,
    /// `requestContext` object — same shape as the proxy event sub-object.
    pub request_context: Value,
    /// What the caller must do for authorization before running the
    /// integration. May be `NotConfigured` (run integration unchanged),
    /// `Allowed` (cache hit / Cognito), `InvokeLambda` (caller invokes),
    /// or a short-circuit `Unauthorized` / `Forbidden`.
    pub authorization: crate::authorizer::AuthorizationStep,
}

/// Look up a stage-invocation against the REST API's resources, returning
/// enough info for the caller to dispatch the request to the right
/// integration.
///
/// Path matching supports `{param}` placeholders. Returns `None` when:
///   * the API doesn't exist, or
///   * no resource path matches, or
///   * the matched resource has no method for the HTTP verb, or
///   * the matched method has no integration attached.
// SAFETY: each parameter is a distinct piece of the incoming HTTP
// request needed to build the API Gateway event payload.
#[allow(clippy::too_many_arguments)]
pub fn proxy_request(
    state: &Arc<ApiGatewayV1State>,
    api_id: &str,
    stage: &str,
    method: &str,
    path: &str,
    query_string: &str,
    headers: &std::collections::HashMap<String, String>,
    body: &[u8],
    account_id: &str,
    region: &str,
) -> Option<V1ProxyMatch> {
    let api = state.apis.get(api_id)?;
    let resource = match_resource(&api.resources, path)?;
    let http_method_upper = method.to_uppercase();
    let m = resource
        .methods
        .get(&http_method_upper)
        .or_else(|| resource.methods.get("ANY"))?;
    let integration = m.integration.as_ref()?;

    let body_str = std::str::from_utf8(body).ok().map(|s| s.to_string());
    let path_params_value = extract_path_params(&resource.path, path);
    let query_params_value = parse_query_params(query_string);
    let path_params_map = json_string_map(&path_params_value);
    let query_params_map = json_string_map(&query_params_value);
    let stage_vars = api
        .stages
        .get(stage)
        .map(|s| s.variables.clone())
        .unwrap_or_default();
    let stage_vars_value = if stage_vars.is_empty() {
        Value::Null
    } else {
        Value::Object(
            stage_vars
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect(),
        )
    };

    let request_context = json!({
        "apiId": api_id,
        "httpMethod": http_method_upper,
        "path": path,
        "stage": stage,
        "requestId": Uuid::new_v4().to_string(),
        "identity": {
            "sourceIp": "127.0.0.1",
        },
    });

    let event = json!({
        "resource": resource.path,
        "path": path,
        "httpMethod": http_method_upper,
        "headers": headers,
        "queryStringParameters": query_params_value,
        "pathParameters": path_params_value,
        "stageVariables": stage_vars_value,
        "requestContext": request_context,
        "body": body_str,
        "isBase64Encoded": false,
    });

    let authorization = if m.api_key_required {
        let api_key_value = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
            .map(|(_, v)| v.as_str());
        match validate_api_key(state, api_key_value, api_id, stage) {
            Ok(()) => crate::authorizer::evaluate(
                &state.authorizer_cache,
                m,
                &api.authorizers,
                headers,
                &path_params_map,
                &query_params_map,
                &stage_vars,
                &request_context,
                region,
                account_id,
                api_id,
                stage,
                &http_method_upper,
                path,
            ),
            Err(reason) => crate::authorizer::AuthorizationStep::Forbidden(reason.to_string()),
        }
    } else {
        crate::authorizer::evaluate(
            &state.authorizer_cache,
            m,
            &api.authorizers,
            headers,
            &path_params_map,
            &query_params_map,
            &stage_vars,
            &request_context,
            region,
            account_id,
            api_id,
            stage,
            &http_method_upper,
            path,
        )
    };

    Some(V1ProxyMatch {
        integration_type: integration.r#type.clone(),
        integration_uri: interpolate_stage_variables(&integration.uri, &stage_vars),
        event,
        matched_resource_path: resource.path.clone(),
        integration: integration.clone(),
        stage_variables: stage_vars,
        path_params: path_params_map,
        query_params: query_params_map,
        headers: headers.clone(),
        request_context,
        authorization,
    })
}

/// Replace `${stageVariables.foo}` and `$stageVariables.foo` occurrences
/// with the named stage variable. AWS does this substitution before
/// dispatching to the integration; missing variables collapse to an
/// empty string, matching AWS's behavior of failing-soft when a stage
/// variable is not defined.
pub fn interpolate_stage_variables(template: &str, stage_vars: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    // ${stageVariables.foo}
    while let Some(start) = out.find("${stageVariables.") {
        let after = start + "${stageVariables.".len();
        let Some(rel_end) = out[after..].find('}') else {
            break;
        };
        let end = after + rel_end;
        let key = &out[after..end];
        let value = stage_vars.get(key).cloned().unwrap_or_default();
        out.replace_range(start..=end, &value);
    }
    // $stageVariables.foo (only when the next char would terminate an identifier)
    let mut search = 0;
    while let Some(found) = out[search..].find("$stageVariables.") {
        let start = search + found;
        let after = start + "$stageVariables.".len();
        let key_end = out[after..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map(|n| after + n)
            .unwrap_or(out.len());
        if key_end == after {
            search = after;
            continue;
        }
        let key = out[after..key_end].to_string();
        let value = stage_vars.get(&key).cloned().unwrap_or_default();
        out.replace_range(start..key_end, &value);
        search = start + value.len();
    }
    out
}

fn json_string_map(v: &Value) -> HashMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn match_resource<'a>(
    resources: &'a HashMap<String, Resource>,
    path: &str,
) -> Option<&'a Resource> {
    // Exact-path match wins over a path-with-params match (so a resource
    // configured at `/users/me` shadows `/users/{id}` when the user hits
    // `/users/me`).
    let mut param_match: Option<&Resource> = None;
    for r in resources.values() {
        if r.path == path {
            return Some(r);
        }
        if path_matches(&r.path, path) {
            param_match = Some(r);
        }
    }
    param_match
}

/// True when `pattern_segment` is a `{name+}` greedy capture.
fn is_greedy_segment(seg: &str) -> bool {
    seg.starts_with('{') && seg.ends_with("+}")
}

/// True when `pattern_segment` is a normal `{name}` capture.
fn is_named_segment(seg: &str) -> bool {
    seg.starts_with('{') && seg.ends_with('}') && !seg.ends_with("+}")
}

fn path_matches(pattern: &str, actual: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let act: Vec<&str> = actual.split('/').collect();
    let mut pi = 0usize;
    let mut ai = 0usize;
    while pi < pat.len() {
        let p = pat[pi];
        if is_greedy_segment(p) {
            // Greedy capture must be the final segment in the pattern
            // and matches every remaining segment of the actual path.
            return pi == pat.len() - 1 && ai <= act.len();
        }
        if ai >= act.len() {
            return false;
        }
        if !is_named_segment(p) && p != act[ai] {
            return false;
        }
        pi += 1;
        ai += 1;
    }
    ai == act.len()
}

fn extract_path_params(pattern: &str, actual: &str) -> Value {
    let pat: Vec<&str> = pattern.split('/').collect();
    let act: Vec<&str> = actual.split('/').collect();
    let mut out = serde_json::Map::new();
    let mut ai = 0usize;
    for (pi, p) in pat.iter().enumerate() {
        if is_greedy_segment(p) {
            // Greedy capture: name without the trailing `+`. Joins the
            // rest of the actual path's segments back together with `/`.
            let name = &p[1..p.len() - 2];
            let tail: Vec<&str> = act.iter().skip(pi).copied().collect();
            out.insert(name.to_string(), Value::String(tail.join("/")));
            ai = act.len();
            break;
        }
        if ai >= act.len() {
            return Value::Null;
        }
        if is_named_segment(p) {
            let name = &p[1..p.len() - 1];
            out.insert(name.to_string(), Value::String(act[ai].to_string()));
        }
        ai += 1;
    }
    if ai != act.len() {
        return Value::Null;
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
}

fn parse_query_params(qs: &str) -> Value {
    if qs.is_empty() {
        return Value::Null;
    }
    let mut out = serde_json::Map::new();
    for pair in qs.split('&') {
        let mut parts = pair.splitn(2, '=');
        let Some(k) = parts.next() else { continue };
        let v = parts.next().unwrap_or("").to_string();
        out.insert(k.to_string(), Value::String(v));
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
}

/// Result of evaluating an API Gateway response against the per-API
/// `minimumCompressionSize` setting. AWS gzips the response body only
/// when all of (a) the API configures a non-negative size, (b) the body
/// is at least that many bytes, and (c) the caller's `Accept-Encoding`
/// header contains `gzip`. Otherwise the body passes through untouched.
pub struct CompressedResponse {
    pub body: Vec<u8>,
    pub content_encoding: Option<&'static str>,
}

pub fn maybe_compress_response(
    api: &RestApi,
    body: &[u8],
    accept_encoding: Option<&str>,
) -> CompressedResponse {
    let Some(min) = api.minimum_compression_size else {
        return passthrough(body);
    };
    if body.len() < min as usize {
        return passthrough(body);
    }
    if !accept_encoding
        .unwrap_or("")
        .split(',')
        .any(|enc| enc.trim().eq_ignore_ascii_case("gzip"))
    {
        return passthrough(body);
    }
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::with_capacity(body.len()), Compression::default());
    if enc.write_all(body).is_err() {
        return passthrough(body);
    }
    match enc.finish() {
        Ok(out) => CompressedResponse {
            body: out,
            content_encoding: Some("gzip"),
        },
        Err(_) => passthrough(body),
    }
}

fn passthrough(body: &[u8]) -> CompressedResponse {
    CompressedResponse {
        body: body.to_vec(),
        content_encoding: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest_api_with_compression(min: Option<u32>) -> RestApi {
        RestApi {
            id: "api1".into(),
            name: "n".into(),
            description: String::new(),
            version: String::new(),
            created_date: 0,
            api_key_source: "HEADER".into(),
            endpoint_types: vec![],
            vpc_endpoint_ids: vec![],
            binary_media_types: vec![],
            minimum_compression_size: min,
            resources: HashMap::new(),
            stages: HashMap::new(),
            deployments: vec![],
            authorizers: HashMap::new(),
        }
    }

    #[test]
    fn content_handling_convert_to_binary_decodes_base64() {
        let out = apply_response_content_handling(b"aGVsbG8=", Some("CONVERT_TO_BINARY"));
        assert_eq!(out, b"hello");
    }

    #[test]
    fn content_handling_convert_to_text_base64_encodes_bytes() {
        let out = apply_response_content_handling(&[0x01, 0x02, 0x03], Some("CONVERT_TO_TEXT"));
        assert_eq!(out, b"AQID");
    }

    #[test]
    fn content_handling_missing_passes_through() {
        let out = apply_response_content_handling(b"hello", None);
        assert_eq!(out, b"hello");
    }

    #[test]
    fn content_handling_invalid_base64_falls_through() {
        // CONVERT_TO_BINARY on something that isn't valid base64 should
        // return the original bytes rather than panicking — matches AWS.
        let out = apply_response_content_handling(b"not-base64!!", Some("CONVERT_TO_BINARY"));
        assert_eq!(out, b"not-base64!!");
    }

    #[test]
    fn maybe_compress_gzips_when_threshold_met_and_accept_encoding_supports_gzip() {
        let api = rest_api_with_compression(Some(8));
        let body = b"abcdefghij".repeat(20);
        let out = maybe_compress_response(&api, &body, Some("gzip, deflate"));
        assert_eq!(out.content_encoding, Some("gzip"));
        assert!(out.body.len() < body.len());
        assert_eq!(&out.body[..2], &[0x1f, 0x8b]);
    }

    #[test]
    fn maybe_compress_skips_when_body_smaller_than_threshold() {
        let api = rest_api_with_compression(Some(1024));
        let body = b"hi".to_vec();
        let out = maybe_compress_response(&api, &body, Some("gzip"));
        assert!(out.content_encoding.is_none());
        assert_eq!(out.body, body);
    }

    #[test]
    fn maybe_compress_skips_when_client_does_not_accept_gzip() {
        let api = rest_api_with_compression(Some(8));
        let body = b"abcdefghij".repeat(20);
        let out = maybe_compress_response(&api, &body, Some("br"));
        assert!(out.content_encoding.is_none());
        assert_eq!(out.body, body);
    }

    #[test]
    fn maybe_compress_skips_when_min_compression_size_unset() {
        let api = rest_api_with_compression(None);
        let body = b"abcdefghij".repeat(50);
        let out = maybe_compress_response(&api, &body, Some("gzip"));
        assert!(out.content_encoding.is_none());
    }

    #[test]
    fn interpolate_stage_variables_handles_braced_and_unbraced() {
        let mut vars = HashMap::new();
        vars.insert("host".to_string(), "api.example.com".to_string());
        vars.insert("path".to_string(), "v1".to_string());
        assert_eq!(
            interpolate_stage_variables("https://${stageVariables.host}/x", &vars),
            "https://api.example.com/x"
        );
        assert_eq!(
            interpolate_stage_variables("$stageVariables.path/items", &vars),
            "v1/items"
        );
        assert_eq!(
            interpolate_stage_variables("$stageVariables.missing/end", &vars),
            "/end"
        );
    }

    fn ctx() -> RequestContext {
        RequestContext {
            account_id: "000000000000".to_string(),
            region: "us-east-1".to_string(),
            service: "apigateway".to_string(),
            access_key: None,
            request_id: "test".to_string(),
            method: "GET".to_string(),
            uri: "/".to_string(),
            event_bus: None,
            ..Default::default()
        }
    }

    #[test]
    fn path_matches_exact_and_named_captures() {
        assert!(path_matches("/users", "/users"));
        assert!(path_matches("/users/{id}", "/users/42"));
        assert!(!path_matches("/users", "/users/42"));
        assert!(!path_matches("/users/{id}", "/users/42/orders"));
    }

    #[test]
    fn path_matches_greedy_proxy_captures_remaining_segments() {
        assert!(path_matches("/api/{proxy+}", "/api/users"));
        assert!(path_matches("/api/{proxy+}", "/api/users/42"));
        assert!(path_matches("/api/{proxy+}", "/api/users/42/orders/abc"));
        // Pattern prefix doesn't match → no greedy save.
        assert!(!path_matches("/api/{proxy+}", "/other/users"));
    }

    #[test]
    fn extract_path_params_includes_greedy_value() {
        let params = extract_path_params("/api/{proxy+}", "/api/users/42/orders");
        assert_eq!(params["proxy"], "users/42/orders");

        let params = extract_path_params("/users/{id}/files/{path+}", "/users/42/files/a/b/c.txt");
        assert_eq!(params["id"], "42");
        assert_eq!(params["path"], "a/b/c.txt");
    }

    #[test]
    fn extract_path_params_returns_null_for_unmatched_pattern() {
        // Without greedy + segment count differs → no match.
        let params = extract_path_params("/users/{id}", "/users/42/extra");
        assert_eq!(params, Value::Null);
    }

    #[tokio::test]
    async fn create_then_list_round_trips() {
        let svc = ApiGatewayV1Service::new();
        let created = svc
            .handle("CreateRestApi", json!({"name": "demo"}), &ctx())
            .await
            .unwrap();
        assert_eq!(created["name"], "demo");

        let listed = svc.handle("GetRestApis", json!({}), &ctx()).await.unwrap();
        let items = listed["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "demo");
    }

    #[tokio::test]
    async fn create_rest_api_persists_binary_media_and_endpoint_config() {
        let svc = ApiGatewayV1Service::new();
        let created = svc
            .handle(
                "CreateRestApi",
                json!({
                    "name": "demo",
                    "binaryMediaTypes": ["application/octet-stream", "image/*"],
                    "minimumCompressionSize": 1024,
                    "endpointConfiguration": {
                        "types": ["PRIVATE"],
                        "vpcEndpointIds": ["vpce-1234"],
                    },
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let got = svc
            .handle("GetRestApi", json!({"restapi_id": id}), &ctx())
            .await
            .unwrap();
        assert_eq!(got["binaryMediaTypes"][0], "application/octet-stream");
        assert_eq!(got["minimumCompressionSize"], 1024);
        assert_eq!(got["endpointConfiguration"]["types"][0], "PRIVATE");
        assert_eq!(
            got["endpointConfiguration"]["vpcEndpointIds"][0],
            "vpce-1234"
        );
    }

    #[tokio::test]
    async fn create_rest_api_rejects_invalid_endpoint_type() {
        let svc = ApiGatewayV1Service::new();
        let err = svc
            .handle(
                "CreateRestApi",
                json!({
                    "name": "bad",
                    "endpointConfiguration": { "types": ["GLOBAL"] },
                }),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[tokio::test]
    async fn create_rest_api_rejects_vpc_endpoints_without_private_type() {
        let svc = ApiGatewayV1Service::new();
        let err = svc
            .handle(
                "CreateRestApi",
                json!({
                    "name": "bad",
                    "endpointConfiguration": {
                        "types": ["REGIONAL"],
                        "vpcEndpointIds": ["vpce-1"],
                    },
                }),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[tokio::test]
    async fn newly_created_api_has_root_resource() {
        let svc = ApiGatewayV1Service::new();
        let created = svc
            .handle("CreateRestApi", json!({"name": "demo"}), &ctx())
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let resources = svc
            .handle("GetResources", json!({"restapi_id": id}), &ctx())
            .await
            .unwrap();
        let items = resources["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["path"], "/");
        assert_eq!(items[0]["parentId"], "");
    }

    #[tokio::test]
    async fn delete_removes_api() {
        let svc = ApiGatewayV1Service::new();
        let created = svc
            .handle("CreateRestApi", json!({"name": "demo"}), &ctx())
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        svc.handle("DeleteRestApi", json!({"restapi_id": id.clone()}), &ctx())
            .await
            .unwrap();
        let listed = svc.handle("GetRestApis", json!({}), &ctx()).await.unwrap();
        assert_eq!(listed["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_deployment_with_stage() {
        let svc = ApiGatewayV1Service::new();
        let created = svc
            .handle("CreateRestApi", json!({"name": "demo"}), &ctx())
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        svc.handle(
            "CreateDeployment",
            json!({"restapi_id": id.clone(), "stageName": "prod", "description": "first"}),
            &ctx(),
        )
        .await
        .unwrap();

        let stages = svc
            .handle("GetStages", json!({"restapi_id": id.clone()}), &ctx())
            .await
            .unwrap();
        let stage_items = stages["item"].as_array().unwrap();
        assert_eq!(stage_items.len(), 1);
        assert_eq!(stage_items[0]["stageName"], "prod");

        let deployments = svc
            .handle("GetDeployments", json!({"restapi_id": id}), &ctx())
            .await
            .unwrap();
        assert_eq!(deployments["items"].as_array().unwrap().len(), 1);
    }

    async fn make_api(svc: &ApiGatewayV1Service) -> (String, String) {
        let api = svc
            .handle("CreateRestApi", json!({"name": "demo"}), &ctx())
            .await
            .unwrap();
        let api_id = api["id"].as_str().unwrap().to_string();
        let resources = svc
            .handle(
                "GetResources",
                json!({"restapi_id": api_id.clone()}),
                &ctx(),
            )
            .await
            .unwrap();
        let root_id = resources["items"][0]["id"].as_str().unwrap().to_string();
        (api_id, root_id)
    }

    #[tokio::test]
    async fn create_resource_appends_path_to_parent() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        let users = svc
            .handle(
                "CreateResource",
                json!({"restapi_id": api_id, "parent_id": root_id, "pathPart": "users"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(users["path"], "/users");
        assert_eq!(users["pathPart"], "users");
    }

    #[tokio::test]
    async fn create_resource_rejects_duplicates_under_same_parent() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "CreateResource",
            json!({"restapi_id": api_id.clone(), "parent_id": root_id.clone(), "pathPart": "users"}),
            &ctx(),
        )
        .await
        .unwrap();
        let err = svc
            .handle(
                "CreateResource",
                json!({"restapi_id": api_id, "parent_id": root_id, "pathPart": "users"}),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "ConflictException");
    }

    #[tokio::test]
    async fn delete_resource_refuses_root() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        let err = svc
            .handle(
                "DeleteResource",
                json!({"restapi_id": api_id, "resource_id": root_id}),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[tokio::test]
    async fn put_method_then_put_integration_then_get() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "authorizationType": "NONE",
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "type": "MOCK",
                "uri": "",
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let method = svc
            .handle(
                "GetMethod",
                json!({"restapi_id": api_id, "resource_id": root_id, "http_method": "GET"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(method["httpMethod"], "GET");
        assert_eq!(method["methodIntegration"]["type"], "MOCK");
    }

    #[tokio::test]
    async fn delete_deployment_refuses_when_referenced_by_stage() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, _root) = make_api(&svc).await;
        let dep = svc
            .handle(
                "CreateDeployment",
                json!({"restapi_id": api_id.clone(), "stageName": "prod"}),
                &ctx(),
            )
            .await
            .unwrap();
        let dep_id = dep["id"].as_str().unwrap().to_string();
        let err = svc
            .handle(
                "DeleteDeployment",
                json!({"restapi_id": api_id.clone(), "deployment_id": dep_id.clone()}),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "ConflictException");

        // After deleting the stage, deployment delete succeeds.
        svc.handle(
            "DeleteStage",
            json!({"restapi_id": api_id.clone(), "stage_name": "prod"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "DeleteDeployment",
            json!({"restapi_id": api_id, "deployment_id": dep_id}),
            &ctx(),
        )
        .await
        .unwrap();
    }

    #[test]
    fn path_matches_handles_params_and_exact() {
        assert!(path_matches("/users", "/users"));
        assert!(!path_matches("/users", "/posts"));
        assert!(path_matches("/users/{id}", "/users/42"));
        assert!(!path_matches("/users/{id}", "/users/42/extra"));
        assert!(path_matches("/u/{id}/c/{cid}", "/u/1/c/2"));
    }

    #[test]
    fn extract_path_params_pulls_named_segments() {
        let v = extract_path_params("/u/{id}/c/{cid}", "/u/1/c/2");
        assert_eq!(v["id"], "1");
        assert_eq!(v["cid"], "2");
    }

    #[tokio::test]
    async fn proxy_match_resolves_method_and_integration() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root) = make_api(&svc).await;
        let users = svc
            .handle(
                "CreateResource",
                json!({"restapi_id": api_id.clone(), "parent_id": root, "pathPart": "users"}),
                &ctx(),
            )
            .await
            .unwrap();
        let users_id = users["id"].as_str().unwrap().to_string();
        let by_id = svc
            .handle(
                "CreateResource",
                json!({"restapi_id": api_id.clone(), "parent_id": users_id, "pathPart": "{id}"}),
                &ctx(),
            )
            .await
            .unwrap();
        let by_id_rid = by_id["id"].as_str().unwrap().to_string();
        svc.handle(
            "PutMethod",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": by_id_rid.clone(),
                "http_method": "GET",
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": by_id_rid,
                "http_method": "GET",
                "type": "MOCK",
            }),
            &ctx(),
        )
        .await
        .unwrap();

        let store = svc.store.get("000000000000", "us-east-1");
        let m = proxy_request(
            &store,
            &api_id,
            "prod",
            "GET",
            "/users/42",
            "",
            &HashMap::new(),
            &[],
            "000000000000",
            "us-east-1",
        )
        .expect("match");
        assert_eq!(m.matched_resource_path, "/users/{id}");
        assert_eq!(m.integration_type, "MOCK");
        assert_eq!(m.event["pathParameters"]["id"], "42");
    }

    #[tokio::test]
    async fn create_authorizer_round_trips() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, _root) = make_api(&svc).await;
        let auth = svc
            .handle(
                "CreateAuthorizer",
                json!({
                    "restapi_id": api_id.clone(),
                    "name": "lambda-auth",
                    "type": "REQUEST",
                    "authorizerUri": "arn:aws:lambda:us-east-1:000:function:auth",
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let auth_id = auth["id"].as_str().unwrap().to_string();

        let listed = svc
            .handle(
                "GetAuthorizers",
                json!({"restapi_id": api_id.clone()}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(listed["items"].as_array().unwrap().len(), 1);

        svc.handle(
            "DeleteAuthorizer",
            json!({"restapi_id": api_id.clone(), "authorizer_id": auth_id}),
            &ctx(),
        )
        .await
        .unwrap();
        let listed = svc
            .handle("GetAuthorizers", json!({"restapi_id": api_id}), &ctx())
            .await
            .unwrap();
        assert_eq!(listed["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn put_integration_round_trips_request_templates() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "type": "MOCK",
                "requestTemplates": {
                    "application/json": "{\"statusCode\": 200}"
                }
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let got = svc
            .handle(
                "GetIntegration",
                json!({"restapi_id": api_id, "resource_id": root_id, "http_method": "GET"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(
            got["requestTemplates"]["application/json"],
            "{\"statusCode\": 200}"
        );
    }

    #[tokio::test]
    async fn put_integration_response_round_trips() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "type": "MOCK"
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegrationResponse",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "status_code": "200",
                "selectionPattern": "",
                "responseTemplates": {
                    "application/json": "{\"ok\":true}"
                }
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let got = svc
            .handle(
                "GetIntegrationResponse",
                json!({
                    "restapi_id": api_id,
                    "resource_id": root_id,
                    "http_method": "GET",
                    "status_code": "200"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(got["statusCode"], "200");
        assert_eq!(
            got["responseTemplates"]["application/json"],
            "{\"ok\":true}"
        );
    }

    #[tokio::test]
    async fn put_integration_response_persists_content_handling() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "type": "MOCK"
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegrationResponse",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id.clone(),
                "http_method": "GET",
                "status_code": "200",
                "contentHandling": "CONVERT_TO_BINARY",
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let got = svc
            .handle(
                "GetIntegrationResponse",
                json!({
                    "restapi_id": api_id,
                    "resource_id": root_id,
                    "http_method": "GET",
                    "status_code": "200"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(got["contentHandling"], "CONVERT_TO_BINARY");
    }

    #[tokio::test]
    async fn put_integration_response_rejects_unknown_content_handling() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id,
                "http_method": "GET",
                "type": "MOCK"
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let err = svc
            .handle(
                "PutIntegrationResponse",
                json!({
                    "restapi_id": api_id,
                    "resource_id": "root",
                    "http_method": "GET",
                    "status_code": "200",
                    "contentHandling": "MAGIC",
                }),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[tokio::test]
    async fn proxy_match_includes_stage_variables() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id,
                "http_method": "GET",
                "type": "MOCK"
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let dep = svc
            .handle(
                "CreateDeployment",
                json!({"restapi_id": api_id.clone(), "stageName": "prod"}),
                &ctx(),
            )
            .await
            .unwrap();
        // CreateDeployment with stageName auto-creates the stage.
        let _ = dep;
        // Set stage variables via internal mutation, since UpdateStage isn't
        // implemented — walk the state directly.
        {
            let store = svc.store.get("000000000000", "us-east-1");
            let mut entry = store.apis.get_mut(&api_id).unwrap();
            let stage = entry.value_mut().stages.get_mut("prod").unwrap();
            stage.variables.insert("env".into(), "live".into());
        }
        let store = svc.store.get("000000000000", "us-east-1");
        let m = proxy_request(
            &store,
            &api_id,
            "prod",
            "GET",
            "/",
            "",
            &HashMap::new(),
            &[],
            "000000000000",
            "us-east-1",
        )
        .expect("match");
        assert_eq!(
            m.stage_variables.get("env").map(String::as_str),
            Some("live")
        );
        assert_eq!(m.event["stageVariables"]["env"], "live");
    }

    #[tokio::test]
    async fn proxy_match_interpolates_stage_variables_into_integration_uri() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, root_id) = make_api(&svc).await;
        svc.handle(
            "PutMethod",
            json!({"restapi_id": api_id.clone(), "resource_id": root_id.clone(), "http_method": "GET"}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutIntegration",
            json!({
                "restapi_id": api_id.clone(),
                "resource_id": root_id,
                "http_method": "GET",
                "type": "HTTP",
                "uri": "https://${stageVariables.host}/items/$stageVariables.path",
                "integrationHttpMethod": "GET"
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "CreateDeployment",
            json!({"restapi_id": api_id.clone(), "stageName": "prod"}),
            &ctx(),
        )
        .await
        .unwrap();
        {
            let store = svc.store.get("000000000000", "us-east-1");
            let mut entry = store.apis.get_mut(&api_id).unwrap();
            let stage = entry.value_mut().stages.get_mut("prod").unwrap();
            stage
                .variables
                .insert("host".into(), "api.example.com".into());
            stage.variables.insert("path".into(), "v2".into());
        }
        let store = svc.store.get("000000000000", "us-east-1");
        let m = proxy_request(
            &store,
            &api_id,
            "prod",
            "GET",
            "/",
            "",
            &HashMap::new(),
            &[],
            "000000000000",
            "us-east-1",
        )
        .expect("match");
        assert_eq!(m.integration_uri, "https://api.example.com/items/v2");
    }

    #[tokio::test]
    async fn create_api_key_round_trips_and_lists() {
        let svc = ApiGatewayV1Service::new();
        let resp = svc
            .handle(
                "CreateApiKey",
                json!({"name": "k1", "enabled": true}),
                &ctx(),
            )
            .await
            .unwrap();
        let id = resp["id"].as_str().unwrap().to_string();
        let raw_value = resp["value"].as_str().unwrap();
        assert_eq!(raw_value.len(), 40);

        // Default GetApiKeys hides the value.
        let listed = svc.handle("GetApiKeys", json!({}), &ctx()).await.unwrap();
        let item = &listed["items"][0];
        assert_eq!(item["id"], id);
        assert!(item["value"].is_null());

        let with_values = svc
            .handle("GetApiKeys", json!({"includeValues": true}), &ctx())
            .await
            .unwrap();
        assert_eq!(with_values["items"][0]["value"], raw_value);
    }

    #[tokio::test]
    async fn validate_api_key_requires_usage_plan_for_stage() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, _root) = make_api(&svc).await;
        // Make the stage exist so we have something to bind the plan to.
        svc.handle(
            "CreateDeployment",
            json!({"restapi_id": api_id.clone(), "stageName": "prod"}),
            &ctx(),
        )
        .await
        .unwrap();

        let key = svc
            .handle("CreateApiKey", json!({"name": "k1"}), &ctx())
            .await
            .unwrap();
        let key_id = key["id"].as_str().unwrap().to_string();
        let key_value = key["value"].as_str().unwrap().to_string();

        let store = svc.store.get("000000000000", "us-east-1");

        // No usage plan yet → rejected.
        assert!(validate_api_key(&store, Some(&key_value), &api_id, "prod").is_err());

        let plan = svc
            .handle(
                "CreateUsagePlan",
                json!({
                    "name": "plan1",
                    "apiStages": [{"apiId": &api_id, "stage": "prod"}]
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let plan_id = plan["id"].as_str().unwrap().to_string();

        // Plan exists but no key linkage → still rejected.
        assert!(validate_api_key(&store, Some(&key_value), &api_id, "prod").is_err());

        svc.handle(
            "CreateUsagePlanKey",
            json!({
                "usageplanId": plan_id,
                "keyId": key_id,
                "keyType": "API_KEY"
            }),
            &ctx(),
        )
        .await
        .unwrap();

        // Now associated with a plan covering this stage → accepted.
        validate_api_key(&store, Some(&key_value), &api_id, "prod").unwrap();
        // Wrong stage still rejected.
        assert!(validate_api_key(&store, Some(&key_value), &api_id, "dev").is_err());
        // Missing header rejected.
        assert!(validate_api_key(&store, None, &api_id, "prod").is_err());
        // Disabled key rejected.
        store.api_keys.alter(&key_id, |_, mut k| {
            k.enabled = false;
            k
        });
        assert!(validate_api_key(&store, Some(&key_value), &api_id, "prod").is_err());
    }

    #[tokio::test]
    async fn delete_usage_plan_drops_associated_keys() {
        let svc = ApiGatewayV1Service::new();
        let (api_id, _root) = make_api(&svc).await;
        let key = svc
            .handle("CreateApiKey", json!({"name": "k"}), &ctx())
            .await
            .unwrap();
        let key_id = key["id"].as_str().unwrap().to_string();
        let plan = svc
            .handle(
                "CreateUsagePlan",
                json!({
                    "name": "p",
                    "apiStages": [{"apiId": &api_id, "stage": "prod"}]
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let plan_id = plan["id"].as_str().unwrap().to_string();
        svc.handle(
            "CreateUsagePlanKey",
            json!({"usageplanId": &plan_id, "keyId": &key_id}),
            &ctx(),
        )
        .await
        .unwrap();

        svc.handle(
            "DeleteUsagePlan",
            json!({"usageplanId": plan_id.clone()}),
            &ctx(),
        )
        .await
        .unwrap();

        let store = svc.store.get("000000000000", "us-east-1");
        let edge_id = format!("{plan_id}/{key_id}");
        assert!(!store.usage_plan_keys.contains_key(&edge_id));
    }
}
