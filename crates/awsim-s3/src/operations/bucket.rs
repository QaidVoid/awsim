use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Bucket, S3State};
use crate::util::now_iso8601;

use super::require_str;

/// GET / — list all buckets.
pub fn list_buckets(state: &S3State, ctx: &RequestContext) -> Result<Value, AwsError> {
    let mut buckets: Vec<Value> = state
        .buckets
        .iter()
        .map(|entry| {
            let b = entry.value();
            json!({
                "Name": b.name,
                "CreationDate": b.created_at,
            })
        })
        .collect();

    // Sort by name for deterministic output.
    buckets.sort_by(|a, b| {
        a.get("Name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(b.get("Name").and_then(Value::as_str).unwrap_or(""))
    });

    Ok(json!({
        "__xml_root": "ListAllMyBucketsResult",
        "Buckets": { "Bucket": buckets },
        "Owner": {
            "ID": ctx.account_id,
            "DisplayName": ctx.account_id,
        }
    }))
}

/// PUT /{Bucket} — create a bucket.
pub fn create_bucket(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    validate_bucket_name(bucket_name)?;

    if state.buckets.contains_key(bucket_name) {
        return Err(AwsError::conflict(
            "BucketAlreadyOwnedByYou",
            format!("The bucket '{bucket_name}' already exists and is owned by you"),
        ));
    }

    let bucket = Bucket::new(bucket_name, &ctx.region, now_iso8601());
    state.buckets.insert(bucket_name.to_string(), bucket);

    Ok(json!({}))
}

/// DELETE /{Bucket} — delete an empty bucket.
pub fn delete_bucket(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    if !bucket.objects.is_empty() {
        return Err(AwsError::conflict(
            "BucketNotEmpty",
            format!("The bucket '{bucket_name}' is not empty"),
        ));
    }

    drop(bucket);
    state.buckets.remove(bucket_name);

    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_bucket("objects", bucket_name)
    {
        tracing::warn!(bucket = %bucket_name, error = %e, "delete bucket bodies");
    }

    Ok(json!({}))
}

/// HEAD /{Bucket} — check if bucket exists.
pub fn head_bucket(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    if !state.buckets.contains_key(bucket_name) {
        return Err(no_such_bucket(bucket_name));
    }

    Ok(json!({}))
}

/// GET /{Bucket}?location — return bucket region.
pub fn get_bucket_location(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let location = if bucket.region == "us-east-1" {
        // S3 returns empty string for us-east-1 (the default region).
        String::new()
    } else {
        bucket.region.clone()
    };

    Ok(json!({ "LocationConstraint": location }))
}

/// Validate that a bucket name follows S3 naming rules.
fn validate_bucket_name(name: &str) -> Result<(), AwsError> {
    let len = name.len();
    if !(3..=63).contains(&len) {
        return Err(AwsError::bad_request(
            "InvalidBucketName",
            "Bucket name must be between 3 and 63 characters",
        ));
    }

    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '.' {
            return Err(AwsError::bad_request(
                "InvalidBucketName",
                format!("Bucket name '{name}' contains invalid characters"),
            ));
        }
    }

    if name.starts_with('-') || name.ends_with('-') || name.starts_with('.') || name.ends_with('.')
    {
        return Err(AwsError::bad_request(
            "InvalidBucketName",
            "Bucket name cannot start or end with a hyphen or period",
        ));
    }

    Ok(())
}

/// Shorthand for the NoSuchBucket error.
pub fn no_such_bucket(name: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchBucket",
        format!("The specified bucket '{name}' does not exist"),
    )
}
