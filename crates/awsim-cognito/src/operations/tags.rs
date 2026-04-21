use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::CognitoState;

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let mut entry = state
        .resource_tags
        .entry(resource_arn.to_string())
        .or_insert_with(HashMap::new);

    if let Some(tags_obj) = input["Tags"].as_object() {
        for (k, v) in tags_obj {
            if let Some(val) = v.as_str() {
                entry.insert(k.clone(), val.to_string());
            }
        }
    }

    // Also update pool tags if the ARN points to a pool
    for mut pool in state.user_pools.iter_mut() {
        if pool.arn == resource_arn {
            if let Some(tags_obj) = input["Tags"].as_object() {
                for (k, v) in tags_obj {
                    if let Some(val) = v.as_str() {
                        pool.tags.insert(k.clone(), val.to_string());
                    }
                }
            }
            break;
        }
    }

    info!(resource_arn = %resource_arn, "Cognito: tagged resource");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let tag_keys: Vec<String> = input["TagKeys"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if let Some(mut tags) = state.resource_tags.get_mut(resource_arn) {
        for key in &tag_keys {
            tags.remove(key);
        }
    }

    for mut pool in state.user_pools.iter_mut() {
        if pool.arn == resource_arn {
            for key in &tag_keys {
                pool.tags.remove(key);
            }
            break;
        }
    }

    info!(resource_arn = %resource_arn, "Cognito: untagged resource");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    // Check pool tags first
    for pool in state.user_pools.iter() {
        if pool.arn == resource_arn {
            return Ok(json!({ "Tags": pool.tags }));
        }
    }

    // Fall back to resource_tags store
    let tags = state
        .resource_tags
        .get(resource_arn)
        .map(|t| t.clone())
        .unwrap_or_default();

    Ok(json!({ "Tags": tags }))
}
