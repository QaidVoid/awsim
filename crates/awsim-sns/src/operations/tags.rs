use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SnsState;

pub fn tag_resource(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let tag_list = input["Tags"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Tags is required"))?;

    let mut topic = state.topics.get_mut(resource_arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Resource not found: {resource_arn}"))
    })?;

    for tag in tag_list {
        if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
            topic.tags.insert(k.to_string(), v.to_string());
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TagKeys is required"))?;

    let mut topic = state.topics.get_mut(resource_arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Resource not found: {resource_arn}"))
    })?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            topic.tags.remove(k);
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let topic = state.topics.get(resource_arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Resource not found: {resource_arn}"))
    })?;

    let tags: Vec<Value> = topic
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "Tags": tags }))
}
