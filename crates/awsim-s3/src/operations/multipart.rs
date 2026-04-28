use std::collections::BTreeMap;

use awsim_core::AwsError;
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

use awsim_core::Body;

use crate::state::{MultipartUpload, PartData, S3Object, S3State};
use crate::util::{compute_etag, now_rfc7231};

use super::bucket::no_such_bucket;
use super::require_str;

/// POST /{Bucket}/{Key+}?uploads — initiate a multipart upload.
pub fn create_multipart_upload(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let _bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let upload_id = Uuid::new_v4().to_string();
    let upload = MultipartUpload {
        upload_id: upload_id.clone(),
        key: key.to_string(),
        parts: BTreeMap::new(),
        created_at: now_rfc7231(),
        bucket: bucket_name.to_string(),
    };

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

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

    let data: Vec<u8> = if let Some(raw) = input.get("__raw_body").and_then(Value::as_str) {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("InvalidRequest", "Cannot decode part body"))?
    } else {
        Vec::new()
    };

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
        let obj = bucket.objects.get(src_key).ok_or_else(|| {
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
pub fn complete_multipart_upload(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;
    let upload_id = require_str(input, "uploadId")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let (_, upload) = bucket
        .multipart_uploads
        .remove(upload_id)
        .ok_or_else(|| no_such_upload(upload_id))?;

    let mut combined_data: Vec<u8> = Vec::new();
    for part in upload.parts.values() {
        let bytes = part
            .body
            .read_all()
            .map_err(|e| AwsError::internal(format!("read part body: {e}")))?;
        combined_data.extend_from_slice(&bytes);
    }

    let etag = compute_etag(&combined_data);
    let content_length = combined_data.len() as u64;
    let last_modified = now_rfc7231();

    let body = match state.body_store() {
        Some(store) => {
            let path = store
                .write_blob("objects", bucket_name, key, &combined_data)
                .map_err(|e| AwsError::internal(format!("persist object: {e}")))?;
            Body::OnDisk(path)
        }
        None => Body::InMemory(combined_data),
    };

    let version_id = match bucket.versioning {
        crate::state::VersioningStatus::Enabled => Some(uuid::Uuid::new_v4().simple().to_string()),
        _ => None,
    };

    let obj = S3Object {
        key: key.to_string(),
        body,
        content_type: "application/octet-stream".to_string(),
        content_length,
        etag: etag.clone(),
        last_modified,
        metadata: Default::default(),
        version_id: version_id.clone(),
        tags: Default::default(),
    };

    bucket.objects.insert(key.to_string(), obj);

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
        "Parts": parts,
        "IsTruncated": false,
    }))
}

fn no_such_upload(upload_id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchUpload",
        format!("The specified upload '{upload_id}' does not exist"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Bucket, S3Object, S3State};

    fn state_with_source(src_bucket: &str, src_key: &str, body: &[u8]) -> S3State {
        let state = S3State::default();
        let bucket = Bucket::new(src_bucket, "us-east-1", "now");
        bucket.objects.insert(
            src_key.to_string(),
            S3Object {
                key: src_key.to_string(),
                body: Body::InMemory(body.to_vec()),
                content_type: "application/octet-stream".to_string(),
                content_length: body.len() as u64,
                etag: "\"src-etag\"".to_string(),
                last_modified: "now".to_string(),
                metadata: Default::default(),
                version_id: None,
                tags: Default::default(),
            },
        );
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
}
