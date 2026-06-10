use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Header, encode};
use serde_json::{Value, json};

use crate::keys;

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

fn rs256_header() -> Header {
    let mut h = Header::new(jsonwebtoken::Algorithm::RS256);
    h.kid = Some(keys::KID.to_string());
    h.typ = Some("JWT".to_string());
    h
}

/// Sign a payload with the process-wide RSA key as RS256.
fn sign(payload: &Value) -> String {
    encode(&rs256_header(), payload, keys::encoding_key())
        .expect("RS256 signing with a freshly generated key should not fail")
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
///
/// `read_attributes` is the app client's Cognito `ReadAttributes`.
/// An empty slice is the AWS default (every attribute claim included);
/// a custom set restricts the token's user-attribute claims to exactly
/// that set. Identity/protocol claims (`sub`, `aud`, `cognito:*`, etc.)
/// come from dedicated parameters and are never filtered.
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
    read_attributes: &[String],
    scopes: &[String],
    nonce: Option<&str>,
    groups: &[GroupRolePair],
    issuer_override: Option<&str>,
    expires_in: u64,
) -> String {
    let now = now_epoch();

    // Cognito gates ID-token attribute claims by the app client's
    // ReadAttributes. Empty = AWS default (all readable); a custom
    // set restricts the token to exactly those attributes.
    let filtered_attrs;
    let attributes: &HashMap<String, String> = if read_attributes.is_empty() {
        attributes
    } else {
        filtered_attrs = attributes
            .iter()
            .filter(|(k, _)| read_attributes.iter().any(|a| a == *k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<String, String>>();
        &filtered_attrs
    };
    let issuer = issuer_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"));

    // Cognito ID tokens carry no `scope` claim; `origin_jti` ties the token to
    // its auth event and `jti` uniquely identifies it (both used for
    // revocation).
    let mut payload = json!({
        "sub": sub,
        "iss": issuer,
        "aud": client_id,
        "token_use": "id",
        "cognito:username": username,
        "origin_jti": uuid::Uuid::new_v4().to_string(),
        "jti": uuid::Uuid::new_v4().to_string(),
        "auth_time": now,
        "iat": now,
        "exp": now + expires_in,
    });

    if let Some(n) = nonce
        && !n.is_empty()
    {
        payload["nonce"] = Value::String(n.to_string());
    }

    // SAFETY: payload was created by json!() macro above, which always produces a JSON object.
    let obj = payload
        .as_object_mut()
        .expect("json!() always produces an object");

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

    if scopes.iter().any(|s| s == "email") {
        let scope_attrs = ["email", "email_verified"];
        for attr in &scope_attrs {
            if let Some(v) = attributes.get(*attr) {
                obj.insert(attr.to_string(), Value::String(v.clone()));
            }
        }
        // When an email is present but its verified flag was never recorded,
        // Cognito reports email_verified=true in the ID token.
        if obj.contains_key("email") && !obj.contains_key("email_verified") {
            obj.insert("email_verified".to_string(), Value::Bool(true));
        }
    }

    if scopes.iter().any(|s| s == "phone") {
        let scope_attrs = ["phone_number", "phone_number_verified"];
        for attr in &scope_attrs {
            if let Some(v) = attributes.get(*attr) {
                obj.insert(attr.to_string(), Value::String(v.clone()));
            }
        }
    }

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

    for (k, v) in attributes {
        obj.entry(k.clone())
            .or_insert_with(|| Value::String(v.clone()));
    }

    sign(&payload)
}

/// Generate an access token for a user.
///
/// The `scopes` list is included as a space-separated `scope` claim.
/// `groups` is used to include `cognito:groups` in the access token (no roles
/// are emitted: those are ID-token only per AWS spec).
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
        "origin_jti": uuid::Uuid::new_v4().to_string(),
        "jti": uuid::Uuid::new_v4().to_string()
    });

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

    sign(&payload)
}

/// Generate an opaque refresh token of the form
/// `refresh-{sub}.{issued_at}.{uuid}`. The embedded issue time lets the auth
/// endpoints reject tokens minted before a global sign-out without tracking
/// every outstanding token. The `sub` is a UUID and so never contains a `.`,
/// keeping the segment layout unambiguous.
pub fn refresh_token(sub: &str) -> String {
    format!("refresh-{sub}.{}.{}", now_epoch(), uuid::Uuid::new_v4())
}

/// Extract the issue time (Unix seconds) embedded in a refresh token produced
/// by [`refresh_token`]. Returns `None` for a malformed token or one minted by
/// the older timestamp-less format.
pub fn refresh_token_issued_at(token: &str) -> Option<u64> {
    token
        .strip_prefix("refresh-")?
        .split('.')
        .nth(1)?
        .parse()
        .ok()
}

/// Verified access-token claims.
pub struct AccessClaims {
    pub username: String,
    pub sub: String,
    /// The app client the token was issued for. Used to enforce that
    /// client's `ReadAttributes` / `WriteAttributes` on the
    /// access-token user APIs. Empty if the token carried no
    /// `client_id` (older tokens / hand-rolled test tokens).
    pub client_id: String,
}

/// Verify an access token's RS256 signature, expiry, and `token_use=access`,
/// returning the claims of interest. Returns `None` for any failure (bad
/// signature, expired, wrong token kind, malformed payload).
pub fn verify_access_token(token: &str) -> Option<AccessClaims> {
    let data =
        jsonwebtoken::decode::<Value>(token, keys::decoding_key(), &keys::validation()).ok()?;
    if data.claims.get("token_use").and_then(|v| v.as_str()) != Some("access") {
        return None;
    }
    let username = data
        .claims
        .get("username")
        .and_then(|v| v.as_str())
        .map(String::from)?;
    let sub = data
        .claims
        .get("sub")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    let client_id = data
        .claims
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    Some(AccessClaims {
        username,
        sub,
        client_id,
    })
}

/// Convenience: verify the access token and return its `username` claim.
pub fn extract_username_from_access_token(token: &str) -> Option<String> {
    verify_access_token(token).map(|c| c.username)
}

/// Convenience: verify the access token and return its `sub` claim.
#[allow(dead_code)]
pub fn extract_sub_from_access_token(token: &str) -> Option<String> {
    verify_access_token(token).map(|c| c.sub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn access_token_round_trips_through_real_signature() {
        let token = access_token(
            "sub-123",
            "us-east-1",
            "us-east-1_pool",
            "client-abc",
            "alice",
            &[],
            &[],
            None,
            3600,
        );
        let claims = verify_access_token(&token).expect("token verifies under real RS256");
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.sub, "sub-123");
    }

    #[test]
    fn forged_token_is_rejected() {
        // A token with the right shape but a bogus signature must not verify.
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT","kid":"awsim-key-1"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            br#"{"sub":"x","iss":"https://cognito-idp.us-east-1.amazonaws.com/p","exp":9999999999,"token_use":"access","username":"mallory"}"#,
        );
        let sig = URL_SAFE_NO_PAD.encode(b"forged");
        let forged = format!("{header}.{payload}.{sig}");
        assert!(verify_access_token(&forged).is_none());
    }

    #[test]
    fn wrong_token_use_is_rejected() {
        // An ID-token-shaped JWT signed with our key must not pass the
        // `token_use=access` gate.
        let id = id_token(
            "sub-123",
            "us-east-1",
            "us-east-1_pool",
            "client-abc",
            "alice",
            &Default::default(),
            &[],
            &[],
            None,
            &[],
            None,
            3600,
        );
        assert!(verify_access_token(&id).is_none());
    }

    fn decode_claims(token: &str) -> serde_json::Value {
        let payload = token.split('.').nth(1).expect("jwt has a payload segment");
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .expect("payload is base64url");
        serde_json::from_slice(&bytes).expect("payload is JSON")
    }

    #[test]
    fn token_claims_match_cognito_shape() {
        let mut attrs = HashMap::new();
        attrs.insert("email".to_string(), "e@x.com".to_string());
        let scopes = vec!["openid".to_string(), "email".to_string()];
        let id = id_token(
            "s",
            "us-east-1",
            "p",
            "c",
            "alice",
            &attrs,
            &[],
            &scopes,
            None,
            &[],
            None,
            3600,
        );
        let c = decode_claims(&id);
        // ID tokens carry no scope claim, but do carry origin_jti + jti.
        assert!(c.get("scope").is_none(), "id token must not carry scope");
        assert!(c["origin_jti"].is_string());
        assert!(c["jti"].is_string());
        // email present without an explicit verified flag defaults to true.
        assert_eq!(c["email_verified"], serde_json::json!(true));

        // SDK-flow access tokens use the fixed admin scope and carry origin_jti.
        let access = access_token("s", "us-east-1", "p", "c", "alice", &[], &[], None, 3600);
        let ac = decode_claims(&access);
        assert_eq!(ac["scope"], "aws.cognito.signin.user.admin");
        assert!(ac["origin_jti"].is_string());
        assert!(ac["jti"].is_string());
    }

    #[test]
    fn id_token_filters_attributes_by_read_set() {
        let mut attrs = HashMap::new();
        attrs.insert("email".to_string(), "e@x.com".to_string());
        attrs.insert("name".to_string(), "Al".to_string());
        attrs.insert("custom:plan".to_string(), "pro".to_string());
        let scopes = vec!["openid".to_string(), "email".to_string()];

        // Empty ReadAttributes = AWS default: every attribute claim.
        let tok = id_token(
            "s",
            "us-east-1",
            "p",
            "c",
            "alice",
            &attrs,
            &[],
            &scopes,
            None,
            &[],
            None,
            3600,
        );
        let c = decode_claims(&tok);
        assert_eq!(c["email"], "e@x.com");
        assert_eq!(c["custom:plan"], "pro");
        assert_eq!(c["name"], "Al");

        // A custom set restricts the token to exactly those attributes;
        // identity/protocol claims are always present.
        let read = vec!["email".to_string()];
        let tok = id_token(
            "s",
            "us-east-1",
            "p",
            "c",
            "alice",
            &attrs,
            &read,
            &scopes,
            None,
            &[],
            None,
            3600,
        );
        let c = decode_claims(&tok);
        assert_eq!(c["email"], "e@x.com");
        assert!(
            c.get("custom:plan").is_none(),
            "custom:plan must be filtered"
        );
        assert!(c.get("name").is_none(), "name must be filtered");
        assert_eq!(c["sub"], "s");
        assert_eq!(c["aud"], "c");
        assert_eq!(c["token_use"], "id");
    }
}
