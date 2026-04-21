use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::CloudFrontState;

fn parse_tags(input: &Value) -> Vec<(String, String)> {
    let mut tags = Vec::new();

    let tags_val = input
        .get("Tags")
        .or_else(|| input.get("tags"))
        .unwrap_or(input);

    let items = tags_val
        .get("Items")
        .and_then(|v| v.get("Tag"))
        .or_else(|| tags_val.get("Items"))
        .unwrap_or(tags_val);

    let list: Vec<&Value> = match items {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(_) => vec![items],
        _ => vec![],
    };

    for item in list {
        if let (Some(k), Some(v)) = (
            item.get("Key").and_then(|v| v.as_str()),
            item.get("Value").and_then(|v| v.as_str()),
        ) {
            tags.push((k.to_string(), v.to_string()));
        }
    }

    tags
}

/// POST /2020-05-31/tagging?Operation=Tag
pub fn tag_resource(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    // The resource ARN is passed as a query param; it's in input["Resource"] after parsing.
    let resource_arn = input
        .get("Resource")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Extract distribution ID from ARN: arn:aws:cloudfront::{account}:distribution/{id}
    let dist_id = resource_arn.rsplit('/').next().unwrap_or("");
    let tags = parse_tags(input);

    if let Some(mut dist) = state.distributions.get_mut(dist_id) {
        for (k, v) in tags {
            dist.tags.insert(k, v);
        }
    }
    // Silently succeed even if distribution not found (matches AWS behavior)

    Ok(json!({}))
}

/// GET /2020-05-31/tagging?Resource={arn}
pub fn list_tags_for_resource(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input
        .get("Resource")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let dist_id = resource_arn.rsplit('/').next().unwrap_or("");

    let tags_map = state
        .distributions
        .get(dist_id)
        .map(|d| d.tags.clone())
        .unwrap_or_default();

    let tag_list: Vec<Value> = tags_map
        .into_iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    let qty = tag_list.len();

    Ok(json!({
        "Tags": {
            "Items": { "Tag": tag_list },
            "Quantity": qty
        }
    }))
}
