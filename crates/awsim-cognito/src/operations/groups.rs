use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{CognitoGroup, CognitoState};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateGroup
// ---------------------------------------------------------------------------

pub fn create_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let group_name = input["GroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "GroupName is required"))?;
    let description = input["Description"].as_str().map(String::from);
    let role_arn = input["RoleArn"].as_str().map(String::from);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.groups.contains_key(group_name) {
        return Err(AwsError::conflict(
            "GroupExistsException",
            format!("Group already exists: {group_name}"),
        ));
    }

    let now = now_epoch();
    let group = CognitoGroup {
        group_name: group_name.to_string(),
        description: description.clone(),
        role_arn: role_arn.clone(),
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
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// AdminAddUserToGroup
// ---------------------------------------------------------------------------

pub fn admin_add_user_to_group(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let group_name = input["GroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "GroupName is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if !pool.groups.contains_key(group_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Group not found: {group_name}"),
        ));
    }

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
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
// AdminListGroupsForUser
// ---------------------------------------------------------------------------

pub fn admin_list_groups_for_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let groups: Vec<Value> = user
        .groups
        .iter()
        .filter_map(|gname| pool.groups.get(gname))
        .map(|g| {
            json!({
                "GroupName": g.group_name,
                "UserPoolId": g.user_pool_id,
                "Description": g.description,
                "RoleArn": g.role_arn,
                "CreationDate": g.created_date,
                "LastModifiedDate": g.created_date
            })
        })
        .collect();

    Ok(json!({ "Groups": groups }))
}
