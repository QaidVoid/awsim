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

/// Resolve the (pool_id, username) a software-token request targets, from
/// either its AccessToken (the token's client_id pins the pool) or its
/// Session (the MFA session stores the pool). Scoping to a single pool avoids
/// mutating a same-named user in a different pool.
fn resolve_token_target(state: &CognitoState, input: &Value) -> Result<(String, String), AwsError> {
    if let Some(token) = input["AccessToken"].as_str() {
        let claims = crate::jwt::verify_access_token(token).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid Access Token")
        })?;
        // The token's client_id is unique to one pool; find it.
        let pool_id = state
            .user_pools
            .iter()
            .find(|p| p.clients.contains_key(&claims.client_id))
            .map(|p| p.id.clone())
            // Fall back to the pool that actually holds the user when the
            // token carried no resolvable client_id (older/hand-rolled tokens).
            .or_else(|| {
                state
                    .user_pools
                    .iter()
                    .find(|p| p.users.contains_key(&claims.username))
                    .map(|p| p.id.clone())
            })
            .ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
        Ok((pool_id, claims.username))
    } else if let Some(session) = input["Session"].as_str() {
        let entry = state.mfa_sessions.get(session).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
        Ok((entry.pool_id.clone(), entry.username.clone()))
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            "Either AccessToken or Session is required",
        ))
    }
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

    // SMS and email MFA config are stored verbatim so GetUserPoolMfaConfig
    // round-trips them (awsim does not model SMS/email delivery itself).
    for key in ["SmsMfaConfiguration", "EmailMfaConfiguration"] {
        if !input[key].is_null() {
            pool.extra_config
                .insert(key.to_string(), input[key].clone());
        }
    }

    info!(pool_id = %pool_id, mfa = %pool.mfa_configuration, "Cognito: set user pool MFA config");

    Ok(user_pool_mfa_config_value(&pool))
}

/// Build the GetUserPoolMfaConfig / SetUserPoolMfaConfig response, echoing the
/// software-token state plus any stored SMS / email MFA config (defaulting to
/// AWS's empty `SmsMfaConfiguration`).
fn user_pool_mfa_config_value(pool: &crate::state::UserPool) -> Value {
    let sms = pool
        .extra_config
        .get("SmsMfaConfiguration")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut out = json!({
        "MfaConfiguration": pool.mfa_configuration,
        "SoftwareTokenMfaConfiguration": { "Enabled": pool.software_token_mfa_enabled },
        "SmsMfaConfiguration": sms,
    });
    if let Some(email) = pool.extra_config.get("EmailMfaConfiguration") {
        out["EmailMfaConfiguration"] = email.clone();
    }
    out
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

    Ok(user_pool_mfa_config_value(&pool))
}

// ---------------------------------------------------------------------------
// AssociateSoftwareToken
// ---------------------------------------------------------------------------

pub fn associate_software_token(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let (pool_id, username) = resolve_token_target(state, input)?;

    let secret = generate_totp_secret();
    let session = Uuid::new_v4().to_string();

    // Store the secret on the resolved user (not yet verified).
    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    user.totp_secret = Some(secret.clone());

    info!(username = %username, pool_id = %pool_id, "Cognito: associated software token");

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

    let (pool_id, username) = resolve_token_target(state, input)?;

    // Verify the supplied code against the user's TOTP secret. Without a
    // secret this call is meaningless: AssociateSoftwareToken must have run
    // first.
    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
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
    info!(username = %username, pool_id = %pool_id, "Cognito: software token verified");

    // AWS returns a Session continuation token so the caller can proceed
    // through the rest of the auth flow; echo the one supplied or mint a new.
    let session = input["Session"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    Ok(json!({ "Status": "SUCCESS", "Session": session }))
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
