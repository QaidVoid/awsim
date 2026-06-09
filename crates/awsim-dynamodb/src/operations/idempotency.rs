//! `ClientRequestToken` idempotency for transactional writes.
//!
//! `TransactWriteItems` and `ExecuteTransaction` accept a `ClientRequestToken`.
//! Within a 10-minute window, a repeated token with an identical payload
//! replays the original response without re-applying the writes, and a repeated
//! token with a different payload is rejected with
//! `IdempotentParameterMismatchException`. This module centralizes that logic so
//! both operations share one implementation.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::Value;

use crate::state::{DynamoState, IdempotencyEntry};

use super::opt_str;

/// AWS keeps a `ClientRequestToken` idempotent for 10 minutes.
const WINDOW_SECS: f64 = 600.0;

/// Outstanding idempotency bookkeeping for one in-flight transaction. Returned
/// by [`begin`]; call [`Guard::record`] with the successful response so an
/// identical retry can replay it.
pub struct Guard {
    /// `(cache key, fingerprint, now)` when a token was supplied; `None` when
    /// idempotency is disabled for this request (no token).
    pending: Option<(String, u64, f64)>,
}

impl Guard {
    /// Cache `response` under the request's token so a later identical call
    /// replays it. No-op when the request carried no `ClientRequestToken`.
    pub fn record(self, state: &DynamoState, response: &Value) {
        if let Some((key, fingerprint, stored_at)) = self.pending {
            state.idempotency.insert(
                key,
                IdempotencyEntry {
                    fingerprint,
                    response: response.clone(),
                    stored_at,
                },
            );
        }
    }
}

/// Begin an idempotent operation.
///
/// Returns the [`Guard`] to record the eventual response with, plus an optional
/// cached response: when `Some`, the caller must return it immediately without
/// running the operation (an idempotent replay). Returns
/// `IdempotentParameterMismatchException` when the token was last seen with a
/// different payload inside the window.
pub fn begin(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<(Guard, Option<Value>), AwsError> {
    let Some(token) = opt_str(input, "ClientRequestToken") else {
        return Ok((Guard { pending: None }, None));
    };
    let key = format!("{}:{}:{}", ctx.account_id, ctx.region, token);
    let fingerprint = fingerprint(input);
    let now = now_secs();

    if let Some(entry) = state.idempotency.get(&key)
        && now - entry.stored_at <= WINDOW_SECS
    {
        if entry.fingerprint == fingerprint {
            return Ok((Guard { pending: None }, Some(entry.response.clone())));
        }
        return Err(AwsError::bad_request(
            "IdempotentParameterMismatchException",
            "Request parameters do not match the parameters of a previous \
             call that used the same ClientRequestToken.",
        ));
    }

    // No entry, or an expired one we will overwrite on success.
    Ok((
        Guard {
            pending: Some((key, fingerprint, now)),
        },
        None,
    ))
}

/// Hash the request payload (excluding the token itself) so a retry with the
/// same parameters matches and one with changed parameters does not.
fn fingerprint(input: &Value) -> u64 {
    let mut payload = input.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("ClientRequestToken");
    }
    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    hasher.finish()
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
