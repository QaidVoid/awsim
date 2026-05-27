use std::collections::BTreeMap;

use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Map, Value, json};

use crate::state::TaggingState;

/// `TagResources` — apply a `Tags` map to one or more `ResourceARNList` ARNs.
///
/// Returns `FailedResourcesMap` (empty here, since the emulator never fails).
pub fn tag_resources(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns = arn_list(input, "ResourceARNList")?;
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;
    let tags = parse_tags(input.get("Tags"))?;

    for arn in &arns {
        let mut entry = state.resources.entry(arn.clone()).or_default();
        for (k, v) in &tags {
            entry.insert(k.clone(), v.clone());
        }
    }

    Ok(json!({ "FailedResourcesMap": Map::new() }))
}

/// `UntagResources` — remove the given `TagKeys` from each ARN.
pub fn untag_resources(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns = arn_list(input, "ResourceARNList")?;
    validate_aws_tag_keys(&input["TagKeys"])?;
    let keys: Vec<String> = input
        .get("TagKeys")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    for arn in &arns {
        if let Some(mut entry) = state.resources.get_mut(arn) {
            for key in &keys {
                entry.remove(key);
            }
        }
    }

    Ok(json!({ "FailedResourcesMap": Map::new() }))
}

fn arn_list(input: &Value, field: &str) -> Result<Vec<String>, AwsError> {
    let arns = input
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| AwsError::validation(format!("{field} is required")))?;
    if arns.is_empty() {
        return Err(AwsError::validation(format!("{field} cannot be empty")));
    }
    Ok(arns
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect())
}

fn parse_tags(value: Option<&Value>) -> Result<BTreeMap<String, String>, AwsError> {
    let map = value
        .and_then(Value::as_object)
        .ok_or_else(|| AwsError::validation("Tags is required"))?;
    Ok(map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::RequestContext;

    fn ctx() -> RequestContext {
        RequestContext::new("tagging", "us-east-1")
    }

    #[test]
    fn tag_resources_rejects_aws_prefix_on_write() {
        let state = TaggingState::default();
        let err = tag_resources(
            &state,
            &json!({
                "ResourceARNList": ["arn:aws:sqs:us-east-1:000000000000:q"],
                "Tags": { "aws:internal": "v" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}"
        );
    }

    #[test]
    fn tag_resources_rejects_oversize_value() {
        let state = TaggingState::default();
        let big = "v".repeat(257);
        let err = tag_resources(
            &state,
            &json!({
                "ResourceARNList": ["arn:aws:sqs:us-east-1:000000000000:q"],
                "Tags": { "k": big },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}"
        );
    }

    #[test]
    fn untag_resources_rejects_aws_prefix() {
        let state = TaggingState::default();
        let err = untag_resources(
            &state,
            &json!({
                "ResourceARNList": ["arn:aws:sqs:us-east-1:000000000000:q"],
                "TagKeys": ["aws:internal"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}"
        );
    }

    #[test]
    fn tag_resources_accepts_well_formed_tags() {
        let state = TaggingState::default();
        tag_resources(
            &state,
            &json!({
                "ResourceARNList": ["arn:aws:sqs:us-east-1:000000000000:q"],
                "Tags": { "env": "prod", "team": "data" },
            }),
            &ctx(),
        )
        .unwrap();
        let stored = state
            .resources
            .get("arn:aws:sqs:us-east-1:000000000000:q")
            .unwrap();
        assert_eq!(stored.get("env"), Some(&"prod".to_string()));
        assert_eq!(stored.get("team"), Some(&"data".to_string()));
    }
}
