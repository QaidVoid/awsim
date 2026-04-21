use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{delete_conflict, entity_already_exists, no_such_entity},
    ids::{new_role_id, normalize_path, now_iso8601},
    state::{IamState, Role},
};

use super::{opt_str, require_str};

fn role_to_value(r: &Role) -> Value {
    let mut v = json!({
        "RoleName": r.role_name,
        "RoleId": r.role_id,
        "Arn": r.arn,
        "Path": r.path,
        "AssumeRolePolicyDocument": r.assume_role_policy_document,
        "CreateDate": r.create_date,
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

    if state.roles.contains_key(role_name) {
        return Err(entity_already_exists("Role", role_name));
    }

    let role_id = new_role_id();
    let arn = format!(
        "arn:aws:iam::{}:role{}{}",
        ctx.account_id, path, role_name
    );

    let role = Role {
        role_name: role_name.to_string(),
        role_id,
        arn,
        path,
        assume_role_policy_document: assume_role_policy.to_string(),
        description,
        create_date: now_iso8601(),
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
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
    Ok(json!({ "Role": role_to_value(&role) }))
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

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.assume_role_policy_document = policy_document.to_string();
    Ok(json!({}))
}
