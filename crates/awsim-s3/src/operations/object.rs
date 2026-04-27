use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use base64::Engine;
use serde_json::{Value, json};

use crate::state::{ObjectBody, S3Object, S3State};
use crate::util::{compute_etag, now_iso8601, now_rfc7231};

use super::bucket::no_such_bucket;
use super::{opt_str, require_str};

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

    {
        let bucket = state
            .buckets
            .get(bucket_name)
            .ok_or_else(|| no_such_bucket(bucket_name))?;

        let body = match state.body_store() {
            Some(store) => {
                let path = store
                    .write_object(bucket_name, key, &data)
                    .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
                ObjectBody::OnDisk(path)
            }
            None => ObjectBody::InMemory(data),
        };

        let obj = S3Object {
            key: key.to_string(),
            body,
            content_type,
            content_length,
            etag: etag.clone(),
            last_modified,
            metadata,
            version_id: None,
            tags: Default::default(),
        };

        bucket.objects.insert(key.to_string(), obj);
    }

    Ok(json!({
        "ETag": etag,
    }))
}

/// GET /{Bucket}/{Key+} — retrieve object data.
pub fn get_object(
    state: &S3State,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let range_header = opt_str(input, "Range");

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let obj = bucket.objects.get(key).ok_or_else(|| no_such_key(key))?;

    let body_bytes = obj
        .body
        .read_all()
        .map_err(|e| AwsError::internal(format!("read object body: {e}")))?;
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

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let obj = bucket.objects.get(key).ok_or_else(|| no_such_key(key))?;

    Ok(json!({
        "ContentType": obj.content_type,
        "ContentLength": obj.content_length,
        "ETag": obj.etag,
        "LastModified": obj.last_modified,
    }))
}

/// DELETE /{Bucket}/{Key+} — delete a single object.
pub fn delete_object(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.objects.remove(key);

    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_object(bucket_name, key)
    {
        tracing::warn!(bucket = %bucket_name, key = %key, error = %e, "delete object body");
    }

    Ok(json!({}))
}

/// PUT /{Bucket}/{Key+} with x-amz-copy-source — copy an object.
fn copy_object(state: &S3State, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let dst_bucket = require_str(input, "Bucket")?;
    let dst_key = require_str(input, "Key")?;
    let copy_source = require_str(input, "CopySource")?;

    // copy_source format: "src-bucket/src-key" (URL may start with /)
    let copy_source = copy_source.trim_start_matches('/');
    let slash_pos = copy_source.find('/').ok_or_else(|| {
        AwsError::bad_request("InvalidArgument", "CopySource must be in format bucket/key")
    })?;

    let src_bucket = &copy_source[..slash_pos];
    let src_key = &copy_source[slash_pos + 1..];

    // Read source object.
    let (data, content_type, metadata) = {
        let bucket = state
            .buckets
            .get(src_bucket)
            .ok_or_else(|| no_such_bucket(src_bucket))?;

        let obj = bucket
            .objects
            .get(src_key)
            .ok_or_else(|| no_such_key(src_key))?;

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

    let body = match state.body_store() {
        Some(store) => {
            let path = store
                .write_object(dst_bucket, dst_key, &data)
                .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
            ObjectBody::OnDisk(path)
        }
        None => ObjectBody::InMemory(data),
    };

    let new_obj = S3Object {
        key: dst_key.to_string(),
        body,
        content_type,
        content_length,
        etag: etag.clone(),
        last_modified: last_modified_http,
        metadata,
        version_id: None,
        tags: Default::default(),
    };

    dst_bucket_ref.objects.insert(dst_key.to_string(), new_obj);

    Ok(json!({
        "CopyObjectResult": {
            "ETag": etag,
            "LastModified": last_modified_iso,
        }
    }))
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
fn to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    result
}
