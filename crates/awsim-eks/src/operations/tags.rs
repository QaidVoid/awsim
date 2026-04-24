use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EksState;

pub fn tag_resource(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;
    let mut entry = state.resource_tags.entry(arn.to_string()).or_default();
    if let Some(tags) = input["tags"].as_object() {
        for (k, v) in tags {
            if let Some(s) = v.as_str() {
                entry.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;
    if let Some(mut t) = state.resource_tags.get_mut(arn)
        && let Some(keys) = input["tagKeys"].as_array() {
            for k in keys {
                if let Some(s) = k.as_str() {
                    t.remove(s);
                }
            }
        }
    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;
    let tags = state
        .resource_tags
        .get(arn)
        .map(|t| t.clone())
        .unwrap_or_default();
    Ok(json!({ "tags": tags }))
}
