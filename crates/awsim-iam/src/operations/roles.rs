use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{
        delete_conflict, entity_already_exists, malformed_policy_document, no_such_entity,
        validation_error,
    },
    ids::{new_role_id, normalize_path, now_iso8601},
    state::{IamState, Role},
};

const MIN_MAX_SESSION_DURATION: u32 = 3600;
const MAX_MAX_SESSION_DURATION: u32 = 43_200;

fn validate_policy_document(doc: &str) -> Result<(), AwsError> {
    awsim_iam_policy::parse(doc)
        .map(|_| ())
        .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))
}

/// AWS rejects MaxSessionDuration values outside [3600, 43200] seconds
/// with ValidationError ("1 validation error detected"). Mirror that.
fn validate_max_session_duration(value: u32) -> Result<(), AwsError> {
    if !(MIN_MAX_SESSION_DURATION..=MAX_MAX_SESSION_DURATION).contains(&value) {
        return Err(validation_error(format!(
            "1 validation error detected: Value '{value}' at 'maxSessionDuration' \
             failed to satisfy constraint: Member must have value less than or equal to \
             {MAX_MAX_SESSION_DURATION} and greater than or equal to {MIN_MAX_SESSION_DURATION}"
        )));
    }
    Ok(())
}

use super::{opt_str, require_str};

fn role_to_value(r: &Role) -> Value {
    let mut v = json!({
        "RoleName": r.role_name,
        "RoleId": r.role_id,
        "Arn": r.arn,
        "Path": r.path,
        "AssumeRolePolicyDocument": r.assume_role_policy_document,
        "CreateDate": r.create_date,
        "MaxSessionDuration": r.max_session_duration,
    });
    if let Some(desc) = &r.description {
        v["Description"] = Value::String(desc.clone());
    }
    v
}

pub fn create_role(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let assume_role_policy = require_str(input, "AssumeRolePolicyDocument")?;
    let path = normalize_path(opt_str(input, "Path"));
    let description = opt_str(input, "Description").map(|s| s.to_string());

    validate_policy_document(assume_role_policy)?;

    if state.roles.contains_key(role_name) {
        return Err(entity_already_exists("Role", role_name));
    }

    let role_id = new_role_id();
    let arn = format!("arn:aws:iam::{}:role{}{}", ctx.account_id, path, role_name);

    let max_session_duration = input
        .get("MaxSessionDuration")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(3600);
    validate_max_session_duration(max_session_duration)?;

    let role = Role {
        role_name: role_name.to_string(),
        role_id,
        arn,
        path,
        assume_role_policy_document: assume_role_policy.to_string(),
        description,
        create_date: now_iso8601(),
        max_session_duration,
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
        tags: HashMap::new(),
    };

    let result = role_to_value(&role);
    state.roles.insert(role_name.to_string(), role);

    Ok(json!({ "Role": result }))
}

pub fn get_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;
    let mut v = json!({ "Role": role_to_value(&role) });
    if let Some(boundary) = state.role_permissions_boundaries.get(&role.role_name) {
        v["Role"]["PermissionsBoundary"] = json!({
            "PermissionsBoundaryType": "Policy",
            "PermissionsBoundaryArn": boundary.value().clone(),
        });
    }
    Ok(v)
}

pub fn delete_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    {
        let role = state
            .roles
            .get(role_name)
            .ok_or_else(|| no_such_entity("Role", role_name))?;

        if !role.attached_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role has attached policies"
            )));
        }
        if !role.inline_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role has inline policies"
            )));
        }
    }

    // Ensure no instance profile references this role
    for ip in state.instance_profiles.iter() {
        if ip.roles.contains(&role_name.to_string()) {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role is associated with instance profile {}",
                ip.instance_profile_name
            )));
        }
    }

    state.roles.remove(role_name);
    Ok(json!({}))
}

pub fn list_roles(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let roles: Vec<Value> = state
        .roles
        .iter()
        .filter(|r| r.path.starts_with(path_prefix))
        .map(|r| role_to_value(&r))
        .collect();

    Ok(json!({
        "Roles": { "member": roles },
        "IsTruncated": false,
    }))
}

pub fn update_assume_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    validate_policy_document(policy_document)?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.assume_role_policy_document = policy_document.to_string();
    Ok(json!({}))
}

pub fn update_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    if let Some(desc) = opt_str(input, "Description") {
        role.description = Some(desc.to_string());
    }
    if let Some(dur) = input.get("MaxSessionDuration").and_then(|v| v.as_u64()) {
        let dur = dur as u32;
        validate_max_session_duration(dur)?;
        role.max_session_duration = dur;
    }

    Ok(json!({ "Role": role_to_value(&role) }))
}

pub fn update_role_description(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let description = require_str(input, "Description")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.description = Some(description.to_string());

    Ok(json!({ "Role": role_to_value(&role) }))
}

pub fn put_role_permissions_boundary(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let boundary_arn = require_str(input, "PermissionsBoundary")?;

    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }
    state
        .role_permissions_boundaries
        .insert(role_name.to_string(), boundary_arn.to_string());
    Ok(json!({}))
}

pub fn delete_role_permissions_boundary(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }
    state.role_permissions_boundaries.remove(role_name);
    Ok(json!({}))
}

// ── Inline policy read/delete ────────────────────────────────────────────────

pub fn get_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let doc = role
        .inline_policies
        .get(policy_name)
        .ok_or_else(|| no_such_entity("InlinePolicy", policy_name))?
        .clone();

    Ok(json!({
        "RoleName": role_name,
        "PolicyName": policy_name,
        "PolicyDocument": doc,
    }))
}

pub fn delete_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    if role.inline_policies.remove(policy_name).is_none() {
        return Err(no_such_entity("InlinePolicy", policy_name));
    }

    Ok(json!({}))
}

pub fn list_role_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let names: Vec<Value> = role
        .inline_policies
        .keys()
        .map(|k| Value::String(k.clone()))
        .collect();

    Ok(json!({
        "PolicyNames": { "member": names },
        "IsTruncated": false,
    }))
}
