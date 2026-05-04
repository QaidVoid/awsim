use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{delete_conflict, entity_already_exists, limit_exceeded, no_such_entity},
    ids::{new_group_id, normalize_path, now_iso8601},
    state::{Group, IamState},
};

use super::{opt_str, require_str};

/// AWS quota: maximum groups a single IAM user may belong to.
const MAX_GROUPS_PER_USER: usize = 10;

fn group_to_value(g: &Group) -> Value {
    json!({
        "GroupName": g.group_name,
        "GroupId": g.group_id,
        "Arn": g.arn,
        "Path": g.path,
        "CreateDate": g.create_date,
    })
}

pub fn create_group(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let path = normalize_path(opt_str(input, "Path"));

    if state.groups.contains_key(group_name) {
        return Err(entity_already_exists("Group", group_name));
    }

    let group_id = new_group_id();
    let arn = format!(
        "arn:aws:iam::{}:group{}{}",
        ctx.account_id, path, group_name
    );

    let group = Group {
        group_name: group_name.to_string(),
        group_id,
        arn,
        path,
        create_date: now_iso8601(),
        members: Vec::new(),
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
        tags: HashMap::new(),
    };

    let result = group_to_value(&group);
    state.groups.insert(group_name.to_string(), group);

    Ok(json!({ "Group": result }))
}

pub fn get_group(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;

    let group = state
        .groups
        .get(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    // Collect user details for members
    let users: Vec<Value> = group
        .members
        .iter()
        .filter_map(|uname| {
            state.users.get(uname).map(|u| {
                json!({
                    "UserName": u.user_name,
                    "UserId": u.user_id,
                    "Arn": u.arn,
                    "Path": u.path,
                    "CreateDate": u.create_date,
                })
            })
        })
        .collect();

    Ok(json!({
        "Group": group_to_value(&group),
        "Users": { "member": users },
        "IsTruncated": false,
    }))
}

pub fn delete_group(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;

    {
        let group = state
            .groups
            .get(group_name)
            .ok_or_else(|| no_such_entity("Group", group_name))?;

        if !group.members.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete group {group_name}: group has members"
            )));
        }
        if !group.attached_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete group {group_name}: group has attached policies"
            )));
        }
    }

    state.groups.remove(group_name);
    Ok(json!({}))
}

pub fn list_groups(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let groups: Vec<Value> = state
        .groups
        .iter()
        .filter(|g| g.path.starts_with(path_prefix))
        .map(|g| group_to_value(&g))
        .collect();

    Ok(json!({
        "Groups": { "member": groups },
        "IsTruncated": false,
    }))
}

pub fn add_user_to_group(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let user_name = require_str(input, "UserName")?;

    // Validate both exist
    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    // Check the per-user group cap before mutating either side. Skip the
    // count when the user is already in the target group — re-issuing the
    // call must remain idempotent even at the cap.
    {
        let user = state.users.get(user_name).expect("user just verified");
        if !user.groups.contains(&group_name.to_string())
            && user.groups.len() >= MAX_GROUPS_PER_USER
        {
            return Err(limit_exceeded(format!(
                "Cannot exceed quota for GroupsPerUser: {MAX_GROUPS_PER_USER}"
            )));
        }
    }

    {
        let mut group = state
            .groups
            .get_mut(group_name)
            .ok_or_else(|| no_such_entity("Group", group_name))?;

        if !group.members.contains(&user_name.to_string()) {
            group.members.push(user_name.to_string());
        }
    }

    // Add group to user's group list
    if let Some(mut user) = state.users.get_mut(user_name)
        && !user.groups.contains(&group_name.to_string())
    {
        user.groups.push(group_name.to_string());
    }

    Ok(json!({}))
}

pub fn remove_user_from_group(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let user_name = require_str(input, "UserName")?;

    {
        let mut group = state
            .groups
            .get_mut(group_name)
            .ok_or_else(|| no_such_entity("Group", group_name))?;

        let before = group.members.len();
        group.members.retain(|m| m != user_name);

        if group.members.len() == before {
            return Err(no_such_entity(
                "User in group",
                &format!("{user_name} in {group_name}"),
            ));
        }
    }

    // Remove group from user's group list
    if let Some(mut user) = state.users.get_mut(user_name) {
        user.groups.retain(|g| g != group_name);
    }

    Ok(json!({}))
}

// ── Inline policy read/delete ────────────────────────────────────────────────

pub fn get_group_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let group = state
        .groups
        .get(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    let doc = group
        .inline_policies
        .get(policy_name)
        .ok_or_else(|| no_such_entity("InlinePolicy", policy_name))?
        .clone();

    Ok(json!({
        "GroupName": group_name,
        "PolicyName": policy_name,
        "PolicyDocument": doc,
    }))
}

pub fn delete_group_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let mut group = state
        .groups
        .get_mut(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    if group.inline_policies.remove(policy_name).is_none() {
        return Err(no_such_entity("InlinePolicy", policy_name));
    }

    Ok(json!({}))
}

pub fn list_group_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;

    let group = state
        .groups
        .get(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    let names: Vec<Value> = group
        .inline_policies
        .keys()
        .map(|k| Value::String(k.clone()))
        .collect();

    Ok(json!({
        "PolicyNames": { "member": names },
        "IsTruncated": false,
    }))
}

pub fn update_group(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let new_group_name = opt_str(input, "NewGroupName");
    let new_path = opt_str(input, "NewPath");

    if !state.groups.contains_key(group_name) {
        return Err(no_such_entity("Group", group_name));
    }

    if let Some(new_name) = new_group_name
        && new_name != group_name
        && state.groups.contains_key(new_name)
    {
        return Err(entity_already_exists("Group", new_name));
    }

    if new_group_name.is_none() && new_path.is_none() {
        return Ok(json!({}));
    }

    let (_, mut group) = state.groups.remove(group_name).unwrap();
    if let Some(np) = new_path {
        group.path = normalize_path(Some(np));
    }
    let final_name = if let Some(nn) = new_group_name {
        group.group_name = nn.to_string();
        nn.to_string()
    } else {
        group_name.to_string()
    };
    state.groups.insert(final_name, group);
    Ok(json!({}))
}
