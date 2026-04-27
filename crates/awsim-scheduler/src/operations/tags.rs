use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SchedulerState;

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "ResourceArn is required"))?
        .to_string();

    let tags_input = input["Tags"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Tags is required"))?;

    let mut tags_entry = state.tags.entry(resource_arn).or_default();

    for (k, v) in tags_input {
        if let Some(val) = v.as_str() {
            tags_entry.insert(k.clone(), val.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "ResourceArn is required"))?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "TagKeys is required"))?;

    if let Some(mut tags) = state.tags.get_mut(resource_arn) {
        for key in tag_keys {
            if let Some(k) = key.as_str() {
                tags.remove(k);
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "ResourceArn is required"))?;

    let tags = state
        .tags
        .get(resource_arn)
        .map(|t| {
            let mut map = serde_json::Map::new();
            for (k, v) in t.iter() {
                map.insert(k.clone(), json!(v));
            }
            Value::Object(map)
        })
        .unwrap_or_else(|| json!({}));

    Ok(json!({ "Tags": tags }))
}
