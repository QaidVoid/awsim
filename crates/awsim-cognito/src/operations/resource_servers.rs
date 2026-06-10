use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{CognitoState, ResourceServer, ResourceServerScope};

fn scope_to_value(s: &ResourceServerScope) -> Value {
    json!({
        "ScopeName": s.scope_name,
        "ScopeDescription": s.scope_description
    })
}

fn server_to_value(rs: &ResourceServer) -> Value {
    json!({
        "UserPoolId": rs.user_pool_id,
        "Identifier": rs.identifier,
        "Name": rs.name,
        "Scopes": rs.scopes.iter().map(scope_to_value).collect::<Vec<_>>()
    })
}

fn parse_scopes(input: &Value) -> Vec<ResourceServerScope> {
    input["Scopes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let name = s["ScopeName"].as_str()?;
                    let desc = s["ScopeDescription"].as_str().unwrap_or("");
                    Some(ResourceServerScope {
                        scope_name: name.to_string(),
                        scope_description: desc.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// CreateResourceServer
// ---------------------------------------------------------------------------

pub fn create_resource_server(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let identifier = input["Identifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Identifier is required")
    })?;
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Name is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool
        .resource_servers
        .iter()
        .any(|rs| rs.identifier == identifier)
    {
        return Err(AwsError::conflict(
            "InvalidParameterException",
            format!("Resource server already exists: {identifier}"),
        ));
    }

    let scopes = parse_scopes(input);
    let rs = ResourceServer {
        identifier: identifier.to_string(),
        name: name.to_string(),
        scopes,
        user_pool_id: pool_id.to_string(),
    };

    let rs_value = server_to_value(&rs);
    pool.resource_servers.push(rs);
    info!(identifier = %identifier, pool_id = %pool_id, "Cognito: created resource server");

    Ok(json!({ "ResourceServer": rs_value }))
}

// ---------------------------------------------------------------------------
// DescribeResourceServer
// ---------------------------------------------------------------------------

pub fn describe_resource_server(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let identifier = input["Identifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Identifier is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let rs = pool
        .resource_servers
        .iter()
        .find(|rs| rs.identifier == identifier)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Resource server not found: {identifier}"),
            )
        })?;

    Ok(json!({ "ResourceServer": server_to_value(rs) }))
}

// ---------------------------------------------------------------------------
// UpdateResourceServer
// ---------------------------------------------------------------------------

pub fn update_resource_server(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let identifier = input["Identifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Identifier is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let rs = pool
        .resource_servers
        .iter_mut()
        .find(|rs| rs.identifier == identifier)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Resource server not found: {identifier}"),
            )
        })?;

    if let Some(name) = input["Name"].as_str() {
        rs.name = name.to_string();
    }
    if !input["Scopes"].is_null() {
        rs.scopes = parse_scopes(input);
    }

    let rs_value = server_to_value(rs);
    info!(identifier = %identifier, pool_id = %pool_id, "Cognito: updated resource server");

    Ok(json!({ "ResourceServer": rs_value }))
}

// ---------------------------------------------------------------------------
// DeleteResourceServer
// ---------------------------------------------------------------------------

pub fn delete_resource_server(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let identifier = input["Identifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Identifier is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let len_before = pool.resource_servers.len();
    pool.resource_servers
        .retain(|rs| rs.identifier != identifier);

    if pool.resource_servers.len() == len_before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Resource server not found: {identifier}"),
        ));
    }

    info!(identifier = %identifier, pool_id = %pool_id, "Cognito: deleted resource server");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListResourceServers
// ---------------------------------------------------------------------------

pub fn list_resource_servers(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let limit = cap_max_results(input["MaxResults"].as_i64(), 50, 50);

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let mut servers: Vec<ResourceServer> = pool.resource_servers.clone();
    servers.sort_by(|a, b| a.identifier.cmp(&b.identifier));

    let token = input["NextToken"].as_str();
    let page = paginate(servers, limit, token, |rs| rs.identifier.clone())?;
    let server_values: Vec<Value> = page.items.iter().map(server_to_value).collect();

    let mut resp = json!({ "ResourceServers": server_values });
    if let Some(next) = page.next_token {
        resp["NextToken"] = json!(next);
    }
    Ok(resp)
}
