//! Opaque-token pagination helper shared by every service's `List*` operation.
//!
//! AWS pagination tokens (`NextToken`, `NextMarker`, `ContinuationToken`,
//! `NextKeyMarker`, …) are opaque base64 strings. SDK clients send them back
//! verbatim and must not attempt to decode or compare them. This module
//! encodes a service-chosen marker string into a URL-safe base64 token, and
//! decodes it back when the client returns.
//!
//! Callers provide:
//! - a sorted `Vec<T>` of results (sorted by the same key used to derive the
//!   marker — typically alphabetical resource name)
//! - the page size requested by the caller (already capped via
//!   [`cap_max_results`])
//! - the optional starting token from the request
//! - a closure that extracts the marker key from each item
//!
//! The result is a [`Page<T>`] containing the items for this page plus the
//! token to resume from. The marker stored in the token is the key of the
//! *first item not yet returned*, so resuming a page hands back exactly the
//! next slice with no overlap or gap.
//!
//! Items whose keys compare lexicographically less than the marker are
//! skipped, which means the helper handles a resource being deleted between
//! list calls gracefully — it advances to the first key still present.

use crate::error::AwsError;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

const TOKEN_INVALID_CODE: &str = "InvalidParameterValue";
const TOKEN_INVALID_MSG: &str = "The pagination token is malformed or expired.";

/// One page of results plus the token to resume from.
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_token: Option<String>,
}

/// Encode a marker string as a URL-safe base64 token.
pub fn encode_token(marker: &str) -> String {
    URL_SAFE_NO_PAD.encode(marker.as_bytes())
}

/// Decode a URL-safe base64 token back to its marker string.
pub fn decode_token(token: &str) -> Result<String, AwsError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| AwsError::bad_request(TOKEN_INVALID_CODE, TOKEN_INVALID_MSG))?;
    String::from_utf8(bytes)
        .map_err(|_| AwsError::bad_request(TOKEN_INVALID_CODE, TOKEN_INVALID_MSG))
}

/// Cap a caller-requested page size to a service-defined range.
///
/// `default` applies when the caller did not specify a value; `max` is the
/// service's documented hard limit. Negative or zero values are coerced to
/// `1`. AWS itself also rejects values out of range with a validation error;
/// services that want strict behavior should validate before calling this.
pub fn cap_max_results(requested: Option<i64>, default: usize, max: usize) -> usize {
    match requested {
        None => default.min(max),
        Some(n) if n < 1 => 1,
        Some(n) => (n as usize).min(max),
    }
}

/// Paginate a sorted owned `Vec<T>`.
///
/// `key_fn` extracts the marker key from an item. Items must be sorted by
/// that same key for the resume-after-deletion behavior to work correctly.
///
/// `max_results` is the page size; the caller is expected to have already
/// applied any service-specific bounds via [`cap_max_results`].
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
            let marker = decode_token(token)?;
            items
                .iter()
                .position(|item| key_fn(item) >= marker)
                .unwrap_or(items.len())
        }
    };

    let total_len = items.len();
    let take_n = max_results.min(total_len.saturating_sub(start_idx));

    let mut iter = items.into_iter().skip(start_idx);
    let page_items: Vec<T> = iter.by_ref().take(take_n).collect();

    let next_token = iter.next().map(|next| encode_token(&key_fn(&next)));

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
        let page = paginate(items, 2, None, key).unwrap();
        assert_eq!(page.items, vec!["alpha", "bravo"]);
        assert_eq!(
            decode_token(page.next_token.as_deref().unwrap()).unwrap(),
            "charlie"
        );
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
    fn invalid_utf8_token_returns_error() {
        let bad = URL_SAFE_NO_PAD.encode([0xff, 0xfe, 0xfd]);
        let items = vec!["alpha"];
        let err = paginate(items, 10, Some(&bad), key).unwrap_err();
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
}
