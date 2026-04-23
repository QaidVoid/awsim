use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{delete_conflict, entity_already_exists, no_such_entity},
    ids::{new_instance_profile_id, normalize_path, now_iso8601},
    state::{IamState, InstanceProfile},
};

use super::{opt_str, require_str};

fn ip_to_value(ip: &InstanceProfile, state: &IamState) -> Value {
    let roles: Vec<Value> = ip
        .roles
        .iter()
        .filter_map(|rname| {
            state.roles.get(rname).map(|r| {
                json!({
                    "RoleName": r.role_name,
                    "RoleId": r.role_id,
                    "Arn": r.arn,
                    "Path": r.path,
                    "AssumeRolePolicyDocument": r.assume_role_policy_document,
                    "CreateDate": r.create_date,
                })
            })
        })
        .collect();

    json!({
        "InstanceProfileName": ip.instance_profile_name,
        "InstanceProfileId": ip.instance_profile_id,
        "Arn": ip.arn,
        "Path": ip.path,
        "CreateDate": ip.create_date,
        "Roles": { "member": roles },
    })
}

pub fn create_instance_profile(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let path = normalize_path(opt_str(input, "Path"));

    if state.instance_profiles.contains_key(name) {
        return Err(entity_already_exists("InstanceProfile", name));
    }

    let ip = InstanceProfile {
        instance_profile_name: name.to_string(),
        instance_profile_id: new_instance_profile_id(),
        arn: format!(
            "arn:aws:iam::{}:instance-profile{}{}",
            ctx.account_id, path, name
        ),
        path,
        create_date: now_iso8601(),
        roles: Vec::new(),
        tags: std::collections::HashMap::new(),
    };

    let result = ip_to_value(&ip, state);
    state.instance_profiles.insert(name.to_string(), ip);

    Ok(json!({ "InstanceProfile": result }))
}

pub fn delete_instance_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;

    {
        let ip = state
            .instance_profiles
            .get(name)
            .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

        if !ip.roles.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete instance profile {name}: profile has associated roles"
            )));
        }
    }

    state.instance_profiles.remove(name);
    Ok(json!({}))
}

pub fn get_instance_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let ip = state
        .instance_profiles
        .get(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;
    Ok(json!({ "InstanceProfile": ip_to_value(&ip, state) }))
}

pub fn add_role_to_instance_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let role_name = require_str(input, "RoleName")?;

    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }

    let mut ip = state
        .instance_profiles
        .get_mut(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

    if ip.roles.len() >= 1 {
        // AWS only allows one role per instance profile
        return Err(awsim_core::AwsError::conflict(
            "LimitExceeded",
            format!("Instance profile {name} already has a role associated"),
        ));
    }

    if !ip.roles.contains(&role_name.to_string()) {
        ip.roles.push(role_name.to_string());
    }

    Ok(json!({}))
}

pub fn remove_role_from_instance_profile(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let role_name = require_str(input, "RoleName")?;

    let mut ip = state
        .instance_profiles
        .get_mut(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

    let before = ip.roles.len();
    ip.roles.retain(|r| r != role_name);

    if ip.roles.len() == before {
        return Err(no_such_entity(
            "Role in InstanceProfile",
            &format!("{role_name} in {name}"),
        ));
    }

    Ok(json!({}))
}

/// ListInstanceProfiles — Return all instance profiles for the account.
pub fn list_instance_profiles(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let mut profiles: Vec<Value> = state
        .instance_profiles
        .iter()
        .filter(|e| e.value().path.starts_with(path_prefix))
        .map(|e| ip_to_value(e.value(), state))
        .collect();

    profiles.sort_by(|a, b| {
        a.get("InstanceProfileName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(
                b.get("InstanceProfileName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
    });

    Ok(json!({
        "InstanceProfiles": { "member": profiles },
        "IsTruncated": false
    }))
}

/// ListInstanceProfilesForRole — Return all instance profiles referencing the given role.
pub fn list_instance_profiles_for_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    // Verify the role exists.
    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }

    let mut profiles: Vec<Value> = state
        .instance_profiles
        .iter()
        .filter(|e| e.value().roles.iter().any(|r| r == role_name))
        .map(|e| ip_to_value(e.value(), state))
        .collect();

    profiles.sort_by(|a, b| {
        a.get("InstanceProfileName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(
                b.get("InstanceProfileName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
    });

    Ok(json!({
        "InstanceProfiles": { "member": profiles },
        "IsTruncated": false
    }))
}
