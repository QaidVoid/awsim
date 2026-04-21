use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::{S3State, VersioningStatus};

use super::require_str;
use super::bucket::no_such_bucket;

// ─── Tagging ─────────────────────────────────────────────────────────────────

/// PUT /{Bucket}?tagging
pub fn put_bucket_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let tags = parse_tags(input);

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.tags = tags;
    Ok(json!({}))
}

/// GET /{Bucket}?tagging
pub fn get_bucket_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let tags: Vec<Value> = bucket
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "TagSet": { "Tag": tags } }))
}

/// DELETE /{Bucket}?tagging
pub fn delete_bucket_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.tags.clear();
    Ok(json!({}))
}

// ─── Versioning ───────────────────────────────────────────────────────────────

/// PUT /{Bucket}?versioning
pub fn put_bucket_versioning(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    // The XML body is parsed into a map by the core gateway.
    // Expected structure: {"VersioningConfiguration": {"Status": "Enabled"}}
    let status_str = input
        .get("VersioningConfiguration")
        .and_then(|v| v.get("Status"))
        .and_then(Value::as_str)
        .or_else(|| input.get("Status").and_then(Value::as_str))
        .unwrap_or("");

    let versioning = match status_str {
        "Enabled" => VersioningStatus::Enabled,
        "Suspended" => VersioningStatus::Suspended,
        _ => VersioningStatus::Disabled,
    };

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.versioning = versioning;
    Ok(json!({}))
}

/// GET /{Bucket}?versioning
pub fn get_bucket_versioning(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let status = bucket.versioning.as_str();

    if status.is_empty() {
        Ok(json!({ "VersioningConfiguration": {} }))
    } else {
        Ok(json!({ "VersioningConfiguration": { "Status": status } }))
    }
}

// ─── Policy ──────────────────────────────────────────────────────────────────

/// PUT /{Bucket}?policy
pub fn put_bucket_policy(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    // The policy JSON comes in as the raw body; if it arrived as XML-parsed,
    // it might be in __raw_body (base64). Otherwise the body itself is the policy.
    let policy = if let Some(raw) = input.get("__raw_body").and_then(Value::as_str) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AwsError::bad_request("MalformedPolicy", "Cannot decode policy body"))?;
        String::from_utf8(bytes)
            .map_err(|_| AwsError::bad_request("MalformedPolicy", "Policy is not valid UTF-8"))?
    } else {
        // Body was valid JSON; serialize it back.
        input.to_string()
    };

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.policy = Some(policy);
    Ok(json!({}))
}

/// GET /{Bucket}?policy
pub fn get_bucket_policy(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match &bucket.policy {
        Some(policy) => Ok(json!({ "Policy": policy })),
        None => Err(AwsError::not_found(
            "NoSuchBucketPolicy",
            format!("The bucket '{bucket_name}' policy does not exist"),
        )),
    }
}

/// DELETE /{Bucket}?policy
pub fn delete_bucket_policy(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.policy = None;
    Ok(json!({}))
}

// ─── CORS ─────────────────────────────────────────────────────────────────────

/// PUT /{Bucket}?cors
pub fn put_bucket_cors(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let cors_config = input.to_string();

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.cors = Some(cors_config);
    Ok(json!({}))
}

/// GET /{Bucket}?cors
pub fn get_bucket_cors(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match &bucket.cors {
        Some(cors) => {
            // Parse back so we return structured data.
            let parsed: Value = serde_json::from_str(cors).unwrap_or(json!({}));
            Ok(parsed)
        }
        None => Err(AwsError::not_found(
            "NoSuchCORSConfiguration",
            format!("The CORS configuration for bucket '{bucket_name}' does not exist"),
        )),
    }
}

/// DELETE /{Bucket}?cors
pub fn delete_bucket_cors(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.cors = None;
    Ok(json!({}))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse tags from the XML-parsed input.
/// Expected: {"Tagging": {"TagSet": {"Tag": [...]}}} or similar.
fn parse_tags(input: &Value) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    // Navigate: Tagging → TagSet → Tag (may be array or single object)
    let tag_list = input
        .get("Tagging")
        .and_then(|v| v.get("TagSet"))
        .and_then(|v| v.get("Tag"))
        .or_else(|| input.get("TagSet").and_then(|v| v.get("Tag")));

    let Some(tag_list) = tag_list else {
        return tags;
    };

    match tag_list {
        Value::Array(arr) => {
            for tag in arr {
                if let (Some(k), Some(v)) = (
                    tag.get("Key").and_then(Value::as_str),
                    tag.get("Value").and_then(Value::as_str),
                ) {
                    tags.insert(k.to_string(), v.to_string());
                }
            }
        }
        Value::Object(_) => {
            if let (Some(k), Some(v)) = (
                tag_list.get("Key").and_then(Value::as_str),
                tag_list.get("Value").and_then(Value::as_str),
            ) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
        _ => {}
    }

    tags
}
