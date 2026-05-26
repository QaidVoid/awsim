use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Connection, EventBridgeState};
use crate::util::now_iso8601;

/// Validate AuthorizationType + AuthParameters shape per AWS Smithy:
/// BASIC requires `BasicAuthParameters { Username, Password }`,
/// API_KEY requires `ApiKeyAuthParameters { ApiKeyName, ApiKeyValue }`,
/// OAUTH_CLIENT_CREDENTIALS requires `OAuthParameters` with
/// AuthorizationEndpoint + HttpMethod + ClientParameters.
fn validate_auth_parameters(auth_type: &str, params: &Value) -> Result<(), AwsError> {
    match auth_type {
        "BASIC" => {
            let basic = params.get("BasicAuthParameters").ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "BasicAuthParameters is required when AuthorizationType is BASIC.",
                )
            })?;
            if basic
                .get("Username")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || basic
                    .get("Password")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(AwsError::bad_request(
                    "InvalidParameter",
                    "BasicAuthParameters requires Username and Password.",
                ));
            }
        }
        "API_KEY" => {
            let api = params.get("ApiKeyAuthParameters").ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "ApiKeyAuthParameters is required when AuthorizationType is API_KEY.",
                )
            })?;
            if api
                .get("ApiKeyName")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || api
                    .get("ApiKeyValue")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(AwsError::bad_request(
                    "InvalidParameter",
                    "ApiKeyAuthParameters requires ApiKeyName and ApiKeyValue.",
                ));
            }
        }
        "OAUTH_CLIENT_CREDENTIALS" => {
            let oauth = params.get("OAuthParameters").ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "OAuthParameters is required when AuthorizationType is OAUTH_CLIENT_CREDENTIALS.",
                )
            })?;
            if oauth
                .get("AuthorizationEndpoint")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || oauth
                    .get("HttpMethod")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(AwsError::bad_request(
                    "InvalidParameter",
                    "OAuthParameters requires AuthorizationEndpoint and HttpMethod.",
                ));
            }
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                format!(
                    "AuthorizationType `{other}` must be BASIC, API_KEY, or OAUTH_CLIENT_CREDENTIALS."
                ),
            ));
        }
    }
    Ok(())
}

fn connection_to_value(c: &Connection) -> Value {
    json!({
        "ConnectionArn": c.arn,
        "Name": c.name,
        "Description": c.description,
        "AuthorizationType": c.auth_type,
        "AuthParameters": c.auth_parameters,
        "ConnectionState": c.state,
        "CreationTime": c.creation_time,
        "LastModifiedTime": c.last_modified_time,
    })
}

// ---------------------------------------------------------------------------
// CreateConnection
// ---------------------------------------------------------------------------

pub fn create_connection(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    if state.connections.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Connection {name} already exists"),
        ));
    }

    let auth_type = input["AuthorizationType"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "AuthorizationType is required")
    })?;
    let auth_params = &input["AuthParameters"];
    validate_auth_parameters(auth_type, auth_params)?;

    let arn = format!(
        "arn:aws:events:{}:{}:connection/{}",
        ctx.region, ctx.account_id, name
    );

    let now = now_iso8601();
    let connection = Connection {
        name: name.to_string(),
        arn: arn.clone(),
        description: input["Description"].as_str().unwrap_or("").to_string(),
        auth_type: auth_type.to_string(),
        auth_parameters: input["AuthParameters"].clone(),
        state: "AUTHORIZED".to_string(),
        creation_time: now.clone(),
        last_modified_time: now,
    };

    state.connections.insert(name.to_string(), connection);

    Ok(json!({
        "ConnectionArn": arn,
        "ConnectionState": "AUTHORIZED",
        "CreationTime": state.connections.get(name).map(|c| c.creation_time.clone()).unwrap_or_default(),
        "LastModifiedTime": state.connections.get(name).map(|c| c.last_modified_time.clone()).unwrap_or_default(),
    }))
}

// ---------------------------------------------------------------------------
// DeleteConnection
// ---------------------------------------------------------------------------

pub fn delete_connection(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let (_, conn) = state.connections.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Connection {name} does not exist"),
        )
    })?;

    Ok(json!({
        "ConnectionArn": conn.arn,
        "ConnectionState": "DEAUTHORIZED",
        "LastModifiedTime": conn.last_modified_time,
    }))
}

// ---------------------------------------------------------------------------
// DescribeConnection
// ---------------------------------------------------------------------------

pub fn describe_connection(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let conn = state.connections.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Connection {name} does not exist"),
        )
    })?;

    Ok(connection_to_value(&conn))
}

// ---------------------------------------------------------------------------
// ListConnections
// ---------------------------------------------------------------------------

pub fn list_connections(
    state: &EventBridgeState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let connections: Vec<Value> = state
        .connections
        .iter()
        .map(|entry| connection_to_value(entry.value()))
        .collect();

    Ok(json!({ "Connections": connections }))
}
