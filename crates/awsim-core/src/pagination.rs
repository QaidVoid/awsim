//! Opaque-token pagination helper shared by every service's `List*` operation.
//!
//! AWS pagination tokens (`NextToken`, `NextMarker`, `ContinuationToken`,
//! `NextKeyMarker`, ...) are opaque base64 strings. SDK clients send them
//! back verbatim and must not attempt to decode or compare them. This
//! module wraps a service-chosen marker string in a per-process
//! HMAC-SHA256 envelope with an expiry timestamp, then base64-encodes the
//! result.
//!
//! Callers provide:
//! - a sorted `Vec<T>` of results (sorted by the same key used to derive
//!   the marker, typically alphabetical resource name)
//! - the page size requested by the caller (already capped via
//!   [`cap_max_results`] or rejected via [`clamp_max_results_strict`])
//! - the optional starting token from the request
//! - a closure that extracts the marker key from each item
//!
//! The result is a [`Page<T>`] containing the items for this page plus
//! the token to resume from. The marker stored in the token is the key of
//! the *first item not yet returned* together with how many earlier items
//! shared that key, so a non-unique sort key cannot re-emit an item across a
//! page boundary; resuming a page hands back exactly the next slice with no
//! overlap or gap.
//!
//! Items whose keys compare lexicographically less than the marker are
//! skipped, which means the helper handles a resource being deleted
//! between list calls gracefully: it advances to the first key still
//! present.
//!
//! ## Token format
//!
//! Tokens are URL-safe base64 (no padding) of a binary envelope:
//!
//! ```text
//! version (1 byte) || expiry_unix_seconds (8 bytes, big-endian)
//!     || marker_bytes (variable) || hmac_sha256_truncated (16 bytes)
//! ```
//!
//! The HMAC key is generated once per process from OS randomness. This
//! means tokens issued by one process cannot be redeemed by another, and
//! tokens do not survive a process restart. That matches how AWS itself
//! behaves across regional failovers: an in-flight paginator must restart
//! from the beginning if the backend rotates.
//!
//! Tokens carry a 6-hour TTL. Expired tokens are rejected with the same
//! error as malformed or forged tokens, since AWS does not distinguish.

use crate::error::AwsError;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_INVALID_CODE: &str = "InvalidParameterValue";
const TOKEN_INVALID_MSG: &str = "The pagination token is malformed or expired.";

const MAX_RESULTS_INVALID_CODE: &str = "ValidationException";

const TOKEN_VERSION: u8 = 1;
const TAG_LEN: usize = 16;
const MIN_ENVELOPE_LEN: usize = 1 + 8 + TAG_LEN;

/// Default time-to-live for pagination tokens (6 hours).
pub const TOKEN_TTL_SECONDS: u64 = 6 * 60 * 60;

type HmacSha256 = Hmac<Sha256>;

static SIGNING_KEY: OnceLock<[u8; 32]> = OnceLock::new();

fn signing_key() -> &'static [u8; 32] {
    SIGNING_KEY.get_or_init(|| {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        key
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One page of results plus the token to resume from.
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_token: Option<String>,
}

/// Encode a marker string as a signed, time-limited pagination token.
pub fn encode_token(marker: &str) -> String {
    encode_token_with_expiry(marker, now_unix().saturating_add(TOKEN_TTL_SECONDS))
}

fn encode_token_with_expiry(marker: &str, expiry: u64) -> String {
    let marker_bytes = marker.as_bytes();
    let mut envelope = Vec::with_capacity(1 + 8 + marker_bytes.len() + TAG_LEN);
    envelope.push(TOKEN_VERSION);
    envelope.extend_from_slice(&expiry.to_be_bytes());
    envelope.extend_from_slice(marker_bytes);

    let mut mac = HmacSha256::new_from_slice(signing_key()).expect("HMAC accepts any key length");
    mac.update(&envelope);
    let tag = mac.finalize().into_bytes();
    envelope.extend_from_slice(&tag[..TAG_LEN]);

    URL_SAFE_NO_PAD.encode(&envelope)
}

/// Decode a signed pagination token back to its marker string.
///
/// Rejects tokens that are malformed, signed with a different key, or
/// past their expiry timestamp.
pub fn decode_token(token: &str) -> Result<String, AwsError> {
    let envelope = URL_SAFE_NO_PAD.decode(token).map_err(|_| token_invalid())?;
    if envelope.len() < MIN_ENVELOPE_LEN {
        return Err(token_invalid());
    }
    if envelope[0] != TOKEN_VERSION {
        return Err(token_invalid());
    }

    let tag_start = envelope.len() - TAG_LEN;
    let (signed, tag) = envelope.split_at(tag_start);

    let mut mac = HmacSha256::new_from_slice(signing_key()).expect("HMAC accepts any key length");
    mac.update(signed);
    let expected = mac.finalize().into_bytes();
    if !constant_time_eq(tag, &expected[..TAG_LEN]) {
        return Err(token_invalid());
    }

    let mut expiry_bytes = [0u8; 8];
    expiry_bytes.copy_from_slice(&signed[1..9]);
    let expiry = u64::from_be_bytes(expiry_bytes);
    if expiry < now_unix() {
        return Err(token_invalid());
    }

    let marker_bytes = &signed[9..];
    String::from_utf8(marker_bytes.to_vec()).map_err(|_| token_invalid())
}

fn token_invalid() -> AwsError {
    AwsError::bad_request(TOKEN_INVALID_CODE, TOKEN_INVALID_MSG)
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

/// Cap a caller-requested page size to a service-defined range.
///
/// `default` applies when the caller did not specify a value; `max` is
/// the service's documented hard limit. Negative or zero values are
/// coerced to `1`. AWS itself rejects values out of range with a
/// validation error; services that want strict behavior should use
/// [`clamp_max_results_strict`] instead.
pub fn cap_max_results(requested: Option<i64>, default: usize, max: usize) -> usize {
    match requested {
        None => default.min(max),
        Some(n) if n < 1 => 1,
        Some(n) => (n as usize).min(max),
    }
}

/// Strict variant of [`cap_max_results`] that returns a validation error
/// instead of silently clamping. Use this when implementing services
/// whose documented contract is to reject `MaxResults` outside the
/// allowed range.
pub fn clamp_max_results_strict(
    requested: Option<i64>,
    default: usize,
    max: usize,
) -> Result<usize, AwsError> {
    let n = match requested {
        None => return Ok(default.min(max)),
        Some(n) => n,
    };
    if n < 1 {
        return Err(AwsError::bad_request(
            MAX_RESULTS_INVALID_CODE,
            format!("MaxResults must be at least 1, got {n}."),
        ));
    }
    let n = n as usize;
    if n > max {
        return Err(AwsError::bad_request(
            MAX_RESULTS_INVALID_CODE,
            format!("MaxResults must be at most {max}, got {n}."),
        ));
    }
    Ok(n)
}

/// Encode a resume marker as `{dup_before}:{key}`, where `dup_before` is the
/// number of already-returned items that share `key`.
fn join_marker(key: &str, dup_before: usize) -> String {
    format!("{dup_before}:{key}")
}

/// Parse a marker written by [`join_marker`]. Tolerates a bare key (treated as
/// `dup_before` 0) for tokens seeded outside [`paginate`].
fn split_marker(marker: &str) -> (String, usize) {
    if let Some((count, key)) = marker.split_once(':')
        && let Ok(dup) = count.parse::<usize>()
    {
        return (key.to_string(), dup);
    }
    (marker.to_string(), 0)
}

/// Paginate a sorted owned `Vec<T>`.
///
/// `key_fn` extracts the marker key from an item. Items must be sorted
/// by that same key for the resume-after-deletion behavior to work
/// correctly. The key need not be unique: the resume marker records how many
/// items sharing the boundary key were already returned, so duplicate keys do
/// not cause an item to be re-emitted or skipped.
///
/// `max_results` is the page size; the caller is expected to have
/// already applied any service-specific bounds via [`cap_max_results`]
/// or [`clamp_max_results_strict`].
pub fn paginate<T, F>(
    items: Vec<T>,
    max_results: usize,
    starting_token: Option<&str>,
    key_fn: F,
) -> Result<Page<T>, AwsError>
where
    F: Fn(&T) -> String,
{
    if max_results == 0 {
        return Ok(Page {
            items: Vec::new(),
            next_token: None,
        });
    }

    let start_idx = match starting_token {
        None => 0,
        Some(token) => {
            let (marker_key, dup_skip) = split_marker(&decode_token(token)?);
            let mut equal_seen = 0usize;
            items
                .iter()
                .position(|item| {
                    let k = key_fn(item);
                    if k < marker_key {
                        return false;
                    }
                    if k == marker_key && equal_seen < dup_skip {
                        equal_seen += 1;
                        return false;
                    }
                    true
                })
                .unwrap_or(items.len())
        }
    };

    let total_len = items.len();
    let take_n = max_results.min(total_len.saturating_sub(start_idx));
    let boundary_idx = start_idx + take_n;

    // Derive the resume marker before `items` is consumed.
    let next_token = (boundary_idx < total_len).then(|| {
        let boundary_key = key_fn(&items[boundary_idx]);
        let dup_before = items[..boundary_idx]
            .iter()
            .filter(|item| key_fn(item) == boundary_key)
            .count();
        encode_token(&join_marker(&boundary_key, dup_before))
    });

    let page_items: Vec<T> = items.into_iter().skip(start_idx).take(take_n).collect();

    Ok(Page {
        items: page_items,
        next_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &&'static str) -> String {
        (*s).to_string()
    }

    #[test]
    fn empty_input_returns_empty_page() {
        let page = paginate::<&str, _>(vec![], 10, None, key).unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_token.is_none());
    }

    #[test]
    fn page_smaller_than_max_results_no_token() {
        let items = vec!["alpha", "bravo", "charlie"];
        let page = paginate(items, 10, None, key).unwrap();
        assert_eq!(page.items, vec!["alpha", "bravo", "charlie"]);
        assert!(page.next_token.is_none());
    }

    #[test]
    fn page_exactly_full_no_token_when_no_more() {
        let items = vec!["alpha", "bravo", "charlie"];
        let page = paginate(items, 3, None, key).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(page.next_token.is_none());
    }

    #[test]
    fn page_full_with_more_emits_token() {
        let items = vec!["alpha", "bravo", "charlie", "delta"];
        let page = paginate(items.clone(), 2, None, key).unwrap();
        assert_eq!(page.items, vec!["alpha", "bravo"]);
        let token = page.next_token.expect("more items remain");
        let next = paginate(items, 2, Some(&token), key).unwrap();
        assert_eq!(next.items, vec!["charlie", "delta"]);
    }

    #[test]
    fn duplicate_keys_are_not_re_emitted_across_pages() {
        // Two of the four items share the sort key "2"; paging two at a time
        // must still yield each item exactly once.
        let items = vec![("a", 2), ("b", 2), ("c", 2), ("d", 3)];
        let key2 = |it: &(&'static str, i32)| it.1.to_string();
        let mut seen: Vec<&'static str> = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = paginate(items.clone(), 2, token.as_deref(), key2).unwrap();
            seen.extend(page.items.iter().map(|it| it.0));
            match page.next_token {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        assert_eq!(seen, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn resuming_with_token_returns_next_page() {
        let items = vec!["alpha", "bravo", "charlie", "delta"];
        let token = encode_token("charlie");
        let page = paginate(items, 2, Some(&token), key).unwrap();
        assert_eq!(page.items, vec!["charlie", "delta"]);
        assert!(page.next_token.is_none());
    }

    #[test]
    fn token_pointing_at_deleted_key_advances_to_next_present() {
        let items = vec!["alpha", "charlie", "delta"];
        let token = encode_token("bravo");
        let page = paginate(items, 10, Some(&token), key).unwrap();
        assert_eq!(page.items, vec!["charlie", "delta"]);
        assert!(page.next_token.is_none());
    }

    #[test]
    fn token_past_end_returns_empty_page() {
        let items = vec!["alpha", "bravo"];
        let token = encode_token("zzz");
        let page = paginate(items, 10, Some(&token), key).unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_token.is_none());
    }

    #[test]
    fn invalid_base64_token_returns_error() {
        let items = vec!["alpha"];
        let err = paginate(items, 10, Some("!!!not-base64!!!"), key).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn invalid_utf8_marker_returns_error() {
        let bad_marker = [0xff, 0xfe, 0xfd];
        let expiry = now_unix().saturating_add(TOKEN_TTL_SECONDS);
        let mut envelope = Vec::new();
        envelope.push(TOKEN_VERSION);
        envelope.extend_from_slice(&expiry.to_be_bytes());
        envelope.extend_from_slice(&bad_marker);
        let mut mac = HmacSha256::new_from_slice(signing_key()).unwrap();
        mac.update(&envelope);
        let tag = mac.finalize().into_bytes();
        envelope.extend_from_slice(&tag[..TAG_LEN]);
        let token = URL_SAFE_NO_PAD.encode(&envelope);

        let items = vec!["alpha"];
        let err = paginate(items, 10, Some(&token), key).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn tampered_token_is_rejected() {
        let token = encode_token("charlie");
        let mut bytes = URL_SAFE_NO_PAD.decode(&token).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = URL_SAFE_NO_PAD.encode(&bytes);
        let err = decode_token(&tampered).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn forged_token_with_wrong_key_is_rejected() {
        let foreign_key = [0u8; 32];
        let mut envelope = Vec::new();
        envelope.push(TOKEN_VERSION);
        envelope.extend_from_slice(&now_unix().saturating_add(TOKEN_TTL_SECONDS).to_be_bytes());
        envelope.extend_from_slice(b"charlie");
        let mut mac = HmacSha256::new_from_slice(&foreign_key).unwrap();
        mac.update(&envelope);
        let tag = mac.finalize().into_bytes();
        envelope.extend_from_slice(&tag[..TAG_LEN]);
        let forged = URL_SAFE_NO_PAD.encode(&envelope);

        let err = decode_token(&forged).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn expired_token_is_rejected() {
        let already_expired = now_unix().saturating_sub(60);
        let token = encode_token_with_expiry("charlie", already_expired);
        let err = decode_token(&token).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn wrong_version_byte_is_rejected() {
        let mut envelope = Vec::new();
        envelope.push(99);
        envelope.extend_from_slice(&now_unix().saturating_add(TOKEN_TTL_SECONDS).to_be_bytes());
        envelope.extend_from_slice(b"charlie");
        let mut mac = HmacSha256::new_from_slice(signing_key()).unwrap();
        mac.update(&envelope);
        let tag = mac.finalize().into_bytes();
        envelope.extend_from_slice(&tag[..TAG_LEN]);
        let token = URL_SAFE_NO_PAD.encode(&envelope);

        let err = decode_token(&token).unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn truncated_envelope_is_rejected() {
        let err = decode_token("YQ").unwrap_err();
        assert_eq!(err.code, TOKEN_INVALID_CODE);
    }

    #[test]
    fn round_trip_through_full_collection_yields_every_item_once() {
        let all: Vec<&'static str> = vec!["a", "b", "c", "d", "e", "f", "g"];
        let mut seen: Vec<&'static str> = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = paginate(all.clone(), 2, token.as_deref(), key).unwrap();
            seen.extend(page.items);
            match page.next_token {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        assert_eq!(seen, all);
    }

    #[test]
    fn cap_max_results_honors_default_when_unset() {
        assert_eq!(cap_max_results(None, 100, 1000), 100);
    }

    #[test]
    fn cap_max_results_caps_at_max() {
        assert_eq!(cap_max_results(Some(5000), 100, 1000), 1000);
    }

    #[test]
    fn cap_max_results_floors_at_one() {
        assert_eq!(cap_max_results(Some(0), 100, 1000), 1);
        assert_eq!(cap_max_results(Some(-3), 100, 1000), 1);
    }

    #[test]
    fn cap_max_results_caps_default_at_max() {
        assert_eq!(cap_max_results(None, 5000, 1000), 1000);
    }

    #[test]
    fn clamp_strict_accepts_in_range() {
        assert_eq!(clamp_max_results_strict(Some(50), 100, 1000).unwrap(), 50);
    }

    #[test]
    fn clamp_strict_uses_default_when_unset() {
        assert_eq!(clamp_max_results_strict(None, 100, 1000).unwrap(), 100);
    }

    #[test]
    fn clamp_strict_rejects_zero() {
        let err = clamp_max_results_strict(Some(0), 100, 1000).unwrap_err();
        assert_eq!(err.code, MAX_RESULTS_INVALID_CODE);
    }

    #[test]
    fn clamp_strict_rejects_above_max() {
        let err = clamp_max_results_strict(Some(2000), 100, 1000).unwrap_err();
        assert_eq!(err.code, MAX_RESULTS_INVALID_CODE);
    }

    #[test]
    fn clamp_strict_rejects_negative() {
        let err = clamp_max_results_strict(Some(-5), 100, 1000).unwrap_err();
        assert_eq!(err.code, MAX_RESULTS_INVALID_CODE);
    }
}
