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
            // Methods + integrations (per-resource)
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}",
                operation: "GetMethod",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/resources/{resource_id}/methods/{http_method}/integration",
                operation: "GetIntegration",
                required_query_param: None,
            },
            // Stages
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/stages",
                operation: "GetStages",
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
            // Authorizers
            RouteDefinition {
                method: "GET",
                path_pattern: "/restapis/{restapi_id}/authorizers",
                operation: "GetAuthorizers",
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
            "GetMethod" => get_method(&state, &input),
            "GetIntegration" => get_integration(&state, &input),
            "GetStages" => get_stages(&state, &input),
            "GetDeployments" => get_deployments(&state, &input),
            "CreateDeployment" => create_deployment(&state, &input),
            "GetAuthorizers" => get_authorizers(&state, &input),
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
}
