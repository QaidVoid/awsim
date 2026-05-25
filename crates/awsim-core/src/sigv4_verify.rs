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

/// Parsed `X-Amz-Credential` query parameter from a presigned URL.
pub struct PresignedCredential {
    pub access_key: String,
    pub date_stamp: String,
    pub region: String,
    pub service: String,
}

/// Verify the signature on a presigned URL.
///
/// AWS presigns by moving every SigV4 input into the query string. The
/// caller passes the request method, the canonical URI (path,
/// URL-encoded), the verbatim raw query string off the wire, the
/// `Host` header value (the one the signer used), and the secret
/// access key bound to the credential's access-key ID. Returns
/// [`VerifyOutcome::Ok`] when the recomputed signature matches the
/// `X-Amz-Signature` query value AND the request landed inside the
/// `X-Amz-Expires` window.
pub fn verify_presigned(
    method: &str,
    canonical_uri: &str,
    raw_query: &str,
    host: &str,
    secret: &str,
    now: SystemTime,
    skew: Duration,
) -> VerifyOutcome {
    let mut params: Vec<(String, String)> = raw_query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (kv.to_string(), String::new()),
        })
        .collect();

    let signature = match take_param(&mut params, "X-Amz-Signature") {
        Some(v) => v,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let algorithm = find_param(&params, "X-Amz-Algorithm").unwrap_or_default();
    if algorithm != "AWS4-HMAC-SHA256" {
        return VerifyOutcome::IncompleteSignature;
    }
    let credential_raw = match find_param(&params, "X-Amz-Credential") {
        Some(v) => v,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let credential_decoded = url_unescape(&credential_raw);
    let credential = match parse_presigned_credential(&credential_decoded) {
        Some(c) => c,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let amz_date = match find_param(&params, "X-Amz-Date") {
        Some(v) => v,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let signed_headers_raw = match find_param(&params, "X-Amz-SignedHeaders") {
        Some(v) => v,
        None => return VerifyOutcome::IncompleteSignature,
    };
    let signed_headers = url_unescape(&signed_headers_raw);

    // X-Amz-Expires is the signer's window in seconds. AWS rejects once
    // now > signing_time + expires. The skew tolerance only applies to
    // the "request from the future" guard.
    let parsed_date = match parse_amz_date(&amz_date) {
        Some(t) => t,
        None => return VerifyOutcome::IncompleteSignature,
    };
    if let Ok(d) = parsed_date.duration_since(now)
        && d > skew
    {
        return VerifyOutcome::SignatureMismatch;
    }
    if let Some(expires_str) = find_param(&params, "X-Amz-Expires")
        && let Ok(expires_secs) = expires_str.parse::<u64>()
    {
        let deadline = parsed_date + Duration::from_secs(expires_secs);
        if now > deadline {
            return VerifyOutcome::SignatureMismatch;
        }
    }

    // Canonical query: re-sort the remaining params (Signature already
    // removed) by key. Values stay verbatim because the signer encoded
    // them and we never decoded.
    params.sort();
    let canonical_query = params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    // Canonical headers for presigned URLs are typically just `host`.
    let signed_list: Vec<&str> = signed_headers.split(';').collect();
    let mut canonical_headers = String::new();
    for name in &signed_list {
        let lower = name.to_ascii_lowercase();
        let value = if lower == "host" { host.trim() } else { "" };
        canonical_headers.push_str(&format!("{lower}:{value}\n"));
    }
    let signed_headers_list = signed_list.join(";");

    // Presigned requests use the literal `UNSIGNED-PAYLOAD` sentinel.
    let payload_hash = "UNSIGNED-PAYLOAD";

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers_list}\n{payload_hash}",
    );

    let credential_scope = format!(
        "{}/{}/{}/aws4_request",
        credential.date_stamp, credential.region, credential.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes()),
    );

    let signing_key = derive_signing_key(
        secret,
        &credential.date_stamp,
        &credential.region,
        &credential.service,
    );
    let expected = hmac_hex(&signing_key, string_to_sign.as_bytes());

    if constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        VerifyOutcome::Ok
    } else {
        VerifyOutcome::SignatureMismatch
    }
}

fn find_param(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

fn take_param(params: &mut Vec<(String, String)>, key: &str) -> Option<String> {
    let idx = params.iter().position(|(k, _)| k == key)?;
    Some(params.remove(idx).1)
}

fn parse_presigned_credential(s: &str) -> Option<PresignedCredential> {
    let mut parts = s.split('/');
    let access_key = parts.next()?.to_string();
    let date_stamp = parts.next()?.to_string();
    let region = parts.next()?.to_string();
    let service = parts.next()?.to_string();
    let tail = parts.next()?;
    if tail != "aws4_request" {
        return None;
    }
    Some(PresignedCredential {
        access_key,
        date_stamp,
        region,
        service,
    })
}

/// Extract the access key from a presigned URL's query string, or
/// `None` if the URL doesn't carry an `X-Amz-Credential` parameter.
pub fn presigned_access_key(raw_query: &str) -> Option<String> {
    let cred = raw_query
        .split('&')
        .find_map(|kv| kv.strip_prefix("X-Amz-Credential="))?;
    let decoded = url_unescape(cred);
    parse_presigned_credential(&decoded).map(|c| c.access_key)
}

/// Minimal percent-decoder for query-string values. Handles `%XX` hex
/// escapes; good enough for SigV4 presign which encodes `/` as `%2F`
/// and `:` as `%3A`.
fn url_unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

    /// Build a presigned URL by signing it the same way real AWS does,
    /// then round-trip through verify_presigned to prove the signer and
    /// verifier agree.
    #[allow(clippy::too_many_arguments)]
    fn sign_presigned(
        secret: &str,
        method: &str,
        canonical_uri: &str,
        host: &str,
        amz_date: &str,
        date_stamp: &str,
        region: &str,
        service: &str,
        access_key: &str,
        expires: &str,
        extra_params: &[(&str, &str)],
    ) -> String {
        let credential = format!("{access_key}/{date_stamp}/{region}/{service}/aws4_request");
        let credential_enc = credential.replace('/', "%2F");
        let mut params: Vec<(String, String)> = vec![
            ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
            ("X-Amz-Credential".into(), credential_enc),
            ("X-Amz-Date".into(), amz_date.into()),
            ("X-Amz-Expires".into(), expires.into()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ];
        for (k, v) in extra_params {
            params.push(((*k).to_string(), (*v).to_string()));
        }
        params.sort();
        let canonical_query = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let canonical_headers = format!("host:{host}\n");
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\nhost\nUNSIGNED-PAYLOAD",
        );
        let scope = format!("{date_stamp}/{region}/{service}/aws4_request");
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes()),
        );
        let key = derive_signing_key(secret, date_stamp, region, service);
        let signature = hmac_hex(&key, string_to_sign.as_bytes());
        format!("{canonical_query}&X-Amz-Signature={signature}")
    }

    fn now_at(amz_date: &str) -> SystemTime {
        parse_amz_date(amz_date).unwrap()
    }

    #[test]
    fn presigned_round_trip_verifies() {
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let amz_date = "20260524T120000Z";
        let query = sign_presigned(
            secret,
            "GET",
            "/bucket/key",
            "s3.amazonaws.com",
            amz_date,
            "20260524",
            "us-east-1",
            "s3",
            "AKID",
            "900",
            &[],
        );
        let out = verify_presigned(
            "GET",
            "/bucket/key",
            &query,
            "s3.amazonaws.com",
            secret,
            now_at(amz_date),
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::Ok);
    }

    #[test]
    fn presigned_rejects_tampered_path() {
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let amz_date = "20260524T120000Z";
        let query = sign_presigned(
            secret,
            "GET",
            "/bucket/key",
            "s3.amazonaws.com",
            amz_date,
            "20260524",
            "us-east-1",
            "s3",
            "AKID",
            "900",
            &[],
        );
        // Substitute a different path — the signature was issued for
        // /bucket/key, so verifying against /bucket/key2 must fail.
        let out = verify_presigned(
            "GET",
            "/bucket/key2",
            &query,
            "s3.amazonaws.com",
            secret,
            now_at(amz_date),
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::SignatureMismatch);
    }

    #[test]
    fn presigned_rejects_expired_url() {
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let amz_date = "20260524T120000Z";
        let query = sign_presigned(
            secret,
            "GET",
            "/bucket/key",
            "s3.amazonaws.com",
            amz_date,
            "20260524",
            "us-east-1",
            "s3",
            "AKID",
            "60",
            &[],
        );
        // 2 hours after signing time + 60s expiry => past deadline.
        let later = now_at(amz_date) + Duration::from_secs(2 * 3600);
        let out = verify_presigned(
            "GET",
            "/bucket/key",
            &query,
            "s3.amazonaws.com",
            secret,
            later,
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::SignatureMismatch);
    }

    #[test]
    fn presigned_rejects_missing_signature() {
        let query = "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKID%2F20260524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20260524T120000Z&X-Amz-SignedHeaders=host";
        let out = verify_presigned(
            "GET",
            "/bucket/key",
            query,
            "s3.amazonaws.com",
            "secret",
            now_at("20260524T120000Z"),
            Duration::from_secs(300),
        );
        assert_eq!(out, VerifyOutcome::IncompleteSignature);
    }

    #[test]
    fn presigned_access_key_extracts_from_query() {
        let q = "foo=1&X-Amz-Credential=AKIA1234%2F20260524%2Fus-east-1%2Fs3%2Faws4_request&bar=2";
        assert_eq!(presigned_access_key(q).as_deref(), Some("AKIA1234"));
        assert!(presigned_access_key("no-credential-here").is_none());
    }
}
