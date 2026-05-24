//! HMAC-signed bearer-token store with TTL.
//!
//! Used by services that issue opaque bearer tokens to clients and
//! later need to recover the associated principal: CodeArtifact's
//! `GetAuthorizationToken`, IAM Identity Center's SCIM endpoint,
//! upcoming operator-auth session cookies. The token is a base64
//! envelope of `{principal_id, expiry, hmac}` so verification is
//! self-contained (no per-token storage) and the issuer cannot be
//! forged across process restarts (the HMAC key is regenerated each
//! boot).
//!
//! Lifetime model mirrors how AWS handles its own bearer credentials:
//! the token carries an absolute expiry; clients must reissue past
//! that point; the server stores nothing per-token.

use crate::error::AwsError;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TOKEN_VERSION: u8 = 1;
const TAG_LEN: usize = 16;
const MIN_ENVELOPE_LEN: usize = 1 + 8 + TAG_LEN;

type HmacSha256 = Hmac<Sha256>;

static SIGNING_KEY: OnceLock<[u8; 32]> = OnceLock::new();

fn signing_key() -> &'static [u8; 32] {
    SIGNING_KEY.get_or_init(|| {
        let mut k = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut k);
        k
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Issue a bearer token for `principal`, valid for `ttl`.
///
/// The principal is opaque to the helper - services use it as a
/// stringified principal ARN, user id, session id, etc. The
/// returned token is URL-safe base64 of an HMAC-SHA256 envelope and
/// can be sent as the value of an `Authorization: Bearer ...`
/// header.
pub fn mint(principal: impl AsRef<str>, ttl: Duration) -> String {
    let expiry = now_unix().saturating_add(ttl.as_secs());
    let principal = principal.as_ref().as_bytes();
    let mut envelope = Vec::with_capacity(1 + 8 + principal.len() + TAG_LEN);
    envelope.push(TOKEN_VERSION);
    envelope.extend_from_slice(&expiry.to_be_bytes());
    envelope.extend_from_slice(principal);

    let mut mac = HmacSha256::new_from_slice(signing_key()).expect("HMAC accepts any key length");
    mac.update(&envelope);
    let tag = mac.finalize().into_bytes();
    envelope.extend_from_slice(&tag[..TAG_LEN]);

    URL_SAFE_NO_PAD.encode(&envelope)
}

/// Verify a token and return the principal string it was minted
/// against.
///
/// Rejects malformed, foreign-key, expired, or wrong-version
/// envelopes with `AccessDeniedException` so service handlers can
/// `?` the result.
pub fn verify(token: &str) -> Result<String, AwsError> {
    let envelope = URL_SAFE_NO_PAD.decode(token).map_err(|_| denied())?;
    if envelope.len() < MIN_ENVELOPE_LEN {
        return Err(denied());
    }
    if envelope[0] != TOKEN_VERSION {
        return Err(denied());
    }

    let tag_start = envelope.len() - TAG_LEN;
    let (signed, tag) = envelope.split_at(tag_start);

    let mut mac = HmacSha256::new_from_slice(signing_key()).expect("HMAC accepts any key length");
    mac.update(signed);
    let expected = mac.finalize().into_bytes();
    if !constant_time_eq(tag, &expected[..TAG_LEN]) {
        return Err(denied());
    }

    let mut expiry_bytes = [0u8; 8];
    expiry_bytes.copy_from_slice(&signed[1..9]);
    let expiry = u64::from_be_bytes(expiry_bytes);
    if expiry < now_unix() {
        return Err(denied());
    }

    let principal = String::from_utf8(signed[9..].to_vec()).map_err(|_| denied())?;
    Ok(principal)
}

fn denied() -> AwsError {
    AwsError::access_denied("Invalid or expired bearer token.")
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovers_principal() {
        let tok = mint(
            "arn:aws:iam::111122223333:user/alice",
            Duration::from_secs(60),
        );
        let got = verify(&tok).unwrap();
        assert_eq!(got, "arn:aws:iam::111122223333:user/alice");
    }

    #[test]
    fn tampered_token_rejected() {
        let tok = mint("alice", Duration::from_secs(60));
        let mut bytes = URL_SAFE_NO_PAD.decode(&tok).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = URL_SAFE_NO_PAD.encode(&bytes);
        assert!(verify(&tampered).is_err());
    }

    #[test]
    fn forged_with_wrong_key_rejected() {
        let foreign = [0u8; 32];
        let principal = b"alice";
        let mut envelope = Vec::new();
        envelope.push(TOKEN_VERSION);
        let expiry = now_unix() + 60;
        envelope.extend_from_slice(&expiry.to_be_bytes());
        envelope.extend_from_slice(principal);
        let mut mac = HmacSha256::new_from_slice(&foreign).unwrap();
        mac.update(&envelope);
        let tag = mac.finalize().into_bytes();
        envelope.extend_from_slice(&tag[..TAG_LEN]);
        let tok = URL_SAFE_NO_PAD.encode(&envelope);
        assert!(verify(&tok).is_err());
    }

    #[test]
    fn truncated_token_rejected() {
        assert!(verify("YQ").is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let tok = mint("alice", Duration::from_secs(0));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(verify(&tok).is_err());
    }

    #[test]
    fn empty_principal_round_trips() {
        let tok = mint("", Duration::from_secs(60));
        assert_eq!(verify(&tok).unwrap(), "");
    }

    #[test]
    fn wrong_version_byte_rejected() {
        let mut envelope = Vec::new();
        envelope.push(99);
        envelope.extend_from_slice(&(now_unix() + 60).to_be_bytes());
        envelope.extend_from_slice(b"alice");
        let mut mac = HmacSha256::new_from_slice(signing_key()).unwrap();
        mac.update(&envelope);
        let tag = mac.finalize().into_bytes();
        envelope.extend_from_slice(&tag[..TAG_LEN]);
        let tok = URL_SAFE_NO_PAD.encode(&envelope);
        assert!(verify(&tok).is_err());
    }
}
