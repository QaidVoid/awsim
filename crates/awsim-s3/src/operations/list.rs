use std::collections::BTreeSet;

use awsim_core::pagination::{decode_token, encode_token};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::S3State;
use crate::util::rfc7231_to_iso8601;

use super::bucket::no_such_bucket;
use super::require_str;

/// Percent-encode a key for `EncodingType=url` responses. AWS encodes
/// every byte that isn't unreserved-per-RFC-3986 (alphanumeric and
/// `-_.~`). The forward slash is encoded too — keys treated as paths
/// don't change semantics for SDK clients that decode the value back
/// before use.
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Conditional encoder driven by the request's EncodingType.
fn encode_if_url(s: &str, encoding: Option<&str>) -> String {
    if encoding == Some("url") {
        pct_encode(s)
    } else {
        s.to_string()
    }
}

fn owner_entry(account_id: &str) -> Value {
    json!({
        "ID": account_id,
        "DisplayName": account_id,
    })
}

/// GET /{Bucket}?list-type=2 — list objects with prefix/delimiter/pagination.
pub fn list_objects_v2(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
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
    let continuation_token_in = input
        .get("continuation-token")
        .and_then(Value::as_str)
        .unwrap_or("");
    let start_after = input
        .get("start-after")
        .and_then(Value::as_str)
        .unwrap_or("");
    let encoding_type = input.get("encoding-type").and_then(Value::as_str);

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

    // Decode the continuation token (opaque base64) back to its marker key.
    // ContinuationToken takes precedence over StartAfter when both are
    // supplied; if neither, no skip.
    let resume_marker: Option<String> = if !continuation_token_in.is_empty() {
        Some(decode_token(continuation_token_in)?)
    } else if !start_after.is_empty() {
        Some(start_after.to_string())
    } else {
        None
    };
    // ContinuationToken's marker is the first NOT-yet-returned key, so we
    // include it (>=). StartAfter's marker is the last seen key, so we
    // exclude it (>); but since StartAfter is only used when there's no
    // continuation token, gate by which input was decoded.
    let matching_keys: Vec<String> = match resume_marker {
        None => matching_keys,
        Some(marker) if !continuation_token_in.is_empty() => matching_keys
            .into_iter()
            .filter(|k| k.as_str() >= marker.as_str())
            .collect(),
        Some(marker) => matching_keys
            .into_iter()
            .filter(|k| k.as_str() > marker.as_str())
            .collect(),
    };

    let mut contents: Vec<Value> = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    let mut key_count = 0usize;
    // The marker for the next page: the first key we did NOT emit on this
    // page, so resuming with this token returns the next slice with no
    // overlap or gap. Stored raw; encoded as base64 only when emitted.
    let mut next_unemitted_key: Option<&str> = None;

    for key in &matching_keys {
        if key_count >= max_keys {
            next_unemitted_key = Some(key.as_str());
            break;
        }

        if !delimiter.is_empty() {
            let suffix = &key[prefix.len()..];
            if let Some(delim_pos) = suffix.find(delimiter) {
                let common_prefix = format!("{}{}{}", prefix, &suffix[..delim_pos], delimiter);
                if common_prefixes.insert(common_prefix) {
                    key_count += 1;
                }
                continue;
            }
        }

        if let Some(versions) = bucket.objects.get(key.as_str())
            && let Some(obj) = versions.current()
        {
            contents.push(json!({
                "Key": encode_if_url(&obj.key, encoding_type),
                "ETag": obj.etag,
                "Size": obj.content_length,
                "LastModified": rfc7231_to_iso8601(&obj.last_modified),
                "StorageClass": "STANDARD",
                "Owner": owner_entry(&ctx.account_id),
            }));
            key_count += 1;
        }
    }

    let common_prefix_list: Vec<Value> = common_prefixes
        .iter()
        .map(|p| json!({ "Prefix": encode_if_url(p, encoding_type) }))
        .collect();

    let is_truncated = next_unemitted_key.is_some();

    let mut result = json!({
        "__xml_root": "ListBucketResult",
        "Name": bucket_name,
        "Prefix": encode_if_url(prefix, encoding_type),
        "MaxKeys": max_keys,
        "KeyCount": key_count,
        "IsTruncated": is_truncated,
        "Contents": contents,
    });

    if !delimiter.is_empty() {
        result["Delimiter"] = json!(encode_if_url(delimiter, encoding_type));
        result["CommonPrefixes"] = json!(common_prefix_list);
    }

    if let Some(marker) = next_unemitted_key {
        result["NextContinuationToken"] = json!(encode_token(marker));
    }

    if !continuation_token_in.is_empty() {
        result["ContinuationToken"] = json!(continuation_token_in);
    }

    if let Some(et) = encoding_type {
        result["EncodingType"] = json!(et);
    }

    Ok(result)
}

/// GET /{Bucket} — list objects (v1).
pub fn list_objects(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
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
                "Owner": owner_entry(&ctx.account_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::object::record_version;
    use crate::state::VersioningStatus;
    use crate::state::{Bucket, S3Object, S3State};

    fn ctx() -> RequestContext {
        RequestContext::new("s3", "us-east-1")
    }

    fn state_with_keys(keys: &[&str]) -> S3State {
        let bucket = Bucket::new("b", "us-east-1", "now");
        let state = S3State::default();
        state.buckets.insert(bucket.name.clone(), bucket);
        {
            let bucket_ref = state.buckets.get_mut("b").unwrap();
            for k in keys {
                let obj = S3Object {
                    key: (*k).to_string(),
                    etag: "\"x\"".to_string(),
                    last_modified: "Mon, 01 Jan 2024 00:00:00 GMT".to_string(),
                    content_length: 0,
                    content_type: "application/octet-stream".to_string(),
                    metadata: Default::default(),
                    version_id: None,
                    tags: Default::default(),
                    is_delete_marker: false,
                    content_encoding: None,
                    cache_control: None,
                    content_disposition: None,
                    content_language: None,
                    expires: None,
                    body: awsim_core::Body::from_bytes(Vec::new()),
                };
                let mut versions = bucket_ref.objects.entry((*k).to_string()).or_default();
                record_version(&mut versions, obj, &VersioningStatus::Disabled);
            }
        }
        state
    }

    #[test]
    fn next_continuation_token_is_base64_of_first_unemitted_key() {
        let state = state_with_keys(&["alpha", "bravo", "charlie", "delta"]);
        let resp =
            list_objects_v2(&state, &json!({ "Bucket": "b", "max-keys": 2u64 }), &ctx()).unwrap();

        assert_eq!(resp["IsTruncated"], json!(true));
        assert_eq!(resp["KeyCount"], json!(2));
        let token = resp["NextContinuationToken"].as_str().unwrap();
        // Token must be opaque (base64) — not a raw key.
        assert_ne!(token, "charlie", "token must not be raw key");
        assert_eq!(decode_token(token).unwrap(), "charlie");
    }

    #[test]
    fn resuming_with_token_returns_next_slice_with_no_overlap() {
        let state = state_with_keys(&["alpha", "bravo", "charlie", "delta"]);
        let page1 =
            list_objects_v2(&state, &json!({ "Bucket": "b", "max-keys": 2u64 }), &ctx()).unwrap();
        let token = page1["NextContinuationToken"].as_str().unwrap().to_string();

        let page2 = list_objects_v2(
            &state,
            &json!({
                "Bucket": "b",
                "max-keys": 10u64,
                "continuation-token": token,
            }),
            &ctx(),
        )
        .unwrap();

        let returned: Vec<&str> = page2["Contents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Key"].as_str().unwrap())
            .collect();
        assert_eq!(returned, vec!["charlie", "delta"]);
        assert_eq!(page2["IsTruncated"], json!(false));
        assert!(page2.get("NextContinuationToken").is_none());
    }

    #[test]
    fn no_token_when_exactly_max_keys_match() {
        // Regression: previously emitted a spurious continuation token when
        // total matching keys equaled max_keys exactly.
        let state = state_with_keys(&["alpha", "bravo", "charlie"]);
        let resp =
            list_objects_v2(&state, &json!({ "Bucket": "b", "max-keys": 3u64 }), &ctx()).unwrap();
        assert_eq!(resp["IsTruncated"], json!(false));
        assert!(resp.get("NextContinuationToken").is_none());
    }

    #[test]
    fn list_v2_url_encodes_keys_when_encoding_type_url() {
        let state = state_with_keys(&["folder/file with spaces & symbols.txt", "alpha"]);
        let resp = list_objects_v2(
            &state,
            &json!({ "Bucket": "b", "encoding-type": "url" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["EncodingType"], json!("url"));
        let keys: Vec<String> = resp["Contents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["Key"].as_str().unwrap().to_string())
            .collect();
        // alpha sorts first; "folder/..." second.
        assert_eq!(keys[0], "alpha");
        assert!(keys[1].contains("%20"), "space encoded: {}", keys[1]);
        assert!(keys[1].contains("%26"), "ampersand encoded: {}", keys[1]);
        assert!(keys[1].contains("%2F"), "slash encoded: {}", keys[1]);
    }

    #[test]
    fn list_v2_no_encoding_returns_keys_verbatim() {
        let state = state_with_keys(&["a/b c"]);
        let resp = list_objects_v2(&state, &json!({ "Bucket": "b" }), &ctx()).unwrap();
        assert!(resp.get("EncodingType").is_none());
        assert_eq!(resp["Contents"][0]["Key"].as_str(), Some("a/b c"));
    }

    #[test]
    fn invalid_continuation_token_returns_error() {
        let state = state_with_keys(&["alpha"]);
        let err = list_objects_v2(
            &state,
            &json!({
                "Bucket": "b",
                "continuation-token": "!!!not-valid-base64!!!",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }
}
