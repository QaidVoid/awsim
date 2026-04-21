use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SsmState;

// ---------------------------------------------------------------------------
// AddTagsToResource
// ---------------------------------------------------------------------------

pub fn add_tags_to_resource(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input["ResourceType"].as_str().unwrap_or("");
    let resource_id = input["ResourceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceId is required"))?;

    if resource_type != "Parameter" {
        return Err(AwsError::bad_request(
            "InvalidResourceType",
            format!("Unsupported resource type: {resource_type}"),
        ));
    }

    let tag_list = input["Tags"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Tags is required"))?;

    let mut param = state.parameters.get_mut(resource_id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidResourceId",
            format!("Parameter {resource_id} not found"),
        )
    })?;

    for tag in tag_list {
        if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
            param.tags.insert(k.to_string(), v.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RemoveTagsFromResource
// ---------------------------------------------------------------------------

pub fn remove_tags_from_resource(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input["ResourceType"].as_str().unwrap_or("");
    let resource_id = input["ResourceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceId is required"))?;

    if resource_type != "Parameter" {
        return Err(AwsError::bad_request(
            "InvalidResourceType",
            format!("Unsupported resource type: {resource_type}"),
        ));
    }

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TagKeys is required"))?;

    let mut param = state.parameters.get_mut(resource_id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidResourceId",
            format!("Parameter {resource_id} not found"),
        )
    })?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            param.tags.remove(k);
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input["ResourceType"].as_str().unwrap_or("");
    let resource_id = input["ResourceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceId is required"))?;

    if resource_type != "Parameter" {
        return Err(AwsError::bad_request(
            "InvalidResourceType",
            format!("Unsupported resource type: {resource_type}"),
        ));
    }

    let param = state.parameters.get(resource_id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidResourceId",
            format!("Parameter {resource_id} not found"),
        )
    })?;

    let tags: Vec<Value> = param
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "TagList": tags }))
}
