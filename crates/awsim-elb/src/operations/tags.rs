use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{error::resource_not_found, state::ElbState};

/// Parse tags from `Tags.member.N` or `Tags` array.
fn parse_tags(input: &Value) -> Vec<(String, String)> {
    let mut tags = Vec::new();

    if let Some(t) = input.get("Tags") {
        let items: Vec<&Value> = match t {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                let members = if let Some(Value::Object(m)) = map.get("member") {
                    m.values().collect()
                } else {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    pairs.into_iter().map(|(_, v)| v).collect()
                };
                members
            }
            _ => vec![],
        };

        for item in items {
            if let (Some(k), Some(v)) = (
                item.get("Key").and_then(|v| v.as_str()),
                item.get("Value").and_then(|v| v.as_str()),
            ) {
                tags.push((k.to_string(), v.to_string()));
            }
        }
    }

    tags
}

pub fn add_tags(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let resource_arns = super::extract_string_list(input, "ResourceArns");
    let tags = parse_tags(input);

    for arn in &resource_arns {
        if let Some(mut lb) = state.load_balancers.get_mut(arn) {
            for (k, v) in &tags {
                lb.tags.insert(k.clone(), v.clone());
            }
            continue;
        }
        if let Some(mut tg) = state.target_groups.get_mut(arn) {
            for (k, v) in &tags {
                tg.tags.insert(k.clone(), v.clone());
            }
            continue;
        }
        // Silently skip unknown ARNs (real ELB does the same)
    }

    Ok(json!({}))
}

pub fn remove_tags(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let resource_arns = super::extract_string_list(input, "ResourceArns");
    let tag_keys = super::extract_string_list(input, "TagKeys");

    for arn in &resource_arns {
        if let Some(mut lb) = state.load_balancers.get_mut(arn) {
            for k in &tag_keys {
                lb.tags.remove(k);
            }
            continue;
        }
        if let Some(mut tg) = state.target_groups.get_mut(arn) {
            for k in &tag_keys {
                tg.tags.remove(k);
            }
            continue;
        }
    }

    Ok(json!({}))
}

pub fn describe_tags(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let resource_arns = super::extract_string_list(input, "ResourceArns");

    let mut descriptions = Vec::new();

    for arn in &resource_arns {
        let tags_map = if let Some(lb) = state.load_balancers.get(arn) {
            lb.tags.clone()
        } else if let Some(tg) = state.target_groups.get(arn) {
            tg.tags.clone()
        } else {
            return Err(resource_not_found("resource", arn));
        };

        let tag_list: Vec<Value> = tags_map
            .into_iter()
            .map(|(k, v)| json!({ "Key": k, "Value": v }))
            .collect();

        descriptions.push(json!({
            "ResourceArn": arn,
            "Tags": { "member": tag_list }
        }));
    }

    Ok(json!({
        "TagDescriptions": {
            "member": descriptions
        }
    }))
}
