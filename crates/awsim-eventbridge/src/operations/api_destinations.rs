use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{ApiDestination, EventBridgeState};

fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn api_dest_to_value(d: &ApiDestination) -> Value {
    json!({
        "ApiDestinationArn": d.arn,
        "Name": d.name,
        "Description": d.description,
        "ConnectionArn": d.connection_arn,
        "InvocationEndpoint": d.invocation_endpoint,
        "HttpMethod": d.http_method,
        "InvocationRateLimitPerSecond": d.invocation_rate_limit_per_second,
        "ApiDestinationState": d.state,
        "CreationTime": d.creation_time,
        "LastModifiedTime": d.last_modified_time,
    })
}

// ---------------------------------------------------------------------------
// CreateApiDestination
// ---------------------------------------------------------------------------

pub fn create_api_destination(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    if state.api_destinations.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("ApiDestination {name} already exists"),
        ));
    }

    let connection_arn = input["ConnectionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ConnectionArn is required"))?;

    let invocation_endpoint = input["InvocationEndpoint"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "InvocationEndpoint is required")
    })?;

    let http_method = input["HttpMethod"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "HttpMethod is required"))?;

    let arn = format!(
        "arn:aws:events:{}:{}:api-destination/{}",
        ctx.region, ctx.account_id, name
    );

    let now = now_iso8601();
    let dest = ApiDestination {
        name: name.to_string(),
        arn: arn.clone(),
        description: input["Description"].as_str().unwrap_or("").to_string(),
        connection_arn: connection_arn.to_string(),
        invocation_endpoint: invocation_endpoint.to_string(),
        http_method: http_method.to_string(),
        invocation_rate_limit_per_second: input["InvocationRateLimitPerSecond"]
            .as_u64()
            .unwrap_or(300) as u32,
        state: "ACTIVE".to_string(),
        creation_time: now.clone(),
        last_modified_time: now,
    };

    state.api_destinations.insert(name.to_string(), dest);

    Ok(json!({
        "ApiDestinationArn": arn,
        "ApiDestinationState": "ACTIVE",
        "CreationTime": state.api_destinations.get(name).map(|d| d.creation_time.clone()).unwrap_or_default(),
        "LastModifiedTime": state.api_destinations.get(name).map(|d| d.last_modified_time.clone()).unwrap_or_default(),
    }))
}

// ---------------------------------------------------------------------------
// DeleteApiDestination
// ---------------------------------------------------------------------------

pub fn delete_api_destination(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    state.api_destinations.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("ApiDestination {name} does not exist"),
        )
    })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeApiDestination
// ---------------------------------------------------------------------------

pub fn describe_api_destination(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let dest = state.api_destinations.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("ApiDestination {name} does not exist"),
        )
    })?;

    Ok(api_dest_to_value(&dest))
}

// ---------------------------------------------------------------------------
// ListApiDestinations
// ---------------------------------------------------------------------------

pub fn list_api_destinations(
    state: &EventBridgeState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let destinations: Vec<Value> = state
        .api_destinations
        .iter()
        .map(|entry| api_dest_to_value(entry.value()))
        .collect();

    Ok(json!({ "ApiDestinations": destinations }))
}
