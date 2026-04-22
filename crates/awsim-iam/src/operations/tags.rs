use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::no_such_entity,
    state::IamState,
};

use super::require_str;

pub fn parse_tags(input: &Value) -> HashMap<String, String> {
    let mut tags = HashMap::new();
    if let Some(members) = input
        .get("Tags")
        .and_then(|t| t.get("member"))
        .and_then(|m| m.as_array())
    {
        for member in members {
            if let (Some(k), Some(v)) = (
                member.get("Key").and_then(|k| k.as_str()),
                member.get("Value").and_then(|v| v.as_str()),
            ) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }
    tags
}

pub fn parse_tag_keys(input: &Value) -> Vec<String> {
    input
        .get("TagKeys")
        .and_then(|t| t.get("member"))
        .and_then(|m| m.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn tags_to_value(tags: &HashMap<String, String>) -> Value {
    let members: Vec<Value> = tags
        .iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    json!({ "member": members })
}

// ── User Tags ────────────────────────────────────────────────────────────────

pub fn tag_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let new_tags = parse_tags(input);

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    for (k, v) in new_tags {
        user.tags.insert(k, v);
    }

    Ok(json!({}))
}

pub fn untag_user(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let keys = parse_tag_keys(input);

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    for k in &keys {
        user.tags.remove(k);
    }

    Ok(json!({}))
}

pub fn list_user_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    Ok(json!({
        "Tags": tags_to_value(&user.tags),
        "IsTruncated": false,
    }))
}

// ── Role Tags ────────────────────────────────────────────────────────────────

pub fn tag_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let new_tags = parse_tags(input);

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    for (k, v) in new_tags {
        role.tags.insert(k, v);
    }

    Ok(json!({}))
}

pub fn untag_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let keys = parse_tag_keys(input);

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    for k in &keys {
        role.tags.remove(k);
    }

    Ok(json!({}))
}

pub fn list_role_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    Ok(json!({
        "Tags": tags_to_value(&role.tags),
        "IsTruncated": false,
    }))
}

// ── Instance Profile Tags ────────────────────────────────────────────────────

pub fn tag_instance_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let new_tags = parse_tags(input);

    let mut ip = state
        .instance_profiles
        .get_mut(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

    for (k, v) in new_tags {
        ip.tags.insert(k, v);
    }

    Ok(json!({}))
}

pub fn untag_instance_profile(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let keys = parse_tag_keys(input);

    let mut ip = state
        .instance_profiles
        .get_mut(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

    for k in &keys {
        ip.tags.remove(k);
    }

    Ok(json!({}))
}

pub fn list_instance_profile_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "InstanceProfileName")?;
    let ip = state
        .instance_profiles
        .get(name)
        .ok_or_else(|| no_such_entity("InstanceProfile", name))?;

    Ok(json!({
        "Tags": tags_to_value(&ip.tags),
        "IsTruncated": false,
    }))
}
