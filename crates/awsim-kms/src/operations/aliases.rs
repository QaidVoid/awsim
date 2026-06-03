use awsim_core::pagination::{cap_max_results, paginate};
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
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let limit = cap_max_results(input["Limit"].as_i64(), 100, 1000);
    let mut items: Vec<(String, Value)> = state
        .aliases
        .iter()
        .map(|entry| {
            let alias_name = entry.key().clone();
            let target_key_id = entry.value().clone();
            let alias_arn = format!(
                "arn:{}:kms:us-east-1:000000000000:{alias_name}",
                ctx.partition
            );
            (
                alias_name.clone(),
                json!({
                    "AliasName": alias_name,
                    "AliasArn": alias_arn,
                    "TargetKeyId": target_key_id,
                }),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let page = paginate(items, limit, input["Marker"].as_str(), |(name, _)| {
        name.clone()
    })?;
    let aliases: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    let mut resp = json!({ "Aliases": aliases, "Truncated": page.next_token.is_some() });
    if let Some(marker) = page.next_token {
        resp["NextMarker"] = json!(marker);
    }
    Ok(resp)
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
