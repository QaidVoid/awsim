use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::{NotificationConfiguration, NotificationDestination, S3State, VersioningStatus};

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

// ─── Notification Configuration ──────────────────────────────────────────────

/// PUT /{Bucket}?notification
pub fn put_bucket_notification_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;

    let mut destinations: Vec<NotificationDestination> = Vec::new();

    // Parse QueueConfigurations
    if let Some(queue_configs) = input.get("NotificationConfiguration")
        .and_then(|n| n.get("QueueConfiguration"))
        .or_else(|| input.get("QueueConfiguration"))
    {
        let configs = match queue_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config.get("Queue").and_then(Value::as_str).unwrap_or("").to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination { dest_type: "sqs".to_string(), arn, events });
            }
        }
    }

    // Parse TopicConfigurations (SNS)
    if let Some(topic_configs) = input.get("NotificationConfiguration")
        .and_then(|n| n.get("TopicConfiguration"))
        .or_else(|| input.get("TopicConfiguration"))
    {
        let configs = match topic_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config.get("Topic").and_then(Value::as_str).unwrap_or("").to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination { dest_type: "sns".to_string(), arn, events });
            }
        }
    }

    // Parse LambdaFunctionConfigurations
    if let Some(lambda_configs) = input.get("NotificationConfiguration")
        .and_then(|n| n.get("CloudFunctionConfiguration"))
        .or_else(|| input.get("CloudFunctionConfiguration"))
        .or_else(|| input.get("NotificationConfiguration").and_then(|n| n.get("LambdaFunctionConfiguration")))
        .or_else(|| input.get("LambdaFunctionConfiguration"))
    {
        let configs = match lambda_configs {
            Value::Array(arr) => arr.clone(),
            other => vec![other.clone()],
        };
        for config in configs {
            let arn = config.get("CloudFunction")
                .or_else(|| config.get("LambdaFunctionArn"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let events = parse_event_list(&config);
            if !arn.is_empty() {
                destinations.push(NotificationDestination { dest_type: "lambda".to_string(), arn, events });
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
pub fn get_bucket_notification_configuration(state: &S3State, input: &Value) -> Result<Value, AwsError> {
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
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
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
