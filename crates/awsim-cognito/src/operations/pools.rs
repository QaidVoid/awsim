use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, UserPool, UserPoolClient};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn pool_arn(region: &str, account_id: &str, pool_id: &str) -> String {
    format!("arn:aws:cognito-idp:{region}:{account_id}:userpool/{pool_id}")
}

// ---------------------------------------------------------------------------
// CreateUserPool
// ---------------------------------------------------------------------------

pub fn create_user_pool(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_name = input["PoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PoolName is required"))?;

    let random = &Uuid::new_v4().to_string()[..8];
    let pool_id = format!("{0}_{1}", ctx.region, random);
    let arn = pool_arn(&ctx.region, &ctx.account_id, &pool_id);
    let now = now_epoch();

    let pool = UserPool {
        id: pool_id.clone(),
        name: pool_name.to_string(),
        arn: arn.clone(),
        clients: HashMap::new(),
        users: HashMap::new(),
        groups: HashMap::new(),
        created_date: now,
    };

    info!(pool_id = %pool_id, "Cognito: created user pool");
    state.user_pools.insert(pool_id.clone(), pool);

    Ok(json!({
        "UserPool": {
            "Id": pool_id,
            "Name": pool_name,
            "Arn": arn,
            "Status": "Active",
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteUserPool
// ---------------------------------------------------------------------------

pub fn delete_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    if state.user_pools.remove(pool_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        ));
    }

    info!(pool_id = %pool_id, "Cognito: deleted user pool");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeUserPool
// ---------------------------------------------------------------------------

pub fn describe_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    Ok(json!({
        "UserPool": {
            "Id": pool.id,
            "Name": pool.name,
            "Arn": pool.arn,
            "Status": "Active",
            "CreationDate": pool.created_date,
            "LastModifiedDate": pool.created_date
        }
    }))
}

// ---------------------------------------------------------------------------
// ListUserPools
// ---------------------------------------------------------------------------

pub fn list_user_pools(
    state: &CognitoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pools: Vec<Value> = state
        .user_pools
        .iter()
        .map(|e| {
            json!({
                "Id": e.id,
                "Name": e.name,
                "Status": "Active",
                "CreationDate": e.created_date,
                "LastModifiedDate": e.created_date
            })
        })
        .collect();

    Ok(json!({ "UserPools": pools }))
}

// ---------------------------------------------------------------------------
// CreateUserPoolClient
// ---------------------------------------------------------------------------

pub fn create_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_name = input["ClientName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientName is required"))?;

    let explicit_auth_flows: Vec<String> = input["ExplicitAuthFlows"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let client_id = Uuid::new_v4().to_string().replace('-', "")[..26].to_string();
    let now = now_epoch();

    let client = UserPoolClient {
        client_id: client_id.clone(),
        client_name: client_name.to_string(),
        user_pool_id: pool_id.to_string(),
        explicit_auth_flows,
        created_date: now,
    };

    pool.clients.insert(client_id.clone(), client);

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: created user pool client");

    Ok(json!({
        "UserPoolClient": {
            "UserPoolId": pool_id,
            "ClientName": client_name,
            "ClientId": client_id,
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// DescribeUserPoolClient
// ---------------------------------------------------------------------------

pub fn describe_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let client = pool.clients.get(client_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    Ok(json!({
        "UserPoolClient": {
            "UserPoolId": pool_id,
            "ClientName": client.client_name,
            "ClientId": client.client_id,
            "ExplicitAuthFlows": client.explicit_auth_flows,
            "CreationDate": client.created_date,
            "LastModifiedDate": client.created_date
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteUserPoolClient
// ---------------------------------------------------------------------------

pub fn delete_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.clients.remove(client_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        ));
    }

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: deleted user pool client");
    Ok(json!({}))
}
