use std::collections::HashMap;

use awsim_core::{AwsError, Body, RequestContext};
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{ObjectVersions, S3Object, S3State, VersioningStatus};
use crate::util::{compute_etag, now_iso8601, now_rfc7231};

use super::bucket::no_such_bucket;
use super::{opt_str, require_str};

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

    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    // Decode body: may be raw bytes (base64 in __raw_body) or a plain string.
    let data: Vec<u8> = if let Some(raw) = input.get("__raw_body").and_then(Value::as_str) {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("InvalidRequest", "Cannot decode request body"))?
    } else if let Some(body_str) = input.get("Body").and_then(Value::as_str) {
        // Client passed body as a string field.
        body_str.as_bytes().to_vec()
    } else {
        Vec::new()
    };

    // Collect x-amz-meta-* entries that arrived as PascalCase headers.
    // The gateway strips "x-amz-" prefix and converts to PascalCase via header_to_param_name.
    // We re-extract metadata from the input object: any key starting with "Meta" that isn't
    // a well-known field, we treat as user metadata.
    let content_type = opt_str(input, "ContentType")
        .unwrap_or("application/octet-stream")
        .to_string();

    let mut metadata = HashMap::new();
    if let Some(obj) = input.as_object() {
        for (k, v) in obj {
            // Look for keys that start with "Meta" (from x-amz-meta-* headers).
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
pub fn get_object(
    state: &S3State,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
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
    });

    if let Some(range) = content_range {
        result["ContentRange"] = json!(range);
    }

    if let Some(vid) = &obj.version_id {
        result["VersionId"] = Value::String(vid.clone());
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
pub fn head_object(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let requested_version = version_id_input(input);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key).ok_or_else(|| no_such_key(key))?;
    let obj = resolve_or_delete_marker(&versions, requested_version, key)?;

    let mut result = json!({
        "ContentType": obj.content_type,
        "ContentLength": obj.content_length,
        "ETag": obj.etag,
        "LastModified": obj.last_modified,
    });
    if let Some(vid) = &obj.version_id {
        result["VersionId"] = Value::String(vid.clone());
    }
    for (k, v) in &obj.metadata {
        result[k.clone()] = Value::String(v.clone());
    }
    Ok(result)
}

/// DELETE /{Bucket}/{Key+} — delete an object.
///
/// Behaviour depends on bucket versioning and whether `VersionId` is supplied:
///   * With `VersionId` — permanently remove that single version.
///   * Without, on Enabled bucket — append a delete marker (DeleteMarker=true).
///   * Without, on Suspended bucket — overwrite the `null`-version slot with
///     a delete marker.
///   * Without, on Disabled bucket — drop the (single) version entirely.
pub fn delete_object(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let requested_version = version_id_input(input);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

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
                let marker = S3Object {
                    key: key.to_string(),
                    body: Body::InMemory(Vec::new()),
                    content_type: "application/x-directory".to_string(),
                    content_length: 0,
                    etag: String::new(),
                    last_modified: now_rfc7231(),
                    metadata: Default::default(),
                    version_id: Some(dm_id.clone()),
                    tags: Default::default(),
                    is_delete_marker: true,
                };
                bucket
                    .objects
                    .entry(key.to_string())
                    .or_default()
                    .push(marker);
                response["DeleteMarker"] = Value::Bool(true);
                response["VersionId"] = Value::String(dm_id);
            }
            VersioningStatus::Suspended => {
                // Clean up the existing null-slot blob (if any) before
                // overwriting that slot with a delete marker.
                if let Some(store) = state.body_store() {
                    let blob_key = versioned_blob_key(key, None);
                    let _ = store.delete_blob("objects", bucket_name, &blob_key);
                }
                let marker = S3Object {
                    key: key.to_string(),
                    body: Body::InMemory(Vec::new()),
                    content_type: "application/x-directory".to_string(),
                    content_length: 0,
                    etag: String::new(),
                    last_modified: now_rfc7231(),
                    metadata: Default::default(),
                    version_id: None,
                    tags: Default::default(),
                    is_delete_marker: true,
                };
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
fn copy_object(state: &S3State, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
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
    let (data, content_type, metadata) = {
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
        is_delete_marker: false,
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
        return Err(AwsError::bad_request(
            "InvalidRange",
            "The requested range is not satisfiable",
        ));
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
        let head = head_object(&state, &json!({ "Bucket": "vbucket", "Key": "k" })).unwrap();
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
        let del = delete_object(&state, &json!({ "Bucket": "v", "Key": "k" })).unwrap();
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
        let head = head_object(&state, &json!({ "Bucket": "v", "Key": "k" })).unwrap();
        assert_eq!(head["VersionId"].as_str(), Some(v2.as_str()));
    }
}
