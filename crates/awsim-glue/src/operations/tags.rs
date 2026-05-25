use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::GlueState;

// ---------------------------------------------------------------------------
// GetTags
// ---------------------------------------------------------------------------

pub fn get_tags(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "ResourceArn is required"))?;

    let tags_map = state.tags.get(resource_arn);
    let tags: Value = if let Some(tags) = tags_map {
        let obj: serde_json::Map<String, Value> =
            tags.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        json!(obj)
    } else {
        json!({})
    };

    Ok(json!({ "Tags": tags }))
}

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "ResourceArn is required"))?;

    validate_aws_tags(&input["TagsToAdd"], &TagOpts::aws_default())?;

    let new_tags = input["TagsToAdd"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TagsToAdd is required"))?;

    let mut entry = state.tags.entry(resource_arn.to_string()).or_default();

    for (k, v) in new_tags {
        if let Some(val) = v.as_str() {
            entry.insert(k.clone(), val.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "ResourceArn is required"))?;

    validate_aws_tag_keys(&input["TagsToRemove"])?;

    let tags_to_remove = input["TagsToRemove"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidInputException", "TagsToRemove is required")
    })?;

    if let Some(mut entry) = state.tags.get_mut(resource_arn) {
        for key_val in tags_to_remove {
            if let Some(k) = key_val.as_str() {
                entry.remove(k);
            }
        }
    }

    Ok(json!({}))
}
