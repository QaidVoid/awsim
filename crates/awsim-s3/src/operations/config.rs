use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{NotificationConfiguration, NotificationDestination, S3State, VersioningStatus};

use super::bucket::no_such_bucket;
use super::require_str;

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

    Ok(json!({ "__xml_root": "Tagging", "TagSet": { "Tag": tags } }))
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
        let mut clean = serde_json::Map::new();
        if let Some(obj) = input.as_object() {
            for (k, v) in obj {
                if k.starts_with("__") || k == "Bucket" || k == "Key" {
                    continue;
                }
                clean.insert(k.clone(), v.clone());
            }
        }
        Value::Object(clean).to_string()
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
            // Extract just the CORSConfiguration rules, wrapping with __xml_root.
            let rules = parsed
                .get("CORSConfiguration")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            Ok(
                json!({ "__xml_root": "CORSConfiguration", "CORSRule": rules.get("CORSRule").cloned().unwrap_or(json!([])) }),
            )
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

// ─── Notification Configuration ──────────────────────────────────────────────

/// PUT /{Bucket}?notification
pub fn put_bucket_notification_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut destinations: Vec<NotificationDestination> = Vec::new();

    // Parse QueueConfigurations
    if let Some(queue_configs) = input
        .get("NotificationConfiguration")
        .and_then(|n| n.get("QueueConfiguration"))
        .or_else(|| input.get("QueueConfiguration"))
    {
        let configs = match queue_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config
                .get("Queue")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination {
                    dest_type: "sqs".to_string(),
                    arn,
                    events,
                });
            }
        }
    }

    // Parse TopicConfigurations (SNS)
    if let Some(topic_configs) = input
        .get("NotificationConfiguration")
        .and_then(|n| n.get("TopicConfiguration"))
        .or_else(|| input.get("TopicConfiguration"))
    {
        let configs = match topic_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config
                .get("Topic")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination {
                    dest_type: "sns".to_string(),
                    arn,
                    events,
                });
            }
        }
    }

    // Parse LambdaFunctionConfigurations
    if let Some(lambda_configs) = input
        .get("NotificationConfiguration")
        .and_then(|n| n.get("CloudFunctionConfiguration"))
        .or_else(|| input.get("CloudFunctionConfiguration"))
        .or_else(|| {
            input
                .get("NotificationConfiguration")
                .and_then(|n| n.get("LambdaFunctionConfiguration"))
        })
        .or_else(|| input.get("LambdaFunctionConfiguration"))
    {
        let configs = match lambda_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config
                .get("CloudFunction")
                .or_else(|| config.get("LambdaFunctionArn"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination {
                    dest_type: "lambda".to_string(),
                    arn,
                    events,
                });
            }
        }
    }

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.notification_config = NotificationConfiguration { destinations };
    Ok(json!({}))
}

/// GET /{Bucket}?notification
pub fn get_bucket_notification_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let queue_configs: Vec<Value> = bucket
        .notification_config
        .destinations
        .iter()
        .filter(|d| d.dest_type == "sqs")
        .map(|d| json!({ "Queue": d.arn, "Event": d.events }))
        .collect();

    let topic_configs: Vec<Value> = bucket
        .notification_config
        .destinations
        .iter()
        .filter(|d| d.dest_type == "sns")
        .map(|d| json!({ "Topic": d.arn, "Event": d.events }))
        .collect();

    let lambda_configs: Vec<Value> = bucket
        .notification_config
        .destinations
        .iter()
        .filter(|d| d.dest_type == "lambda")
        .map(|d| json!({ "CloudFunction": d.arn, "Event": d.events }))
        .collect();

    Ok(json!({
        "NotificationConfiguration": {
            "QueueConfiguration": queue_configs,
            "TopicConfiguration": topic_configs,
            "CloudFunctionConfiguration": lambda_configs,
        }
    }))
}

/// Parse event list from a notification config entry.
fn parse_event_list(config: &Value) -> Vec<String> {
    let event_val = config.get("Event");
    match event_val {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
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

// ─── Object Tagging ─────────────────────────────────────────────────────────

/// PUT /{Bucket}/{Key+}?tagging
pub fn put_object_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = input["Bucket"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingBucket", "Bucket is required"))?;
    let key = input["Key"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingKey", "Key is required"))?;

    let bucket = state.buckets.get(bucket_name).ok_or_else(|| {
        AwsError::not_found("NoSuchBucket", format!("Bucket '{bucket_name}' not found"))
    })?;

    let mut versions = bucket
        .objects
        .get_mut(key)
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;
    let obj = versions
        .current_mut()
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;

    let tags = parse_tags(input);
    obj.tags = tags;

    Ok(json!({}))
}

/// GET /{Bucket}/{Key+}?tagging
pub fn get_object_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = input["Bucket"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingBucket", "Bucket is required"))?;
    let key = input["Key"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingKey", "Key is required"))?;

    let bucket = state.buckets.get(bucket_name).ok_or_else(|| {
        AwsError::not_found("NoSuchBucket", format!("Bucket '{bucket_name}' not found"))
    })?;

    let versions = bucket
        .objects
        .get(key)
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;
    let obj = versions
        .current()
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;

    let tag_set: Vec<Value> = obj
        .tags
        .iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();

    Ok(json!({
        "__xml_root": "Tagging",
        "TagSet": { "Tag": tag_set }
    }))
}

/// DELETE /{Bucket}/{Key+}?tagging
pub fn delete_object_tagging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = input["Bucket"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingBucket", "Bucket is required"))?;
    let key = input["Key"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingKey", "Key is required"))?;

    let bucket = state.buckets.get(bucket_name).ok_or_else(|| {
        AwsError::not_found("NoSuchBucket", format!("Bucket '{bucket_name}' not found"))
    })?;

    let mut versions = bucket
        .objects
        .get_mut(key)
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;
    if let Some(obj) = versions.current_mut() {
        obj.tags.clear();
    }

    Ok(json!({}))
}

// ─── ACL ──────────────────────────────────────────────────────────────────────

/// GET /{Bucket}?acl — Return default owner-full-control ACL for a bucket.
pub fn get_bucket_acl(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    if let Some(acl_str) = &bucket.acl {
        let parsed: Value =
            serde_json::from_str(acl_str).unwrap_or(default_bucket_acl(&ctx.account_id));
        return Ok(parsed);
    }

    Ok(default_bucket_acl(&ctx.account_id))
}

/// PUT /{Bucket}?acl — Store ACL for a bucket (accept and store).
pub fn put_bucket_acl(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.acl = Some(input.to_string());
    Ok(json!({}))
}

/// GET /{Bucket}/{Key+}?acl — Return default ACL for an object.
pub fn get_object_acl(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = input["Bucket"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingBucket", "Bucket is required"))?;
    let key = input["Key"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingKey", "Key is required"))?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    let obj_exists = versions.is_some_and(|v| v.current().is_some());
    if !obj_exists {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    Ok(default_bucket_acl(&ctx.account_id))
}

fn default_bucket_acl(owner_id: &str) -> Value {
    json!({
        "AccessControlPolicy": {
            "Owner": {
                "ID": owner_id,
                "DisplayName": owner_id
            },
            "AccessControlList": {
                "Grant": [{
                    "Grantee": {
                        "ID": owner_id,
                        "DisplayName": owner_id,
                        "xsi:type": "CanonicalUser"
                    },
                    "Permission": "FULL_CONTROL"
                }]
            }
        }
    })
}

// ─── Lifecycle Configuration ─────────────────────────────────────────────────

/// GET /{Bucket}?lifecycle — Return stored lifecycle configuration.
pub fn get_bucket_lifecycle_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match &bucket.lifecycle {
        Some(lc) => {
            let parsed: Value = serde_json::from_str(lc).unwrap_or(json!({}));
            // Extract lifecycle rules from stored input - they may be nested under LifecycleConfiguration
            let lc_val = parsed
                .get("LifecycleConfiguration")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let rules = lc_val.get("Rule").cloned().unwrap_or(json!([]));
            Ok(json!({ "__xml_root": "LifecycleConfiguration", "Rule": rules }))
        }
        None => Err(AwsError::not_found(
            "NoSuchLifecycleConfiguration",
            format!("The lifecycle configuration does not exist for bucket '{bucket_name}'"),
        )),
    }
}

/// PUT /{Bucket}?lifecycle — Store lifecycle configuration.
pub fn put_bucket_lifecycle_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.lifecycle = Some(input.to_string());
    Ok(json!({}))
}

/// DELETE /{Bucket}?lifecycle — Remove lifecycle configuration.
pub fn delete_bucket_lifecycle_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.lifecycle = None;
    Ok(json!({}))
}

// ─── Encryption ──────────────────────────────────────────────────────────────

/// GET /{Bucket}?encryption — Return stored encryption configuration or default SSE-S3.
pub fn get_bucket_encryption(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match &bucket.encryption {
        Some(enc) => {
            let parsed: Value = serde_json::from_str(enc).unwrap_or(default_sse_s3_config());
            // Extract just the rules from ServerSideEncryptionConfiguration
            let sse_config = parsed
                .get("ServerSideEncryptionConfiguration")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let rules = sse_config
                .get("Rule")
                .or_else(|| sse_config.get("Rules"))
                .cloned()
                .unwrap_or(json!([]));
            Ok(json!({ "__xml_root": "ServerSideEncryptionConfiguration", "Rule": rules }))
        }
        None => Err(AwsError::not_found(
            "ServerSideEncryptionConfigurationNotFoundError",
            format!(
                "The server side encryption configuration was not found for bucket '{bucket_name}'"
            ),
        )),
    }
}

/// PUT /{Bucket}?encryption — Store encryption configuration.
pub fn put_bucket_encryption(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.encryption = Some(input.to_string());
    Ok(json!({}))
}

/// DELETE /{Bucket}?encryption — Remove encryption configuration.
pub fn delete_bucket_encryption(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.encryption = None;
    Ok(json!({}))
}

fn default_sse_s3_config() -> Value {
    json!({
        "ServerSideEncryptionConfiguration": {
            "Rules": [{
                "ApplyServerSideEncryptionByDefault": {
                    "SSEAlgorithm": "AES256"
                },
                "BucketKeyEnabled": false
            }]
        }
    })
}

// ─── Generic config helpers ──────────────────────────────────────────────────

/// GET /{Bucket}?<param> — Retrieve a stored JSON config from bucket.configs.
/// Returns `not_found_code` error if not set.
/// `xml_root` is the expected XML root element name (used to wrap the response).
/// `config_key` is the JSON key under which the config data was stored.
pub fn get_bucket_config_xml(
    state: &S3State,
    input: &Value,
    config_name: &str,
    not_found_code: &str,
    xml_root: Option<&str>,
    config_key: Option<&str>,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match bucket.configs.get(config_name) {
        Some(raw) => {
            let parsed: Value =
                serde_json::from_str(raw).unwrap_or(serde_json::Value::Object(Default::default()));
            if let (Some(root), Some(key)) = (xml_root, config_key) {
                // Extract sub-config and wrap with xml_root
                let config_data = parsed.get(key).cloned().unwrap_or_else(|| parsed.clone());
                let mut result = serde_json::Map::new();
                result.insert("__xml_root".to_string(), Value::String(root.to_string()));
                if let Some(obj) = config_data.as_object() {
                    for (k, v) in obj {
                        result.insert(k.clone(), v.clone());
                    }
                }
                Ok(Value::Object(result))
            } else {
                Ok(parsed)
            }
        }
        None => Err(AwsError::not_found(
            not_found_code,
            format!(
                "The {} configuration does not exist for bucket '{}'",
                config_name, bucket_name
            ),
        )),
    }
}

/// PUT /{Bucket}?<param> — Store a JSON config on bucket.configs.
/// Extracts `config_key` subfield if present, otherwise stores the input JSON (excluding path params).
pub fn put_bucket_config_key(
    state: &S3State,
    input: &Value,
    config_name: &str,
    config_key: Option<&str>,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let to_store = if let Some(key) = config_key {
        input.get(key).cloned().unwrap_or_else(|| input.clone())
    } else {
        input.clone()
    };

    bucket
        .configs
        .insert(config_name.to_string(), to_store.to_string());
    Ok(json!({}))
}

/// DELETE /{Bucket}?<param> — Remove a stored config from bucket.configs.
pub fn delete_bucket_config(
    state: &S3State,
    input: &Value,
    config_name: &str,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.remove(config_name);
    Ok(json!({}))
}

// ─── Website ─────────────────────────────────────────────────────────────────

pub fn get_bucket_website(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    get_bucket_config_xml(
        state,
        input,
        "website",
        "NoSuchWebsiteConfiguration",
        Some("WebsiteConfiguration"),
        Some("WebsiteConfiguration"),
    )
}

pub fn put_bucket_website(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    put_bucket_config_key(state, input, "website", Some("WebsiteConfiguration"))
}

pub fn delete_bucket_website(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    delete_bucket_config(state, input, "website")
}

// ─── Replication ─────────────────────────────────────────────────────────────

pub fn get_bucket_replication(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    get_bucket_config_xml(
        state,
        input,
        "replication",
        "ReplicationConfigurationNotFoundError",
        Some("ReplicationConfiguration"),
        Some("ReplicationConfiguration"),
    )
}

pub fn put_bucket_replication(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    put_bucket_config_key(
        state,
        input,
        "replication",
        Some("ReplicationConfiguration"),
    )
}

pub fn delete_bucket_replication(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    delete_bucket_config(state, input, "replication")
}

// ─── Request Payment ─────────────────────────────────────────────────────────

pub fn get_bucket_request_payment(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let payer = match bucket.configs.get("requestpayment") {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            parsed
                .get("Payer")
                .and_then(Value::as_str)
                .unwrap_or("BucketOwner")
                .to_string()
        }
        None => "BucketOwner".to_string(),
    };

    Ok(json!({ "RequestPaymentConfiguration": { "Payer": payer } }))
}

pub fn put_bucket_request_payment(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let payer = input
        .get("RequestPaymentConfiguration")
        .and_then(|v| v.get("Payer"))
        .and_then(Value::as_str)
        .or_else(|| input.get("Payer").and_then(Value::as_str))
        .unwrap_or("BucketOwner");

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.insert(
        "requestpayment".to_string(),
        json!({ "Payer": payer }).to_string(),
    );
    Ok(json!({}))
}

// ─── Accelerate Configuration ─────────────────────────────────────────────────

pub fn get_bucket_accelerate_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let status = match bucket.configs.get("accelerate") {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            parsed
                .get("Status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }
        None => String::new(),
    };

    if status.is_empty() {
        Ok(json!({ "AccelerateConfiguration": {} }))
    } else {
        Ok(json!({ "AccelerateConfiguration": { "Status": status } }))
    }
}

pub fn put_bucket_accelerate_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let status = input
        .get("AccelerateConfiguration")
        .and_then(|v| v.get("Status"))
        .and_then(Value::as_str)
        .or_else(|| input.get("Status").and_then(Value::as_str))
        .unwrap_or("");

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.insert(
        "accelerate".to_string(),
        json!({ "Status": status }).to_string(),
    );
    Ok(json!({}))
}

// ─── Analytics Configurations (keyed by Id) ───────────────────────────────────

pub fn get_bucket_analytics_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    // If no Id is present, this is actually a ListBucketAnalyticsConfigurations request.
    if input.get("Id").and_then(Value::as_str).is_none() {
        return list_bucket_analytics_configurations(state, input);
    }
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let key = format!("analytics:{}", id);
    match bucket.configs.get(&key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            Ok(json!({ "AnalyticsConfiguration": parsed }))
        }
        None => Err(AwsError::not_found(
            "NoSuchConfiguration",
            format!(
                "The analytics configuration with ID '{}' does not exist",
                id
            ),
        )),
    }
}

pub fn put_bucket_analytics_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let config = input.get("AnalyticsConfiguration").unwrap_or(input);

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket
        .configs
        .insert(format!("analytics:{}", id), config.to_string());
    Ok(json!({}))
}

pub fn delete_bucket_analytics_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.remove(&format!("analytics:{}", id));
    Ok(json!({}))
}

pub fn list_bucket_analytics_configurations(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let configs: Vec<Value> = bucket
        .configs
        .iter()
        .filter(|(k, _)| k.starts_with("analytics:"))
        .map(|(_, v)| serde_json::from_str(v).unwrap_or(json!({})))
        .collect();

    Ok(json!({ "AnalyticsConfigurationList": configs, "IsTruncated": false }))
}

// ─── Metrics Configurations (keyed by Id) ────────────────────────────────────

pub fn get_bucket_metrics_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    if input.get("Id").and_then(Value::as_str).is_none() {
        return list_bucket_metrics_configurations(state, input);
    }
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let key = format!("metrics:{}", id);
    match bucket.configs.get(&key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            Ok(json!({ "MetricsConfiguration": parsed }))
        }
        None => Err(AwsError::not_found(
            "NoSuchConfiguration",
            format!("The metrics configuration with ID '{}' does not exist", id),
        )),
    }
}

pub fn put_bucket_metrics_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let config = input.get("MetricsConfiguration").unwrap_or(input);

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket
        .configs
        .insert(format!("metrics:{}", id), config.to_string());
    Ok(json!({}))
}

pub fn delete_bucket_metrics_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.remove(&format!("metrics:{}", id));
    Ok(json!({}))
}

pub fn list_bucket_metrics_configurations(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let configs: Vec<Value> = bucket
        .configs
        .iter()
        .filter(|(k, _)| k.starts_with("metrics:"))
        .map(|(_, v)| serde_json::from_str(v).unwrap_or(json!({})))
        .collect();

    Ok(json!({ "MetricsConfigurationList": configs, "IsTruncated": false }))
}

// ─── Intelligent Tiering Configurations (keyed by Id) ────────────────────────

pub fn get_bucket_intelligent_tiering_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    if input.get("Id").and_then(Value::as_str).is_none() {
        return list_bucket_intelligent_tiering_configurations(state, input);
    }
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let key = format!("intelligent-tiering:{}", id);
    match bucket.configs.get(&key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            Ok(json!({ "IntelligentTieringConfiguration": parsed }))
        }
        None => Err(AwsError::not_found(
            "NoSuchConfiguration",
            format!(
                "The intelligent tiering configuration with ID '{}' does not exist",
                id
            ),
        )),
    }
}

pub fn put_bucket_intelligent_tiering_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let config = input
        .get("IntelligentTieringConfiguration")
        .unwrap_or(input);

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket
        .configs
        .insert(format!("intelligent-tiering:{}", id), config.to_string());
    Ok(json!({}))
}

pub fn delete_bucket_intelligent_tiering_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket
        .configs
        .remove(&format!("intelligent-tiering:{}", id));
    Ok(json!({}))
}

pub fn list_bucket_intelligent_tiering_configurations(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let configs: Vec<Value> = bucket
        .configs
        .iter()
        .filter(|(k, _)| k.starts_with("intelligent-tiering:"))
        .map(|(_, v)| serde_json::from_str(v).unwrap_or(json!({})))
        .collect();

    Ok(json!({ "IntelligentTieringConfigurationList": configs, "IsTruncated": false }))
}

// ─── Inventory Configurations (keyed by Id) ───────────────────────────────────

pub fn get_bucket_inventory_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    if input.get("Id").and_then(Value::as_str).is_none() {
        return list_bucket_inventory_configurations(state, input);
    }
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let key = format!("inventory:{}", id);
    match bucket.configs.get(&key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            Ok(json!({ "InventoryConfiguration": parsed }))
        }
        None => Err(AwsError::not_found(
            "NoSuchConfiguration",
            format!(
                "The inventory configuration with ID '{}' does not exist",
                id
            ),
        )),
    }
}

pub fn put_bucket_inventory_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let config = input.get("InventoryConfiguration").unwrap_or(input);

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket
        .configs
        .insert(format!("inventory:{}", id), config.to_string());
    Ok(json!({}))
}

pub fn delete_bucket_inventory_configuration(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let id = require_str(input, "Id")?;

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.configs.remove(&format!("inventory:{}", id));
    Ok(json!({}))
}

pub fn list_bucket_inventory_configurations(
    state: &S3State,
    input: &Value,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let configs: Vec<Value> = bucket
        .configs
        .iter()
        .filter(|(k, _)| k.starts_with("inventory:"))
        .map(|(_, v)| serde_json::from_str(v).unwrap_or(json!({})))
        .collect();

    Ok(json!({ "InventoryConfigurationList": configs, "IsTruncated": false }))
}

// ─── Ownership Controls ───────────────────────────────────────────────────────

pub fn get_bucket_ownership_controls(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    get_bucket_config_xml(
        state,
        input,
        "ownership-controls",
        "OwnershipControlsNotFoundError",
        Some("OwnershipControls"),
        Some("OwnershipControls"),
    )
}

pub fn put_bucket_ownership_controls(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    put_bucket_config_key(
        state,
        input,
        "ownership-controls",
        Some("OwnershipControls"),
    )
}

pub fn delete_bucket_ownership_controls(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    delete_bucket_config(state, input, "ownership-controls")
}

// ─── Public Access Block ──────────────────────────────────────────────────────

pub fn get_public_access_block(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match bucket.configs.get("public-access-block") {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            // Extract the PublicAccessBlockConfiguration fields
            let config = parsed
                .get("PublicAccessBlockConfiguration")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let mut result = serde_json::Map::new();
            result.insert(
                "__xml_root".to_string(),
                Value::String("PublicAccessBlockConfiguration".to_string()),
            );
            if let Some(obj) = config.as_object() {
                for (k, v) in obj {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(result))
        }
        None => Err(AwsError::not_found(
            "NoSuchPublicAccessBlockConfiguration",
            format!(
                "The public access block configuration was not found for bucket '{bucket_name}'"
            ),
        )),
    }
}

pub fn put_public_access_block(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    put_bucket_config_key(
        state,
        input,
        "public-access-block",
        Some("PublicAccessBlockConfiguration"),
    )
}

pub fn delete_public_access_block(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    delete_bucket_config(state, input, "public-access-block")
}

// ─── SelectObjectContent (stub) ───────────────────────────────────────────────

pub fn select_object_content(_state: &S3State, _input: &Value) -> Result<Value, AwsError> {
    // Stub — returns empty payload. Real implementation requires streaming.
    Ok(json!({ "Payload": [] }))
}

// ─── Bucket Policy Status ────────────────────────────────────────────────────

pub fn get_bucket_policy_status(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let is_public = bucket
        .policy
        .as_deref()
        .map(|p| p.contains("\"Principal\":\"*\"") || p.contains("\"AWS\":\"*\""))
        .unwrap_or(false);

    Ok(json!({
        "__xml_root": "PolicyStatus",
        "IsPublic": is_public,
    }))
}

// ─── Object Lock Configuration ───────────────────────────────────────────────

pub fn get_object_lock_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match bucket.configs.get("object-lock") {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            let cfg = parsed
                .get("ObjectLockConfiguration")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let mut result = serde_json::Map::new();
            result.insert(
                "__xml_root".to_string(),
                Value::String("ObjectLockConfiguration".to_string()),
            );
            if let Some(obj) = cfg.as_object() {
                for (k, v) in obj {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(result))
        }
        None => Err(AwsError::not_found(
            "ObjectLockConfigurationNotFoundError",
            format!("Object Lock configuration does not exist for bucket '{bucket_name}'"),
        )),
    }
}

pub fn put_object_lock_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    put_bucket_config_key(state, input, "object-lock", Some("ObjectLockConfiguration"))
}

// ─── Object Legal Hold ───────────────────────────────────────────────────────

pub fn get_object_legal_hold(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    let obj_exists = versions.is_some_and(|v| v.current().is_some());
    if !obj_exists {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    let cfg_key = format!("legal-hold:{}", key);
    let status = match bucket.configs.get(&cfg_key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            parsed
                .get("Status")
                .and_then(Value::as_str)
                .unwrap_or("OFF")
                .to_string()
        }
        None => "OFF".to_string(),
    };

    Ok(json!({
        "__xml_root": "LegalHold",
        "Status": status,
    }))
}

pub fn put_object_legal_hold(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let status = input
        .get("LegalHold")
        .and_then(|v| v.get("Status"))
        .and_then(Value::as_str)
        .or_else(|| input.get("Status").and_then(Value::as_str))
        .unwrap_or("OFF");

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    if !versions.is_some_and(|v| v.current().is_some()) {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    bucket.configs.insert(
        format!("legal-hold:{}", key),
        json!({"Status": status}).to_string(),
    );
    Ok(json!({}))
}

// ─── Object Retention ────────────────────────────────────────────────────────

pub fn get_object_retention(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    if !versions.is_some_and(|v| v.current().is_some()) {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    let cfg_key = format!("retention:{}", key);
    match bucket.configs.get(&cfg_key) {
        Some(raw) => {
            let parsed: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            let mode = parsed
                .get("Mode")
                .and_then(Value::as_str)
                .unwrap_or("GOVERNANCE")
                .to_string();
            let until = parsed
                .get("RetainUntilDate")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Ok(json!({
                "__xml_root": "Retention",
                "Mode": mode,
                "RetainUntilDate": until,
            }))
        }
        None => Err(AwsError::not_found(
            "NoSuchObjectLockConfiguration",
            format!("No retention for key '{key}'"),
        )),
    }
}

pub fn put_object_retention(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let retention = input
        .get("Retention")
        .cloned()
        .unwrap_or_else(|| input.clone());
    let mode = retention
        .get("Mode")
        .and_then(Value::as_str)
        .unwrap_or("GOVERNANCE");
    let until = retention
        .get("RetainUntilDate")
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    if !versions.is_some_and(|v| v.current().is_some()) {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    bucket.configs.insert(
        format!("retention:{}", key),
        json!({"Mode": mode, "RetainUntilDate": until}).to_string(),
    );
    Ok(json!({}))
}

// ─── Put Object ACL ──────────────────────────────────────────────────────────

pub fn put_object_acl(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    if !versions.is_some_and(|v| v.current().is_some()) {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    Ok(json!({}))
}

// ─── Get Object Attributes ───────────────────────────────────────────────────

pub fn get_object_attributes(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket
        .objects
        .get(key)
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;
    let obj = versions
        .current()
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{key}' not found")))?;

    Ok(json!({
        "__xml_root": "GetObjectAttributesOutput",
        "ETag": obj.etag.trim_matches('"'),
        "ObjectSize": obj.content_length,
        "StorageClass": "STANDARD",
    }))
}

// ─── Restore Object ──────────────────────────────────────────────────────────

pub fn restore_object(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let versions = bucket.objects.get(key);
    if !versions.is_some_and(|v| v.current().is_some()) {
        return Err(AwsError::not_found(
            "NoSuchKey",
            format!("Key '{key}' not found"),
        ));
    }

    Ok(json!({}))
}

// ─── Rename Object ───────────────────────────────────────────────────────────

pub fn rename_object(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let dst_key = require_str(input, "Key")?;
    let rename_source = require_str(input, "RenameSource")?;

    let src_key = rename_source.trim_start_matches('/');
    let src_key = src_key.split_once('/').map(|(_, k)| k).unwrap_or(src_key);

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    let (_, mut versions) = bucket
        .objects
        .remove(src_key)
        .ok_or_else(|| AwsError::not_found("NoSuchKey", format!("Key '{src_key}' not found")))?;

    for v in &mut versions.versions {
        v.key = dst_key.to_string();
    }
    bucket.objects.insert(dst_key.to_string(), versions);

    Ok(json!({}))
}

// ─── Create Session ──────────────────────────────────────────────────────────

pub fn create_session(_state: &S3State, _input: &Value) -> Result<Value, AwsError> {
    use crate::util::now_iso8601;
    Ok(json!({
        "__xml_root": "CreateSessionOutput",
        "Credentials": {
            "AccessKeyId": "ASIAAWSIMSESSION",
            "SecretAccessKey": "secretkey",
            "SessionToken": "sessiontoken",
            "Expiration": now_iso8601(),
        }
    }))
}

// ─── Logging ─────────────────────────────────────────────────────────────────

/// GET /{Bucket}?logging — Return empty logging configuration.
pub fn get_bucket_logging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let bucket = state
        .buckets
        .get(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    match &bucket.logging {
        Some(log) => {
            let parsed: Value = serde_json::from_str(log).unwrap_or(json!({}));
            // Extract BucketLoggingStatus fields and wrap with __xml_root
            let status = parsed
                .get("BucketLoggingStatus")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let mut result = serde_json::Map::new();
            result.insert(
                "__xml_root".to_string(),
                Value::String("BucketLoggingStatus".to_string()),
            );
            if let Some(obj) = status.as_object() {
                for (k, v) in obj {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(result))
        }
        None => Ok(json!({ "__xml_root": "BucketLoggingStatus" })),
    }
}

/// PUT /{Bucket}?logging — Store logging configuration.
pub fn put_bucket_logging(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let to_store = input
        .get("BucketLoggingStatus")
        .cloned()
        .unwrap_or_else(|| input.clone());

    let mut bucket = state
        .buckets
        .get_mut(bucket_name)
        .ok_or_else(|| no_such_bucket(bucket_name))?;

    bucket.logging = Some(json!({ "BucketLoggingStatus": to_store }).to_string());
    Ok(json!({}))
}
