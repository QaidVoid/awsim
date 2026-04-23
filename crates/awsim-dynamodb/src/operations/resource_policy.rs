use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::DynamoState;

use super::require_str;

pub fn put_resource_policy(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;
    let policy = require_str(input, "Policy")?;

    state
        .resource_policies
        .insert(resource_arn.to_string(), policy.to_string());

    Ok(json!({
        "RevisionId": format!("{:x}", policy.len() as u64)
    }))
}

pub fn get_resource_policy(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;

    let policy = state.resource_policies.get(resource_arn).ok_or_else(|| {
        AwsError::not_found(
            "PolicyNotFoundException",
            format!("No resource policy attached to: {resource_arn}"),
        )
    })?;

    Ok(json!({
        "Policy": policy.value(),
        "RevisionId": format!("{:x}", policy.value().len() as u64)
    }))
}

pub fn delete_resource_policy(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;

    let removed = state
        .resource_policies
        .remove(resource_arn)
        .map(|(_, p)| p);

    let revision = removed.map(|p| format!("{:x}", p.len() as u64)).unwrap_or_default();

    Ok(json!({
        "RevisionId": revision
    }))
}
