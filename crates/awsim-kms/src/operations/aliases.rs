use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::operations::keys::resolve_key_id;
use crate::state::KmsState;

// ---------------------------------------------------------------------------
// CreateAlias
// ---------------------------------------------------------------------------

pub fn create_alias(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let alias_name = input["AliasName"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("AliasName"))?;

    let target_key_id = input["TargetKeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("TargetKeyId"))?;

    if !alias_name.starts_with("alias/") {
        return Err(error::invalid_parameter(
            "AliasName must start with 'alias/'",
        ));
    }

    if alias_name.starts_with("alias/aws/") {
        return Err(error::invalid_parameter(
            "AliasName cannot use the reserved 'alias/aws/' prefix",
        ));
    }

    if state.aliases.contains_key(alias_name) {
        return Err(error::alias_exists(alias_name));
    }

    // Validate target key exists
    let key_id = resolve_key_id(state, target_key_id)?;

    state.aliases.insert(alias_name.to_string(), key_id);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteAlias
// ---------------------------------------------------------------------------

pub fn delete_alias(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let alias_name = input["AliasName"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("AliasName"))?;

    if state.aliases.remove(alias_name).is_none() {
        return Err(error::not_found("Alias"));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListAliases
// ---------------------------------------------------------------------------

pub fn list_aliases(
    state: &KmsState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let aliases: Vec<Value> = state
        .aliases
        .iter()
        .map(|entry| {
            let alias_name = entry.key().clone();
            let target_key_id = entry.value().clone();
            let alias_arn = format!(
                "arn:{}:kms:us-east-1:000000000000:{alias_name}",
                ctx.partition
            );
            json!({
                "AliasName": alias_name,
                "AliasArn": alias_arn,
                "TargetKeyId": target_key_id,
            })
        })
        .collect();

    Ok(json!({ "Aliases": aliases, "Truncated": false }))
}

pub fn update_alias(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let alias_name = input["AliasName"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("AliasName"))?;
    let target_key_id = input["TargetKeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("TargetKeyId"))?;

    if !state.aliases.contains_key(alias_name) {
        return Err(error::not_found("Alias"));
    }

    let key_id = resolve_key_id(state, target_key_id)?;
    state.aliases.insert(alias_name.to_string(), key_id);
    Ok(json!({}))
}
