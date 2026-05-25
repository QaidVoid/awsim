use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::jwt;
use crate::state::{CognitoState, DeviceInfo};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn device_to_value(d: &DeviceInfo) -> Value {
    json!({
        "DeviceKey": d.device_key,
        "DeviceGroupKey": d.device_group_key,
        "DeviceAttributes": [
            { "Name": "device_name", "Value": d.device_name.as_deref().unwrap_or("") }
        ],
        "DeviceCreateDate": d.created_date,
        "DeviceLastModifiedDate": d.last_modified_date,
        "DeviceLastAuthenticatedDate": d.last_authenticated_date
    })
}

fn get_username_from_token(
    state: &CognitoState,
    token: &str,
) -> Result<(String, String), AwsError> {
    let username = jwt::extract_username_from_access_token(token)
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid access token"))?;

    // Find pool containing this user
    for pool_ref in state.user_pools.iter() {
        if pool_ref.users.contains_key(&username) {
            return Ok((pool_ref.id.clone(), username));
        }
    }
    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// ConfirmDevice
// ---------------------------------------------------------------------------

pub fn confirm_device(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;
    let device_name = input["DeviceName"].as_str().map(String::from);

    let (pool_id, username) = get_username_from_token(state, token)?;
    let now = now_epoch();

    let mut pool = state
        .user_pools
        .get_mut(&pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;

    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    // Remove existing device with same key if present
    user.devices.retain(|d| d.device_key != device_key);

    user.devices.push(DeviceInfo {
        device_key: device_key.to_string(),
        device_group_key: format!("-{}", &Uuid::new_v4().to_string()[..8]),
        device_name,
        remembered: true,
        created_date: now,
        last_authenticated_date: now,
        last_modified_date: now,
    });

    info!(username = %username, device_key = %device_key, "Cognito: confirmed device");
    Ok(json!({ "UserConfirmationNecessary": false }))
}

// ---------------------------------------------------------------------------
// GetDevice
// ---------------------------------------------------------------------------

pub fn get_device(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;

    let (pool_id, username) = get_username_from_token(state, token)?;

    let pool = state
        .user_pools
        .get(&pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let user = pool.users.get(&username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let device = user
        .devices
        .iter()
        .find(|d| d.device_key == device_key)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Device not found: {device_key}"),
            )
        })?;

    Ok(json!({ "Device": device_to_value(device) }))
}

// ---------------------------------------------------------------------------
// ListDevices
// ---------------------------------------------------------------------------

pub fn list_devices(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let limit = input["Limit"].as_u64().unwrap_or(60) as usize;

    let (pool_id, username) = get_username_from_token(state, token)?;

    let pool = state
        .user_pools
        .get(&pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let user = pool.users.get(&username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let devices: Vec<Value> = user
        .devices
        .iter()
        .take(limit)
        .map(device_to_value)
        .collect();
    Ok(json!({ "Devices": devices }))
}

// ---------------------------------------------------------------------------
// UpdateDeviceStatus
// ---------------------------------------------------------------------------

pub fn update_device_status(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;
    let status = input["DeviceRememberedStatus"]
        .as_str()
        .unwrap_or("remembered");

    let (pool_id, username) = get_username_from_token(state, token)?;
    let now = now_epoch();

    let mut pool = state
        .user_pools
        .get_mut(&pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let device = user
        .devices
        .iter_mut()
        .find(|d| d.device_key == device_key)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Device not found: {device_key}"),
            )
        })?;

    device.remembered = status == "remembered";
    device.last_modified_date = now;

    info!(username = %username, device_key = %device_key, status = %status, "Cognito: updated device status");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ForgetDevice
// ---------------------------------------------------------------------------

pub fn forget_device(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;

    let (pool_id, username) = get_username_from_token(state, token)?;

    let mut pool = state
        .user_pools
        .get_mut(&pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let len_before = user.devices.len();
    user.devices.retain(|d| d.device_key != device_key);
    if user.devices.len() == len_before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Device not found: {device_key}"),
        ));
    }

    info!(username = %username, device_key = %device_key, "Cognito: forgot device");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminGetDevice
// ---------------------------------------------------------------------------

pub fn admin_get_device(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let user = pool.users.get(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let device = user
        .devices
        .iter()
        .find(|d| d.device_key == device_key)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Device not found: {device_key}"),
            )
        })?;

    Ok(json!({ "Device": device_to_value(device) }))
}

// ---------------------------------------------------------------------------
// AdminListDevices
// ---------------------------------------------------------------------------

pub fn admin_list_devices(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let limit = input["Limit"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let user = pool.users.get(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let devices: Vec<Value> = user
        .devices
        .iter()
        .take(limit)
        .map(device_to_value)
        .collect();
    Ok(json!({ "Devices": devices }))
}

// ---------------------------------------------------------------------------
// AdminUpdateDeviceStatus
// ---------------------------------------------------------------------------

pub fn admin_update_device_status(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;
    let status = input["DeviceRememberedStatus"]
        .as_str()
        .unwrap_or("remembered");

    let now = now_epoch();
    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let device = user
        .devices
        .iter_mut()
        .find(|d| d.device_key == device_key)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Device not found: {device_key}"),
            )
        })?;

    device.remembered = status == "remembered";
    device.last_modified_date = now;

    info!(username = %username, pool_id = %pool_id, device_key = %device_key, "Cognito: admin updated device status");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminForgetDevice
// ---------------------------------------------------------------------------

pub fn admin_forget_device(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let device_key = input["DeviceKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DeviceKey is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let len_before = user.devices.len();
    user.devices.retain(|d| d.device_key != device_key);
    if user.devices.len() == len_before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Device not found: {device_key}"),
        ));
    }

    info!(username = %username, pool_id = %pool_id, device_key = %device_key, "Cognito: admin forgot device");
    Ok(json!({}))
}
