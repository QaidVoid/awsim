use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{ClientSecretDescriptor, CognitoState};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn descriptor_to_value(d: &ClientSecretDescriptor) -> Value {
    json!({
        "ClientSecretId": d.client_secret_id,
        "ClientSecretValue": d.client_secret_value,
        "ClientSecretCreateDate": d.create_date
    })
}

pub fn add_user_pool_client_secret(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let provided_secret = input["ClientSecret"].as_str().map(String::from);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let app_client = pool.clients.get_mut(client_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    let descriptor = ClientSecretDescriptor {
        client_secret_id: Uuid::new_v4().to_string(),
        client_secret_value: provided_secret
            .unwrap_or_else(|| Uuid::new_v4().to_string().replace('-', "")),
        create_date: now_epoch(),
    };
    let val = descriptor_to_value(&descriptor);
    app_client.additional_client_secrets.push(descriptor);

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: added user pool client secret");
    Ok(json!({ "ClientSecretDescriptor": val }))
}

pub fn delete_user_pool_client_secret(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let secret_id = input["ClientSecretId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientSecretId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let app_client = pool.clients.get_mut(client_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    let len_before = app_client.additional_client_secrets.len();
    app_client
        .additional_client_secrets
        .retain(|s| s.client_secret_id != secret_id);
    if app_client.additional_client_secrets.len() == len_before {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Client secret not found: {secret_id}"),
        ));
    }

    info!(pool_id = %pool_id, client_id = %client_id, secret_id = %secret_id, "Cognito: deleted user pool client secret");
    Ok(json!({}))
}

pub fn list_user_pool_client_secrets(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let app_client = pool.clients.get(client_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    let secrets: Vec<Value> = app_client
        .additional_client_secrets
        .iter()
        .map(descriptor_to_value)
        .collect();

    Ok(json!({ "ClientSecrets": secrets, "NextToken": Value::Null }))
}
