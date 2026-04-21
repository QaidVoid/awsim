use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::state::RdsState;

use super::require_str;

/// Parse a tags list from input.
/// AWS SDK sends tags as `Tags.Tag.N.Key` / `Tags.Tag.N.Value` but after
/// the AwsQuery parser flattens them we receive a JSON array under "Tags".
fn parse_tags(input: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(arr) = input.get("Tags").and_then(|v| v.as_array()) {
        for tag in arr {
            if let (Some(k), Some(v)) = (
                tag.get("Key").and_then(|v| v.as_str()),
                tag.get("Value").and_then(|v| v.as_str()),
            ) {
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    map
}

pub fn add_tags_to_resource(
    state: &RdsState,
    input: &Value,
) -> Result<Value, AwsError> {
    let resource_name = require_str(input, "ResourceName")?;
    let new_tags = parse_tags(input);

    let mut entry = state.tags.entry(resource_name.to_string()).or_default();
    entry.extend(new_tags);

    Ok(json!({}))
}

pub fn remove_tags_from_resource(
    state: &RdsState,
    input: &Value,
) -> Result<Value, AwsError> {
    let resource_name = require_str(input, "ResourceName")?;

    let keys_to_remove: Vec<String> = input
        .get("TagKeys")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|k| k.as_str()).map(|s| s.to_string()).collect())
        .unwrap_or_default();

    if let Some(mut entry) = state.tags.get_mut(resource_name) {
        for key in &keys_to_remove {
            entry.remove(key);
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &RdsState,
    input: &Value,
) -> Result<Value, AwsError> {
    let resource_name = require_str(input, "ResourceName")?;

    let tags: Vec<Value> = state
        .tags
        .get(resource_name)
        .map(|entry| {
            entry
                .iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "TagList": { "Tag": tags },
    }))
}
