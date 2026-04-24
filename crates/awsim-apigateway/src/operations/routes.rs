use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{ApiGatewayState, ApiRoute};

pub fn create_route(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let route_key = input["RouteKey"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "Missing required field: RouteKey")
        })?
        .to_string();

    let target = input["Target"].as_str().map(|s| s.to_string());

    let route_id = format!("r{}", &Uuid::new_v4().to_string().replace('-', "")[..8]);

    let route = ApiRoute {
        route_id: route_id.clone(),
        route_key: route_key.clone(),
        target: target.clone(),
        route_response_selection_expression: None,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    api.routes.insert(route_id.clone(), route);

    Ok(json!({
        "RouteId": route_id,
        "RouteKey": route_key,
        "Target": target,
    }))
}

pub fn get_route(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let route_id = input["RouteId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: RouteId")
    })?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    let route = api.routes.get(route_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Route with ID {route_id} not found"),
        )
    })?;

    Ok(route_to_json(route))
}

pub fn get_routes(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    let items: Vec<Value> = api.routes.values().map(route_to_json).collect();

    Ok(json!({ "Items": items }))
}

pub fn delete_route(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let route_id = input["RouteId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: RouteId")
    })?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    api.routes.remove(route_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Route with ID {route_id} not found"),
        )
    })?;

    Ok(json!({}))
}

fn route_to_json(route: &ApiRoute) -> Value {
    json!({
        "RouteId": route.route_id,
        "RouteKey": route.route_key,
        "Target": route.target,
    })
}
