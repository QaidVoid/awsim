use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    error::{entity_already_exists, no_such_entity},
    state::{AccountPasswordPolicy, IamState},
};

use super::{opt_str, require_str};

fn opt_bool(input: &Value, key: &str) -> Option<bool> {
    input.get(key).and_then(|v| v.as_bool())
}

fn opt_u32(input: &Value, key: &str) -> Option<u32> {
    input.get(key).and_then(|v| v.as_u64()).map(|n| n as u32)
}

fn password_policy_to_value(p: &AccountPasswordPolicy) -> Value {
    json!({
        "MinimumPasswordLength": p.minimum_password_length,
        "RequireSymbols": p.require_symbols,
        "RequireNumbers": p.require_numbers,
        "RequireUppercaseCharacters": p.require_uppercase_characters,
        "RequireLowercaseCharacters": p.require_lowercase_characters,
        "AllowUsersToChangePassword": p.allow_users_to_change_password,
        "MaxPasswordAge": p.max_password_age,
        "PasswordReusePrevention": p.password_reuse_prevention,
        "HardExpiry": p.hard_expiry,
        "ExpirePasswords": p.max_password_age > 0,
    })
}

// ── Account Aliases ──────────────────────────────────────────────────────────

pub fn create_account_alias(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let alias = require_str(input, "AccountAlias")?;

    let mut aliases = state.account_aliases.lock().unwrap();
    if aliases.contains(&alias.to_string()) {
        return Err(entity_already_exists("AccountAlias", alias));
    }
    aliases.push(alias.to_string());

    Ok(json!({}))
}

pub fn delete_account_alias(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let alias = require_str(input, "AccountAlias")?;

    let mut aliases = state.account_aliases.lock().unwrap();
    let before = aliases.len();
    aliases.retain(|a| a != alias);

    if aliases.len() == before {
        return Err(no_such_entity("AccountAlias", alias));
    }

    Ok(json!({}))
}

pub fn list_account_aliases(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let aliases = state.account_aliases.lock().unwrap();
    let members: Vec<Value> = aliases.iter().map(|a| Value::String(a.clone())).collect();

    Ok(json!({
        "AccountAliases": { "member": members },
        "IsTruncated": false,
    }))
}

// ── Password Policy ──────────────────────────────────────────────────────────

pub fn get_account_password_policy(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let guard = state.account_password_policy.lock().unwrap();
    let policy = guard.as_ref().cloned().unwrap_or_default();
    Ok(json!({ "PasswordPolicy": password_policy_to_value(&policy) }))
}

pub fn update_account_password_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let mut guard = state.account_password_policy.lock().unwrap();
    let policy = guard.get_or_insert_with(AccountPasswordPolicy::default);

    if let Some(v) = opt_u32(input, "MinimumPasswordLength") {
        policy.minimum_password_length = v;
    }
    if let Some(v) = opt_bool(input, "RequireSymbols") {
        policy.require_symbols = v;
    }
    if let Some(v) = opt_bool(input, "RequireNumbers") {
        policy.require_numbers = v;
    }
    if let Some(v) = opt_bool(input, "RequireUppercaseCharacters") {
        policy.require_uppercase_characters = v;
    }
    if let Some(v) = opt_bool(input, "RequireLowercaseCharacters") {
        policy.require_lowercase_characters = v;
    }
    if let Some(v) = opt_bool(input, "AllowUsersToChangePassword") {
        policy.allow_users_to_change_password = v;
    }
    if let Some(v) = opt_u32(input, "MaxPasswordAge") {
        policy.max_password_age = v;
    }
    if let Some(v) = opt_u32(input, "PasswordReusePrevention") {
        policy.password_reuse_prevention = v;
    }
    if let Some(v) = opt_bool(input, "HardExpiry") {
        policy.hard_expiry = v;
    }

    Ok(json!({}))
}

pub fn delete_account_password_policy(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let mut guard = state.account_password_policy.lock().unwrap();
    if guard.is_none() {
        return Err(no_such_entity("PasswordPolicy", "default"));
    }
    *guard = None;
    Ok(json!({}))
}

// ── Account Summary ──────────────────────────────────────────────────────────

pub fn get_account_summary(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let users = state.users.len() as u64;
    let groups = state.groups.len() as u64;
    let roles = state.roles.len() as u64;
    let policies = state.policies.len() as u64;
    let instance_profiles = state.instance_profiles.len() as u64;
    let server_certificates = state.server_certificates.len() as u64;
    let mfa_devices = state.virtual_mfa_devices.len() as u64;

    // Count access keys across all users
    let access_keys: u64 = state.users.iter().map(|u| u.access_keys.len() as u64).sum();

    Ok(json!({
        "SummaryMap": {
            "Users": users,
            "UsersQuota": 5000,
            "Groups": groups,
            "GroupsQuota": 300,
            "Roles": roles,
            "RolesQuota": 1000,
            "Policies": policies,
            "PoliciesQuota": 1500,
            "AttachedPoliciesPerUserQuota": 10,
            "AttachedPoliciesPerGroupQuota": 10,
            "AttachedPoliciesPerRoleQuota": 10,
            "InstanceProfiles": instance_profiles,
            "InstanceProfilesQuota": 1000,
            "ServerCertificates": server_certificates,
            "ServerCertificatesQuota": 20,
            "MFADevices": mfa_devices,
            "MFADevicesInUse": mfa_devices,
            "AccountAccessKeysPresent": 0u64,
            "AccountMFAEnabled": 0u64,
            "AccessKeys": access_keys,
            "AccessKeysPerUserQuota": 2,
            "GroupsPerUserQuota": 10,
            "UserPolicySizeQuota": 2048,
            "GroupPolicySizeQuota": 5120,
            "RolePolicySizeQuota": 10240,
            "PolicySizeQuota": 6144,
            "PolicyVersionsInUse": policies,
            "PolicyVersionsInUseQuota": 10000,
            "VersionsPerPolicyQuota": 5,
            "GlobalEndpointTokenVersion": 1u64,
        }
    }))
}

// ── Account Authorization Details ───────────────────────────────────────────

pub fn get_account_authorization_details(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let filter = opt_str(input, "Filter").unwrap_or("All");
    let include_all = filter == "All";
    let include_users = include_all || filter == "User";
    let include_roles = include_all || filter == "Role";
    let include_groups = include_all || filter == "Group";
    let include_local = include_all || filter == "LocalManagedPolicy";
    let include_aws = include_all || filter == "AWSManagedPolicy";

    let user_detail_list: Vec<Value> = if include_users {
        state
            .users
            .iter()
            .map(|u| {
                let inline: Vec<Value> = u
                    .inline_policies
                    .iter()
                    .map(|(name, doc)| json!({"PolicyName": name, "PolicyDocument": doc}))
                    .collect();
                let attached: Vec<Value> = u
                    .attached_policies
                    .iter()
                    .map(|arn| json!({"PolicyArn": arn}))
                    .collect();
                json!({
                    "UserName": u.user_name,
                    "UserId": u.user_id,
                    "Arn": u.arn,
                    "Path": u.path,
                    "CreateDate": u.create_date,
                    "UserPolicyList": { "member": inline },
                    "AttachedManagedPolicies": { "member": attached },
                    "GroupList": { "member": u.groups.iter().map(|g| Value::String(g.clone())).collect::<Vec<_>>() },
                })
            })
            .collect()
    } else {
        vec![]
    };

    let role_detail_list: Vec<Value> = if include_roles {
        state
            .roles
            .iter()
            .map(|r| {
                let inline: Vec<Value> = r
                    .inline_policies
                    .iter()
                    .map(|(name, doc)| json!({"PolicyName": name, "PolicyDocument": doc}))
                    .collect();
                let attached: Vec<Value> = r
                    .attached_policies
                    .iter()
                    .map(|arn| json!({"PolicyArn": arn}))
                    .collect();
                json!({
                    "RoleName": r.role_name,
                    "RoleId": r.role_id,
                    "Arn": r.arn,
                    "Path": r.path,
                    "CreateDate": r.create_date,
                    "AssumeRolePolicyDocument": r.assume_role_policy_document,
                    "RolePolicyList": { "member": inline },
                    "AttachedManagedPolicies": { "member": attached },
                })
            })
            .collect()
    } else {
        vec![]
    };

    let group_detail_list: Vec<Value> = if include_groups {
        state
            .groups
            .iter()
            .map(|g| {
                let inline: Vec<Value> = g
                    .inline_policies
                    .iter()
                    .map(|(name, doc)| json!({"PolicyName": name, "PolicyDocument": doc}))
                    .collect();
                let attached: Vec<Value> = g
                    .attached_policies
                    .iter()
                    .map(|arn| json!({"PolicyArn": arn}))
                    .collect();
                json!({
                    "GroupName": g.group_name,
                    "GroupId": g.group_id,
                    "Arn": g.arn,
                    "Path": g.path,
                    "CreateDate": g.create_date,
                    "GroupPolicyList": { "member": inline },
                    "AttachedManagedPolicies": { "member": attached },
                })
            })
            .collect()
    } else {
        vec![]
    };

    let policy_list: Vec<Value> = if include_local || include_aws {
        state
            .policies
            .iter()
            .filter(|p| {
                // AWS managed policies have arn:aws:iam::aws:policy/...
                let is_aws = p.arn.contains(":aws:policy") || p.arn.contains("iam::aws:");
                if is_aws { include_aws } else { include_local }
            })
            .map(|p| {
                json!({
                    "PolicyName": p.policy_name,
                    "PolicyId": p.policy_id,
                    "Arn": p.arn,
                    "Path": p.path,
                    "AttachmentCount": p.attachment_count,
                    "CreateDate": p.create_date,
                    "UpdateDate": p.update_date,
                    "IsAttachable": true,
                })
            })
            .collect()
    } else {
        vec![]
    };

    Ok(json!({
        "UserDetailList": { "member": user_detail_list },
        "RoleDetailList": { "member": role_detail_list },
        "GroupDetailList": { "member": group_detail_list },
        "Policies": { "member": policy_list },
        "IsTruncated": false,
    }))
}
