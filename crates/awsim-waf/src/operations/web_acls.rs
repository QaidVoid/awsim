use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{WafState, WebAcl};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn validate_scope(scope: &str) -> Result<(), AwsError> {
    if !["REGIONAL", "CLOUDFRONT"].contains(&scope) {
        return Err(AwsError::bad_request(
            "WAFInvalidParameterException",
            "Scope must be REGIONAL or CLOUDFRONT",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CreateWebACL
// ---------------------------------------------------------------------------

pub fn create_web_acl(
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

    validate_scope(&scope)?;

    let key = format!("{scope}:{name}");
    if state.web_acls.contains_key(&key) {
        return Err(AwsError::conflict(
            "WAFDuplicateItemException",
            format!("WebACL with name '{name}' already exists in scope '{scope}'"),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let arn = format!(
        "arn:aws:wafv2:{}:{}:regional/webacl/{}/{}",
        ctx.region, ctx.account_id, name, id
    );
    let lock_token = Uuid::new_v4().to_string();

    let default_action = input["DefaultAction"].clone();
    let rules = input["Rules"].as_array().cloned().unwrap_or_default();
    let visibility_config = input["VisibilityConfig"].clone();

    let acl = WebAcl {
        id: id.clone(),
        name: name.clone(),
        scope: scope.clone(),
        arn: arn.clone(),
        default_action,
        rules,
        visibility_config,
        lock_token: lock_token.clone(),
        created_at: now_secs(),
    };

    state.web_acls.insert(key, acl);

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
// GetWebACL
// ---------------------------------------------------------------------------

pub fn get_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Name is required"))?;

    let scope = input["Scope"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "Scope is required")
    })?;

    let key = format!("{scope}:{name}");
    let acl = state.web_acls.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "WAFNonexistentItemException",
            format!("WebACL not found: {name}"),
        )
    })?;

    Ok(json!({
        "WebACL": {
            "ARN": acl.arn,
            "Id": acl.id,
            "Name": acl.name,
            "DefaultAction": acl.default_action,
            "Rules": acl.rules,
            "VisibilityConfig": acl.visibility_config,
        },
        "LockToken": acl.lock_token,
    }))
}

// ---------------------------------------------------------------------------
// ListWebACLs
// ---------------------------------------------------------------------------

pub fn list_web_acls(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let scope = input["Scope"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "Scope is required")
    })?;

    validate_scope(scope)?;

    let list: Vec<Value> = state
        .web_acls
        .iter()
        .filter(|e| e.value().scope == scope)
        .map(|e| {
            let acl = e.value();
            json!({
                "ARN": acl.arn,
                "Id": acl.id,
                "Name": acl.name,
                "LockToken": acl.lock_token,
            })
        })
        .collect();

    Ok(json!({ "WebACLs": list }))
}

// ---------------------------------------------------------------------------
// DeleteWebACL
// ---------------------------------------------------------------------------

pub fn delete_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Name is required"))?;

    let scope = input["Scope"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "Scope is required")
    })?;

    let _lock_token = input["LockToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "LockToken is required")
    })?;

    let key = format!("{scope}:{name}");
    if state.web_acls.remove(&key).is_none() {
        return Err(AwsError::not_found(
            "WAFNonexistentItemException",
            format!("WebACL not found: {name}"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateWebACL
// ---------------------------------------------------------------------------

pub fn update_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Name is required"))?;

    let scope = input["Scope"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "Scope is required")
    })?;

    let _lock_token = input["LockToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "LockToken is required")
    })?;

    let key = format!("{scope}:{name}");
    let mut acl = state.web_acls.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "WAFNonexistentItemException",
            format!("WebACL not found: {name}"),
        )
    })?;

    if !input["DefaultAction"].is_null() {
        acl.default_action = input["DefaultAction"].clone();
    }
    if let Some(rules) = input["Rules"].as_array() {
        acl.rules = rules.clone();
    }
    if !input["VisibilityConfig"].is_null() {
        acl.visibility_config = input["VisibilityConfig"].clone();
    }

    let new_lock = Uuid::new_v4().to_string();
    acl.lock_token = new_lock.clone();

    Ok(json!({ "NextLockToken": new_lock }))
}
