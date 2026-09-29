pub mod bucket;
pub mod config;
pub mod list;
pub mod multipart;
pub mod object;
pub mod post;
pub mod select;

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

/// The payload of an upload request, with any `aws-chunked` framing removed, and the trailers
/// that framing carried.
///
/// SigV4-streaming uploads signal the framing with `Content-Encoding: aws-chunked` and/or
/// `x-amz-content-sha256: STREAMING-...`, and send the post-decode length in
/// `x-amz-decoded-content-length`. A decoded body of any other length is refused rather than
/// stored as a partial object.
pub fn upload_body(input: &Value) -> Result<crate::util::DecodedChunked, AwsError> {
    use base64::Engine as _;

    let data: Vec<u8> = if let Some(raw) = opt_str(input, "__raw_body") {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("InvalidRequest", "Cannot decode request body"))?
    } else if let Some(body) = opt_str(input, "Body") {
        body.as_bytes().to_vec()
    } else {
        Vec::new()
    };

    let is_chunked = opt_str(input, "ContentEncoding").is_some_and(|v| {
        v.split(',')
            .any(|t| t.trim().eq_ignore_ascii_case("aws-chunked"))
    }) || opt_str(input, "ContentSha256")
        .is_some_and(|v| v.starts_with("STREAMING-"));
    if !is_chunked {
        return Ok((data, Vec::new()));
    }

    let (decoded, trailers) = crate::util::decode_aws_chunked_with_trailers(&data)?;
    if let Some(expected) =
        opt_str(input, "DecodedContentLength").and_then(|s| s.parse::<usize>().ok())
        && expected != decoded.len()
    {
        return Err(AwsError::bad_request(
            "InvalidRequest",
            format!(
                "x-amz-decoded-content-length {expected} does not match \
                 decoded body length {}",
                decoded.len()
            ),
        ));
    }
    Ok((decoded, trailers))
}

/// The `Content-Encoding` an object is stored with. `aws-chunked` describes the upload's
/// framing, not the object, so S3 drops it and keeps any real content codings.
pub fn stored_content_encoding(input: &Value) -> Option<String> {
    let kept: Vec<&str> = opt_str(input, "ContentEncoding")?
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("aws-chunked"))
        .collect();
    (!kept.is_empty()).then(|| kept.join(","))
}

/// Map an `x-amz-checksum-*` trailer name (lower-cased) to the algorithm name
/// `verify_object_checksum` expects, or `None` if it is not a checksum trailer.
pub fn trailer_checksum_algorithm(name: &str) -> Option<&'static str> {
    match name {
        "x-amz-checksum-crc32" => Some("CRC32"),
        "x-amz-checksum-crc32c" => Some("CRC32C"),
        "x-amz-checksum-crc64nvme" => Some("CRC64NVME"),
        "x-amz-checksum-sha1" => Some("SHA1"),
        "x-amz-checksum-sha256" => Some("SHA256"),
        _ => None,
    }
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
