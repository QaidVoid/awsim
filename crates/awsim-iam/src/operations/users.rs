use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{delete_conflict, entity_already_exists, no_such_entity},
    ids::{new_access_key_id, new_secret_access_key, new_user_id, normalize_path, now_iso8601},
    state::{AccessKey, IamState, LoginProfile, User},
};

use super::{opt_str, require_str};

fn user_to_value(u: &User) -> Value {
    json!({
        "UserName": u.user_name,
        "UserId": u.user_id,
        "Arn": u.arn,
        "Path": u.path,
        "CreateDate": u.create_date,
    })
}

pub fn create_user(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let path = normalize_path(opt_str(input, "Path"));

    if state.users.contains_key(user_name) {
        return Err(entity_already_exists("User", user_name));
    }

    let user_id = new_user_id();
    let arn = format!("arn:aws:iam::{}:user{}{}", ctx.account_id, path, user_name);

    let user = User {
        user_name: user_name.to_string(),
        user_id,
        arn: arn.clone(),
        path,
        create_date: now_iso8601(),
        access_keys: Vec::new(),
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
        groups: Vec::new(),
        tags: HashMap::new(),
        mfa_devices: Vec::new(),
        ssh_public_keys: Vec::new(),
        password_last_used: None,
    };

    let result = user_to_value(&user);
    state.users.insert(user_name.to_string(), user);

    Ok(json!({ "User": result }))
}

pub fn get_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;
    Ok(json!({ "User": user_to_value(&user) }))
}

pub fn delete_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    {
        let user = state
            .users
            .get(user_name)
            .ok_or_else(|| no_such_entity("User", user_name))?;

        if !user.access_keys.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete user {user_name}: user has access keys"
            )));
        }
        if !user.attached_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete user {user_name}: user has attached policies"
            )));
        }
        if !user.groups.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete user {user_name}: user is a member of groups"
            )));
        }
    }

    state.users.remove(user_name);
    Ok(json!({}))
}

pub fn list_users(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let users: Vec<Value> = state
        .users
        .iter()
        .filter(|u| u.path.starts_with(path_prefix))
        .map(|u| user_to_value(&u))
        .collect();

    Ok(json!({
        "Users": { "member": users },
        "IsTruncated": false,
    }))
}

pub fn update_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let new_user_name = opt_str(input, "NewUserName");
    let new_path = opt_str(input, "NewPath");

    // Validate target exists
    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    // Check new name isn't already taken
    if let Some(new_name) = new_user_name
        && new_name != user_name
        && state.users.contains_key(new_name)
    {
        return Err(entity_already_exists("User", new_name));
    }

    if new_user_name.is_none() && new_path.is_none() {
        // Nothing to do
        return Ok(json!({}));
    }

    let (_, mut user) = state.users.remove(user_name).unwrap();

    if let Some(np) = new_path {
        user.path = normalize_path(Some(np));
    }

    let final_name = if let Some(nn) = new_user_name {
        user.user_name = nn.to_string();
        nn.to_string()
    } else {
        user_name.to_string()
    };

    state.users.insert(final_name, user);
    Ok(json!({}))
}

pub fn create_access_key(
    state: &IamState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let key = AccessKey {
        access_key_id: new_access_key_id(),
        secret_access_key: new_secret_access_key(),
        status: "Active".to_string(),
        create_date: now_iso8601(),
    };

    let result = json!({
        "UserName": user_name,
        "AccessKeyId": key.access_key_id,
        "SecretAccessKey": key.secret_access_key,
        "Status": key.status,
        "CreateDate": key.create_date,
    });

    user.access_keys.push(key);
    Ok(json!({ "AccessKey": result }))
}

pub fn delete_access_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let key_id = require_str(input, "AccessKeyId")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let before = user.access_keys.len();
    user.access_keys.retain(|k| k.access_key_id != key_id);

    if user.access_keys.len() == before {
        return Err(no_such_entity("AccessKey", key_id));
    }

    Ok(json!({}))
}

pub fn list_access_keys(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let keys: Vec<Value> = user
        .access_keys
        .iter()
        .map(|k| {
            json!({
                "UserName": user_name,
                "AccessKeyId": k.access_key_id,
                "Status": k.status,
                "CreateDate": k.create_date,
            })
        })
        .collect();

    Ok(json!({
        "AccessKeyMetadata": { "member": keys },
        "IsTruncated": false,
    }))
}

// ── Inline policy read/delete ────────────────────────────────────────────────

pub fn get_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let doc = user
        .inline_policies
        .get(policy_name)
        .ok_or_else(|| no_such_entity("InlinePolicy", policy_name))?
        .clone();

    Ok(json!({
        "UserName": user_name,
        "PolicyName": policy_name,
        "PolicyDocument": doc,
    }))
}

pub fn delete_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    if user.inline_policies.remove(policy_name).is_none() {
        return Err(no_such_entity("InlinePolicy", policy_name));
    }

    Ok(json!({}))
}

pub fn list_user_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let names: Vec<Value> = user
        .inline_policies
        .keys()
        .map(|k| Value::String(k.clone()))
        .collect();

    Ok(json!({
        "PolicyNames": { "member": names },
        "IsTruncated": false,
    }))
}

// ── ListGroupsForUser ────────────────────────────────────────────────────────

pub fn list_groups_for_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    let groups: Vec<Value> = state
        .groups
        .iter()
        .filter(|g| g.members.contains(&user_name.to_string()))
        .map(|g| {
            json!({
                "GroupName": g.group_name,
                "GroupId": g.group_id,
                "Arn": g.arn,
                "Path": g.path,
                "CreateDate": g.create_date,
            })
        })
        .collect();

    Ok(json!({
        "Groups": { "member": groups },
        "IsTruncated": false,
    }))
}

// ── Login Profile ─────────────────────────────────────────────────────────────

pub fn create_login_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let password_reset_required = input
        .get("PasswordResetRequired")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Verify user exists.
    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    if state.login_profiles.contains_key(user_name) {
        return Err(entity_already_exists("LoginProfile", user_name));
    }

    let profile = LoginProfile {
        user_name: user_name.to_string(),
        create_date: now_iso8601(),
        password_reset_required,
    };

    let result = json!({
        "LoginProfile": {
            "UserName": profile.user_name,
            "CreateDate": profile.create_date,
            "PasswordResetRequired": profile.password_reset_required,
        }
    });

    state.login_profiles.insert(user_name.to_string(), profile);

    Ok(result)
}

pub fn get_login_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    let profile = state
        .login_profiles
        .get(user_name)
        .ok_or_else(|| no_such_entity("LoginProfile", user_name))?;

    Ok(json!({
        "LoginProfile": {
            "UserName": profile.user_name,
            "CreateDate": profile.create_date,
            "PasswordResetRequired": profile.password_reset_required,
        }
    }))
}

pub fn update_login_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    let mut profile = state
        .login_profiles
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("LoginProfile", user_name))?;

    if let Some(reset) = input.get("PasswordResetRequired").and_then(|v| v.as_bool()) {
        profile.password_reset_required = reset;
    }
    // Password itself is not stored (emulator doesn't validate passwords).

    Ok(json!({}))
}

pub fn get_access_key_last_used(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let key_id = require_str(input, "AccessKeyId")?;

    let mut user_name = String::new();
    for entry in state.users.iter() {
        if entry
            .value()
            .access_keys
            .iter()
            .any(|k| k.access_key_id == key_id)
        {
            user_name = entry.value().user_name.clone();
            break;
        }
    }
    if user_name.is_empty() {
        return Err(no_such_entity("AccessKey", key_id));
    }

    let last_used = state
        .access_key_last_used
        .get(key_id)
        .map(|e| e.value().clone())
        .unwrap_or_default();

    let mut last_used_value = json!({
        "ServiceName": if last_used.service_name.is_empty() { "N/A".to_string() } else { last_used.service_name.clone() },
        "Region": if last_used.region.is_empty() { "N/A".to_string() } else { last_used.region.clone() },
    });
    if let Some(d) = last_used.last_used_date {
        last_used_value["LastUsedDate"] = Value::String(d);
    }

    Ok(json!({
        "UserName": user_name,
        "AccessKeyLastUsed": last_used_value,
    }))
}

pub fn update_access_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let key_id = require_str(input, "AccessKeyId")?;
    let status = require_str(input, "Status")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let key = user
        .access_keys
        .iter_mut()
        .find(|k| k.access_key_id == key_id)
        .ok_or_else(|| no_such_entity("AccessKey", key_id))?;
    key.status = status.to_string();
    Ok(json!({}))
}

pub fn change_password(_state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let _old = require_str(input, "OldPassword")?;
    let _new = require_str(input, "NewPassword")?;
    Ok(json!({}))
}

pub fn put_user_permissions_boundary(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let boundary_arn = require_str(input, "PermissionsBoundary")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }
    state
        .user_permissions_boundaries
        .insert(user_name.to_string(), boundary_arn.to_string());
    Ok(json!({}))
}

pub fn delete_user_permissions_boundary(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }
    state.user_permissions_boundaries.remove(user_name);
    Ok(json!({}))
}

pub fn delete_login_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    state
        .login_profiles
        .remove(user_name)
        .ok_or_else(|| no_such_entity("LoginProfile", user_name))?;

    Ok(json!({}))
}
