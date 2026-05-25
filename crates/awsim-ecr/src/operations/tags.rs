use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EcrState;

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    // Extract repo name from ARN: arn:aws:ecr:{region}:{account}:repository/{name}
    let repo_name = resource_arn
        .split("/")
        .last()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Invalid resourceArn"))?;

    let repo = state.repositories.get(repo_name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let tags: Vec<Value> = repo
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "tags": tags }))
}

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let repo_name = resource_arn
        .split("/")
        .last()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Invalid resourceArn"))?;

    let mut repo = state.repositories.get_mut(repo_name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    if let Some(tag_list) = input["tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                repo.tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let repo_name = resource_arn
        .split("/")
        .last()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Invalid resourceArn"))?;

    let mut repo = state.repositories.get_mut(repo_name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    if let Some(key_list) = input["tagKeys"].as_array() {
        for k in key_list {
            if let Some(key) = k.as_str() {
                repo.tags.remove(key);
            }
        }
    }

    Ok(json!({}))
}
