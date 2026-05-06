use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, CognitoUser};

/// Fire-and-forget Lambda trigger via the event bus.
fn invoke_trigger(ctx: &RequestContext, trigger_source: &str, lambda_arn: &str, event: &Value) {
    if let Some(ref bus) = ctx.event_bus {
        bus.publish(InternalEvent {
            source: "cognito-idp".to_string(),
            event_type: "cognito:LambdaTrigger".to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "triggerSource": trigger_source,
                "functionArn": lambda_arn,
                "event": event,
            }),
        });
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn user_to_value(user: &CognitoUser) -> Value {
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
        "Attributes": &attributes,
        "UserAttributes": &attributes
    })
}

fn make_user(
    username: &str,
    password: &str,
    attributes: HashMap<String, String>,
    status: &str,
) -> CognitoUser {
    let sub = Uuid::new_v4().to_string();
    let mut attrs = attributes;
    attrs.insert("sub".to_string(), sub.clone());
    CognitoUser {
        username: username.to_string(),
        sub,
        password: password.to_string(),
        attributes: attrs,
        status: status.to_string(),
        enabled: true,
        groups: Vec::new(),
        created_date: now_epoch(),
        pending_verifications: HashMap::new(),
        revoked_refresh_tokens: Vec::new(),
        mfa_enabled: false,
        mfa_preferred: None,
        totp_secret: None,
        totp_verified: false,
        devices: Vec::new(),
        linked_providers: Vec::new(),
        mfa_options: Vec::new(),
        webauthn_credentials: Vec::new(),
        webauthn_pending_challenge: None,
        failed_login_attempts: 0,
        locked_until_secs: None,
        auth_events: Vec::new(),
    }
}

fn parse_user_attributes(input: &Value, key: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    if let Some(arr) = input[key].as_array() {
        for attr in arr {
            if let (Some(k), Some(v)) = (attr["Name"].as_str(), attr["Value"].as_str()) {
                attrs.insert(k.to_string(), v.to_string());
            }
        }
    }
    attrs
}

// ---------------------------------------------------------------------------
// SignUp
// ---------------------------------------------------------------------------

pub fn sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
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

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let mut pool = match pool_entry {
        Some(e) => {
            let pool_id = e.id.clone();
            drop(e);
            state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?
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

    super::auth_policy::validate_password(&pool.policies, password)?;

    let attributes = parse_user_attributes(input, "UserAttributes");
    let user = make_user(username, password, attributes, "UNCONFIRMED");
    let sub = user.sub.clone();

    // Pre Sign-Up trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PreSignUp") {
        let trigger_event = json!({
            "userPoolId": pool.id,
            "userName": username,
            "callerContext": { "clientId": client_id },
            "request": { "userAttributes": {} }
        });
        invoke_trigger(ctx, "PreSignUp_SignUp", arn, &trigger_event);
    }

    // Custom Message trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("CustomMessage") {
        let trigger_event = json!({
            "userPoolId": pool.id,
            "userName": username,
            "triggerSource": "CustomMessage_SignUp"
        });
        invoke_trigger(ctx, "CustomMessage_SignUp", arn, &trigger_event);
    }

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
    ctx: &RequestContext,
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

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No user pool found for client: {client_id}"),
        )
    })?;

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let code_key = format!("{pool_id}:{username}");
    if let Some(expected) = state.confirmation_codes.get(&code_key) {
        let provided = input["ConfirmationCode"].as_str().unwrap_or("");
        if provided != *expected {
            return Err(AwsError::bad_request(
                "CodeMismatchException",
                "Invalid verification code provided",
            ));
        }
    } else if !input["ConfirmationCode"].is_null() {
        let provided = input["ConfirmationCode"].as_str().unwrap_or("");
        if provided.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "ConfirmationCode is required",
            ));
        }
    }

    let _ = state.confirmation_codes.remove(&code_key);

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.status = "CONFIRMED".to_string();
    info!(username = %username, "Cognito: user confirmed sign-up");

    // Post-Confirmation trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PostConfirmation") {
        let trigger_event = json!({
            "userPoolId": pool_id,
            "userName": username,
            "callerContext": { "clientId": client_id }
        });
        invoke_trigger(ctx, "PostConfirmation_ConfirmSignUp", arn, &trigger_event);
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminConfirmSignUp
// ---------------------------------------------------------------------------

pub fn admin_confirm_sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
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

    // Post-Confirmation trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PostConfirmation") {
        let trigger_event = json!({
            "userPoolId": pool_id,
            "userName": username,
        });
        invoke_trigger(ctx, "PostConfirmation_ConfirmSignUp", arn, &trigger_event);
    }

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

    let password = input["TemporaryPassword"].as_str().unwrap_or("Temp@1234");

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

    super::auth_policy::validate_password(&pool.policies, password)?;

    let attributes = parse_user_attributes(input, "UserAttributes");
    let user = make_user(username, password, attributes, "FORCE_CHANGE_PASSWORD");
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

    super::auth_policy::validate_password(&pool.policies, password)?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.password = password.to_string();
    // AWS semantics: Permanent=true → CONFIRMED, Permanent=false → the
    // password is treated as temporary and the user must change it on
    // next sign-in. We were previously only flipping to CONFIRMED on
    // Permanent=true and leaving the status alone otherwise, which let
    // a CONFIRMED user keep CONFIRMED status when given a temp password
    // — opposite of what AWS does.
    user.status = if permanent {
        "CONFIRMED".to_string()
    } else {
        "FORCE_CHANGE_PASSWORD".to_string()
    };
    // Setting a fresh password administratively unlocks the account.
    user.failed_login_attempts = 0;
    user.locked_until_secs = None;

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

    // Collect and sort users for deterministic pagination
    let mut users: Vec<&CognitoUser> = pool.users.values().collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));

    // Apply Filter if provided
    if let Some(filter_str) = input["Filter"].as_str() {
        users.retain(|u| evaluate_cognito_filter(u, filter_str));
    }

    // Apply PaginationToken — skip users up to and including the token username
    if let Some(token) = input["PaginationToken"].as_str()
        && let Some(pos) = users.iter().position(|u| u.username == token)
    {
        users = users.into_iter().skip(pos + 1).collect();
    }

    // Apply Limit
    let limit = input["Limit"].as_u64().unwrap_or(60) as usize;
    let has_more = users.len() > limit;
    let next_token = if has_more {
        users.get(limit - 1).map(|u| u.username.clone())
    } else {
        None
    };
    users.truncate(limit);

    let user_values: Vec<Value> = users.iter().map(|u| user_to_value(u)).collect();

    let mut resp = json!({ "Users": user_values });
    if let Some(token) = next_token {
        resp["PaginationToken"] = json!(token);
    }
    Ok(resp)
}

/// Evaluate a Cognito ListUsers filter expression against a user.
///
/// Cognito filter format: `attribute operator "value"`
/// Operators: `=` (exact match), `^=` (starts with)
fn evaluate_cognito_filter(user: &CognitoUser, filter: &str) -> bool {
    // Determine operator and split
    let (attr_name, operator, value) = if let Some(idx) = filter.find("^=") {
        (filter[..idx].trim(), "^=", filter[idx + 2..].trim())
    } else if let Some(idx) = filter.find('=') {
        (filter[..idx].trim(), "=", filter[idx + 1..].trim())
    } else {
        return true; // Unrecognised filter — pass all
    };

    // Strip surrounding quotes from value
    let value = value.trim_matches('"');

    let user_value: Option<&str> = match attr_name {
        "cognito:user_status" | "status" => Some(user.status.as_str()),
        "username" => Some(user.username.as_str()),
        "sub" => Some(user.sub.as_str()),
        "enabled" => Some(if user.enabled { "true" } else { "false" }),
        attr => user.attributes.get(attr).map(|s| s.as_str()),
    };

    match (user_value, operator) {
        (Some(v), "=") => v == value,
        (Some(v), "^=") => v.starts_with(value),
        _ => false,
    }
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

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

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
    ctx: &RequestContext,
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

    if pool_entry.is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        ));
    }
    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        )
    })?;
    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if !pool.users.contains_key(username) {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    let dest = pool
        .users
        .get(username)
        .and_then(|u| u.attributes.get("email").cloned())
        .unwrap_or_else(|| "***@example.com".to_string());

    // Custom Message trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("CustomMessage") {
        let trigger_event = json!({
            "userPoolId": pool_id,
            "userName": username,
            "triggerSource": "CustomMessage_ForgotPassword"
        });
        invoke_trigger(ctx, "CustomMessage_ForgotPassword", arn, &trigger_event);
    }

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

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        )
    })?;

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    super::auth_policy::validate_password(&pool.policies, password)?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.password = password.to_string();
    user.status = "CONFIRMED".to_string();
    user.failed_login_attempts = 0;
    user.locked_until_secs = None;

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

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            super::auth_policy::validate_password(&pool_entry.policies, proposed)?;
            let user = pool_entry.users.get_mut(&username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
            })?;
            if user.password != previous {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect previous password",
                ));
            }
            user.password = proposed.to_string();
            user.failed_login_attempts = 0;
            user.locked_until_secs = None;
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

// ---------------------------------------------------------------------------
// AdminEnableUser
// ---------------------------------------------------------------------------

pub fn admin_enable_user(
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

    user.enabled = true;
    info!(username = %username, pool_id = %pool_id, "Cognito: admin enabled user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminDisableUser
// ---------------------------------------------------------------------------

pub fn admin_disable_user(
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

    user.enabled = false;
    info!(username = %username, pool_id = %pool_id, "Cognito: admin disabled user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminResetUserPassword
// ---------------------------------------------------------------------------

pub fn admin_reset_user_password(
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

    user.status = "RESET_REQUIRED".to_string();
    info!(username = %username, pool_id = %pool_id, "Cognito: admin reset user password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminUpdateUserAttributes
// ---------------------------------------------------------------------------

pub fn admin_update_user_attributes(
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

    let new_attrs = parse_user_attributes(input, "UserAttributes");
    for (k, v) in new_attrs {
        user.attributes.insert(k, v);
    }

    info!(username = %username, pool_id = %pool_id, "Cognito: admin updated user attributes");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminDeleteUserAttributes
// ---------------------------------------------------------------------------

pub fn admin_delete_user_attributes(
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

    if let Some(names) = input["UserAttributeNames"].as_array() {
        for name in names {
            if let Some(n) = name.as_str() {
                user.attributes.remove(n);
            }
        }
    }

    info!(username = %username, pool_id = %pool_id, "Cognito: admin deleted user attributes");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateUserAttributes (authenticated user updates own attributes)
// ---------------------------------------------------------------------------

pub fn update_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    let new_attrs = parse_user_attributes(input, "UserAttributes");

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            for (k, v) in new_attrs {
                user.attributes.insert(k, v);
            }
            return Ok(json!({ "CodeDeliveryDetailsList": [] }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// DeleteUserAttributes (authenticated user deletes own attributes)
// ---------------------------------------------------------------------------

pub fn delete_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    let attr_names: Vec<String> = input["UserAttributeNames"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            for name in &attr_names {
                user.attributes.remove(name);
            }
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// DeleteUser (authenticated user deletes own account)
// ---------------------------------------------------------------------------

pub fn delete_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.remove(&username).is_some() {
            state
                .revoked_tokens
                .revoked
                .insert(access_token.to_string(), ());
            info!(username = %username, "Cognito: user deleted own account");
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// ResendConfirmationCode
// ---------------------------------------------------------------------------

pub fn resend_confirmation_code(
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

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        )
    })?;

    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    if !pool.users.contains_key(username) {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
    state
        .confirmation_codes
        .insert(format!("{pool_id}:{username}"), code.clone());

    info!(username = %username, code = %code, "Cognito: resend confirmation code");
    Ok(json!({
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": "***"
        }
    }))
}

// ---------------------------------------------------------------------------
// GetUserAttributeVerificationCode
// ---------------------------------------------------------------------------

pub fn get_user_attribute_verification_code(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let attribute_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
            user.pending_verifications
                .insert(attribute_name.to_string(), code.clone());
            info!(username = %username, attribute_name = %attribute_name, code = %code, "Cognito: attribute verification code sent");
            return Ok(json!({
                "CodeDeliveryDetails": {
                    "AttributeName": attribute_name,
                    "DeliveryMedium": "EMAIL",
                    "Destination": "***"
                }
            }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// VerifyUserAttribute
// ---------------------------------------------------------------------------

pub fn verify_user_attribute(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let attribute_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;
    let _code = input["Code"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Code is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            if let Some(expected) = user.pending_verifications.get(attribute_name)
                && _code != expected
            {
                return Err(AwsError::bad_request(
                    "CodeMismatchException",
                    "Invalid verification code provided",
                ));
            }
            let verified_key = format!("{attribute_name}_verified");
            user.attributes.insert(verified_key, "true".to_string());
            user.pending_verifications.remove(attribute_name);
            info!(username = %username, attribute_name = %attribute_name, "Cognito: verified user attribute");
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// AdminUserGlobalSignOut
// ---------------------------------------------------------------------------

pub fn admin_user_global_sign_out(
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

    for token in &user.revoked_refresh_tokens {
        state.revoked_tokens.revoked.insert(token.clone(), ());
    }
    user.revoked_refresh_tokens.clear();

    info!(username = %username, pool_id = %pool_id, "Cognito: admin global sign out");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RevokeToken
// ---------------------------------------------------------------------------

pub fn revoke_token(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["Token"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Token is required"))?;
    let _client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    state.revoked_tokens.revoked.insert(token.to_string(), ());
    info!("Cognito: revoke token");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminListUserAuthEvents
// ---------------------------------------------------------------------------

pub fn admin_list_user_auth_events(
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
    let max_results = input["MaxResults"].as_u64().unwrap_or(60).clamp(1, 60) as usize;

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

    // Newest first per AWS semantics, capped at MaxResults.
    let events: Vec<Value> = user
        .auth_events
        .iter()
        .rev()
        .take(max_results)
        .map(|e| {
            json!({
                "EventId": e.event_id,
                "EventType": e.event_type,
                "CreationDate": e.creation_date,
                "EventResponse": e.event_response,
                "EventRisk": {
                    "RiskDecision": e.risk_decision,
                    "RiskLevel": e.risk_level,
                    "CompromisedCredentialsDetected": e.compromised_credentials_detected,
                },
                "EventFeedback": e.feedback_value.as_ref().map(|v| json!({
                    "FeedbackValue": v,
                    "Provider": "Cognito"
                })),
            })
        })
        .collect();

    Ok(json!({ "AuthEvents": events }))
}
