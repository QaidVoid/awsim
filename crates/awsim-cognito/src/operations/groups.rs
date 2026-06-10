use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::operations::users::user_to_value;
use crate::state::{CognitoGroup, CognitoState};

/// Warn if a role ARN doesn't match the expected format.
/// Does NOT call IAM — just a format check to catch obvious mistakes.
fn warn_if_invalid_role_arn(arn: &str) {
    // Expected format: arn:aws:iam::<account-id>:role/<role-name>
    if !arn.starts_with("arn:") || !arn.contains(":iam:") || !arn.contains(":role/") {
        warn!(role_arn = %arn, "CreateGroup/UpdateGroup: RoleArn does not appear to be a valid IAM role ARN");
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn group_to_value(g: &CognitoGroup) -> Value {
    json!({
        "GroupName": g.group_name,
        "UserPoolId": g.user_pool_id,
        "Description": g.description,
        "RoleArn": g.role_arn,
        "Precedence": g.precedence,
        "CreationDate": g.created_date,
        "LastModifiedDate": g.created_date
    })
}

// ---------------------------------------------------------------------------
// CreateGroup
// ---------------------------------------------------------------------------

pub fn create_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;
    let description = input["Description"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let role_arn = input["RoleArn"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let precedence = input["Precedence"].as_u64().map(|v| v as u32);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.groups.contains_key(group_name) {
        return Err(AwsError::bad_request(
            "GroupExistsException",
            format!("Group already exists: {group_name}"),
        ));
    }

    if let Some(ref arn) = role_arn {
        warn_if_invalid_role_arn(arn);
    }

    let now = now_epoch();
    let group = CognitoGroup {
        group_name: group_name.to_string(),
        description: description.clone(),
        role_arn: role_arn.clone(),
        precedence,
        user_pool_id: pool_id.to_string(),
        created_date: now,
    };

    pool.groups.insert(group_name.to_string(), group);
    info!(group_name = %group_name, pool_id = %pool_id, "Cognito: created group");

    Ok(json!({
        "Group": {
            "GroupName": group_name,
            "UserPoolId": pool_id,
            "Description": description,
            "RoleArn": role_arn,
            "Precedence": precedence,
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// GetGroup
// ---------------------------------------------------------------------------

pub fn get_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let group = pool.groups.get(group_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        )
    })?;

    Ok(json!({ "Group": group_to_value(group) }))
}

// ---------------------------------------------------------------------------
// UpdateGroup
// ---------------------------------------------------------------------------

pub fn update_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let group = pool.groups.get_mut(group_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        )
    })?;

    // Empty string clears, present value sets, absent leaves
    // unchanged. AWS UpdateGroup itself uses "absent = leave alone";
    // we extend that with empty-string-as-clear so the UI's edit
    // dialog can blank a field without a separate Delete affordance.
    if let Some(desc) = input["Description"].as_str() {
        group.description = (!desc.is_empty()).then(|| desc.to_string());
    }
    if let Some(arn) = input["RoleArn"].as_str() {
        if arn.is_empty() {
            group.role_arn = None;
        } else {
            warn_if_invalid_role_arn(arn);
            group.role_arn = Some(arn.to_string());
        }
    }
    if let Some(p) = input["Precedence"].as_u64() {
        group.precedence = Some(p as u32);
    }

    let group_value = group_to_value(group);
    info!(group_name = %group_name, pool_id = %pool_id, "Cognito: updated group");

    Ok(json!({ "Group": group_value }))
}

// ---------------------------------------------------------------------------
// DeleteGroup
// ---------------------------------------------------------------------------

pub fn delete_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.groups.remove(group_name).is_none() {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        ));
    }

    // Remove group from all users
    for user in pool.users.values_mut() {
        user.groups.retain(|g| g != group_name);
    }

    info!(group_name = %group_name, pool_id = %pool_id, "Cognito: deleted group");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListGroups
// ---------------------------------------------------------------------------

pub fn list_groups(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let limit = cap_max_results(input["Limit"].as_i64(), 60, 60);

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let mut groups: Vec<CognitoGroup> = pool.groups.values().cloned().collect();
    groups.sort_by(|a, b| a.group_name.cmp(&b.group_name));

    let token = input["NextToken"].as_str();
    let page = paginate(groups, limit, token, |g| g.group_name.clone())?;
    let group_values: Vec<Value> = page.items.iter().map(group_to_value).collect();

    let mut resp = json!({ "Groups": group_values });
    if let Some(next) = page.next_token {
        resp["NextToken"] = json!(next);
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// AdminAddUserToGroup
// ---------------------------------------------------------------------------

pub fn admin_add_user_to_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if !pool.groups.contains_key(group_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        ));
    }

    let username = super::users::resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    if !user.groups.contains(&group_name.to_string()) {
        user.groups.push(group_name.to_string());
    }

    info!(username = %username, group_name = %group_name, pool_id = %pool_id, "Cognito: added user to group");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminRemoveUserFromGroup
// ---------------------------------------------------------------------------

pub fn admin_remove_user_from_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let username = super::users::resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.groups.retain(|g| g != group_name);

    info!(username = %username, group_name = %group_name, pool_id = %pool_id, "Cognito: removed user from group");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminListGroupsForUser
// ---------------------------------------------------------------------------

pub fn admin_list_groups_for_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let username = super::users::resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let user = pool.users.get(&username).ok_or_else(|| {
        AwsError::service_not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let groups: Vec<Value> = user
        .groups
        .iter()
        .filter_map(|gname| pool.groups.get(gname))
        .map(group_to_value)
        .collect();

    Ok(json!({ "Groups": groups }))
}

// ---------------------------------------------------------------------------
// ListUsersInGroup
// ---------------------------------------------------------------------------

pub fn list_users_in_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let group_name = input["GroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "GroupName is required")
    })?;

    let limit = input["Limit"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if !pool.groups.contains_key(group_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        ));
    }

    let users: Vec<Value> = pool
        .users
        .values()
        .filter(|u| u.groups.contains(&group_name.to_string()))
        .take(limit)
        .map(user_to_value)
        .collect();

    Ok(json!({ "Users": users }))
}
