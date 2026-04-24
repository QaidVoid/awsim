use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::CognitoState;

fn parse_mfa_options(input: &Value) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    if let Some(arr) = input.as_array() {
        for item in arr {
            let mut map = HashMap::new();
            if let Some(medium) = item["DeliveryMedium"].as_str() {
                map.insert("DeliveryMedium".to_string(), medium.to_string());
            }
            if let Some(name) = item["AttributeName"].as_str() {
                map.insert("AttributeName".to_string(), name.to_string());
            }
            out.push(map);
        }
    }
    out
}

pub fn admin_set_user_settings(
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
    let mfa_options = parse_mfa_options(&input["MFAOptions"]);

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
    user.mfa_options = mfa_options;
    info!(username = %username, pool_id = %pool_id, "Cognito: admin set user settings");
    Ok(json!({}))
}

pub fn set_user_settings(
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
    let mfa_options = parse_mfa_options(&input["MFAOptions"]);

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            user.mfa_options = mfa_options;
            return Ok(json!({}));
        }
    }
    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}
