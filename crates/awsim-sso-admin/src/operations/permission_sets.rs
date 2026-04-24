use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{PermissionSet, SsoAdminState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn create_permission_set(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?
        .to_string();

    let instance_arn = input["InstanceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "InstanceArn is required"))?;

    let id = format!("ps-{}", uuid::Uuid::new_v4().simple());
    let arn = format!("{instance_arn}/permissionSet/{id}");

    let ps = PermissionSet {
        arn: arn.clone(),
        name: name.clone(),
        description: input["Description"].as_str().unwrap_or("").to_string(),
        session_duration: input["SessionDuration"]
            .as_str()
            .unwrap_or("PT1H")
            .to_string(),
        relay_state: input["RelayState"].as_str().unwrap_or("").to_string(),
        created_at: now_secs(),
        managed_policies: vec![],
        inline_policy: String::new(),
    };

    state.permission_sets.insert(arn.clone(), ps);

    Ok(json!({
        "PermissionSet": {
            "Name": name,
            "PermissionSetArn": arn,
            "Description": input["Description"].as_str().unwrap_or(""),
            "CreatedDate": 0,
            "SessionDuration": input["SessionDuration"].as_str().unwrap_or("PT1H"),
            "RelayState": input["RelayState"].as_str().unwrap_or(""),
        }
    }))
}

pub fn describe_permission_set(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;

    let ps = state.permission_sets.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Permission set not found: {arn}"),
        )
    })?;

    Ok(json!({
        "PermissionSet": {
            "Name": ps.name,
            "PermissionSetArn": ps.arn,
            "Description": ps.description,
            "CreatedDate": ps.created_at,
            "SessionDuration": ps.session_duration,
            "RelayState": ps.relay_state,
        }
    }))
}

pub fn delete_permission_set(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;
    state.permission_sets.remove(arn);
    Ok(json!({}))
}

pub fn list_permission_sets(
    state: &SsoAdminState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns: Vec<String> = state
        .permission_sets
        .iter()
        .map(|e| e.value().arn.clone())
        .collect();

    Ok(json!({ "PermissionSets": arns }))
}

pub fn update_permission_set(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;

    let mut ps = state.permission_sets.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Permission set not found: {arn}"),
        )
    })?;

    if let Some(d) = input["Description"].as_str() {
        ps.description = d.to_string();
    }
    if let Some(s) = input["SessionDuration"].as_str() {
        ps.session_duration = s.to_string();
    }
    if let Some(r) = input["RelayState"].as_str() {
        ps.relay_state = r.to_string();
    }

    Ok(json!({}))
}

pub fn attach_managed_policy(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;
    let policy_arn = input["ManagedPolicyArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "ManagedPolicyArn is required")
    })?;

    let mut ps = state.permission_sets.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Permission set not found: {arn}"),
        )
    })?;
    if !ps.managed_policies.iter().any(|p| p == policy_arn) {
        ps.managed_policies.push(policy_arn.to_string());
    }

    Ok(json!({}))
}

pub fn detach_managed_policy(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;
    let policy_arn = input["ManagedPolicyArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "ManagedPolicyArn is required")
    })?;

    if let Some(mut ps) = state.permission_sets.get_mut(arn) {
        ps.managed_policies.retain(|p| p != policy_arn);
    }

    Ok(json!({}))
}

pub fn list_managed_policies(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;

    let ps = state.permission_sets.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Permission set not found: {arn}"),
        )
    })?;

    let policies: Vec<Value> = ps
        .managed_policies
        .iter()
        .map(|p| {
            let name = p.rsplit('/').next().unwrap_or(p).to_string();
            json!({ "Arn": p, "Name": name })
        })
        .collect();

    Ok(json!({ "AttachedManagedPolicies": policies }))
}

pub fn put_inline_policy(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PermissionSetArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "PermissionSetArn is required")
    })?;
    let policy = input["InlinePolicy"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "InlinePolicy is required"))?;

    let mut ps = state.permission_sets.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Permission set not found: {arn}"),
        )
    })?;
    ps.inline_policy = policy.to_string();

    Ok(json!({}))
}
