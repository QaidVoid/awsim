use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::StepFunctionsState;

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "resourceArn is required"))?;

    validate_aws_tags(&input["tags"], &TagOpts::aws_default())?;

    let tags_input = input["tags"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "tags is required"))?;

    if let Some(mut sm) = state.state_machines.get_mut(resource_arn) {
        for tag in tags_input {
            if let (Some(k), Some(v)) = (tag["key"].as_str(), tag["value"].as_str()) {
                sm.tags.insert(k.to_string(), v.to_string());
            }
        }
        return Ok(json!({}));
    }

    if let Some(mut activity) = state.activities.get_mut(resource_arn) {
        for tag in tags_input {
            if let (Some(k), Some(v)) = (tag["key"].as_str(), tag["value"].as_str()) {
                activity.tags.insert(k.to_string(), v.to_string());
            }
        }
        return Ok(json!({}));
    }

    Err(AwsError::not_found(
        "ResourceNotFound",
        format!("Resource not found: {resource_arn}"),
    ))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "resourceArn is required"))?;

    validate_aws_tag_keys(&input["tagKeys"])?;

    let tag_keys = input["tagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "tagKeys is required"))?;

    if let Some(mut sm) = state.state_machines.get_mut(resource_arn) {
        for key in tag_keys {
            if let Some(k) = key.as_str() {
                sm.tags.remove(k);
            }
        }
        return Ok(json!({}));
    }

    if let Some(mut activity) = state.activities.get_mut(resource_arn) {
        for key in tag_keys {
            if let Some(k) = key.as_str() {
                activity.tags.remove(k);
            }
        }
        return Ok(json!({}));
    }

    Err(AwsError::not_found(
        "ResourceNotFound",
        format!("Resource not found: {resource_arn}"),
    ))
}

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "resourceArn is required"))?;

    if let Some(sm) = state.state_machines.get(resource_arn) {
        let tags: Vec<Value> = sm
            .tags
            .iter()
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect();
        return Ok(json!({ "tags": tags }));
    }

    if let Some(activity) = state.activities.get(resource_arn) {
        let tags: Vec<Value> = activity
            .tags
            .iter()
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect();
        return Ok(json!({ "tags": tags }));
    }

    Err(AwsError::not_found(
        "ResourceNotFound",
        format!("Resource not found: {resource_arn}"),
    ))
}
