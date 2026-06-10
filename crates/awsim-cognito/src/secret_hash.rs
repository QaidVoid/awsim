//! Validation of Cognito's SecretHash parameter.
//!
//! When a user pool client is created with `GenerateSecret=true`, every
//! authenticated request that targets it must include a `SecretHash` field
//! computed as:
//!
//! ```text
//! Base64( HMAC-SHA256( client_secret, username + client_id ) )
//! ```
//!
//! Real Cognito rejects requests with a missing or incorrect SecretHash with
//! `NotAuthorizedException`. awsim previously accepted any caller, which
//! defeated the protection a client_secret is meant to provide: an attacker
//! who knew only the client_id could call the auth APIs.

use awsim_core::AwsError;
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::{CognitoState, UserPoolClient};

type HmacSha256 = Hmac<Sha256>;

/// Compute the expected SecretHash for a given client + username.
fn compute(secret: &str, username: &str, client_id: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(username.as_bytes());
    mac.update(client_id.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Constant-time comparison of two byte slices of equal length.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Validate the supplied SecretHash for whichever pool owns `client_id`.
///
/// Returns Ok if the client is public (`GenerateSecret=false`) regardless of
/// what the caller passed; otherwise the supplied hash must match exactly.
/// `client_id` not being found is a separate concern handled by the caller's
/// own pool lookup; this helper silently passes through that case so the
/// caller's existing error path is preserved.
pub fn validate_for_client(
    state: &CognitoState,
    client_id: &str,
    provided: Option<&str>,
    username: &str,
) -> Result<(), AwsError> {
    for pool in state.user_pools.iter() {
        if let Some(client) = pool.clients.get(client_id) {
            return validate(client, provided, username, client_id);
        }
    }
    Ok(())
}

/// Validate a SecretHash that may have been computed over any of several
/// username candidates. On REFRESH_TOKEN_AUTH the original sign-in username
/// is not on the wire, so AWS accepts a hash computed with either the user's
/// username or their `sub`. Passes for public clients; on a confidential
/// client the supplied hash must match one of `candidates`.
pub fn validate_any_username(
    client: &UserPoolClient,
    provided: Option<&str>,
    candidates: &[&str],
    client_id: &str,
) -> Result<(), AwsError> {
    let Some(secret) = client.client_secret.as_deref() else {
        return Ok(());
    };
    let supplied = provided.ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            format!("Unable to verify secret hash for client {client_id}."),
        )
    })?;
    let ok = candidates.iter().any(|u| {
        ct_eq(
            compute(secret, u, client_id).as_bytes(),
            supplied.as_bytes(),
        )
    });
    if ok {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "NotAuthorizedException",
            format!("Unable to verify secret hash for client {client_id}."),
        ))
    }
}

/// Validate the supplied SecretHash against `client`'s configured secret.
///
/// If the client has no secret (`GenerateSecret=false`), this is a no-op:
/// public clients do not need to (and cannot) provide a SecretHash. If the
/// client has a secret, `provided` must be present and match exactly.
pub fn validate(
    client: &UserPoolClient,
    provided: Option<&str>,
    username: &str,
    client_id: &str,
) -> Result<(), AwsError> {
    let Some(secret) = client.client_secret.as_deref() else {
        return Ok(());
    };

    let supplied = provided.ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            format!("Unable to verify secret hash for client {client_id}."),
        )
    })?;

    let expected = compute(secret, username, client_id);
    if ct_eq(expected.as_bytes(), supplied.as_bytes()) {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "NotAuthorizedException",
            format!("Unable to verify secret hash for client {client_id}."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UserPoolClient;

    fn client_with_secret(secret: Option<&str>) -> UserPoolClient {
        UserPoolClient {
            client_id: "abc123".into(),
            client_name: "test".into(),
            user_pool_id: "us-east-1_xxx".into(),
            explicit_auth_flows: Vec::new(),
            created_date: 0,
            client_secret: secret.map(String::from),
            callback_urls: Vec::new(),
            logout_urls: Vec::new(),
            allowed_oauth_flows: Vec::new(),
            allowed_oauth_scopes: Vec::new(),
            supported_identity_providers: Vec::new(),
            access_token_validity: 3600,
            id_token_validity: 3600,
            refresh_token_validity: 30 * 24 * 3600,
            additional_client_secrets: Vec::new(),
            read_attributes: Vec::new(),
            write_attributes: Vec::new(),
        }
    }

    #[test]
    fn public_client_does_not_require_hash() {
        let c = client_with_secret(None);
        assert!(validate(&c, None, "alice", "abc123").is_ok());
        assert!(validate(&c, Some("garbage"), "alice", "abc123").is_ok());
    }

    #[test]
    fn confidential_client_rejects_missing_hash() {
        let c = client_with_secret(Some("topsecret"));
        let err = validate(&c, None, "alice", "abc123").expect_err("must reject");
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn confidential_client_rejects_wrong_hash() {
        let c = client_with_secret(Some("topsecret"));
        let err =
            validate(&c, Some("not-the-right-hash"), "alice", "abc123").expect_err("must reject");
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn confidential_client_accepts_correct_hash() {
        let c = client_with_secret(Some("topsecret"));
        let good = compute("topsecret", "alice", "abc123");
        assert!(validate(&c, Some(&good), "alice", "abc123").is_ok());
    }
}
