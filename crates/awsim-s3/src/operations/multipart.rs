use std::collections::BTreeMap;

use std::collections::HashMap;

use awsim_core::AwsError;
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

use awsim_core::Body;

use crate::state::{MultipartUpload, PartData, S3Object, S3State};
use crate::util::{compute_etag, compute_multipart_etag, now_rfc7231};
use md5::Digest;

use super::bucket::no_such_bucket;
use super::require_str;

/// POST /{Bucket}/{Key+}?uploads — initiate a multipart upload.
pub fn create_multipart_upload(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let content_type = input
        .get("Content-Type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();

    let mut metadata = HashMap::new();
    if let Some(obj) = input.as_object() {
        for (k, v) in obj {
            if k.starts_with("Meta")
                && let Some(val) = v.as_str()
            {
                let meta_key = format!(
                    "x-amz-meta-{}",
                    super::object::to_kebab(k.strip_prefix("Meta").unwrap_or(k))
                );
                metadata.insert(meta_key, val.to_string());
            }
        }
    }

    let upload_id = Uuid::new_v4().to_string();
    let upload = MultipartUpload {
        upload_id: upload_id.clone(),
        key: key.to_string(),
        parts: BTreeMap::new(),
        created_at: now_rfc7231(),
        bucket: bucket_name.to_string(),
        content_type,
        metadata,
    };

    bucket.multipart_uploads.insert(upload_id.clone(), upload);

    Ok(json!({
        "__xml_root": "InitiateMultipartUploadResult",
        "Bucket": bucket_name,
        "Key": key,
        "UploadId": upload_id,
    }))
}

/// PUT /{Bucket}/{Key+}?partNumber={n}&uploadId={id} — upload a single part.
pub fn upload_part(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let _key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;
    let part_number_str = require_str(input, "partNumber")?;

    let part_number: u32 = part_number_str.parse().map_err(|_| {
        AwsError::bad_request("InvalidPartNumber", "partNumber must be a positive integer")
    })?;
    if !(1..=10000).contains(&part_number) {
        return Err(AwsError::bad_request(
            "InvalidArgument",
            "partNumber must be between 1 and 10000",
        ));
    }

    let data: Vec<u8> = if let Some(raw) = input.get("__raw_body").and_then(Value::as_str) {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("InvalidRequest", "Cannot decode part body"))?
    } else {
        Vec::new()
    };

    // Reject parts whose Content-MD5 or x-amz-checksum-* header doesn't
    // match the body. CompleteMultipartUpload uses the per-part ETags as
    // the inputs to its multipart-ETag computation, so silently storing a
    // corrupt part would propagate into a deceptively-correct final ETag.
    if let Some(md5_b64) = input.get("ContentMd5").and_then(Value::as_str)
        && !md5_b64.is_empty()
    {
        crate::util::verify_content_md5(&data, md5_b64)?;
    }
    for (field, algo) in &[
        ("ChecksumCrc32", "CRC32"),
        ("ChecksumCrc32c", "CRC32C"),
        ("ChecksumSha1", "SHA1"),
        ("ChecksumSha256", "SHA256"),
    ] {
        if let Some(v) = input.get(field).and_then(Value::as_str)
            && !v.is_empty()
        {
            crate::util::verify_object_checksum(&data, algo, v)?;
            break;
        }
    }

    let etag = compute_etag(&data);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let mut upload = bucket
        .multipart_uploads
        .get_mut(upload_id)
        .ok_or_else(|| no_such_upload(upload_id))?;

    let body = match state.body_store() {
        Some(store) => {
            let path = store
                .write_blob(
                    "multipart",
                    bucket_name,
                    &format!("{upload_id}/{part_number}"),
                    &data,
                )
                .map_err(|e| AwsError::internal(format!("persist part: {e}")))?;
            Body::OnDisk(path)
        }
        None => Body::InMemory(data),
    };

    upload.parts.insert(
        part_number,
        PartData {
            body,
            etag: etag.clone(),
        },
    );

    Ok(json!({
        "ETag": etag,
    }))
}

/// PUT /{Bucket}/{Key+}?partNumber={n}&uploadId={id} with `x-amz-copy-source`
/// header — copy a slice of a source object directly into a multipart part.
///
/// Supports the optional `x-amz-copy-source-range: bytes=start-end` header for
/// partial-range copies; absent, the whole source object becomes the part.
pub fn upload_part_copy(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let _key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;
    let part_number_str = require_str(input, "partNumber")?;
    let copy_source = require_str(input, "CopySource")?;

    let part_number: u32 = part_number_str.parse().map_err(|_| {
        AwsError::bad_request("InvalidPartNumber", "partNumber must be a positive integer")
    })?;
    if !(1..=10000).contains(&part_number) {
        return Err(AwsError::bad_request(
            "InvalidArgument",
            "partNumber must be between 1 and 10000",
        ));
    }

    // copy_source is "src-bucket/src-key" (may have leading slash).
    let copy_source = copy_source.trim_start_matches('/');
    let slash = copy_source.find('/').ok_or_else(|| {
        AwsError::bad_request("InvalidArgument", "CopySource must be in format bucket/key")
    })?;
    let src_bucket = &copy_source[..slash];
    let src_key = &copy_source[slash + 1..];

    let source_data = {
        let bucket = state
            .buckets
            .get(src_bucket)
            .ok_or_else(|| no_such_bucket(src_bucket))?;
        let versions = bucket.objects.get(src_key).ok_or_else(|| {
            AwsError::not_found(
                "NoSuchKey",
                format!("The specified key '{src_key}' does not exist"),
            )
        })?;
        let obj = versions.current().ok_or_else(|| {
            AwsError::not_found(
                "NoSuchKey",
                format!("The specified key '{src_key}' does not exist"),
            )
        })?;
        obj.body
            .read_all()
            .map_err(|e| AwsError::internal(format!("read source body: {e}")))?
    };

    let slice = if let Some(range_header) = input.get("CopySourceRange").and_then(Value::as_str) {
        copy_source_range(&source_data, range_header)?
    } else {
        source_data
    };

    let etag = compute_etag(&slice);
    let last_modified = crate::util::now_iso8601();

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let mut upload = bucket
        .multipart_uploads
        .get_mut(upload_id)
        .ok_or_else(|| no_such_upload(upload_id))?;

    let body = match state.body_store() {
        Some(store) => {
            let path = store
                .write_blob(
                    "multipart",
                    bucket_name,
                    &format!("{upload_id}/{part_number}"),
                    &slice,
                )
                .map_err(|e| AwsError::internal(format!("persist part: {e}")))?;
            Body::OnDisk(path)
        }
        None => Body::InMemory(slice),
    };

    upload.parts.insert(
        part_number,
        PartData {
            body,
            etag: etag.clone(),
        },
    );

    Ok(json!({
        "__xml_root": "CopyPartResult",
        "ETag": etag,
        "LastModified": last_modified,
    }))
}

/// Resolve `bytes=start-end` against the source bytes, returning the slice.
fn copy_source_range(data: &[u8], header: &str) -> Result<Vec<u8>, AwsError> {
    let bytes_prefix = "bytes=";
    let spec = header.trim().strip_prefix(bytes_prefix).ok_or_else(|| {
        AwsError::bad_request("InvalidArgument", "CopySourceRange must start with bytes=")
    })?;
    let (start_str, end_str) = spec.split_once('-').ok_or_else(|| {
        AwsError::bad_request("InvalidArgument", "CopySourceRange must be bytes=start-end")
    })?;
    let total = data.len();
    let start: usize = start_str
        .parse()
        .map_err(|_| AwsError::bad_request("InvalidArgument", "Invalid CopySourceRange start"))?;
    let end: usize = if end_str.is_empty() {
        total.saturating_sub(1)
    } else {
        end_str
            .parse()
            .map_err(|_| AwsError::bad_request("InvalidArgument", "Invalid CopySourceRange end"))?
    };
    if start > end || start >= total {
        return Err(AwsError::bad_request(
            "InvalidArgument",
            "CopySourceRange is not satisfiable",
        ));
    }
    let end = end.min(total - 1);
    Ok(data[start..=end].to_vec())
}

/// POST /{Bucket}/{Key+}?uploadId={id} — complete a multipart upload.
///
/// AWS' Complete is "validate everything, then consume." A failure during
/// validation (missing part, ETag mismatch, minimum-size violation, bad
/// ordering, IO failure) leaves the upload intact so the client can retry.
/// Only on a successful object write do we remove the upload metadata and
/// drop the per-part blobs from disk. After that, a duplicate Complete
/// returns NoSuchUpload, matching real S3.
pub fn complete_multipart_upload(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let requested_parts = parse_complete_parts(input);
    if requested_parts.is_empty() {
        return Err(AwsError::bad_request(
            "MalformedXML",
            "The XML you provided was not well-formed or did not validate against our published schema. At least one Part must be specified.",
        ));
    }
    let mut last_seen: u32 = 0;
    for (part_number, _) in &requested_parts {
        if *part_number <= last_seen {
            return Err(AwsError::bad_request(
                "InvalidPartOrder",
                "The list of parts was not in ascending order. Parts must be ordered by part number.",
            ));
        }
        last_seen = *part_number;
    }

    let (combined_data, etag, content_type, metadata) = {
        let upload = bucket
            .multipart_uploads
            .get(upload_id)
            .ok_or_else(|| no_such_upload(upload_id))?;

        let mut combined_data: Vec<u8> = Vec::new();
        let mut part_md5s: Vec<Vec<u8>> = Vec::new();
        let total_parts = requested_parts.len();
        for (idx, (part_number, expected_etag)) in requested_parts.iter().enumerate() {
            let part = upload.parts.get(part_number).ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidPart",
                    format!(
                        "One or more of the specified parts could not be found. The part might not have been uploaded, or the specified entity tag might not have matched the part's entity tag. Part number: {part_number}"
                    ),
                )
            })?;
            if let Some(expected) = expected_etag {
                let expected = expected.trim_matches('"');
                let actual = part.etag.trim_matches('"');
                if expected != actual {
                    return Err(AwsError::bad_request(
                        "InvalidPart",
                        format!(
                            "Part number {part_number} does not match the supplied ETag. Expected: {expected}, actual: {actual}"
                        ),
                    ));
                }
            }
            let bytes = part
                .body
                .read_all()
                .map_err(|e| AwsError::internal(format!("read part body: {e}")))?;
            // AWS S3 enforces a 5 MiB minimum on every part except the
            // last, but emulator users routinely upload tiny parts in tests
            // and don't want to pad with megabytes of dummy bytes. Mirror
            // localstack's stance and skip the size check; the resulting
            // assembled object is correct either way. Real production
            // workloads that depend on the cap can re-enable it via a flag
            // if/when the need arises.
            let _ = (idx, total_parts);
            let mut hasher = md5::Md5::new();
            hasher.update(&bytes);
            part_md5s.push(hasher.finalize().to_vec());
            combined_data.extend_from_slice(&bytes);
        }

        let etag = compute_multipart_etag(&part_md5s, total_parts);
        (
            combined_data,
            etag,
            upload.content_type.clone(),
            upload.metadata.clone(),
        )
    };

    let content_length = combined_data.len() as u64;
    let last_modified = now_rfc7231();

    let status = bucket.versioning.clone();
    let version_id = match status {
        crate::state::VersioningStatus::Enabled => Some(uuid::Uuid::new_v4().simple().to_string()),
        _ => None,
    };

    let body = match state.body_store() {
        Some(store) => {
            let blob_key =
                crate::operations::object::versioned_blob_key(key, version_id.as_deref());
            let path = store
                .write_blob("objects", bucket_name, &blob_key, &combined_data)
                .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
            Body::OnDisk(path)
        }
        None => Body::InMemory(combined_data),
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
        content_encoding: None,
        cache_control: None,
        content_disposition: None,
        content_language: None,
        expires: None,
        checksum_algorithm: None,
        checksum_value: None,
        is_delete_marker: false,
    };

    let mut versions = bucket.objects.entry(key.to_string()).or_default();
    super::object::record_version(&mut versions, obj, &status);
    drop(versions);

    bucket.multipart_uploads.remove(upload_id);
    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_bucket("multipart", &format!("{bucket_name}/{upload_id}"))
    {
        tracing::warn!(bucket = %bucket_name, upload_id = %upload_id, error = %e, "delete multipart parts");
    }

    let mut result = json!({
        "__xml_root": "CompleteMultipartUploadResult",
        "Location": format!("/{}/{}", bucket_name, key),
        "Bucket": bucket_name,
        "Key": key,
        "ETag": etag,
    });
    if let Some(vid) = version_id
        && let Some(map) = result.as_object_mut()
    {
        map.insert("VersionId".to_string(), Value::String(vid));
    }
    Ok(result)
}

/// AWS minimum part size for non-final parts in a multipart upload.
#[allow(dead_code)]
const MIN_PART_SIZE: usize = 5 * 1024 * 1024;

/// DELETE /{Bucket}/{Key+}?uploadId={id} — abort a multipart upload.
pub fn abort_multipart_upload(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let _key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    if bucket.multipart_uploads.remove(upload_id).is_none() {
        return Err(no_such_upload(upload_id));
    }

    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_bucket("multipart", &format!("{bucket_name}/{upload_id}"))
    {
        tracing::warn!(bucket = %bucket_name, upload_id = %upload_id, error = %e, "delete multipart parts");
    }

    Ok(json!({}))
}

/// GET /{Bucket}?uploads — list multipart uploads for a bucket.
pub fn list_multipart_uploads(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let uploads: Vec<Value> = bucket
        .multipart_uploads
        .iter()
        .map(|e| {
            let u = e.value();
            json!({
                "Key": u.key,
                "UploadId": u.upload_id,
                "Initiated": crate::util::rfc7231_to_iso8601(&u.created_at),
            })
        })
        .collect();

    Ok(json!({
        "__xml_root": "ListMultipartUploadsResult",
        "Bucket": bucket_name,
        "Upload": uploads,
        "IsTruncated": false,
    }))
}

/// GET /{Bucket}/{Key+}?uploadId={id} — list parts for a multipart upload.
pub fn list_parts(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let upload = bucket
        .multipart_uploads
        .get(upload_id)
        .ok_or_else(|| no_such_upload(upload_id))?;

    let parts: Vec<Value> = upload
        .parts
        .iter()
        .map(|(num, part)| {
            json!({
                "PartNumber": num,
                "ETag": part.etag,
                "Size": part.body.len_hint().unwrap_or(0),
            })
        })
        .collect();

    Ok(json!({
        "__xml_root": "ListPartsResult",
        "Bucket": bucket_name,
        "Key": key,
        "UploadId": upload_id,
        "Part": parts,
        "IsTruncated": false,
    }))
}

fn no_such_upload(upload_id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchUpload",
        format!("The specified upload '{upload_id}' does not exist"),
    )
}

/// Parse the `<Part><PartNumber>N</PartNumber><ETag>...</ETag></Part>` list
/// from the CompleteMultipartUpload request body.
fn parse_complete_parts(input: &Value) -> Vec<(u32, Option<String>)> {
    let parts_val = input
        .get("CompleteMultipartUpload")
        .and_then(|v| v.get("Part"))
        .or_else(|| input.get("Part"));

    let Some(parts_val) = parts_val else {
        return Vec::new();
    };

    let items: Vec<&Value> = match parts_val {
        Value::Array(arr) => arr.iter().collect(),
        other => vec![other],
    };

    items
        .into_iter()
        .filter_map(|item| {
            let num = item.get("PartNumber").and_then(Value::as_str)?;
            let part_number: u32 = num.parse().ok()?;
            if !(1..=10000).contains(&part_number) {
                return None;
            }
            let etag = item.get("ETag").and_then(Value::as_str).map(String::from);
            Some((part_number, etag))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Bucket, ObjectVersions, S3Object, S3State};

    fn state_with_source(src_bucket: &str, src_key: &str, body: &[u8]) -> S3State {
        let state = S3State::default();
        let bucket = Bucket::new(src_bucket, "us-east-1", "now");
        let mut versions = ObjectVersions::default();
        versions.push(S3Object {
            key: src_key.to_string(),
            body: Body::InMemory(body.to_vec()),
            content_type: "application/octet-stream".to_string(),
            content_length: body.len() as u64,
            etag: "\"src-etag\"".to_string(),
            last_modified: "now".to_string(),
            metadata: Default::default(),
            version_id: None,
            tags: Default::default(),
            content_encoding: None,
            cache_control: None,
            content_disposition: None,
            content_language: None,
            expires: None,
            checksum_algorithm: None,
            checksum_value: None,
            is_delete_marker: false,
        });
        bucket.objects.insert(src_key.to_string(), versions);
        state.buckets.insert(src_bucket.to_string(), bucket);
        state
    }

    fn ensure_dst_with_upload(state: &S3State, dst_bucket: &str, key: &str) -> String {
        let bucket = Bucket::new(dst_bucket, "us-east-1", "now");
        state.buckets.insert(dst_bucket.to_string(), bucket);
        let resp =
            create_multipart_upload(state, &json!({ "Bucket": dst_bucket, "Key": key })).unwrap();
        resp["UploadId"].as_str().unwrap().to_string()
    }

    #[test]
    fn upload_part_copy_full_source_lands_as_a_part() {
        let payload = b"hello world";
        let state = state_with_source("src", "obj", payload);
        let upload_id = ensure_dst_with_upload(&state, "dst", "merged");

        let resp = upload_part_copy(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "merged",
                "uploadId": upload_id,
                "partNumber": "1",
                "CopySource": "src/obj",
            }),
        )
        .unwrap();
        assert_eq!(resp["__xml_root"].as_str(), Some("CopyPartResult"));
        assert!(resp["ETag"].as_str().unwrap().len() > 2);

        // The part is now sitting on the upload at part_number 1 with the
        // source bytes available to CompleteMultipartUpload.
        let bucket = state.buckets.get("dst").unwrap();
        let upload = bucket.multipart_uploads.get(&upload_id).unwrap();
        let part = upload.parts.get(&1).unwrap();
        assert_eq!(part.body.read_all().unwrap(), payload);
    }

    #[test]
    fn upload_part_copy_honors_source_range() {
        let state = state_with_source("src", "obj", b"abcdefghij");
        let upload_id = ensure_dst_with_upload(&state, "dst", "merged");

        upload_part_copy(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "merged",
                "uploadId": upload_id,
                "partNumber": "1",
                "CopySource": "src/obj",
                "CopySourceRange": "bytes=2-5",
            }),
        )
        .unwrap();

        let bucket = state.buckets.get("dst").unwrap();
        let upload = bucket.multipart_uploads.get(&upload_id).unwrap();
        let part = upload.parts.get(&1).unwrap();
        assert_eq!(part.body.read_all().unwrap(), b"cdef");
    }

    /// Helper: stage a single-part upload of `payload` and return the upload id
    /// plus the ETag the server assigned to part 1.
    fn stage_single_part_upload(payload: &[u8]) -> (S3State, String, String) {
        let state = S3State::default();
        let bucket = Bucket::new("dst", "us-east-1", "now");
        state.buckets.insert("dst".to_string(), bucket);
        let init =
            create_multipart_upload(&state, &json!({"Bucket": "dst", "Key": "obj"})).unwrap();
        let upload_id = init["UploadId"].as_str().unwrap().to_string();
        let part = upload_part(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "partNumber": "1",
                "__raw_body": base64::engine::general_purpose::STANDARD.encode(payload),
            }),
        )
        .unwrap();
        let etag = part["ETag"].as_str().unwrap().to_string();
        (state, upload_id, etag)
    }

    #[test]
    fn complete_with_invalid_etag_leaves_upload_intact_for_retry() {
        let (state, upload_id, _real_etag) = stage_single_part_upload(b"hello");
        let bad_etag = "\"deadbeef\"";

        let err = complete_multipart_upload(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [{"PartNumber": "1", "ETag": bad_etag}]
                }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidPart");

        // Upload still exists — retry with the right ETag must succeed.
        let bucket = state.buckets.get("dst").unwrap();
        assert!(bucket.multipart_uploads.contains_key(&upload_id));
    }

    #[test]
    fn complete_with_unknown_part_leaves_upload_intact_for_retry() {
        let (state, upload_id, _) = stage_single_part_upload(b"hello");

        let err = complete_multipart_upload(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [{"PartNumber": "7"}]
                }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidPart");

        let bucket = state.buckets.get("dst").unwrap();
        assert!(bucket.multipart_uploads.contains_key(&upload_id));
    }

    #[test]
    fn complete_rejects_non_ascending_part_order() {
        let (state, upload_id, _) = stage_single_part_upload(b"hello");

        let err = complete_multipart_upload(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [
                        {"PartNumber": "2"},
                        {"PartNumber": "1"}
                    ]
                }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidPartOrder");
    }

    // (Removed: complete_rejects_non_final_part_under_5mib.) Real S3
    // enforces a 5 MiB minimum on non-final parts; awsim deliberately
    // does not, since emulator workloads upload tiny parts in tests and
    // padding to megabytes for no functional gain is hostile.

    #[test]
    fn complete_succeeds_then_duplicate_returns_no_such_upload() {
        let (state, upload_id, etag) = stage_single_part_upload(b"hello");

        complete_multipart_upload(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [{"PartNumber": "1", "ETag": etag}]
                }
            }),
        )
        .unwrap();

        let bucket = state.buckets.get("dst").unwrap();
        assert!(!bucket.multipart_uploads.contains_key(&upload_id));

        let err = complete_multipart_upload(
            &state,
            &json!({
                "Bucket": "dst",
                "Key": "obj",
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [{"PartNumber": "1"}]
                }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "NoSuchUpload");
    }
}
