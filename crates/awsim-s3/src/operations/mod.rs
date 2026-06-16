pub mod bucket;
pub mod config;
pub mod list;
pub mod multipart;
pub mod object;
pub mod post;

use awsim_core::{AwsError, RequestContext};
use serde_json::Value;

/// Extract a required string field from a JSON Value.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(Value::as_str).ok_or_else(|| {
        AwsError::bad_request(
            "MissingParameter",
            format!("Missing required parameter: {key}"),
        )
    })
}

/// Extract an optional string field from a JSON Value.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

/// Enforce the `x-amz-expected-bucket-owner` header on a bucket operation.
///
/// AWS S3 lets a caller assert which account they expect to own the
/// bucket: requests carrying `x-amz-expected-bucket-owner: <account>`
/// are rejected with 403 `AccessDenied` when the bucket owner doesn't
/// match. AWSim stores every bucket inside a per-account
/// `AccountRegionStore` slot, so the implicit bucket owner is always
/// `ctx.account_id`. The check therefore reduces to "header value
/// equals the calling account".
///
/// Use this helper at the top of every bucket-scoped operation
/// (PutObject, GetObject, DeleteObject, CopyObject, ListObjects*,
/// CreateMultipartUpload, etc.) before doing the actual work, so a
/// mismatch is rejected before any side effects.
pub fn check_expected_bucket_owner(input: &Value, ctx: &RequestContext) -> Result<(), AwsError> {
    if let Some(expected) = opt_str(input, "ExpectedBucketOwner")
        && expected != ctx.account_id
    {
        return Err(AwsError::access_denied(format!(
            "The expected bucket owner ({expected}) does not match the actual bucket owner ({})",
            ctx.account_id
        )));
    }
    Ok(())
}
