use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        delete_conflict, entity_already_exists, limit_exceeded, malformed_policy_document,
        no_such_entity,
    },
    ids::{new_policy_id, normalize_path, now_iso8601},
    state::{IamState, Policy, PolicyVersion},
};

/// AWS quota: maximum managed policies that can be attached to a single
/// user, role, or group. The default account quota is 10 (raisable to 20
/// via support); we enforce the documented default.
const MAX_ATTACHED_POLICIES_PER_ENTITY: usize = 10;

/// Match an AWS-managed policy ARN: `arn:aws:iam::aws:policy/<Name>` or
/// `arn:aws:iam::aws:policy/service-role/<Name>` (path-prefixed). The
/// literal account "aws" is the marker.
fn aws_managed_policy_name(arn: &str) -> Option<&str> {
    arn.strip_prefix("arn:aws:iam::aws:policy/")
        .and_then(|rest| rest.rsplit('/').next())
}

/// Lazily materialize an AWS-managed policy stub when first referenced.
///
/// Real AWS pre-populates the entire AWS-managed catalog
/// (AdministratorAccess, AmazonS3FullAccess, …); awsim doesn't ship the
/// catalog, but Attach* shouldn't fail just because we haven't seen the
/// ARN before — the caller is referencing a known AWS resource. We
/// synthesize a placeholder policy carrying the supplied ARN so
/// downstream Get/List/Detach all succeed.
///
/// Returns `true` if the ARN refers to an AWS-managed policy (regardless
/// of whether it was just materialized) so callers know the policy
/// existence check should pass.
fn ensure_aws_managed_policy(state: &IamState, arn: &str) -> bool {
    let Some(name) = aws_managed_policy_name(arn) else {
        return false;
    };
    if state.policies.contains_key(arn) {
        return true;
    }
    let now = now_iso8601();
    let policy = Policy {
        policy_name: name.to_string(),
        policy_id: new_policy_id(),
        arn: arn.to_string(),
        path: "/".to_string(),
        description: Some(format!("AWS managed policy {name}")),
        policy_document: "{\"Version\":\"2012-10-17\",\"Statement\":[]}".to_string(),
        create_date: now.clone(),
        update_date: now,
        attachment_count: 0,
        versions: Vec::new(),
        default_version_id: "v1".to_string(),
        tags: HashMap::new(),
    };
    state.policies.insert(arn.to_string(), policy);
    true
}

fn policy_exists_or_managed(state: &IamState, arn: &str) -> bool {
    state.policies.contains_key(arn) || ensure_aws_managed_policy(state, arn)
}

use super::{opt_str, require_str};

fn validate_policy_document(doc: &str) -> Result<(), AwsError> {
    awsim_iam_policy::parse(doc)
        .map(|_| ())
        .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))
}

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

fn build_policy_arn(partition: &str, account_id: &str, path: &str, policy_name: &str) -> String {
    format!("arn:{partition}:iam::{account_id}:policy{path}{policy_name}")
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

    if policy_document.len() > MANAGED_POLICY_SIZE_LIMIT {
        return Err(AwsError::bad_request(
            "LimitExceeded",
            format!(
                "Managed policy document is {} characters; maximum is {MANAGED_POLICY_SIZE_LIMIT}.",
                policy_document.len()
            ),
        ));
    }
    validate_policy_document(policy_document)?;

    let arn = build_policy_arn(&ctx.partition, &ctx.account_id, &path, policy_name);

    if state.policies.contains_key(&arn) {
        return Err(entity_already_exists("Policy", policy_name));
    }

    let now = now_iso8601();
    let initial_version = PolicyVersion {
        version_id: "v1".to_string(),
        document: policy_document.to_string(),
        is_default_version: true,
        create_date: now.clone(),
    };
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
        versions: vec![initial_version],
        default_version_id: "v1".to_string(),
        tags: std::collections::HashMap::new(),
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
    use awsim_core::pagination::{cap_max_results, paginate};

    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");
    // Scope: "All", "Local", "AWS" — we only have local policies.
    let _scope = opt_str(input, "Scope").unwrap_or("Local");

    let mut all_policies: Vec<Policy> = state
        .policies
        .iter()
        .filter(|p| p.path.starts_with(path_prefix))
        .map(|p| p.value().clone())
        .collect();
    // Sort by ARN — the marker key needs to be globally unique so a
    // duplicate policy_name (rare but possible across paths) doesn't
    // confuse pagination.
    all_policies.sort_by(|a, b| a.arn.cmp(&b.arn));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 100, 1000);
    let marker = input.get("Marker").and_then(Value::as_str);

    let page = paginate(all_policies, max, marker, |p| p.arn.clone())?;
    let policies: Vec<Value> = page.items.iter().map(policy_to_value).collect();

    let mut result = json!({
        "Policies": { "member": policies },
        "IsTruncated": page.next_token.is_some(),
    });
    if let Some(token) = page.next_token {
        result["Marker"] = json!(token);
    }
    Ok(result)
}

// ── Attach / detach managed policies ────────────────────────────────────────

pub fn attach_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_arn = require_str(input, "PolicyArn")?;

    if !policy_exists_or_managed(state, policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut user = state
            .users
            .get_mut(user_name)
            .ok_or_else(|| no_such_entity("User", user_name))?;

        if !user.attached_policies.contains(&policy_arn.to_string()) {
            if user.attached_policies.len() >= MAX_ATTACHED_POLICIES_PER_ENTITY {
                return Err(limit_exceeded(format!(
                    "Cannot exceed quota for PoliciesPerUser: {MAX_ATTACHED_POLICIES_PER_ENTITY}"
                )));
            }
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

    if !policy_exists_or_managed(state, policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut role = state
            .roles
            .get_mut(role_name)
            .ok_or_else(|| no_such_entity("Role", role_name))?;

        if !role.attached_policies.contains(&policy_arn.to_string()) {
            if role.attached_policies.len() >= MAX_ATTACHED_POLICIES_PER_ENTITY {
                return Err(limit_exceeded(format!(
                    "Cannot exceed quota for PoliciesPerRole: {MAX_ATTACHED_POLICIES_PER_ENTITY}"
                )));
            }
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

    if !policy_exists_or_managed(state, policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    {
        let mut group = state
            .groups
            .get_mut(group_name)
            .ok_or_else(|| no_such_entity("Group", group_name))?;

        if !group.attached_policies.contains(&policy_arn.to_string()) {
            if group.attached_policies.len() >= MAX_ATTACHED_POLICIES_PER_ENTITY {
                return Err(limit_exceeded(format!(
                    "Cannot exceed quota for PoliciesPerGroup: {MAX_ATTACHED_POLICIES_PER_ENTITY}"
                )));
            }
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

// ── Policy versions ──────────────────────────────────────────────────────────

fn version_to_value(v: &PolicyVersion) -> Value {
    json!({
        "VersionId": v.version_id,
        "Document": v.document,
        "IsDefaultVersion": v.is_default_version,
        "CreateDate": v.create_date,
    })
}

pub fn create_policy_version(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;
    let policy_document = require_str(input, "PolicyDocument")?;
    let set_as_default = input
        .get("SetAsDefault")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if policy_document.len() > MANAGED_POLICY_SIZE_LIMIT {
        return Err(AwsError::bad_request(
            "LimitExceeded",
            format!(
                "Managed policy document is {} characters; maximum is {MANAGED_POLICY_SIZE_LIMIT}.",
                policy_document.len()
            ),
        ));
    }
    validate_policy_document(policy_document)?;

    let mut policy = state
        .policies
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;

    if policy.versions.len() >= 5 {
        return Err(AwsError::bad_request(
            "LimitExceeded",
            "A managed policy can have no more than 5 versions",
        ));
    }

    // Compute next version number
    let next_num = policy
        .versions
        .iter()
        .filter_map(|v| {
            v.version_id
                .strip_prefix('v')
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0)
        + 1;
    let version_id = format!("v{next_num}");

    let now = now_iso8601();

    if set_as_default {
        for v in &mut policy.versions {
            v.is_default_version = false;
        }
        policy.default_version_id = version_id.clone();
        policy.policy_document = policy_document.to_string();
        policy.update_date = now.clone();
    }

    let new_version = PolicyVersion {
        version_id: version_id.clone(),
        document: policy_document.to_string(),
        is_default_version: set_as_default,
        create_date: now,
    };

    let result = version_to_value(&new_version);
    policy.versions.push(new_version);

    Ok(json!({ "PolicyVersion": result }))
}

pub fn delete_policy_version(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;
    let version_id = require_str(input, "VersionId")?;

    let mut policy = state
        .policies
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;

    if policy.default_version_id == version_id {
        return Err(AwsError::bad_request(
            "DeleteConflict",
            "Cannot delete the default version of a managed policy",
        ));
    }

    let before = policy.versions.len();
    policy.versions.retain(|v| v.version_id != version_id);

    if policy.versions.len() == before {
        return Err(no_such_entity("PolicyVersion", version_id));
    }

    Ok(json!({}))
}

pub fn get_policy_version(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;
    let version_id = require_str(input, "VersionId")?;

    let policy = state
        .policies
        .get(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;

    let version = policy
        .versions
        .iter()
        .find(|v| v.version_id == version_id)
        .ok_or_else(|| no_such_entity("PolicyVersion", version_id))?;

    Ok(json!({ "PolicyVersion": version_to_value(version) }))
}

pub fn list_policy_versions(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;

    let policy = state
        .policies
        .get(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;

    let versions: Vec<Value> = policy.versions.iter().map(version_to_value).collect();

    Ok(json!({
        "Versions": { "member": versions },
        "IsTruncated": false,
    }))
}

pub fn set_default_policy_version(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "PolicyArn")?;
    let version_id = require_str(input, "VersionId")?;

    let mut policy = state
        .policies
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("Policy", arn))?;

    // Verify the version exists
    if !policy.versions.iter().any(|v| v.version_id == version_id) {
        return Err(no_such_entity("PolicyVersion", version_id));
    }

    policy.default_version_id = version_id.to_string();
    for v in &mut policy.versions {
        v.is_default_version = v.version_id == version_id;
    }

    // Update the canonical policy_document to the new default
    if let Some(doc) = policy
        .versions
        .iter()
        .find(|v| v.version_id == version_id)
        .map(|v| v.document.clone())
    {
        policy.policy_document = doc;
        policy.update_date = now_iso8601();
    }

    Ok(json!({}))
}

// ── List attached policies ────────────────────────────────────────────────────

pub fn list_attached_user_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let attached: Vec<Value> = user
        .attached_policies
        .iter()
        .filter_map(|arn| {
            state.policies.get(arn).and_then(|p| {
                if p.path.starts_with(path_prefix) {
                    Some(json!({ "PolicyName": p.policy_name, "PolicyArn": p.arn }))
                } else {
                    None
                }
            })
        })
        .collect();

    Ok(json!({
        "AttachedPolicies": { "member": attached },
        "IsTruncated": false,
    }))
}

pub fn list_attached_role_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let attached: Vec<Value> = role
        .attached_policies
        .iter()
        .filter_map(|arn| {
            state.policies.get(arn).and_then(|p| {
                if p.path.starts_with(path_prefix) {
                    Some(json!({ "PolicyName": p.policy_name, "PolicyArn": p.arn }))
                } else {
                    None
                }
            })
        })
        .collect();

    Ok(json!({
        "AttachedPolicies": { "member": attached },
        "IsTruncated": false,
    }))
}

pub fn list_attached_group_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?;
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let group = state
        .groups
        .get(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    let attached: Vec<Value> = group
        .attached_policies
        .iter()
        .filter_map(|arn| {
            state.policies.get(arn).and_then(|p| {
                if p.path.starts_with(path_prefix) {
                    Some(json!({ "PolicyName": p.policy_name, "PolicyArn": p.arn }))
                } else {
                    None
                }
            })
        })
        .collect();

    Ok(json!({
        "AttachedPolicies": { "member": attached },
        "IsTruncated": false,
    }))
}

// ── ListEntitiesForPolicy ────────────────────────────────────────────────────

pub fn list_entities_for_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let policy_arn = require_str(input, "PolicyArn")?;
    let entity_filter = opt_str(input, "EntityFilter");

    if !policy_exists_or_managed(state, policy_arn) {
        return Err(no_such_entity("Policy", policy_arn));
    }

    let include_users = entity_filter
        .is_none_or(|f| f == "User" || f == "LocalManagedPolicy" || f == "AWSManagedPolicy");
    let include_roles = entity_filter
        .is_none_or(|f| f == "Role" || f == "LocalManagedPolicy" || f == "AWSManagedPolicy");
    let include_groups = entity_filter
        .is_none_or(|f| f == "Group" || f == "LocalManagedPolicy" || f == "AWSManagedPolicy");

    // With explicit User/Role/Group filters:
    let include_users = include_users || entity_filter == Some("User");
    let include_roles = include_roles || entity_filter == Some("Role");
    let include_groups = include_groups || entity_filter == Some("Group");

    let policy_users: Vec<Value> = if include_users {
        state
            .users
            .iter()
            .filter(|u| u.attached_policies.contains(&policy_arn.to_string()))
            .map(|u| json!({ "UserName": u.user_name, "UserId": u.user_id }))
            .collect()
    } else {
        vec![]
    };

    let policy_roles: Vec<Value> = if include_roles {
        state
            .roles
            .iter()
            .filter(|r| r.attached_policies.contains(&policy_arn.to_string()))
            .map(|r| json!({ "RoleName": r.role_name, "RoleId": r.role_id }))
            .collect()
    } else {
        vec![]
    };

    let policy_groups: Vec<Value> = if include_groups {
        state
            .groups
            .iter()
            .filter(|g| g.attached_policies.contains(&policy_arn.to_string()))
            .map(|g| json!({ "GroupName": g.group_name, "GroupId": g.group_id }))
            .collect()
    } else {
        vec![]
    };

    Ok(json!({
        "PolicyUsers": { "member": policy_users },
        "PolicyRoles": { "member": policy_roles },
        "PolicyGroups": { "member": policy_groups },
        "IsTruncated": false,
    }))
}

// ── Policy tags ──────────────────────────────────────────────────────────────

pub fn tag_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let policy_arn = require_str(input, "PolicyArn")?;

    let mut policy = state
        .policies
        .get_mut(policy_arn)
        .ok_or_else(|| no_such_entity("Policy", policy_arn))?;

    if let Some(tags_val) = input.get("Tags") {
        // Tags come as {"member": [...]} or as an array
        let members = tags_val
            .get("member")
            .and_then(|m| m.as_array())
            .or_else(|| tags_val.as_array());

        if let Some(tags) = members {
            for tag in tags {
                if let (Some(k), Some(v)) = (
                    tag.get("Key").and_then(|k| k.as_str()),
                    tag.get("Value").and_then(|v| v.as_str()),
                ) {
                    policy.tags.insert(k.to_string(), v.to_string());
                }
            }
        }
    }

    Ok(json!({}))
}

pub fn untag_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let policy_arn = require_str(input, "PolicyArn")?;

    let mut policy = state
        .policies
        .get_mut(policy_arn)
        .ok_or_else(|| no_such_entity("Policy", policy_arn))?;

    if let Some(keys_val) = input.get("TagKeys") {
        let members = keys_val
            .get("member")
            .and_then(|m| m.as_array())
            .or_else(|| keys_val.as_array());

        if let Some(keys) = members {
            for key in keys {
                if let Some(k) = key.as_str() {
                    policy.tags.remove(k);
                }
            }
        }
    }

    Ok(json!({}))
}

pub fn list_policy_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let policy_arn = require_str(input, "PolicyArn")?;

    let policy = state
        .policies
        .get(policy_arn)
        .ok_or_else(|| no_such_entity("Policy", policy_arn))?;

    let tags: Vec<Value> = policy
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({
        "Tags": { "member": tags },
        "IsTruncated": false,
    }))
}

// ── Inline policies ──────────────────────────────────────────────────────────

const USER_POLICY_SIZE_LIMIT: usize = 2048;
const ROLE_POLICY_SIZE_LIMIT: usize = 10240;
const GROUP_POLICY_SIZE_LIMIT: usize = 5120;
/// AWS hard limit on managed policy document length (per version).
pub(crate) const MANAGED_POLICY_SIZE_LIMIT: usize = 6144;

fn check_policy_size(doc: &str, limit: usize, entity_type: &str) -> Result<(), AwsError> {
    if doc.len() > limit {
        return Err(AwsError::bad_request(
            "LimitExceeded",
            format!(
                "Inline policy document for {entity_type} exceeds maximum size of {limit} characters"
            ),
        ));
    }
    Ok(())
}

pub fn put_user_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let policy_name = require_str(input, "PolicyName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    check_policy_size(policy_document, USER_POLICY_SIZE_LIMIT, "user")?;
    validate_policy_document(policy_document)?;

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

    check_policy_size(policy_document, ROLE_POLICY_SIZE_LIMIT, "role")?;
    validate_policy_document(policy_document)?;

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

    check_policy_size(policy_document, GROUP_POLICY_SIZE_LIMIT, "group")?;
    validate_policy_document(policy_document)?;

    let mut group = state
        .groups
        .get_mut(group_name)
        .ok_or_else(|| no_such_entity("Group", group_name))?;

    group
        .inline_policies
        .insert(policy_name.to_string(), policy_document.to_string());
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("iam", "us-east-1")
    }

    fn oversize_policy_doc(len: usize) -> String {
        let template = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::PAD"}]}"#;
        let extra = len.saturating_sub(template.len()) + "PAD".len();
        let padded = template.replace("PAD", &"x".repeat(extra));
        assert!(padded.len() >= len);
        padded
    }

    #[test]
    fn create_policy_rejects_oversize_document() {
        let state = IamState::default();
        let doc = oversize_policy_doc(MANAGED_POLICY_SIZE_LIMIT + 1);
        let err = create_policy(
            &state,
            &json!({ "PolicyName": "big", "PolicyDocument": doc }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "LimitExceeded");
        assert!(err.message.contains("maximum is 6144"));
    }

    #[test]
    fn create_policy_version_rejects_oversize_document() {
        let state = IamState::default();
        let small_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#;
        let resp = create_policy(
            &state,
            &json!({ "PolicyName": "p", "PolicyDocument": small_doc }),
            &ctx(),
        )
        .unwrap();
        let arn = resp["Policy"]["Arn"].as_str().unwrap().to_string();

        let big = oversize_policy_doc(MANAGED_POLICY_SIZE_LIMIT + 1);
        let err =
            create_policy_version(&state, &json!({ "PolicyArn": arn, "PolicyDocument": big }))
                .unwrap_err();
        assert_eq!(err.code, "LimitExceeded");
    }
}
