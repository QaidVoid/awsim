use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{delete_conflict, entity_already_exists, limit_exceeded, no_such_entity},
    ids::{new_access_key_id, new_secret_access_key, new_user_id, normalize_path, now_iso8601},
    state::{AccessKey, IamState, LoginProfile, User},
};

use super::{opt_str, require_str};

/// AWS hard limit: at most 2 access keys per IAM user.
const MAX_ACCESS_KEYS_PER_USER: usize = 2;

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
    let arn = arn::build_global(ctx, "iam", format!("user{path}{user_name}"));

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

/// Resolve the user name an operation should target when the caller
/// omits `UserName`. AWS allows certain operations (GetUser,
/// CreateAccessKey, ListAccessKeys, …) to default to the caller's own
/// IAM user when omitted. We map the SigV4-signed access key to its
/// matching user when present, falling back to any existing user.
fn resolve_target_user_name(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<String, AwsError> {
    if let Some(name) = opt_str(input, "UserName") {
        return Ok(name.to_string());
    }
    // SigV4 caller's access key, if any, used as a self-identity hint.
    if let Some(ak) = ctx.access_key.as_deref()
        && state.users.contains_key(ak)
    {
        return Ok(ak.to_string());
    }
    state
        .users
        .iter()
        .next()
        .map(|e| e.user_name.clone())
        .ok_or_else(|| no_such_entity("User", "default"))
}

pub fn get_user(state: &IamState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let name = resolve_target_user_name(state, input, ctx)?;
    let user = state
        .users
        .get(&name)
        .ok_or_else(|| no_such_entity("User", &name))?;
    let mut v = json!({ "User": user_to_value(&user) });
    if let Some(boundary) = state.user_permissions_boundaries.get(&user.user_name) {
        v["User"]["PermissionsBoundary"] = json!({
            "PermissionsBoundaryType": "Policy",
            "PermissionsBoundaryArn": boundary.value().clone(),
        });
    }
    Ok(v)
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
        if !user.inline_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete user {user_name}: user has inline policies"
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
    use awsim_core::pagination::{cap_max_results, paginate};

    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    // Sort by name so the marker key is stable across calls.
    let mut all_users: Vec<crate::state::User> = state
        .users
        .iter()
        .filter(|u| u.path.starts_with(path_prefix))
        .map(|u| u.value().clone())
        .collect();
    all_users.sort_by(|a, b| a.user_name.cmp(&b.user_name));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 100, 1000);
    let marker = input.get("Marker").and_then(Value::as_str);

    let page = paginate(all_users, max, marker, |u| u.user_name.clone())?;
    let users: Vec<Value> = page.items.iter().map(user_to_value).collect();

    let mut result = json!({
        "Users": { "member": users },
        "IsTruncated": page.next_token.is_some(),
    });
    if let Some(token) = page.next_token {
        result["Marker"] = json!(token);
    }
    Ok(result)
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
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user_name = resolve_target_user_name(state, input, ctx)?;

    let mut user = state
        .users
        .get_mut(&user_name)
        .ok_or_else(|| no_such_entity("User", &user_name))?;

    if user.access_keys.len() >= MAX_ACCESS_KEYS_PER_USER {
        return Err(limit_exceeded(format!(
            "Cannot exceed quota for AccessKeysPerUser: {MAX_ACCESS_KEYS_PER_USER}"
        )));
    }

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

pub fn list_access_keys(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user_name = resolve_target_user_name(state, input, ctx)?;

    let user = state
        .users
        .get(&user_name)
        .ok_or_else(|| no_such_entity("User", &user_name))?;

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
    let password = require_str(input, "Password")?;
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

    enforce_password_policy(state, password)?;
    let hash = hash_password(password)?;

    let profile = LoginProfile {
        user_name: user_name.to_string(),
        create_date: now_iso8601(),
        password_reset_required,
        password_hash: Some(hash),
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
    if let Some(new_password) = opt_str(input, "Password") {
        drop(profile);
        enforce_password_policy(state, new_password)?;
        let hash = hash_password(new_password)?;
        let mut profile = state
            .login_profiles
            .get_mut(user_name)
            .ok_or_else(|| no_such_entity("LoginProfile", user_name))?;
        profile.password_hash = Some(hash);
    }

    Ok(json!({}))
}

/// Verify a user-supplied plaintext password against the stored
/// bcrypt hash for `user_name`. Returns `Ok(())` on match,
/// `AccessDeniedException` on no profile / no hash / bad password.
pub fn verify_password(state: &IamState, user_name: &str, plaintext: &str) -> Result<(), AwsError> {
    let profile = state
        .login_profiles
        .get(user_name)
        .ok_or_else(|| AwsError::access_denied("Invalid credentials."))?;
    let hash = profile
        .password_hash
        .as_ref()
        .ok_or_else(|| AwsError::access_denied("Invalid credentials."))?;
    match bcrypt::verify(plaintext, hash) {
        Ok(true) => Ok(()),
        _ => Err(AwsError::access_denied("Invalid credentials.")),
    }
}

fn hash_password(plaintext: &str) -> Result<String, AwsError> {
    bcrypt::hash(plaintext, bcrypt::DEFAULT_COST)
        .map_err(|_| AwsError::internal("Failed to hash password"))
}

/// Validate `password` against the account's password policy (if set).
///
/// AWS rejects passwords that do not meet the active policy with
/// `PasswordPolicyViolation`. The default policy permits any
/// non-empty string, matching the AWS console default.
fn enforce_password_policy(state: &IamState, password: &str) -> Result<(), AwsError> {
    let guard = state.account_password_policy.lock().unwrap();
    let policy = match guard.as_ref() {
        Some(p) => p.clone(),
        None => return Ok(()),
    };
    drop(guard);

    let len = password.chars().count();
    let min = policy.minimum_password_length as usize;
    if len < min {
        return Err(AwsError::bad_request(
            "PasswordPolicyViolation",
            format!("Password must be at least {min} characters."),
        ));
    }
    if policy.require_uppercase_characters && !password.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(AwsError::bad_request(
            "PasswordPolicyViolation",
            "Password must contain at least one uppercase letter.",
        ));
    }
    if policy.require_lowercase_characters && !password.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(AwsError::bad_request(
            "PasswordPolicyViolation",
            "Password must contain at least one lowercase letter.",
        ));
    }
    if policy.require_numbers && !password.chars().any(|c| c.is_ascii_digit()) {
        return Err(AwsError::bad_request(
            "PasswordPolicyViolation",
            "Password must contain at least one digit.",
        ));
    }
    if policy.require_symbols
        && !password
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !c.is_whitespace())
    {
        return Err(AwsError::bad_request(
            "PasswordPolicyViolation",
            "Password must contain at least one symbol.",
        ));
    }
    Ok(())
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

pub fn change_password(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let old_password = require_str(input, "OldPassword")?;
    let new_password = require_str(input, "NewPassword")?;
    // ChangePassword acts on the calling user; it has no UserName parameter.
    let user_name = resolve_target_user_name(state, input, ctx)?;
    let user_name = user_name.as_str();

    verify_password(state, user_name, old_password)?;
    enforce_password_policy(state, new_password)?;
    let hash = hash_password(new_password)?;

    let mut profile = state
        .login_profiles
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("LoginProfile", user_name))?;
    profile.password_hash = Some(hash);
    profile.password_reset_required = false;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AccountPasswordPolicy;

    fn state_with_user(user: &str) -> IamState {
        let state = IamState::default();
        state.users.insert(
            user.to_string(),
            User {
                user_name: user.to_string(),
                user_id: "AIDA0000".into(),
                arn: format!("arn:aws:iam::000000000000:user/{user}"),
                path: "/".into(),
                create_date: now_iso8601(),
                tags: HashMap::new(),
                access_keys: Vec::new(),
                attached_policies: Vec::new(),
                inline_policies: HashMap::new(),
                groups: Vec::new(),
                mfa_devices: Vec::new(),
                ssh_public_keys: Vec::new(),
                password_last_used: None,
            },
        );
        state
    }

    #[test]
    fn create_login_profile_stores_hash_and_verifies() {
        let state = state_with_user("alice");
        create_login_profile(
            &state,
            &json!({ "UserName": "alice", "Password": "hunter2!ABC" }),
        )
        .unwrap();
        verify_password(&state, "alice", "hunter2!ABC").unwrap();
        assert!(verify_password(&state, "alice", "wrong").is_err());
    }

    #[test]
    fn create_login_profile_rejects_when_policy_requires_uppercase() {
        let state = state_with_user("alice");
        *state.account_password_policy.lock().unwrap() = Some(AccountPasswordPolicy {
            minimum_password_length: 1,
            require_uppercase_characters: true,
            ..Default::default()
        });
        let err = create_login_profile(
            &state,
            &json!({ "UserName": "alice", "Password": "lowercase-only" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "PasswordPolicyViolation");
    }

    #[test]
    fn create_login_profile_rejects_short_password_against_min_length() {
        let state = state_with_user("alice");
        *state.account_password_policy.lock().unwrap() = Some(AccountPasswordPolicy {
            minimum_password_length: 20,
            ..Default::default()
        });
        let err =
            create_login_profile(&state, &json!({ "UserName": "alice", "Password": "short" }))
                .unwrap_err();
        assert_eq!(err.code, "PasswordPolicyViolation");
    }

    #[test]
    fn change_password_swaps_hash_and_clears_reset_required() {
        let state = state_with_user("alice");
        create_login_profile(
            &state,
            &json!({
                "UserName": "alice",
                "Password": "first-secret",
                "PasswordResetRequired": true
            }),
        )
        .unwrap();
        change_password(
            &state,
            &json!({
                "UserName": "alice",
                "OldPassword": "first-secret",
                "NewPassword": "second-secret"
            }),
            &RequestContext::new("iam", "us-east-1"),
        )
        .unwrap();
        verify_password(&state, "alice", "second-secret").unwrap();
        assert!(verify_password(&state, "alice", "first-secret").is_err());
        let p = state.login_profiles.get("alice").unwrap();
        assert!(!p.password_reset_required);
    }

    #[test]
    fn change_password_does_not_require_username() {
        let state = state_with_user("alice");
        create_login_profile(
            &state,
            &json!({ "UserName": "alice", "Password": "first-secret" }),
        )
        .unwrap();
        // No UserName: ChangePassword resolves the calling user, matching AWS.
        change_password(
            &state,
            &json!({ "OldPassword": "first-secret", "NewPassword": "second-secret" }),
            &RequestContext::new("iam", "us-east-1"),
        )
        .unwrap();
        verify_password(&state, "alice", "second-secret").unwrap();
    }

    #[test]
    fn change_password_rejects_when_old_password_wrong() {
        let state = state_with_user("alice");
        create_login_profile(
            &state,
            &json!({ "UserName": "alice", "Password": "first-secret" }),
        )
        .unwrap();
        let err = change_password(
            &state,
            &json!({
                "UserName": "alice",
                "OldPassword": "WRONG",
                "NewPassword": "second-secret"
            }),
            &RequestContext::new("iam", "us-east-1"),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }

    #[test]
    fn verify_password_rejects_unknown_user() {
        let state = state_with_user("alice");
        assert!(verify_password(&state, "bob", "any").is_err());
    }

    #[test]
    fn verify_password_rejects_profile_without_hash() {
        let state = state_with_user("alice");
        state.login_profiles.insert(
            "alice".to_string(),
            LoginProfile {
                user_name: "alice".to_string(),
                create_date: now_iso8601(),
                password_reset_required: false,
                password_hash: None,
            },
        );
        assert!(verify_password(&state, "alice", "any").is_err());
    }
}
