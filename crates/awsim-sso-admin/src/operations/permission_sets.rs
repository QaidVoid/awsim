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
    validate_permission_set_name(&name)?;

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

/// AWS documents `PermissionSet.Name` as matching `[\w+=,.@-]+`
/// (word-chars plus the punctuation listed) with length 1-32. The
/// character class is ASCII-only in AWS's regex flavour.
fn validate_permission_set_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 32 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "PermissionSet Name must be 1-32 characters; got {}.",
                name.len()
            ),
        ));
    }
    let ok =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-');
    if !name.chars().all(ok) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("PermissionSet Name '{name}' must match `[\\w+=,.@-]+`.",),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("sso", "us-east-1")
    }

    fn instance_arn() -> &'static str {
        "arn:aws:sso:::instance/ssoins-1234567890abcdef"
    }

    #[test]
    fn create_rejects_empty_name() {
        let state = SsoAdminState::default();
        let err = create_permission_set(
            &state,
            &json!({ "Name": "", "InstanceArn": instance_arn() }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_rejects_name_over_32_chars() {
        let state = SsoAdminState::default();
        let too_long = "a".repeat(33);
        let err = create_permission_set(
            &state,
            &json!({ "Name": too_long, "InstanceArn": instance_arn() }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_rejects_name_with_invalid_chars() {
        let state = SsoAdminState::default();
        for bad in [
            "has space",
            "tab\there",
            "slash/no",
            "colon:no",
            "fire\u{1f525}",
        ] {
            let err = create_permission_set(
                &state,
                &json!({ "Name": bad, "InstanceArn": instance_arn() }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad:?}");
        }
    }

    #[test]
    fn create_accepts_documented_charset_at_bounds() {
        let state = SsoAdminState::default();
        // Use every allowed punctuation char at least once and stay inside 32 chars.
        create_permission_set(
            &state,
            &json!({ "Name": "a_b+c=d,e.f@g-h", "InstanceArn": instance_arn() }),
            &ctx(),
        )
        .unwrap();
        // Exactly 32 chars: still ok.
        let max = "a".repeat(32);
        create_permission_set(
            &state,
            &json!({ "Name": max, "InstanceArn": instance_arn() }),
            &ctx(),
        )
        .unwrap();
    }
}
