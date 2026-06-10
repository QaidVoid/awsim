use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, WebAuthnCredential};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn credential_to_value(c: &WebAuthnCredential) -> Value {
    json!({
        "CredentialId": c.credential_id,
        "FriendlyCredentialName": c.friendly_credential_name,
        "RelyingPartyId": c.relying_party_id,
        "AuthenticatorAttachment": c.authenticator_attachment,
        "AuthenticatorTransports": c.authenticator_transports,
        "CreatedAt": c.created_at
    })
}

fn username_for_token(state: &CognitoState, access_token: &str) -> Result<String, AwsError> {
    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }
    crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))
}

pub fn start_webauthn_registration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let username = username_for_token(state, access_token)?;
    let challenge = Uuid::new_v4().to_string();

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            user.webauthn_pending_challenge = Some(challenge.clone());
            return Ok(json!({
                "CredentialCreationOptions": {
                    "challenge": challenge,
                    "rp": { "id": "awsim.local", "name": "awsim" },
                    "user": {
                        "id": user.sub,
                        "name": user.username,
                        "displayName": user.username,
                    },
                    "pubKeyCredParams": [
                        { "type": "public-key", "alg": -7 }
                    ],
                    "timeout": 60000,
                    "attestation": "none"
                }
            }));
        }
    }
    Err(AwsError::service_not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

pub fn complete_webauthn_registration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let credential = &input["Credential"];
    let username = username_for_token(state, access_token)?;

    let credential_id = credential["id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let friendly = credential["friendlyName"].as_str().map(String::from);

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            if user.webauthn_pending_challenge.is_none() {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    "No pending WebAuthn registration",
                ));
            }
            user.webauthn_pending_challenge = None;
            user.webauthn_credentials.push(WebAuthnCredential {
                credential_id,
                friendly_credential_name: friendly,
                relying_party_id: "awsim.local".to_string(),
                authenticator_attachment: Some("platform".to_string()),
                authenticator_transports: vec!["internal".to_string()],
                created_at: now_epoch(),
            });
            info!(username = %username, "Cognito: completed WebAuthn registration");
            return Ok(json!({}));
        }
    }
    Err(AwsError::service_not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

pub fn delete_webauthn_credential(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let credential_id = input["CredentialId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "CredentialId is required")
    })?;
    let username = username_for_token(state, access_token)?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            let len_before = user.webauthn_credentials.len();
            user.webauthn_credentials
                .retain(|c| c.credential_id != credential_id);
            if user.webauthn_credentials.len() == len_before {
                return Err(AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("Credential not found: {credential_id}"),
                ));
            }
            info!(username = %username, credential_id = %credential_id, "Cognito: deleted WebAuthn credential");
            return Ok(json!({}));
        }
    }
    Err(AwsError::service_not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

pub fn list_webauthn_credentials(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;
    let username = username_for_token(state, access_token)?;

    for pool_entry in state.user_pools.iter() {
        if let Some(user) = pool_entry.users.get(&username) {
            let creds: Vec<Value> = user
                .webauthn_credentials
                .iter()
                .take(max_results)
                .map(credential_to_value)
                .collect();
            return Ok(json!({ "Credentials": creds, "NextToken": Value::Null }));
        }
    }
    Err(AwsError::service_not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}
