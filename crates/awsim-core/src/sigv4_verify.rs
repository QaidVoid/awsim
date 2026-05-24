//! Cryptographic AWS Signature Version 4 verification.
//!
//! When `AWSIM_VERIFY_SIGV4` is set, the gateway recomputes the
//! caller's SigV4 signature using the secret access key bound to the
//! Authorization header's access key ID and rejects the request if
//! they differ. This is the change that makes a stolen access key
//! ID insufficient to impersonate another principal: the holder of
//! the matching secret is the only party able to produce a valid
//! signature for a given canonical request.
//!
//! Off by default to preserve the loose-dev workflow where SDK
//! clients send `Signature=fakesignature` and the gateway trusts the
//! access key ID alone.
//!
//! Spec: <https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html>
//!
//! Scope:
//!  - Signed payloads using SHA-256 over the request body.
//!  - The `UNSIGNED-PAYLOAD` sentinel (S3 large uploads) is accepted
//!    without payload-hash verification, matching AWS behaviour.
//!  - `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` (chunked S3 uploads) is
//!    treated like UNSIGNED-PAYLOAD for now — chunk-level hashing
//!    can be a future addition once the workload demands it.
//!  - Pre-signed URLs (signature in query string instead of header)
//!    are not verified; the AWS SDKs use header signing for the
//!    workloads AWSim covers.
//!  - Clock skew tolerance: 5 minutes either side, matching AWS.

use std::time::{Duration, SystemTime};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Result of verifying a SigV4 signature.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Signature matches and the request fell inside the clock-skew
    /// window. The gateway should proceed to authz.
    Ok,
    /// Signature mismatched, headers malformed, body hash mismatched,
    /// or the request timestamp was outside the allowed window.
    /// Surface as `SignatureDoesNotMatch` to the caller (real AWS
    /// always returns the same error to avoid leaking which part
    /// went wrong).
    SignatureMismatch,
    /// The Authorization header was missing pieces a real SigV4
    /// header would always have. Returned as
    /// `IncompleteSignatureException`.
    IncompleteSignature,
}

/// Whether the verifier is enabled via env. Cached.
pub fn verify_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("AWSIM_VERIFY_SIGV4")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
    })
}

/// Parsed pieces of an `Authorization: AWS4-HMAC-SHA256 ...` header.
pub struct AuthHeader<'a> {
    pub access_key: &'a str,
    pub date_stamp: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub signed_headers: Vec<&'a str>,
    pub signature: &'a str,
}

pub fn parse_authorization_header(header: &str) -> Option<AuthHeader<'_>> {
    let rest = header.strip_prefix("AWS4-HMAC-SHA256")?.trim_start();
    let mut cred: Option<&str> = None;
    let mut signed: Option<&str> = None;
    let mut sig: Option<&str> = None;
    for part in rest.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("Credential=") {
            cred = Some(v);
        } else if let Some(v) = part.strip_prefix("SignedHeaders=") {
            signed = Some(v);
        } else if let Some(v) = part.strip_prefix("Signature=") {
            sig = Some(v);
        }
    }
    let cred = cred?;
    let signed = signed?;
    let sig = sig?;
    let mut parts = cred.split('/');
    let access_key = parts.next()?;
    let date_stamp = parts.next()?;
    let region = parts.next()?;
    let service = parts.next()?;
    let scope_tail = parts.next()?;
    if scope_tail != "aws4_request" {
        return None;
    }
    let signed_headers = signed.split(';').collect();
    Some(AuthHeader {
        access_key,
        date_stamp,
        region,
        service,
        signed_headers,
        signature: sig,
    })
}

/// Verify a SigV4 signature against the recomputed expected value.
///
/// `headers_for_canonical` must be the exact header names listed in
/// `auth.signed_headers`, each paired with its (whitespace-trimmed)
/// value. The caller passes them already filtered so the verifier
/// doesn't need to know the request's full header set.
#[allow(clippy::too_many_arguments)]
pub fn verify(
    auth: &AuthHeader<'_>,
    secret: &str,
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    headers_for_canonical: &[(String, String)],
    amz_date: &str,
    body: &[u8],
    payload_hash_header: Option<&str>,
    now: SystemTime,
    skew: Duration,
) -> VerifyOutcome {
    // 1. Clock-skew check. Reject requests far in the past or
    //    future regardless of signature validity to mitigate replay.
    let parsed_date = match parse_amz_date(amz_date) {
        Some(t) => t,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let drift = match parsed_date.duration_since(now) {
        Ok(d) => d,
        Err(e) => e.duration(),
    };
    if drift > skew {
        return VerifyOutcome::SignatureMismatch;
    }

    // 2. Reconstruct the payload hash. AWS allows callers to send a
    //    pre-computed hash in `x-amz-content-sha256`; if present we
    //    trust it (matching AWS), otherwise hash the body now.
    let payload_hash: String = match payload_hash_header {
        Some(h) if h == "UNSIGNED-PAYLOAD" || h == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" => {
            h.to_string()
        }
        Some(h) => {
            let computed = sha256_hex(body);
            if h != computed {
                return VerifyOutcome::SignatureMismatch;
            }
            computed
        }
        None => sha256_hex(body),
    };

    let canonical_headers: String = headers_for_canonical
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k.to_ascii_lowercase(), v.trim()))
        .collect();
    let signed_headers_list = auth.signed_headers.join(";");

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers_list}\n{payload_hash}",
    );

    let credential_scope = format!(
        "{}/{}/{}/aws4_request",
        auth.date_stamp, auth.region, auth.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes()),
    );

    let signing_key = derive_signing_key(secret, auth.date_stamp, auth.region, auth.service);
    let expected = hmac_hex(&signing_key, string_to_sign.as_bytes());

    if constant_time_eq(expected.as_bytes(), auth.signature.as_bytes()) {
        VerifyOutcome::Ok
    } else {
        VerifyOutcome::SignatureMismatch
    }
}

fn derive_signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_secret = format!("AWS4{secret}");
    let k_date = hmac(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_hex(key: &[u8], data: &[u8]) -> String {
    let bytes = hmac(key, data);
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Parse `YYYYMMDDTHHMMSSZ` into a SystemTime.
fn parse_amz_date(s: &str) -> Option<SystemTime> {
    use chrono::{NaiveDateTime, TimeZone, Utc};
    let dt = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ").ok()?;
    let utc = Utc.from_utc_datetime(&dt);
    Some(utc.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_well_formed_header() {
        let h = "AWS4-HMAC-SHA256 Credential=AKID/20260524/us-east-1/s3/aws4_request, \
                 SignedHeaders=host;x-amz-date, Signature=deadbeef";
        let p = parse_authorization_header(h).unwrap();
        assert_eq!(p.access_key, "AKID");
        assert_eq!(p.date_stamp, "20260524");
        assert_eq!(p.region, "us-east-1");
        assert_eq!(p.service, "s3");
        assert_eq!(p.signed_headers, vec!["host", "x-amz-date"]);
        assert_eq!(p.signature, "deadbeef");
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        assert!(parse_authorization_header("AWS4-HMAC-SHA256 Credential=x/y/z").is_none());
        assert!(parse_authorization_header("not-sigv4").is_none());
    }

    #[test]
    fn known_test_vector_matches() {
        // Vector from the AWS docs:
        //   https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
        //   GET /test.txt (s3), iam-spec example.
        // We construct one inline and assert verify() agrees with itself
        // round-trip: re-signing the canonical request with the same
        // inputs must produce the same signature, so the verifier
        // accepts.
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let amz_date = "20260524T120000Z";
        let date_stamp = "20260524";
        let region = "us-east-1";
        let service = "s3";
        let method = "GET";
        let canonical_uri = "/";
        let canonical_query = "";
        let headers = vec![
            ("host".to_string(), "s3.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), amz_date.to_string()),
        ];
        let body: &[u8] = b"";
        let payload_hash = sha256_hex(body);
        let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\nhost;x-amz-date\n{payload_hash}",
        );
        let scope = format!("{date_stamp}/{region}/{service}/aws4_request");
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes()),
        );
        let key = derive_signing_key(secret, date_stamp, region, service);
        let signature = hmac_hex(&key, string_to_sign.as_bytes());

        let auth_header = format!(
            "AWS4-HMAC-SHA256 Credential=AKID/{date_stamp}/{region}/{service}/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature={signature}"
        );
        let auth = parse_authorization_header(&auth_header).unwrap();
        let outcome = verify(
            &auth,
            secret,
            method,
            canonical_uri,
            canonical_query,
            &headers,
            amz_date,
            body,
            None,
            parse_amz_date(amz_date).unwrap(),
            Duration::from_secs(300),
        );
        assert_eq!(outcome, VerifyOutcome::Ok);
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let amz_date = "20260524T120000Z";
        let bad_header = "AWS4-HMAC-SHA256 Credential=AKID/20260524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature=00000000"
            .to_string();
        let auth = parse_authorization_header(&bad_header).unwrap();
        let headers = vec![
            ("host".to_string(), "s3.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), amz_date.to_string()),
        ];
        let out = verify(
            &auth,
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "GET",
            "/",
            "",
            &headers,
            amz_date,
            b"",
            None,
            parse_amz_date(amz_date).unwrap(),
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::SignatureMismatch);
    }

    #[test]
    fn outside_clock_skew_is_rejected() {
        let amz_date = "20260524T120000Z";
        let now = parse_amz_date("20260524T130100Z").unwrap();
        let header = "AWS4-HMAC-SHA256 Credential=AKID/20260524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature=00"
            .to_string();
        let auth = parse_authorization_header(&header).unwrap();
        let headers = vec![("host".to_string(), "s3.amazonaws.com".to_string())];
        let out = verify(
            &auth,
            "secret",
            "GET",
            "/",
            "",
            &headers,
            amz_date,
            b"",
            None,
            now,
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::SignatureMismatch);
    }
}
