use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

/// Group membership with IAM role info for JWT claim generation.
pub struct GroupRolePair {
    pub group_name: String,
    pub role_arn: Option<String>,
    pub precedence: Option<u32>,
}

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
///
/// The `scopes` list controls which claims are included:
/// - `openid`: base claims always present when this scope is in the list
/// - `email`: includes `email` and `email_verified`
/// - `phone`: includes `phone_number` and `phone_number_verified`
/// - `profile`: includes `name`, `given_name`, `family_name`, `nickname`,
///   `preferred_username`, `picture`, `website`, `gender`,
///   `birthdate`, `zoneinfo`, `locale`, `updated_at`
///
/// The `nonce` parameter (if Some) is included in the token.
// SAFETY: each parameter is an independent JWT claim sourced from distinct callers; bundling
// would require a builder layer that would not improve clarity at the call sites.
#[allow(clippy::too_many_arguments)]
pub fn id_token(
    sub: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    username: &str,
    attributes: &HashMap<String, String>,
    scopes: &[String],
    nonce: Option<&str>,
    groups: &[GroupRolePair],
    issuer_override: Option<&str>,
    expires_in: u64,
) -> String {
    let now = now_epoch();
    let header = json!({
        "alg": "RS256",
        "typ": "JWT",
        "kid": "awsim-key-1"
    });

    let scope_str = scopes.join(" ");
    let issuer = issuer_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"));

    let mut payload = json!({
        "sub": sub,
        "iss": issuer,
        "aud": client_id,
        "token_use": "id",
        "cognito:username": username,
        "auth_time": now,
        "iat": now,
        "exp": now + expires_in,
        "scope": scope_str
    });

    if let Some(n) = nonce
        && !n.is_empty()
    {
        payload["nonce"] = Value::String(n.to_string());
    }

    // SAFETY: payload was created by json!() macro above, which always produces a JSON object.
    let obj = payload.as_object_mut().expect("json!() always produces an object");

    // Inject group/role claims if the user belongs to any groups.
    if !groups.is_empty() {
        let group_names: Vec<Value> = groups
            .iter()
            .map(|g| Value::String(g.group_name.clone()))
            .collect();
        obj.insert("cognito:groups".to_string(), Value::Array(group_names));

        let roles: Vec<Value> = groups
            .iter()
            .filter_map(|g| g.role_arn.as_ref())
            .map(|arn| Value::String(arn.clone()))
            .collect();
        if !roles.is_empty() {
            // preferred_role = role from group with lowest precedence (None treated as infinity).
            let preferred = groups
                .iter()
                .filter(|g| g.role_arn.is_some())
                .min_by(|a, b| match (a.precedence, b.precedence) {
                    (Some(pa), Some(pb)) => {
                        pa.cmp(&pb).then_with(|| a.group_name.cmp(&b.group_name))
                    }
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.group_name.cmp(&b.group_name),
                })
                .and_then(|g| g.role_arn.as_ref());

            obj.insert("cognito:roles".to_string(), Value::Array(roles));
            if let Some(pref) = preferred {
                obj.insert(
                    "cognito:preferred_role".to_string(),
                    Value::String(pref.clone()),
                );
            }
        }
    }

    // email scope: include email claims.
    if scopes.iter().any(|s| s == "email") {
        let scope_attrs = ["email", "email_verified"];
        for attr in &scope_attrs {
            if let Some(v) = attributes.get(*attr) {
                obj.insert(attr.to_string(), Value::String(v.clone()));
            }
        }
    }

    // phone scope: include phone claims.
    if scopes.iter().any(|s| s == "phone") {
        let scope_attrs = ["phone_number", "phone_number_verified"];
        for attr in &scope_attrs {
            if let Some(v) = attributes.get(*attr) {
                obj.insert(attr.to_string(), Value::String(v.clone()));
            }
        }
    }

    // profile scope: include profile claims.
    if scopes.iter().any(|s| s == "profile") {
        let profile_attrs = [
            "name",
            "given_name",
            "family_name",
            "middle_name",
            "nickname",
            "preferred_username",
            "picture",
            "website",
            "gender",
            "birthdate",
            "zoneinfo",
            "locale",
            "updated_at",
        ];
        for attr in &profile_attrs {
            if let Some(v) = attributes.get(*attr) {
                obj.insert(attr.to_string(), Value::String(v.clone()));
            }
        }
    }

    // Always merge remaining user attributes (cognito:* etc.) not already present.
    for (k, v) in attributes {
        obj.entry(k.clone())
            .or_insert_with(|| Value::String(v.clone()));
    }

    build_jwt(&header, &payload)
}

/// Generate an access token for a user.
///
/// The `scopes` list is included as a space-separated `scope` claim.
/// `groups` is used to include `cognito:groups` in the access token (no roles — those are ID-token only per AWS spec).
// SAFETY: each parameter is an independent JWT claim sourced from distinct callers.
#[allow(clippy::too_many_arguments)]
pub fn access_token(
    sub: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    username: &str,
    scopes: &[String],
    groups: &[GroupRolePair],
    issuer_override: Option<&str>,
    expires_in: u64,
) -> String {
    let now = now_epoch();
    let header = json!({
        "alg": "RS256",
        "typ": "JWT",
        "kid": "awsim-key-1"
    });

    let scope_str = if scopes.is_empty() {
        "aws.cognito.signin.user.admin".to_string()
    } else {
        scopes.join(" ")
    };

    let issuer = issuer_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"));

    let mut payload = json!({
        "sub": sub,
        "iss": issuer,
        "client_id": client_id,
        "token_use": "access",
        "scope": scope_str,
        "username": username,
        "auth_time": now,
        "iat": now,
        "exp": now + expires_in,
        "jti": uuid::Uuid::new_v4().to_string()
    });

    // Include cognito:groups in access token (but NOT roles — those are ID-token only).
    if !groups.is_empty() {
        let group_names: Vec<Value> = groups
            .iter()
            .map(|g| Value::String(g.group_name.clone()))
            .collect();
        // SAFETY: payload was created by json!() macro above, which always produces a JSON object.
        payload
            .as_object_mut()
            .expect("json!() always produces an object")
            .insert("cognito:groups".to_string(), Value::Array(group_names));
    }

    build_jwt(&header, &payload)
}

/// Generate a refresh token (opaque for local dev — just a UUID).
pub fn refresh_token(sub: &str) -> String {
    format!("refresh-{sub}.{}", uuid::Uuid::new_v4())
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
