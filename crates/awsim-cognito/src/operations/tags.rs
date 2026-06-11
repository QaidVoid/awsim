use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::CognitoState;

/// Cognito tags only attach to user pools, so a ResourceArn that matches no
/// pool is a ResourceNotFoundException rather than a silently-accepted no-op.
fn require_known_resource(state: &CognitoState, resource_arn: &str) -> Result<(), AwsError> {
    if state.user_pools.iter().any(|p| p.arn == resource_arn) {
        Ok(())
    } else {
        Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Resource {resource_arn} not found."),
        ))
    }
}

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ResourceArn is required")
    })?;

    require_known_resource(state, resource_arn)?;
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;

    let mut entry = state
        .resource_tags
        .entry(resource_arn.to_string())
        .or_default();

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
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ResourceArn is required")
    })?;

    require_known_resource(state, resource_arn)?;
    validate_aws_tag_keys(&input["TagKeys"])?;

    let tag_keys: Vec<String> = input["TagKeys"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
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
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ResourceArn is required")
    })?;

    for pool in state.user_pools.iter() {
        if pool.arn == resource_arn {
            return Ok(json!({ "Tags": pool.tags }));
        }
    }

    Err(AwsError::service_not_found(
        "ResourceNotFoundException",
        format!("Resource {resource_arn} not found."),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::pools::create_user_pool;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    #[test]
    fn tag_resource_rejects_unknown_arn() {
        let state = CognitoState::default();
        let err = tag_resource(
            &state,
            &json!({ "ResourceArn": "arn:aws:cognito-idp:us-east-1:0:userpool/nope",
                     "Tags": { "k": "v" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn tag_and_list_round_trip_on_pool_arn() {
        let state = CognitoState::default();
        let created = create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let arn = created["UserPool"]["Arn"].as_str().unwrap().to_string();
        tag_resource(
            &state,
            &json!({ "ResourceArn": arn, "Tags": { "team": "auth" } }),
            &ctx(),
        )
        .unwrap();
        let listed =
            list_tags_for_resource(&state, &json!({ "ResourceArn": arn }), &ctx()).unwrap();
        assert_eq!(listed["Tags"]["team"], "auth");
    }
}
