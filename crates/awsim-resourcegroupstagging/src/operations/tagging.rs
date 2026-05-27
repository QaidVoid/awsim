use std::collections::BTreeMap;

use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Map, Value, json};

use crate::state::TaggingState;

/// `TagResources` — apply a `Tags` map to one or more `ResourceARNList` ARNs.
///
/// Returns `FailedResourcesMap[ARN -> { StatusCode, ErrorCode, ErrorMessage }]`
/// populated with one entry per ARN that the index has no record of.
/// Reserved `aws:` keys raise a top-level `ConstraintViolationException`
/// (no resource is tagged) because the same `Tags` map applies to the
/// whole batch — there is no per-ARN partial-success in that case.
pub fn tag_resources(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns = arn_list(input, "ResourceARNList")?;
    let tags_value = input
        .get("Tags")
        .ok_or_else(|| AwsError::validation("Tags is required"))?;
    reject_reserved_tag_keys(tags_value)?;
    validate_aws_tags(tags_value, &TagOpts::aws_default())?;
    let tags = parse_tags(Some(tags_value))?;

    let mut failed = Map::new();
    for arn in &arns {
        match state.resources.get_mut(arn) {
            Some(mut entry) => {
                for (k, v) in &tags {
                    entry.insert(k.clone(), v.clone());
                }
            }
            None => {
                failed.insert(arn.clone(), resource_not_found_entry(arn));
            }
        }
    }

    Ok(json!({ "FailedResourcesMap": failed }))
}

/// `UntagResources` — remove the given `TagKeys` from each ARN.
///
/// Mirrors [`tag_resources`]: reserved `aws:` keys fail the whole
/// request with `ConstraintViolationException`; unknown ARNs land in
/// `FailedResourcesMap` so the caller can retry just those entries.
pub fn untag_resources(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns = arn_list(input, "ResourceARNList")?;
    let tag_keys_value = input
        .get("TagKeys")
        .ok_or_else(|| AwsError::validation("TagKeys is required"))?;
    reject_reserved_tag_key_list(tag_keys_value)?;
    validate_aws_tag_keys(tag_keys_value)?;
    let keys: Vec<String> = tag_keys_value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let mut failed = Map::new();
    for arn in &arns {
        match state.resources.get_mut(arn) {
            Some(mut entry) => {
                for key in &keys {
                    entry.remove(key);
                }
            }
            None => {
                failed.insert(arn.clone(), resource_not_found_entry(arn));
            }
        }
    }

    Ok(json!({ "FailedResourcesMap": failed }))
}

fn reject_reserved_tag_keys(tags: &Value) -> Result<(), AwsError> {
    let Some(map) = tags.as_object() else {
        return Ok(());
    };
    for key in map.keys() {
        if key.starts_with("aws:") {
            return Err(AwsError::bad_request(
                "ConstraintViolationException",
                format!("Tag key '{key}' uses the reserved 'aws:' prefix."),
            ));
        }
    }
    Ok(())
}

fn reject_reserved_tag_key_list(keys: &Value) -> Result<(), AwsError> {
    let Some(arr) = keys.as_array() else {
        return Ok(());
    };
    for k in arr.iter().filter_map(Value::as_str) {
        if k.starts_with("aws:") {
            return Err(AwsError::bad_request(
                "ConstraintViolationException",
                format!("Tag key '{k}' uses the reserved 'aws:' prefix."),
            ));
        }
    }
    Ok(())
}

fn resource_not_found_entry(arn: &str) -> Value {
    json!({
        "StatusCode": 404,
        "ErrorCode": "ResourceNotFoundException",
        "ErrorMessage": format!("Resource '{arn}' was not found in the tagging index."),
    })
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

    const Q: &str = "arn:aws:sqs:us-east-1:000000000000:q";

    fn registered(arn: &str) -> TaggingState {
        let s = TaggingState::default();
        s.resources.insert(arn.into(), BTreeMap::new());
        s
    }

    #[test]
    fn tag_resources_rejects_aws_prefix_with_constraint_violation() {
        let state = registered(Q);
        let err = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "Tags": { "aws:internal": "v" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ConstraintViolationException");
    }

    #[test]
    fn tag_resources_rejects_out_of_charset_key() {
        let state = registered(Q);
        let err = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "Tags": { "fire\u{1f525}": "v" } }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}",
        );
    }

    #[test]
    fn tag_resources_rejects_out_of_charset_value() {
        let state = registered(Q);
        let err = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "Tags": { "env": "\u{0007}beep" } }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}",
        );
    }

    #[test]
    fn tag_resources_rejects_oversize_value() {
        let state = registered(Q);
        let big = "v".repeat(257);
        let err = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "Tags": { "k": big } }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation exception, got {err:?}",
        );
    }

    #[test]
    fn untag_resources_rejects_aws_prefix_with_constraint_violation() {
        let state = registered(Q);
        let err = untag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "TagKeys": ["aws:internal"] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ConstraintViolationException");
    }

    #[test]
    fn tag_resources_accepts_well_formed_tags() {
        let state = registered(Q);
        let resp = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q], "Tags": { "env": "prod", "team": "data" } }),
            &ctx(),
        )
        .unwrap();
        assert!(
            resp["FailedResourcesMap"].as_object().unwrap().is_empty(),
            "expected empty FailedResourcesMap, got {resp}",
        );
        let stored = state.resources.get(Q).unwrap();
        assert_eq!(stored.get("env"), Some(&"prod".to_string()));
        assert_eq!(stored.get("team"), Some(&"data".to_string()));
    }

    #[test]
    fn tag_resources_unknown_arn_lands_in_failed_map() {
        let state = registered(Q);
        let unknown = "arn:aws:sqs:us-east-1:000000000000:ghost";
        let resp = tag_resources(
            &state,
            &json!({ "ResourceARNList": [Q, unknown], "Tags": { "env": "prod" } }),
            &ctx(),
        )
        .unwrap();

        let failed = resp["FailedResourcesMap"].as_object().unwrap();
        assert!(!failed.contains_key(Q));
        let entry = failed.get(unknown).expect("unknown ARN must be in map");
        assert_eq!(entry["StatusCode"], 404);
        assert_eq!(entry["ErrorCode"], "ResourceNotFoundException");
        assert!(entry["ErrorMessage"].as_str().unwrap().contains(unknown));

        // Known ARN was still tagged despite the partial failure.
        assert_eq!(
            state.resources.get(Q).unwrap().get("env"),
            Some(&"prod".to_string()),
        );
        // Unknown ARN was NOT created.
        assert!(state.resources.get(unknown).is_none());
    }

    #[test]
    fn untag_resources_unknown_arn_lands_in_failed_map() {
        let state = registered(Q);
        state
            .resources
            .get_mut(Q)
            .unwrap()
            .insert("env".into(), "prod".into());
        let unknown = "arn:aws:sqs:us-east-1:000000000000:missing";

        let resp = untag_resources(
            &state,
            &json!({ "ResourceARNList": [Q, unknown], "TagKeys": ["env"] }),
            &ctx(),
        )
        .unwrap();

        let failed = resp["FailedResourcesMap"].as_object().unwrap();
        assert!(!failed.contains_key(Q));
        let entry = failed.get(unknown).expect("unknown ARN must be in map");
        assert_eq!(entry["ErrorCode"], "ResourceNotFoundException");

        // Known ARN had its tag removed.
        assert!(state.resources.get(Q).unwrap().get("env").is_none());
    }
}
