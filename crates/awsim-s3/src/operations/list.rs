use std::collections::BTreeSet;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::S3State;

use super::require_str;
use super::bucket::no_such_bucket;

/// GET /{Bucket}?list-type=2 — list objects with prefix/delimiter/pagination.
pub fn list_objects_v2(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let prefix = input.get("prefix").and_then(Value::as_str).unwrap_or("");
    let delimiter = input.get("delimiter").and_then(Value::as_str).unwrap_or("");
    let max_keys: usize = input
        .get("max-keys")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| input.get("max-keys").and_then(Value::as_u64).map(|n| n as usize))
        .unwrap_or(1000)
        .min(1000);
    let continuation_token = input
        .get("continuation-token")
        .and_then(Value::as_str)
        .unwrap_or("");

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    // Collect and sort all keys that match the prefix.
    let mut matching_keys: Vec<String> = bucket
        .objects
        .iter()
        .map(|e| e.key().clone())
        .filter(|k| k.starts_with(prefix))
        .collect();
    matching_keys.sort();

    // Apply continuation token (start after this token alphabetically).
    let matching_keys: Vec<String> = if continuation_token.is_empty() {
        matching_keys
    } else {
        matching_keys
            .into_iter()
            .filter(|k| k.as_str() > continuation_token)
            .collect()
    };

    let mut contents: Vec<Value> = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    let mut key_count = 0usize;
    let mut next_continuation_token: Option<String> = None;

    for key in &matching_keys {
        if key_count >= max_keys {
            next_continuation_token = Some(key.clone());
            break;
        }

        // If delimiter is set, check if the key (after stripping prefix) contains the delimiter.
        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                // This key collapses into a common prefix.
                let common_prefix = format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter);
                common_prefixes.insert(common_prefix);
                // Don't count against key_count — common prefixes are counted separately.
                continue;
            }
        }

        // Full object entry.
        if let Some(obj) = bucket.objects.get(key.as_str()) {
            contents.push(json!({
                "Key": obj.key,
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": obj.last_modified,
                "StorageClass": "STANDARD",
            }));
        }

        key_count += 1;
    }

    let common_prefix_list: Vec<Value> = common_prefixes
        .iter()
        .map(|p| json!({ "Prefix": p }))
        .collect();

    let is_truncated = next_continuation_token.is_some();
    let actual_key_count = contents.len() + common_prefix_list.len();

    let mut result = json!({
        "Name": bucket_name,
        "Prefix": prefix,
        "MaxKeys": max_keys,
        "KeyCount": actual_key_count,
        "IsTruncated": is_truncated,
        "Contents": contents,
    });

    if !delimiter.is_empty() {
        result["Delimiter"] = json!(delimiter);
        result["CommonPrefixes"] = json!(common_prefix_list);
    }

    if let Some(token) = next_continuation_token {
        result["NextContinuationToken"] = json!(token);
    }

    if !continuation_token.is_empty() {
        result["ContinuationToken"] = json!(continuation_token);
    }

    Ok(result)
}

/// POST /{Bucket}?delete — batch delete objects.
pub fn delete_objects(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    // Parse the Delete request body.
    // XML parsed structure: {"Delete": {"Object": [{"Key": "..."}, ...]}}
    let objects = input
        .get("Delete")
        .and_then(|d| d.get("Object"))
        .or_else(|| input.get("Object"));

    let mut deleted: Vec<Value> = Vec::new();
    let errors: Vec<Value> = Vec::new();

    let process_key = |key: &str| {
        bucket.objects.remove(key);
        json!({ "Key": key })
    };

    match objects {
        Some(Value::Array(arr)) => {
            for item in arr {
                if let Some(key) = item.get("Key").and_then(Value::as_str) {
                    deleted.push(process_key(key));
                }
            }
        }
        Some(Value::Object(_)) => {
            if let Some(key) = objects.and_then(|o| o.get("Key")).and_then(Value::as_str) {
                deleted.push(process_key(key));
            }
        }
        _ => {}
    }

    Ok(json!({
        "Deleted": deleted,
        "Error": errors,
    }))
}
