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

    // CreateBucketConfiguration.LocationConstraint must match the request
    // region when supplied. AWS rejects mismatches with
    // IllegalLocationConstraintException; us-east-1 is the special case
    // where omitting LocationConstraint or supplying "us-east-1" / "" /
    // "US" all map to the same region.
    if let Some(constraint) = parse_location_constraint(input) {
        if constraint != ctx.region && !(constraint == "US" && ctx.region == "us-east-1") {
            return Err(AwsError::bad_request(
                "IllegalLocationConstraintException",
                format!(
                    "The {constraint} location constraint is incompatible for the region \
                     specific endpoint this request was sent to ({region})",
                    region = ctx.region,
                ),
            ));
        }
    } else if ctx.region != "us-east-1" {
        // Outside us-east-1, AWS requires LocationConstraint to be
        // supplied — there is no implicit default.
        return Err(AwsError::bad_request(
            "IllegalLocationConstraintException",
            format!(
                "CreateBucket without a LocationConstraint is not allowed in {region}; \
                 supply CreateBucketConfiguration.LocationConstraint",
                region = ctx.region,
            ),
        ));
    }

    if state.buckets.contains_key(bucket_name) {
        return Err(AwsError::conflict(
            "BucketAlreadyOwnedByYou",
            format!("The bucket '{bucket_name}' already exists and is owned by you"),
        ));
    }

    let bucket = Bucket::new(bucket_name, &ctx.region, now_iso8601());
    state.buckets.insert(bucket_name.to_string(), bucket);

    Ok(json!({ "Location": format!("/{bucket_name}") }))
}

/// Pull `CreateBucketConfiguration.LocationConstraint` out of the input,
/// accepting it under either the modern field or the legacy
/// `CreateBucketConfiguration` flat shape.
fn parse_location_constraint(input: &Value) -> Option<String> {
    let raw = input
        .get("CreateBucketConfiguration")
        .and_then(|v| v.get("LocationConstraint"))
        .or_else(|| input.get("LocationConstraint"))?;
    let s = raw.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
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
    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_bucket("multipart", bucket_name)
    {
        tracing::warn!(bucket = %bucket_name, error = %e, "delete bucket multipart data");
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

    // Consecutive dots.
    if name.contains("..") {
        return Err(AwsError::bad_request(
            "InvalidBucketName",
            "Bucket name cannot contain consecutive periods",
        ));
    }

    // IP-address format.
    if name.split('.').all(|part| part.parse::<u8>().is_ok()) {
        return Err(AwsError::bad_request(
            "InvalidBucketName",
            "Bucket name must not be formatted as an IP address",
        ));
    }

    // Reserved prefixes/suffixes.
    if name.starts_with("xn--")
        || name.ends_with("-s3alias")
        || name.ends_with("--ol-s3")
        || name.ends_with("--x-s3")
    {
        return Err(AwsError::bad_request(
            "InvalidBucketName",
            format!("Bucket name '{name}' uses a reserved prefix or suffix"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_in(region: &str) -> RequestContext {
        RequestContext::new("s3", region)
    }

    #[test]
    fn create_bucket_in_us_east_1_without_location_constraint_succeeds() {
        let state = S3State::default();
        create_bucket(
            &state,
            &json!({ "Bucket": "test-bucket" }),
            &ctx_in("us-east-1"),
        )
        .unwrap();
    }

    #[test]
    fn create_bucket_in_other_region_requires_location_constraint() {
        let state = S3State::default();
        let err = create_bucket(
            &state,
            &json!({ "Bucket": "test-bucket" }),
            &ctx_in("eu-west-1"),
        )
        .unwrap_err();
        assert_eq!(err.code, "IllegalLocationConstraintException");
    }

    #[test]
    fn create_bucket_rejects_mismatched_location_constraint() {
        let state = S3State::default();
        let err = create_bucket(
            &state,
            &json!({
                "Bucket": "test-bucket",
                "CreateBucketConfiguration": { "LocationConstraint": "us-west-2" },
            }),
            &ctx_in("eu-west-1"),
        )
        .unwrap_err();
        assert_eq!(err.code, "IllegalLocationConstraintException");
    }

    #[test]
    fn create_bucket_accepts_matching_location_constraint() {
        let state = S3State::default();
        create_bucket(
            &state,
            &json!({
                "Bucket": "test-bucket",
                "CreateBucketConfiguration": { "LocationConstraint": "eu-west-1" },
            }),
            &ctx_in("eu-west-1"),
        )
        .unwrap();
    }

    #[test]
    fn create_bucket_accepts_legacy_us_constraint_in_us_east_1() {
        let state = S3State::default();
        create_bucket(
            &state,
            &json!({
                "Bucket": "test-bucket",
                "CreateBucketConfiguration": { "LocationConstraint": "US" },
            }),
            &ctx_in("us-east-1"),
        )
        .unwrap();
    }
}
