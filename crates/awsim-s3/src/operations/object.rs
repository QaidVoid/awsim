use std::collections::HashMap;

use awsim_core::{AwsError, Body, RequestContext};
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{Bucket, ObjectVersions, S3Object, S3State, VersioningStatus};
use crate::util::{compute_etag, now_iso8601, now_rfc7231, parse_rfc7231};

use super::bucket::no_such_bucket;
use super::{opt_str, require_str};

/// Outcome of evaluating RFC 7232 conditional headers on a GET/HEAD.
enum ConditionOutcome {
    /// All conditions passed; serve the object normally.
    Proceed,
    /// `If-None-Match` matched (or `If-Modified-Since` failed): respond 304
    /// with metadata headers and no body.
    NotModified,
}

/// Check whether `key` is currently locked against modification or deletion
/// by an Object Lock retention or legal hold.
///
/// Real S3 enforces:
/// - Legal hold "ON" -> blocks delete and overwrite until cleared, regardless
///   of retention mode.
/// - Retention "GOVERNANCE" -> blocks until RetainUntilDate, but a caller
///   that supplies `x-amz-bypass-governance-retention: true` may proceed.
/// - Retention "COMPLIANCE" -> blocks until RetainUntilDate, never bypassable.
fn check_object_lock(bucket: &Bucket, key: &str, bypass_governance: bool) -> Result<(), AwsError> {
    if let Some(raw) = bucket.configs.get(&format!("legal-hold:{key}"))
        && let Ok(parsed) = serde_json::from_str::<Value>(raw)
        && parsed.get("Status").and_then(Value::as_str) == Some("ON")
    {
        return Err(AwsError::bad_request(
            "AccessDenied",
            format!("Object '{key}' is under a legal hold"),
        ));
    }

    if let Some(raw) = bucket.configs.get(&format!("retention:{key}"))
        && let Ok(parsed) = serde_json::from_str::<Value>(raw)
    {
        let mode = parsed
            .get("Mode")
            .and_then(Value::as_str)
            .unwrap_or("GOVERNANCE");
        let until = parsed
            .get("RetainUntilDate")
            .and_then(Value::as_str)
            .unwrap_or("");
        let until_secs = parse_iso8601_or_rfc7231(until);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Some(until_secs) = until_secs
            && until_secs > now
        {
            if mode == "COMPLIANCE" {
                return Err(AwsError::bad_request(
                    "AccessDenied",
                    format!("Object '{key}' is under a COMPLIANCE retention until {until}"),
                ));
            }
            if mode == "GOVERNANCE" && !bypass_governance {
                return Err(AwsError::bad_request(
                    "AccessDenied",
                    format!(
                        "Object '{key}' is under GOVERNANCE retention until {until}; \
                         set x-amz-bypass-governance-retention to override"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn parse_iso8601_or_rfc7231(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    if let Some(secs) = parse_rfc7231(s) {
        return Some(secs);
    }
    // Best-effort ISO 8601 parser: `YYYY-MM-DDTHH:MM:SS[.fff]Z`. Only the
    // up-to-second prefix is needed for retention comparisons.
    let main = s.split('.').next()?.trim_end_matches('Z');
    let mut parts = main.split('T');
    let date = parts.next()?;
    let time = parts.next().unwrap_or("00:00:00");
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let dd: i64 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next().unwrap_or("0").parse().ok()?;
    if y < 1970 {
        return None;
    }
    let is_leap = |yy: i64| (yy % 4 == 0 && yy % 100 != 0) || yy % 400 == 0;
    let mut days: i64 = 0;
    for yy in 1970..y {
        days += if is_leap(yy) { 366 } else { 365 };
    }
    let leap = is_leap(y);
    let monthly: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for &dim in monthly.iter().take((mo - 1) as usize) {
        days += dim;
    }
    days += dd - 1;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

/// Compare an `If-Match` / `If-None-Match` header value against an ETag.
///
/// The header may be `*` (matches any existing object), a single quoted
/// ETag, or a comma-separated list of quoted ETags. ETag matching is byte-
/// for-byte against the stored representation including surrounding quotes.
fn etag_list_matches(header: &str, etag: &str) -> bool {
    if header.trim() == "*" {
        return true;
    }
    header.split(',').any(|piece| piece.trim() == etag)
}

/// Evaluate RFC 7232 conditional headers (`If-Match`, `If-None-Match`,
/// `If-Modified-Since`, `If-Unmodified-Since`) for a GET or HEAD request.
///
/// Per RFC 7232 §6 and the S3 documentation, `If-Match` takes precedence
/// over `If-Unmodified-Since` (the latter is ignored when the former is
/// present), and `If-None-Match` takes precedence over `If-Modified-Since`.
fn check_get_conditions(obj: &S3Object, input: &Value) -> Result<ConditionOutcome, AwsError> {
    let if_match = opt_str(input, "IfMatch");
    let if_none_match = opt_str(input, "IfNoneMatch");
    let if_modified_since = opt_str(input, "IfModifiedSince");
    let if_unmodified_since = opt_str(input, "IfUnmodifiedSince");

    if let Some(header) = if_match {
        if !etag_list_matches(header, &obj.etag) {
            return Err(precondition_failed("If-Match header did not match ETag"));
        }
    } else if let Some(header) = if_unmodified_since
        && let (Some(req), Some(stored)) =
            (parse_rfc7231(header), parse_rfc7231(&obj.last_modified))
        && stored > req
    {
        return Err(precondition_failed(
            "If-Unmodified-Since header is older than object's last modification time",
        ));
    }

    if let Some(header) = if_none_match {
        if etag_list_matches(header, &obj.etag) {
            return Ok(ConditionOutcome::NotModified);
        }
    } else if let Some(header) = if_modified_since
        && let (Some(req), Some(stored)) =
            (parse_rfc7231(header), parse_rfc7231(&obj.last_modified))
        && stored <= req
    {
        return Ok(ConditionOutcome::NotModified);
    }

    Ok(ConditionOutcome::Proceed)
}

fn precondition_failed(message: &str) -> AwsError {
    AwsError::precondition_failed("PreconditionFailed", message)
}

/// Build a 304 Not Modified response carrying the ETag and Last-Modified
/// headers that AWS includes on conditional cache responses.
fn not_modified_response(obj: &S3Object) -> Value {
    let mut result = json!({
        "__raw_body": "",
        "__status_code": 304,
        "ETag": obj.etag,
        "LastModified": obj.last_modified,
    });
    if let Some(vid) = &obj.version_id {
        result["VersionId"] = Value::String(vid.clone());
    }
    result
}

/// When the bucket has versioning Enabled, generate a fresh opaque version ID;
/// otherwise leave the object un-versioned. (Suspended buckets emit `null` for
/// new puts, matching AWS behaviour.)
fn next_version_id(versioning: &VersioningStatus) -> Option<String> {
    match versioning {
        VersioningStatus::Enabled => Some(Uuid::new_v4().simple().to_string()),
        VersioningStatus::Suspended | VersioningStatus::Disabled => None,
    }
}

/// Append `obj` as a new version, but for Disabled / Suspended buckets the
/// existing un-versioned ("null") slot is replaced rather than retained.
pub fn record_version(versions: &mut ObjectVersions, obj: S3Object, status: &VersioningStatus) {
    if !matches!(status, VersioningStatus::Enabled) {
        // Disabled buckets only ever keep one entry; Suspended buckets keep
        // prior ID-bearing versions but overwrite the single "null" slot.
        versions.versions.retain(|o| o.version_id.is_some());
    }
    versions.push(obj);
}

/// Build the BodyStore "key" used to persist a single object version. Each
/// version gets its own blob — keying by `{key}@v={version_id_or_null}` keeps
/// historical bodies recoverable across snapshot/restore.
pub(crate) fn versioned_blob_key(key: &str, version_id: Option<&str>) -> String {
    let marker = version_id.unwrap_or("null");
    format!("{key}@v={marker}")
}

/// Read the caller's VersionId from input, accepting either the Smithy member
/// name (`VersionId`, used by JSON callers and our internal tests) or the wire
/// query parameter spelling (`versionId`, populated by the REST gateway from
/// `?versionId=...` on real SDK requests).
fn version_id_input(input: &Value) -> Option<&str> {
    input
        .get("VersionId")
        .or_else(|| input.get("versionId"))
        .and_then(Value::as_str)
}

/// Parse the `x-amz-checksum-*` integrity headers off a PutObject (or
/// CopyObject / UploadPart) input. Returns `(algorithm, value)` when the
/// caller supplied a precomputed checksum, or `(None, None)` otherwise.
///
/// AWS validates that the base64-decoded value has the right length for
/// the named algorithm:
///   CRC32 / CRC32C → 4 bytes (base64 `XXXXXXXX`, with `=` padding)
///   SHA1            → 20 bytes
///   SHA256          → 32 bytes
fn parse_request_checksum(input: &Value) -> Result<(Option<String>, Option<String>), AwsError> {
    // Each pair: (input field, algorithm name, expected decoded bytes)
    const FIELDS: &[(&str, &str, usize)] = &[
        ("ChecksumCrc32", "CRC32", 4),
        ("ChecksumCrc32c", "CRC32C", 4),
        ("ChecksumSha1", "SHA1", 20),
        ("ChecksumSha256", "SHA256", 32),
    ];
    for (field, algo, expected_bytes) in FIELDS {
        let Some(value) = opt_str(input, field) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| {
                AwsError::bad_request(
                    "InvalidRequest",
                    format!("{algo} checksum value is not valid base64"),
                )
            })?;
        if decoded.len() != *expected_bytes {
            return Err(AwsError::bad_request(
                "InvalidRequest",
                format!(
                    "{algo} checksum must decode to {expected_bytes} bytes, got {}",
                    decoded.len()
                ),
            ));
        }
        return Ok((Some(algo.to_string()), Some(value.to_string())));
    }
    Ok((None, None))
}

/// Pull the user-metadata sub-map out of the input. The protocol layer
/// converts incoming `x-amz-meta-*` headers into PascalCase keys
/// (`Meta<Suffix>`); this reverses that to the wire form so we store
/// them under the original `x-amz-meta-{name}` key.
fn extract_user_metadata(input: &Value) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    if let Some(obj) = input.as_object() {
        for (k, v) in obj {
            if k.starts_with("Meta")
                && let Some(val) = v.as_str()
            {
                let meta_key = format!(
                    "x-amz-meta-{}",
                    to_kebab(k.strip_prefix("Meta").unwrap_or(k))
                );
                metadata.insert(meta_key, val.to_string());
            }
        }
    }
    metadata
}

/// Strip an optional `?versionId=X` suffix from a CopySource value, returning
/// `(bucket_and_key, version_id)`.
fn split_copy_source_version(raw: &str) -> (&str, Option<&str>) {
    if let Some((path, query)) = raw.split_once('?') {
        for kv in query.split('&') {
            if let Some(v) = kv.strip_prefix("versionId=") {
                return (path, Some(v));
            }
        }
        (path, None)
    } else {
        (raw, None)
    }
}

/// Look up an object respecting an optional caller-supplied VersionId. Returns
/// the matched entry (which may itself be a delete marker — callers decide how
/// to react). When no VersionId is supplied, returns the latest non-DM entry.
fn resolve_version<'a>(
    versions: &'a ObjectVersions,
    version_id: Option<&str>,
) -> Option<&'a S3Object> {
    match version_id {
        Some(vid) => versions.find(vid),
        None => versions.current(),
    }
}

/// Read-side resolution: return the requested entry if it exists and is a
/// real object, otherwise build a NoSuchKey error that carries the
/// `DeleteMarker` / `VersionId` extras when the caller's read landed on a
/// tombstone (so the SDK sees the actual `x-amz-delete-marker` signal).
fn resolve_or_delete_marker<'a>(
    versions: &'a ObjectVersions,
    version_id: Option<&str>,
    key: &str,
) -> Result<&'a S3Object, AwsError> {
    // First try: the caller-requested version (or the latest entry).
    let entry = match version_id {
        Some(vid) => versions.find(vid),
        None => versions.latest(),
    };
    match entry {
        Some(obj) if !obj.is_delete_marker => Ok(obj),
        Some(obj) => {
            let mut err = no_such_key(key);
            if let Some(vid) = &obj.version_id {
                err = err.with_extra("VersionId", Value::String(vid.clone()));
            }
            err = err.with_extra("DeleteMarker", Value::Bool(true));
            Err(err)
        }
        None => Err(no_such_key(key)),
    }
}

/// PUT /{Bucket}/{Key+} — store an object.
/// If `x-amz-copy-source` header is present, this is a CopyObject.
pub fn put_object(state: &S3State, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    // CopyObject is distinguished by the CopySource header.
    if input.get("CopySource").is_some() {
        return copy_object(state, input, ctx);
    }

    super::check_expected_bucket_owner(input, ctx)?;
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    validate_object_key(key)?;

    // Decode body: may be raw bytes (base64 in __raw_body) or a plain string.
    let mut data: Vec<u8> = if let Some(raw) = input.get("__raw_body").and_then(Value::as_str) {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("InvalidRequest", "Cannot decode request body"))?
    } else if let Some(body_str) = input.get("Body").and_then(Value::as_str) {
        // Client passed body as a string field.
        body_str.as_bytes().to_vec()
    } else {
        Vec::new()
    };

    // SigV4-streaming uploads send the body as `aws-chunked` framing; the
    // SDK signals it with `Content-Encoding: aws-chunked` and/or
    // `x-amz-content-sha256: STREAMING-...`. Strip the framing so the
    // rest of the path operates on the raw body. The SDK also sends the
    // post-decode length in `x-amz-decoded-content-length`; if it
    // disagrees with what we decoded, surface an error rather than
    // storing a partial object.
    let is_chunked = opt_str(input, "ContentEncoding")
        .map(|v| v.eq_ignore_ascii_case("aws-chunked"))
        .unwrap_or(false)
        || opt_str(input, "ContentSha256")
            .map(|v| v.starts_with("STREAMING-"))
            .unwrap_or(false);
    if is_chunked {
        data = crate::util::decode_aws_chunked(&data)?;
        if let Some(expected) =
            opt_str(input, "DecodedContentLength").and_then(|s| s.parse::<usize>().ok())
            && expected != data.len()
        {
            return Err(AwsError::bad_request(
                "InvalidRequest",
                format!(
                    "x-amz-decoded-content-length {expected} does not match \
                     decoded body length {}",
                    data.len()
                ),
            ));
        }
    }

    // The 5 GiB single-PUT cap that real S3 enforces is applied at the
    // gateway via the `--max-s3-upload-bytes` flag: once that many bytes
    // have streamed in, axum aborts the body read and returns 413 before
    // this handler ever sees the request. We deliberately don't repeat
    // the check here so the configured CLI value is the single source
    // of truth.

    // Collect x-amz-meta-* entries that arrived as PascalCase headers.
    // The gateway strips "x-amz-" prefix and converts to PascalCase via header_to_param_name.
    // We re-extract metadata from the input object: any key starting with "Meta" that isn't
    // a well-known field, we treat as user metadata.
    let content_type = opt_str(input, "ContentType")
        .unwrap_or("application/octet-stream")
        .to_string();
    let content_encoding = opt_str(input, "ContentEncoding").map(String::from);
    let cache_control = opt_str(input, "CacheControl").map(String::from);
    let content_disposition = opt_str(input, "ContentDisposition").map(String::from);
    let content_language = opt_str(input, "ContentLanguage").map(String::from);
    let expires = opt_str(input, "Expires").map(String::from);

    let metadata = extract_user_metadata(input);
    let (checksum_algorithm, checksum_value) = parse_request_checksum(input)?;
    let bypass_governance = input
        .get("BypassGovernanceRetention")
        .and_then(Value::as_str)
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // PutObject on a non-versioned bucket overwrites the existing object,
    // which counts as a modification under Object Lock. On versioned buckets
    // the previous version is preserved, so the lock has nothing to defend.
    {
        let bucket = state
            .buckets
            .get(bucket_name)
            .ok_or_else(|| no_such_bucket(bucket_name))?;
        if matches!(bucket.versioning, VersioningStatus::Disabled) {
            check_object_lock(&bucket, key, bypass_governance)?;
        }
    }

    // Verify any caller-supplied integrity headers against the body before
    // we commit it to storage. Real S3 rejects mismatches with BadDigest.
    if let Some(md5_b64) = opt_str(input, "ContentMd5")
        && !md5_b64.is_empty()
    {
        crate::util::verify_content_md5(&data, md5_b64)?;
    }
    if let (Some(algo), Some(val)) = (checksum_algorithm.as_deref(), checksum_value.as_deref()) {
        crate::util::verify_object_checksum(&data, algo, val)?;
    }

    let content_length = data.len() as u64;
    let etag = compute_etag(&data);
    let last_modified = now_rfc7231();

    let version_id = {
        let bucket = state
            .buckets
            .get(bucket_name)
            .ok_or_else(|| no_such_bucket(bucket_name))?;

        let status = bucket.versioning.clone();
        let version_id = next_version_id(&status);

        let body = match state.body_store() {
            Some(store) => {
                let blob_key = versioned_blob_key(key, version_id.as_deref());
                let path = store
                    .write_blob("objects", bucket_name, &blob_key, &data)
                    .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
                Body::OnDisk(path)
            }
            None => Body::InMemory(data),
        };

        let obj = S3Object {
            key: key.to_string(),
            body,
            content_type,
            content_length,
            etag: etag.clone(),
            last_modified,
            metadata,
            version_id: version_id.clone(),
            tags: Default::default(),
            content_encoding,
            cache_control,
            content_disposition,
            content_language,
            expires,
            checksum_algorithm: checksum_algorithm.clone(),
            checksum_value: checksum_value.clone(),
            is_delete_marker: false,
        };

        // Suspended / Disabled buckets overwrite the existing null-slot
        // blob — purge it before we record the new version so the stale
        // file doesn't linger.
        if !matches!(status, VersioningStatus::Enabled)
            && let Some(store) = state.body_store()
            && let Some(versions_ref) = bucket.objects.get(key)
            && let Some(prev) = versions_ref
                .versions
                .iter()
                .find(|o| o.version_id.is_none())
            && !prev.is_delete_marker
        {
            let prev_key = versioned_blob_key(key, None);
            let _ = store.delete_blob("objects", bucket_name, &prev_key);
        }

        let mut versions = bucket.objects.entry(key.to_string()).or_default();
        record_version(&mut versions, obj, &status);
        version_id
    };

    let mut result = json!({ "ETag": etag });
    if let Some(vid) = version_id
        && let Some(map) = result.as_object_mut()
    {
        map.insert("VersionId".to_string(), Value::String(vid));
    }
    Ok(result)
}

/// GET /{Bucket}/{Key+} — retrieve object data, optionally a specific version.
pub fn get_object(state: &S3State, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    super::check_expected_bucket_owner(input, ctx)?;
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let range_header = opt_str(input, "Range");
    let requested_version = version_id_input(input);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key).ok_or_else(|| no_such_key(key))?;
    let obj = resolve_or_delete_marker(&versions, requested_version, key)?;

    if let ConditionOutcome::NotModified = check_get_conditions(obj, input)? {
        return Ok(not_modified_response(obj));
    }

    let body_bytes = obj.body.read_all().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            no_such_key(key)
        } else {
            AwsError::internal(format!("read object body: {e}"))
        }
    })?;
    let (data_slice, content_range) = apply_range(&body_bytes, range_header)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data_slice);

    let mut result = json!({
        "__raw_body": encoded,
        "ContentType": obj.content_type,
        "ContentLength": data_slice.len(),
        "ETag": obj.etag,
        "LastModified": obj.last_modified,
        "Body": encoded,
        "AcceptRanges": "bytes",
    });

    if let Some(range) = content_range {
        result["ContentRange"] = json!(range);
        result["__status_code"] = json!(206);
    }

    if let Some(vid) = &obj.version_id {
        result["VersionId"] = Value::String(vid.clone());
    }

    if let Some(ce) = &obj.content_encoding {
        result["ContentEncoding"] = Value::String(ce.clone());
    }
    if let Some(cc) = &obj.cache_control {
        result["CacheControl"] = Value::String(cc.clone());
    }
    if let Some(cd) = &obj.content_disposition {
        result["ContentDisposition"] = Value::String(cd.clone());
    }
    if let Some(cl) = &obj.content_language {
        result["ContentLanguage"] = Value::String(cl.clone());
    }
    if let Some(ex) = &obj.expires {
        result["Expires"] = Value::String(ex.clone());
    }

    // Surface the stored x-amz-checksum-* value when the caller asks
    // for it via ChecksumMode=ENABLED. AWS includes both the algorithm
    // name and the value-bearing field on a checksum-mode response.
    let want_checksum = opt_str(input, "ChecksumMode")
        .map(|m| m.eq_ignore_ascii_case("ENABLED"))
        .unwrap_or(false);
    if want_checksum
        && let (Some(algo), Some(value)) = (&obj.checksum_algorithm, &obj.checksum_value)
    {
        let field = match algo.as_str() {
            "CRC32" => "ChecksumCRC32",
            "CRC32C" => "ChecksumCRC32C",
            "SHA1" => "ChecksumSHA1",
            "SHA256" => "ChecksumSHA256",
            _ => "ChecksumSHA256",
        };
        result[field] = Value::String(value.clone());
    }

    // Add user metadata.
    for (k, v) in &obj.metadata {
        if let Some(obj_map) = result.as_object_mut() {
            obj_map.insert(k.clone(), json!(v));
        }
    }

    Ok(result)
}

/// HEAD /{Bucket}/{Key+} — return object metadata only.
pub fn head_object(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    super::check_expected_bucket_owner(input, ctx)?;
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let requested_version = version_id_input(input);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key).ok_or_else(|| no_such_key(key))?;
    let obj = resolve_or_delete_marker(&versions, requested_version, key)?;

    if let ConditionOutcome::NotModified = check_get_conditions(obj, input)? {
        return Ok(not_modified_response(obj));
    }

    let mut result = json!({
        "ContentType": obj.content_type,
        "ContentLength": obj.content_length,
        "ETag": obj.etag,
        "LastModified": obj.last_modified,
    });
    if let Some(vid) = &obj.version_id {
        result["VersionId"] = Value::String(vid.clone());
    }
    if let Some(ce) = &obj.content_encoding {
        result["ContentEncoding"] = Value::String(ce.clone());
    }
    if let Some(cc) = &obj.cache_control {
        result["CacheControl"] = Value::String(cc.clone());
    }
    if let Some(cd) = &obj.content_disposition {
        result["ContentDisposition"] = Value::String(cd.clone());
    }
    if let Some(cl) = &obj.content_language {
        result["ContentLanguage"] = Value::String(cl.clone());
    }
    if let Some(ex) = &obj.expires {
        result["Expires"] = Value::String(ex.clone());
    }
    result["AcceptRanges"] = json!("bytes");
    for (k, v) in &obj.metadata {
        result[k.clone()] = Value::String(v.clone());
    }
    Ok(result)
}

fn delete_marker_object(key: &str, version_id: Option<String>) -> S3Object {
    S3Object {
        key: key.to_string(),
        body: Body::InMemory(Vec::new()),
        content_type: "application/x-directory".to_string(),
        content_length: 0,
        etag: String::new(),
        last_modified: now_rfc7231(),
        metadata: Default::default(),
        version_id,
        tags: Default::default(),
        content_encoding: None,
        cache_control: None,
        content_disposition: None,
        content_language: None,
        expires: None,
        is_delete_marker: true,
        checksum_algorithm: None,
        checksum_value: None,
    }
}

/// DELETE /{Bucket}/{Key+} — delete an object.
///
/// Behaviour depends on bucket versioning and whether `VersionId` is supplied:
///   * With `VersionId` — permanently remove that single version.
///   * Without, on Enabled bucket — append a delete marker (DeleteMarker=true).
///   * Without, on Suspended bucket — overwrite the `null`-version slot with
///     a delete marker.
///   * Without, on Disabled bucket — drop the (single) version entirely.
pub fn delete_object(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    super::check_expected_bucket_owner(input, ctx)?;
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let requested_version = version_id_input(input);
    let bypass_governance = input
        .get("BypassGovernanceRetention")
        .and_then(Value::as_str)
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    // Object Lock applies to permanent deletes (per-version delete and
    // delete-on-non-versioned-bucket). Creating a delete marker on a
    // versioned bucket leaves the underlying versions untouched, so the
    // lock check is skipped in that case.
    let needs_lock_check =
        requested_version.is_some() || matches!(bucket.versioning, VersioningStatus::Disabled);
    if needs_lock_check {
        check_object_lock(&bucket, key, bypass_governance)?;
    }

    let mut response = json!({});

    if let Some(vid) = requested_version {
        // Permanent per-version delete — succeeds (no-op) when the VersionId
        // is unknown, matching real DynamoDB / S3 behaviour.
        let removed = if let Some(mut versions) = bucket.objects.get_mut(key) {
            let removed = versions.remove(vid);
            if versions.is_empty() {
                drop(versions);
                bucket.objects.remove(key);
            }
            removed
        } else {
            None
        };
        if let Some(obj) = removed {
            if !obj.is_delete_marker
                && let Some(store) = state.body_store()
            {
                let blob_key = versioned_blob_key(key, obj.version_id.as_deref());
                let _ = store.delete_blob("objects", bucket_name, &blob_key);
            }
            if let Some(rvid) = obj.version_id {
                response["VersionId"] = Value::String(rvid);
            } else {
                response["VersionId"] = Value::String("null".to_string());
            }
            if obj.is_delete_marker {
                response["DeleteMarker"] = Value::Bool(true);
            }
        }
    } else {
        let status = bucket.versioning.clone();
        match status {
            VersioningStatus::Disabled => {
                let removed = bucket.objects.remove(key);
                if let Some(store) = state.body_store()
                    && let Some((_, versions)) = removed
                {
                    for v in versions.versions {
                        if v.is_delete_marker {
                            continue;
                        }
                        let blob_key = versioned_blob_key(key, v.version_id.as_deref());
                        let _ = store.delete_blob("objects", bucket_name, &blob_key);
                    }
                }
            }
            VersioningStatus::Enabled => {
                let dm_id = Uuid::new_v4().simple().to_string();
                let marker = delete_marker_object(key, Some(dm_id.clone()));
                bucket
                    .objects
                    .entry(key.to_string())
                    .or_default()
                    .push(marker);
                response["DeleteMarker"] = Value::Bool(true);
                response["VersionId"] = Value::String(dm_id);
            }
            VersioningStatus::Suspended => {
                if let Some(store) = state.body_store() {
                    let blob_key = versioned_blob_key(key, None);
                    let _ = store.delete_blob("objects", bucket_name, &blob_key);
                }
                let marker = delete_marker_object(key, None);
                let mut versions = bucket.objects.entry(key.to_string()).or_default();
                versions.versions.retain(|o| o.version_id.is_some());
                versions.push(marker);
                response["DeleteMarker"] = Value::Bool(true);
                response["VersionId"] = Value::String("null".to_string());
            }
        }
    }

    Ok(response)
}

/// PUT /{Bucket}/{Key+} with x-amz-copy-source — copy an object.
fn copy_object(state: &S3State, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    super::check_expected_bucket_owner(input, ctx)?;
    // CopyObject also accepts ExpectedSourceBucketOwner; treat the
    // bucket owner of the source as the calling account because all
    // buckets in one process live in the same account slot.
    if let Some(expected) = super::opt_str(input, "ExpectedSourceBucketOwner")
        && expected != ctx.account_id
    {
        return Err(AwsError::access_denied(format!(
            "The expected source bucket owner ({expected}) does not match the actual owner ({})",
            ctx.account_id
        )));
    }
    let dst_bucket = require_str(input, "Bucket")?;
    let dst_key = require_str(input, "Key")?;
    let copy_source = require_str(input, "CopySource")?;

    // copy_source format: "src-bucket/src-key[?versionId=X]" (may start with /)
    let copy_source = copy_source.trim_start_matches('/');
    let (path, src_version) = split_copy_source_version(copy_source);
    let slash_pos = path.find('/').ok_or_else(|| {
        AwsError::bad_request("InvalidArgument", "CopySource must be in format bucket/key")
    })?;

    let src_bucket = &path[..slash_pos];
    let src_key = &path[slash_pos + 1..];

    // Read source object (possibly a specific historical version).
    let (
        data,
        src_content_type,
        src_metadata,
        src_content_encoding,
        src_cache_control,
        src_content_disposition,
        src_content_language,
        src_expires,
        src_version_id,
        src_etag,
        src_last_modified,
    ) = {
        let bucket = state
            .buckets
            .get(src_bucket)
            .ok_or_else(|| no_such_bucket(src_bucket))?;

        let versions = bucket
            .objects
            .get(src_key)
            .ok_or_else(|| no_such_key(src_key))?;
        let obj = resolve_version(&versions, src_version).ok_or_else(|| no_such_key(src_key))?;
        if obj.is_delete_marker {
            return Err(no_such_key(src_key));
        }

        (
            obj.body
                .read_all()
                .map_err(|e| AwsError::internal(format!("read source body: {e}")))?,
            obj.content_type.clone(),
            obj.metadata.clone(),
            obj.content_encoding.clone(),
            obj.cache_control.clone(),
            obj.content_disposition.clone(),
            obj.content_language.clone(),
            obj.expires.clone(),
            obj.version_id.clone(),
            obj.etag.clone(),
            obj.last_modified.clone(),
        )
    };

    // Conditional CopySource-If-* headers gate the copy on the *source*
    // object's ETag / Last-Modified. Real S3 returns 412 PreconditionFailed
    // for any failure (note: NoneMatch + ModifiedSince fail with 412 here,
    // not the 304 you would see on a regular GET).
    if let Some(header) = opt_str(input, "CopySourceIfMatch")
        && !etag_list_matches(header, &src_etag)
    {
        return Err(precondition_failed(
            "CopySourceIfMatch did not match source ETag",
        ));
    }
    if let Some(header) = opt_str(input, "CopySourceIfNoneMatch")
        && etag_list_matches(header, &src_etag)
    {
        return Err(precondition_failed(
            "CopySourceIfNoneMatch matched source ETag",
        ));
    }
    if let Some(header) = opt_str(input, "CopySourceIfUnmodifiedSince")
        && let (Some(req), Some(stored)) =
            (parse_rfc7231(header), parse_rfc7231(&src_last_modified))
        && stored > req
    {
        return Err(precondition_failed(
            "Source object was modified after CopySourceIfUnmodifiedSince",
        ));
    }
    if let Some(header) = opt_str(input, "CopySourceIfModifiedSince")
        && let (Some(req), Some(stored)) =
            (parse_rfc7231(header), parse_rfc7231(&src_last_modified))
        && stored <= req
    {
        return Err(precondition_failed(
            "Source object was not modified after CopySourceIfModifiedSince",
        ));
    }

    // MetadataDirective controls whether the destination's user metadata
    // and Content-* fields come from the source (COPY, default) or from
    // the request itself (REPLACE). Per AWS:
    //   COPY     — ignore request metadata; carry source metadata over
    //   REPLACE  — drop source metadata; use only what the request supplies
    let metadata_directive = opt_str(input, "MetadataDirective").unwrap_or("COPY");
    let (
        content_type,
        metadata,
        content_encoding,
        cache_control,
        content_disposition,
        content_language,
        expires,
    ) = if metadata_directive == "REPLACE" {
        (
            opt_str(input, "ContentType")
                .map(str::to_string)
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            extract_user_metadata(input),
            opt_str(input, "ContentEncoding").map(str::to_string),
            opt_str(input, "CacheControl").map(str::to_string),
            opt_str(input, "ContentDisposition").map(str::to_string),
            opt_str(input, "ContentLanguage").map(str::to_string),
            opt_str(input, "Expires").map(str::to_string),
        )
    } else {
        (
            src_content_type,
            src_metadata,
            src_content_encoding,
            src_cache_control,
            src_content_disposition,
            src_content_language,
            src_expires,
        )
    };

    let etag = compute_etag(&data);
    // Use RFC 7231 for the stored object (used in HTTP response headers like Last-Modified).
    let last_modified_http = now_rfc7231();
    // The CopyObjectResult XML body requires ISO 8601 / RFC 3339 timestamp.
    let last_modified_iso = now_iso8601();
    let content_length = data.len() as u64;

    let dst_bucket_ref = state
        .buckets
        .get(dst_bucket)
        .ok_or_else(|| no_such_bucket(dst_bucket))?;

    let status = dst_bucket_ref.versioning.clone();
    let version_id = next_version_id(&status);

    let body = match state.body_store() {
        Some(store) => {
            let blob_key = versioned_blob_key(dst_key, version_id.as_deref());
            let path = store
                .write_blob("objects", dst_bucket, &blob_key, &data)
                .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
            Body::OnDisk(path)
        }
        None => Body::InMemory(data),
    };

    let new_obj = S3Object {
        key: dst_key.to_string(),
        body,
        content_type,
        content_length,
        etag: etag.clone(),
        last_modified: last_modified_http,
        metadata,
        version_id: version_id.clone(),
        tags: Default::default(),
        content_encoding,
        cache_control,
        content_disposition,
        content_language,
        expires,
        is_delete_marker: false,
        checksum_algorithm: None,
        checksum_value: None,
    };

    // Same null-slot housekeeping as PutObject: clean up any prior null
    // blob on Disabled/Suspended copies before recording the new version.
    if !matches!(status, VersioningStatus::Enabled)
        && let Some(store) = state.body_store()
        && let Some(versions_ref) = dst_bucket_ref.objects.get(dst_key)
        && let Some(prev) = versions_ref
            .versions
            .iter()
            .find(|o| o.version_id.is_none())
        && !prev.is_delete_marker
    {
        let prev_key = versioned_blob_key(dst_key, None);
        let _ = store.delete_blob("objects", dst_bucket, &prev_key);
    }

    let mut versions = dst_bucket_ref
        .objects
        .entry(dst_key.to_string())
        .or_default();
    record_version(&mut versions, new_obj, &status);
    drop(versions);

    let mut result = json!({
        "CopyObjectResult": {
            "ETag": etag,
            "LastModified": last_modified_iso,
        }
    });
    if let Some(src_vid) = src_version_id
        && let Some(map) = result.as_object_mut()
    {
        map.insert("CopySourceVersionId".to_string(), Value::String(src_vid));
    }
    if let Some(vid) = version_id
        && let Some(map) = result.as_object_mut()
    {
        map.insert("VersionId".to_string(), Value::String(vid));
    }
    Ok(result)
}

// ─── Range handling ──────────────────────────────────────────────────────────

/// Parse a `Range: bytes=start-end` header and return the data slice + Content-Range string.
fn apply_range<'a>(
    data: &'a [u8],
    range: Option<&str>,
) -> Result<(&'a [u8], Option<String>), AwsError> {
    let Some(range_str) = range else {
        return Ok((data, None));
    };

    let range_str = range_str.trim();
    let bytes_prefix = "bytes=";
    if !range_str.starts_with(bytes_prefix) {
        return Err(AwsError::bad_request(
            "InvalidRange",
            "Unsupported range unit",
        ));
    }

    // A range request against a zero-byte object is treated as a non-range
    // GET (HTTP 200, full empty body) rather than 416. This matches AWS
    // behavior — the entire object IS what was requested.
    if data.is_empty() {
        return Ok((data, None));
    }

    let range_spec = &range_str[bytes_prefix.len()..];
    let parts: Vec<&str> = range_spec.splitn(2, '-').collect();

    if parts.len() != 2 {
        return Err(AwsError::bad_request(
            "InvalidRange",
            "Invalid range format",
        ));
    }

    let total = data.len();

    let start: usize = if parts[0].is_empty() {
        // Suffix range: bytes=-N
        let suffix_len: usize = parts[1]
            .parse()
            .map_err(|_| AwsError::bad_request("InvalidRange", "Invalid range value"))?;
        total.saturating_sub(suffix_len)
    } else {
        parts[0]
            .parse()
            .map_err(|_| AwsError::bad_request("InvalidRange", "Invalid range start"))?
    };

    let end: usize = if parts[1].is_empty() {
        total.saturating_sub(1)
    } else {
        let e: usize = parts[1]
            .parse()
            .map_err(|_| AwsError::bad_request("InvalidRange", "Invalid range end"))?;
        e.min(total.saturating_sub(1))
    };

    if start > end || start >= total {
        return Err(AwsError::range_not_satisfiable(
            "InvalidRange",
            "The requested range is not satisfiable",
        )
        .with_extra("ActualObjectSize", Value::from(total))
        .with_extra("RangeRequested", Value::String(range_str.to_string())));
    }

    let slice = &data[start..=end];
    let content_range = format!("bytes {start}-{end}/{total}");
    Ok((slice, Some(content_range)))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn no_such_key(key: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchKey",
        format!("The specified key '{key}' does not exist"),
    )
}

/// Validate an S3 object key against the documented constraints:
///
/// - Length 1..=1024 bytes (UTF-8). Empty keys are rejected as
///   InvalidRequest by AWS; over-1024 keys are rejected as
///   KeyTooLongError.
fn validate_object_key(key: &str) -> Result<(), AwsError> {
    if key.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidRequest",
            "Object key must not be empty",
        ));
    }
    // AWS caps object keys at 1024 *bytes* of UTF-8, not characters.
    // Rust's `str::len()` already returns the byte length, so a key of
    // 200 emoji (each 4 bytes wide) crosses the cap at the 257th char.
    let bytes = key.len();
    if bytes > 1024 {
        return Err(AwsError::bad_request(
            "KeyTooLongError",
            format!("Object key length {bytes} bytes exceeds the 1024-byte UTF-8 limit"),
        ));
    }
    Ok(())
}

/// Convert PascalCase to kebab-case for metadata key reconstruction.
pub fn to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Bucket, S3State};

    fn ctx() -> RequestContext {
        RequestContext::new("s3", "us-east-1")
    }

    fn state_with(bucket: Bucket) -> S3State {
        let state = S3State::default();
        state.buckets.insert(bucket.name.clone(), bucket);
        state
    }

    #[test]
    fn put_object_assigns_version_id_only_when_versioning_enabled() {
        // Versioning Disabled — no VersionId in response.
        let mut bucket = Bucket::new("plain", "us-east-1", "now");
        bucket.versioning = VersioningStatus::Disabled;
        let state = state_with(bucket);
        let resp = put_object(
            &state,
            &json!({ "Bucket": "plain", "Key": "k", "Body": "hi" }),
            &ctx(),
        )
        .unwrap();
        assert!(resp.get("VersionId").is_none(), "expected no VersionId");

        // Versioning Enabled — distinct VersionId per put.
        let mut bucket = Bucket::new("vbucket", "us-east-1", "now");
        bucket.versioning = VersioningStatus::Enabled;
        let state = state_with(bucket);
        let r1 = put_object(
            &state,
            &json!({ "Bucket": "vbucket", "Key": "k", "Body": "v1" }),
            &ctx(),
        )
        .unwrap();
        let r2 = put_object(
            &state,
            &json!({ "Bucket": "vbucket", "Key": "k", "Body": "v2" }),
            &ctx(),
        )
        .unwrap();
        let v1 = r1["VersionId"].as_str().expect("v1 has VersionId");
        let v2 = r2["VersionId"].as_str().expect("v2 has VersionId");
        assert_ne!(v1, v2, "successive puts must produce distinct VersionIds");

        // GetObject and HeadObject surface the current VersionId.
        let head =
            head_object(&state, &json!({ "Bucket": "vbucket", "Key": "k" }), &ctx()).unwrap();
        assert_eq!(head["VersionId"].as_str(), Some(v2));
    }

    #[test]
    fn enabled_bucket_keeps_history_and_supports_version_lookup() {
        let mut bucket = Bucket::new("v", "us-east-1", "now");
        bucket.versioning = VersioningStatus::Enabled;
        let state = state_with(bucket);
        let r1 = put_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "Body": "first" }),
            &ctx(),
        )
        .unwrap();
        let r2 = put_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "Body": "second" }),
            &ctx(),
        )
        .unwrap();
        let v1 = r1["VersionId"].as_str().unwrap().to_string();
        let v2 = r2["VersionId"].as_str().unwrap().to_string();

        // GetObject without VersionId returns the latest body.
        let latest = get_object(&state, &json!({ "Bucket": "v", "Key": "k" }), &ctx()).unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(latest["Body"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, b"second");

        // GetObject with the older VersionId returns the historical body.
        let historic = get_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "VersionId": v1 }),
            &ctx(),
        )
        .unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(historic["Body"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, b"first");

        // DeleteObject without VersionId pushes a delete marker; subsequent
        // GetObject sees NoSuchKey + DeleteMarker but the older VersionId
        // remains readable.
        let del = delete_object(&state, &json!({ "Bucket": "v", "Key": "k" }), &ctx()).unwrap();
        assert_eq!(del["DeleteMarker"], json!(true));

        let err = get_object(&state, &json!({ "Bucket": "v", "Key": "k" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "NoSuchKey");
        let extras = err.extras.as_ref().expect("DeleteMarker extras");
        assert_eq!(extras["DeleteMarker"], json!(true));

        let still_there = get_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "VersionId": v2 }),
            &ctx(),
        )
        .unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(still_there["Body"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, b"second");
    }

    #[test]
    fn delete_object_with_version_id_permanently_removes_only_that_version() {
        let mut bucket = Bucket::new("v", "us-east-1", "now");
        bucket.versioning = VersioningStatus::Enabled;
        let state = state_with(bucket);
        let r1 = put_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "Body": "a" }),
            &ctx(),
        )
        .unwrap();
        let r2 = put_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "Body": "b" }),
            &ctx(),
        )
        .unwrap();
        let v1 = r1["VersionId"].as_str().unwrap().to_string();
        let v2 = r2["VersionId"].as_str().unwrap().to_string();

        delete_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "VersionId": v1.clone() }),
            &ctx(),
        )
        .unwrap();

        // v1 is gone, v2 is still latest.
        let err = get_object(
            &state,
            &json!({ "Bucket": "v", "Key": "k", "VersionId": v1 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NoSuchKey");
        let head = head_object(&state, &json!({ "Bucket": "v", "Key": "k" }), &ctx()).unwrap();
        assert_eq!(head["VersionId"].as_str(), Some(v2.as_str()));
    }

    fn put_and_get_etag(state: &S3State, bucket: &str, key: &str, body: &str) -> String {
        let resp = put_object(
            state,
            &json!({ "Bucket": bucket, "Key": key, "Body": body }),
            &ctx(),
        )
        .unwrap();
        resp["ETag"].as_str().unwrap().to_string()
    }

    #[test]
    fn get_object_returns_412_when_if_match_does_not_match() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_and_get_etag(&state, "b", "k", "hello");

        let err = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "IfMatch": "\"deadbeef\"" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PreconditionFailed");
        assert_eq!(err.status.as_u16(), 412);
    }

    #[test]
    fn get_object_returns_object_when_if_match_matches() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let etag = put_and_get_etag(&state, "b", "k", "hello");

        let resp = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "IfMatch": etag }),
            &ctx(),
        )
        .unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(resp["Body"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, b"hello");
    }

    #[test]
    fn if_match_accepts_star_for_any_existing_object() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_and_get_etag(&state, "b", "k", "hello");
        let resp = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "IfMatch": "*" }),
            &ctx(),
        )
        .unwrap();
        assert!(resp.get("Body").is_some());
    }

    #[test]
    fn get_object_returns_304_when_if_none_match_matches() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let etag = put_and_get_etag(&state, "b", "k", "hello");

        let resp = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "IfNoneMatch": etag.clone() }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["__status_code"], json!(304));
        assert_eq!(resp["ETag"].as_str(), Some(etag.as_str()));
        // 304 must not include a body field.
        assert_eq!(resp["__raw_body"].as_str(), Some(""));
    }

    #[test]
    fn head_object_honors_if_none_match() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let etag = put_and_get_etag(&state, "b", "k", "hello");

        let resp = head_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "IfNoneMatch": etag }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["__status_code"], json!(304));
    }

    #[test]
    fn if_match_takes_precedence_over_if_unmodified_since() {
        // Per RFC 7232 §6: when If-Match succeeds, If-Unmodified-Since must
        // be ignored — even if the object was modified after the supplied
        // timestamp.
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let etag = put_and_get_etag(&state, "b", "k", "hello");

        let resp = get_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "IfMatch": etag,
                // Far in the past — would normally fail If-Unmodified-Since.
                "IfUnmodifiedSince": "Thu, 01 Jan 1970 00:00:00 GMT",
            }),
            &ctx(),
        )
        .unwrap();
        assert!(resp.get("Body").is_some());
    }

    #[test]
    fn put_object_stores_and_returns_checksum_with_mode_enabled() {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        // SHA-256 of "hello": precomputed.
        let sha256: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        let b64 = BASE64.encode(sha256);

        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "Body": "hello",
                "ChecksumSha256": b64.clone(),
            }),
            &ctx(),
        )
        .unwrap();

        // Without ChecksumMode the response shouldn't carry the value.
        let plain = get_object(&state, &json!({ "Bucket": "b", "Key": "k" }), &ctx()).unwrap();
        assert!(plain.get("ChecksumSHA256").is_none());

        // With ChecksumMode=ENABLED the response carries it.
        let checked = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "ChecksumMode": "ENABLED" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(checked["ChecksumSHA256"].as_str(), Some(b64.as_str()));
    }

    #[test]
    fn put_object_rejects_checksum_with_wrong_decoded_length() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        // 16 bytes base64-encoded — not 32, so SHA256 length check fails.
        let too_short = "AAAAAAAAAAAAAAAAAAAAAAAA";
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "Body": "hello",
                "ChecksumSha256": too_short,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidRequest");
    }

    #[test]
    fn put_object_rejects_keys_over_1024_bytes() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let huge_key = "a".repeat(1025);
        let err = put_object(
            &state,
            &json!({ "Bucket": "b", "Key": huge_key, "Body": "x" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "KeyTooLongError");
    }

    #[test]
    fn put_object_decodes_aws_chunked_body() {
        // SDK sends a SigV4-streaming PUT: ContentEncoding aws-chunked,
        // ContentSha256 marker, body framed with one chunk.
        use base64::Engine as _;
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let framed = b"5;chunk-signature=ab\r\nhello\r\n0;chunk-signature=cd\r\n";
        let raw_b64 = base64::engine::general_purpose::STANDARD.encode(framed);
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "__raw_body": raw_b64,
                "ContentEncoding": "aws-chunked",
                "ContentSha256": "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
                "DecodedContentLength": "5",
            }),
            &ctx(),
        )
        .unwrap();
        let got = get_object(&state, &json!({ "Bucket": "b", "Key": "k" }), &ctx()).unwrap();
        // Body comes back base64-encoded on the wire.
        let body_b64 = got["Body"].as_str().unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(body_b64)
            .unwrap();
        assert_eq!(body, b"hello");
        // Stored content length matches the decoded payload, not the framed
        // bytes — the chunk framing was stripped before storage.
        assert_eq!(got["ContentLength"].as_u64(), Some(5));
    }

    #[test]
    fn put_object_rejects_aws_chunked_with_wrong_decoded_length() {
        use base64::Engine as _;
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let framed = b"5;chunk-signature=ab\r\nhello\r\n0;chunk-signature=cd\r\n";
        let raw_b64 = base64::engine::general_purpose::STANDARD.encode(framed);
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "__raw_body": raw_b64,
                "ContentEncoding": "aws-chunked",
                "DecodedContentLength": "999",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidRequest");
    }

    #[test]
    fn put_object_rejects_multibyte_key_above_byte_limit() {
        // Each emoji is 4 UTF-8 bytes; 257 emoji = 1028 bytes, just over.
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let key: String = std::iter::repeat_n("\u{1F980}", 257).collect();
        assert_eq!(key.len(), 1028);
        let err = put_object(
            &state,
            &json!({ "Bucket": "b", "Key": key, "Body": "x" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "KeyTooLongError");
    }

    #[test]
    fn put_object_accepts_multibyte_key_at_byte_limit() {
        // 256 emoji = 1024 bytes exactly, must be accepted.
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let key: String = std::iter::repeat_n("\u{1F980}", 256).collect();
        assert_eq!(key.len(), 1024);
        put_object(
            &state,
            &json!({ "Bucket": "b", "Key": key, "Body": "x" }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn put_object_accepts_key_at_1024_byte_limit() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        let max_key = "a".repeat(1024);
        put_object(
            &state,
            &json!({ "Bucket": "b", "Key": max_key, "Body": "x" }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn copy_object_default_directive_carries_source_metadata() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        // Source object with custom Content-Type and a user metadata field.
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "src",
                "Body": "data",
                "ContentType": "text/plain",
                "MetaProject": "alpha",
            }),
            &ctx(),
        )
        .unwrap();

        // CopyObject without MetadataDirective defaults to COPY.
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
            }),
            &ctx(),
        )
        .unwrap();
        let head = head_object(&state, &json!({ "Bucket": "b", "Key": "dst" }), &ctx()).unwrap();
        assert_eq!(head["ContentType"].as_str(), Some("text/plain"));
        // User metadata round-trips on Head.
        assert_eq!(head["x-amz-meta-project"].as_str(), Some("alpha"));
    }

    #[test]
    fn copy_object_replace_directive_overwrites_metadata_from_request() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "src",
                "Body": "data",
                "ContentType": "text/plain",
                "MetaProject": "alpha",
            }),
            &ctx(),
        )
        .unwrap();

        // REPLACE: source metadata is dropped; only request metadata applies.
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "MetadataDirective": "REPLACE",
                "ContentType": "application/json",
                "MetaTeam": "infra",
            }),
            &ctx(),
        )
        .unwrap();
        let head = head_object(&state, &json!({ "Bucket": "b", "Key": "dst" }), &ctx()).unwrap();
        assert_eq!(head["ContentType"].as_str(), Some("application/json"));
        // New request metadata appears.
        assert_eq!(head["x-amz-meta-team"].as_str(), Some("infra"));
        // Source metadata does NOT appear.
        assert!(head.get("x-amz-meta-project").is_none());
    }

    #[test]
    fn range_on_zero_byte_object_returns_200_with_empty_body() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        // Zero-byte object.
        put_object(
            &state,
            &json!({ "Bucket": "b", "Key": "empty", "Body": "" }),
            &ctx(),
        )
        .unwrap();

        let resp = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "empty", "Range": "bytes=0-100" }),
            &ctx(),
        )
        .unwrap();
        // No 206 → returned as a normal 200 (no __status_code).
        assert!(resp.get("__status_code").is_none());
        // Empty body.
        let body = base64::engine::general_purpose::STANDARD
            .decode(resp["Body"].as_str().unwrap())
            .unwrap();
        assert!(body.is_empty());
    }

    #[test]
    fn unsatisfiable_range_returns_416_with_actual_object_size() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_and_get_etag(&state, "b", "k", "hello"); // body length 5

        // Request bytes well past end of object.
        let err = get_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "Range": "bytes=100-200" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.status.as_u16(), 416);
        assert_eq!(err.code, "InvalidRange");
        let extras = err.extras.as_ref().expect("extras");
        assert_eq!(extras["ActualObjectSize"], json!(5));
        assert_eq!(extras["RangeRequested"], json!("bytes=100-200"));
    }

    #[test]
    fn if_unmodified_since_in_past_returns_412_when_no_if_match() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_and_get_etag(&state, "b", "k", "hello");

        let err = get_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "IfUnmodifiedSince": "Thu, 01 Jan 1970 00:00:00 GMT",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PreconditionFailed");
    }

    fn put_with_legal_hold(state: &S3State, bucket: &str, key: &str) {
        put_and_get_etag(state, bucket, key, "hello");
        let mut b = state.buckets.get_mut(bucket).unwrap();
        b.configs.insert(
            format!("legal-hold:{key}"),
            r#"{"Status":"ON"}"#.to_string(),
        );
    }

    fn put_with_compliance_retention(state: &S3State, bucket: &str, key: &str, until_iso: &str) {
        put_and_get_etag(state, bucket, key, "hello");
        let mut b = state.buckets.get_mut(bucket).unwrap();
        b.configs.insert(
            format!("retention:{key}"),
            format!(r#"{{"Mode":"COMPLIANCE","RetainUntilDate":"{until_iso}"}}"#),
        );
    }

    fn put_with_governance_retention(state: &S3State, bucket: &str, key: &str, until_iso: &str) {
        put_and_get_etag(state, bucket, key, "hello");
        let mut b = state.buckets.get_mut(bucket).unwrap();
        b.configs.insert(
            format!("retention:{key}"),
            format!(r#"{{"Mode":"GOVERNANCE","RetainUntilDate":"{until_iso}"}}"#),
        );
    }

    #[test]
    fn legal_hold_blocks_delete_on_unversioned_bucket() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_with_legal_hold(&state, "b", "k");
        let err = delete_object(&state, &json!({ "Bucket": "b", "Key": "k" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn compliance_retention_blocks_delete_even_with_bypass() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_with_compliance_retention(&state, "b", "k", "2999-01-01T00:00:00Z");
        let err = delete_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "BypassGovernanceRetention": "true" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn governance_retention_can_be_bypassed_explicitly() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_with_governance_retention(&state, "b", "k", "2999-01-01T00:00:00Z");
        // Without the bypass header: blocked.
        let err = delete_object(&state, &json!({ "Bucket": "b", "Key": "k" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "AccessDenied");
        // With the bypass header: allowed.
        delete_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "BypassGovernanceRetention": "true" }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn expired_retention_does_not_block_delete() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_with_compliance_retention(&state, "b", "k", "1971-01-01T00:00:00Z");
        delete_object(&state, &json!({ "Bucket": "b", "Key": "k" }), &ctx()).unwrap();
    }

    #[test]
    fn legal_hold_blocks_overwrite_on_unversioned_bucket() {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = state_with(bucket);
        put_with_legal_hold(&state, "b", "k");
        let err = put_object(
            &state,
            &json!({ "Bucket": "b", "Key": "k", "Body": "second" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    fn put_then_get_etag(state: &S3State, bucket: &str, key: &str, body: &str) -> String {
        let r = put_object(
            state,
            &json!({ "Bucket": bucket, "Key": key, "Body": body }),
            &ctx(),
        )
        .unwrap();
        r["ETag"].as_str().unwrap().to_string()
    }

    #[test]
    fn copy_object_if_match_rejects_when_source_etag_differs() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        let _ = put_then_get_etag(&state, "b", "src", "hello");
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "CopySourceIfMatch": "\"deadbeef\"",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PreconditionFailed");
    }

    #[test]
    fn copy_object_if_match_passes_when_source_etag_matches() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        let etag = put_then_get_etag(&state, "b", "src", "hello");
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "CopySourceIfMatch": etag,
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn copy_object_if_none_match_rejects_when_source_etag_matches() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        let etag = put_then_get_etag(&state, "b", "src", "hello");
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "CopySourceIfNoneMatch": etag,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PreconditionFailed");
    }

    #[test]
    fn copy_object_if_unmodified_since_rejects_when_source_modified_later() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        put_then_get_etag(&state, "b", "src", "hello");
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "CopySourceIfUnmodifiedSince": "Thu, 01 Jan 1970 00:00:00 GMT",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PreconditionFailed");
    }

    fn other_account_ctx() -> RequestContext {
        RequestContext::new_with_account("s3", "us-east-1", "999999999999")
    }

    #[test]
    fn put_object_rejects_mismatched_expected_bucket_owner() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "Body": "hello",
                "ExpectedBucketOwner": "111111111111",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }

    #[test]
    fn put_object_accepts_matching_expected_bucket_owner() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        // Default ctx is account 000000000000.
        put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "k",
                "Body": "hello",
                "ExpectedBucketOwner": "000000000000",
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn get_object_rejects_mismatched_expected_bucket_owner() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        put_object(
            &state,
            &json!({"Bucket": "b", "Key": "k", "Body": "hi"}),
            &ctx(),
        )
        .unwrap();
        // Caller from a different account.
        let err = get_object(
            &state,
            &json!({"Bucket": "b", "Key": "k", "ExpectedBucketOwner": "000000000000"}),
            &other_account_ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }

    #[test]
    fn delete_object_rejects_mismatched_expected_bucket_owner() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        put_object(
            &state,
            &json!({"Bucket": "b", "Key": "k", "Body": "hi"}),
            &ctx(),
        )
        .unwrap();
        let err = delete_object(
            &state,
            &json!({"Bucket": "b", "Key": "k", "ExpectedBucketOwner": "555555555555"}),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }

    #[test]
    fn copy_object_checks_both_expected_owners() {
        let state = state_with(Bucket::new("b", "us-east-1", "now"));
        put_object(
            &state,
            &json!({"Bucket": "b", "Key": "src", "Body": "hi"}),
            &ctx(),
        )
        .unwrap();
        // Mismatched destination owner.
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "ExpectedBucketOwner": "555555555555",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
        // Mismatched source owner.
        let err = put_object(
            &state,
            &json!({
                "Bucket": "b",
                "Key": "dst",
                "CopySource": "b/src",
                "ExpectedSourceBucketOwner": "555555555555",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }
}
