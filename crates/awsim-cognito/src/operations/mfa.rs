use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::CognitoState;

// ---------------------------------------------------------------------------
// Base32 encoder (RFC 4648, no padding)
// ---------------------------------------------------------------------------

fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::new();
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for &byte in data {
        buffer = (buffer << 8) | byte as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0x1F) as usize;
            result.push(ALPHABET[index] as char);
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1F) as usize;
        result.push(ALPHABET[index] as char);
    }
    result
}

/// Generate a random 20-byte TOTP secret encoded as base32.
fn generate_totp_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

// ---------------------------------------------------------------------------
// Helpers to resolve a username from an AccessToken
// ---------------------------------------------------------------------------

fn username_from_access_token(access_token: &str) -> Option<String> {
    crate::jwt::extract_username_from_access_token(access_token)
}

// ---------------------------------------------------------------------------
// SetUserPoolMfaConfig
// ---------------------------------------------------------------------------

pub fn set_user_pool_mfa_config(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if let Some(mfa) = input["MfaConfiguration"].as_str() {
        pool.mfa_configuration = mfa.to_string();
    }

    if let Some(enabled) = input["SoftwareTokenMfaConfiguration"]["Enabled"].as_bool() {
        pool.software_token_mfa_enabled = enabled;
    }

    info!(pool_id = %pool_id, mfa = %pool.mfa_configuration, "Cognito: set user pool MFA config");

    Ok(json!({
        "MfaConfiguration": pool.mfa_configuration,
        "SoftwareTokenMfaConfiguration": {
            "Enabled": pool.software_token_mfa_enabled
        }
    }))
}

// ---------------------------------------------------------------------------
// GetUserPoolMfaConfig
// ---------------------------------------------------------------------------

pub fn get_user_pool_mfa_config(
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
        "MfaConfiguration": pool.mfa_configuration,
        "SoftwareTokenMfaConfiguration": {
            "Enabled": pool.software_token_mfa_enabled
        }
    }))
}

// ---------------------------------------------------------------------------
// AssociateSoftwareToken
// ---------------------------------------------------------------------------

pub fn associate_software_token(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Resolve username from AccessToken or Session
    let username = if let Some(token) = input["AccessToken"].as_str() {
        username_from_access_token(token).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid access token")
        })?
    } else if let Some(_session) = input["Session"].as_str() {
        // Session-based flow: session stores pool_id+username in MFA session map.
        // For dev emulator we look it up from the session store on CognitoState.
        let session = input["Session"].as_str().unwrap();
        state
            .mfa_sessions
            .get(session)
            .map(|e| e.username.clone())
            .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid session"))?
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Either AccessToken or Session is required",
        ));
    };

    let secret = generate_totp_secret();
    let session = Uuid::new_v4().to_string();

    // Store the secret on the user (not yet verified)
    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            user.totp_secret = Some(secret.clone());
            break;
        }
    }

    info!(username = %username, "Cognito: associated software token");

    Ok(json!({
        "SecretCode": secret,
        "Session": session
    }))
}

// ---------------------------------------------------------------------------
// VerifySoftwareToken
// ---------------------------------------------------------------------------

pub fn verify_software_token(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user_code = input["UserCode"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserCode is required"))?;

    // Dev emulator: accept any 6-digit code
    if user_code.len() != 6 || !user_code.chars().all(|c| c.is_ascii_digit()) {
        return Err(AwsError::bad_request(
            "EnableSoftwareTokenMFAException",
            "UserCode must be a 6-digit number",
        ));
    }

    let username = if let Some(token) = input["AccessToken"].as_str() {
        username_from_access_token(token).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid access token")
        })?
    } else if let Some(session) = input["Session"].as_str() {
        state
            .mfa_sessions
            .get(session)
            .map(|e| e.username.clone())
            .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid session"))?
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Either AccessToken or Session is required",
        ));
    };

    // Mark TOTP as verified on the user
    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            user.totp_verified = true;
            info!(username = %username, "Cognito: software token verified");
            break;
        }
    }

    Ok(json!({ "Status": "SUCCESS" }))
}

// ---------------------------------------------------------------------------
// SetUserMFAPreference
// ---------------------------------------------------------------------------

pub fn set_user_mfa_preference(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    let username = username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    apply_mfa_preference(state, &username, input);

    info!(username = %username, "Cognito: set user MFA preference");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminSetUserMFAPreference
// ---------------------------------------------------------------------------

pub fn admin_set_user_mfa_preference(
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

    apply_mfa_settings_to_user(user, input);

    info!(username = %username, pool_id = %pool_id, "Cognito: admin set user MFA preference");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn apply_mfa_preference(state: &CognitoState, username: &str, input: &Value) {
    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(username) {
            apply_mfa_settings_to_user(user, input);
            break;
        }
    }
}

fn apply_mfa_settings_to_user(user: &mut crate::state::CognitoUser, input: &Value) {
    let swt = &input["SoftwareTokenMfaSettings"];
    let sms = &input["SMSMfaSettings"];

    if let Some(enabled) = swt["Enabled"].as_bool() {
        user.mfa_enabled = enabled;
        if let Some(preferred) = swt["PreferredMfa"].as_bool() {
            if preferred {
                user.mfa_preferred = Some("SOFTWARE_TOKEN_MFA".to_string());
            }
        }
    }

    if let Some(enabled) = sms["Enabled"].as_bool() {
        if enabled {
            user.mfa_enabled = true;
            if let Some(preferred) = sms["PreferredMfa"].as_bool() {
                if preferred {
                    user.mfa_preferred = Some("SMS_MFA".to_string());
                }
            }
        }
    }
}
