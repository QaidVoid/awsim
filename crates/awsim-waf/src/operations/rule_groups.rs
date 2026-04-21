use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{RuleGroup, WafState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateRuleGroup
// ---------------------------------------------------------------------------

pub fn create_rule_group(
    state: &WafState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Name is required"))?
        .to_string();

    let scope = input["Scope"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Scope is required"))?
        .to_string();

    let capacity = input["Capacity"].as_u64().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "Capacity is required")
    })?;

    let key = format!("{scope}:{name}");
    if state.rule_groups.contains_key(&key) {
        return Err(AwsError::conflict(
            "WAFDuplicateItemException",
            format!("RuleGroup with name '{name}' already exists in scope '{scope}'"),
        ));
    }

    let rules = input["Rules"].as_array().cloned().unwrap_or_default();

    let id = Uuid::new_v4().to_string();
    let arn = format!(
        "arn:aws:wafv2:{}:{}:regional/rulegroup/{}/{}",
        ctx.region, ctx.account_id, name, id
    );
    let lock_token = Uuid::new_v4().to_string();

    let rg = RuleGroup {
        id: id.clone(),
        name: name.clone(),
        scope: scope.clone(),
        arn: arn.clone(),
        capacity,
        rules,
        lock_token: lock_token.clone(),
        created_at: now_secs(),
    };

    state.rule_groups.insert(key, rg);

    Ok(json!({
        "Summary": {
            "ARN": arn,
            "Id": id,
            "Name": name,
            "LockToken": lock_token,
        }
    }))
}

// ---------------------------------------------------------------------------
// ListRuleGroups
// ---------------------------------------------------------------------------

pub fn list_rule_groups(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let scope = input["Scope"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Scope is required"))?;

    let list: Vec<Value> = state
        .rule_groups
        .iter()
        .filter(|e| e.value().scope == scope)
        .map(|e| {
            let rg = e.value();
            json!({
                "ARN": rg.arn,
                "Id": rg.id,
                "Name": rg.name,
                "LockToken": rg.lock_token,
            })
        })
        .collect();

    Ok(json!({ "RuleGroups": list }))
}

// ---------------------------------------------------------------------------
// DeleteRuleGroup
// ---------------------------------------------------------------------------

pub fn delete_rule_group(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Name is required"))?;

    let scope = input["Scope"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Scope is required"))?;

    let _lock_token = input["LockToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "LockToken is required"))?;

    let key = format!("{scope}:{name}");
    if state.rule_groups.remove(&key).is_none() {
        return Err(AwsError::not_found(
            "WAFNonexistentItemException",
            format!("RuleGroup not found: {name}"),
        ));
    }

    Ok(json!({}))
}
