use std::collections::HashMap;
use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{ApiGatewayState, HttpApi};
use crate::util::now_iso8601;

pub fn create_api(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: Name"))?
        .to_string();

    let protocol_type = input["ProtocolType"]
        .as_str()
        .unwrap_or("HTTP")
        .to_string();

    if protocol_type != "HTTP" && protocol_type != "WEBSOCKET" {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("Invalid ProtocolType: {protocol_type}. Must be HTTP or WEBSOCKET"),
        ));
    }

    let api_id = Uuid::new_v4()
        .to_string()
        .replace('-', "")
        .chars()
        .take(10)
        .collect::<String>();

    let api_endpoint = format!("http://localhost:4566/restapis/{api_id}");
    let created_date = now_iso8601();

    let description = input["Description"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let tags = input["Tags"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<HashMap<String, String>>()
        })
        .unwrap_or_default();

    let api = HttpApi {
        api_id: api_id.clone(),
        name: name.clone(),
        protocol_type: protocol_type.clone(),
        api_endpoint: api_endpoint.clone(),
        routes: HashMap::new(),
        integrations: HashMap::new(),
        stages: HashMap::new(),
        deployments: HashMap::new(),
        created_date: created_date.clone(),
        description: description.clone(),
        cors_configuration: None,
        tags,
    };

    state.apis.insert(api_id.clone(), api);

    Ok(json!({
        "ApiId": api_id,
        "Name": name,
        "ProtocolType": protocol_type,
        "ApiEndpoint": api_endpoint,
        "CreatedDate": created_date,
        "Description": description,
    }))
}

pub fn get_api(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    Ok(api_to_json(&api))
}

pub fn get_apis(
    state: &Arc<ApiGatewayState>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .apis
        .iter()
        .map(|entry| api_to_json(entry.value()))
        .collect();

    Ok(json!({
        "Items": items,
    }))
}

pub fn delete_api(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    state.apis.remove(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    Ok(json!({}))
}

pub fn update_api(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    if let Some(name) = input["Name"].as_str() {
        api.name = name.to_string();
    }
    if let Some(desc) = input["Description"].as_str() {
        api.description = desc.to_string();
    }

    Ok(api_to_json(&api))
}

fn api_to_json(api: &HttpApi) -> Value {
    json!({
        "ApiId": api.api_id,
        "Name": api.name,
        "ProtocolType": api.protocol_type,
        "ApiEndpoint": api.api_endpoint,
        "CreatedDate": api.created_date,
        "Description": api.description,
        "Tags": api.tags,
    })
}

