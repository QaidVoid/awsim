use awsim_core::AwsError;
use awsim_core::tags::{TagOpts, reject_aws_prefix_on_write, validate};
use serde_json::{Value, json};

use crate::{error::resource_not_found, state::ElbState};

/// Parse tags from `Tags.member.N` or `Tags` array. AWS's `member.N`
/// shape is sparse-tolerant: callers may skip indices, and the parser
/// must still order results by N so duplicate keys resolve
/// deterministically.
fn parse_tags(input: &Value) -> Vec<(String, String)> {
    let mut tags = Vec::new();

    if let Some(t) = input.get("Tags") {
        let items: Vec<&Value> = match t {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                let inner = if let Some(Value::Object(m)) = map.get("member") {
                    m
                } else {
                    map
                };
                let mut pairs: Vec<_> = inner.iter().collect();
                pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                pairs.into_iter().map(|(_, v)| v).collect()
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
    validate(&tags, &TagOpts::aws_default())?;

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
    reject_aws_prefix_on_write(&tag_keys)?;

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

#[cfg(test)]
mod parse_tags_tests {
    use super::*;

    #[test]
    fn sparse_member_indices_preserve_n_ordering() {
        let input = json!({
            "Tags": { "member": {
                "10": { "Key": "z", "Value": "10" },
                "1":  { "Key": "a", "Value": "1" },
                "3":  { "Key": "m", "Value": "3" }
            } }
        });
        let parsed = parse_tags(&input);
        assert_eq!(
            parsed,
            vec![
                ("a".to_string(), "1".to_string()),
                ("m".to_string(), "3".to_string()),
                ("z".to_string(), "10".to_string()),
            ]
        );
    }

    #[test]
    fn direct_numeric_keys_without_member_wrapper() {
        let input = json!({
            "Tags": {
                "2": { "Key": "b", "Value": "2" },
                "1": { "Key": "a", "Value": "1" }
            }
        });
        let parsed = parse_tags(&input);
        assert_eq!(parsed[0].0, "a");
        assert_eq!(parsed[1].0, "b");
    }
}
