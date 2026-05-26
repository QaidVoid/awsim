use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{
        delete_conflict, entity_already_exists, malformed_policy_document, no_such_entity,
        validation_error,
    },
    ids::{new_role_id, normalize_path, now_iso8601},
    state::{IamState, Role},
};

const MIN_MAX_SESSION_DURATION: u32 = 3600;
const MAX_MAX_SESSION_DURATION: u32 = 43_200;

fn validate_policy_document(doc: &str) -> Result<(), AwsError> {
    awsim_iam_policy::parse(doc)
        .map(|_| ())
        .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))
}

/// Trust policies (the AssumeRolePolicyDocument on a role) must
/// authorise at least one of the `sts:AssumeRole*` actions, or no
/// caller will ever be able to assume the role. Real AWS rejects
/// trust policies that don't contain any AssumeRole action with
/// MalformedPolicyDocument; mirror that so a typo isn't caught at
/// the next AssumeRole call instead of at role creation.
fn validate_trust_policy_actions(doc: &str) -> Result<(), AwsError> {
    use awsim_iam_policy::Effect;
    let parsed = awsim_iam_policy::parse(doc)
        .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))?;

    let has_assume_action = parsed.statements.iter().any(|st| {
        if !matches!(st.effect, Effect::Allow) {
            return false;
        }
        match &st.action {
            Some(list) => list
                .iter()
                .any(|a| a == "*" || a == "sts:*" || a.starts_with("sts:AssumeRole")),
            None => false,
        }
    });

    if !has_assume_action {
        return Err(malformed_policy_document(
            "Trust policy must allow at least one sts:AssumeRole* action.",
        ));
    }

    // Trust policies must also identify a Principal that's well-formed.
    // AWS rejects shapes like "Service": "ec2" (no `.amazonaws.com`) or
    // "AWS": "alice" (not an ARN or account id) with
    // MalformedPolicyDocument.
    for st in &parsed.statements {
        if let Some(principal) = &st.principal {
            validate_principal_shape(principal)?;
        }
    }
    Ok(())
}

fn validate_principal_shape(principal: &awsim_iam_policy::Principal) -> Result<(), AwsError> {
    use awsim_iam_policy::Principal;
    fn check_aws_entries(items: &[String]) -> Result<(), AwsError> {
        for entry in items {
            if entry == "*" {
                continue;
            }
            if entry.starts_with("arn:") {
                continue;
            }
            // 12-digit account id shortcut is allowed by AWS.
            if entry.len() == 12 && entry.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            return Err(malformed_policy_document(format!(
                "Principal.AWS `{entry}` must be `*`, a 12-digit account id, or an ARN."
            )));
        }
        Ok(())
    }
    fn check_service_entries(items: &[String]) -> Result<(), AwsError> {
        for entry in items {
            if !entry.ends_with(".amazonaws.com") && !entry.ends_with(".aws.com") {
                return Err(malformed_policy_document(format!(
                    "Principal.Service `{entry}` must look like a service domain (ends with .amazonaws.com)."
                )));
            }
        }
        Ok(())
    }
    fn check_federated_entries(items: &[String]) -> Result<(), AwsError> {
        for entry in items {
            // Federated principals are either SAML/OIDC provider ARNs
            // or known web-identity issuers.
            let is_arn = entry.starts_with("arn:");
            let is_known_idp = matches!(
                entry.as_str(),
                "accounts.google.com"
                    | "cognito-identity.amazonaws.com"
                    | "graph.facebook.com"
                    | "www.amazon.com"
            );
            if !is_arn && !is_known_idp {
                return Err(malformed_policy_document(format!(
                    "Principal.Federated `{entry}` must be an IdP ARN or a known web-identity issuer."
                )));
            }
        }
        Ok(())
    }
    match principal {
        Principal::Wildcard | Principal::CanonicalUser(_) => Ok(()),
        Principal::Aws(items) => check_aws_entries(items),
        Principal::Service(items) => check_service_entries(items),
        Principal::Federated(items) => check_federated_entries(items),
        Principal::Mixed {
            aws,
            service,
            federated,
            ..
        } => {
            check_aws_entries(aws)?;
            check_service_entries(service)?;
            check_federated_entries(federated)?;
            Ok(())
        }
    }
}

/// Validate an IAM role name against AWS's documented constraint:
/// 1-64 characters from the set `[A-Za-z0-9+=,.@_-]+`. Real IAM
/// rejects anything else with ValidationError.
fn validate_role_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 64 {
        return Err(validation_error(format!(
            "1 validation error detected: Value '{name}' at 'roleName' \
             failed to satisfy constraint: Member must have length less than \
             or equal to 64 and greater than or equal to 1"
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '=' | ',' | '.' | '@' | '_' | '-'))
    {
        return Err(validation_error(format!(
            "1 validation error detected: Value '{name}' at 'roleName' \
             failed to satisfy constraint: Member must satisfy regular \
             expression pattern: [\\w+=,.@-]+"
        )));
    }
    Ok(())
}

/// AWS rejects MaxSessionDuration values outside [3600, 43200] seconds
/// with ValidationError ("1 validation error detected"). Mirror that.
fn validate_max_session_duration(value: u32) -> Result<(), AwsError> {
    if !(MIN_MAX_SESSION_DURATION..=MAX_MAX_SESSION_DURATION).contains(&value) {
        return Err(validation_error(format!(
            "1 validation error detected: Value '{value}' at 'maxSessionDuration' \
             failed to satisfy constraint: Member must have value less than or equal to \
             {MAX_MAX_SESSION_DURATION} and greater than or equal to {MIN_MAX_SESSION_DURATION}"
        )));
    }
    Ok(())
}

use super::{opt_str, require_str};

fn role_to_value(r: &Role) -> Value {
    let mut v = json!({
        "RoleName": r.role_name,
        "RoleId": r.role_id,
        "Arn": r.arn,
        "Path": r.path,
        "AssumeRolePolicyDocument": r.assume_role_policy_document,
        "CreateDate": r.create_date,
        "MaxSessionDuration": r.max_session_duration,
    });
    if let Some(desc) = &r.description {
        v["Description"] = Value::String(desc.clone());
    }
    v
}

pub fn create_role(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    validate_role_name(role_name)?;
    let assume_role_policy = require_str(input, "AssumeRolePolicyDocument")?;
    let path = normalize_path(opt_str(input, "Path"));
    let description = opt_str(input, "Description").map(|s| s.to_string());

    validate_policy_document(assume_role_policy)?;
    validate_trust_policy_actions(assume_role_policy)?;

    if state.roles.contains_key(role_name) {
        return Err(entity_already_exists("Role", role_name));
    }

    let role_id = new_role_id();
    let arn = format!("arn:aws:iam::{}:role{}{}", ctx.account_id, path, role_name);

    let max_session_duration = input
        .get("MaxSessionDuration")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(3600);
    validate_max_session_duration(max_session_duration)?;

    let role = Role {
        role_name: role_name.to_string(),
        role_id,
        arn,
        path,
        assume_role_policy_document: assume_role_policy.to_string(),
        description,
        create_date: now_iso8601(),
        max_session_duration,
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
        tags: HashMap::new(),
    };

    let result = role_to_value(&role);
    state.roles.insert(role_name.to_string(), role);

    Ok(json!({ "Role": result }))
}

pub fn get_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;
    let mut v = json!({ "Role": role_to_value(&role) });
    if let Some(boundary) = state.role_permissions_boundaries.get(&role.role_name) {
        v["Role"]["PermissionsBoundary"] = json!({
            "PermissionsBoundaryType": "Policy",
            "PermissionsBoundaryArn": boundary.value().clone(),
        });
    }
    Ok(v)
}

pub fn delete_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    {
        let role = state
            .roles
            .get(role_name)
            .ok_or_else(|| no_such_entity("Role", role_name))?;

        if !role.attached_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role has attached policies"
            )));
        }
        if !role.inline_policies.is_empty() {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role has inline policies"
            )));
        }
    }

    // Ensure no instance profile references this role
    for ip in state.instance_profiles.iter() {
        if ip.roles.contains(&role_name.to_string()) {
            return Err(delete_conflict(format!(
                "Cannot delete role {role_name}: role is associated with instance profile {}",
                ip.instance_profile_name
            )));
        }
    }

    state.roles.remove(role_name);
    Ok(json!({}))
}

pub fn list_roles(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");

    let mut all_roles: Vec<crate::state::Role> = state
        .roles
        .iter()
        .filter(|r| r.path.starts_with(path_prefix))
        .map(|r| r.value().clone())
        .collect();
    all_roles.sort_by(|a, b| a.role_name.cmp(&b.role_name));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 100, 1000);
    let marker = input.get("Marker").and_then(Value::as_str);

    let page = paginate(all_roles, max, marker, |r| r.role_name.clone())?;
    let roles: Vec<Value> = page.items.iter().map(role_to_value).collect();

    let mut result = json!({
        "Roles": { "member": roles },
        "IsTruncated": page.next_token.is_some(),
    });
    if let Some(token) = page.next_token {
        result["Marker"] = json!(token);
    }
    Ok(result)
}

/// `UpdateAssumeRolePolicy` rewrites a role's trust policy. Validates
/// both the document syntax and the requirement that at least one
/// AssumeRole-shaped action is present, since a trust policy without
/// one renders the role unassumable.
pub fn update_assume_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_document = require_str(input, "PolicyDocument")?;

    validate_policy_document(policy_document)?;
    validate_trust_policy_actions(policy_document)?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.assume_role_policy_document = policy_document.to_string();
    Ok(json!({}))
}

pub fn update_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    // UpdateRole is documented as accepting only Description and
    // MaxSessionDuration. Path / Arn / RoleId are immutable; AWS returns
    // ValidationError (HTTP 400) when callers try to mutate them via
    // this operation, so reject explicitly rather than silently dropping.
    for immutable in ["Path", "Arn", "RoleId"] {
        if input.get(immutable).and_then(Value::as_str).is_some() {
            return Err(AwsError::bad_request(
                "ValidationError",
                format!("{immutable} is not modifiable via UpdateRole."),
            ));
        }
    }

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    if let Some(desc) = opt_str(input, "Description") {
        role.description = Some(desc.to_string());
    }
    if let Some(dur) = input.get("MaxSessionDuration").and_then(|v| v.as_u64()) {
        let dur = dur as u32;
        validate_max_session_duration(dur)?;
        role.max_session_duration = dur;
    }

    Ok(json!({ "Role": role_to_value(&role) }))
}

pub fn update_role_description(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let description = require_str(input, "Description")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    role.description = Some(description.to_string());

    Ok(json!({ "Role": role_to_value(&role) }))
}

pub fn put_role_permissions_boundary(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let boundary_arn = require_str(input, "PermissionsBoundary")?;

    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }
    state
        .role_permissions_boundaries
        .insert(role_name.to_string(), boundary_arn.to_string());
    Ok(json!({}))
}

pub fn delete_role_permissions_boundary(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }
    state.role_permissions_boundaries.remove(role_name);
    Ok(json!({}))
}

// ── Inline policy read/delete ────────────────────────────────────────────────

pub fn get_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let doc = role
        .inline_policies
        .get(policy_name)
        .ok_or_else(|| no_such_entity("InlinePolicy", policy_name))?
        .clone();

    Ok(json!({
        "RoleName": role_name,
        "PolicyName": policy_name,
        "PolicyDocument": doc,
    }))
}

pub fn delete_role_policy(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;
    let policy_name = require_str(input, "PolicyName")?;

    let mut role = state
        .roles
        .get_mut(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    if role.inline_policies.remove(policy_name).is_none() {
        return Err(no_such_entity("InlinePolicy", policy_name));
    }

    Ok(json!({}))
}

pub fn list_role_policies(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    let role = state
        .roles
        .get(role_name)
        .ok_or_else(|| no_such_entity("Role", role_name))?;

    let names: Vec<Value> = role
        .inline_policies
        .keys()
        .map(|k| Value::String(k.clone()))
        .collect();

    Ok(json!({
        "PolicyNames": { "member": names },
        "IsTruncated": false,
    }))
}

#[cfg(test)]
mod update_role_immutable_tests {
    use super::*;
    use crate::state::IamState;

    fn ctx() -> RequestContext {
        RequestContext::new("iam", "us-east-1")
    }

    fn create_test_role(state: &IamState, name: &str) {
        let trust_policy = serde_json::to_string(&json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Service": "ec2.amazonaws.com" },
                "Action": "sts:AssumeRole"
            }]
        }))
        .unwrap();
        create_role(
            state,
            &json!({
                "RoleName": name,
                "AssumeRolePolicyDocument": trust_policy,
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn update_role_rejects_path_mutation() {
        let state = IamState::default();
        create_test_role(&state, "r1");
        let err = update_role(&state, &json!({ "RoleName": "r1", "Path": "/new/" })).unwrap_err();
        assert_eq!(err.code, "ValidationError");
        assert!(err.message.contains("Path"));
    }

    #[test]
    fn update_role_rejects_arn_mutation() {
        let state = IamState::default();
        create_test_role(&state, "r2");
        let err = update_role(
            &state,
            &json!({ "RoleName": "r2", "Arn": "arn:aws:iam::000000000000:role/new" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn update_role_still_updates_description() {
        let state = IamState::default();
        create_test_role(&state, "r3");
        let resp = update_role(
            &state,
            &json!({ "RoleName": "r3", "Description": "updated" }),
        )
        .unwrap();
        assert_eq!(resp["Role"]["Description"], "updated");
    }
}

#[cfg(test)]
mod trust_policy_principal_tests {
    use super::*;
    use crate::state::IamState;

    fn ctx() -> RequestContext {
        RequestContext::new("iam", "us-east-1")
    }

    fn create_with_trust(state: &IamState, name: &str, policy: &str) -> Result<Value, AwsError> {
        create_role(
            state,
            &json!({
                "RoleName": name,
                "AssumeRolePolicyDocument": policy,
            }),
            &ctx(),
        )
    }

    #[test]
    fn accepts_service_principal() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Service": "ec2.amazonaws.com" },
                "Action": "sts:AssumeRole"
            }]
        }"#;
        create_with_trust(&state, "r1", policy).unwrap();
    }

    #[test]
    fn rejects_service_without_amazonaws_com_suffix() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Service": "ec2" },
                "Action": "sts:AssumeRole"
            }]
        }"#;
        let err = create_with_trust(&state, "r2", policy).unwrap_err();
        assert_eq!(err.code, "MalformedPolicyDocument");
    }

    #[test]
    fn accepts_aws_account_id_principal() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "123456789012" },
                "Action": "sts:AssumeRole"
            }]
        }"#;
        create_with_trust(&state, "r3", policy).unwrap();
    }

    #[test]
    fn rejects_aws_principal_that_is_not_arn_or_account() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "alice" },
                "Action": "sts:AssumeRole"
            }]
        }"#;
        let err = create_with_trust(&state, "r4", policy).unwrap_err();
        assert_eq!(err.code, "MalformedPolicyDocument");
    }

    #[test]
    fn accepts_federated_known_idp() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Federated": "accounts.google.com" },
                "Action": "sts:AssumeRoleWithWebIdentity"
            }]
        }"#;
        create_with_trust(&state, "r5", policy).unwrap();
    }

    #[test]
    fn rejects_federated_garbage() {
        let state = IamState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Federated": "garbage" },
                "Action": "sts:AssumeRoleWithWebIdentity"
            }]
        }"#;
        let err = create_with_trust(&state, "r6", policy).unwrap_err();
        assert_eq!(err.code, "MalformedPolicyDocument");
    }
}
