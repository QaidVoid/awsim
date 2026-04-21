use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::Route53State;

fn resolve_zone_id(id_raw: &str) -> String {
    if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    }
}

/// POST /2013-04-01/tags/{ResourceType}/{ResourceId}
pub fn change_tags_for_resource(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input
        .get("ResourceType")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceType is required"))?;
    let resource_id = input
        .get("ResourceId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceId is required"))?;

    // Tags to add
    let add_tags: Vec<(String, String)> = input
        .get("AddTags")
        .and_then(|t| t.get("Tag"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|tag| {
                    let k = tag.get("Key").and_then(Value::as_str)?.to_string();
                    let v = tag.get("Value").and_then(Value::as_str)?.to_string();
                    Some((k, v))
                })
                .collect()
        })
        .unwrap_or_default();

    // Tags to remove
    let remove_keys: Vec<String> = input
        .get("RemoveTagKeys")
        .and_then(|k| k.get("Key"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match resource_type {
        "hostedzone" => {
            let id = resolve_zone_id(resource_id);
            let mut zone = state
                .hosted_zones
                .get_mut(&id)
                .ok_or_else(|| AwsError::not_found("NoSuchHostedZone", "Hosted zone not found"))?;
            for key in &remove_keys {
                zone.tags.remove(key);
            }
            for (k, v) in add_tags {
                zone.tags.insert(k, v);
            }
        }
        "healthcheck" => {
            let _hc = state
                .health_checks
                .get(resource_id)
                .ok_or_else(|| AwsError::not_found("NoSuchHealthCheck", "Health check not found"))?;
            // Health checks don't have tags in our simplified state; accept silently.
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("Unsupported ResourceType: {other}"),
            ));
        }
    }

    Ok(json!({}))
}

/// GET /2013-04-01/tags/{ResourceType}/{ResourceId}
pub fn list_tags_for_resource(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input
        .get("ResourceType")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceType is required"))?;
    let resource_id = input
        .get("ResourceId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceId is required"))?;

    let tags: Vec<Value> = match resource_type {
        "hostedzone" => {
            let id = resolve_zone_id(resource_id);
            let zone = state
                .hosted_zones
                .get(&id)
                .ok_or_else(|| AwsError::not_found("NoSuchHostedZone", "Hosted zone not found"))?;
            zone.tags
                .iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        }
        "healthcheck" => {
            let _hc = state
                .health_checks
                .get(resource_id)
                .ok_or_else(|| AwsError::not_found("NoSuchHealthCheck", "Health check not found"))?;
            vec![]
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("Unsupported ResourceType: {other}"),
            ));
        }
    };

    Ok(json!({
        "ResourceTagSet": {
            "ResourceType": resource_type,
            "ResourceId": resource_id,
            "Tags": tags,
        }
    }))
}
