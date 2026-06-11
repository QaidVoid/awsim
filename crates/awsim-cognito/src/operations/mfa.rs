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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
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
            AwsError::bad_request("NotAuthorizedException", "Invalid Access Token")
        })?
    } else if let Some(_session) = input["Session"].as_str() {
        // Session-based flow: session stores pool_id+username in MFA session map.
        // For dev emulator we look it up from the session store on CognitoState.
        let session = input["Session"].as_str().unwrap_or("");
        state
            .mfa_sessions
            .get(session)
            .map(|e| e.username.clone())
            .ok_or_else(|| {
                AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
            })?
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
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
    let user_code = input["UserCode"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserCode is required")
    })?;

    if user_code.len() != 6 || !user_code.chars().all(|c| c.is_ascii_digit()) {
        return Err(AwsError::bad_request(
            "EnableSoftwareTokenMFAException",
            "UserCode must be a 6-digit number",
        ));
    }

    let username = if let Some(token) = input["AccessToken"].as_str() {
        username_from_access_token(token).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid Access Token")
        })?
    } else if let Some(session) = input["Session"].as_str() {
        state
            .mfa_sessions
            .get(session)
            .map(|e| e.username.clone())
            .ok_or_else(|| {
                AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
            })?
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Either AccessToken or Session is required",
        ));
    };

    // Verify the supplied code against the user's TOTP secret. Without a
    // secret this call is meaningless: AssociateSoftwareToken must have run
    // first.
    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            let secret = user.totp_secret.clone().ok_or_else(|| {
                AwsError::bad_request(
                    "EnableSoftwareTokenMFAException",
                    "No software token associated for this user",
                )
            })?;
            if !awsim_core::totp::verify_str(&secret, user_code, 1) {
                return Err(AwsError::bad_request(
                    "EnableSoftwareTokenMFAException",
                    "Code mismatch",
                ));
            }
            user.totp_verified = true;
            info!(username = %username, "Cognito: software token verified");
            return Ok(json!({ "Status": "SUCCESS" }));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
    ))
}

// ---------------------------------------------------------------------------
// SetUserMFAPreference
// ---------------------------------------------------------------------------

pub fn set_user_mfa_preference(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    let username = username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

    apply_mfa_preference(state, &username, input)?;

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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = super::users::resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    apply_mfa_settings_to_user(user, input)?;

    info!(username = %username, pool_id = %pool_id, "Cognito: admin set user MFA preference");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn apply_mfa_preference(
    state: &CognitoState,
    username: &str,
    input: &Value,
) -> Result<(), AwsError> {
    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(username) {
            return apply_mfa_settings_to_user(user, input);
        }
    }
    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
    ))
}

/// Apply SMS / software-token MFA settings independently. Each factor's
/// `Enabled` flag is honoured on its own (disabling one never affects the
/// other), the preference is cleared when its method is disabled, and asking
/// to prefer a method that is not enabled is rejected the way Cognito does.
fn apply_mfa_settings_to_user(
    user: &mut crate::state::CognitoUser,
    input: &Value,
) -> Result<(), AwsError> {
    let swt = &input["SoftwareTokenMfaSettings"];
    let sms = &input["SMSMfaSettings"];

    if let Some(enabled) = swt["Enabled"].as_bool() {
        user.software_token_mfa_enabled = enabled;
        if !enabled && user.mfa_preferred.as_deref() == Some("SOFTWARE_TOKEN_MFA") {
            user.mfa_preferred = None;
        }
    }
    if let Some(enabled) = sms["Enabled"].as_bool() {
        user.sms_mfa_enabled = enabled;
        if !enabled && user.mfa_preferred.as_deref() == Some("SMS_MFA") {
            user.mfa_preferred = None;
        }
    }

    if swt["PreferredMfa"].as_bool() == Some(true) {
        if !user.software_token_mfa_enabled {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "Software Token MFA cannot be preferred without being enabled.",
            ));
        }
        user.mfa_preferred = Some("SOFTWARE_TOKEN_MFA".to_string());
    }
    if sms["PreferredMfa"].as_bool() == Some(true) {
        if !user.sms_mfa_enabled {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "SMS MFA cannot be preferred without being enabled.",
            ));
        }
        user.mfa_preferred = Some("SMS_MFA".to_string());
    }
    Ok(())
}
