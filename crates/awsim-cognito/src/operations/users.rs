use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, CognitoUser};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn user_to_value(user: &CognitoUser) -> Value {
    let attributes: Vec<Value> = user
        .attributes
        .iter()
        .map(|(k, v)| json!({"Name": k, "Value": v}))
        .collect();

    json!({
        "Username": user.username,
        "UserStatus": user.status,
        "Enabled": user.enabled,
        "UserCreateDate": user.created_date,
        "UserLastModifiedDate": user.created_date,
        "Attributes": attributes
    })
}

// ---------------------------------------------------------------------------
// SignUp
// ---------------------------------------------------------------------------

pub fn sign_up(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;

    // Find the pool that owns this client
    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let mut pool = match pool_entry {
        Some(e) => {
            // We need a mutable ref — drop the immutable ref and get_mut
            let pool_id = e.id.clone();
            drop(e);
            state.user_pools.get_mut(&pool_id).unwrap()
        }
        None => {
            return Err(AwsError::not_found(
                "ResourceNotFoundException",
                format!("No user pool found for client: {client_id}"),
            ));
        }
    };

    if pool.users.contains_key(username) {
        return Err(AwsError::conflict(
            "UsernameExistsException",
            format!("Username already exists: {username}"),
        ));
    }

    let mut attributes: HashMap<String, String> = HashMap::new();
    if let Some(arr) = input["UserAttributes"].as_array() {
        for attr in arr {
            if let (Some(k), Some(v)) = (attr["Name"].as_str(), attr["Value"].as_str()) {
                attributes.insert(k.to_string(), v.to_string());
            }
        }
    }

    let sub = Uuid::new_v4().to_string();
    attributes.insert("sub".to_string(), sub.clone());

    let user = CognitoUser {
        username: username.to_string(),
        sub: sub.clone(),
        password: password.to_string(),
        attributes,
        status: "UNCONFIRMED".to_string(),
        enabled: true,
        groups: Vec::new(),
        created_date: now_epoch(),
    };

    info!(username = %username, pool_id = %pool.id, "Cognito: user signed up");
    pool.users.insert(username.to_string(), user);

    Ok(json!({
        "UserSub": sub,
        "UserConfirmed": false,
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": "***"
        }
    }))
}

// ---------------------------------------------------------------------------
// ConfirmSignUp
// ---------------------------------------------------------------------------

pub fn confirm_sign_up(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry
        .map(|e| e.id.clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("No user pool found for client: {client_id}"),
            )
        })?;

    let mut pool = state.user_pools.get_mut(&pool_id).unwrap();

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.status = "CONFIRMED".to_string();
    info!(username = %username, "Cognito: user confirmed sign-up");

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminConfirmSignUp
// ---------------------------------------------------------------------------

pub fn admin_confirm_sign_up(
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

    user.status = "CONFIRMED".to_string();
    info!(username = %username, pool_id = %pool_id, "Cognito: admin confirmed sign-up");

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminCreateUser
// ---------------------------------------------------------------------------

pub fn admin_create_user(
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

    let password = input["TemporaryPassword"]
        .as_str()
        .unwrap_or("Temp@1234");

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.users.contains_key(username) {
        return Err(AwsError::conflict(
            "UsernameExistsException",
            format!("Username already exists: {username}"),
        ));
    }

    let mut attributes: HashMap<String, String> = HashMap::new();
    if let Some(arr) = input["UserAttributes"].as_array() {
        for attr in arr {
            if let (Some(k), Some(v)) = (attr["Name"].as_str(), attr["Value"].as_str()) {
                attributes.insert(k.to_string(), v.to_string());
            }
        }
    }

    let sub = Uuid::new_v4().to_string();
    attributes.insert("sub".to_string(), sub.clone());

    let user = CognitoUser {
        username: username.to_string(),
        sub,
        password: password.to_string(),
        attributes,
        status: "FORCE_CHANGE_PASSWORD".to_string(),
        enabled: true,
        groups: Vec::new(),
        created_date: now_epoch(),
    };

    let user_value = user_to_value(&user);
    info!(username = %username, pool_id = %pool_id, "Cognito: admin created user");
    pool.users.insert(username.to_string(), user);

    Ok(json!({ "User": user_value }))
}

// ---------------------------------------------------------------------------
// AdminDeleteUser
// ---------------------------------------------------------------------------

pub fn admin_delete_user(
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

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.users.remove(username).is_none() {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    info!(username = %username, pool_id = %pool_id, "Cognito: admin deleted user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminGetUser
// ---------------------------------------------------------------------------

pub fn admin_get_user(
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

    Ok(user_to_value(user))
}

// ---------------------------------------------------------------------------
// AdminSetUserPassword
// ---------------------------------------------------------------------------

pub fn admin_set_user_password(
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
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;
    let permanent = input["Permanent"].as_bool().unwrap_or(true);

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

    user.password = password.to_string();
    if permanent {
        user.status = "CONFIRMED".to_string();
    }

    info!(username = %username, pool_id = %pool_id, permanent, "Cognito: admin set user password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListUsers
// ---------------------------------------------------------------------------

pub fn list_users(
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

    let users: Vec<Value> = pool.users.values().map(user_to_value).collect();

    Ok(json!({ "Users": users }))
}

// ---------------------------------------------------------------------------
// GetUser (uses AccessToken)
// ---------------------------------------------------------------------------

pub fn get_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    // Check revocation
    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token).ok_or_else(
        || AwsError::bad_request("NotAuthorizedException", "Invalid access token"),
    )?;

    // Find the user across all pools (access token doesn't carry pool_id in our impl)
    for pool_entry in state.user_pools.iter() {
        if let Some(user) = pool_entry.users.get(&username) {
            let attributes: Vec<Value> = user
                .attributes
                .iter()
                .map(|(k, v)| json!({"Name": k, "Value": v}))
                .collect();

            return Ok(json!({
                "Username": user.username,
                "UserAttributes": attributes
            }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// ForgotPassword
// ---------------------------------------------------------------------------

pub fn forgot_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    // Verify the user exists
    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    if pool_entry.is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        ));
    }
    let pool_id = pool_entry.unwrap().id.clone();
    let pool = state.user_pools.get(&pool_id).unwrap();

    if !pool.users.contains_key(username) {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    // For local dev, return a mock delivery destination
    let dest = pool
        .users
        .get(username)
        .and_then(|u| u.attributes.get("email").cloned())
        .unwrap_or_else(|| "***@example.com".to_string());

    Ok(json!({
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": dest
        }
    }))
}

// ---------------------------------------------------------------------------
// ConfirmForgotPassword
// ---------------------------------------------------------------------------

pub fn confirm_forgot_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;
    // ConfirmationCode — auto-confirm for local dev, so we just accept anything

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry
        .map(|e| e.id.clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("No pool found for client: {client_id}"),
            )
        })?;

    let mut pool = state.user_pools.get_mut(&pool_id).unwrap();
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.password = password.to_string();
    user.status = "CONFIRMED".to_string();

    info!(username = %username, "Cognito: confirm forgot password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ChangePassword
// ---------------------------------------------------------------------------

pub fn change_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let previous = input["PreviousPassword"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PreviousPassword is required"))?;
    let proposed = input["ProposedPassword"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProposedPassword is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token).ok_or_else(
        || AwsError::bad_request("NotAuthorizedException", "Invalid access token"),
    )?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            if user.password != previous {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect previous password",
                ));
            }
            user.password = proposed.to_string();
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// GlobalSignOut
// ---------------------------------------------------------------------------

pub fn global_sign_out(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    state
        .revoked_tokens
        .revoked
        .insert(access_token.to_string(), ());

    info!("Cognito: global sign out");
    Ok(json!({}))
}
