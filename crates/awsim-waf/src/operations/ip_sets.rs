use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{IpSet, WafState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateIPSet
// ---------------------------------------------------------------------------

pub fn create_ip_set(
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

    let ip_address_version = input["IPAddressVersion"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("WAFInvalidParameterException", "IPAddressVersion is required")
        })?
        .to_string();

    if !["IPV4", "IPV6"].contains(&ip_address_version.as_str()) {
        return Err(AwsError::bad_request(
            "WAFInvalidParameterException",
            "IPAddressVersion must be IPV4 or IPV6",
        ));
    }

    let key = format!("{scope}:{name}");
    if state.ip_sets.contains_key(&key) {
        return Err(AwsError::conflict(
            "WAFDuplicateItemException",
            format!("IPSet with name '{name}' already exists in scope '{scope}'"),
        ));
    }

    let addresses: Vec<String> = input["Addresses"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let id = Uuid::new_v4().to_string();
    let arn = format!(
        "arn:aws:wafv2:{}:{}:regional/ipset/{}/{}",
        ctx.region, ctx.account_id, name, id
    );
    let lock_token = Uuid::new_v4().to_string();

    let ip_set = IpSet {
        id: id.clone(),
        name: name.clone(),
        scope: scope.clone(),
        arn: arn.clone(),
        ip_address_version,
        addresses,
        lock_token: lock_token.clone(),
        created_at: now_secs(),
    };

    state.ip_sets.insert(key, ip_set);

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
// GetIPSet
// ---------------------------------------------------------------------------

pub fn get_ip_set(
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

    let key = format!("{scope}:{name}");
    let ip_set = state.ip_sets.get(&key).ok_or_else(|| {
        AwsError::not_found("WAFNonexistentItemException", format!("IPSet not found: {name}"))
    })?;

    Ok(json!({
        "IPSet": {
            "ARN": ip_set.arn,
            "Id": ip_set.id,
            "Name": ip_set.name,
            "IPAddressVersion": ip_set.ip_address_version,
            "Addresses": ip_set.addresses,
        },
        "LockToken": ip_set.lock_token,
    }))
}

// ---------------------------------------------------------------------------
// ListIPSets
// ---------------------------------------------------------------------------

pub fn list_ip_sets(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let scope = input["Scope"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("WAFInvalidParameterException", "Scope is required"))?;

    let list: Vec<Value> = state
        .ip_sets
        .iter()
        .filter(|e| e.value().scope == scope)
        .map(|e| {
            let s = e.value();
            json!({
                "ARN": s.arn,
                "Id": s.id,
                "Name": s.name,
                "LockToken": s.lock_token,
            })
        })
        .collect();

    Ok(json!({ "IPSets": list }))
}

// ---------------------------------------------------------------------------
// UpdateIPSet
// ---------------------------------------------------------------------------

pub fn update_ip_set(
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

    let _lock_token = input["LockToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "LockToken is required")
    })?;

    let key = format!("{scope}:{name}");
    let mut ip_set = state.ip_sets.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "WAFNonexistentItemException",
            format!("IPSet not found: {name}"),
        )
    })?;

    if let Some(addresses) = input["Addresses"].as_array() {
        ip_set.addresses = addresses
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    let new_lock = Uuid::new_v4().to_string();
    ip_set.lock_token = new_lock.clone();

    Ok(json!({ "NextLockToken": new_lock }))
}

// ---------------------------------------------------------------------------
// DeleteIPSet
// ---------------------------------------------------------------------------

pub fn delete_ip_set(
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
    if state.ip_sets.remove(&key).is_none() {
        return Err(AwsError::not_found(
            "WAFNonexistentItemException",
            format!("IPSet not found: {name}"),
        ));
    }

    Ok(json!({}))
}
