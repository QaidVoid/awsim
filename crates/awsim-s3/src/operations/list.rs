use std::collections::BTreeSet;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::S3State;
use crate::util::rfc7231_to_iso8601;

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
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
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
        "__xml_root": "ListBucketResult",
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

/// GET /{Bucket} — list objects (v1).
pub fn list_objects(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let prefix = input.get("prefix").and_then(Value::as_str).unwrap_or("");
    let delimiter = input.get("delimiter").and_then(Value::as_str).unwrap_or("");
    let marker = input.get("marker").and_then(Value::as_str).unwrap_or("");
    let max_keys: usize = input
        .get("max-keys")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| input.get("max-keys").and_then(Value::as_u64).map(|n| n as usize))
        .unwrap_or(1000)
        .min(1000);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let mut matching_keys: Vec<String> = bucket
        .objects
        .iter()
        .map(|e| e.key().clone())
        .filter(|k| k.starts_with(prefix) && k.as_str() > marker)
        .collect();
    matching_keys.sort();

    let mut contents: Vec<Value> = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    let mut next_marker: Option<String> = None;

    for key in &matching_keys {
        if contents.len() + common_prefixes.len() >= max_keys {
            next_marker = Some(key.clone());
            break;
        }
        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                let cp = format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter);
                common_prefixes.insert(cp);
                continue;
            }
        }
        if let Some(obj) = bucket.objects.get(key.as_str()) {
            contents.push(json!({
                "Key": obj.key,
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
                "StorageClass": "STANDARD",
            }));
        }
    }

    let cp_list: Vec<Value> = common_prefixes.iter().map(|p| json!({"Prefix": p})).collect();
    let is_truncated = next_marker.is_some();

    let mut result = json!({
        "__xml_root": "ListBucketResult",
        "Name": bucket_name,
        "Prefix": prefix,
        "Marker": marker,
        "MaxKeys": max_keys,
        "IsTruncated": is_truncated,
        "Contents": contents,
    });

    if !delimiter.is_empty() {
        result["Delimiter"] = json!(delimiter);
        result["CommonPrefixes"] = json!(cp_list);
    }
    if let Some(nm) = next_marker {
        result["NextMarker"] = json!(nm);
    }

    Ok(result)
}

/// GET /{Bucket}?versions — list object versions.
pub fn list_object_versions(state: &S3State, input: &Value) -> Result<Value, AwsError> {
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

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let mut matching: Vec<String> = bucket
        .objects
        .iter()
        .map(|e| e.key().clone())
        .filter(|k| k.starts_with(prefix))
        .collect();
    matching.sort();

    let mut versions: Vec<Value> = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    for key in matching.iter().take(max_keys) {
        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                common_prefixes.insert(format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter));
                continue;
            }
        }
        if let Some(obj) = bucket.objects.get(key.as_str()) {
            versions.push(json!({
                "Key": obj.key,
                "VersionId": obj.version_id.clone().unwrap_or_else(|| "null".to_string()),
                "IsLatest": true,
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
                "StorageClass": "STANDARD",
            }));
        }
    }

    let cp_list: Vec<Value> = common_prefixes.iter().map(|p| json!({"Prefix": p})).collect();

    let mut result = json!({
        "__xml_root": "ListVersionsResult",
        "Name": bucket_name,
        "Prefix": prefix,
        "MaxKeys": max_keys,
        "IsTruncated": false,
        "Version": versions,
    });
    if !delimiter.is_empty() {
        result["Delimiter"] = json!(delimiter);
        result["CommonPrefixes"] = json!(cp_list);
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
        "__xml_root": "DeleteResult",
        "Deleted": deleted,
        "Error": errors,
    }))
}
