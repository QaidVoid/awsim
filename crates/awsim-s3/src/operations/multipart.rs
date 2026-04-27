use std::collections::BTreeMap;

use awsim_core::AwsError;
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{MultipartUpload, ObjectBody, PartData, S3Object, S3State};
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

    upload.parts.insert(
        part_number,
        PartData {
            body: ObjectBody::InMemory(data),
            etag: etag.clone(),
        },
    );

    Ok(json!({
        "ETag": etag,
    }))
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

    let obj = S3Object {
        key: key.to_string(),
        body: ObjectBody::InMemory(combined_data),
        content_type: "application/octet-stream".to_string(),
        content_length,
        etag: etag.clone(),
        last_modified,
        metadata: Default::default(),
        version_id: None,
        tags: Default::default(),
    };

    bucket.objects.insert(key.to_string(), obj);

    Ok(json!({
        "__xml_root": "CompleteMultipartUploadResult",
        "Location": format!("/{}/{}", bucket_name, key),
        "Bucket": bucket_name,
        "Key": key,
        "ETag": etag,
    }))
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

    // S3 returns 404 if the upload doesn't exist.
    if bucket.multipart_uploads.remove(upload_id).is_none() {
        return Err(no_such_upload(upload_id));
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
