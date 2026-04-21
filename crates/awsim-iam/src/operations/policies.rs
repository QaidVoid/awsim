use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{delete_conflict, entity_already_exists, no_such_entity},
    ids::{new_policy_id, normalize_path, now_iso8601},
    state::{IamState, Policy},
};

use super::{opt_str, require_str};

fn policy_to_value(p: &Policy) -> Value {
    let mut v = json!({
        "PolicyName": p.policy_name,
        "PolicyId": p.policy_id,
        "Arn": p.arn,
        "Path": p.path,
        "PolicyDocument": p.policy_document,
        "AttachmentCount": p.attachment_count,
        "CreateDate": p.create_date,
        "UpdateDate": p.update_date,
    });
    if let Some(desc) = &p.description {
        v["Description"] = Value::String(desc.clone());
    }
    v
}

fn build_policy_arn(account_id: &str, path: &str, policy_name: &str) -> String {
    format!("arn:aws:iam::{}:policy{}{}", account_id, path, policy_name)
}

// ── Managed policy CRUD ─────────────────────────────────────────────────────

pub fn create_policy(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policy_name = require_str(input, "PolicyName")?;
    let policy_document = require_str(input, "PolicyDocument")?;
    let path = normalize_path(opt_str(input, "Path"));
    let description = opt_str(input, "Description").map(|s| s.to_string());

    let arn = build_policy_arn(&ctx.account_id, &path, policy_name);

    if state.policies.contains_key(&arn) {
        return Err(entity_already_exists("Policy", policy_name));
    }

    let now = now_iso8601();
    let policy = Policy {
        policy_name: policy_name.to_string(),
        policy_id: new_policy_id(),
        arn: arn.clone(),
        path,
        description,
        policy_document: policy_document.to_string(),
        create_date: now.clone(),
        update_date: now,
        attachment_count: 0,
    };

    let result = policy_to_value(&policy);
    state.policies.insert(arn, policy);

    Ok(json!({ "Policy": result }))
}

pub fn get_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;
    let policy = state
        .policies
        .get(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;
    Ok(json!({ "Policy": policy_to_value(&policy) }))
}

pub fn delete_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;

    {
        let policy = state
            .policies
            .get(arn)
            .ok_or_else(|| no_such_entity("Policy", arn))?;

        if policy.attachment_count > 0 {
            return Err(delete_conflict(format!(
                "Cannot delete policy {arn}: policy is attached to {} entities",
                policy.attachment_count
            )));
        }
    }

    state.policies.remove(arn);
    Ok(json!({}))
}

pub fn list_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");
    // Scope: "All", "Local", "AWS" — we only have local policies.
    let _scope = opt_str(input, "Scope").unwrap_or("Local");

    let policies: Vec<Value> = state
        .policies
        .iter()
        .filter(|p| p.path.starts_with(path_prefix))
        .map(|p| policy_to_value(&p))
        .collect();

    Ok(json!({
        "Policies": { "member": policies },
        "IsTruncated": false,
    }))
}

// ── Attach / detach managed policies ────────────────────────────────────────

pub fn attach_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    if !state.policies.contains_key(policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut user = state
            .users
            .get_mut(user_name)
            .ok_or_else(|| no_such_entity("User", user_name))?;

        if !user.attached_policies.contains(&policy_arn.to_string()) {
            user.attached_policies.push(policy_arn.to_string());
            if let Some(mut p) = state.policies.get_mut(policy_arn) {
                p.attachment_count += 1;
            }
        }
    }

    Ok(json!({}))
}

pub fn detach_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let before = user.attached_policies.len();
    user.attached_policies.retain(|a| a != policy_arn);

    if user.attached_policies.len() < before {
        if let Some(mut p) = state.policies.get_mut(policy_arn) {
            p.attachment_count = p.attachment_count.saturating_sub(1);
        }
    } else {
        return Err(no_such_entity(
            "PolicyAttachment",
            &format!("{policy_arn} on user {user_name}"),
        ));
    }

    Ok(json!({}))
}

pub fn attach_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    if !state.policies.contains_key(policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut role = state
            .roles
            .get_mut(role_name)
            .ok_or_else(|| no_such_entity("Role", role_name))?;

        if !role.attached_policies.contains(&policy_arn.to_string()) {
            role.attached_policies.push(policy_arn.to_string());
            if let Some(mut p) = state.policies.get_mut(policy_arn) {
                p.attachment_count += 1;
            }
        }
    }

    Ok(json!({}))
}

pub fn detach_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let before = role.attached_policies.len();
    role.attached_policies.retain(|a| a != policy_arn);

    if role.attached_policies.len() < before {
        if let Some(mut p) = state.policies.get_mut(policy_arn) {
            p.attachment_count = p.attachment_count.saturating_sub(1);
        }
    } else {
        return Err(no_such_entity(
            "PolicyAttachment",
            &format!("{policy_arn} on role {role_name}"),
        ));
    }

    Ok(json!({}))
}

pub fn attach_group_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    if !state.policies.contains_key(policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut group = state
            .groups
            .get_mut(group_name)
            .ok_or_else(|| no_such_entity("Group", group_name))?;

        if !group.attached_policies.contains(&policy_arn.to_string()) {
            group.attached_policies.push(policy_arn.to_string());
            if let Some(mut p) = state.policies.get_mut(policy_arn) {
                p.attachment_count += 1;
            }
        }
    }

    Ok(json!({}))
}

pub fn detach_group_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    let mut group = state
        .groups
        .get_mut(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    let before = group.attached_policies.len();
    group.attached_policies.retain(|a| a != policy_arn);

    if group.attached_policies.len() < before {
        if let Some(mut p) = state.policies.get_mut(policy_arn) {
            p.attachment_count = p.attachment_count.saturating_sub(1);
        }
    } else {
        return Err(no_such_entity(
            "PolicyAttachment",
            &format!("{policy_arn} on group {group_name}"),
        ));
    }

    Ok(json!({}))
}

// ── Inline policies ──────────────────────────────────────────────────────────

pub fn put_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_name = require_str(input, "PolicyName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    user.inline_policies
        .insert(policy_name.to_string(), policy_document.to_string());
    Ok(json!({}))
}

pub fn put_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_name = require_str(input, "PolicyName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.inline_policies
        .insert(policy_name.to_string(), policy_document.to_string());
    Ok(json!({}))
}

pub fn put_group_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let policy_name = require_str(input, "PolicyName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    let mut group = state
        .groups
        .get_mut(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    group
        .inline_policies
        .insert(policy_name.to_string(), policy_document.to_string());
    Ok(json!({}))
}
