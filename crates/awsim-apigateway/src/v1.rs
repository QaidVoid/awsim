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
}

#[derive(Default)]
pub struct ApiGatewayV1State {
    pub apis: DashMap<String, RestApi>,
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
            "GetStages" => get_stages(&state, &input),
            "CreateStage" => create_stage(&state, &input),
            "DeleteStage" => delete_stage(&state, &input),
            "GetDeployments" => get_deployments(&state, &input),
            "CreateDeployment" => create_deployment(&state, &input),
            "DeleteDeployment" => delete_deployment(&state, &input),
            "GetAuthorizers" => get_authorizers(&state, &input),
            "CreateAuthorizer" => create_authorizer(&state, &input),
            "DeleteAuthorizer" => delete_authorizer(&state, &input),
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
    json!({
        "id": api.id,
        "name": api.name,
        "description": api.description,
        "createdDate": api.created_date,
        "version": api.version,
        "apiKeySource": api.api_key_source,
        "endpointConfiguration": { "types": api.endpoint_types },
    })
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
    json!({
        "type": i.r#type,
        "httpMethod": i.http_method,
        "uri": i.uri,
        "connectionType": i.connection_type,
        "passthroughBehavior": i.passthrough_behavior,
        "timeoutInMillis": i.timeout_in_millis,
        "cacheNamespace": i.cache_namespace,
    })
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
    json!({
        "id": a.id,
        "name": a.name,
        "type": a.r#type,
        "authType": a.auth_type,
        "authorizerUri": a.authorizer_uri,
        "identitySource": a.identity_source,
    })
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

    let api = RestApi {
        id: id.clone(),
        name,
        description,
        version: "2015-07-09".to_string(),
        created_date: now_epoch(),
        api_key_source: "HEADER".to_string(),
        endpoint_types: vec!["REGIONAL".to_string()],
        resources,
        stages: HashMap::new(),
        deployments: Vec::new(),
        authorizers: HashMap::new(),
    };
    state.apis.insert(id.clone(), api.clone());
    Ok(rest_api_to_json(&api))
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

    with_api_mut(state, &api_id, |api| {
        let authorizer = Authorizer {
            id: short_id(),
            name,
            r#type,
            auth_type,
            authorizer_uri,
            identity_source,
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
    let path_params = extract_path_params(&resource.path, path);
    let query_params = parse_query_params(query_string);

    let event = json!({
        "resource": resource.path,
        "path": path,
        "httpMethod": http_method_upper,
        "headers": headers,
        "queryStringParameters": query_params,
        "pathParameters": path_params,
        "stageVariables": null,
        "requestContext": {
            "apiId": api_id,
            "httpMethod": http_method_upper,
            "path": path,
            "stage": stage,
            "requestId": Uuid::new_v4().to_string(),
            "identity": {
                "sourceIp": "127.0.0.1",
            },
        },
        "body": body_str,
        "isBase64Encoded": false,
    });

    Some(V1ProxyMatch {
        integration_type: integration.r#type.clone(),
        integration_uri: integration.uri.clone(),
        event,
        matched_resource_path: resource.path.clone(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
