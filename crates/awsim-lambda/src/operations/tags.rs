use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{error::resource_not_found, state::LambdaState, util::require_str};

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Resource is the function ARN; resolve to function name
    let resource = require_str(input, "Resource")?;
    let name = function_name_from_arn_or_name(resource);

    let mut func = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let tags = input["Tags"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValueException", "Tags is required"))?;

    for (k, v) in tags {
        if let Some(s) = v.as_str() {
            func.tags.insert(k.clone(), s.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource = require_str(input, "Resource")?;
    let name = function_name_from_arn_or_name(resource);

    let mut func = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValueException", "TagKeys is required"))?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            func.tags.remove(k);
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTags
// ---------------------------------------------------------------------------

pub fn list_tags(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource = require_str(input, "Resource")?;
    let name = function_name_from_arn_or_name(resource);

    let func = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let tags: serde_json::Map<String, Value> = func
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "Tags": tags }))
}

// ---------------------------------------------------------------------------
// Helper — resolve function name from ARN or bare name
// ---------------------------------------------------------------------------

fn function_name_from_arn_or_name(resource: &str) -> &str {
    // ARN: arn:aws:lambda:{region}:{account}:function:{name}
    if resource.starts_with("arn:") {
        resource.rsplit(':').next().unwrap_or(resource)
    } else {
        resource
    }
}
