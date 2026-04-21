use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn encode_part(v: &Value) -> String {
    URL_SAFE_NO_PAD.encode(v.to_string().as_bytes())
}

/// Build a simple JWT with a dummy RS256 signature (not cryptographically valid,
/// but structurally correct so SDKs that skip verification accept it).
fn build_jwt(header: &Value, payload: &Value) -> String {
    let h = encode_part(header);
    let p = encode_part(payload);
    let sig = URL_SAFE_NO_PAD.encode(b"awsim-signature");
    format!("{h}.{p}.{sig}")
}

/// Generate an ID token for a user.
pub fn id_token(
    sub: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    username: &str,
    attributes: &HashMap<String, String>,
) -> String {
    let now = now_epoch();
    let header = json!({
        "alg": "RS256",
        "typ": "JWT",
        "kid": "awsim-key-1"
    });

    let mut payload = json!({
        "sub": sub,
        "iss": format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"),
        "aud": client_id,
        "token_use": "id",
        "cognito:username": username,
        "auth_time": now,
        "iat": now,
        "exp": now + 3600
    });

    // Merge user attributes into payload
    if let Some(obj) = payload.as_object_mut() {
        for (k, v) in attributes {
            obj.insert(k.clone(), Value::String(v.clone()));
        }
    }

    build_jwt(&header, &payload)
}

/// Generate an access token for a user.
pub fn access_token(
    sub: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    username: &str,
) -> String {
    let now = now_epoch();
    let header = json!({
        "alg": "RS256",
        "typ": "JWT",
        "kid": "awsim-key-1"
    });

    let payload = json!({
        "sub": sub,
        "iss": format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"),
        "client_id": client_id,
        "token_use": "access",
        "scope": "aws.cognito.signin.user.admin",
        "username": username,
        "auth_time": now,
        "iat": now,
        "exp": now + 3600,
        "jti": uuid::Uuid::new_v4().to_string()
    });

    build_jwt(&header, &payload)
}

/// Generate a refresh token (opaque for local dev — just a UUID).
pub fn refresh_token(sub: &str) -> String {
    format!("refresh-{sub}-{}", uuid::Uuid::new_v4())
}

/// Extract the sub claim from an access token without verifying the signature.
/// Returns None if the token is malformed.
#[allow(dead_code)]
pub fn extract_sub_from_access_token(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload: Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload["sub"].as_str().map(String::from)
}

/// Extract the username claim from an access token.
pub fn extract_username_from_access_token(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload: Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload["username"].as_str().map(String::from)
}
