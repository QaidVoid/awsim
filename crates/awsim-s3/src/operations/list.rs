use std::collections::BTreeSet;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::S3State;
use crate::util::rfc7231_to_iso8601;

use super::bucket::no_such_bucket;
use super::require_str;

/// GET /{Bucket}?list-type=2 — list objects with prefix/delimiter/pagination.
pub fn list_objects_v2(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let prefix = input.get("prefix").and_then(Value::as_str).unwrap_or("");
    let delimiter = input.get("delimiter").and_then(Value::as_str).unwrap_or("");
    let max_keys: usize = input
        .get("max-keys")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            input
                .get("max-keys")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
        })
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
    let mut last_emitted_key: Option<&str> = None;

    for key in &matching_keys {
        if key_count >= max_keys {
            next_continuation_token = last_emitted_key.map(|k| k.to_string());
            break;
        }

        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                let common_prefix = format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter);
                if common_prefixes.insert(common_prefix) {
                    key_count += 1;
                    last_emitted_key = Some(key.as_str());
                }
                continue;
            }
        }

        if let Some(versions) = bucket.objects.get(key.as_str())
            && let Some(obj) = versions.current()
        {
            contents.push(json!({
                "Key": obj.key,
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
                "StorageClass": "STANDARD",
            }));
            key_count += 1;
            last_emitted_key = Some(key.as_str());
        }
    }

    // If we exhausted all matching keys without hitting max_keys, there is no next page.
    // If we broke out early, last_emitted_key is set. If max_keys is 0 we have no emitted keys
    // and should use the continuation token as a sentinel to skip nothing extra.
    if next_continuation_token.is_none() && key_count >= max_keys && !matching_keys.is_empty() {
        next_continuation_token = last_emitted_key.map(|k| k.to_string());
    }

    let common_prefix_list: Vec<Value> = common_prefixes
        .iter()
        .map(|p| json!({ "Prefix": p }))
        .collect();

    let is_truncated = next_continuation_token.is_some();

    let mut result = json!({
        "__xml_root": "ListBucketResult",
        "Name": bucket_name,
        "Prefix": prefix,
        "MaxKeys": max_keys,
        "KeyCount": key_count,
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
        .or_else(|| {
            input
                .get("max-keys")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
        })
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
    let mut last_emitted_key: Option<&str> = None;

    for key in &matching_keys {
        if contents.len() + common_prefixes.len() >= max_keys {
            next_marker = last_emitted_key.map(|k| k.to_string());
            break;
        }
        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                let cp = format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter);
                if common_prefixes.insert(cp) {
                    last_emitted_key = Some(key.as_str());
                }
                continue;
            }
        }
        if let Some(versions) = bucket.objects.get(key.as_str())
            && let Some(obj) = versions.current()
        {
            contents.push(json!({
                "Key": obj.key,
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
                "StorageClass": "STANDARD",
            }));
            last_emitted_key = Some(key.as_str());
        }
    }

    let cp_list: Vec<Value> = common_prefixes
        .iter()
        .map(|p| json!({"Prefix": p}))
        .collect();
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
        .or_else(|| {
            input
                .get("max-keys")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
        })
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

    let mut versions_out: Vec<Value> = Vec::new();
    let mut delete_markers: Vec<Value> = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    'keys: for key in matching.iter() {
        if versions_out.len() + delete_markers.len() >= max_keys {
            break;
        }
        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                common_prefixes.insert(format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter));
                continue;
            }
        }
        let Some(versions) = bucket.objects.get(key.as_str()) else {
            continue;
        };
        // The most recent entry per key is `IsLatest=true`; everything older
        // is a historical version (or DM) and `IsLatest=false`.
        let last_idx = versions.versions.len().saturating_sub(1);
        for (i, obj) in versions.iter().enumerate().rev() {
            if versions_out.len() + delete_markers.len() >= max_keys {
                break 'keys;
            }
            let entry = json!({
                "Key": obj.key,
                "VersionId": obj.version_id.clone().unwrap_or_else(|| "null".to_string()),
                "IsLatest": i == last_idx,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
            });
            if obj.is_delete_marker {
                delete_markers.push(entry);
            } else {
                let mut v = entry;
                v["ETag"] = json!(obj.etag);
                v["Size"] = json!(obj.content_length);
                v["StorageClass"] = json!("STANDARD");
                versions_out.push(v);
            }
        }
    }

    let cp_list: Vec<Value> = common_prefixes
        .iter()
        .map(|p| json!({"Prefix": p}))
        .collect();

    let mut result = json!({
        "__xml_root": "ListVersionsResult",
        "Name": bucket_name,
        "Prefix": prefix,
        "MaxKeys": max_keys,
        "IsTruncated": false,
        "Version": versions_out,
        "DeleteMarker": delete_markers,
    });
    if !delimiter.is_empty() {
        result["Delimiter"] = json!(delimiter);
        result["CommonPrefixes"] = json!(cp_list);
    }

    Ok(result)
}

/// POST /{Bucket}?delete — batch delete objects, version-aware.
pub fn delete_objects(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    // XML parsed structure: {"Delete": {"Object": [{"Key": "...", "VersionId"?: "..."}, ...]}}
    let objects = input
        .get("Delete")
        .and_then(|d| d.get("Object"))
        .or_else(|| input.get("Object"));

    let mut entries: Vec<(String, Option<String>)> = Vec::new();
    match objects {
        Some(Value::Array(arr)) => {
            for item in arr {
                if let Some(key) = item.get("Key").and_then(Value::as_str) {
                    let vid = item
                        .get("VersionId")
                        .and_then(Value::as_str)
                        .map(String::from);
                    entries.push((key.to_string(), vid));
                }
            }
        }
        Some(Value::Object(_)) => {
            if let Some(item) = objects
                && let Some(key) = item.get("Key").and_then(Value::as_str)
            {
                let vid = item
                    .get("VersionId")
                    .and_then(Value::as_str)
                    .map(String::from);
                entries.push((key.to_string(), vid));
            }
        }
        _ => {}
    }

    let status = bucket.versioning.clone();
    drop(bucket);

    let mut deleted: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for (key, vid) in entries {
        let mut req = json!({ "Bucket": bucket_name, "Key": key.clone() });
        if let Some(v) = &vid {
            req["VersionId"] = Value::String(v.clone());
        }
        let _ = status;
        let resp = match super::object::delete_object(state, &req) {
            Ok(r) => r,
            Err(e) => {
                errors.push(json!({
                    "Key": key,
                    "Code": e.code,
                    "Message": e.message,
                }));
                continue;
            }
        };
        let mut entry = json!({ "Key": key });
        if let Some(rvid) = resp.get("VersionId").and_then(Value::as_str) {
            entry["VersionId"] = Value::String(rvid.to_string());
        }
        if let Some(true) = resp.get("DeleteMarker").and_then(Value::as_bool) {
            entry["DeleteMarker"] = Value::Bool(true);
            // Real S3 also echoes DeleteMarkerVersionId for the new marker.
            if let Some(rvid) = resp.get("VersionId").and_then(Value::as_str) {
                entry["DeleteMarkerVersionId"] = Value::String(rvid.to_string());
            }
        }
        deleted.push(entry);
    }

    Ok(json!({
        "__xml_root": "DeleteResult",
        "Deleted": deleted,
        "Error": errors,
    }))
}
