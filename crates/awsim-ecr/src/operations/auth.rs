use awsim_core::{AwsError, RequestContext};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::operations::repositories::now_epoch_str;

type HmacSha256 = Hmac<Sha256>;

const TOKEN_TTL_SECS: u64 = 12 * 60 * 60;

/// Per-process HMAC signing key for ECR authorization tokens.
/// Generated lazily on first use from OS randomness; stays in
/// memory for the life of the process so every mint/verify round
/// uses the same key.
fn signing_key() -> &'static [u8] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut k = [0u8; 32];
        // FNV-1a over a few unstable bits — Uuid pulls from OS randomness on
        // every call, so two consecutive uuids xored give us 256 bits of
        // process-unique state without an extra rand dependency.
        let a = uuid::Uuid::new_v4().as_bytes().to_owned();
        let b = uuid::Uuid::new_v4().as_bytes().to_owned();
        k[..16].copy_from_slice(&a);
        k[16..].copy_from_slice(&b);
        k
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Mint an HMAC-signed bearer token good for `TOKEN_TTL_SECS`
/// seconds. The wire format is the AWS-documented
/// `base64(AWS:<credential>)` so docker / OCI clients sign their
/// requests with Basic auth as usual; the `<credential>` part is
/// `<random_id>|<expires_at>|<hmac>`. The HMAC is computed over the
/// first two segments so a tampered expiry or random-id is rejected
/// by [`validate_authorization_token`].
pub fn mint_authorization_token(expires_at: u64) -> String {
    let random_id = uuid::Uuid::new_v4().to_string();
    let body = format!("{random_id}|{expires_at}");
    let mut mac =
        HmacSha256::new_from_slice(signing_key()).expect("HMAC-SHA256 accepts any key length");
    mac.update(body.as_bytes());
    let sig = BASE64.encode(mac.finalize().into_bytes());
    let credential = format!("AWS:{body}|{sig}");
    BASE64.encode(credential.as_bytes())
}

/// Verify an ECR authorization token produced by
/// [`mint_authorization_token`]. The token is the value docker / OCI
/// clients place in the `Authorization: Basic …` header (after they
/// base64-decode the credentials). Returns the credential body on
/// success so the registry HTTP layer can log per-request audit
/// information; returns an `AwsError` with code `InvalidAuthorization`
/// on tamper / expiry / shape failure.
pub fn validate_authorization_token(token: &str) -> Result<(), AwsError> {
    let decoded = BASE64
        .decode(token.as_bytes())
        .map_err(|_| invalid("authorization token is not valid base64"))?;
    let credential = std::str::from_utf8(&decoded)
        .map_err(|_| invalid("authorization token is not valid UTF-8"))?;
    let body = credential
        .strip_prefix("AWS:")
        .ok_or_else(|| invalid("authorization token is missing the AWS: prefix"))?;
    let parts: Vec<&str> = body.split('|').collect();
    if parts.len() != 3 {
        return Err(invalid(
            "authorization token must contain 3 pipe-separated segments",
        ));
    }
    let (random_id, expires_at, sig) = (parts[0], parts[1], parts[2]);
    let expires_at: u64 = expires_at
        .parse()
        .map_err(|_| invalid("authorization token expiry is not a number"))?;
    if now_secs() >= expires_at {
        return Err(invalid("authorization token has expired"));
    }

    let signed_body = format!("{random_id}|{expires_at}");
    let mut mac =
        HmacSha256::new_from_slice(signing_key()).expect("HMAC-SHA256 accepts any key length");
    mac.update(signed_body.as_bytes());
    let expected = BASE64.encode(mac.finalize().into_bytes());
    if expected != sig {
        return Err(invalid("authorization token signature mismatch"));
    }
    Ok(())
}

fn invalid(msg: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidAuthorization", msg)
}

// ---------------------------------------------------------------------------
// GetAuthorizationToken
// ---------------------------------------------------------------------------

pub fn get_authorization_token(_input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let expires_at = now_secs() + TOKEN_TTL_SECS;
    let token = mint_authorization_token(expires_at);

    let proxy_endpoint = format!(
        "https://{}.dkr.ecr.{}.localhost",
        ctx.account_id, ctx.region
    );

    let auth_data = json!({
        "authorizationToken": token,
        "expiresAt": expires_at,
        "proxyEndpoint": proxy_endpoint
    });

    Ok(json!({ "authorizationData": [auth_data] }))
}

pub fn _now_epoch_str() -> String {
    now_epoch_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_and_validate_roundtrip() {
        let token = mint_authorization_token(now_secs() + 60);
        validate_authorization_token(&token).unwrap();
    }

    #[test]
    fn rejects_tampered_token() {
        let token = mint_authorization_token(now_secs() + 60);
        let mut bytes = BASE64.decode(token.as_bytes()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = BASE64.encode(&bytes);
        let err = validate_authorization_token(&tampered).unwrap_err();
        assert_eq!(err.code, "InvalidAuthorization");
    }

    #[test]
    fn rejects_expired_token() {
        let token = mint_authorization_token(now_secs().saturating_sub(60));
        let err = validate_authorization_token(&token).unwrap_err();
        assert!(err.message.contains("expired"), "{err:?}");
    }

    #[test]
    fn rejects_malformed_token() {
        let err = validate_authorization_token("not-base64!!").unwrap_err();
        assert_eq!(err.code, "InvalidAuthorization");
    }

    #[test]
    fn rejects_token_without_aws_prefix() {
        let plain = BASE64.encode(b"FOO:xyz");
        let err = validate_authorization_token(&plain).unwrap_err();
        assert!(err.message.contains("AWS:"), "{err:?}");
    }
}
