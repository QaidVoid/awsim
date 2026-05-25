use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::PipesState;

fn pipe_name_from_arn(arn: &str) -> Option<String> {
    arn.rsplit_once('/').map(|(_, n)| n.to_string())
}

pub fn tag_resource(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input
        .get("resourceArn")
        .or_else(|| input.get("ResourceArn"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Missing resourceArn"))?;
    let name = pipe_name_from_arn(arn).unwrap_or_default();
    let tags_val = input
        .get("tags")
        .or_else(|| input.get("Tags"))
        .cloned()
        .unwrap_or(Value::Null);
    validate_aws_tags(&tags_val, &TagOpts::aws_default())?;
    let tags = tags_val
        .as_object()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Missing tags"))?
        .clone();
    let tags = &tags;
    if let Some(mut p) = state.pipes.get_mut(&name) {
        for (k, v) in tags {
            if let Some(s) = v.as_str() {
                p.tags.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input
        .get("resourceArn")
        .or_else(|| input.get("ResourceArn"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Missing resourceArn"))?;
    let name = pipe_name_from_arn(arn).unwrap_or_default();
    let keys_val = input
        .get("tagKeys")
        .or_else(|| input.get("TagKeys"))
        .cloned()
        .unwrap_or(Value::Null);
    validate_aws_tag_keys(&keys_val)?;
    let keys = keys_val
        .as_array()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Missing tagKeys"))?
        .clone();
    let keys = &keys;
    if let Some(mut p) = state.pipes.get_mut(&name) {
        for k in keys {
            if let Some(s) = k.as_str() {
                p.tags.remove(s);
            }
        }
    }
    Ok(json!({}))
}
